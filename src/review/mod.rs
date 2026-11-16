//! Review mode (`heckle review FILE`): comments on a Markdown file's source lines, saved to a
//! review file that Claude reads, with Claude's replies (`heckle reply`) shown in the open viewer.
//! Ported from diffy, which does the same for diffs.

pub mod anchor;
pub mod editor;
pub mod model;
pub mod output;
pub mod store;

use std::path::Path;

use crate::render::layout::{Thread, ThreadMsg};
use model::{Author, Comment, Msg, Progress, Review, Target, Verdict, lines_label};
use std::collections::HashSet;
use store::Store;

/// Starts a review of `file`, runs the viewer with `view`, and marks the review closed if it
/// wasn't finished. Returns the viewer's exit code.
pub fn open(file: &Path, out: Option<&Path>, view: impl FnOnce(Session) -> i32) -> Result<i32, String> {
    let abs = std::fs::canonicalize(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let name = abs.file_stem().map_or_else(|| "review".into(), |s| s.to_string_lossy().into_owned());
    let store = Store::create(out, &name)?;
    let text = read(&abs)?;
    // `--out` at an earlier review of this file opens it again, with its conversations.
    let earlier = std::fs::read_to_string(&store.json).ok().and_then(|t| output::restore(&lines(&text), &t).ok()).filter(|r| r.source == abs.display().to_string());
    let review = match earlier {
        Some(mut r) => {
            r.progress = Progress::InProgress;
            r.merge_claude(&store.load_replies());
            r
        }
        None => Review::new(abs.display().to_string()),
    };
    store.save(&lines(&text), &review)?;
    let code = view(Session::new(store.clone(), review));
    let text = read(&abs)?;
    let saved = std::fs::read_to_string(&store.json).map_err(|e| format!("{}: {e}", store.json.display()))?;
    let mut r = output::restore(&lines(&text), &saved)?;
    // review.json keeps only the user's messages; Claude's come from the replies file.
    r.merge_claude(&store.load_replies());
    if r.progress == Progress::InProgress {
        r.progress = Progress::Closed;
    }
    store.save(&lines(&text), &r)?;
    Ok(code)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// A review open in the viewer: what's been said, and where it's saved.
pub struct Session {
    pub store: Store,
    pub review: Review,
    /// Resolved or outdated conversations the user opened with Show.
    shown: HashSet<u32>,
    /// The message whose Delete was pressed once: (conversation, message).
    confirm_delete: Option<(u32, usize)>,
}

/// The overall review's conversation id in cards and links.
pub const OVERALL: u32 = 0;

impl Session {
    pub fn new(store: Store, review: Review) -> Self {
        Self { store, review, shown: HashSet::new(), confirm_delete: None }
    }

    /// Takes in Claude's replies from the replies file and saves. Returns how many are new.
    pub fn merge_replies(&mut self, src: &str) -> Result<usize, String> {
        let replies = self.store.load_replies();
        let new = self.review.merge_claude(&replies);
        if new > 0 {
            self.save(src)?;
        }
        Ok(new)
    }

    /// The user's reply in conversation `id` (`OVERALL` for the overall review).
    pub fn reply(&mut self, src: &str, id: u32, body: String) -> Result<(), String> {
        let msg = Msg::you(body);
        if id == OVERALL {
            self.review.overall_msgs.push(msg);
        } else if let Some(c) = self.review.comments.iter_mut().find(|c| c.id == id) {
            c.msgs.push(msg);
        }
        self.save(src)
    }

    /// Resolves conversation `id`, or unresolves it. Resolving folds it; unresolving opens it.
    pub fn toggle_resolved(&mut self, src: &str, id: u32) -> Result<(), String> {
        if let Some(c) = self.review.comments.iter_mut().find(|c| c.id == id) {
            c.resolved = !c.resolved;
            self.shown.remove(&id);
        }
        self.save(src)
    }

    /// Opens a folded conversation (`show`), or folds an open one.
    pub fn set_shown(&mut self, id: u32, show: bool) {
        if show {
            self.shown.insert(id);
        } else {
            self.shown.remove(&id);
        }
    }

    /// The text of message `j` in conversation `id`, if it's the user's.
    pub fn message(&self, id: u32, j: usize) -> Option<String> {
        if id == OVERALL {
            return match self.overall_index(j) {
                None => Some(self.review.overall.clone()),
                Some(k) => self.review.overall_msgs.get(k).filter(|m| m.author == Author::You).map(|m| m.body.clone()),
            };
        }
        let c = self.review.comments.iter().find(|c| c.id == id)?;
        c.msgs.get(j).filter(|m| m.author == Author::You).map(|m| m.body.clone())
    }

    /// Replaces the text of the user's message `j` in conversation `id`.
    pub fn edit(&mut self, src: &str, id: u32, j: usize, body: String) -> Result<(), String> {
        if id == OVERALL {
            match self.overall_index(j) {
                None => self.review.overall = body,
                Some(k) => {
                    if let Some(m) = self.review.overall_msgs.get_mut(k).filter(|m| m.author == Author::You) {
                        m.body = body;
                    }
                }
            }
        } else if let Some(m) = self.review.comments.iter_mut().find(|c| c.id == id).and_then(|c| c.msgs.get_mut(j)).filter(|m| m.author == Author::You) {
            m.body = body;
        }
        self.save(src)
    }

    /// Delete on the user's message `j` in conversation `id`. The first press asks to confirm
    /// and returns false; the second deletes. Deleting a conversation's first message deletes
    /// the conversation.
    pub fn delete(&mut self, src: &str, id: u32, j: usize) -> Result<bool, String> {
        if self.confirm_delete != Some((id, j)) {
            self.confirm_delete = Some((id, j));
            return Ok(false);
        }
        self.confirm_delete = None;
        if id == OVERALL {
            match self.overall_index(j) {
                None => self.review.overall.clear(),
                Some(k) => {
                    if self.review.overall_msgs.get(k).is_some_and(|m| m.author == Author::You) {
                        self.review.overall_msgs.remove(k);
                    }
                }
            }
        } else if j == 0 {
            self.review.comments.retain(|c| c.id != id);
        } else if let Some(c) = self.review.comments.iter_mut().find(|c| c.id == id)
            && c.msgs.get(j).is_some_and(|m| m.author == Author::You)
        {
            c.msgs.remove(j);
        }
        self.save(src)?;
        Ok(true)
    }

    /// Any other click forgets a pending Delete. Returns whether one was pending.
    pub fn cancel_delete(&mut self) -> bool {
        self.confirm_delete.take().is_some()
    }

    /// Message `j` of the overall card: None for the overall comment itself, else an index
    /// into `overall_msgs`.
    fn overall_index(&self, j: usize) -> Option<usize> {
        if self.review.overall.trim().is_empty() { Some(j) } else { j.checked_sub(1) }
    }

    /// Places the comments against the file's new text and saves.
    pub fn reanchor(&mut self, src: &str) -> Result<(), String> {
        self.review.reanchor(&lines(src));
        self.save(src)
    }

    pub fn save(&self, src: &str) -> Result<(), String> {
        self.store.save(&lines(src), &self.review)
    }

    /// Starts a conversation on source lines `start..=end`. Its id is new even to the replies
    /// file, so a reply to a deleted conversation never lands on it.
    pub fn add(&mut self, src: &str, start: u32, end: u32, body: String) -> Result<(), String> {
        let replied = self.store.load_replies().iter().filter_map(|r| r.to).max().unwrap_or(0);
        let id = self.review.next_comment_id().max(replied + 1);
        self.review.comments.push(Comment::new(&lines(src), id, start, end, body));
        self.save(src)
    }

    pub fn set_overall(&mut self, src: &str, body: String) -> Result<(), String> {
        self.review.overall = body;
        self.save(src)
    }

    pub fn finish(&mut self, src: &str, verdict: Verdict) -> Result<(), String> {
        self.review.verdict = Some(verdict);
        self.review.progress = Progress::Finished;
        self.save(src)
    }

    /// The conversations as cards for the page, the overall one last.
    pub fn threads(&self) -> Vec<Thread> {
        let msgs = |id: u32, v: &[Msg]| -> Vec<ThreadMsg> {
            v.iter()
                .enumerate()
                .map(|(j, m)| ThreadMsg {
                    name: m.author.name().to_string(),
                    claude: m.author == Author::Claude,
                    verb: if j == 0 { "commented" } else { "replied" }.to_string(),
                    body: m.body.clone(),
                    mine: m.author == Author::You,
                    confirm_delete: self.confirm_delete == Some((id, j)),
                })
                .collect()
        };
        let mut out: Vec<Thread> = self
            .review
            .comments
            .iter()
            .map(|c| {
                let (start, end) = match c.target {
                    Target::Lines { start, end } => (start, end),
                    Target::Outdated { .. } => (c.anchor.start, c.anchor.end),
                };
                let head = format!("Conversation on {}", lines_label(start, end));
                let folded = (c.resolved || c.outdated()) && !self.shown.contains(&c.id);
                Thread { id: c.id, after: c.shown_after(), head, outdated: c.outdated(), resolved: c.resolved, folded, resolvable: true, msgs: msgs(c.id, &c.msgs) }
            })
            .collect();
        let r = &self.review;
        if !r.overall.trim().is_empty() || r.verdict.is_some() {
            let head = match (r.progress, r.verdict) {
                (Progress::Finished, Some(v)) => format!("Overall review · {}", v.outcome()),
                _ => "Overall review".to_string(),
            };
            let mut all: Vec<Msg> = Vec::new();
            if !r.overall.trim().is_empty() {
                all.push(Msg { author: Author::You, body: r.overall.clone(), at: 0 });
            }
            all.extend(r.overall_msgs.iter().cloned());
            out.push(Thread { id: OVERALL, after: None, head, outdated: false, resolved: false, folded: false, resolvable: false, msgs: msgs(OVERALL, &all) });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "# Plan\n\nStep one.\n";

    fn session(name: &str) -> Session {
        let dir = std::env::temp_dir().join(format!("heckle-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("review.md");
        Session::new(Store { id: name.into(), json: md.with_extension("json"), md }, Review::new("/x/plan.md".into()))
    }

    fn reply_file(s: &Session, json: &str) {
        std::fs::write(s.store.replies_path(), json).unwrap();
    }

    #[test]
    fn replies_resolve_and_show_change_the_cards() {
        let mut s = session("cards");
        s.add(SRC, 3, 3, "Why?".into()).unwrap();
        reply_file(&s, r#"{"replies":[{"to":1,"body":"Because.","at_ms":1}]}"#);
        assert_eq!(s.merge_replies(SRC).unwrap(), 1);
        s.reply(SRC, 1, "Thanks".into()).unwrap();
        let t = &s.threads()[0];
        let who: Vec<(&str, &str, bool)> = t.msgs.iter().map(|m| (m.name.as_str(), m.verb.as_str(), m.mine)).collect();
        assert_eq!(who, [("You", "commented", true), ("Claude", "replied", false), ("You", "replied", true)]);
        assert!(!t.folded && t.resolvable);
        s.toggle_resolved(SRC, 1).unwrap();
        assert!(s.threads()[0].resolved && s.threads()[0].folded);
        s.set_shown(1, true);
        assert!(!s.threads()[0].folded);
        assert!(std::fs::read_to_string(&s.store.md).unwrap().contains("(resolved)"));
    }

    #[test]
    fn edit_changes_only_your_messages_and_delete_asks_first() {
        let mut s = session("edit");
        s.add(SRC, 3, 3, "Why?".into()).unwrap();
        s.reply(SRC, 1, "Also this".into()).unwrap();
        s.edit(SRC, 1, 0, "Why not?".into()).unwrap();
        assert_eq!(s.message(1, 0).as_deref(), Some("Why not?"));
        // The first Delete only asks; the second deletes.
        assert!(!s.delete(SRC, 1, 1).unwrap());
        assert!(s.threads()[0].msgs[1].confirm_delete);
        assert!(s.delete(SRC, 1, 1).unwrap());
        assert_eq!(s.review.comments[0].msgs.len(), 1);
        // Another click forgets a pending Delete.
        assert!(!s.delete(SRC, 1, 0).unwrap());
        assert!(s.cancel_delete());
        assert!(!s.delete(SRC, 1, 0).unwrap());
        assert!(s.delete(SRC, 1, 0).unwrap());
        assert!(s.review.comments.is_empty());
    }

    #[test]
    fn a_new_conversation_never_reuses_an_id_claude_replied_to() {
        let mut s = session("ids");
        reply_file(&s, r#"{"replies":[{"to":4,"body":"old","at_ms":1}]}"#);
        s.add(SRC, 3, 3, "New".into()).unwrap();
        assert_eq!(s.review.comments[0].id, 5);
    }

    #[test]
    fn the_overall_card_takes_replies_and_edits() {
        let mut s = session("overall");
        s.set_overall(SRC, "Looks fine.".into()).unwrap();
        s.reply(SRC, OVERALL, "One more thing".into()).unwrap();
        assert_eq!(s.message(OVERALL, 0).as_deref(), Some("Looks fine."));
        assert_eq!(s.message(OVERALL, 1).as_deref(), Some("One more thing"));
        s.edit(SRC, OVERALL, 1, "Two more things".into()).unwrap();
        let t = s.threads().pop().unwrap();
        assert_eq!((t.id, t.resolvable, t.msgs.len()), (OVERALL, false, 2));
        assert_eq!(t.msgs[1].body, "Two more things");
    }
}
