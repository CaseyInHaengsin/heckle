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
use model::{Comment, Msg, Progress, Review, Target, Verdict, lines_label};
use store::Store;

/// Starts a review of `file`, runs the viewer with `view`, and marks the review closed if it
/// wasn't finished. Returns the viewer's exit code.
pub fn open(file: &Path, out: Option<&Path>, view: impl FnOnce(Session) -> i32) -> Result<i32, String> {
    let abs = std::fs::canonicalize(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let name = abs.file_stem().map_or_else(|| "review".into(), |s| s.to_string_lossy().into_owned());
    let store = Store::create(out, &name)?;
    let text = read(&abs)?;
    let review = Review::new(abs.display().to_string());
    store.save(&lines(&text), &review)?;
    let code = view(Session { store: store.clone(), review });
    let text = read(&abs)?;
    let saved = std::fs::read_to_string(&store.json).map_err(|e| format!("{}: {e}", store.json.display()))?;
    let mut r = output::restore(&lines(&text), &saved)?;
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
}

impl Session {
    /// Places the comments against the file's new text and saves.
    pub fn reanchor(&mut self, src: &str) -> Result<(), String> {
        self.review.reanchor(&lines(src));
        self.save(src)
    }

    pub fn save(&self, src: &str) -> Result<(), String> {
        self.store.save(&lines(src), &self.review)
    }

    /// Starts a conversation on source lines `start..=end`.
    pub fn add(&mut self, src: &str, start: u32, end: u32, body: String) -> Result<(), String> {
        let id = self.review.next_comment_id();
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
        let msgs = |v: &[Msg]| -> Vec<ThreadMsg> {
            v.iter()
                .enumerate()
                .map(|(j, m)| ThreadMsg {
                    name: m.author.name().to_string(),
                    claude: m.author == model::Author::Claude,
                    verb: if j == 0 { "commented" } else { "replied" }.to_string(),
                    body: m.body.clone(),
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
                Thread { id: c.id, after: c.shown_after(), head, outdated: c.outdated(), resolved: c.resolved, msgs: msgs(&c.msgs) }
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
                all.push(Msg { author: model::Author::You, body: r.overall.clone(), at: 0 });
            }
            all.extend(r.overall_msgs.iter().cloned());
            out.push(Thread { id: 0, after: None, head, outdated: false, resolved: false, msgs: msgs(&all) });
        }
        out
    }
}
