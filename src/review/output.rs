//! The review file Claude reads: Markdown (`review.md`) and JSON (`review.json`).

use std::fmt::Write as _;

use serde_json::json;

use crate::review::anchor;
use crate::review::model::{Anchor, Author, Comment, Msg, Progress, Review, Target, Verdict, lines_label};

pub fn markdown(lines: &[&str], r: &Review) -> String {
    let mut s = String::new();
    let title = match (r.progress, r.verdict) {
        (Progress::Finished, Some(v)) => v.outcome(),
        (Progress::Finished, None) => "finished",
        (Progress::InProgress, _) => "in progress",
        (Progress::Closed, _) => "closed without finishing",
    };
    let _ = writeln!(s, "# Review: {title}");
    let status = match r.progress {
        Progress::InProgress => "in progress (heckle is still open; more comments may come)",
        Progress::Finished => "finished",
        Progress::Closed => "closed without finishing (heckle was closed before Finish)",
    };
    let n = r.comment_count();
    let _ = writeln!(s, "Status: {status}.");
    let _ = writeln!(s, "{n} comment{}.", if n == 1 { "" } else { "s" });
    let _ = writeln!(s, "File: {}", r.source);

    if !r.overall.trim().is_empty() {
        let _ = write!(s, "\n## Overall\n{}\n", r.overall.trim_end());
    }
    write_replies(&mut s, &r.overall_msgs);

    let mut cs: Vec<&Comment> = r.comments.iter().collect();
    cs.sort_by_key(|c| (c.shown_after().unwrap_or(u32::MAX), c.id));
    if !cs.is_empty() {
        let _ = write!(s, "\n## Comments\n");
    }
    for c in cs {
        write_comment(&mut s, lines, c);
    }
    s
}

/// What a comment is on now: its saved anchor if outdated, else taken fresh from the file.
fn anchor_of(lines: &[&str], c: &Comment) -> Anchor {
    match c.target {
        Target::Lines { start, end } => anchor::capture(lines, start, end),
        Target::Outdated { .. } => c.anchor.clone(),
    }
}

/// One conversation: a heading with its number, the lines it's on, the comment, the replies.
fn write_comment(s: &mut String, lines: &[&str], c: &Comment) {
    let a = anchor_of(lines, c);
    let mut flags = String::new();
    if c.outdated() {
        flags.push_str(" (outdated)");
    }
    if c.resolved {
        flags.push_str(" (resolved)");
    }
    let _ = write!(s, "\n**Comment {} on {}{flags}:**\n{}\n{}\n", c.id, lines_label(a.start, a.end), fenced(&a.code), quote(c.body()));
    write_replies(s, &c.msgs[1..]);
}

fn write_replies(s: &mut String, msgs: &[Msg]) {
    for m in msgs {
        let _ = write!(s, "\n{} replied:\n{}\n", m.author.name(), quote(&m.body));
    }
}

fn thread_json(msgs: &[Msg]) -> Vec<serde_json::Value> {
    msgs.iter().map(|m| json!({ "author": m.author.key(), "body": m.body, "at_ms": m.at })).collect()
}

/// The user's messages from a saved thread (Claude's come from the replies file).
fn your_msgs(v: &serde_json::Value) -> Vec<Msg> {
    v.as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|x| x["author"].as_str() == Some("you"))
        .map(|x| Msg { author: Author::You, body: x["body"].as_str().unwrap_or_default().to_string(), at: x["at_ms"].as_u64().unwrap_or(0) })
        .collect()
}

