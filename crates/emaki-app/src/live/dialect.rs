//! Typst's and LaTeX's markup read for the live preview, as `markdown.rs`
//! reads markdown: what each line is and every span with where its marks
//! stand, so a `.typ` or `.tex` file, and a raw block of either in a
//! Quarto document, is drawn lightly as it reads while it is written in.
//!
//! Light, not typeset: a heading is a heading, strong and emphasized
//! words are, a link is a link and math is math, a list has its bullets
//! and numbers, and what is the document's own code (Typst's `#set` and
//! `#let`, a LaTeX preamble, `\begin` and `\end`, a comment) stands as it
//! is written, quiet, in the mono face (`LineKind::Source`). A call whose
//! content is words (`#text(..)[words]`, `\textsc{words}`) shows its words
//! and hides the call until the caret touches it.
//!
//! Read a line at a time: a span does not cross a line, which in both
//! languages is how markup is written. Every offset found is an ASCII
//! byte's, so a slice never cuts a character.

use std::ops::Range;

use super::lines::ListKind;
use super::markdown::{Analysis, LineInfo, LineKind, ListMark, Span, SpanKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    Typst,
    Latex,
}

impl Dialect {
    /// The language a raw block names, as Quarto writes it: `{=typst}`,
    /// `{=latex}`, `{=tex}`.
    pub fn of_raw(lang: &str) -> Option<Dialect> {
        let l: String = lang.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
        match l.as_str() {
            "{=typst}" => Some(Dialect::Typst),
            "{=latex}" | "{=tex}" => Some(Dialect::Latex),
            _ => None,
        }
    }

    /// The marks ⌘B, ⌘I, ⇧⌘C, ⌘U and ⇧⌘X put around words, and the span
    /// each makes, in this language; none where it has none.
    pub fn wrap(self, kind: &SpanKind) -> Option<(&'static str, &'static str)> {
        match (self, kind) {
            (Dialect::Typst, SpanKind::Strong) => Some(("*", "*")),
            (Dialect::Typst, SpanKind::Em) => Some(("_", "_")),
            (Dialect::Typst, SpanKind::Code) => Some(("`", "`")),
            (Dialect::Typst, SpanKind::Underline) => Some(("#underline[", "]")),
            (Dialect::Typst, SpanKind::Strike) => Some(("#strike[", "]")),
            (Dialect::Latex, SpanKind::Strong) => Some(("\\textbf{", "}")),
            (Dialect::Latex, SpanKind::Em) => Some(("\\emph{", "}")),
            (Dialect::Latex, SpanKind::Code) => Some(("\\texttt{", "}")),
            (Dialect::Latex, SpanKind::Underline) => Some(("\\underline{", "}")),
            (Dialect::Latex, SpanKind::Strike) => Some(("\\sout{", "}")),
            _ => None,
        }
    }
}

/// The whole text read in a language, as `markdown::analyze` reads it.
pub fn analyze(text: &str, dialect: Dialect) -> Analysis {
    let line_starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let (lines, spans) = read(text, dialect);
    let mut a = Analysis { line_starts, lines, spans, ..Default::default() };
    super::markdown::index_spans(&mut a);
    a
}

/// Each line of `text` and its spans, in bytes of `text`.
pub fn read(text: &str, dialect: Dialect) -> (Vec<LineInfo>, Vec<Span>) {
    match dialect {
        Dialect::Typst => Typst::default().read(text),
        Dialect::Latex => Latex::new(text).read(text),
    }
}

fn source() -> LineInfo {
    LineInfo { kind: LineKind::Source, ..Default::default() }
}

fn span(range: Range<usize>, kind: SpanKind, open: Range<usize>, close: Range<usize>) -> Span {
    Span { range, kind, open, close }
}

/// Code among words: hidden whole until the caret touches it, then as
/// it is written, quiet (`SpanKind::Source`). `#h(4pt)` reads as the room
/// it makes.
fn code(range: Range<usize>) -> Span {
    Span { open: range.clone(), close: range.end..range.end, range, kind: SpanKind::Source }
}

/// A line's indent, which means nothing in either language: hidden, as
/// code among words is, so a line inside `#block[..]` stands at the left.
fn indent(lead: usize, at: usize, spans: &mut Vec<Span>) {
    if lead > 0 {
        spans.push(code(at..at + lead));
    }
}

/// A span with no marks: nothing hides, the whole is styled.
fn bare(range: Range<usize>, kind: SpanKind) -> Span {
    Span { open: range.start..range.start, close: range.end..range.end, range, kind }
}

