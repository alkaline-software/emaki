//! A text as the editor holds it: lines, a place in them, a selection,
//! and the edits made to it, each one step of undo. From Onyx's
//! `lines.rs`.
//!
//! The text is lines, not one string, since everything the editor does
//! is by the line: lay one out, draw one, find the one under the
//! pointer. A place is a line and a byte offset in it (`Pos`); the
//! offset is always on a character's boundary, and the caret steps by
//! grapheme, so a flag or a composed character is one step.

use std::ops::Range;
use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation as _;

/// A place in the text: a line, and a byte offset in that line.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// What is selected: from the anchor, where it began, to the head,
/// where the caret is. Equal, it is a caret.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sel {
    pub anchor: Pos,
    pub head: Pos,
}

impl Sel {
    pub const fn caret(at: Pos) -> Self {
        Self { anchor: at, head: at }
    }

    pub fn is_caret(&self) -> bool {
        self.anchor == self.head
    }

    /// The selection in order: its start and its end.
    pub fn range(&self) -> (Pos, Pos) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    pub fn start(&self) -> Pos {
        self.range().0
    }

    pub fn end(&self) -> Pos {
        self.range().1
    }
}

/// One piece of a step: what was taken out at `from`, and what was put in.
#[derive(Clone, Debug)]
pub struct Part {
    pub from: Pos,
    pub removed: String,
    pub inserted: String,
}

/// One step of undo: one piece for typing, several for a change made in
/// many places at once (a list renumbered), applied from the last place
/// to the first so each place stays where it was.
#[derive(Clone, Debug)]
pub struct Edit {
    pub parts: Vec<Part>,
    pub sel_before: Sel,
    pub sel_after: Sel,
    at: Instant,
}

/// Typing within this long of the last keystroke joins the same step.
const COALESCE: Duration = Duration::from_millis(900);

#[derive(Clone, Debug)]
pub struct Doc {
    lines: Vec<String>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Counts up at every change, so a cache keyed on it knows the text moved.
    pub revision: u64,
}

impl Default for Doc {
    fn default() -> Self {
        Self::from_text("")
    }
}

impl Doc {
    /// The text as lines. Windows' line ends are read as plain ones; the
    /// text is given back with plain ones.
    pub fn from_text(text: &str) -> Self {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        Self { lines: text.split('\n').map(str::to_string).collect(), undo: Vec::new(), redo: Vec::new(), revision: 0 }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    #[allow(dead_code)] // the text as lines, for a walk over all of them
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map(String::as_str).unwrap_or("")
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn end(&self) -> Pos {
        let last = self.lines.len() - 1;
        Pos::new(last, self.lines[last].len())
    }

    /// The text from `a` to `b`, in order.
    pub fn slice(&self, a: Pos, b: Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a.line == b.line {
            return self.line(a.line)[a.col..b.col].to_string();
        }
        let mut out = self.line(a.line)[a.col..].to_string();
        for i in a.line + 1..b.line {
            out.push('\n');
            out.push_str(self.line(i));
        }
        out.push('\n');
        out.push_str(&self.line(b.line)[..b.col]);
        out
    }

    /// A place held to the text and to a character's boundary.
    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        let text = &self.lines[line];
        let mut col = p.col.min(text.len());
        while col > 0 && !text.is_char_boundary(col) {
            col -= 1;
        }
        Pos::new(line, col)
    }

    // -- places ---------------------------------------------------------------

    /// The byte offset of a place in the whole text, lines joined by one byte.
    pub fn offset_of(&self, p: Pos) -> usize {
        self.lines[..p.line].iter().map(|l| l.len() + 1).sum::<usize>() + p.col
    }

    /// The place at a byte offset in the whole text.
    pub fn pos_of(&self, offset: usize) -> Pos {
        let mut left = offset;
        for (i, l) in self.lines.iter().enumerate() {
            if left <= l.len() {
                return self.clamp(Pos::new(i, left));
            }
            left -= l.len() + 1;
        }
        self.end()
    }

