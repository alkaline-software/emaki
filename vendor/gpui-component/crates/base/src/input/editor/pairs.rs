//! Emaki: a pair's close put in as its open is typed, stepped over when
//! typed itself, and taken out with its open by Backspace; and a pair
//! put around a selection. Each as VS Code does it (`autoClosingBrackets`
//! and `autoClosingQuotes` as a language defines them, `autoClosingOvertype`
//! and `autoClosingDelete` on `auto`, `autoSurround`). Which pairs is the
//! language's, set by the application (`set_auto_pairs`).

/// The pairs a language has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoPairs {
    /// An open typed puts its close after the caret.
    pub closing: Vec<(char, char)>,
    /// An open typed over a selection puts the pair around it.
    pub surround: Vec<(char, char)>,
}

/// What a pair is closed before when the caret is not at the line's
/// end: VS Code's own `autoCloseBefore`, and any pair's close.
const CLOSE_BEFORE: &str = ";:.,=}])> \n\t";

/// What typing a character does.
#[derive(Debug, PartialEq, Eq)]
pub enum Typed {
    /// The open goes in with this close after the caret.
    Close(char),
    /// The close is there already, put in by the editor, and the caret
    /// has stayed inside since: the caret steps over it.
    StepOver,
    /// The open and this close go around the selection, which stays
    /// selected.
    Surround(char),
    /// The character alone.
    Plain,
}

impl AutoPairs {
    fn close_of(&self, open: char) -> Option<char> {
        self.closing.iter().find(|(o, _)| *o == open).map(|(_, c)| *c)
    }

    fn is_close(&self, ch: char) -> bool {
        self.closing.iter().any(|(_, c)| *c == ch)
    }

    /// What typing `ch` does. `before` is the line up to the caret and
    /// `after` the rest of it, `selected` the selection's text when
    /// there is one, and `closed_here` whether the character at the
    /// caret is a close the editor put in with the caret inside since.
    pub fn typed(&self, ch: char, before: &str, after: &str, selected: Option<&str>, closed_here: bool) -> Typed {
        if let Some(sel) = selected {
            // Not around whitespace alone; and a quote typed over that
            // one quote replaces it.
            let Some((_, close)) = self.surround.iter().find(|(o, _)| *o == ch) else { return Typed::Plain };
            let blank = sel.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'));
            let same_quote = is_quote(ch) && sel.chars().eq([ch]);
            return if blank || same_quote { Typed::Plain } else { Typed::Surround(*close) };
        }
        // A close typed where the editor put that close: over it, not
        // one more. A quote after a backslash is escaped, not a close.
        if self.is_close(ch) && after.starts_with(ch) && closed_here && !(is_quote(ch) && before.ends_with('\\')) {
            return Typed::StepOver;
        }
        let Some(close) = self.close_of(ch) else { return Typed::Plain };
        // Only at the line's end, or before a space or what ends a term.
        if let Some(next) = after.chars().next() {
            if !CLOSE_BEFORE.contains(next) && !self.is_close(next) {
                return Typed::Plain;
            }
        }
        if ch == close {
            // A quote: not after a word, where it is an apostrophe or
            // ends a string; nor inside a string begun on the line.
            if before.chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_') || quote_open(before, ch) {
                return Typed::Plain;
            }
        } else if unmatched_close_after(after, ch, close) {
            // A close further on with no open for it is this open's:
            // another would be one over.
            return Typed::Plain;
        }
        Typed::Close(close)
    }

    /// Whether Backspace takes out the open before the caret with the
    /// close after it, which the editor put there.
    pub fn deletes_pair(&self, before: &str, after: &str, closed_here: bool) -> bool {
        closed_here && before.chars().last().zip(after.chars().next()).is_some_and(|pair| self.closing.contains(&pair))
    }
}

fn is_quote(ch: char) -> bool {
    matches!(ch, '\'' | '"' | '`')
}

/// Whether a string in `quote` begun on the line is still open at its
/// end: an odd count of them, escaped ones left out.
fn quote_open(before: &str, quote: char) -> bool {
    let (mut open, mut escaped) = (false, false);
    for c in before.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            open = !open;
        }
    }
    open
}

