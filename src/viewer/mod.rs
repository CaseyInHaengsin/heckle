pub mod app;
pub mod cache;
pub mod chrome;
mod clipboard;
pub mod keys;
mod link;
pub mod loading;
pub mod picker;
pub mod position;
pub mod preview;

use crate::term::Key;

/// What wakes the viewer's event loop.
pub enum Event {
    Key(Key),
    Gone(u32),
    Resize,
    FileChanged,
    /// Review mode: Claude's replies file changed.
    RepliesChanged,
    /// Review mode: `heckle wait` changed what Claude is doing.
    ClaudeChanged,
    /// The file list has new entries.
    Listed,
    Quit,
}

/// Every truecolor in `s`, for the tests of the parts drawn in cells.
#[cfg(test)]
fn sgr_colors(s: &str) -> Vec<crate::theme::Rgb> {
    let re = regex::Regex::new(r"\x1b\[[34]8;2;(\d+);(\d+);(\d+)m").unwrap();
    re.captures_iter(s).map(|c| crate::theme::pack([c[1].parse().unwrap(), c[2].parse().unwrap(), c[3].parse().unwrap()])).collect()
}