    /// The offset of a place in UTF-16 units, as an input method counts.
    pub fn utf16_of(&self, p: Pos) -> usize {
        self.lines[..p.line].iter().map(|l| l.encode_utf16().count() + 1).sum::<usize>() + self.lines[p.line][..p.col].encode_utf16().count()
    }

    /// The place at an offset in UTF-16 units.
    pub fn pos_of_utf16(&self, offset: usize) -> Pos {
        let mut left = offset;
        for (i, l) in self.lines.iter().enumerate() {
            let units = l.encode_utf16().count();
            if left <= units {
                let mut col = 0;
                let mut seen = 0;
                for (at, ch) in l.char_indices() {
                    if seen >= left {
                        col = at;
                        break;
                    }
                    seen += ch.len_utf16();
                    col = at + ch.len_utf8();
                }
                return Pos::new(i, col);
            }
            left -= units + 1;
        }
        self.end()
    }

    pub fn utf16_len(&self) -> usize {
        self.utf16_of(self.end())
    }

    /// One grapheme on from `p`, over a line's end to the next line.
    pub fn next_grapheme(&self, p: Pos) -> Pos {
        let text = self.line(p.line);
        if p.col >= text.len() {
            return if p.line + 1 < self.lines.len() { Pos::new(p.line + 1, 0) } else { p };
        }
        let step = text[p.col..].graphemes(true).next().map_or(1, str::len);
        Pos::new(p.line, p.col + step)
    }

    /// One grapheme back from `p`, over a line's start to the line before.
    pub fn prev_grapheme(&self, p: Pos) -> Pos {
        if p.col == 0 {
            return if p.line > 0 { Pos::new(p.line - 1, self.line(p.line - 1).len()) } else { p };
        }
        let text = self.line(p.line);
        let step = text[..p.col].graphemes(true).next_back().map_or(1, str::len);
        Pos::new(p.line, p.col - step)
    }

    /// The start of the word before `p`, as ⌥← finds it: back over
    /// spaces, then over the word.
    pub fn word_left(&self, p: Pos) -> Pos {
        if p.col == 0 {
            return self.prev_grapheme(p);
        }
        let text = self.line(p.line);
        let mut col = p.col;
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let before = |col: usize| text[..col].chars().next_back();
        while let Some(c) = before(col).filter(|c| !is_word(*c)) {
            col -= c.len_utf8();
            if is_word(c) {
                break;
            }
            if col == 0 || before(col).is_some_and(is_word) {
                break;
            }
        }
        while let Some(c) = before(col).filter(|c| is_word(*c)) {
            col -= c.len_utf8();
        }
        Pos::new(p.line, col)
    }

    /// The end of the word after `p`, as ⌥→ finds it.
    pub fn word_right(&self, p: Pos) -> Pos {
        let text = self.line(p.line);
        if p.col >= text.len() {
            return self.next_grapheme(p);
        }
        let mut col = p.col;
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let after = |col: usize| text[col..].chars().next();
        while let Some(c) = after(col).filter(|c| !is_word(*c)) {
            col += c.len_utf8();
        }
        while let Some(c) = after(col).filter(|c| is_word(*c)) {
            col += c.len_utf8();
        }
        Pos::new(p.line, col)
    }

    /// The word under `p`, for a double click and for ⌘B on a caret.
    pub fn word_at(&self, p: Pos) -> Range<usize> {
        let text = self.line(p.line);
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = p.col.min(text.len());
        while let Some(c) = text[..start].chars().next_back().filter(|c| is_word(*c)) {
            start -= c.len_utf8();
        }
        let mut end = p.col.min(text.len());
        while let Some(c) = text[end..].chars().next().filter(|c| is_word(*c)) {
            end += c.len_utf8();
        }
        start..end
    }

    // -- edits ----------------------------------------------------------------

