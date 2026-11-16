//! `heckle wait`: blocks until the user has said something Claude hasn't answered, so Claude can
//! run it in the background and be woken to reply. It waits for a quiet moment (and for any open
//! comment box to close) so several comments come back as one batch.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::review::model::{Progress, Review};
use crate::review::output;
use crate::review::store::{Store, now, write_atomic};

const HELP: &str = "heckle wait [ID | review.md] [--quiet SECS] [--timeout MINUTES]
Waits until the review has comments or replies Claude hasn't answered and the user has paused
for SECS (default 3), then prints what needs a reply and exits. It also exits when the review
is finished or closed, or after MINUTES with nothing new (default 60). Run it in the background.";

const POLL: Duration = Duration::from_millis(300);
/// A comment box open longer than this is taken as left open, and no longer holds Claude back.
const TYPING_STALE: Duration = Duration::from_secs(600);

pub fn wait_cmd(args: &[String]) -> Result<(), String> {
    let mut id: Option<&str> = None;
    let mut quiet = Duration::from_secs(3);
    let mut timeout = Duration::from_secs(3600);
    let mut i = 0;
    while i < args.len() {
        let num = |k: usize| args.get(k).and_then(|v| v.parse::<f64>().ok()).filter(|v| *v >= 0.0);
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            "--quiet" => {
                quiet = Duration::from_secs_f64(num(i + 1).ok_or("--quiet needs a number of seconds")?);
                i += 1;
            }
            "--timeout" => {
                timeout = Duration::from_secs_f64(num(i + 1).ok_or("--timeout needs a number of minutes")? * 60.0);
                i += 1;
            }
            a if a.starts_with('-') => return Err(format!("unknown option {a}\n\n{HELP}")),
            a => id = Some(a),
        }
        i += 1;
    }
    let s = Store::find(id)?;
    let start = Instant::now();
    write_atomic(&s.claude_path(), format!("listening {}\n", std::process::id()).as_bytes())?;
    let mut seen = String::new();
    // The user's last save: as if long ago, so what's already waiting goes at once.
    let mut changed = start.checked_sub(quiet).unwrap_or(start);
    loop {
        let text = std::fs::read_to_string(&s.json).unwrap_or_default();
        if text != seen {
            if !seen.is_empty() {
                changed = Instant::now();
            }
            seen = text;
        }
        let r = load(&s, &seen)?;
        let pending = r.pending();
        match r.progress {
            Progress::Finished => {
                let _ = std::fs::remove_file(s.claude_path());
                let verdict = r.verdict.map_or("finished", |v| v.outcome());
                println!("Review {} is finished ({verdict}){}. Read it with `heckle result {}`. No need to wait again.", s.id, needs(&pending), s.id);
                return Ok(());
            }
            Progress::Closed => {
                let _ = std::fs::remove_file(s.claude_path());
                println!("Review {} was closed{}. Read it with `heckle result {}`. No need to wait again.", s.id, needs(&pending), s.id);
                return Ok(());
            }
            Progress::InProgress => {}
        }
        let typing = std::fs::metadata(s.typing_path()).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() < TYPING_STALE);
        if !pending.is_empty() && !typing && changed.elapsed() >= quiet {
            write_atomic(&s.claude_path(), format!("replying {}\n", now()).as_bytes())?;
            println!(
                "Review {} has comments waiting for you{}. Read it with `heckle result {}`, answer each one with `heckle reply {} --to N` (or --overall), edit the file as needed, then run `heckle wait {}` in the background again.",
                s.id,
                needs(&pending),
                s.id,
                s.id,
                s.id
            );
            return Ok(());
        }
        if start.elapsed() >= timeout {
            let _ = std::fs::remove_file(s.claude_path());
            println!("No new comments in review {} for {} minutes. Run `heckle wait {}` in the background again if the user is still reviewing.", s.id, timeout.as_secs() / 60, s.id);
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
}

/// The review as saved, with Claude's replies merged in.
fn load(s: &Store, json: &str) -> Result<Review, String> {
    if json.is_empty() {
        return Err(format!("{}: no review there", s.json.display()));
    }
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("{}: {e}", s.json.display()))?;
    let src = v["source"].as_str().and_then(|p| std::fs::read_to_string(Path::new(p)).ok()).unwrap_or_default();
    let mut r = output::restore(&super::lines(&src), json)?;
    r.merge_claude(&s.load_replies());
    Ok(r)
}

/// ": #1, #3 and the overall comment", or "" when nothing is waiting.
fn needs(pending: &[u32]) -> String {
    let mut parts: Vec<String> = pending.iter().filter(|&&id| id != 0).map(|id| format!("#{id}")).collect();
    if pending.contains(&0) {
        parts.push("the overall comment".into());
    }
    if parts.is_empty() { String::new() } else { format!(": {}", parts.join(", ")) }
}
