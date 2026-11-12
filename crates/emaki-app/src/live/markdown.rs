//! A text's markdown read for the live preview: what each line is (a
//! heading, a list item, a line of a fence, a row of a table), and every
//! inline span with where its marks stand, so the editor can hide the
//! marks of a span the caret is not in and draw the words styled.
//!
//! The reading is pulldown-cmark's, with offsets, over the whole text;
//! what is kept is by the line, since the editor draws by the line.
//! Nothing here knows the caret: the editor decides what to hide.
//!
//! From Onyx's `markdown.rs`. A `[[wikilink]]` is not read here, and a
//! block of HTML is told by its lines and nothing more.

use std::ops::Range;

use super::html::{self, Rgb, Style};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::lines::{list_line, quote_line, ListKind};

/// What a line is, by its block.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum LineKind {
    #[default]
    Text,
    Blank,
    Heading(u8),
    /// A setext heading's underline (`===` or `---`), under its words.
    SetextUnderline,
    /// The opening fence of a code block, with its language.
    FenceOpen(String),
    FenceClose,
    /// A line of code, in a fence or indented.
    Code,
    /// The `---` lines around a YAML header.
    FrontMatterEdge,
    FrontMatter,
    Rule,
    /// The first line of a table; the table is drawn here.
    TableFirst(usize),
    /// Any other line of a table, drawn as part of the first.
    TableRow(usize),
    Html,
}

/// A list item's mark on its first line, replaced by a widget: a disc, a
/// number, a box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListMark {
    /// The marker as written, with its trailing space (`- `, `1. `, `- [ ] `).
    pub marker: Range<usize>,
    pub kind: ListKind,
    /// How deep the item is nested, from 0.
    pub depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LineInfo {
    pub kind: LineKind,
    /// Block syntax on the line, shown muted when the caret is on it and
    /// hidden otherwise: a heading's `#`s, a quote's `>`s.
    pub marks: Vec<Range<usize>>,
    pub quote_depth: usize,
    pub list: Option<ListMark>,
    /// How many list levels the line stands inside, for indent guides:
    /// a nested item's line, or a continuation line of one.
    pub list_depth: usize,
    /// Where a task line's words begin, for striking a done task through.
    pub task_done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpanKind {
    Strong,
    Em,
    Strike,
    Code,
    Link(String),
    /// A picture: its source. It is not drawn: its marks always show.
    Image(String),
    Highlight,
    Math,
    Footnote,
    Underline,
    Sub,
    Sup,
    Small,
    /// A tag's ink, its wash, or both.
    Color(Option<Rgb>, Option<Rgb>),
    /// `<br>`: the words after it stand on a line of their own.
    Break,
    /// A tag with no look of its own, or left unpaired; a comment; a
    /// backslash before a mark. Each is hidden and nothing else.
    Quiet,
}

/// An inline span in the whole text: its range, what it is, and its
/// opening and closing marks, which are hidden when the caret is away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: SpanKind,
    pub open: Range<usize>,
    pub close: Range<usize>,
}

impl Span {
    /// The words between the marks.
    pub fn inner(&self) -> Range<usize> {
        self.open.end..self.close.start
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// A table, read from its lines: the header row first, the separator
/// left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub first: usize,
    pub last: usize,
    pub aligns: Vec<Align>,
    pub rows: Vec<Vec<String>>,
    /// Each row's cells, as bytes of their line: the words between the
    /// bars, or an empty range where a cell holds none.
    pub cells: Vec<Vec<Range<usize>>>,
    /// Each column's share of the room in hundredths, when a person has
    /// set them (`tables::shares_of`).
    pub shares: Option<Vec<f32>>,
}

impl Table {
    /// The line a row is written on: the separator stands after the head.
    pub fn line_of(&self, row: usize) -> usize {
        if row == 0 { self.first } else { self.first + 1 + row }
    }

    /// The row written on a line; the separator is none.
    pub fn row_of(&self, line: usize) -> Option<usize> {
        match line.checked_sub(self.first)? {
            0 => Some(0),
            1 => None,
            below => Some(below - 1),
        }
    }

    pub fn columns(&self) -> usize {
        self.aligns.len().max(1)
    }
}

/// A block of HTML: its lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtmlBlock {
    pub first: usize,
    pub last: usize,
}

/// A fenced block of code: its fence lines and its language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
    pub first: usize,
    pub last: usize,
    pub lang: String,
    /// Whether the closing fence is there; without it the block runs to the end.
    pub closed: bool,
}

/// A list as the parser found it: whether it is numbered, and the first
/// line of each of its items, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRange {
    pub ordered: bool,
    pub items: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub lines: Vec<LineInfo>,
    pub lists: Vec<ListRange>,
    /// Every span, in order of its start, the outer before the inner.
    pub spans: Vec<Span>,
    /// For each line, the spans that touch it, as indices into `spans`.
    pub line_spans: Vec<Vec<usize>>,
    pub tables: Vec<Table>,
    pub code_blocks: Vec<CodeBlock>,
    pub html_blocks: Vec<HtmlBlock>,
    pub line_starts: Vec<usize>,
}

impl Analysis {
    /// The line a byte of the whole text is on.
    #[allow(dead_code)]
    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    /// The code block a line is in, fences included.
    pub fn code_block_at(&self, line: usize) -> Option<&CodeBlock> {
        self.code_blocks.iter().find(|b| b.first <= line && line <= b.last)
    }

