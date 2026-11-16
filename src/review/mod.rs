//! Review mode (`mdd review FILE`): comments on a Markdown file's source lines, saved to a
//! review file that Claude reads, with Claude's replies (`mdd reply`) shown in the open viewer.
//! Ported from diffy, which does the same for diffs.

pub mod anchor;
pub mod editor;
pub mod model;
pub mod output;
pub mod store;

use std::path::Path;

use model::{Progress, Review};
use store::Store;

/// Starts a review of `file`, runs the viewer with `view`, and marks the review closed if it
/// wasn't finished. Returns the viewer's exit code.
pub fn open(file: &Path, out: Option<&Path>, view: impl FnOnce() -> i32) -> Result<i32, String> {
    let abs = std::fs::canonicalize(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let name = abs.file_stem().map_or_else(|| "review".into(), |s| s.to_string_lossy().into_owned());
    let store = Store::create(out, &name)?;
    let text = read(&abs)?;
    store.save(&lines(&text), &Review::new(abs.display().to_string()))?;
    let code = view();
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