pub fn json(lines: &[&str], r: &Review, updated_at: u64) -> String {
    let comments: Vec<_> = r
        .comments
        .iter()
        .map(|c| {
            let a = anchor_of(lines, c);
            json!({
                "id": c.id,
                "start": a.start,
                "end": a.end,
                "label": lines_label(a.start, a.end),
                "outdated": c.outdated(),
                "resolved": c.resolved,
                "body": c.body(),
                "at_ms": c.msgs[0].at,
                "replies": thread_json(&c.msgs[1..]),
                "code": a.code,
                "before": a.before,
                "after": a.after,
            })
        })
        .collect();
    let v = json!({
        "version": 1,
        "status": r.progress.key(),
        "verdict": r.verdict.map(Verdict::key),
        "source": r.source,
        "overall": r.overall,
        "overall_replies": thread_json(&r.overall_msgs),
        "comments": comments,
        "updated_at": updated_at,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

/// Rebuilds a review from its `review.json` against the file's current `lines`, placing each
/// conversation again from its anchor (see `anchor::place`).
pub fn restore(lines: &[&str], text: &str) -> Result<Review, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("bad review.json: {e}"))?;
    let mut r = Review::new(v["source"].as_str().unwrap_or_default().to_string());
    r.overall = v["overall"].as_str().unwrap_or_default().to_string();
    r.overall_msgs = your_msgs(&v["overall_replies"]);
    r.verdict = Verdict::ALL.into_iter().find(|x| v["verdict"].as_str() == Some(x.key()));
    r.progress = match v["status"].as_str() {
        Some("finished") => Progress::Finished,
        Some("closed") => Progress::Closed,
        _ => Progress::InProgress,
    };
    let strings = |e: &serde_json::Value| e.as_array().map_or(Vec::new(), |a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect());
    let num = |e: &serde_json::Value| e.as_u64().and_then(|x| u32::try_from(x).ok()).unwrap_or(0);
    for c in v["comments"].as_array().map_or(&[][..], Vec::as_slice) {
        let anchor = Anchor { start: num(&c["start"]), end: num(&c["end"]), code: strings(&c["code"]), before: strings(&c["before"]), after: strings(&c["after"]) };
        let target = anchor::place(lines, &anchor, c["outdated"].as_bool().unwrap_or(false));
        let mut msgs = vec![Msg { author: Author::You, body: c["body"].as_str().unwrap_or_default().to_string(), at: c["at_ms"].as_u64().unwrap_or(0) }];
        msgs.extend(your_msgs(&c["replies"]));
        r.comments.push(Comment { id: num(&c["id"]), target, anchor, msgs, resolved: c["resolved"].as_bool().unwrap_or(false) });
    }
    Ok(r)
}

/// The commented lines as a fenced `md` block, with a fence longer than any backtick run inside.
fn fenced(code: &[String]) -> String {
    let body = code.join("\n");
    let longest = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}md\n{body}\n{fence}")
}

fn quote(body: &str) -> String {
    body.trim_end().lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: [&str; 7] = ["# Plan", "", "Step one.", "", "```sh", "ls", "```"];

    fn review() -> Review {
        let mut r = Review::new("/tmp/plan.md".into());
        r.overall = "Looks good.".into();
        r.verdict = Some(Verdict::RequestChanges);
        r.progress = Progress::Finished;
        r.comments.push(Comment::new(&DOC, 2, 5, 7, "Why `ls`?".into()));
        r.comments.push(Comment::new(&DOC, 1, 1, 3, "Rename this.".into()));
        r.comments[1].resolved = true;
        r
    }

    #[test]
    fn markdown_lists_comments_in_file_order_with_their_lines() {
        let mut r = review();
        r.comments[1].msgs.push(Msg { author: Author::Claude, body: "Renamed.".into(), at: 9 });
        let want = "# Review: changes requested
Status: finished.
3 comments.
File: /tmp/plan.md

## Overall
Looks good.

## Comments

**Comment 1 on lines 1–3 (resolved):**
```md
# Plan

Step one.
```
> Rename this.

Claude replied:
> Renamed.

**Comment 2 on lines 5–7:**
````md
```sh
ls
```
````
> Why `ls`?
";
        assert_eq!(markdown(&DOC, &r), want);
    }

    #[test]
    fn json_round_trips_through_restore() {
        let saved = json(&DOC, &review(), 7);
        let back = restore(&DOC, &saved).unwrap();
        assert_eq!(json(&DOC, &back, 7), saved);
    }

    #[test]
    fn an_edited_file_marks_changed_lines_outdated() {
        let saved = json(&DOC, &review(), 0);
        let new = ["# Plan", "", "Step one.", "", "```sh", "ls -la", "```"];
        let back = restore(&new, &saved).unwrap();
        assert_eq!(back.comments[0].target, Target::Outdated { after: Some(4) });
        assert_eq!(back.comments[1].target, Target::Lines { start: 1, end: 3 });
        let md = markdown(&new, &back);
        assert!(md.contains("**Comment 2 on lines 5–7 (outdated):**\n````md\n```sh\nls\n```\n````"), "{md}");
    }
}
