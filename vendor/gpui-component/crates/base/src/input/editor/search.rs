use crate::input::InputModeKind;
use gpui::{Context, Window};
use regex::Regex;
use ropey::Rope;
use std::{ops::Range, rc::Rc};

use super::{InputBaseState, Replace, RopeExt as _, Search, movement::MoveDirection};

/// Stateful, presentation-independent search engine used by text inputs.
///
/// Emaki: a regular expression, with the query escaped unless `regex`,
/// and whole words kept by the characters around a match, where upstream
/// had one literal through AhoCorasick, case folded for ASCII only.
#[derive(Debug, Clone)]
pub struct SearchMatcher {
    text: Rope,
    pub query: Option<Regex>,
    whole_word: bool,
    regex: bool,
    invalid: bool,
    matched_ranges: Rc<Vec<Range<usize>>>,
    current_match_ix: usize,
    replacing: bool,
}

/// How a query matches: case, whole words, a regular expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_insensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone)]
pub struct SearchSession {
    pub open: bool,
    pub replace_mode: bool,
    pub case_insensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub query: String,
    pub replacement: String,
    pub anchor_offset: Option<usize>,
    pub matcher: SearchMatcher,
    /// Emaki: counts the opens, so a panel shows the query anew at an
    /// open and not at each change its own typing makes.
    pub opened: u64,
    /// Emaki: when it was closed, so the panel is drawn while it goes
    /// (`closing`).
    pub closed_at: Option<std::time::Instant>,
}

/// Emaki: how long the search panel takes to open or close.
pub const SEARCH_ANIM: std::time::Duration = std::time::Duration::from_millis(200);

impl Default for SearchSession {
    fn default() -> Self {
        Self {
            open: false,
            replace_mode: false,
            case_insensitive: true,
            whole_word: false,
            regex: false,
            query: String::new(),
            replacement: String::new(),
            anchor_offset: None,
            matcher: SearchMatcher::new(),
            opened: 0,
            closed_at: None,
        }
    }
}

impl SearchSession {
    pub(crate) fn open(&mut self, replace_mode: bool, replaceable: bool) {
        self.open = true;
        self.opened += 1;
        self.closed_at = None;
        self.replace_mode = replace_mode && replaceable;
    }

    pub(crate) fn close(&mut self) {
        if self.open {
            self.closed_at = Some(std::time::Instant::now());
        }
        self.open = false;
    }

    /// Closed a moment ago: the panel is still drawn, going.
    pub fn closing(&self) -> bool {
        !self.open && self.closed_at.is_some_and(|at| at.elapsed() < SEARCH_ANIM)
    }

    pub(crate) fn update_query(&mut self, query: impl Into<String>, case_insensitive: bool) {
        self.query = query.into();
        self.case_insensitive = case_insensitive;
        self.matcher.set_query(&self.query, self.options());
    }

    pub fn options(&self) -> SearchOptions {
        SearchOptions { case_insensitive: self.case_insensitive, whole_word: self.whole_word, regex: self.regex }
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    pub fn open_search(&mut self, replace_mode: bool, cx: &mut Context<Self>) {
        if !self.searchable {
            return;
        }
        self.search_session
            .open(replace_mode, self.is_replaceable());
        let selected = self.selected_text().to_string();
        if !selected.is_empty() {
            self.search_session.query = selected;
        }
        // Emaki: the find starts from the selection, or the caret, as an
        // editor's does, so a word selected is the match it is on; from
        // the view's top it was the first match in view, wherever the
        // caret was.
        self.search_session.anchor_offset = Some(self.selected_range().start);
        let options = self.search_session.options();
        self.search_session.matcher.set_query(&self.search_session.query, options);
        self.search_session.matcher.update(&self.text);
        if let Some(anchor) = self.search_session.anchor_offset {
            self.search_session.matcher.update_cursor_by_offset(anchor);
        }
        cx.notify();
    }

    pub fn search_session(&self) -> &SearchSession {
        &self.search_session
    }

    #[doc(hidden)]
    pub fn set_search_replace_mode(&mut self, replace_mode: bool, cx: &mut Context<Self>) {
        self.search_session.replace_mode = replace_mode && self.is_replaceable();
        cx.notify();
    }

    /// Returns true if the search panel can replace the matches.
    ///
    /// This is false when the input is not `replaceable`, or when it is
    /// `disabled` or `readonly`.
    pub fn is_replaceable(&self) -> bool {
        self.replaceable && self.is_editable()
    }

    pub fn set_search_query(
        &mut self,
        query: impl Into<String>,
        case_insensitive: bool,
        cx: &mut Context<Self>,
    ) {
        self.search_session.update_query(query, case_insensitive);
        self.search_session.matcher.update(&self.text);
        // Emaki: the match it is on stays the first from where the find
        // began, as the words change.
        if let Some(anchor) = self.search_session.anchor_offset {
            self.search_session.matcher.update_cursor_by_offset(anchor);
        }
        cx.notify();
    }

    /// Emaki: the query with every option, as the panel's toggles set them.
    pub fn set_search_options(&mut self, query: impl Into<String>, options: SearchOptions, cx: &mut Context<Self>) {
        self.search_session.whole_word = options.whole_word;
        self.search_session.regex = options.regex;
        self.set_search_query(query, options.case_insensitive, cx);
    }

    /// Emaki: closed with a match current, the match is what is selected,
    /// so the caret is where the find had come to, as it is in an editor's
    /// find; it stayed where it was before the find opened.
    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        let found = self.search_session.open.then(|| {
            let matcher = &self.search_session.matcher;
            matcher.matched_ranges().get(matcher.current_match_index()).cloned()
        });
        self.search_session.close();
        if let Some(Some(range)) = found {
            self.set_selected_range(range, cx);
        }
        cx.notify();
    }

