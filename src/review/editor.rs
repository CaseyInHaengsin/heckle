//! A plain-text editing buffer for comments: text plus a caret (a byte offset).

#[derive(Clone, Debug, Default)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        Self { text: text.to_string(), cursor: text.len() }
    }

    pub fn insert(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }

    pub fn backspace(&mut self) {
        if let Some(p) = self.prev(self.cursor) {
            self.text.replace_range(p..self.cursor, "");
            self.cursor = p;
        }
    }

    pub fn delete(&mut self) {
        if let Some(n) = self.next(self.cursor) {
            self.text.replace_range(self.cursor..n, "");
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.prev(self.cursor).unwrap_or(0);
    }

    pub fn right(&mut self) {
        self.cursor = self.next(self.cursor).unwrap_or(self.text.len());
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_start(self.cursor);
    }

    pub fn word_right(&mut self) {
        let t = &self.text;
        let mut i = self.cursor;
        let mut chars = t[i..].char_indices().peekable();
        while let Some(&(_, c)) = chars.peek() {
            if c.is_alphanumeric() {
                break;
            }
            chars.next();
        }
        while let Some(&(_, c)) = chars.peek() {
            if !c.is_alphanumeric() {
                break;
            }
            chars.next();
        }
        i += chars.peek().map_or(t.len() - i, |&(j, _)| j);
        self.cursor = i;
    }

    /// Ctrl-W / Alt-Backspace.
    pub fn delete_word_back(&mut self) {
        let s = self.word_start(self.cursor);
        self.text.replace_range(s..self.cursor, "");
        self.cursor = s;
    }

    /// Ctrl-U: delete back to the start of the line.
    pub fn delete_to_line_start(&mut self) {
        let s = self.line_start(self.cursor);
        let s = if s == self.cursor { self.prev(s).unwrap_or(s) } else { s };
        self.text.replace_range(s..self.cursor, "");
        self.cursor = s;
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start(self.cursor);
    }

    pub fn end(&mut self) {
        self.cursor = self.line_end(self.cursor);
    }

    /// Moves to the same column on the line above (by characters, ignoring soft wraps).
    pub fn up(&mut self) {
        let start = self.line_start(self.cursor);
        if start == 0 {
            self.cursor = 0;
            return;
        }
        let col = self.text[start..self.cursor].chars().count();
        let prev_start = self.line_start(start - 1);
        self.cursor = self.at_col(prev_start, col);
    }

    pub fn down(&mut self) {
        let end = self.line_end(self.cursor);
        if end == self.text.len() {
            self.cursor = end;
            return;
        }
        let col = self.text[self.line_start(self.cursor)..self.cursor].chars().count();
        self.cursor = self.at_col(end + 1, col);
    }

    fn at_col(&self, line_start: usize, col: usize) -> usize {
        let end = self.line_end(line_start);
        self.text[line_start..end].char_indices().nth(col).map_or(end, |(i, _)| line_start + i)
    }

    fn line_start(&self, i: usize) -> usize {
        self.text[..i].rfind('\n').map_or(0, |p| p + 1)
    }

    fn line_end(&self, i: usize) -> usize {
        self.text[i..].find('\n').map_or(self.text.len(), |p| i + p)
    }

    fn word_start(&self, i: usize) -> usize {
        let t = &self.text[..i];
        let trimmed = t.trim_end_matches(|c: char| !c.is_alphanumeric());
        trimmed.rfind(|c: char| !c.is_alphanumeric()).map_or(0, |p| p + trimmed[p..].chars().next().map_or(1, char::len_utf8))
    }

    fn prev(&self, i: usize) -> Option<usize> {
        self.text[..i].char_indices().next_back().map(|(p, _)| p)
    }

    fn next(&self, i: usize) -> Option<usize> {
        self.text[i..].chars().next().map(|c| i + c.len_utf8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_deleting() {
        let mut e = Editor::new("");
        e.insert("héllo wörld");
        e.backspace();
        assert_eq!(e.text, "héllo wörl");
        e.delete_word_back();
        assert_eq!(e.text, "héllo ");
        e.left();
        e.left();
        e.insert("X");
        assert_eq!((e.text.as_str(), e.cursor), ("héllXo ", 6));
        e.insert("\r\n");
        assert_eq!(e.text, "héllX\no ");
    }

    #[test]
    fn lines_and_columns() {
        let mut e = Editor::new("abc\nde\nfghij");
        e.up();
        assert_eq!(e.cursor, 6); // end of "de"
        e.up();
        assert_eq!(e.cursor, 2); // column 2 of "abc"
        e.down();
        e.down();
        assert_eq!(e.cursor, 9); // column 2 of "fghij"
        e.home();
        assert_eq!(e.cursor, 7);
        e.delete_to_line_start();
        assert_eq!(e.text, "abc\ndefghij");
    }

    #[test]
    fn word_moves() {
        let mut e = Editor::new("let foo = bar;");
        e.word_left();
        assert_eq!(e.cursor, 10);
        e.word_left();
        assert_eq!(e.cursor, 4);
        e.word_right();
        assert_eq!(e.cursor, 7);
    }
}
