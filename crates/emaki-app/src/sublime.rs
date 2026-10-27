//! Sublime Text's keys in the file's editor, and nowhere else: what its
//! default key map (`Default (OSX).sublime-keymap`, and the Windows and
//! Linux ones off a Mac) does to the text of one view with one selection.
//! Bound under `FileEditor > Input`, the editor's own text, so the
//! composer, a name's field and the find card's field keep their keys,
//! and a key the window has (⌘L, ⌘E, ⌘K) is the editor's while the
//! keyboard is in it. The find card has Find Next and Find Previous of
//! its own (the vendored `SearchPanel`).
//!
//! The commands used most have their keys in `shortcuts.rs`, where
//! Settings, Shortcuts, changes them; the rest are bound here.
//!
//! What needs more than one selection (⌘D's further cursors, ⇧⌘L), a
//! project (⌘P, ⌘R, ⇧⌘F), panes, builds, macros or bookmarks is not
//! here: the editor has one selection in one file. ⇧⌘V stays the
//! window's, rendered or raw, not Sublime's Paste and Indent.

use std::ops::Range;

use gpui::{actions, AnyElement, Context, InteractiveElement as _, IntoElement as _, KeyBinding, ParentElement as _, Styled as _, Window, div, point, px};
use gpui_component::input::{DeleteToBeginningOfLine, DeleteToEndOfLine, Redo};

use crate::workbench::Workbench;

actions!(
    sublime,
    [
        FindNext,
        FindPrevious,
        UseSelectionForFind,
        FindUnder,
        FindUnderPrevious,
        FindUnderExpand,
        ExpandToLine,
        ExpandToLineUp,
        DeleteLine,
        InsertLineAfter,
        InsertLineBefore,
        SwapLineUp,
        SwapLineDown,
        DuplicateLine,
        JoinLines,
        ToggleComment,
        ToggleBlockComment,
        UpperCase,
        LowerCase,
        JumpToBracket,
        ExpandToBrackets,
        Transpose,
        ShowAtCenter,
        ScrollLineUp,
        ScrollLineDown,
        SortLines,
        SortLinesCaseSensitive,
        SubwordLeft,
        SubwordRight,
        SelectSubwordLeft,
        SelectSubwordRight,
        DeleteSubwordLeft,
        DeleteSubwordRight,
        Fold,
        Unfold,
        UnfoldAll,
    ]
);

/// The key context the editor's text stands in (`wrap`).
pub(crate) const CONTEXT: &str = "FileEditor > Input";

/// Sublime Text's keys that do not change. Those of its commands used
/// most are Settings, Shortcuts' (`shortcuts::EDITOR`), bound there.
pub fn bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    #[cfg(target_os = "macos")]
    let keys = vec![
        KeyBinding::new("cmd-g", FindNext, c),
        KeyBinding::new("cmd-shift-g", FindPrevious, c),
        KeyBinding::new("cmd-alt-g", FindUnder, c),
        KeyBinding::new("cmd-alt-shift-g", FindUnderPrevious, c),
        KeyBinding::new("cmd-alt-l", ExpandToLineUp, c),
        KeyBinding::new("ctrl-shift-m", ExpandToBrackets, c),
        KeyBinding::new("ctrl-t", Transpose, c),
        KeyBinding::new("ctrl-l", ShowAtCenter, c),
        KeyBinding::new("cmd-k cmd-c", ShowAtCenter, c),
        KeyBinding::new("ctrl-alt-up", ScrollLineUp, c),
        KeyBinding::new("ctrl-alt-down", ScrollLineDown, c),
        KeyBinding::new("ctrl-f5", SortLinesCaseSensitive, c),
        KeyBinding::new("ctrl-left", SubwordLeft, c),
        KeyBinding::new("ctrl-right", SubwordRight, c),
        KeyBinding::new("ctrl-alt-left", SubwordLeft, c),
        KeyBinding::new("ctrl-alt-right", SubwordRight, c),
        KeyBinding::new("ctrl-shift-left", SelectSubwordLeft, c),
        KeyBinding::new("ctrl-shift-right", SelectSubwordRight, c),
        KeyBinding::new("ctrl-alt-shift-left", SelectSubwordLeft, c),
        KeyBinding::new("ctrl-alt-shift-right", SelectSubwordRight, c),
        KeyBinding::new("ctrl-backspace", DeleteSubwordLeft, c),
        KeyBinding::new("ctrl-delete", DeleteSubwordRight, c),
        KeyBinding::new("cmd-k cmd-0", UnfoldAll, c),
        KeyBinding::new("cmd-k cmd-j", UnfoldAll, c),
        KeyBinding::new("cmd-k cmd-k", DeleteToEndOfLine, c),
        KeyBinding::new("ctrl-k", DeleteToEndOfLine, c),
        KeyBinding::new("cmd-k cmd-backspace", DeleteToBeginningOfLine, c),
        KeyBinding::new("cmd-y", Redo, c),
    ];
    // Sublime Text's own keys on Windows and Linux, where the two maps
    // agree on every one of these.
    #[cfg(not(target_os = "macos"))]
    let keys = vec![
        KeyBinding::new("f3", FindNext, c),
        KeyBinding::new("shift-f3", FindPrevious, c),
        KeyBinding::new("ctrl-f3", FindUnder, c),
        KeyBinding::new("ctrl-shift-f3", FindUnderPrevious, c),
        KeyBinding::new("alt-l", ExpandToLineUp, c),
        KeyBinding::new("ctrl-shift-m", ExpandToBrackets, c),
        KeyBinding::new("ctrl-t", Transpose, c),
        KeyBinding::new("ctrl-k ctrl-c", ShowAtCenter, c),
        KeyBinding::new("ctrl-up", ScrollLineUp, c),
        KeyBinding::new("ctrl-down", ScrollLineDown, c),
        KeyBinding::new("ctrl-f9", SortLinesCaseSensitive, c),
        KeyBinding::new("alt-left", SubwordLeft, c),
        KeyBinding::new("alt-right", SubwordRight, c),
        KeyBinding::new("alt-shift-left", SelectSubwordLeft, c),
        KeyBinding::new("alt-shift-right", SelectSubwordRight, c),
        KeyBinding::new("ctrl-k ctrl-0", UnfoldAll, c),
        KeyBinding::new("ctrl-k ctrl-j", UnfoldAll, c),
        KeyBinding::new("ctrl-k ctrl-k", DeleteToEndOfLine, c),
        KeyBinding::new("ctrl-shift-delete", DeleteToEndOfLine, c),
        KeyBinding::new("ctrl-k ctrl-backspace", DeleteToBeginningOfLine, c),
        KeyBinding::new("ctrl-shift-backspace", DeleteToBeginningOfLine, c),
    ];
    keys
}