    /// `text` put in place of what is between `from` and `to`, as one
    /// step of undo, or joined to the step before when it is typing that
    /// goes on where the last typing stopped. Returns where the new text ends.
    pub fn replace(&mut self, from: Pos, to: Pos, text: &str, sel_before: Sel) -> Pos {
        let (from, to) = if from <= to { (from, to) } else { (to, from) };
        let removed = self.slice(from, to);
        let end = self.splice(from, to, text);
        self.redo.clear();
        self.revision += 1;
        let sel_after = Sel::caret(end);
        let now = Instant::now();
        if let Some(last) = self.undo.last_mut().filter(|e| e.parts.len() == 1) {
            let part = &mut last.parts[0];
            let typing = removed.is_empty() && !text.contains('\n') && text.chars().count() == 1 && part.removed.is_empty() && !part.inserted.is_empty();
            let last_end = end_of(part.from, &part.inserted);
            if typing && last_end == from && now.duration_since(last.at) < COALESCE {
                part.inserted.push_str(text);
                last.sel_after = sel_after;
                last.at = now;
                return end;
            }
            let erasing = text.is_empty() && !removed.contains('\n') && removed.chars().count() == 1 && part.inserted.is_empty() && !part.removed.is_empty();
            if erasing && to == part.from && now.duration_since(last.at) < COALESCE {
                part.removed.insert_str(0, &removed);
                part.from = from;
                last.sel_after = sel_after;
                last.at = now;
                return end;
            }
        }
        self.undo.push(Edit { parts: vec![Part { from, removed, inserted: text.to_string() }], sel_before, sel_after, at: now });
        end
    }

    /// Several changes as one step: each `(from, to, text)` in the text as
    /// it is now, applied from the last to the first. The selection after
    /// is the caller's.
    pub fn replace_many(&mut self, mut changes: Vec<(Pos, Pos, String)>, sel_before: Sel, sel_after: Sel) {
        if changes.is_empty() {
            return;
        }
        changes.sort_by(|a, b| b.0.cmp(&a.0));
        let mut parts = Vec::with_capacity(changes.len());
        for (from, to, text) in changes {
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            let removed = self.slice(from, to);
            self.splice(from, to, &text);
            parts.push(Part { from, removed, inserted: text });
        }
        self.redo.clear();
        self.revision += 1;
        self.undo.push(Edit { parts, sel_before, sel_after, at: Instant::now() - COALESCE });
    }

    /// The text spliced, with no step of undo made: the undo and redo
    /// use it to go back and forth.
    fn splice(&mut self, from: Pos, to: Pos, text: &str) -> Pos {
        let head = self.lines[from.line][..from.col].to_string();
        let tail = self.lines[to.line][to.col..].to_string();
        let mut new_lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let end_col = new_lines.last().map_or(0, String::len) + if new_lines.len() == 1 { head.len() } else { 0 };
        let end_line = from.line + new_lines.len() - 1;
        new_lines[0].insert_str(0, &head);
        new_lines.last_mut().unwrap().push_str(&tail);
        self.lines.splice(from.line..=to.line, new_lines);
        Pos::new(end_line, end_col)
    }

    /// Take the last step back. The selection to show after it.
    pub fn undo(&mut self) -> Option<Sel> {
        let e = self.undo.pop()?;
        // The parts were applied from the last place to the first, so
        // they are taken back from the first to the last.
        for p in e.parts.iter().rev() {
            let to = end_of(p.from, &p.inserted);
            self.splice(p.from, to, &p.removed);
        }
        self.revision += 1;
        let sel = e.sel_before;
        self.redo.push(e);
        Some(sel)
    }

    pub fn redo(&mut self) -> Option<Sel> {
        let e = self.redo.pop()?;
        for p in &e.parts {
            let to = end_of(p.from, &p.removed);
            self.splice(p.from, to, &p.inserted);
        }
        self.revision += 1;
        let sel = e.sel_after;
        self.undo.push(e);
        Some(sel)
    }

    /// A step that is not typing: the next keystroke begins a new one.
    pub fn end_step(&mut self) {
        if let Some(last) = self.undo.last_mut() {
            last.at = Instant::now() - COALESCE;
        }
    }
}

/// Where `text` put in at `from` ends.
fn end_of(from: Pos, text: &str) -> Pos {
    match text.rsplit_once('\n') {
        Some((head, last)) => Pos::new(from.line + head.matches('\n').count() + 1, last.len()),
        None => Pos::new(from.line, from.col + text.len()),
    }
}