/// Whether `after` holds a `close` with no `open` before it there.
fn unmatched_close_after(after: &str, open: char, close: char) -> bool {
    let mut depth = 0i32;
    for c in after.chars() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth < 0 {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs() -> AutoPairs {
        AutoPairs { closing: vec![('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')], surround: vec![('(', ')'), ('*', '*'), ('"', '"')] }
    }

    #[test]
    fn an_open_is_closed_where_vs_code_closes_it() {
        let p = pairs();
        assert_eq!(p.typed('(', "foo", "", None, false), Typed::Close(')'));
        assert_eq!(p.typed('(', "foo", " bar", None, false), Typed::Close(')'));
        assert_eq!(p.typed('(', "foo", "bar", None, false), Typed::Plain);
        assert_eq!(p.typed('(', "foo(", ")", None, false), Typed::Plain, "the close there is this open's");
        assert_eq!(p.typed('(', "foo", "))", None, false), Typed::Plain);
        assert_eq!(p.typed('[', "a = ", "];", None, false), Typed::Plain);
        assert_eq!(p.typed('[', "a = ", ";", None, false), Typed::Close(']'));
        assert_eq!(p.typed('{', "", "}", None, false), Typed::Plain);
        assert_eq!(p.typed('(', "x = (", ")", None, false), Typed::Plain);
        assert_eq!(p.typed('(', "", "()", None, false), Typed::Plain, "not before an open");
        assert_eq!(p.typed('(', "", " ()", None, false), Typed::Close(')'));
    }

    #[test]
    fn a_quote_is_closed_only_where_it_begins_a_string() {
        let p = pairs();
        assert_eq!(p.typed('"', "x = ", "", None, false), Typed::Close('"'));
        assert_eq!(p.typed('"', "don", "", None, false), Typed::Plain, "after a word");
        assert_eq!(p.typed('\'', "don", "t", None, false), Typed::Plain);
        assert_eq!(p.typed('"', "x = \"a ", "", None, false), Typed::Plain, "inside a string");
        assert_eq!(p.typed('"', "x = \"a\\\" ", "", None, false), Typed::Plain, "an escaped quote does not end it");
        assert_eq!(p.typed('"', "x = \"a\" + ", "", None, false), Typed::Close('"'));
        assert_eq!(p.typed('"', "x = ", "\"", None, false), Typed::Close('"'), "before a quote, as VS Code allows");
    }

    #[test]
    fn a_close_steps_over_the_one_the_editor_put_in() {
        let p = pairs();
        assert_eq!(p.typed(')', "foo(", ")", None, true), Typed::StepOver);
        assert_eq!(p.typed(')', "foo(", ")", None, false), Typed::Plain, "one typed by hand is not stepped over");
        assert_eq!(p.typed(')', "foo(", "]", None, true), Typed::Plain);
        assert_eq!(p.typed('"', "\"a", "\"", None, true), Typed::StepOver);
        assert_eq!(p.typed('"', "\"a\\", "\"", None, true), Typed::Plain, "escaped");
    }

    #[test]
    fn a_selection_is_put_inside_a_pair() {
        let p = pairs();
        assert_eq!(p.typed('*', "", "", Some("word"), false), Typed::Surround('*'));
        assert_eq!(p.typed('(', "", "", Some("a\nb"), false), Typed::Surround(')'));
        assert_eq!(p.typed('(', "", "", Some("  \n\t"), false), Typed::Plain, "whitespace alone");
        assert_eq!(p.typed('"', "", "", Some("\""), false), Typed::Plain, "a quote over that quote replaces it");
        assert_eq!(p.typed('[', "", "", Some("x"), false), Typed::Plain, "not a surround pair here");
    }

    #[test]
    fn backspace_takes_both_of_a_pair_the_editor_made() {
        let p = pairs();
        assert!(p.deletes_pair("foo(", ")", true));
        assert!(!p.deletes_pair("foo(", ")", false));
        assert!(!p.deletes_pair("foo(", "]", true));
        assert!(!p.deletes_pair("foo", ")", true));
    }
}