    /// The block of HTML a line is in.
    pub fn html_block_at(&self, line: usize) -> Option<&HtmlBlock> {
        self.html_blocks.iter().find(|b| b.first <= line && line <= b.last)
    }

    /// The table a line is in, by the line's kind.
    pub fn table_at(&self, line: usize) -> Option<&Table> {
        match self.lines.get(line)?.kind {
            LineKind::TableFirst(t) | LineKind::TableRow(t) => self.tables.get(t),
            _ => None,
        }
    }
}

/// The text read whole. The parser is a third party's and what is made
/// of its places slices the text by them, so the reading runs where a
/// panic is caught: a text that cannot be read is shown as plain lines.
pub fn analyze(text: &str) -> Analysis {
    std::panic::catch_unwind(|| read(text)).unwrap_or_else(|_| plain(text))
}

/// Every line as words or as blank, with nothing else known of it.
pub(crate) fn plain(text: &str) -> Analysis {
    let line_starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let n = line_starts.len();
    let blank = |i: usize| {
        let end = line_starts.get(i + 1).map_or(text.len(), |e| e - 1);
        text.get(line_starts[i]..end).is_none_or(|l| l.trim().is_empty())
    };
    let lines = (0..n).map(|i| LineInfo { kind: if blank(i) { LineKind::Blank } else { LineKind::Text }, ..Default::default() }).collect();
    Analysis { lines, line_spans: vec![Vec::new(); n], line_starts, ..Default::default() }
}