/// A line's list item, if it is one: its indentation, its marker as
/// written ("- ", "1. ", "- [ ] "), and what kind it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLine {
    pub indent: String,
    pub marker: String,
    pub kind: ListKind,
    /// The text after the marker.
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Ordered(u64),
    Task(bool),
}

/// The list item a line is, read as markdown reads it: up to three spaces
/// or tabs, a bullet or a number with a dot or a bracket, a space, and
/// for a task a box.
pub fn list_line(text: &str) -> Option<ListLine> {
    let indent_len = text.len() - text.trim_start_matches([' ', '\t']).len();
    let (indent, rest) = text.split_at(indent_len);
    let (marker_end, kind) = if let Some(after) = rest.strip_prefix(['-', '*', '+']) {
        if !after.starts_with(' ') && !after.is_empty() {
            return None;
        }
        let task = after.get(1..4).and_then(|b| match b {
            "[ ]" => Some(false),
            "[x]" | "[X]" => Some(true),
            _ => None,
        });
        match task {
            Some(done) if after.len() == 4 || after.as_bytes().get(4) == Some(&b' ') => (1 + 4 + usize::from(after.len() > 4), ListKind::Task(done)),
            _ => (1 + usize::from(!after.is_empty()), ListKind::Bullet),
        }
    } else {
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let after = &rest[digits..];
        if !(after.starts_with('.') || after.starts_with(')')) {
            return None;
        }
        let after = &after[1..];
        if !after.starts_with(' ') && !after.is_empty() {
            return None;
        }
        (digits + 1 + usize::from(!after.is_empty()), ListKind::Ordered(rest[..digits].parse().unwrap_or(1)))
    };
    Some(ListLine { indent: indent.to_string(), marker: rest[..marker_end].to_string(), kind, body: rest[marker_end..].to_string() })
}

/// The marker the next line of a list takes: the same bullet, the next
/// number, an empty box.
pub fn next_marker(item: &ListLine) -> String {
    match item.kind {
        ListKind::Bullet => item.marker.clone(),
        ListKind::Task(_) => {
            let bullet = item.marker.chars().next().unwrap_or('-');
            format!("{bullet} [ ] ")
        }
        ListKind::Ordered(n) => {
            let sep = if item.marker.contains(')') { ')' } else { '.' };
            format!("{}{sep} ", n + 1)
        }
    }
}

