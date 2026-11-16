//! Review conversations and the comment box, drawn on the page as diffy draws them: a card with
//! a header bar, then each message under a colored dot, the author's name and what they did.
//! All sizes below are CSS px; `Layouter::s` turns them into device px.

use super::{Layouter, Thread};
use crate::render::page::{ComposerBox, Kind, NO_TEXT, ThreadBox};
use crate::render::text::{FontKey, SEMIBOLD};
use crate::render::theme::Rgb;

/// The comment being written: where it goes and what it holds so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composer {
    /// Under the block holding this source line, after its conversations; None: the page's end.
    pub after: Option<u32>,
    /// "Comment on lines 4–6", in the box's header.
    pub label: String,
    pub text: String,
    /// The caret, a byte offset into `text`.
    pub cursor: usize,
    pub placeholder: String,
    /// The primary button's label: "Comment".
    pub save: String,
    /// Drawn at the bottom of this conversation's card (a reply, or an edit) instead of on its own.
    pub in_thread: Option<u32>,
    /// What has the keys: 0 the text (it shows the caret), 1 Cancel, 2 the save button (ringed).
    pub focus: u8,
}

/// What a click on a review card's link does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardAction {
    Reply(u32),
    /// Resolve, or unresolve when resolved.
    Resolve(u32),
    /// Open a folded conversation, or fold an open resolved or outdated one.
    Show(u32),
    Hide(u32),
    Edit(u32, usize),
    Delete(u32, usize),
}

/// One message in a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadMsg {
    pub name: String,
    pub claude: bool,
    /// "commented" or "replied".
    pub verb: String,
    pub body: String,
    /// The user's own message: it offers Edit and Delete.
    pub mine: bool,
    /// Delete was pressed once: the link asks to confirm.
    pub confirm_delete: bool,
}

const GAP: f32 = 12.0;
const RADIUS: f32 = 6.0;
const CPAD: f32 = 12.0;
const HEAD: f32 = 34.0;
const SMALL: f32 = 12.0;
const BODY: f32 = 14.0;
const LINE: f32 = 21.0;
/// A message: its name line above the text, and the space below the text.
const M_HEAD: f32 = 36.0;
const M_BOTTOM: f32 = 12.0;
const E_HEAD: f32 = 30.0;
const E_BUTTONS: f32 = 46.0;
const E_MIN_LINES: usize = 3;
const BTN_H: f32 = 28.0;
const CLAUDE: Rgb = 0xd97757;
/// The Reply… box row at a conversation's bottom.
const FOOT_H: f32 = 52.0;

/// Colors the page theme doesn't name.
struct Ink {
    dark: bool,
}

impl Ink {
    const fn green(&self) -> Rgb {
        if self.dark { 0x3fb950 } else { 0x1a7f37 }
    }
    const fn button(&self) -> Rgb {
        if self.dark { 0x238636 } else { 0x1f883d }
    }
    const fn amber(&self) -> Rgb {
        if self.dark { 0xd29922 } else { 0x9a6700 }
    }
    const fn red(&self) -> Rgb {
        if self.dark { 0xf85149 } else { 0xd1242f }
    }
}

fn is_dark(bg: Rgb) -> bool {
    let (r, g, b) = ((bg >> 16) & 0xff, (bg >> 8) & 0xff, bg & 0xff);
    r * 299 + g * 587 + b * 114 < 128_000
}