fn read(text: &str) -> Analysis {
    let line_starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let n = line_starts.len();
    let line_text = |i: usize| -> &str {
        let start = line_starts[i];
        let end = line_starts.get(i + 1).map_or(text.len(), |e| e - 1);
        &text[start..end]
    };
    let mut lines: Vec<LineInfo> = (0..n).map(|i| LineInfo { kind: if line_text(i).trim().is_empty() { LineKind::Blank } else { LineKind::Text }, ..Default::default() }).collect();
    let mut a = Analysis { line_starts: line_starts.clone(), ..Default::default() };
    let line_of = |offset: usize| match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    // The last line a range touches: its end is past the range's last byte.
    let last_line_of = |r: &Range<usize>| if r.end > r.start { line_of(r.end - 1) } else { line_of(r.start) };

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_MATH | Options::ENABLE_FOOTNOTES);
    let events = Parser::new_ext(text, opts).into_offset_iter();

    // The lists open around the parser's place, as indices into `a.lists`.
    let mut lists: Vec<usize> = Vec::new();
    // A link or a picture open: its range, where its words have got to.
    struct OpenLink {
        range: Range<usize>,
        words_end: usize,
        kind: SpanKind,
        open_len: usize,
    }
    let mut links: Vec<OpenLink> = Vec::new();
    let mut item_line: Option<usize> = None;
    // Tags among the words waiting for their closing one.
    let mut tags: Vec<(String, Style, Range<usize>)> = Vec::new();

    for (ev, r) in events {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                let line = line_of(r.start);
                let t = line_text(line);
                let lead = t.len() - t.trim_start().len();
                let hashes = t[lead..].chars().take_while(|c| *c == '#').count();
                if hashes > 0 && hashes <= 6 && t[lead + hashes..].starts_with([' ', '\t']) || t[lead..].trim_end() == "#".repeat(hashes) {
                    let mut end = lead + hashes;
                    while t[end..].starts_with(' ') {
                        end += 1;
                    }
                    let mut marks = vec![lead..end];
                    // A closing run of `#`s, which markdown drops.
                    let trimmed = t.trim_end();
                    let tail = trimmed.len() - trimmed.trim_end_matches('#').len();
                    if tail > 0 && trimmed[..trimmed.len() - tail].ends_with(' ') && trimmed.len() - tail > end {
                        let from = trimmed[..trimmed.len() - tail].trim_end().len();
                        marks.push(from..t.len());
                    }
                    lines[line].kind = LineKind::Heading(level as u8);
                    lines[line].marks = marks;
                } else {
                    lines[line].kind = LineKind::Heading(level as u8);
                    let under = last_line_of(&r);
                    if under > line && under < n {
                        lines[under].kind = LineKind::SetextUnderline;
                        lines[under].marks = vec![0..line_text(under).len()];
                    }
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                for line in line_of(r.start)..=last_line_of(&r).min(n - 1) {
                    lines[line].quote_depth += 1;
                }
            }
            Event::Start(Tag::List(start)) => {
                lists.push(a.lists.len());
                a.lists.push(ListRange { ordered: start.is_some(), items: Vec::new() });
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let line = line_of(r.start);
                let depth = lists.len().saturating_sub(1);
                let t = line_text(line);
                let body_start = quote_line(t).map_or(0, |(_, at)| at);
                if let Some(item) = list_line(&t[body_start..]) {
                    let from = body_start + item.indent.len();
                    lines[line].list = Some(ListMark { marker: from..from + item.marker.len(), kind: item.kind, depth });
                    lines[line].task_done = matches!(item.kind, ListKind::Task(true));
                }
                for l in line..=last_line_of(&r).min(n - 1) {
                    lines[l].list_depth = lines[l].list_depth.max(depth + 1);
                }
                if let Some(&ix) = lists.last() {
                    a.lists[ix].items.push(line);
                }
                item_line = Some(line);
            }
            Event::TaskListMarker(done) => {
                if let Some(line) = item_line {
                    if let Some(m) = lines[line].list.as_mut() {
                        m.kind = ListKind::Task(done);
                        let t = line_text(line);
                        let mut end = r.end - line_starts[line];
                        if t[end..].starts_with(' ') {
                            end += 1;
                        }
                        m.marker.end = end;
                        lines[line].task_done = done;
                    }
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let first = line_of(r.start);
                let last = last_line_of(&r).min(n - 1);
                match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let fence_of = |t: &str| t.trim_start().starts_with("```") || t.trim_start().starts_with("~~~");
                        let closed = last > first && fence_of(line_text(last));
                        lines[first].kind = LineKind::FenceOpen(lang.trim().to_string());
                        let body_end = if closed { last } else { last + 1 };
                        for l in first + 1..body_end.min(n) {
                            lines[l].kind = LineKind::Code;
                        }
                        if closed {
                            lines[last].kind = LineKind::FenceClose;
                        }
                        a.code_blocks.push(CodeBlock { first, last, lang: lang.trim().to_string(), closed });
                    }
                    CodeBlockKind::Indented => {
                        for l in first..=last {
                            lines[l].kind = LineKind::Code;
                        }
                    }
                }
            }
            Event::Start(Tag::MetadataBlock(_)) => {
                let first = line_of(r.start);
                let last = last_line_of(&r).min(n - 1);
                for l in first..=last {
                    lines[l].kind = if l == first || l == last { LineKind::FrontMatterEdge } else { LineKind::FrontMatter };
                }
            }
            Event::Start(Tag::HtmlBlock) => {
                let (first, last) = (line_of(r.start), last_line_of(&r).min(n - 1));
                for l in first..=last {
                    if lines[l].kind == LineKind::Text {
                        lines[l].kind = LineKind::Html;
                    }
                }
                a.html_blocks.push(HtmlBlock { first, last });
            }
            Event::Rule => {
                lines[line_of(r.start)].kind = LineKind::Rule;
            }
            Event::Start(Tag::Table(aligns)) => {
                let first = line_of(r.start);
                let mut last = last_line_of(&r).min(n - 1);
                // The parser's range may end at the row's newline; a row
                // is a line that starts with a bar.
                while last + 1 < n && line_text(last + 1).trim_start().starts_with('|') {
                    last += 1;
                }
                while last > first && line_text(last).trim().is_empty() {
                    last -= 1;
                }
                let ix = a.tables.len();
                let mut rows = Vec::new();
                let mut spots = Vec::new();
                for l in first..=last {
                    if l != first + 1 {
                        let t = line_text(l);
                        let from = super::tables::body_start(t);
                        rows.push(cells(&t[from..]));
                        let mut row = cell_ranges(t, from);
                        row.truncate(aligns.len().max(1));
                        spots.push(row);
                    }
                    lines[l].kind = if l == first { LineKind::TableFirst(ix) } else { LineKind::TableRow(ix) };
                }
                let shares = (first < last).then(|| super::tables::shares_of(line_text(first + 1))).flatten();
                let aligns = aligns
                    .iter()
                    .map(|al| match al {
                        pulldown_cmark::Alignment::Left => Align::Left,
                        pulldown_cmark::Alignment::Center => Align::Center,
                        pulldown_cmark::Alignment::Right => Align::Right,
                        pulldown_cmark::Alignment::None => Align::None,
                    })
                    .collect();
                a.tables.push(Table { first, last, aligns, rows, cells: spots, shares });
            }
            Event::Start(Tag::Emphasis) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Em, open: r.start..r.start + 1, close: r.end - 1..r.end }),
            Event::Start(Tag::Strong) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Strong, open: r.start..r.start + 2, close: r.end - 2..r.end }),
            Event::Start(Tag::Strikethrough) => {
                let m = if text[r.start..].starts_with("~~") { 2 } else { 1 };
                a.spans.push(Span { range: r.clone(), kind: SpanKind::Strike, open: r.start..r.start + m, close: r.end - m..r.end });
            }
            Event::Code(_) => {
                let m = text[r.start..].chars().take_while(|c| *c == '`').count();
                a.spans.push(Span { range: r.clone(), kind: SpanKind::Code, open: r.start..r.start + m, close: r.end - m..r.end });
            }
            Event::InlineMath(_) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Math, open: r.start..r.start + 1, close: r.end - 1..r.end }),
            Event::DisplayMath(_) => {
                let m = 2.min(r.len() / 2);
                a.spans.push(Span { range: r.clone(), kind: SpanKind::Math, open: r.start..r.start + m, close: r.end - m..r.end });
            }
            Event::FootnoteReference(_) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Footnote, open: r.start..r.start, close: r.end..r.end }),
            Event::InlineHtml(tag) => match html::read_tag(&tag) {
                // A comment, and a tag that is no style of ours, are
                // words of the message: a prompt's `<task>` is not put
                // out of sight.
                html::Tag::Comment => {}
                html::Tag::Break => a.spans.push(Span { range: r.clone(), kind: SpanKind::Break, open: r.clone(), close: r.end..r.end }),
                html::Tag::Image { src, alt } => {
                    let words = if alt.is_empty() { r.end..r.end } else { r.start + alt.start..r.start + alt.end };
                    a.spans.push(Span { range: r.clone(), kind: SpanKind::Image(src), open: r.start..words.start, close: words.end..r.end });
                }
                html::Tag::Open { name, style } => tags.push((name, style, r.clone())),
                html::Tag::Close { name } => match tags.iter().rposition(|(n, _, _)| *n == name) {
                    Some(at) => {
                        tags.truncate(at + 1);
                        if let Some((_, style, open)) = tags.pop().filter(|(_, style, _)| !matches!(style, Style::Plain)) {
                            a.spans.push(Span { range: open.start..r.end, kind: span_of(style), open, close: r.clone() });
                        }
                    }
                    None => {}
                },
                html::Tag::Not => {}
            },
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::TableCell | TagEnd::Item) => {
                tags.clear();
                if let Some(l) = links.last_mut() {
                    l.words_end = l.words_end.max(r.end.min(l.range.end));
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                links.push(OpenLink { range: r.clone(), words_end: r.start + 1, kind: SpanKind::Link(dest_url.to_string()), open_len: 1 });
            }
            // `![words](src)` opens with two marks.
            Event::Start(Tag::Image { dest_url, .. }) => {
                links.push(OpenLink { range: r.clone(), words_end: r.start + 2, kind: SpanKind::Image(dest_url.to_string()), open_len: 2 });
            }
            Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                if let Some(l) = links.pop() {
                    let words_end = l.words_end.max(l.range.start + l.open_len).min(l.range.end);
                    let open = l.range.start..l.range.start + l.open_len;
                    let words_end = words_end.max(open.end);
                    a.spans.push(Span { range: l.range.clone(), kind: l.kind, open, close: words_end..l.range.end });
                }
            }
            Event::Text(_) => {
                if let Some(l) = links.last_mut() {
                    l.words_end = r.end;
                }
                // A backslash before a mark is hidden as the mark is.
                let slashes = text[..r.start].bytes().rev().take_while(|b| *b == b'\\').count();
                if slashes % 2 == 1 && text[r.start..].starts_with(|c: char| c.is_ascii_punctuation()) && lines[line_of(r.start)].kind != LineKind::Code {
                    let at = r.start - 1;
                    a.spans.push(Span { range: at..r.start, kind: SpanKind::Quiet, open: at..r.start, close: r.start..r.start });
                }
                // `==words==`, which the parser does not know.
                for h in highlights(&text[r.clone()]) {
                    let (from, to) = (r.start + h.start, r.start + h.end);
                    a.spans.push(Span { range: from..to, kind: SpanKind::Highlight, open: from..from + 2, close: to - 2..to });
                }
            }
            Event::Start(_) | Event::End(_) | Event::Html(_) | Event::SoftBreak | Event::HardBreak => {
                if let (Some(l), Event::End(_)) = (links.last_mut(), &ev) {
                    l.words_end = l.words_end.max(r.end.min(l.range.end));
                }
                if let Some(l) = links.last_mut() {
                    if matches!(ev, Event::SoftBreak | Event::HardBreak) {
                        l.words_end = r.end;
                    }
                }
            }
        }
    }

    // A quote's marks on each of its lines, by the depth the parser gave it.
    for (i, info) in lines.iter_mut().enumerate() {
        if info.quote_depth > 0 {
            let t = line_text(i);
            let mut at = 0;
            let mut seen = 0;
            let bytes = t.as_bytes();
            while seen < info.quote_depth && at < bytes.len() {
                let mark_start = at;
                while at < bytes.len() && bytes[at] == b' ' {
                    at += 1;
                }
                if at < bytes.len() && bytes[at] == b'>' {
                    at += 1;
                    if at < bytes.len() && bytes[at] == b' ' {
                        at += 1;
                    }
                    info.marks.push(mark_start..at);
                    seen += 1;
                } else {
                    break;
                }
            }
        }
    }

    a.spans.sort_by(|x, y| x.range.start.cmp(&y.range.start).then(y.range.end.cmp(&x.range.end)));
    a.line_spans = vec![Vec::new(); n];
    for (ix, s) in a.spans.iter().enumerate() {
        let first = line_of(s.range.start);
        let last = last_line_of(&s.range).min(n - 1);
        for l in first..=last {
            a.line_spans[l].push(ix);
        }
    }
    a.lines = lines;
    a
}

