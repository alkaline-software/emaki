//! Emaki: words found in a text view. The application holds one
//! [`TextFind`] and hands it to the view (`TextView::find`); each run of
//! text paints its matches as it paints itself, numbered in the order
//! the document is painted, the current one stronger, and says where the
//! current one stands so the application can bring it into view.

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

use gpui::{Bounds, Pixels};

#[derive(Default)]
struct Inner {
    /// The words, as typed; empty finds nothing.
    query: RefCell<String>,
    case_sensitive: Cell<bool>,
    /// Which match is the current one, by its number.
    current: Cell<usize>,
    /// Matches painted so far in this frame, and in the last one whole.
    seen: Cell<usize>,
    count: Cell<usize>,
    /// Where the current match was painted, with its number.
    current_at: Cell<Option<(usize, Bounds<Pixels>)>>,
}

/// What a text view finds, shared between the application and the view.
#[derive(Clone, Default)]
pub struct TextFind(Rc<Inner>);

impl TextFind {
    pub fn new() -> Self {
        Self::default()
    }

    /// The words to find. A change goes back to the first match.
    pub fn set_query(&self, query: &str, case_sensitive: bool) {
        if *self.0.query.borrow() != query || self.0.case_sensitive.get() != case_sensitive {
            *self.0.query.borrow_mut() = query.to_string();
            self.0.case_sensitive.set(case_sensitive);
            self.0.current.set(0);
            self.0.current_at.set(None);
        }
    }

    pub fn query(&self) -> String {
        self.0.query.borrow().clone()
    }

    /// How many matches the view painted when it last painted whole.
    pub fn count(&self) -> usize {
        self.0.count.get()
    }

    pub fn current(&self) -> usize {
        self.0.current.get()
    }

    /// To the next match, or the one before, round the ends.
    pub fn step(&self, by: i64) {
        let n = self.count() as i64;
        if n > 0 {
            self.0.current.set(((self.current() as i64 + by).rem_euclid(n)) as usize);
        }
    }

    /// Where the current match was last painted, once it has been.
    pub fn current_bounds(&self) -> Option<Bounds<Pixels>> {
        self.0.current_at.get().filter(|(ix, _)| *ix == self.current()).map(|(_, b)| b)
    }

    pub(crate) fn begin_frame(&self) {
        self.0.seen.set(0);
    }

    pub(crate) fn end_frame(&self) {
        self.0.count.set(self.0.seen.get());
        if self.current() >= self.count() {
            self.0.current.set(0);
        }
    }

    /// The matches in one run of text, as byte ranges of it, each with
    /// its number and whether it is the current one.
    pub(crate) fn take_matches(&self, text: &str) -> Vec<(Range<usize>, usize, bool)> {
        let query = self.0.query.borrow();
        let ranges = find_in(text, &query, self.0.case_sensitive.get());
        ranges
            .into_iter()
            .map(|range| {
                let ix = self.0.seen.get();
                self.0.seen.set(ix + 1);
                (range, ix, ix == self.current())
            })
            .collect()
    }

    pub(crate) fn painted_current(&self, ix: usize, bounds: Bounds<Pixels>) {
        self.0.current_at.set(Some((ix, bounds)));
    }
}

/// Emaki: where a text view's headings were painted, in the order the
/// document paints them, each with its level. The application holds one
/// (`TextView::headings`) and reads it after a frame: an outline of the
/// document goes to a heading by where it stands.
#[derive(Clone, Default)]
pub struct HeadingSpots(Rc<SpotsInner>);

#[derive(Default)]
struct SpotsInner {
    now: RefCell<Vec<(u8, Bounds<Pixels>)>>,
    last: RefCell<Vec<(u8, Bounds<Pixels>)>>,
}

impl HeadingSpots {
    pub fn new() -> Self {
        Self::default()
    }

    /// Each heading's level and bounds, as the last whole frame painted
    /// them.
    pub fn spots(&self) -> Vec<(u8, Bounds<Pixels>)> {
        self.0.last.borrow().clone()
    }

    pub(crate) fn begin_frame(&self) {
        self.0.now.borrow_mut().clear();
    }

    pub(crate) fn painted(&self, level: u8, bounds: Bounds<Pixels>) {
        self.0.now.borrow_mut().push((level, bounds));
    }

    pub(crate) fn end_frame(&self) {
        let now = std::mem::take(&mut *self.0.now.borrow_mut());
        *self.0.last.borrow_mut() = now;
    }
}

/// Where `query` stands in `text`, case folded unless `case_sensitive`.
/// Folding that changes a text's length in bytes falls back to the case
/// as written, so the ranges stay the text's own.
fn find_in(text: &str, query: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let (hay, needle) = if case_sensitive {
        (text.to_string(), query.to_string())
    } else {
        let (h, n) = (text.to_lowercase(), query.to_lowercase());
        if h.len() == text.len() && n.len() == query.len() { (h, n) } else { (text.to_string(), query.to_string()) }
    };
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        out.push(start..start + needle.len());
        from = start + needle.len().max(1);
        if from > hay.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_in_order_and_steps_round() {
        assert_eq!(find_in("Cat cat CAT", "cat", false), vec![0..3, 4..7, 8..11]);
        assert_eq!(find_in("Cat cat CAT", "cat", true), vec![4..7]);
        assert!(find_in("anything", "", false).is_empty());
        let find = TextFind::new();
        find.set_query("a", false);
        find.begin_frame();
        assert_eq!(find.take_matches("a b a").iter().map(|m| (m.1, m.2)).collect::<Vec<_>>(), vec![(0, true), (1, false)]);
        assert_eq!(find.take_matches("a").len(), 1);
        find.end_frame();
        assert_eq!(find.count(), 3);
        find.step(-1);
        assert_eq!(find.current(), 2);
        find.step(1);
        assert_eq!(find.current(), 0);
    }
}