    /// Emaki: the query's match after the selection, or the one before
    /// it, selected and scrolled to, as Sublime Text's Find Next and Find
    /// Previous are, the panel open or not: the matches are found again
    /// in the text as it is now. None with no query or no match.
    pub fn find_from_selection(&mut self, forward: bool, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let query = self.search_session.query.clone();
        if query.is_empty() {
            return None;
        }
        let options = self.search_session.options();
        self.search_session.matcher.set_query(&query, options);
        self.search_session.matcher.update(&self.text);
        let ranges = self.search_session.matcher.matched_ranges();
        if ranges.is_empty() {
            return None;
        }
        let sel = self.selected_range();
        let ix = if forward {
            ranges.iter().position(|r| r.start >= sel.end && *r != sel).unwrap_or(0)
        } else {
            ranges.iter().rposition(|r| r.end <= sel.start && *r != sel).unwrap_or(ranges.len() - 1)
        };
        let range = ranges[ix].clone();
        self.search_session.matcher.set_current_match_index(ix);
        self.set_selected_range(range.clone(), cx);
        self.scroll_to(if forward { range.end } else { range.start }, None, cx);
        cx.notify();
        Some(range)
    }

    pub fn next_search_match(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let previous = self.search_session.matcher.current_match_index();
        let range = self.search_session.matcher.next()?;
        let direction = (self.search_session.matcher.current_match_index() > previous)
            .then_some(MoveDirection::Down);
        self.scroll_to(range.end, direction, cx);
        Some(range)
    }

    pub fn previous_search_match(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let previous = self.search_session.matcher.current_match_index();
        let range = self.search_session.matcher.next_back()?;
        let direction = (self.search_session.matcher.current_match_index() < previous)
            .then_some(MoveDirection::Up);
        self.scroll_to(range.start, direction, cx);
        Some(range)
    }

    pub fn replace_current_search_match(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_replaceable() {
            return false;
        }
        let matcher = &mut self.search_session.matcher;
        let Some(range) = matcher
            .matched_ranges()
            .get(matcher.current_match_index())
            .cloned()
        else {
            return false;
        };
        let replacement = matcher.replacement_for(&range, replacement);
        let next = matcher.peek().unwrap_or_else(|| range.clone());
        let direction = matcher
            .has_next_without_wrap()
            .then_some(MoveDirection::Down);
        if direction.is_none() {
            matcher.set_current_match_index(0);
        }
        matcher.begin_replacement();
        let range_utf16 = self.range_to_utf16(&range);
        self.scroll_to(next.end, direction, cx);
        self.replace_text_in_range_silent(Some(range_utf16), &replacement, window, cx);
        true
    }

    pub fn replace_all_search_matches(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        if !self.is_replaceable() {
            return 0;
        }
        let ranges = self.search_session.matcher.matched_ranges();
        if ranges.is_empty() {
            return 0;
        }
        let mut text = self.text.clone();
        for range in ranges.iter().rev() {
            let with = self.search_session.matcher.replacement_for(range, replacement);
            text.replace(range.clone(), &with);
        }
        self.search_session.matcher.begin_replacement();
        let count = ranges.len();
        self.replace_text_in_range_silent(Some(0..self.text.len()), &text.to_string(), window, cx);
        self.scroll_to(0, Some(MoveDirection::Down), cx);
        count
    }

