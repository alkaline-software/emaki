//! Emaki: a highlighter for comma- and tab-separated text, which has no
//! grammar worth parsing: each column in a colour of its own, as the
//! Rainbow CSV extension does it, so a value can be followed down a file
//! whose lines are wrapped. Listed in UPSTREAM.md.

use std::ops::Range;

use gpui::{HighlightStyle, SharedString};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
};
use ropey::Rope;

/// The colours the columns take in turn: the theme's own syntax colours,
/// the first column plain, as Rainbow CSV maps its ten to an editor's.
const COLUMNS: [&str; 10] = [
    "",
    "keyword",
    "function",
    "comment",
    "string",
    "variable",
    "number",
    "type",
    "title",
    "constant",
];

pub(crate) struct DelimitedInputHighlighter {
    language: SharedString,
    separator: u8,
    /// Where each field starts and which column it is, through the whole
    /// text in order. A field runs to the next one's start, so it takes
    /// the separator after it.
    fields: Vec<(usize, u16)>,
}

impl DelimitedInputHighlighter {
    pub(crate) fn new(language: &str) -> Self {
        Self {
            language: SharedString::from(language.to_string()),
            separator: if language == "tsv" { b'\t' } else { b',' },
            fields: Vec::new(),
        }
    }

    /// Read the whole text again: a quoted field may hold the separator
    /// and line breaks, so a field's column is known only from the start
    /// of its row, and a row's start only from the rows before it.
    fn scan(&mut self, text: &Rope) {
        let text = text.to_string();
        let bytes = text.as_bytes();
        self.fields.clear();
        let (mut column, mut quoted, mut fresh) = (0u16, false, true);
        for (at, byte) in bytes.iter().enumerate() {
            if fresh {
                self.fields.push((at, column));
                fresh = false;
                quoted = *byte == b'"';
                if quoted {
                    continue;
                }
            } else if quoted {
                // A doubled quote closes and opens again, which comes to
                // the same thing.
                if *byte == b'"' {
                    quoted = false;
                }
                continue;
            } else if *byte == b'"' {
                quoted = true;
                continue;
            }
            if *byte == self.separator {
                column = column.saturating_add(1);
                fresh = true;
            } else if *byte == b'\n' {
                column = 0;
                fresh = true;
            }
        }
    }
}

impl InputHighlighter for DelimitedInputHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn update(
        &mut self,
        _: Option<InputEdit>,
        text: &Rope,
        _: bool,
        _: &mut gpui::Window,
        _: &mut gpui::Context<EditorState>,
    ) {
        self.scan(text);
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let end = range.end;
        let mut styles = Vec::new();
        let first = self.fields.partition_point(|(start, _)| *start <= range.start).saturating_sub(1);
        let mut at = range.start;
        for (ix, (start, column)) in self.fields.iter().enumerate().skip(first) {
            if *start >= end {
                break;
            }
            let until = self.fields.get(ix + 1).map(|(next, _)| *next).unwrap_or(end).min(end);
            let from = (*start).max(at);
            if from > at {
                styles.push((at..from, HighlightStyle::default()));
            }
            if until > from {
                let name = COLUMNS[*column as usize % COLUMNS.len()];
                styles.push((from..until, resolver.style(name).unwrap_or_default()));
                at = until;
            }
        }
        if at < end {
            styles.push((at..end, HighlightStyle::default()));
        }
        styles
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}
