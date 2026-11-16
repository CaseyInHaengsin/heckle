//! Where reviews are saved, and `heckle result` / `heckle reply` for reading and answering them.
//!
//! Each review writes `review.md` and `review.json` (to `--out`, or to
//! `~/.local/state/heckle/reviews/<id>/`). `~/.local/state/heckle/index.tsv` lists every review
//! (id, path) so `heckle result` can find the newest one even when Claude lost the path. A fixed
//! home-relative folder is used instead of `$TMPDIR` because the review pane and Claude's shell
//! can see different temp folders. Claude's replies go to a separate `review.replies.json`, so
//! the viewer and `heckle reply` never write the same file.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::review::model::{ClaudeReply, Review, now_ms};
use crate::review::output;

#[derive(Clone)]
pub struct Store {
    pub id: String,
    pub md: PathBuf,
    pub json: PathBuf,
}

pub fn state_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_STATE_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d).join("heckle");
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".local/state/heckle")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn io(what: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", what.display())
}

impl Store {
    /// Picks the output paths and records them in the index.
    pub fn create(out: Option<&Path>, name: &str) -> Result<Self, String> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let tag = (u64::from(nanos) ^ (u64::from(std::process::id()) << 20) ^ now()) & 0xff_ffff;
        let id = format!("{}-{tag:06x}", sanitize(name));
        let md = match out {
            Some(p) if p.extension().is_some_and(|e| e == "json") => p.with_extension("md"),
            Some(p) if p.is_dir() => p.join("review.md"),
            Some(p) => p.to_path_buf(),
            None => state_dir().join("reviews").join(&id).join("review.md"),
        };
        let md = std::path::absolute(&md).unwrap_or(md);
        if let Some(dir) = md.parent() {
            std::fs::create_dir_all(dir).map_err(io(dir))?;
        }
        let state = state_dir();
        std::fs::create_dir_all(&state).map_err(io(&state))?;
        let index_path = state.join("index.tsv");
        let mut index = std::fs::OpenOptions::new().create(true).append(true).open(&index_path).map_err(io(&index_path))?;
        writeln!(index, "{id}\t{}", md.display()).map_err(io(&index_path))?;
        Ok(Self { id, json: md.with_extension("json"), md })
    }

    /// Opens an existing review by id (or id suffix); the newest when `want` is None.
    pub fn find(want: Option<&str>) -> Result<Self, String> {
        let all: Vec<_> = index().into_iter().filter(|(_, p)| p.exists()).collect();
        let (id, md) = match want {
            Some(w) => all.iter().rev().find(|(i, _)| i == w || i.ends_with(w)).ok_or_else(|| format!("no review with id {w}"))?,
            None => all.last().ok_or("no reviews yet")?,
        };
        Ok(Self { id: id.clone(), json: md.with_extension("json"), md: md.clone() })
    }

    /// Claude's replies, written by `heckle reply` and read by the viewer.
    pub fn replies_path(&self) -> PathBuf {
        self.md.with_extension("replies.json")
    }

    pub fn load_replies(&self) -> Vec<ClaudeReply> {
        let v: serde_json::Value = std::fs::read_to_string(self.replies_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        v["replies"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|x| ClaudeReply { to: x["to"].as_u64().and_then(|id| u32::try_from(id).ok()), body: x["body"].as_str().unwrap_or_default().to_string(), at: x["at_ms"].as_u64().unwrap_or(0) })
            .collect()
    }

    /// Writes both files atomically (temp file + rename). `lines` is the file's current text.
    pub fn save(&self, lines: &[&str], r: &Review) -> Result<(), String> {
        write_atomic(&self.md, output::markdown(lines, r).as_bytes())?;
        write_atomic(&self.json, output::json(lines, r, now()).as_bytes())
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    std::fs::write(&tmp, data).map_err(io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(io(path))
}

fn sanitize(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let t = t.trim_matches('-');
    if t.is_empty() { "review".into() } else { t.chars().take(40).collect() }
}

/// (id, review.md path), oldest first.
fn index() -> Vec<(String, PathBuf)> {
    let text = std::fs::read_to_string(state_dir().join("index.tsv")).unwrap_or_default();
    text.lines().filter_map(|l| l.split_once('\t')).map(|(id, p)| (id.to_string(), PathBuf::from(p))).collect()
}

const REPLY_HELP: &str = "heckle reply [ID] --to N TEXT     reply to comment N (the number in the review file)
heckle reply [ID] --overall TEXT  reply to the overall comment
TEXT may also come on stdin. ID defaults to the newest review.";

/// `heckle reply [ID] (--to N | --overall) [TEXT...]`: adds Claude's reply to a conversation. The
/// text comes from the arguments, or stdin when there are none. An open viewer shows it live.
pub fn reply_cmd(args: &[String]) -> Result<(), String> {
    let mut id: Option<&str> = None;
    let mut to: Option<Option<u32>> = None;
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("{REPLY_HELP}");
                return Ok(());
            }
            "--to" => {
                let n = args.get(i + 1).ok_or("--to needs a comment number")?;
                to = Some(Some(n.trim_start_matches('#').parse().map_err(|_| format!("--to {n}: not a comment number"))?));
                i += 1;
            }
            "--overall" => to = Some(None),
            "--id" => {
                id = Some(args.get(i + 1).ok_or("--id needs a review id")?);
                i += 1;
            }
            a if a.starts_with("--") => return Err(format!("unknown option {a}\n\n{REPLY_HELP}")),
            a if to.is_none() && words.is_empty() && id.is_none() => id = Some(a),
            a => words.push(a),
        }
        i += 1;
    }
    let to = to.ok_or_else(|| format!("say what to reply to\n\n{REPLY_HELP}"))?;
    let mut body = words.join(" ");
    if body.trim().is_empty() {
        use std::io::Read as _;
        std::io::stdin().read_to_string(&mut body).map_err(|e| format!("standard input: {e}"))?;
    }
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err("empty reply".into());
    }
    let s = Store::find(id)?;
    if let Some(n) = to {
        let text = std::fs::read_to_string(&s.json).map_err(io(&s.json))?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", s.json.display()))?;
        let ids: Vec<u64> = v["comments"].as_array().map_or(&[][..], Vec::as_slice).iter().filter_map(|c| c["id"].as_u64()).collect();
        if !ids.contains(&u64::from(n)) {
            return Err(format!("review {} has no comment {n} (comments: {ids:?})", s.id));
        }
    }
    let path = s.replies_path();
    let mut v: serde_json::Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| serde_json::json!({ "replies": [] }));
    let list = v["replies"].as_array_mut().ok_or("bad replies file")?;
    list.push(serde_json::json!({ "to": to, "body": body, "at_ms": now_ms() }));
    write_atomic(&path, (serde_json::to_string_pretty(&v).unwrap_or_default() + "\n").as_bytes())?;
    match to {
        Some(n) => println!("replied to comment {n} in review {}", s.id),
        None => println!("replied to the overall comment in review {}", s.id),
    }
    Ok(())
}

