//! A review of one Markdown file: comments on source line ranges, each a conversation between
//! the user and Claude, plus an overall comment and a verdict.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Comment,
    Approve,
    RequestChanges,
}

impl Verdict {
    pub const ALL: [Self; 3] = [Self::Comment, Self::Approve, Self::RequestChanges];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Comment => "Comment",
            Self::Approve => "Approve",
            Self::RequestChanges => "Request changes",
        }
    }
    /// The word used in the review title: "Review: approved".
    pub const fn outcome(self) -> &'static str {
        match self {
            Self::Comment => "comments",
            Self::Approve => "approved",
            Self::RequestChanges => "changes requested",
        }
    }
    pub const fn key(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::Approve => "approve",
            Self::RequestChanges => "request_changes",
        }
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    InProgress,
    Finished,
    /// The viewer was closed before Finish was pressed.
    Closed,
}

impl Progress {
    pub const fn key(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Finished => "finished",
            Self::Closed => "closed",
        }
    }
}

/// Where a comment is shown in the current text of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Source lines `start..=end`, 1-based.
    Lines { start: u32, end: u32 },
    /// The lines it was on changed. Shown under this line, or at the end of the file (None).
    Outdated { after: Option<u32> },
}

/// What a comment was attached to, kept so it can be found again after the file changes (and
/// quoted as it was once outdated).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// The lines when the comment was made, 1-based.
    pub start: u32,
    pub end: u32,
    /// The commented lines, and up to two lines of context on each side.
    pub code: Vec<String>,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Author {
    You,
    Claude,
}

impl Author {
    pub const fn name(self) -> &'static str {
        match self {
            Self::You => "You",
            Self::Claude => "Claude",
        }
    }
    pub const fn key(self) -> &'static str {
        match self {
            Self::You => "you",
            Self::Claude => "claude",
        }
    }
}

/// One message in a conversation. `at` is milliseconds since the epoch, used to interleave the
/// user's messages (saved by the viewer) with Claude's (written by `heckle reply`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Msg {
    pub author: Author,
    pub body: String,
    pub at: u64,
}

impl Msg {
    pub fn you(body: String) -> Self {
        Self { author: Author::You, body, at: now_ms() }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A conversation on some lines: the user's comment, then replies.
#[derive(Clone, Debug)]
pub struct Comment {
    /// Stable number shown in the review file, so Claude can reply to it (`heckle reply --to N`).
    pub id: u32,
    pub target: Target,
    pub anchor: Anchor,
    /// `msgs[0]` is the comment that started it (always the user's).
    pub msgs: Vec<Msg>,
    pub resolved: bool,
}

impl Comment {
    pub fn new(lines: &[&str], id: u32, start: u32, end: u32, body: String) -> Self {
        let anchor = crate::review::anchor::capture(lines, start, end);
        Self { id, target: Target::Lines { start, end }, anchor, msgs: vec![Msg::you(body)], resolved: false }
    }
    pub const fn outdated(&self) -> bool {
        matches!(self.target, Target::Outdated { .. })
    }
    pub fn body(&self) -> &str {
        &self.msgs[0].body
    }
    /// The source line it's shown under (None: the end of the file).
    pub const fn shown_after(&self) -> Option<u32> {
        match self.target {
            Target::Lines { end, .. } => Some(end),
            Target::Outdated { after } => after,
        }
    }
}

/// "line 4" or "lines 4–6".
pub fn lines_label(start: u32, end: u32) -> String {
    if start == end { format!("line {start}") } else { format!("lines {start}–{end}") }
}

pub struct Review {
    /// The absolute path of the Markdown file under review.
    pub source: String,
    pub comments: Vec<Comment>,
    pub overall: String,
    pub verdict: Option<Verdict>,
    pub progress: Progress,
    /// Replies to the overall comment, from either side.
    pub overall_msgs: Vec<Msg>,
}

/// A reply from Claude, read from `review.replies.json`: `to` is a comment id, or None for the
/// overall comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeReply {
    pub to: Option<u32>,
    pub body: String,
    pub at: u64,
}

impl Review {
    pub const fn new(source: String) -> Self {
        Self { source, comments: Vec::new(), overall: String::new(), verdict: None, progress: Progress::InProgress, overall_msgs: Vec::new() }
    }

    /// Puts Claude's replies into their conversations, in time order (replacing any Claude
    /// messages already there). Returns how many are new.
    pub fn merge_claude(&mut self, replies: &[ClaudeReply]) -> usize {
        let before = self.claude_count();
        let strip = |v: &mut Vec<Msg>| v.retain(|m| m.author != Author::Claude);
        // After every message from `from` on that isn't newer.
        let insert = |v: &mut Vec<Msg>, from: usize, m: Msg| {
            let from = from.min(v.len());
            let pos = from + v[from..].iter().take_while(|x| x.at <= m.at).count();
            v.insert(pos, m);
        };
        for c in &mut self.comments {
            strip(&mut c.msgs);
        }
        strip(&mut self.overall_msgs);
        for r in replies {
            let m = Msg { author: Author::Claude, body: r.body.clone(), at: r.at };
            match r.to {
                Some(id) => {
                    if let Some(c) = self.comments.iter_mut().find(|c| c.id == id) {
                        insert(&mut c.msgs, 1, m);
                    }
                }
                None => insert(&mut self.overall_msgs, 0, m),
            }
        }
        self.claude_count().saturating_sub(before)
    }

    fn claude_count(&self) -> usize {
        self.comments.iter().flat_map(|c| &c.msgs).chain(&self.overall_msgs).filter(|m| m.author == Author::Claude).count()
    }

    /// The conversations whose newest message is the user's, so Claude owes a reply: comment
    /// ids, with 0 for the overall review. Resolved ones are settled.
    pub fn pending(&self) -> Vec<u32> {
        let mut out: Vec<u32> = self.comments.iter().filter(|c| !c.resolved && c.msgs.last().is_some_and(|m| m.author == Author::You)).map(|c| c.id).collect();
        let overall = match self.overall_msgs.last() {
            Some(m) => m.author == Author::You,
            None => !self.overall.trim().is_empty(),
        };
        if overall {
            out.push(0);
        }
        out
    }

    pub fn next_comment_id(&self) -> u32 {
        self.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }

    pub fn comment_count(&self) -> usize {
        self.comments.len() + usize::from(!self.overall.trim().is_empty())
    }

    /// Places every comment again against the file's new text (see `anchor::place`).
    pub fn reanchor(&mut self, lines: &[&str]) {
        for c in &mut self.comments {
            c.target = crate::review::anchor::place(lines, &c.anchor, c.outdated());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_replies_interleave_by_time_and_replace_old_ones() {
        let lines = ["# Title", "", "Body"];
        let mut r = Review::new("/x.md".into());
        let mut c = Comment::new(&lines, 1, 3, 3, "why?".into());
        c.msgs[0].at = 10;
        c.msgs.push(Msg { author: Author::You, body: "and?".into(), at: 30 });
        r.comments.push(c);
        let replies = [ClaudeReply { to: Some(1), body: "because".into(), at: 20 }, ClaudeReply { to: None, body: "thanks".into(), at: 5 }];
        assert_eq!(r.merge_claude(&replies), 2);
        let order: Vec<_> = r.comments[0].msgs.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(order, ["why?", "because", "and?"]);
        assert_eq!(r.overall_msgs[0].body, "thanks");
        // Merging the same file again adds nothing new.
        assert_eq!(r.merge_claude(&replies), 0);
        assert_eq!(r.comments[0].msgs.len(), 3);
    }
}