    pub(super) fn update_search(&mut self, _cx: &mut gpui::App) {
        self.search_session.matcher.update(&self.text);
    }

    pub(super) fn on_action_search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        if !self.searchable {
            // Emaki: a field with no search of its own passes the key
            // on, so the window's own find answers it. Upstream ended
            // the key here, and ⌘F did nothing with the keyboard in the
            // composer.
            cx.propagate();
            return;
        }
        self.open_search(false, cx);
    }

    pub(super) fn on_action_replace(
        &mut self,
        _: &Replace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.searchable {
            // Emaki: passed on as ⌘F is, so the window's ⌘⇧F (the files
            // panel) answers it with the keyboard in a field.
            cx.propagate();
            return;
        }
        self.open_search(true, cx);
    }
}

impl Default for SearchMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchMatcher {
    pub fn new() -> Self {
        Self {
            text: "".into(),
            query: None,
            whole_word: false,
            regex: false,
            invalid: false,
            matched_ranges: Rc::new(Vec::new()),
            current_match_ix: 0,
            replacing: false,
        }
    }

    /// Update the source text and recompute matches.
    pub fn update(&mut self, text: &Rope) {
        if self.text.eq(text) {
            self.replacing = false;
            return;
        }
        self.text = text.clone();
        self.update_matches();
    }

    pub fn update_query(&mut self, query: &str, case_insensitive: bool) {
        self.set_query(query, SearchOptions { case_insensitive, whole_word: false, regex: false });
    }

    /// The query as `options` read it. A regular expression that does not
    /// parse matches nothing and says so (`is_invalid`).
    pub fn set_query(&mut self, query: &str, options: SearchOptions) {
        self.whole_word = options.whole_word;
        self.regex = options.regex;
        let pattern = if options.regex { query.to_string() } else { regex::escape(query) };
        let built = (!query.is_empty()).then(|| regex::RegexBuilder::new(&pattern).case_insensitive(options.case_insensitive).multi_line(true).build());
        self.invalid = matches!(built, Some(Err(_)));
        self.query = built.and_then(Result::ok);
        self.update_matches();
    }

    /// The query is a regular expression that does not parse.
    pub fn is_invalid(&self) -> bool {
        self.invalid
    }

    /// What replaces the match at `range`: `replacement` as written, or,
    /// for a regular expression, with `$1` and `${name}` filled from it.
    pub fn replacement_for(&self, range: &Range<usize>, replacement: &str) -> String {
        if !self.regex {
            return replacement.to_string();
        }
        let Some(query) = &self.query else { return replacement.to_string() };
        let text = self.text.to_string();
        match query.captures_at(&text, range.start).filter(|c| c.get(0).is_some_and(|m| m.range() == *range)) {
            Some(caps) => {
                let mut out = String::new();
                caps.expand(replacement, &mut out);
                out
            }
            None => replacement.to_string(),
        }
    }

    pub fn matched_ranges(&self) -> Rc<Vec<Range<usize>>> {
        self.matched_ranges.clone()
    }

    pub fn current_match_index(&self) -> usize {
        self.current_match_ix
    }

    pub fn len(&self) -> usize {
        self.matched_ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.matched_ranges.is_empty()
    }

    pub fn label(&self) -> String {
        if self.is_empty() {
            "0/0".into()
        } else {
            format!("{}/{}", self.current_match_ix + 1, self.len())
        }
    }

    fn peek(&self) -> Option<Range<usize>> {
        self.next_index()
            .and_then(|ix| self.matched_ranges.get(ix).cloned())
    }

    fn has_next_without_wrap(&self) -> bool {
        self.current_match_ix < self.matched_ranges.len().saturating_sub(1)
    }

    pub fn update_cursor_by_offset(&mut self, offset: usize) {
        for (ix, range) in self.matched_ranges.iter().enumerate() {
            self.current_match_ix = ix;
            if range.contains(&offset) || range.end >= offset {
                return;
            }
        }
    }

    /// Preserve the current logical match while a replacement mutates text.
    fn begin_replacement(&mut self) {
        self.replacing = true;
    }

    fn set_current_match_index(&mut self, index: usize) {
        self.current_match_ix = index.min(self.matched_ranges.len().saturating_sub(1));
    }

