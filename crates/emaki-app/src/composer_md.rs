//! The marks a key puts around what is selected in the composer
//! (`docs/composer.md`, Live markdown). ⌘B and ⌘I are the editor's own
//! (`live/`); what it has no key for, underlined and struck words, is
//! here.

use std::ops::Range;

/// What ⌘U and ⇧⌘X do to the text: the marks go around what is
/// selected, or come off it when it has them, around it or just inside
/// it; with nothing selected an empty pair is put at the caret, the
/// caret between. Answers the text and the selection after.
pub(crate) fn toggle(text: &str, selected: Range<usize>, open: &str, close: &str) -> (String, Range<usize>) {
    let (a, b) = (selected.start.min(text.len()), selected.end.min(text.len()));
    let inner = &text[a..b];
    // The marks stand just outside the selection.
    if text[..a].ends_with(open) && text[b..].starts_with(close) {
        let out = format!("{}{}{}", &text[..a - open.len()], inner, &text[b + close.len()..]);
        return (out, a - open.len()..b - open.len());
    }
    // Or are its own two ends.
    if inner.len() >= open.len() + close.len() && inner.starts_with(open) && inner.ends_with(close) {
        let bare = &inner[open.len()..inner.len() - close.len()];
        return (format!("{}{}{}", &text[..a], bare, &text[b..]), a..a + bare.len());
    }
    let out = format!("{}{}{}{}{}", &text[..a], open, inner, close, &text[b..]);
    (out, a + open.len()..b + open.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marks_go_on_and_come_off() {
        assert_eq!(toggle("a word here", 2..6, "~~", "~~"), ("a ~~word~~ here".into(), 4..8));
        assert_eq!(toggle("a ~~word~~ here", 4..8, "~~", "~~"), ("a word here".into(), 2..6));
        assert_eq!(toggle("a ~~word~~ here", 2..10, "~~", "~~"), ("a word here".into(), 2..6));
        assert_eq!(toggle("ab", 1..1, "<u>", "</u>"), ("a<u></u>b".into(), 4..4));
    }
}