/// Where `==words==` stands in a run of text: each from its opening pair
/// to its closing one. A pair with a space inside it against either mark
/// is not one.
fn highlights(s: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = s[from..].find("==").map(|i| i + from) {
        let after = open + 2;
        let Some(close) = s[after..].find("==").map(|i| i + after) else { break };
        if close > after && !s[after..close].starts_with(' ') && !s[after..close].ends_with(' ') {
            out.push(open..close + 2);
        }
        from = close + 2;
    }
    out
}

/// What a tag does to its words, as a span of the kind the text's own
/// marks make, so a tag and its markdown read alike.
fn span_of(style: Style) -> SpanKind {
    match style {
        Style::Strong => SpanKind::Strong,
        Style::Em => SpanKind::Em,
        Style::Underline => SpanKind::Underline,
        Style::Strike => SpanKind::Strike,
        Style::Mark => SpanKind::Highlight,
        Style::Code => SpanKind::Code,
        Style::Sub => SpanKind::Sub,
        Style::Sup => SpanKind::Sup,
        Style::Small => SpanKind::Small,
        Style::Link(url) => SpanKind::Link(url),
        Style::Color { ink, wash } => SpanKind::Color(ink, wash),
        Style::Plain => SpanKind::Quiet,
    }
}

/// A run of a text's prose, for a check: where it starts in the text,
/// its words with everything that is not prose blanked, byte for byte,
/// and whether it is a paragraph, which has sentences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prose {
    pub start: usize,
    pub text: String,
    pub sentences: bool,
}