/// A change to the text: these bytes become these, and then this is
/// selected, in the new text.
#[derive(Debug, PartialEq)]
pub(crate) struct Edit {
    range: Range<usize>,
    with: String,
    select: Range<usize>,
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

/// The whole lines the selection touches, from the first's start to the
/// last's end, its line break not taken: a selection that ends at a
/// line's start does not take that line.
fn lines_of(text: &str, sel: &Range<usize>) -> (usize, usize) {
    let end = if sel.end > sel.start && sel.end == line_start(text, sel.end) { sel.end - 1 } else { sel.end };
    (line_start(text, sel.start), line_end(text, end.max(sel.start)))
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The word the caret is in or next to; empty where there is none.
fn word_at(text: &str, at: usize) -> Range<usize> {
    let start = text[..at].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(at, |(i, _)| i);
    let end = text[at..].char_indices().find(|(_, c)| !is_word(*c)).map_or(text.len(), |(i, _)| at + i);
    start..end
}

/// What is selected, or the word the caret is in: what Sublime Text's
/// commands of a word take with nothing selected.
fn selection_or_word(text: &str, sel: &Range<usize>) -> Range<usize> {
    if sel.is_empty() { word_at(text, sel.start) } else { sel.clone() }
}

/// ⌃⇧K: the lines gone, the caret where it was across the line under.
pub(crate) fn delete_line(text: &str, sel: Range<usize>) -> Option<Edit> {
    if text.is_empty() {
        return None;
    }
    let (a, b) = lines_of(text, &sel);
    let column = sel.start - a;
    let range = if b < text.len() { a..b + 1 } else if a > 0 { a - 1..b } else { a..b };
    let rest = format!("{}{}", &text[..range.start], &text[range.end..]);
    let at = line_start(&rest, range.start.min(rest.len()));
    let caret = (at + column).min(line_end(&rest, at));
    Some(Edit { range, with: String::new(), select: caret..caret })
}

/// ⌘↩ and ⇧⌘↩: a new line under the caret's, or over it, at its indent.
pub(crate) fn insert_line(text: &str, sel: Range<usize>, after: bool) -> Edit {
    if after {
        let at = line_end(text, sel.end);
        let indent = indent_of(&text[line_start(text, sel.end)..at]).to_string();
        let caret = at + 1 + indent.len();
        Edit { range: at..at, with: format!("\n{indent}"), select: caret..caret }
    } else {
        let at = line_start(text, sel.start);
        let indent = indent_of(&text[at..line_end(text, at)]).to_string();
        let caret = at + indent.len();
        Edit { range: at..at, with: format!("{indent}\n"), select: caret..caret }
    }
}

/// ⌃⌘↑ and ⌃⌘↓: the lines the selection touches change places with the
/// line over them, or under them, and the selection goes with them.
pub(crate) fn swap_lines(text: &str, sel: Range<usize>, up: bool) -> Option<Edit> {
    let (a, b) = lines_of(text, &sel);
    let own = &text[a..b];
    if up {
        if a == 0 {
            return None;
        }
        let p = line_start(text, a - 1);
        let by = a - p;
        Some(Edit { range: p..b, with: format!("{own}\n{}", &text[p..a - 1]), select: sel.start - by..sel.end - by })
    } else {
        if b >= text.len() {
            return None;
        }
        let n = line_end(text, b + 1);
        let by = n - b;
        Some(Edit { range: a..n, with: format!("{}\n{own}", &text[b + 1..n]), select: sel.start + by..sel.end + by })
    }
}

/// ⇧⌘D: what is selected once more after it, or with nothing selected
/// the line, the caret going down onto the copy.
pub(crate) fn duplicate(text: &str, sel: Range<usize>) -> Edit {
    if !sel.is_empty() {
        let copy = text[sel.clone()].to_string();
        return Edit { range: sel.end..sel.end, with: copy, select: sel };
    }
    let (a, b) = lines_of(text, &sel);
    let by = b - a + 1;
    Edit { range: b..b, with: format!("\n{}", &text[a..b]), select: sel.start + by..sel.start + by }
}

/// ⇧⌘J: the lines selected made one, or the caret's line and the one
/// under it, each line's indent let go and a space between.
pub(crate) fn join_lines(text: &str, sel: Range<usize>) -> Option<Edit> {
    let (a, mut b) = lines_of(text, &sel);
    if !text[a..b].contains('\n') {
        if b >= text.len() {
            return None;
        }
        b = line_end(text, b + 1);
    }
    let mut lines = text[a..b].split('\n');
    let mut joined = lines.next().unwrap_or("").trim_end().to_string();
    let first = joined.len();
    for line in lines {
        let line = line.trim();
        if !line.is_empty() {
            if !joined.is_empty() {
                joined.push(' ');
            }
            joined.push_str(line);
        }
    }
    let select = if sel.is_empty() { a + first..a + first } else { a..a + joined.len() };
    Some(Edit { range: a..b, with: joined, select })
}

/// How a file's language comments a line, and a stretch, by its name.
pub(crate) fn comment_marks(path: &std::path::Path) -> (Option<&'static str>, Option<(&'static str, &'static str)>) {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "java" | "kt" | "kts" | "swift" | "go" | "scala" | "dart" | "zig" | "php" | "scss" | "less" | "jsonc" | "proto" | "gradle" => (Some("//"), Some(("/*", "*/"))),
        "css" => (None, Some(("/*", "*/"))),
        "py" | "r" | "rb" | "sh" | "bash" | "zsh" | "fish" | "pl" | "yaml" | "yml" | "toml" | "conf" | "ini" | "cfg" | "nix" | "jl" | "ex" | "exs" | "ps1" | "mk" | "cmake" | "dockerfile" | "gitignore" | "env" | "tf" => (Some("#"), None),
        "sql" | "lua" | "hs" | "elm" | "ada" => (Some("--"), None),
        "tex" | "sty" | "cls" | "bib" | "m" | "erl" | "typ" => if ext == "typ" { (Some("//"), Some(("/*", "*/"))) } else { (Some("%"), None) },
        "lisp" | "clj" | "cljs" | "el" | "scm" | "rkt" => (Some(";"), None),
        "vim" => (Some("\""), None),
        "html" | "htm" | "xml" | "svg" | "md" | "markdown" | "qmd" | "rmd" | "vue" | "svelte" => (None, Some(("<!--", "-->"))),
        _ if name == "makefile" || name == "dockerfile" => (Some("#"), None),
        _ => (None, None),
    }
}

/// ⌘/: each line the selection touches commented, or let go of its mark
/// when every one with words on it has it. A language with no line
/// comment comments the stretch (`block_comment`).
pub(crate) fn toggle_comment(text: &str, sel: Range<usize>, mark: &str) -> Option<Edit> {
    let (a, b) = lines_of(text, &sel);
    let block = &text[a..b];
    let filled: Vec<&str> = block.split('\n').filter(|l| !l.trim().is_empty()).collect();
    if filled.is_empty() {
        return None;
    }
    let all = filled.iter().all(|l| l.trim_start().starts_with(mark));
    let col = filled.iter().map(|l| indent_of(l).len()).min().unwrap_or(0);
    let mut caret_shift: isize = 0;
    let caret_line = line_start(text, sel.start);
    let mut at = a;
    let lines: Vec<String> = block
        .split('\n')
        .map(|l| {
            let here = at;
            at += l.len() + 1;
            let out = if l.trim().is_empty() {
                l.to_string()
            } else if all {
                let ind = indent_of(l).len();
                let rest = &l[ind + mark.len()..];
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                let out = format!("{}{rest}", &l[..ind]);
                if here == caret_line {
                    caret_shift = out.len() as isize - l.len() as isize;
                }
                out
            } else {
                if here == caret_line {
                    caret_shift = mark.len() as isize + 1;
                }
                format!("{}{mark} {}", &l[..col], &l[col..])
            };
            out
        })
        .collect();
    let with = lines.join("\n");
    let select = if sel.is_empty() {
        let c = (sel.start as isize + caret_shift).max(caret_line as isize) as usize;
        c..c
    } else {
        a..a + with.len()
    };
    Some(Edit { range: a..b, with, select })
}

/// ⌥⌘/: what is selected, or the caret's line, put inside the language's
/// marks for a stretch, or taken out of them when it already is.
pub(crate) fn block_comment(text: &str, sel: Range<usize>, (open, close): (&str, &str)) -> Edit {
    let range = if sel.is_empty() {
        let (a, b) = lines_of(text, &sel);
        let line = &text[a..b];
        let ind = indent_of(line).len();
        a + ind..a + line.trim_end().len()
    } else {
        sel.clone()
    };
    let inner = &text[range.clone()];
    if let Some(bare) = inner.strip_prefix(open).and_then(|s| s.strip_suffix(close)) {
        let bare = bare.strip_prefix(' ').unwrap_or(bare);
        let bare = bare.strip_suffix(' ').unwrap_or(bare).to_string();
        let len = bare.len();
        return Edit { range: range.clone(), with: bare, select: range.start..range.start + len };
    }
    let with = format!("{open} {inner} {close}");
    let len = with.len();
    Edit { range: range.clone(), with, select: range.start..range.start + len }
}

/// ⌘K ⌘U and ⌘K ⌘L: what is selected, or the caret's word, in capitals or
/// in small letters, and selected.
pub(crate) fn change_case(text: &str, sel: Range<usize>, upper: bool) -> Option<Edit> {
    let range = selection_or_word(text, &sel);
    if range.is_empty() {
        return None;
    }
    let with = if upper { text[range.clone()].to_uppercase() } else { text[range.clone()].to_lowercase() };
    let len = with.len();
    Some(Edit { range: range.clone(), with, select: range.start..range.start + len })
}

const OPENS: &str = "([{";
const CLOSES: &str = ")]}";

/// The pair of brackets around `at`, nearest first: the open's offset and
/// the close's.
fn enclosing(text: &str, from: usize, to: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut depth = [0i32; 3];
    let mut open = None;
    for i in (0..from).rev() {
        let c = bytes[i] as char;
        if let Some(k) = CLOSES.find(c) {
            depth[k] += 1;
        } else if let Some(k) = OPENS.find(c) {
            if depth[k] == 0 {
                open = Some((i, k));
                break;
            }
            depth[k] -= 1;
        }
    }
    let (o, k) = open?;
    let mut d = 0i32;
    for (i, &b) in bytes.iter().enumerate().skip(to) {
        let c = b as char;
        if c == OPENS.as_bytes()[k] as char {
            d += 1;
        } else if c == CLOSES.as_bytes()[k] as char {
            if d == 0 {
                return Some((o, i));
            }
            d -= 1;
        }
    }
    None
}

/// The bracket matching the one at `at` (`at` or just before it).
fn matching(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    for i in [at, at.wrapping_sub(1)] {
        let Some(&b) = bytes.get(i) else { continue };
        let c = b as char;
        if let Some(k) = OPENS.find(c) {
            return enclosing(text, i + 1, i + 1).filter(|(o, _)| *o == i).map(|(_, close)| close).or_else(|| {
                let (open, close) = (OPENS.as_bytes()[k], CLOSES.as_bytes()[k]);
                let mut d = 0;
                for (j, &x) in bytes.iter().enumerate().skip(i + 1) {
                    if x == open {
                        d += 1;
                    } else if x == close {
                        if d == 0 {
                            return Some(j);
                        }
                        d -= 1;
                    }
                }
                None
            });
        }
        if CLOSES.find(c).is_some() {
            return enclosing(text, i, i).filter(|(_, close)| *close == i).map(|(o, _)| o);
        }
    }
    None
}

/// ⌃M: on a bracket, the caret to its pair; inside brackets, to the one
/// that closes them.
pub(crate) fn jump_to_bracket(text: &str, at: usize) -> Option<usize> {
    matching(text, at).or_else(|| enclosing(text, at, at).map(|(_, close)| close))
}

/// ⌃⇧M: what the nearest brackets around the selection hold, then with
/// each press the next brackets out.
pub(crate) fn expand_to_brackets(text: &str, sel: Range<usize>) -> Option<Range<usize>> {
    let (mut from, mut to) = (sel.start, sel.end);
    loop {
        let (o, c) = enclosing(text, from, to)?;
        let inside = o + 1..c;
        if inside != sel && inside.start <= sel.start && inside.end >= sel.end {
            return Some(inside);
        }
        if (o..c + 1) != sel && o <= sel.start && c + 1 >= sel.end && inside == sel {
            return Some(o..c + 1);
        }
        (from, to) = (o, c + 1);
    }
}

/// ⌃T: the characters either side of the caret change places, the two
/// before it at a line's end.
pub(crate) fn transpose(text: &str, at: usize) -> Option<Edit> {
    let before: Vec<(usize, char)> = text[..at].char_indices().rev().take(2).collect();
    let after = text[at..].chars().next().filter(|c| *c != '\n');
    let (start, a, b) = match (before.first(), after) {
        (Some(&(i, prev)), Some(next)) if prev != '\n' => (i, prev, next),
        (Some(&(_, last)), None) => match before.get(1) {
            Some(&(i, first)) if first != '\n' && last != '\n' => (i, first, last),
            _ => return None,
        },
        _ => return None,
    };
    let end = start + a.len_utf8() + b.len_utf8();
    Some(Edit { range: start..end, with: format!("{b}{a}"), select: end..end })
}

/// F5 and ⌃F5: the lines selected in order, or every line of the file
/// with nothing selected.
pub(crate) fn sort_lines(text: &str, sel: Range<usize>, case_sensitive: bool) -> Option<Edit> {
    let (a, b) = if sel.is_empty() { (0, text.trim_end_matches('\n').len()) } else { lines_of(text, &sel) };
    let mut lines: Vec<&str> = text[a..b].split('\n').collect();
    if lines.len() < 2 {
        return None;
    }
    if case_sensitive {
        lines.sort();
    } else {
        lines.sort_by_key(|l| l.to_lowercase());
    }
    let with = lines.join("\n");
    let len = with.len();
    Some(Edit { range: a..b, with, select: if sel.is_empty() { sel } else { a..a + len } })
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Upper,
    Lower,
    Digit,
    Other,
}

fn kind(c: char) -> Kind {
    if c.is_uppercase() {
        Kind::Upper
    } else if c.is_alphabetic() {
        Kind::Lower
    } else if c.is_numeric() {
        Kind::Digit
    } else {
        Kind::Other
    }
}

/// Where the part of a word after `at` ends, as Sublime Text's subwords
/// go: `fooBarBaz` stops after `foo` and `Bar`, `HTTPServer` after
/// `HTTP`, a run of digits on its own, and what is not a letter or a
/// digit (an underscore, a space, a sign) passed over first.
pub(crate) fn subword_end(text: &str, at: usize) -> usize {
    let chars: Vec<(usize, char)> = text[at..].char_indices().map(|(i, c)| (at + i, c)).collect();
    let mut i = 0;
    while i < chars.len() && kind(chars[i].1) == Kind::Other {
        i += 1;
    }
    if i >= chars.len() {
        return text.len();
    }
    let first = kind(chars[i].1);
    i += 1;
    match first {
        Kind::Upper => {
            if i < chars.len() && kind(chars[i].1) == Kind::Lower {
                while i < chars.len() && kind(chars[i].1) == Kind::Lower {
                    i += 1;
                }
            } else {
                while i < chars.len() && kind(chars[i].1) == Kind::Upper {
                    if chars.get(i + 1).is_some_and(|(_, c)| kind(*c) == Kind::Lower) {
                        break;
                    }
                    i += 1;
                }
            }
        }
        k => {
            while i < chars.len() && kind(chars[i].1) == k {
                i += 1;
            }
        }
    }
    chars.get(i).map_or(text.len(), |(o, _)| *o)
}

/// Where the part of a word before `at` begins: `subword_end` the other
/// way.
pub(crate) fn subword_start(text: &str, at: usize) -> usize {
    let chars: Vec<(usize, char)> = text[..at].char_indices().collect();
    let mut i = chars.len();
    while i > 0 && kind(chars[i - 1].1) == Kind::Other {
        i -= 1;
    }
    if i == 0 {
        return 0;
    }
    let last = kind(chars[i - 1].1);
    i -= 1;
    match last {
        Kind::Lower => {
            while i > 0 && kind(chars[i - 1].1) == Kind::Lower {
                i -= 1;
            }
            if i > 0 && kind(chars[i - 1].1) == Kind::Upper {
                i -= 1;
            }
        }
        k => {
            while i > 0 && kind(chars[i - 1].1) == k {
                i -= 1;
            }
        }
    }
    chars[i].0
}

/// ⌘L: the selection to whole lines, its line break with it, and at each
/// press one line more; ⌘⌥L the same upward.
pub(crate) fn expand_to_line(text: &str, sel: Range<usize>, up: bool) -> Range<usize> {
    let (a, b) = lines_of(text, &sel);
    let whole = a..(b + 1).min(text.len());
    if up {
        if sel == whole && a > 0 {
            return line_start(text, a - 1)..whole.end;
        }
        return whole;
    }
    if sel == whole && whole.end < text.len() {
        return a..(line_end(text, whole.end) + 1).min(text.len());
    }
    whole
}

impl Workbench {
    /// The editor's element, set in the context the keys are bound
    /// under, with what each does.
    pub(crate) fn sublime_wrap(&self, editor: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        div()
            .key_context("FileEditor")
            .size_full()
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.sublime_find(true, false, cx)))
            .on_action(cx.listener(|this, _: &FindPrevious, _, cx| this.sublime_find(false, false, cx)))
            .on_action(cx.listener(|this, _: &FindUnder, _, cx| this.sublime_find(true, true, cx)))
            .on_action(cx.listener(|this, _: &FindUnderPrevious, _, cx| this.sublime_find(false, true, cx)))
            .on_action(cx.listener(|this, _: &UseSelectionForFind, _, cx| {
                this.sublime_slurp(cx);
            }))
            .on_action(cx.listener(|this, _: &FindUnderExpand, _, cx| {
                // One selection here: the word first, then each press the
                // next place it is, where Sublime Text adds a cursor.
                if this.sublime_sel(cx).is_some_and(|(_, sel)| sel.is_empty()) {
                    this.sublime_select(|text, sel| Some(word_at(text, sel.start)), cx);
                    this.sublime_slurp(cx);
                } else {
                    this.sublime_find(true, true, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ExpandToLine, _, cx| this.sublime_select(|text, sel| Some(expand_to_line(text, sel, false)), cx)))
            .on_action(cx.listener(|this, _: &ExpandToLineUp, _, cx| this.sublime_select(|text, sel| Some(expand_to_line(text, sel, true)), cx)))
            .on_action(cx.listener(|this, _: &DeleteLine, window, cx| this.sublime_edit(delete_line, window, cx)))
            .on_action(cx.listener(|this, _: &InsertLineAfter, window, cx| this.sublime_edit(|t, s| Some(insert_line(t, s, true)), window, cx)))
            .on_action(cx.listener(|this, _: &InsertLineBefore, window, cx| this.sublime_edit(|t, s| Some(insert_line(t, s, false)), window, cx)))
            .on_action(cx.listener(|this, _: &SwapLineUp, window, cx| this.sublime_edit(|t, s| swap_lines(t, s, true), window, cx)))
            .on_action(cx.listener(|this, _: &SwapLineDown, window, cx| this.sublime_edit(|t, s| swap_lines(t, s, false), window, cx)))
            .on_action(cx.listener(|this, _: &DuplicateLine, window, cx| this.sublime_edit(|t, s| Some(duplicate(t, s)), window, cx)))
            .on_action(cx.listener(|this, _: &JoinLines, window, cx| this.sublime_edit(join_lines, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleComment, window, cx| {
                let Some(path) = this.file_editor.as_ref().map(|ed| ed.path.clone()) else { return };
                match comment_marks(&path) {
                    (Some(mark), _) => this.sublime_edit(|t, s| toggle_comment(t, s, mark), window, cx),
                    (None, Some(marks)) => this.sublime_edit(|t, s| Some(block_comment(t, s, marks)), window, cx),
                    (None, None) => {}
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleBlockComment, window, cx| {
                let Some(path) = this.file_editor.as_ref().map(|ed| ed.path.clone()) else { return };
                if let (_, Some(marks)) = comment_marks(&path) {
                    this.sublime_edit(|t, s| Some(block_comment(t, s, marks)), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &UpperCase, window, cx| this.sublime_edit(|t, s| change_case(t, s, true), window, cx)))
            .on_action(cx.listener(|this, _: &LowerCase, window, cx| this.sublime_edit(|t, s| change_case(t, s, false), window, cx)))
            .on_action(cx.listener(|this, _: &JumpToBracket, _, cx| this.sublime_select(|t, s| jump_to_bracket(t, s.start).map(|at| at..at), cx)))
            .on_action(cx.listener(|this, _: &ExpandToBrackets, _, cx| this.sublime_select(expand_to_brackets, cx)))
            .on_action(cx.listener(|this, _: &Transpose, window, cx| this.sublime_edit(|t, s| s.is_empty().then(|| transpose(t, s.start)).flatten(), window, cx)))
            .on_action(cx.listener(|this, _: &SortLines, window, cx| this.sublime_edit(|t, s| sort_lines(t, s, false), window, cx)))
            .on_action(cx.listener(|this, _: &SortLinesCaseSensitive, window, cx| this.sublime_edit(|t, s| sort_lines(t, s, true), window, cx)))
            .on_action(cx.listener(|this, _: &SubwordLeft, _, cx| this.sublime_move(false, false, cx)))
            .on_action(cx.listener(|this, _: &SubwordRight, _, cx| this.sublime_move(true, false, cx)))
            .on_action(cx.listener(|this, _: &SelectSubwordLeft, _, cx| this.sublime_move(false, true, cx)))
            .on_action(cx.listener(|this, _: &SelectSubwordRight, _, cx| this.sublime_move(true, true, cx)))
            .on_action(cx.listener(|this, _: &DeleteSubwordLeft, window, cx| this.sublime_edit(|t, s| (s.is_empty() && s.start > 0).then(|| { let a = subword_start(t, s.start); Edit { range: a..s.start, with: String::new(), select: a..a } }), window, cx)))
            .on_action(cx.listener(|this, _: &DeleteSubwordRight, window, cx| this.sublime_edit(|t, s| (s.is_empty() && s.start < t.len()).then(|| Edit { range: s.start..subword_end(t, s.start), with: String::new(), select: s.start..s.start }), window, cx)))
            .on_action(cx.listener(|this, _: &ShowAtCenter, _, cx| this.sublime_scroll(None, cx)))
            .on_action(cx.listener(|this, _: &ScrollLineUp, _, cx| this.sublime_scroll(Some(1.), cx)))
            .on_action(cx.listener(|this, _: &ScrollLineDown, _, cx| this.sublime_scroll(Some(-1.), cx)))
            .on_action(cx.listener(|this, _: &Fold, _, cx| this.sublime_fold(Some(true), cx)))
            .on_action(cx.listener(|this, _: &Unfold, _, cx| this.sublime_fold(Some(false), cx)))
            .on_action(cx.listener(|this, _: &UnfoldAll, _, cx| this.sublime_fold(None, cx)))
            .child(editor)
            .into_any_element()
    }

    /// The editor's text and selection.
    fn sublime_sel(&self, cx: &gpui::App) -> Option<(String, Range<usize>)> {
        let state = self.file_editor.as_ref()?.state.read(cx);
        Some((state.value().to_string(), state.selected_range()))
    }

    /// A change worked out from the text and the selection, made as one
    /// step of undo, in an editor that can be written in.
    fn sublime_edit(&mut self, make: impl FnOnce(&str, Range<usize>) -> Option<Edit>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.file_editor.as_ref() else { return };
        let state = ed.state.clone();
        let (text, sel) = {
            let s = state.read(cx);
            if !s.is_editable() {
                return;
            }
            (s.value().to_string(), s.selected_range())
        };
        let Some(edit) = make(&text, sel) else { return };
        state.update(cx, |s, cx| {
            s.replace_bytes(edit.range, &edit.with, edit.select.end, window, cx);
            s.set_selected_range(edit.select, cx);
        });
    }

    /// A new selection worked out from the text and the selection.
    fn sublime_select(&mut self, make: impl FnOnce(&str, Range<usize>) -> Option<Range<usize>>, cx: &mut Context<Self>) {
        let Some(state) = self.file_editor.as_ref().map(|ed| ed.state.clone()) else { return };
        let (text, sel) = {
            let s = state.read(cx);
            (s.value().to_string(), s.selected_range())
        };
        if let Some(range) = make(&text, sel) {
            state.update(cx, |s, cx| s.set_selected_range(range, cx));
        }
    }

    /// ⌘G and ⇧⌘G go by the find's words; ⌥⌘G and ⇧⌥⌘G (`under`) first
    /// make the words what is selected, or the caret's word.
    fn sublime_find(&mut self, forward: bool, under: bool, cx: &mut Context<Self>) {
        if under {
            self.sublime_slurp(cx);
        }
        if let Some(state) = self.file_editor.as_ref().map(|ed| ed.state.clone()) {
            state.update(cx, |s, cx| {
                s.find_from_selection(forward, cx);
            });
        }
    }

    /// ⌘E: what is selected, or the caret's word, is what the find looks
    /// for, in the card when it opens.
    fn sublime_slurp(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.file_editor.as_ref().map(|ed| ed.state.clone()) else { return };
        state.update(cx, |s, cx| {
            let text = s.value().to_string();
            let range = selection_or_word(&text, &s.selected_range());
            if !range.is_empty() {
                let case_insensitive = s.search_session().case_insensitive;
                s.set_search_query(text[range].to_string(), case_insensitive, cx);
            }
        });
    }

    /// The caret to the next part of a word, or the selection's moving
    /// end there.
    fn sublime_move(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
        let Some(state) = self.file_editor.as_ref().map(|ed| ed.state.clone()) else { return };
        state.update(cx, |s, cx| {
            let text = s.value().to_string();
            let head = s.cursor();
            let to = if forward { subword_end(&text, head) } else { subword_start(&text, head) };
            if extend {
                s.extend_selection_to(to, cx);
            } else {
                s.set_selected_range(to..to, cx);
            }
        });
    }

    /// ⌃L: the caret's line to the middle of the view; with `by`, the
    /// view a line up (1) or down (-1), the caret where it is.
    fn sublime_scroll(&mut self, by: Option<f32>, cx: &mut Context<Self>) {
        let Some(state) = self.file_editor.as_ref().map(|ed| ed.state.clone()) else { return };
        state.update(cx, |s, cx| {
            let at = s.scroll_offset();
            let Some(line) = s.line_height() else { return };
            let y = match by {
                Some(by) => (at.y + line * by).min(px(0.)),
                None => {
                    let row = s.cursor_position().line as usize;
                    let Some((top, view)) = s.line_top(row) else { return };
                    (view / 2. - line / 2. - top).min(px(0.))
                }
            };
            s.set_scroll_offset(point(at.x, y), cx);
        });
    }

    /// ⌥⌘[ and ⌥⌘]: the section the caret is in folded or opened; ⌘K ⌘0
    /// every section open.
    fn sublime_fold(&mut self, fold: Option<bool>, cx: &mut Context<Self>) {
        let Some(state) = self.file_editor.as_ref().map(|ed| ed.state.clone()) else { return };
        state.update(cx, |s, cx| match fold {
            Some(fold) => {
                s.fold_at_caret(fold, cx);
            }
            None => s.unfold_all(cx),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, e: &Edit) -> String {
        format!("{}{}{}", &text[..e.range.start], e.with, &text[e.range.end..])
    }

    #[test]
    fn lines_go_and_come_as_sublime_text_has_them() {
        let t = "one\ntwo\nthree";
        let e = delete_line(t, 5..5).unwrap();
        assert_eq!(apply(t, &e), "one\nthree");
        assert_eq!(e.select, 5..5, "the caret keeps its place across");
        let e = delete_line(t, 10..10).unwrap();
        assert_eq!(apply(t, &e), "one\ntwo", "the last line takes the break before it");
        let e = insert_line("  a\nb", 1..1, true);
        assert_eq!(apply("  a\nb", &e), "  a\n  \nb");
        assert_eq!(e.select, 6..6);
        let e = insert_line("  a\nb", 2..2, false);
        assert_eq!(apply("  a\nb", &e), "  \n  a\nb");
        let e = swap_lines(t, 5..5, true).unwrap();
        assert_eq!(apply(t, &e), "two\none\nthree");
        assert_eq!(e.select, 1..1);
        let e = swap_lines(t, 5..5, false).unwrap();
        assert_eq!(apply(t, &e), "one\nthree\ntwo");
        assert_eq!(&apply(t, &e)[e.select.start..e.select.start + 1], "w");
        assert!(swap_lines(t, 0..0, true).is_none() && swap_lines(t, 9..9, false).is_none());
        let e = duplicate(t, 5..5);
        assert_eq!(apply(t, &e), "one\ntwo\ntwo\nthree");
        assert_eq!(e.select, 9..9, "onto the copy");
        let e = join_lines("a  \n   b\nc", 0..0).unwrap();
        assert_eq!(apply("a  \n   b\nc", &e), "a b\nc");
        let e = sort_lines("b\nA\nc", 0..0, false).unwrap();
        assert_eq!(apply("b\nA\nc", &e), "A\nb\nc");
    }

    #[test]
    fn a_comment_goes_on_and_comes_off() {
        let t = "fn a() {\n    x();\n    y();\n}";
        let e = toggle_comment(t, 13..20, "//").unwrap();
        let on = apply(t, &e);
        assert_eq!(on, "fn a() {\n    // x();\n    // y();\n}");
        let e = toggle_comment(&on, 13..25, "//").unwrap();
        assert_eq!(apply(&on, &e), t);
        let e = block_comment("<p>hi</p>", 0..9, ("<!--", "-->"));
        let on = apply("<p>hi</p>", &e);
        assert_eq!(on, "<!-- <p>hi</p> -->");
        let e = block_comment(&on, 0..on.len(), ("<!--", "-->"));
        assert_eq!(apply(&on, &e), "<p>hi</p>");
        assert_eq!(comment_marks(std::path::Path::new("a.rs")).0, Some("//"));
        assert_eq!(comment_marks(std::path::Path::new("a.qmd")).1, Some(("<!--", "-->")));
    }

    #[test]
    fn words_brackets_and_lines() {
        assert_eq!(subword_end("fooBarBaz", 0), 3);
        assert_eq!(subword_end("fooBarBaz", 3), 6);
        assert_eq!(subword_end("HTTPServer", 0), 4);
        assert_eq!(subword_end("snake_case", 5), 10);
        assert_eq!(subword_start("fooBarBaz", 9), 6);
        assert_eq!(subword_start("snake_case", 6), 0);
        assert_eq!(jump_to_bracket("f(a, [b])", 1), Some(8));
        assert_eq!(jump_to_bracket("f(a, [b])", 9), Some(1));
        assert_eq!(jump_to_bracket("f(a, b)", 3), Some(6));
        assert_eq!(expand_to_brackets("f(a [b] c)", 5..5), Some(5..6));
        assert_eq!(expand_to_brackets("f(a [b] c)", 5..6), Some(4..7));
        assert_eq!(expand_to_brackets("f(a [b] c)", 4..7), Some(2..9));
        let e = transpose("ab", 1).unwrap();
        assert_eq!(apply("ab", &e), "ba");
        assert_eq!(apply("ab", &transpose("ab", 2).unwrap()), "ba");
        assert_eq!(expand_to_line("a\nb\nc", 0..0, false), 0..2);
        assert_eq!(expand_to_line("a\nb\nc", 0..2, false), 0..4);
        assert_eq!(change_case("say hi", 5..5, true).map(|e| apply("say hi", &e)), Some("say HI".into()));
        assert_eq!(word_at("one two", 5), 4..7);
    }
}