impl Layouter<'_> {
    fn ink(&self) -> Ink {
        Ink { dark: is_dark(self.th.bg) }
    }

    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb, radius: f32) -> usize {
        self.push(x, y, w, h, Kind::Rect { fill: 0xff00_0000 | color, stroke: None, radius })
    }

    #[expect(clippy::too_many_arguments, reason = "a box's place, size, colors and corner, each a plain number")]
    fn outline(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Option<Rgb>, stroke: Rgb, radius: f32) -> usize {
        let s = self.s;
        let fill = fill.map_or(0, |c| 0xff00_0000 | c);
        self.push(x, y, w, h, Kind::Rect { fill, stroke: Some((s, 0xff00_0000 | stroke)), radius })
    }

    /// One line of UI text with its top at `y`, cut to end before `max_x`. Returns its width.
    #[expect(clippy::too_many_arguments, reason = "the text, its font, color and place, each a plain value")]
    fn label(&mut self, s: &str, size: f32, bits: u16, color: Rgb, x: f32, y: f32, max_x: f32) -> f32 {
        let f = FontKey::new(size * self.s, bits);
        let mut text = s.to_string();
        while !text.is_empty() && x + self.fonts.measure(f, &text) > max_x {
            text.pop();
            while text.ends_with(' ') {
                text.pop();
            }
            if !text.is_empty() {
                text.pop();
                text.push('…');
            }
        }
        let (asc, desc) = self.fonts.ascent_descent(f);
        let w = self.fonts.measure(f, &text);
        if !text.is_empty() {
            self.push(x, y, w, asc + desc, Kind::Text { s: text, f, color, under: false, strike: false, asc, tx: NO_TEXT });
        }
        w
    }

    /// `text` broken into lines no wider than `width`: each line's byte range.
    fn wrap(&mut self, text: &str, width: f32) -> Vec<(usize, usize)> {
        let f = FontKey::new(BODY * self.s, 0);
        let mut out = Vec::new();
        let mut start = 0;
        for para in text.split('\n') {
            let end = start + para.len();
            let mut line = start;
            let mut x = 0.0;
            // Words, each with the spaces after it.
            let mut i = start;
            while i < end {
                let word_end = text[i..end].find(' ').map_or(end, |k| i + k);
                let next = word_end + text[word_end..end].len() - text[word_end..end].trim_start_matches(' ').len();
                let w = self.fonts.measure(f, &text[i..word_end]);
                if x + w > width && i > line {
                    out.push((line, i));
                    line = i;
                    x = 0.0;
                }
                if w > width {
                    // A word longer than the line: break it between chars.
                    for (k, c) in text[i..word_end].char_indices() {
                        let cw = self.fonts.measure(f, c.encode_utf8(&mut [0; 4]));
                        if x + cw > width && i + k > line {
                            out.push((line, i + k));
                            line = i + k;
                            x = 0.0;
                        }
                        x += cw;
                    }
                } else {
                    x += w;
                }
                x += self.fonts.measure(f, &text[word_end..next]);
                i = next;
            }
            out.push((line, end));
            start = end + 1;
        }
        out
    }

    /// Draws wrapped body text from `y`; returns the lines' byte ranges.
    fn body(&mut self, text: &str, x: f32, y: f32, width: f32, color: Rgb) -> Vec<(usize, usize)> {
        let s = self.s;
        let lines = self.wrap(text, width);
        let f = FontKey::new(BODY * s, 0);
        let (asc, desc) = self.fonts.ascent_descent(f);
        for (k, &(a, b)) in lines.iter().enumerate() {
            let t = text[a..b].trim_end();
            if t.is_empty() {
                continue;
            }
            let w = self.fonts.measure(f, t);
            let ly = y + k as f32 * LINE * s + (LINE * s - asc - desc) / 2.0;
            self.push(x, ly, w, asc + desc, Kind::Text { s: t.to_string(), f, color, under: false, strike: false, asc, tx: NO_TEXT });
        }
        lines
    }

    /// Right-aligned links ending at `right`, the first label rightmost; records where each is.
    fn links(&mut self, links: &[(CardAction, &str, Rgb)], right: f32, y: f32) -> f32 {
        let s = self.s;
        let f = FontKey::new(SMALL * s, 0);
        let mut x = right;
        for &(a, l, color) in links {
            let lw = self.fonts.measure(f, l);
            x -= lw;
            self.label(l, SMALL, 0, color, x, y, x + lw + s);
            self.page.card_links.push((a, (x - 4.0 * s, y - 4.0 * s, lw + 8.0 * s, 24.0 * s)));
            x -= 14.0 * s;
        }
        x
    }

    /// A conversation card: a header bar with its links, the messages, then a Reply… box (or
    /// the reply being written). Folded ones show only the header.
    pub(super) fn thread(&mut self, t: &Thread, composer: Option<&Composer>, x: f32, w: f32, y: &mut f32) {
        let s = self.s;
        let ink = self.ink();
        let (th_border, th_bg, th_alt, th_muted, th_text, th_link) = (self.th.border, self.th.bg, self.th.alt_bg, self.th.fg_muted, self.th.text, self.th.link);
        *y += GAP * s;
        let top = *y;
        let folded = t.folded;
        let card = self.outline(x, top, w, 0.0, Some(th_bg), th_border, RADIUS * s);
        // The header bar, rounded on top only, inside the border.
        let head_h = HEAD * s;
        self.fill(x + s, top + s, w - 2.0 * s, head_h - s, th_alt, (RADIUS - 1.0) * s);
        if !folded {
            self.fill(x + s, top + head_h / 2.0, w - 2.0 * s, head_h / 2.0, th_alt, 0.0);
            self.fill(x, top + head_h - s, w, s, th_border, 0.0);
        }
        let ty = top + (head_h - 16.0 * s) / 2.0;
        let id = t.id;
        let head_links: Vec<(CardAction, &str, Rgb)> = match (folded, t.resolved, t.outdated) {
            (true, ..) => vec![(CardAction::Show(id), "Show", th_link)],
            (false, true, _) => vec![(CardAction::Hide(id), "Hide", th_link), (CardAction::Resolve(id), "Unresolve", th_link)],
            (false, false, true) => vec![(CardAction::Hide(id), "Hide", th_link), (CardAction::Resolve(id), "Resolve conversation", th_link)],
            (false, false, false) if t.resolvable => vec![(CardAction::Resolve(id), "Resolve conversation", th_link)],
            _ => vec![],
        };
        let end = self.links(&head_links, x + w - CPAD * s, ty) - 12.0 * s;
        let mut hx = x + CPAD * s;
        if t.outdated {
            let f = FontKey::new(11.0 * s, SEMIBOLD);
            let lw = self.fonts.measure(f, "Outdated");
            self.outline(hx, ty - 1.0 * s, lw + 14.0 * s, 19.0 * s, None, ink.amber(), 9.5 * s);
            self.label("Outdated", 11.0, SEMIBOLD, ink.amber(), hx + 7.0 * s, ty + 0.5 * s, end);
            hx += lw + 22.0 * s;
        }
        if t.resolved {
            let g = ink.green();
            self.push(hx, ty, 12.0 * s, 12.0 * s, Kind::Poly { pts: vec![(0.0, 7.0 * s), (3.5 * s, 10.5 * s), (10.0 * s, 3.0 * s)], color: 0xff00_0000 | g, width: 1.8 * s, fill: false });
            hx += 16.0 * s;
            hx += self.label("Resolved", SMALL, SEMIBOLD, th_text, hx, ty, end) + 8.0 * s;
        }
        let head = if folded { format!("{} · {}", t.head, t.msgs.first().map_or("", |m| m.body.lines().next().unwrap_or(""))) } else { t.head.clone() };
        self.label(&head, SMALL, 0, th_muted, hx, ty, end);
        *y = top + head_h;
        if !folded {
            for (j, m) in t.msgs.iter().enumerate() {
                if j > 0 {
                    self.fill(x, *y, w, s, th_border, 0.0);
                }
                let ny = *y + CPAD * s;
                let dot = if m.claude { CLAUDE } else { th_link };
                self.fill(x + CPAD * s, ny + s, 14.0 * s, 14.0 * s, dot, 7.0 * s);
                let mut name_end = end.max(x + w - CPAD * s);
                if m.mine {
                    let delete = if m.confirm_delete { "Delete? Click again" } else { "Delete" };
                    name_end = self.links(&[(CardAction::Delete(id, j), delete, ink.red()), (CardAction::Edit(id, j), "Edit", th_link)], x + w - CPAD * s, ny) - 12.0 * s;
                }
                let nx = x + (CPAD + 20.0) * s;
                let nw = self.label(&m.name, SMALL, SEMIBOLD, th_text, nx, ny, name_end);
                self.label(&m.verb, SMALL, 0, th_muted, nx + nw + 6.0 * s, ny, name_end);
                let bx = x + CPAD * s;
                let bw = w - 2.0 * CPAD * s;
                let n = if m.body.trim().is_empty() {
                    self.label("(no text)", BODY, 0, th_muted, bx, *y + M_HEAD * s, end);
                    1
                } else {
                    self.body(&m.body, bx, *y + M_HEAD * s - 3.0 * s, bw, th_text).len()
                };
                *y += (M_HEAD + n as f32 * LINE + M_BOTTOM) * s - 3.0 * s;
            }
            self.fill(x, *y, w, s, th_border, 0.0);
            match composer {
                Some(c) => {
                    self.composer(c, x + CPAD * s, w - 2.0 * CPAD * s, y);
                    *y += CPAD * s;
                }
                None => {
                    // The Reply… box, as on GitHub.
                    let (bx, by, bw, bh) = (x + CPAD * s, *y + 11.0 * s, w - 2.0 * CPAD * s, 30.0 * s);
                    self.outline(bx, by, bw, bh, Some(th_alt), th_border, RADIUS * s);
                    self.label("Reply…", BODY, 0, th_muted, bx + 10.0 * s, by + 6.0 * s, bx + bw);
                    self.page.card_links.push((CardAction::Reply(id), (bx, by, bw, bh)));
                    *y += FOOT_H * s;
                }
            }
        }
        self.page.items[card].h = *y - top;
        self.page.threads.push(ThreadBox { id: t.id, top, h: *y - top });
    }

    /// The box the user types a comment in: a focus ring, a header, the text with its caret,
    /// then the hint and the Cancel and Comment buttons.
    pub(super) fn composer(&mut self, c: &Composer, x: f32, w: f32, y: &mut f32) {
        let s = self.s;
        let ink = self.ink();
        let (th_border, th_bg, th_alt, th_muted, th_text, th_link) = (self.th.border, self.th.bg, self.th.alt_bg, self.th.fg_muted, self.th.text, self.th.link);
        *y += GAP * s;
        let top = *y;
        let tw = w - 2.0 * CPAD * s;
        let lines = self.wrap(&c.text, tw).len().max(E_MIN_LINES);
        let h = (E_HEAD + 10.0 + lines as f32 * LINE + 10.0 + E_BUTTONS) * s;
        // Focus ring: two strokes in the accent color.
        self.outline(x - s, top - s, w + 2.0 * s, h + 2.0 * s, None, th_link, (RADIUS + 1.0) * s);
        self.outline(x, top, w, h, Some(th_bg), th_link, RADIUS * s);
        let end = x + w - CPAD * s;
        self.label(&c.label, SMALL, SEMIBOLD, th_text, x + CPAD * s, top + 8.0 * s, end);
        self.fill(x, top + (E_HEAD - 1.0) * s, w, s, th_border, 0.0);
        let tx = x + CPAD * s;
        let ty = top + (E_HEAD + 10.0) * s;
        let (cx, cy) = if c.text.is_empty() {
            self.label(&c.placeholder, BODY, 0, th_muted, tx, ty + 3.0 * s, end);
            (0.0, 0)
        } else {
            let lines = self.body(&c.text, tx, ty, tw, th_text);
            let k = lines.iter().rposition(|&(a, _)| a <= c.cursor).unwrap_or(0);
            let (a, _) = lines[k];
            let f = FontKey::new(BODY * s, 0);
            (self.fonts.measure(f, &c.text[a..c.cursor.min(c.text.len()).max(a)]), k)
        };
        if c.focus == 0 {
            self.fill(tx + cx, ty + (cy as f32 * LINE + 3.0) * s, 1.5 * s, (LINE - 6.0) * s, th_link, 0.0);
        }
        // Buttons, right-aligned: Cancel, then the primary one.
        let by = top + h - f32::midpoint(E_BUTTONS, BTN_H) * s;
        let fb = FontKey::new(SMALL * s, SEMIBOLD);
        let save_w = (self.fonts.measure(fb, &c.save) + 28.0 * s).max(72.0 * s);
        let cancel_w = (self.fonts.measure(FontKey::new(SMALL * s, 0), "Cancel") + 28.0 * s).max(72.0 * s);
        let save_x = x + w - CPAD * s - save_w;
        let cancel_x = save_x - 8.0 * s - cancel_w;
        // The focused button gets a ring, as a browser draws one.
        let ring = |l: &mut Self, bx: f32, bw: f32| {
            l.outline(bx - 3.0 * s, by - 3.0 * s, bw + 6.0 * s, (BTN_H + 6.0) * s, None, th_link, (RADIUS + 3.0) * s);
            l.outline(bx - 2.0 * s, by - 2.0 * s, bw + 4.0 * s, (BTN_H + 4.0) * s, None, th_link, (RADIUS + 2.0) * s);
        };
        match c.focus {
            1 => ring(self, cancel_x, cancel_w),
            2 => ring(self, save_x, save_w),
            _ => {}
        }
        self.outline(cancel_x, by, cancel_w, BTN_H * s, Some(th_alt), th_border, RADIUS * s);
        let lw = self.fonts.measure(FontKey::new(SMALL * s, 0), "Cancel");
        self.label("Cancel", SMALL, 0, th_text, cancel_x + (cancel_w - lw) / 2.0, by + 7.0 * s, cancel_x + cancel_w);
        self.fill(save_x, by, save_w, BTN_H * s, ink.button(), RADIUS * s);
        let lw = self.fonts.measure(fb, &c.save);
        self.label(&c.save, SMALL, SEMIBOLD, 0xffffff, save_x + (save_w - lw) / 2.0, by + 7.0 * s, save_x + save_w);
        self.label("Ctrl-S saves · Tab to the buttons · Esc cancels", SMALL, 0, th_muted, tx, by + 7.0 * s, cancel_x - 8.0 * s);
        *y = top + h;
        self.page.composer = Some(ComposerBox { top, h, save: (save_x, by, save_w, BTN_H * s), cancel: (cancel_x, by, cancel_w, BTN_H * s) });
    }
}