/// Where the bracket opened at `open` closes on the line, counting the
/// pairs inside and passing by strings and escapes.
fn closing(b: &[u8], open: usize, l: u8, r: u8) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    let mut quoted = false;
    while i < b.len() {
        let c = b[i];
        if c == b'\\' {
            i += 2;
            continue;
        }
        if c == b'"' && l != b'[' {
            quoted = !quoted;
        } else if !quoted {
            if c == l {
                depth += 1;
            } else if c == r {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// How much deeper the brackets stand at the line's end than at its
/// start, strings and a comment left out.
fn depth_change(s: &str) -> i32 {
    let b = s.as_bytes();
    let (mut d, mut quoted, mut i) = (0i32, false, 0);
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'"' => quoted = !quoted,
            b'/' if !quoted && b.get(i + 1) == Some(&b'/') => break,
            b'(' | b'[' | b'{' if !quoted => d += 1,
            b')' | b']' | b'}' if !quoted => d -= 1,
            _ => {}
        }
        i += 1;
    }
    d
}

fn ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

/// An address written bare from `at`: to a space or a bracket, the
/// punctuation that ends a sentence left out.
fn address_end(b: &[u8], at: usize) -> usize {
    let mut end = at;
    while end < b.len() && !b[end].is_ascii_whitespace() && !matches!(b[end], b')' | b']' | b'>' | b'"') {
        end += 1;
    }
    while end > at && matches!(b[end - 1], b'.' | b',' | b';' | b':' | b'!' | b'?') {
        end -= 1;
    }
    end
}

fn starts_address(b: &[u8], at: usize) -> bool {
    (b[at..].starts_with(b"https://") || b[at..].starts_with(b"http://")) && (at == 0 || b[at - 1].is_ascii_whitespace() || b[at - 1] == b'(')
}

// -- Typst ------------------------------------------------------------------

/// Statements that make no content: the line and its brackets' lines are
/// code.
const TYPST_CODE: &[&str] = &["set", "show", "let", "import", "include", "if", "else", "for", "while", "return", "break", "continue", "context"];

#[derive(Default)]
struct Typst {
    comment: bool,
    code: i32,
    raw: bool,
    content: usize,
    /// The numbers of the `+` lists open, by their indent.
    numbers: Vec<(usize, u64)>,
}

impl Typst {
    fn read(mut self, text: &str) -> (Vec<LineInfo>, Vec<Span>) {
        let mut lines = Vec::new();
        let mut spans = Vec::new();
        let mut at = 0;
        for s in text.split('\n') {
            lines.push(self.line(s, at, &mut spans));
            at += s.len() + 1;
        }
        (lines, spans)
    }

    fn line(&mut self, s: &str, at: usize, spans: &mut Vec<Span>) -> LineInfo {
        let t = s.trim_start();
        let lead = s.len() - t.len();
        let b = s.as_bytes();
        if self.raw {
            self.raw = !t.starts_with("```");
            return source();
        }
        if self.comment {
            self.comment = !t.contains("*/");
            return source();
        }
        if self.code > 0 {
            self.code = (self.code + depth_change(s)).max(0);
            return source();
        }
        if t.trim_end().is_empty() {
            self.numbers.clear();
            return LineInfo { kind: LineKind::Blank, ..Default::default() };
        }
        if t.starts_with("//") {
            return source();
        }
        if t.starts_with("/*") {
            self.comment = !t.contains("*/");
            return source();
        }
        if t.starts_with("```") {
            self.raw = t.len() <= 3 || !t[3..].contains("```");
            return source();
        }
        if t.starts_with(']') && self.content > 0 {
            self.content -= 1;
            return source();
        }
        if t.starts_with('#') && t.as_bytes().get(1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') {
            let name: String = t[1..].bytes().take_while(|c| ident(*c)).map(char::from).collect();
            if TYPST_CODE.contains(&name.as_str()) {
                self.code = depth_change(s).max(0);
                return source();
            }
            match call_end(b, lead) {
                None => {
                    self.code = depth_change(s).max(0);
                    return source();
                }
                Some(end) => {
                    let rest = s[end..].trim();
                    if rest.is_empty() || rest.starts_with("//") {
                        return source();
                    }
                    if rest == "[" {
                        self.content += 1;
                        return source();
                    }
                }
            }
        }
        // A heading: `=`s and a space.
        let eq = t.bytes().take_while(|c| *c == b'=').count();
        if eq > 0 && (t.len() == eq || t.as_bytes()[eq] == b' ') {
            let end = (lead + eq + 1).min(s.len());
            typst_inline(s, end, at, spans);
            return LineInfo { kind: LineKind::Heading(eq.min(6) as u8), marks: vec![lead..end], ..Default::default() };
        }
        // A list item: `- ` or `+ `, numbered by its place among the `+`s.
        let mark = t.as_bytes().first().copied().filter(|c| matches!(c, b'-' | b'+')).filter(|_| t.len() == 1 || t.as_bytes()[1] == b' ');
        if let Some(m) = mark {
            self.numbers.retain(|(indent, _)| *indent <= lead);
            let kind = if m == b'+' {
                let n = match self.numbers.last_mut() {
                    Some((indent, n)) if *indent == lead => {
                        *n += 1;
                        *n
                    }
                    _ => {
                        self.numbers.push((lead, 1));
                        1
                    }
                };
                ListKind::Ordered(n)
            } else {
                ListKind::Bullet
            };
            let end = (lead + 2).min(s.len());
            let depth = lead / 2;
            typst_inline(s, end, at, spans);
            return LineInfo { list: Some(ListMark { marker: 0..end, kind, depth }), list_depth: depth + 1, ..Default::default() };
        }
        indent(lead, at, spans);
        typst_inline(s, lead, at, spans);
        LineInfo::default()
    }
}

/// Where a call written from `at` (`#name`, its arguments, a method
/// after it) ends on the line, before any content it takes; none when its
/// arguments run on to the lines after.
fn call_end(b: &[u8], at: usize) -> Option<usize> {
    let mut k = at + 1;
    loop {
        while k < b.len() && ident(b[k]) {
            k += 1;
        }
        while k < b.len() && b[k] == b'(' {
            k = closing(b, k, b'(', b')')? + 1;
        }
        if k + 1 < b.len() && b[k] == b'.' && b[k + 1].is_ascii_alphabetic() {
            k += 1;
            continue;
        }
        return Some(k);
    }
}

/// The first string among a call's arguments, between its quotes.
fn first_string(b: &[u8], from: usize, to: usize) -> Option<Range<usize>> {
    let q = (from..to).find(|&i| b[i] == b'"')?;
    let e = (q + 1..to).find(|&i| b[i] == b'"' && b[i - 1] != b'\\')?;
    Some(q + 1..e)
}

/// The span a call's content takes: what the call does to its words.
fn typst_kind(name: &str, args: &str) -> SpanKind {
    match name {
        "strong" => SpanKind::Strong,
        "emph" => SpanKind::Em,
        "underline" => SpanKind::Underline,
        "strike" => SpanKind::Strike,
        "highlight" => SpanKind::Highlight,
        "sub" => SpanKind::Sub,
        "super" => SpanKind::Sup,
        "footnote" => SpanKind::Small,
        "raw" => SpanKind::Code,
        "text" if args.contains("\"bold\"") => SpanKind::Strong,
        "text" if args.contains("\"italic\"") => SpanKind::Em,
        _ => SpanKind::Color(None, None),
    }
}

/// The spans of a line of Typst's markup from `from`.
fn typst_inline(s: &str, from: usize, base: usize, spans: &mut Vec<Span>) {
    let b = s.as_bytes();
    let g = |r: Range<usize>| base + r.start..base + r.end;
    // The closing marks of spans already found, passed when reached.
    let mut closes: Vec<usize> = Vec::new();
    let mut i = from;
    while i < b.len() {
        if closes.contains(&i) {
            i += 1;
            continue;
        }
        match b[i] {
            b'\\' => {
                // A line's break, or a mark written as itself.
                let next = b.get(i + 1).copied();
                if next.is_none_or(|c| c == b' ' || c.is_ascii_punctuation()) {
                    spans.push(span(g(i..i + 1), SpanKind::Quiet, g(i..i + 1), g(i + 1..i + 1)));
                    i += if next.is_some_and(|c| c.is_ascii_punctuation()) { 2 } else { 1 };
                    continue;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                spans.push(bare(g(i..b.len()), SpanKind::Small));
                return;
            }
            b'h' if starts_address(b, i) => {
                let end = address_end(b, i);
                spans.push(bare(g(i..end), SpanKind::Link(s[i..end].to_string())));
                i = end;
                continue;
            }
            b'#' if b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') => {
                let name: String = s[i + 1..].bytes().take_while(|c| ident(*c)).map(char::from).collect();
                let Some(k) = call_end(b, i) else {
                    spans.push(code(g(i..b.len())));
                    return;
                };
                if b.get(k) == Some(&b'[') {
                    match closing(b, k, b'[', b']') {
                        Some(m) => {
                            let kind = if name == "link" {
                                SpanKind::Link(first_string(b, i, k).map(|r| s[r].to_string()).unwrap_or_default())
                            } else {
                                typst_kind(&name, &s[i..k])
                            };
                            // A footnote's words stay in their brackets, so
                            // they read apart from the sentence.
                            if kind == SpanKind::Small {
                                spans.push(span(g(i..m + 1), kind, g(i..k), g(m + 1..m + 1)));
                            } else {
                                spans.push(span(g(i..m + 1), kind, g(i..k + 1), g(m..m + 1)));
                            }
                            closes.push(m);
                        }
                        None => spans.push(code(g(i..k + 1))),
                    }
                    i = k + 1;
                    continue;
                }
                match (name.as_str(), first_string(b, i, k)) {
                    // `#link("…")` alone: the address is its words.
                    ("link", Some(r)) => spans.push(span(g(i..k), SpanKind::Link(s[r.clone()].to_string()), g(i..r.start), g(r.end..k))),
                    _ => spans.push(code(g(i..k))),
                }
                i = k;
                continue;
            }
            b'*' if b.get(i + 1).is_some_and(|c| *c != b' ') => {
                if let Some(j) = (i + 2..b.len()).find(|&j| b[j] == b'*' && b[j - 1] != b' ' && b[j - 1] != b'\\') {
                    spans.push(span(g(i..j + 1), SpanKind::Strong, g(i..i + 1), g(j..j + 1)));
                    closes.push(j);
                }
            }
            b'_' if (i == 0 || !b[i - 1].is_ascii_alphanumeric()) && b.get(i + 1).is_some_and(|c| *c != b' ') => {
                if let Some(j) = (i + 2..b.len()).find(|&j| b[j] == b'_' && b[j - 1] != b' ' && b[j - 1] != b'\\' && b.get(j + 1).is_none_or(|c| !c.is_ascii_alphanumeric())) {
                    spans.push(span(g(i..j + 1), SpanKind::Em, g(i..i + 1), g(j..j + 1)));
                    closes.push(j);
                }
            }
            b'`' => {
                if let Some(j) = (i + 1..b.len()).find(|&j| b[j] == b'`') {
                    spans.push(span(g(i..j + 1), SpanKind::Code, g(i..i + 1), g(j..j + 1)));
                    i = j + 1;
                    continue;
                }
            }
            b'$' => {
                if let Some(j) = (i + 1..b.len()).find(|&j| b[j] == b'$' && b[j - 1] != b'\\') {
                    spans.push(span(g(i..j + 1), SpanKind::Math, g(i..i + 1), g(j..j + 1)));
                    i = j + 1;
                    continue;
                }
            }
            // A label, `<name>`.
            b'<' if b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) => {
                let e = (i + 1..b.len()).find(|&j| !(ident(b[j]) || b[j] == b':' || b[j] == b'.'));
                if let Some(e) = e.filter(|&e| b[e] == b'>') {
                    spans.push(code(g(i..e + 1)));
                    i = e + 1;
                    continue;
                }
            }
            // A reference, `@name`.
            b'@' if (i == 0 || !b[i - 1].is_ascii_alphanumeric()) && b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) => {
                let mut e = i + 1;
                while e < b.len() && (ident(b[e]) || b[e] == b':' || b[e] == b'.') {
                    e += 1;
                }
                while e > i + 1 && b[e - 1] == b'.' {
                    e -= 1;
                }
                spans.push(bare(g(i..e), SpanKind::Footnote));
                i = e;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
}

// -- LaTeX ------------------------------------------------------------------

const SECTIONS: &[&str] = &["part", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph"];
const VERBATIM: &[&str] = &["verbatim", "verbatim*", "lstlisting", "minted", "Verbatim", "comment"];
const MATH: &[&str] = &["equation", "equation*", "align", "align*", "gather", "gather*", "multline", "multline*", "displaymath", "eqnarray", "eqnarray*", "flalign", "flalign*", "alignat", "alignat*", "math"];

struct Latex {
    /// Whether the text is in the document's body: a fragment (a raw
    /// block) has no preamble and is all body.
    body: bool,
    ended: bool,
    verbatim: Option<String>,
    math: Option<String>,
    /// The lists open: whether each is numbered, and its count.
    lists: Vec<(bool, u64)>,
    /// The sectioning command a document's top level is written with.
    top: usize,
}

impl Latex {
    fn new(text: &str) -> Latex {
        let top = SECTIONS.iter().position(|s| text.contains(&format!("\\{s}{{")) || text.contains(&format!("\\{s}*{{"))).unwrap_or(2);
        Latex { body: !text.contains("\\begin{document}"), ended: false, verbatim: None, math: None, lists: Vec::new(), top }
    }

    fn read(mut self, text: &str) -> (Vec<LineInfo>, Vec<Span>) {
        let mut lines = Vec::new();
        let mut spans = Vec::new();
        let mut at = 0;
        for s in text.split('\n') {
            lines.push(self.line(s, at, &mut spans));
            at += s.len() + 1;
        }
        (lines, spans)
    }

    fn line(&mut self, s: &str, at: usize, spans: &mut Vec<Span>) -> LineInfo {
        let t = s.trim_start();
        let lead = s.len() - t.len();
        let tt = t.trim_end();
        if !self.body || self.ended {
            self.body = self.body || tt.starts_with("\\begin{document}");
            return source();
        }
        if tt.starts_with("\\end{document}") {
            self.ended = true;
            return source();
        }
        if let Some(env) = &self.verbatim {
            if tt.starts_with(&format!("\\end{{{env}}}")) {
                self.verbatim = None;
            }
            return source();
        }
        if let Some(env) = self.math.clone() {
            let end = match env.as_str() {
                "\\[" => tt.ends_with("\\]"),
                "$$" => tt.ends_with("$$"),
                _ => tt.starts_with(&format!("\\end{{{env}}}")),
            };
            if end {
                self.math = None;
                if tt.starts_with("\\end{") || tt == "\\]" || tt == "$$" {
                    return source();
                }
            }
            if !tt.is_empty() {
                spans.push(bare(at + lead..at + lead + tt.len(), SpanKind::Math));
            }
            return LineInfo::default();
        }
        if tt.is_empty() {
            return LineInfo { kind: LineKind::Blank, ..Default::default() };
        }
        if tt.starts_with('%') {
            return source();
        }
        if let Some(env) = env_name(tt, "\\begin{") {
            if VERBATIM.contains(&env.as_str()) {
                self.verbatim = Some(env);
            } else if MATH.contains(&env.as_str()) {
                self.math = Some(env);
            } else if env == "itemize" || env == "description" {
                self.lists.push((false, 0));
            } else if env == "enumerate" {
                self.lists.push((true, 0));
            }
            return source();
        }
        if let Some(env) = env_name(tt, "\\end{") {
            if matches!(env.as_str(), "itemize" | "description" | "enumerate") {
                self.lists.pop();
            }
            return source();
        }
        if tt == "\\[" || tt == "$$" {
            self.math = Some(tt.to_string());
            return source();
        }
        if (tt.starts_with("\\[") && tt.ends_with("\\]")) || (tt.len() > 4 && tt.starts_with("$$") && tt.ends_with("$$")) {
            spans.push(bare(at + lead..at + lead + tt.len(), SpanKind::Math));
            return LineInfo::default();
        }
        let b = s.as_bytes();
        // An item: `\item` and its label, numbered in an enumerate.
        if tt.starts_with("\\item") && tt.as_bytes().get(5).is_none_or(|c| !c.is_ascii_alphabetic()) {
            let mut end = lead + 5;
            if b.get(end) == Some(&b'[') {
                end = closing(b, end, b'[', b']').map_or(end, |e| e + 1);
            }
            while end < b.len() && b[end] == b' ' {
                end += 1;
            }
            let depth = self.lists.len().saturating_sub(1);
            let kind = match self.lists.last_mut() {
                Some((true, n)) => {
                    *n += 1;
                    ListKind::Ordered(*n)
                }
                _ => ListKind::Bullet,
            };
            latex_inline(s, end, at, spans);
            return LineInfo { list: Some(ListMark { marker: 0..end, kind, depth }), list_depth: depth + 1, ..Default::default() };
        }
        if t.starts_with('\\') {
            let name: String = t[1..].bytes().take_while(|c| c.is_ascii_alphabetic()).map(char::from).collect();
            // A heading: its command hidden, its words drawn as a heading's.
            if let Some(rank) = SECTIONS.iter().position(|s| *s == name) {
                let mut k = lead + 1 + name.len();
                if b.get(k) == Some(&b'*') {
                    k += 1;
                }
                if b.get(k) == Some(&b'[') {
                    k = closing(b, k, b'[', b']').map_or(k, |e| e + 1);
                }
                if b.get(k) == Some(&b'{') {
                    let level = (rank.saturating_sub(self.top) + 1).min(6) as u8;
                    let mut marks = vec![lead..k + 1];
                    if let Some(e) = closing(b, k, b'{', b'}') {
                        marks.push(e..s.len());
                        latex_inline(&s[..e], k + 1, at, spans);
                    } else {
                        latex_inline(s, k + 1, at, spans);
                    }
                    return LineInfo { kind: LineKind::Heading(level), marks, ..Default::default() };
                }
            }
            // A command that stands alone and makes no words of its own
            // (`\vspace{6pt}`, `\maketitle`, `\label{x}`): code.
            if !WORDS.contains(&name.as_str()) {
                if let Some(end) = command_end(b, lead) {
                    let rest = s[end..].trim();
                    if rest.is_empty() || rest.starts_with('%') {
                        return source();
                    }
                }
            }
        }
        indent(lead, at, spans);
        latex_inline(s, lead, at, spans);
        LineInfo::default()
    }
}

/// The commands whose argument is words drawn as they read.
const WORDS: &[&str] = &["textbf", "emph", "textit", "textsl", "texttt", "underline", "uline", "sout", "st", "href", "url", "footnote", "textsc", "textrm", "textsf", "textnormal", "mbox", "text", "textup", "textmd"];

/// The environment a `\begin{…}` or `\end{…}` line names.
fn env_name(t: &str, head: &str) -> Option<String> {
    let rest = t.strip_prefix(head)?;
    let end = rest.find('}')?;
    Some(rest[..end].to_string())
}

/// Where a command written from `at` and its arguments end on the line;
/// none when one runs on to the lines after.
fn command_end(b: &[u8], at: usize) -> Option<usize> {
    let mut k = at + 1;
    while k < b.len() && b[k].is_ascii_alphabetic() {
        k += 1;
    }
    if b.get(k) == Some(&b'*') {
        k += 1;
    }
    while let Some(&c) = b.get(k) {
        match c {
            b'[' => k = closing(b, k, b'[', b']')? + 1,
            b'{' => k = closing(b, k, b'{', b'}')? + 1,
            _ => break,
        }
    }
    Some(k)
}

/// The spans of a line of LaTeX from `from`.
fn latex_inline(s: &str, from: usize, base: usize, spans: &mut Vec<Span>) {
    let b = s.as_bytes();
    let g = |r: Range<usize>| base + r.start..base + r.end;
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'%' => {
                spans.push(bare(g(i..b.len()), SpanKind::Small));
                return;
            }
            b'\\' => {
                let next = b.get(i + 1).copied();
                match next {
                    // A line's break, and its room after it.
                    Some(b'\\') => {
                        let mut e = i + 2;
                        if b.get(e) == Some(&b'[') {
                            e = closing(b, e, b'[', b']').map_or(e, |x| x + 1);
                        }
                        spans.push(span(g(i..e), SpanKind::Quiet, g(i..e), g(e..e)));
                        i = e;
                        continue;
                    }
                    Some(b'(') => {
                        if let Some(j) = (i + 2..b.len().saturating_sub(1)).find(|&j| b[j] == b'\\' && b[j + 1] == b')') {
                            spans.push(span(g(i..j + 2), SpanKind::Math, g(i..i + 2), g(j..j + 2)));
                            i = j + 2;
                            continue;
                        }
                    }
                    // A mark written as itself: `\%`, `\&`.
                    Some(c) if c.is_ascii_punctuation() => {
                        spans.push(span(g(i..i + 1), SpanKind::Quiet, g(i..i + 1), g(i + 1..i + 1)));
                        i += 2;
                        continue;
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        let name: String = s[i + 1..].bytes().take_while(|c| c.is_ascii_alphabetic()).map(char::from).collect();
                        let Some(end) = command_end(b, i) else {
                            spans.push(code(g(i..b.len())));
                            return;
                        };
                        // Its arguments in braces, inside them.
                        let mut args: Vec<Range<usize>> = Vec::new();
                        let mut k = i + 1 + name.len();
                        while k < end {
                            match b[k] {
                                b'{' => {
                                    let e = closing(b, k, b'{', b'}').unwrap_or(end - 1);
                                    args.push(k + 1..e);
                                    k = e + 1;
                                }
                                b'[' => k = closing(b, k, b'[', b']').map_or(end, |e| e + 1),
                                _ => k += 1,
                            }
                        }
                        let words = |kind: SpanKind, arg: &Range<usize>| span(g(i..end), kind, g(i..arg.start), g(arg.end..end));
                        let made = match (name.as_str(), args.as_slice()) {
                            ("textbf", [a, ..]) => Some((words(SpanKind::Strong, a), a.start)),
                            ("emph" | "textit" | "textsl", [a, ..]) => Some((words(SpanKind::Em, a), a.start)),
                            ("texttt", [a, ..]) => Some((words(SpanKind::Code, a), a.start)),
                            ("underline" | "uline", [a, ..]) => Some((words(SpanKind::Underline, a), a.start)),
                            ("sout" | "st", [a, ..]) => Some((words(SpanKind::Strike, a), a.start)),
                            ("footnote", [a, ..]) => Some((span(g(i..end), SpanKind::Small, g(i..a.start - 1), g(end..end)), a.start)),
                            ("textsc" | "textrm" | "textsf" | "textnormal" | "mbox" | "text" | "textup" | "textmd", [a, ..]) => Some((words(SpanKind::Color(None, None), a), a.start)),
                            ("href", [url, a, ..]) => Some((words(SpanKind::Link(s[url.clone()].to_string()), a), a.start)),
                            ("url", [a, ..]) => Some((words(SpanKind::Link(s[a.clone()].to_string()), a), end)),
                            // A citation or a reference: its key, as a reference reads.
                            ("cite" | "citep" | "citet" | "parencite" | "textcite" | "ref" | "eqref" | "cref" | "Cref" | "autoref" | "pageref", [a, ..]) => Some((words(SpanKind::Footnote, a), end)),
                            _ => None,
                        };
                        match made {
                            Some((sp, inside)) => {
                                spans.push(sp);
                                i = inside;
                            }
                            None => {
                                spans.push(code(g(i..end)));
                                i = end;
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            b'$' => {
                let two = b.get(i + 1) == Some(&b'$');
                let w = if two { 2 } else { 1 };
                let close = (i + w..b.len()).find(|&j| b[j] == b'$' && b[j - 1] != b'\\' && (!two || b.get(j + 1) == Some(&b'$')));
                if let Some(j) = close {
                    spans.push(span(g(i..j + w), SpanKind::Math, g(i..i + w), g(j..j + w)));
                    i = j + w;
                    continue;
                }
            }
            b'h' if starts_address(b, i) => {
                let end = address_end(b, i);
                spans.push(bare(g(i..end), SpanKind::Link(s[i..end].to_string())));
                i = end;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
}

// -- folds ------------------------------------------------------------------

/// What a run of a document's own code is drawn as while the caret is
/// away from it (`panels/doc_edit.rs` draws it over its lines, and a
/// press opens it as written).
#[derive(Clone, Debug, PartialEq)]
pub enum FoldKind {
    /// One quiet row: the run's own first words worth reading (a comment's,
    /// else its first line), and how many lines it holds.
    Code { summary: String },
    /// Vertical room only (`#v(6pt)`, `\vspace{..}`, `\bigskip`): drawn as
    /// that room, in pixels.
    Space(f32),
    /// A picture placed by `#image(..)` or `\includegraphics{..}`: its path
    /// as written and its width, as Quarto writes one (`1.6in`, `50%`).
    Picture { path: String, width: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fold {
    pub first: usize,
    pub last: usize,
    pub kind: FoldKind,
}

/// The runs of code lines a text read in a language holds, each one fold:
/// code lines together with the blank lines between them, a picture's
/// line by itself.
pub fn folds(text: &str, a: &Analysis) -> Vec<Fold> {
    let lines: Vec<&str> = text.split('\n').collect();
    let n = lines.len().min(a.lines.len());
    let source = |l: usize| a.lines.get(l).is_some_and(|i| i.kind == LineKind::Source);
    let blank = |l: usize| a.lines.get(l).is_some_and(|i| i.kind == LineKind::Blank);
    let mut out = Vec::new();
    let mut l = 0;
    while l < n {
        if !source(l) {
            l += 1;
            continue;
        }
        if let Some((path, width)) = picture(lines[l]) {
            out.push(Fold { first: l, last: l, kind: FoldKind::Picture { path, width } });
            l += 1;
            continue;
        }
        let first = l;
        let mut last = l;
        loop {
            let mut j = last + 1;
            while j < n && blank(j) {
                j += 1;
            }
            if j < n && source(j) && picture(lines[j]).is_none() {
                last = j;
            } else {
                break;
            }
        }
        let body: Vec<&str> = lines[first..=last].iter().copied().filter(|t| !t.trim().is_empty()).collect();
        // Room, or only structure with nothing to read (the brackets a
        // call closes with, `\begin{..}` and `\end{..}`): drawn as room.
        let quiet = body.iter().all(|t| spacing(t.trim()).is_some() || structure(t.trim()));
        let room: f32 = body.iter().filter_map(|t| spacing(t.trim())).sum();
        let kind = if quiet { FoldKind::Space((room * 4. / 3.).clamp(2., 48.)) } else { FoldKind::Code { summary: summary(&body) } };
        out.push(Fold { first, last, kind });
        l = last + 1;
    }
    out
}

/// Whether a line is only structure, with nothing to read: the brackets
/// a call closes with, or an environment's `\begin{..}` or `\end{..}`.
fn structure(t: &str) -> bool {
    t.chars().all(|c| matches!(c, ']' | ')' | '}' | ',' | ';')) || ((t.starts_with("\\begin{") || t.starts_with("\\end{")) && t.ends_with('}') && t.matches('{').count() == 1)
}

/// The room a line of vertical spacing makes, in points; none when the
/// line is anything else.
fn spacing(t: &str) -> Option<f32> {
    let named = match t {
        "\\bigskip" => Some(12.),
        "\\medskip" => Some(6.),
        "\\smallskip" => Some(3.),
        "\\newpage" | "\\clearpage" | "\\pagebreak" | "#pagebreak()" => Some(12.),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let inner = t.strip_prefix("#v(").and_then(|r| r.strip_suffix(')')).or_else(|| t.strip_prefix("\\vspace*{").or_else(|| t.strip_prefix("\\vspace{")).and_then(|r| r.strip_suffix('}')))?;
    let v = inner.split(',').next()?.trim();
    let digits = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(v.len());
    let (num, unit) = v.split_at(digits);
    let num: f32 = num.parse().ok()?;
    let per = match unit.trim() {
        "pt" | "" => 1.,
        "em" => 11.,
        "ex" => 5.,
        "in" => 72.,
        "cm" => 28.35,
        "mm" => 2.835,
        "fr" => return Some(12.),
        _ => return None,
    };
    Some((num * per).max(0.))
}

/// A run's first words worth reading: a comment's that has letters in it,
/// its rules of `═` and `=` left off; else its first line.
fn summary(body: &[&str]) -> String {
    let comment = body.iter().find_map(|t| {
        let t = t.trim();
        let words = t.strip_prefix("//").or_else(|| t.strip_prefix('%')).or_else(|| t.strip_prefix("/*"))?;
        let words = words.trim_matches(|c: char| !c.is_alphanumeric() && c != '(' && c != ')');
        words.chars().any(char::is_alphabetic).then(|| words.to_string())
    });
    let line = comment.unwrap_or_else(|| body.first().map(|t| t.trim().to_string()).unwrap_or_default());
    // Rules drawn with `═` or `=` among the words go.
    let line = line.split_whitespace().filter(|w| !(w.chars().count() > 1 && w.chars().all(|c| matches!(c, '═' | '=' | '─' | '━' | '-' | '*' | '#' | '~')))).collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(90) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

/// A picture a line places: `#image("path", width: 1.6in)` or
/// `\includegraphics[width=0.5\textwidth]{path}`.
fn picture(line: &str) -> Option<(String, Option<String>)> {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("#image(") {
        let b = rest.as_bytes();
        let path = first_string(b, 0, b.len())?;
        let width = rest.find("width:").map(|w| rest[w + 6..].split([',', ')']).next().unwrap_or("").trim().to_string()).filter(|w| !w.is_empty());
        return Some((rest[path].to_string(), width));
    }
    let rest = t.strip_prefix("\\includegraphics")?;
    let (opts, rest) = match rest.strip_prefix('[') {
        Some(r) => {
            let e = r.find(']')?;
            (&r[..e], &r[e + 1..])
        }
        None => ("", rest),
    };
    let path = rest.strip_prefix('{')?.split('}').next()?.to_string();
    let width = opts.split(',').find_map(|o| o.trim().strip_prefix("width=")).map(|w| {
        let w = w.trim();
        match w.strip_suffix("\\textwidth").or_else(|| w.strip_suffix("\\linewidth")).or_else(|| w.strip_suffix("\\columnwidth")) {
            Some(f) => format!("{}%", (f.trim().parse::<f32>().unwrap_or(1.) * 100.).round()),
            None => w.to_string(),
        }
    });
    Some((path, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, d: Dialect) -> Vec<LineKind> {
        read(text, d).0.into_iter().map(|l| l.kind).collect()
    }

    fn styled(text: &str, d: Dialect) -> Vec<(String, SpanKind)> {
        read(text, d).1.into_iter().map(|s| (text[s.inner()].to_string(), s.kind)).collect()
    }

    #[test]
    fn typst_code_is_quiet_and_words_read() {
        let text = "// a comment\n#set page(\n  paper: \"a4\",\n)\n= Title\n#v(6pt)\n#block(breakable: false)[\n  Yours *sincerely*,\n]\n- one\n+ first\n+ second";
        let k = kinds(text, Dialect::Typst);
        assert_eq!(k[..4], [LineKind::Source, LineKind::Source, LineKind::Source, LineKind::Source]);
        assert_eq!(k[4], LineKind::Heading(1));
        assert_eq!(k[5], LineKind::Source);
        assert_eq!(k[6], LineKind::Source);
        assert_eq!(k[7], LineKind::Text);
        assert_eq!(k[8], LineKind::Source);
        let (lines, _) = read(text, Dialect::Typst);
        assert_eq!(lines[9].list.as_ref().map(|m| m.kind), Some(ListKind::Bullet));
        assert_eq!(lines[11].list.as_ref().map(|m| m.kind), Some(ListKind::Ordered(2)));
        assert!(styled(text, Dialect::Typst).contains(&("sincerely".into(), SpanKind::Strong)));
    }

    #[test]
    fn typst_calls_show_their_words() {
        let text = "#text(size: 13pt, weight: \"bold\")[Pingfan Hu] \\\nSee #link(\"https://pingfan.org\")[pingfan.org] and _this_ at #h(4pt) $x^2$ @fig-a.";
        let s = styled(text, Dialect::Typst);
        assert!(s.contains(&("Pingfan Hu".into(), SpanKind::Strong)));
        assert!(s.contains(&("pingfan.org".into(), SpanKind::Link("https://pingfan.org".into()))));
        assert!(s.contains(&("this".into(), SpanKind::Em)));
        // Code among words hides whole away from the caret.
        let (_, spans) = read(text, Dialect::Typst);
        let h = spans.iter().find(|s| s.kind == SpanKind::Source).unwrap();
        assert_eq!(&text[h.open.clone()], "#h(4pt)");
        assert!(s.contains(&("x^2".into(), SpanKind::Math)));
        assert!(s.contains(&("@fig-a".into(), SpanKind::Footnote)));
        assert!(s.iter().any(|(_, k)| *k == SpanKind::Quiet));
    }

    #[test]
    fn latex_preamble_sections_lists_and_words() {
        let text = "\\documentclass{article}\n\\usepackage{x}\n\\begin{document}\n\\section{Intro}\n\\subsection*{More}\nSome \\textbf{bold}, \\emph{em}, \\href{https://a.com}{a link} and $x$. % note\n\\vspace{6pt}\n\\begin{enumerate}\n\\item One\n\\item Two\n\\end{enumerate}\n\\begin{equation}\nE = mc^2\n\\end{equation}\n50\\% off\n\\end{document}";
        let (lines, _) = read(text, Dialect::Latex);
        let k: Vec<LineKind> = lines.iter().map(|l| l.kind.clone()).collect();
        assert_eq!(k[..3], [LineKind::Source, LineKind::Source, LineKind::Source]);
        assert_eq!(k[3], LineKind::Heading(1));
        assert_eq!(k[4], LineKind::Heading(2));
        assert_eq!(k[5], LineKind::Text);
        assert_eq!(k[6], LineKind::Source);
        assert_eq!(lines[9].list.as_ref().map(|m| m.kind), Some(ListKind::Ordered(2)));
        assert_eq!(k[12], LineKind::Text);
        assert_eq!(*k.last().unwrap(), LineKind::Source);
        let s = styled(text, Dialect::Latex);
        assert!(s.contains(&("bold".into(), SpanKind::Strong)));
        assert!(s.contains(&("em".into(), SpanKind::Em)));
        assert!(s.contains(&("a link".into(), SpanKind::Link("https://a.com".into()))));
        assert!(s.contains(&("x".into(), SpanKind::Math)));
        assert!(s.contains(&("E = mc^2".into(), SpanKind::Math)));
        // `\section{` and `}` hide away from the caret.
        assert_eq!(lines[3].marks.len(), 2);
    }

    #[test]
    fn code_folds_into_rows_room_and_pictures() {
        let text = "// ═══ PAGE ═══\n#set page(\n  paper: \"a4\",\n)\n\n#set text(size: 11pt)\nDear Hiring Manager,\n\n#v(6pt)\n\nYours,\n#v(4pt)\n#image(\"sig.png\", width: 1.6in)\n#v(2pt)";
        let a = analyze(text, Dialect::Typst);
        let f = folds(text, &a);
        assert_eq!(f[0], Fold { first: 0, last: 5, kind: FoldKind::Code { summary: "PAGE".into() } });
        assert_eq!(f[1].kind, FoldKind::Space(8.));
        assert_eq!((f[1].first, f[1].last), (8, 8));
        assert_eq!(f[3].kind, FoldKind::Picture { path: "sig.png".into(), width: Some("1.6in".into()) });
        assert_eq!(f.len(), 5);
        assert_eq!(picture("\\includegraphics[width=0.5\\textwidth]{fig/a.png}"), Some(("fig/a.png".into(), Some("50%".into()))));
        assert_eq!(spacing("\\vspace{1em}"), Some(11.));
        let tex = "\\section{A}\nWords.\n\n\\bigskip\n\n\\begin{itemize}\n\\item One\n\\end{itemize}";
        let f = folds(tex, &analyze(tex, Dialect::Latex));
        assert_eq!(f[0].kind, FoldKind::Space(16.));
        assert_eq!(f[1].kind, FoldKind::Space(2.));
    }

    #[test]
    fn a_raw_block_names_its_language() {
        assert_eq!(Dialect::of_raw("{=typst}"), Some(Dialect::Typst));
        assert_eq!(Dialect::of_raw("{=latex}"), Some(Dialect::Latex));
        assert_eq!(Dialect::of_raw("{r}"), None);
    }
}