/// What of a text is prose: the whole text with what is not blanked,
/// byte for byte, and its runs, one to a block, so a check reads a list's
/// items or a table's cells apart and a change costs only its own block.
pub fn prose(text: &str, a: &Analysis) -> (String, Vec<Prose>) {
    let mut mask = text.as_bytes().to_vec();
    let blank = |mask: &mut Vec<u8>, r: Range<usize>| {
        for b in mask.iter_mut().take(r.end.min(text.len())).skip(r.start.min(text.len())) {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    };
    let line_range = |i: usize| a.line_starts[i]..a.line_starts.get(i + 1).map_or(text.len(), |e| e - 1);
    for s in &a.spans {
        match &s.kind {
            SpanKind::Code | SpanKind::Math | SpanKind::Footnote | SpanKind::Image(_) | SpanKind::Quiet | SpanKind::Break => blank(&mut mask, s.range.clone()),
            _ => {
                blank(&mut mask, s.open.clone());
                blank(&mut mask, s.close.clone());
            }
        }
    }
    for (i, info) in a.lines.iter().enumerate() {
        let from = a.line_starts[i];
        for m in &info.marks {
            blank(&mut mask, from + m.start..from + m.end);
        }
        if let Some(m) = &info.list {
            blank(&mut mask, from..from + m.marker.end);
        }
        if matches!(info.kind, LineKind::Code | LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::FrontMatter | LineKind::FrontMatterEdge | LineKind::Html | LineKind::Rule | LineKind::SetextUnderline) {
            blank(&mut mask, line_range(i));
        }
    }
    for t in &a.tables {
        for l in t.first..=t.last {
            let range = line_range(l);
            let keep: Vec<Range<usize>> = t.row_of(l).and_then(|r| t.cells.get(r)).cloned().unwrap_or_default();
            let mut at = range.start;
            for c in &keep {
                blank(&mut mask, at..range.start + c.start);
                at = range.start + c.end;
            }
            blank(&mut mask, at..range.end);
        }
    }
    for b in &a.html_blocks {
        blank(&mut mask, a.line_starts[b.first]..line_range(b.last).end);
    }
    let whole = String::from_utf8(mask).unwrap_or_else(|_| " ".repeat(text.len()));
    let mut runs = Vec::new();
    let mut push = |range: Range<usize>, sentences: bool| {
        if let Some(words) = whole.get(range.clone()) {
            if !words.trim().is_empty() {
                runs.push(Prose { start: range.start, text: words.to_string(), sentences });
            }
        }
    };
    let n = a.lines.len();
    let mut i = 0;
    while i < n {
        let info = &a.lines[i];
        match info.kind {
            LineKind::Heading(_) => push(line_range(i), false),
            LineKind::TableFirst(t) => {
                let table = &a.tables[t];
                for (r, cells) in table.cells.iter().enumerate() {
                    let from = a.line_starts[table.line_of(r)];
                    for c in cells {
                        push(from + c.start..from + c.end, false);
                    }
                }
                i = table.last;
            }
            LineKind::Text => {
                let sentences = info.list.is_none();
                let start = i;
                while i + 1 < n && a.lines[i + 1].kind == LineKind::Text && a.lines[i + 1].list.is_none() && a.lines[i + 1].quote_depth == info.quote_depth {
                    i += 1;
                }
                push(a.line_starts[start]..line_range(i).end, sentences);
            }
            _ => {}
        }
        i += 1;
    }
    (whole, runs)
}

/// Where each cell of a row stands in its line, the words between the
/// bars, from `from`, where the row begins. A cell of no words is an
/// empty range past the first space after its bar.
pub fn cell_ranges(line: &str, from: usize) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let end = line.trim_end().len();
    let mut at = from;
    if bytes.get(at) == Some(&b'|') {
        at += 1;
    }
    let mut out = Vec::new();
    let mut cell = at;
    let mut i = at;
    let mut push = |a: usize, b: usize| {
        let part = &line[a..b];
        let trimmed = part.trim();
        out.push(if trimmed.is_empty() {
            let spot = (a + 1).min(b);
            spot..spot
        } else {
            let lead = part.len() - part.trim_start().len();
            a + lead..a + lead + trimmed.len()
        });
    };
    while i < end {
        match bytes[i] {
            b'\\' => i += 2,
            b'|' => {
                push(cell, i);
                cell = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    if cell < end {
        push(cell, end);
    }
    out
}

/// A table row's cells, as written between its bars, trimmed. A bar
/// escaped with a backslash is part of its cell.
pub fn cells(row: &str) -> Vec<String> {
    let t = row.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cell.push('|');
            chars.next();
        } else if c == '|' {
            out.push(cell.trim().to_string());
            cell.clear();
        } else {
            cell.push(c);
        }
    }
    out.push(cell.trim().to_string());
    out
}

/// A line shown with some of its bytes hidden: the words that show, and
/// for each shown piece where it came from, so a place in the words maps
/// back to the text and the other way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    /// (start in `text`, start in the source, length in the source,
    /// length in `text`), in order. The two lengths differ only for a
    /// character drawn as another (`Shown::with`).
    pub pieces: Vec<(usize, usize, usize, usize)>,
}