/// The depth of a quote line: its leading `>` marks, with the spaces
/// between, and where its own text begins.
pub fn quote_line(text: &str) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut at = 0;
    let bytes = text.as_bytes();
    loop {
        while at < bytes.len() && bytes[at] == b' ' && at < 3 + depth * 2 {
            at += 1;
        }
        if at < bytes.len() && bytes[at] == b'>' {
            depth += 1;
            at += 1;
            if at < bytes.len() && bytes[at] == b' ' {
                at += 1;
            }
        } else {
            break;
        }
    }
    (depth > 0).then_some((depth, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_splice_lines_and_come_back_in_order() {
        let mut d = Doc::from_text("one\ntwo\nthree");
        let sel = Sel::caret(Pos::new(1, 1));
        let end = d.replace(Pos::new(1, 1), Pos::new(1, 1), "X\nY", sel);
        assert_eq!(d.text(), "one\ntX\nYwo\nthree");
        assert_eq!(end, Pos::new(2, 1));
        let end = d.replace(Pos::new(0, 1), Pos::new(2, 1), "", sel);
        assert_eq!(d.text(), "owo\nthree");
        assert_eq!(end, Pos::new(0, 1));
        assert_eq!(d.undo(), Some(sel));
        assert_eq!(d.text(), "one\ntX\nYwo\nthree");
        assert!(d.undo().is_some());
        assert_eq!(d.text(), "one\ntwo\nthree");
        assert!(d.undo().is_none());
        assert!(d.redo().is_some());
        assert_eq!(d.text(), "one\ntX\nYwo\nthree");
    }

    #[test]
    fn typing_is_one_step_and_a_pause_begins_another() {
        let mut d = Doc::from_text("");
        let mut at = Pos::default();
        for c in ["a", "b", "c"] {
            at = d.replace(at, at, c, Sel::caret(at));
        }
        d.end_step();
        d.replace(at, at, "d", Sel::caret(at));
        assert_eq!(d.text(), "abcd");
        d.undo();
        assert_eq!(d.text(), "abc");
        d.undo();
        assert_eq!(d.text(), "");
        // Erasing joins too.
        let mut d = Doc::from_text("abc");
        let mut at = Pos::new(0, 3);
        for _ in 0..2 {
            let from = d.prev_grapheme(at);
            at = d.replace(from, at, "", Sel::caret(at));
        }
        assert_eq!(d.text(), "a");
        d.undo();
        assert_eq!(d.text(), "abc");
    }

    #[test]
    fn many_changes_are_one_step() {
        let mut d = Doc::from_text("1. a\n3. b\n7. c");
        let sel = Sel::caret(Pos::new(0, 0));
        d.replace_many(vec![(Pos::new(1, 0), Pos::new(1, 1), "2".into()), (Pos::new(2, 0), Pos::new(2, 1), "3".into())], sel, sel);
        assert_eq!(d.text(), "1. a\n2. b\n3. c");
        assert_eq!(d.undo(), Some(sel));
        assert_eq!(d.text(), "1. a\n3. b\n7. c");
        assert!(d.redo().is_some());
        assert_eq!(d.text(), "1. a\n2. b\n3. c");
    }

    #[test]
    fn places_step_by_grapheme_and_count_in_utf16() {
        let d = Doc::from_text("héllo 🇫🇷x\nnext");
        let p = d.next_grapheme(Pos::new(0, 7));
        assert_eq!(p, Pos::new(0, 15), "the flag is one step");
        assert_eq!(d.prev_grapheme(p), Pos::new(0, 7));
        assert_eq!(d.next_grapheme(Pos::new(0, 16)), Pos::new(1, 0));
        assert_eq!(d.prev_grapheme(Pos::new(1, 0)), Pos::new(0, 16));
        assert_eq!(d.utf16_of(Pos::new(0, 15)), 10);
        assert_eq!(d.pos_of_utf16(10), Pos::new(0, 15));
        assert_eq!(d.pos_of_utf16(12), Pos::new(1, 0));
        assert_eq!(d.utf16_len(), 16);
        assert_eq!(d.offset_of(Pos::new(1, 2)), 19);
        assert_eq!(d.pos_of(19), Pos::new(1, 2));
    }

    #[test]
    fn words_are_found_as_the_keys_find_them() {
        let d = Doc::from_text("  some words_here, more");
        assert_eq!(d.word_left(Pos::new(0, 6)), Pos::new(0, 2));
        assert_eq!(d.word_left(Pos::new(0, 23)), Pos::new(0, 19));
        assert_eq!(d.word_right(Pos::new(0, 0)), Pos::new(0, 6));
        assert_eq!(d.word_right(Pos::new(0, 6)), Pos::new(0, 17));
        assert_eq!(d.word_at(Pos::new(0, 9)), 7..17);
    }

    #[test]
    fn list_lines_are_read_and_continued() {
        let l = list_line("  - [ ] a task").unwrap();
        assert_eq!((l.indent.as_str(), l.marker.as_str(), l.kind, l.body.as_str()), ("  ", "- [ ] ", ListKind::Task(false), "a task"));
        assert_eq!(next_marker(&l), "- [ ] ");
        let l = list_line("3) three").unwrap();
        assert_eq!((l.marker.as_str(), l.kind), ("3) ", ListKind::Ordered(3)));
        assert_eq!(next_marker(&l), "4) ");
        assert_eq!(list_line("* ").unwrap().kind, ListKind::Bullet);
        assert_eq!(list_line("-").unwrap().marker, "-");
        assert!(list_line("-not a list").is_none());
        assert!(list_line("1.5 not a list").is_none());
        assert_eq!(quote_line("> > deep"), Some((2, 4)));
        assert_eq!(quote_line(">plain"), Some((1, 1)));
        assert_eq!(quote_line("plain"), None);
    }
}
