//! Keeping conversations attached to their lines when the file changes.
//!
//! A comment remembers the lines it's on (`Anchor`: their text, a little context, and the line
//! numbers). When the file changes, `place` looks for those lines again. Found unchanged (even
//! if they moved), the conversation follows them. Otherwise it's outdated, like on GitHub, and
//! shown where its lines were, or at the end of the file.

use crate::review::model::{Anchor, Target};

/// How many lines of context to keep on each side.
const CONTEXT: usize = 2;

/// What a comment on `start..=end` (1-based) is attached to in `lines`.
pub fn capture(lines: &[&str], start: u32, end: u32) -> Anchor {
    let (a, b) = (start.max(1) as usize - 1, (end as usize).min(lines.len()));
    let a = a.min(b);
    let text = |r: std::ops::Range<usize>| lines[r].iter().map(|s| (*s).to_string()).collect();
    Anchor { start, end, code: text(a..b), before: text(a.saturating_sub(CONTEXT)..a), after: text(b..(b + CONTEXT).min(lines.len())) }
}

/// Where a comment with `anchor` goes in `lines`. `outdated` comments stay outdated (as on
/// GitHub); only where they're shown is worked out again.
pub fn place(lines: &[&str], anchor: &Anchor, outdated: bool) -> Target {
    match (find(lines, anchor), outdated) {
        (Some((start, end)), false) => Target::Lines { start, end },
        (Some((_, end)), true) => Target::Outdated { after: Some(end) },
        (None, _) => Target::Outdated { after: where_it_was(lines, anchor) },
    }
}

/// The lines holding exactly the anchored text (1-based), preferring matching context, then the
/// spot nearest the old line number.
fn find(lines: &[&str], l: &Anchor) -> Option<(u32, u32)> {
    let n = l.code.len();
    if n == 0 || n > lines.len() {
        return None;
    }
    let mut best: Option<((usize, u32), usize)> = None;
    for i in 0..=lines.len() - n {
        if !(0..n).all(|k| lines[i + k] == l.code[k]) {
            continue;
        }
        let ctx_before = l.before.iter().rev().enumerate().take_while(|&(k, t)| i > k && lines[i - k - 1] == t).count();
        let ctx_after = l.after.iter().enumerate().take_while(|&(k, t)| i + n + k < lines.len() && lines[i + n + k] == t).count();
        let dist = line_no(i).abs_diff(l.start);
        // More context wins, then less distance.
        let score = (ctx_before + ctx_after, u32::MAX - dist);
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| (line_no(i), line_no(i + n - 1)))
}

/// For changed lines: the line to show the conversation under. The last context line before
/// them if it's still there (nearest the old spot), else None (the end of the file).
fn where_it_was(lines: &[&str], l: &Anchor) -> Option<u32> {
    let t = l.before.last()?;
    lines.iter().enumerate().filter(|(_, s)| **s == t).map(|(i, _)| line_no(i)).min_by_key(|&n| n.abs_diff(l.start))
}

fn line_no(i: usize) -> u32 {
    u32::try_from(i + 1).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: [&str; 7] = ["# Plan", "", "Step one.", "", "Step two.", "", "Done."];

    #[test]
    fn unchanged_lines_stay_put() {
        let a = capture(&DOC, 5, 5);
        assert_eq!(a.code, ["Step two."]);
        assert_eq!(a.before, ["Step one.", ""]);
        assert_eq!(place(&DOC, &a, false), Target::Lines { start: 5, end: 5 });
    }

    #[test]
    fn moved_lines_are_followed() {
        let a = capture(&DOC, 5, 5);
        let new = ["# Plan", "", "Intro.", "", "Step one.", "", "Step two.", "", "Done."];
        assert_eq!(place(&new, &a, false), Target::Lines { start: 7, end: 7 });
    }

    #[test]
    fn edited_lines_become_outdated_under_their_old_context() {
        let a = capture(&DOC, 5, 5);
        let new = ["# Plan", "", "Step one.", "", "Step two, revised.", "", "Done."];
        // The last context line before it was the blank line 4.
        assert_eq!(place(&new, &a, false), Target::Outdated { after: Some(4) });
    }

    #[test]
    fn deleted_lines_with_no_context_go_to_the_end() {
        let a = capture(&DOC, 1, 1);
        assert_eq!(place(&["Other."], &a, false), Target::Outdated { after: None });
    }

    #[test]
    fn duplicate_text_prefers_matching_context_then_nearest() {
        let doc = ["A", "x", "B", "x"];
        let a = capture(&doc, 4, 4);
        assert_eq!(place(&doc, &a, false), Target::Lines { start: 4, end: 4 });
        let a = capture(&doc, 2, 2);
        assert_eq!(place(&doc, &a, false), Target::Lines { start: 2, end: 2 });
    }

    #[test]
    fn outdated_stays_outdated_even_when_found() {
        let a = capture(&DOC, 5, 5);
        assert_eq!(place(&DOC, &a, true), Target::Outdated { after: Some(5) });
    }
}