impl Shown {
    /// `source` with `hidden` taken out. The ranges are in the line's bytes,
    /// in any order, and may overlap.
    pub fn new(source: &str, hidden: &[Range<usize>]) -> Shown {
        Self::with(source, hidden, &[])
    }

    /// As `new`, with some bytes drawn as other words: `²` for the `2` in
    /// `<sup>2</sup>`. `swaps` are in order and hold no hidden byte.
    pub fn with(source: &str, hidden: &[Range<usize>], swaps: &[(Range<usize>, String)]) -> Shown {
        let mut hidden: Vec<Range<usize>> = hidden.iter().filter(|r| r.end > r.start).cloned().collect();
        hidden.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for r in hidden {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        let mut shown = Shown::default();
        let mut at = 0;
        for r in merged.iter().chain(std::iter::once(&(source.len()..source.len()))) {
            let start = r.start.min(source.len());
            let mut from = at;
            for (swap, with) in swaps {
                if swap.start < from || swap.end > start {
                    continue;
                }
                if swap.start > from {
                    shown.push_same(source, from, swap.start);
                }
                shown.pieces.push((shown.text.len(), swap.start, swap.len(), with.len()));
                shown.text.push_str(with);
                from = swap.end;
            }
            if start > from {
                shown.push_same(source, from, start);
            }
            at = at.max(r.end.min(source.len()));
        }
        shown
    }

    fn push_same(&mut self, source: &str, from: usize, to: usize) {
        self.pieces.push((self.text.len(), from, to - from, to - from));
        self.text.push_str(&source[from..to]);
    }

    /// Where a byte of the source stands in the words; a hidden byte
    /// stands where its hidden run begins.
    pub fn to_shown(&self, source_col: usize) -> usize {
        for &(d, s, len, out) in &self.pieces {
            if source_col < s {
                return d;
            }
            if source_col <= s + len {
                return if len == out { d + (source_col - s) } else if source_col == s { d } else { d + out };
            }
        }
        self.text.len()
    }

    /// The byte of the source a place in the words stands for. At the
    /// seam between two pieces it is the earlier piece's end, before the
    /// hidden bytes: a caret put at a span's left edge stands before the
    /// whole span, and the editor snaps one at the right edge past it.
    pub fn to_source(&self, shown_col: usize, source_len: usize) -> usize {
        for &(d, s, len, out) in &self.pieces {
            if shown_col <= d + out {
                return if len == out { s + shown_col.saturating_sub(d) } else if shown_col <= d { s } else { s + len };
            }
        }
        source_len
    }

    /// Whether a byte of the source is among the hidden.
    pub fn is_hidden(&self, source_col: usize) -> bool {
        !self.pieces.iter().any(|&(_, s, len, _)| source_col >= s && source_col < s + len)
    }
}

/// The part of a line from `from` to `to` with the hidden ranges taken
/// out. The ranges may overlap and come in any order, and may reach past
/// either end.
pub fn visible_part(text: &str, from: usize, to: usize, hidden: &[Range<usize>]) -> String {
    let mut ranges: Vec<&Range<usize>> = hidden.iter().collect();
    ranges.sort_by_key(|r| r.start);
    let mut out = String::new();
    let mut at = from;
    for r in ranges {
        if r.start >= to {
            break;
        }
        if r.start > at {
            out.push_str(&text[at..r.start]);
        }
        at = at.max(r.end);
    }
    if at < to {
        out.push_str(&text[at..to]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_told_by_the_line() {
        let text = "intro\n\n\n# Title ##\n\n> quoted\n> > deep\n\n- one\n  - [x] two\n3. three\n\n```rust\nlet a = 1;\n```\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\n***\nSetext\n======\n";
        let a = analyze(text);
        let kinds: Vec<&LineKind> = a.lines.iter().map(|l| &l.kind).collect();
        // A message is no file: lines of dashes at its top are a rule,
        // not a header's edges.
        assert_eq!(analyze("---\ntitle: x\n---\n").lines[0].kind, LineKind::Rule);
        assert_eq!(kinds[3], &LineKind::Heading(1));
        assert_eq!(a.lines[3].marks, vec![0..2, 7..10]);
        assert_eq!(kinds[4], &LineKind::Blank);
        assert_eq!(a.lines[5].quote_depth, 1);
        assert_eq!(a.lines[5].marks, vec![0..2]);
        assert_eq!(a.lines[6].quote_depth, 2);
        assert_eq!(a.lines[6].marks, vec![0..2, 2..4]);
        let one = a.lines[8].list.clone().unwrap();
        assert_eq!((one.marker, one.kind, one.depth), (0..2, ListKind::Bullet, 0));
        let two = a.lines[9].list.clone().unwrap();
        assert_eq!((two.marker, two.kind, two.depth), (2..8, ListKind::Task(true), 1));
        assert!(a.lines[9].task_done);
        assert_eq!(a.lines[9].list_depth, 2);
        let three = a.lines[10].list.clone().unwrap();
        assert_eq!((three.marker, three.kind), (0..3, ListKind::Ordered(3)));
        assert_eq!(kinds[12], &LineKind::FenceOpen("rust".into()));
        assert_eq!(kinds[13], &LineKind::Code);
        assert_eq!(kinds[14], &LineKind::FenceClose);
        assert_eq!(a.code_blocks[0], CodeBlock { first: 12, last: 14, lang: "rust".into(), closed: true });
        assert_eq!(kinds[16], &LineKind::TableFirst(0));
        assert_eq!(kinds[17], &LineKind::TableRow(0));
        assert_eq!(kinds[18], &LineKind::TableRow(0));
        assert_eq!(a.tables[0].rows, vec![vec!["a".to_string(), "b".to_string()], vec!["1".to_string(), "2".to_string()]]);
        assert_eq!(a.tables[0].aligns, vec![Align::None, Align::Center]);
        assert_eq!(kinds[20], &LineKind::Rule);
        assert_eq!(kinds[21], &LineKind::Heading(1));
        assert_eq!(kinds[22], &LineKind::SetextUnderline);
    }

    #[test]
    fn lists_know_their_items() {
        let a = analyze("1. a\n3. b\n   - x\n   - y\n9. c\n\n- z");
        assert_eq!(a.lists.len(), 3);
        assert_eq!(a.lists[0], ListRange { ordered: true, items: vec![0, 1, 4] });
        assert_eq!(a.lists[1], ListRange { ordered: false, items: vec![2, 3] });
        assert_eq!(a.lists[2], ListRange { ordered: false, items: vec![6] });
    }

    #[test]
    fn an_open_fence_runs_to_the_end() {
        let a = analyze("```\ncode\nmore");
        assert_eq!(a.lines[0].kind, LineKind::FenceOpen(String::new()));
        assert_eq!(a.lines[1].kind, LineKind::Code);
        assert_eq!(a.lines[2].kind, LineKind::Code);
        assert!(!a.code_blocks[0].closed);
    }

    #[test]
    fn spans_have_their_marks() {
        let text = "a **bold** and *em* and ~~gone~~ and `code` and [link](http://x) and [[Note|alias]] and ![pic](p.png) and ==hi== and $x^2$ and [[Plain]]";
        // A wikilink is words: nothing of it is a span.
        assert!(analyze("[[Note|alias]] and [[Plain]]").spans.is_empty());
        let a = analyze(text);
        let find = |k: &SpanKind| a.spans.iter().find(|s| &s.kind == k).unwrap().clone();
        let s = find(&SpanKind::Strong);
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("**", "bold", "**"));
        let s = find(&SpanKind::Em);
        assert_eq!(&text[s.inner()], "em");
        let s = find(&SpanKind::Strike);
        assert_eq!((&text[s.open.clone()], &text[s.inner()]), ("~~", "gone"));
        let s = find(&SpanKind::Code);
        assert_eq!(&text[s.inner()], "code");
        let s = find(&SpanKind::Link("http://x".into()));
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("[", "link", "](http://x)"));
        let s = find(&SpanKind::Image("p.png".into()));
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("![", "pic", "](p.png)"));
        let s = find(&SpanKind::Highlight);
        assert_eq!(&text[s.inner()], "hi");
        let s = find(&SpanKind::Math);
        assert_eq!(&text[s.inner()], "x^2");
    }