/// `heckle result [--list | --json | --path] [ID]`
pub fn result_cmd(args: &[String]) -> Result<(), String> {
    let mut list = false;
    let mut as_json = false;
    let mut path_only = false;
    let mut id: Option<&str> = None;
    for a in args {
        match a.as_str() {
            "--list" => list = true,
            "--json" => as_json = true,
            "--path" => path_only = true,
            "-h" | "--help" => {
                println!("heckle result [ID] [--json | --path]   print a review (the newest by default)\nheckle result --list                   list recent reviews");
                return Ok(());
            }
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => id = Some(other),
        }
    }
    if list {
        for (id, md) in index().iter().rev().filter(|(_, p)| p.exists()).take(20) {
            let (status, updated) = summary(&md.with_extension("json"));
            println!("{id}\t{status}\t{updated}\t{}", md.display());
        }
        return Ok(());
    }
    let s = Store::find(id)?;
    if path_only {
        println!("{}", s.md.display());
        return Ok(());
    }
    let file = if as_json { &s.json } else { &s.md };
    let text = std::fs::read_to_string(file).map_err(io(file))?;
    if !as_json {
        println!("<!-- heckle review {}: {} -->", s.id, s.md.display());
    }
    print!("{text}");
    Ok(())
}

fn summary(json: &Path) -> (String, String) {
    let v: serde_json::Value = std::fs::read_to_string(json).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let status = v["status"].as_str().unwrap_or("?").to_string();
    let ago = v["updated_at"].as_u64().map_or_else(
        || "?".into(),
        |t| {
            let s = now().saturating_sub(t);
            match s {
                0..60 => format!("{s}s ago"),
                60..3600 => format!("{}m ago", s / 60),
                3600..86400 => format!("{}h ago", s / 3600),
                _ => format!("{}d ago", s / 86400),
            }
        },
    );
    (status, ago)
}