    fn next_index(&self) -> Option<usize> {
        if self.is_empty() {
            None
        } else if self.has_next_without_wrap() {
            Some(self.current_match_ix + 1)
        } else {
            Some(0)
        }
    }

    fn update_matches(&mut self) {
        let mut ranges = Vec::new();
        if let Some(query) = &self.query {
            let text = self.text.to_string();
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            ranges.extend(query.find_iter(&text).map(|m| m.range()).filter(|r| {
                !r.is_empty() && (!self.whole_word || (!word(text[..r.start].chars().next_back()) && !word(text[r.end..].chars().next())))
            }));
        }
        self.matched_ranges = Rc::new(ranges);
        if !self.replacing || self.is_empty() {
            self.current_match_ix = 0;
        } else {
            self.current_match_ix = self.current_match_ix.min(self.len() - 1);
        }
        self.replacing = false;
    }
}

impl Iterator for SearchMatcher {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let ix = self.next_index()?;
        self.current_match_ix = ix;
        self.matched_ranges.get(ix).cloned()
    }
}

impl DoubleEndedIterator for SearchMatcher {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }
        if self.current_match_ix == 0 {
            self.current_match_ix = self.len();
        }
        self.current_match_ix -= 1;
        self.matched_ranges.get(self.current_match_ix).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_navigates_and_preserves_replacement_position() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo FOO foo"));
        matcher.update_query("foo", true);
        assert_eq!(&*matcher.matched_ranges(), &[0..3, 4..7, 8..11]);
        assert_eq!(matcher.next(), Some(4..7));
        assert_eq!(matcher.next_back(), Some(0..3));

        matcher.set_current_match_index(2);
        matcher.begin_replacement();
        matcher.update(&Rope::from("foo FOO bar"));
        assert_eq!(matcher.current_match_index(), 1);
    }

    #[test]
    fn whole_words_regex_and_case() {
        let opts = |case_insensitive, whole_word, regex| SearchOptions { case_insensitive, whole_word, regex };
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("cat catalog Cat cat_x é É"));
        matcher.set_query("cat", opts(true, true, false));
        assert_eq!(&*matcher.matched_ranges(), &[0..3, 12..15]);
        matcher.set_query("cat", opts(false, false, false));
        assert_eq!(matcher.len(), 3);
        matcher.set_query("é", opts(true, false, false));
        assert_eq!(matcher.len(), 2);
        matcher.set_query("c(a)t", opts(false, false, true));
        assert_eq!(matcher.len(), 3);
        assert_eq!(matcher.replacement_for(&(4..7), "[$1]"), "[a]");
        matcher.set_query("c(", opts(false, false, true));
        assert!(matcher.is_invalid() && matcher.is_empty());
        matcher.set_query("c(", opts(false, false, false));
        assert!(!matcher.is_invalid());
        matcher.set_query("x*", opts(false, false, true));
        assert_eq!(&*matcher.matched_ranges(), &[20..21]);
    }

    #[test]
    fn next_wraps_to_start() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from(".....aaaaa.....aaaaa.....aaaaa"));
        matcher.update_query("aaaaa", false);
        matcher.set_current_match_index(2);
        assert_eq!(matcher.next(), Some(5..10));
    }

    #[test]
    fn replacement_keeps_current_match_index_on_next_match() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo foo foo"));
        matcher.update_query("foo", true);
        assert_eq!(matcher.label(), "1/3");

        assert!(matcher.has_next_without_wrap());
        matcher.begin_replacement();
        matcher.update(&Rope::from("bar foo foo"));
        assert_eq!(matcher.current_match_index(), 0);
        assert_eq!(matcher.matched_ranges()[0], 4..7);
        assert_eq!(matcher.label(), "1/2");

        matcher.set_current_match_index(1);
        assert!(!matcher.has_next_without_wrap());
        matcher.set_current_match_index(0);
        matcher.begin_replacement();
        matcher.update(&Rope::from("bar foo bar"));
        assert_eq!(matcher.current_match_index(), 0);
        assert_eq!(matcher.matched_ranges()[0], 4..7);
        assert_eq!(matcher.label(), "1/1");
    }

    #[test]
    fn update_matches_clamps_current_match_index_while_replacing() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("foo foo foo"));
        matcher.update_query("foo", true);
        matcher.set_current_match_index(2);
        matcher.begin_replacement();

        matcher.update(&Rope::from("foo xoo foo"));

        assert_eq!(matcher.len(), 2);
        assert_eq!(matcher.current_match_index(), 1);
        assert_eq!(matcher.label(), "2/2");
    }
}