    #[test]
    fn nested_emphasis_keeps_each_layers_marks() {
        let text = "***both*** and **a *b* c**";
        let a = analyze(text);
        let em = a.spans.iter().filter(|s| s.kind == SpanKind::Em).count();
        let strong = a.spans.iter().filter(|s| s.kind == SpanKind::Strong).count();
        assert_eq!((em, strong), (2, 2));
        let outer = &a.spans[0];
        assert_eq!(&text[outer.range.clone()], "***both***");
        assert_eq!(a.line_spans[0].len(), 4);
    }

    #[test]
    fn spans_crossing_lines_are_on_each() {
        let a = analyze("some **bold\nwords** here\nplain");
        assert_eq!(a.line_spans[0].len(), 1);
        assert_eq!(a.line_spans[1].len(), 1);
        assert!(a.line_spans[2].is_empty());
    }

    #[test]
    fn shown_text_maps_both_ways() {
        let src = "**bold** and `x`";
        let s = Shown::new(src, &[0..2, 6..8, 13..14, 15..16]);
        assert_eq!(s.text, "bold and x");
        assert_eq!(s.to_shown(2), 0);
        assert_eq!(s.to_shown(1), 0);
        assert_eq!(s.to_shown(6), 4);
        assert_eq!(s.to_shown(9), 5);
        assert_eq!(s.to_shown(16), 10);
        assert_eq!(s.to_source(0, src.len()), 2);
        assert_eq!(s.to_source(4, src.len()), 6);
        assert_eq!(s.to_source(9, src.len()), 13);
        assert_eq!(s.to_source(10, src.len()), 15);
        assert!(s.is_hidden(0));
        assert!(!s.is_hidden(3));
        let none = Shown::new("abc", &[]);
        assert_eq!(none.text, "abc");
        assert_eq!(none.to_source(3, 3), 3);
    }

    #[test]
    fn a_character_drawn_as_another_maps_both_ways() {
        // x<sup>2</sup>y with the tags hidden and the 2 drawn raised.
        let src = "x<sup>2</sup>y";
        let s = Shown::with(src, &[1..6, 7..13], &[(6..7, "²".to_string())]);
        assert_eq!(s.text, "x²y");
        assert_eq!(s.to_shown(6), 1);
        assert_eq!(s.to_shown(7), 3);
        assert_eq!(s.to_shown(13), 3);
        assert_eq!(s.to_source(1, src.len()), 1);
        assert_eq!(s.to_source(3, src.len()), 7);
        assert_eq!(s.to_source(4, src.len()), 14);
    }

    #[test]
    fn table_cells_are_split_on_unescaped_bars() {
        assert_eq!(cells("| a \\| b | c |"), vec!["a | b", "c"]);
        assert_eq!(cells("x|y"), vec!["x", "y"]);
    }

    #[test]
    fn a_cell_knows_where_it_stands() {
        let line = "| ab | 费城 |  | c\\|d |";
        let cells = cell_ranges(line, 0);
        assert_eq!(cells.iter().map(|c| &line[c.clone()]).collect::<Vec<_>>(), vec!["ab", "费城", "", "c\\|d"]);
        assert!(cells[2].is_empty());
        assert_eq!(cell_ranges("a | b", 0).iter().map(|c| &"a | b"[c.clone()]).collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(cell_ranges("> | a |", 2).len(), 1);
        assert_eq!(cell_ranges("||", 0), vec![1..1]);
    }

    #[test]
    fn a_table_reads_its_words_as_any_line_does() {
        let text = "| [a](http://x) | **b** |\n|---|---|\n| c<br>d | e\\|f |";
        let a = analyze(text);
        let t = &a.tables[0];
        assert_eq!((t.line_of(0), t.line_of(1), t.row_of(1), t.row_of(2)), (0, 2, None, Some(1)));
        assert!(a.line_spans[0].iter().any(|&s| matches!(a.spans[s].kind, SpanKind::Link(_))));
        assert!(a.line_spans[2].iter().any(|&s| a.spans[s].kind == SpanKind::Break));
        assert!(a.line_spans[2].iter().any(|&s| a.spans[s].kind == SpanKind::Quiet), "the backslash before a bar is hidden");
    }

    #[test]
    fn tags_among_the_words_read_as_marks_do() {
        let text = "a <u>under</u> <b>bold</b> <span style=\"color: red\">red</span> <mark>hi</mark> <!-- no --> <x>y</x> <i>open";
        let a = analyze(text);
        let find = |k: &SpanKind| a.spans.iter().find(|s| &s.kind == k).unwrap().clone();
        let s = find(&SpanKind::Underline);
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("<u>", "under", "</u>"));
        assert_eq!(&text[find(&SpanKind::Strong).inner()], "bold");
        assert_eq!(&text[find(&SpanKind::Highlight).inner()], "hi");
        assert_eq!(&text[find(&SpanKind::Color(Some(Rgb(255, 0, 0)), None)).inner()], "red");
        // The comment, `<x>`, `</x>` and the `<i>` left open are words
        // of the message, and stay in sight.
        assert!(!a.spans.iter().any(|s| s.kind == SpanKind::Quiet));
    }

    #[test]
    fn prose_is_the_words_and_nothing_else() {
        let text = "# Heading **bold**\n\nOne `code` [word](http://x.y) and $m$\ntwo\n\n- item\n- more\n\n```\nfn x\n```\n\n| cell | two |\n|---|---|\n| three | four |\n\n<div>skip</div>\n";
        let a = analyze(text);
        let (whole, runs) = prose(text, &a);
        assert_eq!(whole.len(), text.len());
        let words: Vec<(String, bool)> = runs.iter().map(|r| (r.text.split_whitespace().collect::<Vec<_>>().join(" "), r.sentences)).collect();
        let want = [("Heading bold", false), ("One word and two", true), ("item", false), ("more", false), ("cell", false), ("two", false), ("three", false), ("four", false)];
        assert_eq!(words, want.iter().map(|(w, s)| (w.to_string(), *s)).collect::<Vec<_>>());
        for r in &runs {
            assert_eq!(whole.get(r.start..r.start + r.text.len()), Some(r.text.as_str()));
        }
    }

    #[test]
    fn a_line_reads_without_what_is_hidden() {
        let line = "Plain **bold** and [text](http://a.b).";
        let hidden = [6..8, 12..14, 19..20, 24..37];
        assert_eq!(visible_part(line, 0, line.len(), &hidden), "Plain bold and text.");
        // Out of order and overlapping.
        assert_eq!(visible_part(line, 0, line.len(), &[24..37, 19..20, 12..14, 6..8, 7..8]), "Plain bold and text.");
        // A selection that begins or ends inside a mark, or inside the words.
        assert_eq!(visible_part(line, 7, 10, &hidden), "bo");
        assert_eq!(visible_part(line, 10, 13, &hidden), "ld");
        assert_eq!(visible_part(line, 0, 0, &hidden), "");
        // Characters of many bytes.
        assert_eq!(visible_part("费城 **纽约**", 0, 17, &[7..9, 15..17]), "费城 纽约");
    }
}
