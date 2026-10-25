//! Quarto's syntax over markdown, read when [`MarkdownExtensions::quarto`]
//! is on (`markdown.rs` calls in here). What Quarto adds to Pandoc's
//! markdown and how each is drawn:
//!
//! - A fenced div, `::: {.class}` to `:::`, nested with more colons, is a
//!   `BlockNode::Div` holding what is between. The fence lines stand in
//!   paragraphs as text, so a paragraph is cut at them and the pieces
//!   read again on their own.
//! - Attributes after a heading's words (`## Title {.class}`) are kept on
//!   the heading, after a picture (`![](a.png){width="800"}`) set its
//!   width and place, and after a link or a span (`[word]{.class}`) are
//!   dropped: a class there is a style the site's CSS gives, not a thing
//!   to read. A span unwraps to its words; an empty one (`[]{.glyph}`)
//!   goes.
//! - A shortcode that was not resolved (`{{< video … >}}`) is shown as a
//!   quiet chip of its words.
//! - A raw block ```` ```{=html} ```` is the HTML it holds, drawn as the
//!   view draws HTML; raw inline, `` `<b>x</b>`{=html} ``, the same in
//!   the line. Raw blocks of other formats stay blocks of code.
//!
//! (Emaki addition.)

use std::ops::Range;

use gpui::{px, relative, rems, DefiniteLength};
use markdown::mdast;

use crate::text::math::{room, untag, MathSource};
use crate::text::node::{BlockNode, DivAttrs, ImageAlign, InlineNode, NodeContext, Paragraph, Span, TextMark};

/// A fence line of a paragraph: where it is in the paragraph's source,
/// and what it opens, or that it closes.
pub(super) struct Fence {
    pub range: Range<usize>,
    pub open: Option<DivAttrs>,
}

/// The fence lines in a paragraph's source. A line of three or more
/// colons opens a div with the attributes after them, or a bare class
/// (`::: warning`), and closes one with nothing after them.
pub(super) fn fences(text: &str) -> Vec<Fence> {
    let mut out = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let body = bare.trim_start();
        let colons = body.len() - body.trim_start_matches(':').len();
        if colons >= 3 {
            let rest = body[colons..].trim();
            let valid = rest.is_empty() || rest.starts_with('{') || rest.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if valid {
                let open = (!rest.is_empty()).then(|| DivAttrs::parse(rest));
                out.push(Fence { range: at..at + bare.len(), open });
            }
        }
        at += line.len();
    }
    out
}

/// What a paragraph was cut into at its fences.
pub(super) enum Piece {
    Open(DivAttrs, Option<Span>),
    Close(Option<Span>),
    Block(BlockNode),
}

/// The pieces nested: what stands between an opening fence and its
/// close becomes one `Div`. A close with nothing open is dropped, and
/// what is open at the end is closed there.
pub(super) fn nest(pieces: Vec<Piece>) -> Vec<BlockNode> {
    let mut stack: Vec<(DivAttrs, Option<Span>, Vec<BlockNode>)> = Vec::new();
    let mut out: Vec<BlockNode> = Vec::new();
    let push = |stack: &mut Vec<(DivAttrs, Option<Span>, Vec<BlockNode>)>, out: &mut Vec<BlockNode>, block: BlockNode| match stack.last_mut() {
        Some((_, _, children)) => children.push(block),
        None => out.push(block),
    };
    for piece in pieces {
        match piece {
            Piece::Open(attrs, span) => stack.push((attrs, span, Vec::new())),
            Piece::Close(close) => {
                if let Some((attrs, open, children)) = stack.pop() {
                    let span = match (open, close) {
                        (Some(a), Some(b)) => Some(Span { start: a.start, end: b.end }),
                        (a, b) => a.or(b),
                    };
                    let chip = Paragraph::new(format!("{{{}}}", attrs.text));
                    push(&mut stack, &mut out, BlockNode::Div { attrs, chip, children, span });
                }
            }
            Piece::Block(block) => push(&mut stack, &mut out, block),
        }
    }
    while let Some((attrs, span, children)) = stack.pop() {
        let end = children.last().and_then(|c| c.span()).map(|s| s.end);
        let span = span.map(|s| Span { start: s.start, end: end.unwrap_or(s.end) });
        let chip = Paragraph::new(format!("{{{}}}", attrs.text));
        push(&mut stack, &mut out, BlockNode::Div { attrs, chip, children, span });
    }
    out
}

/// How far an attribute set at the start of `text` runs: the index after
/// its closing brace, braces inside quotes not counted.
pub(super) fn attrs_end(text: &str) -> Option<usize> {
    if !text.starts_with('{') {
        return None;
    }
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (ix, c) in text.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '{' => depth += 1,
            None if c == '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(ix + 1);
                }
            }
            None => {}
        }
    }
    None
}

/// The attributes found after a picture, a link or inline code, applied:
/// a picture's width and place; raw inline HTML drawn as HTML; a link's
/// dropped.
pub(super) fn apply_inline_attrs(paragraph: &mut Paragraph, before: &mdast::Node, attrs: &str, cx: &mut NodeContext) {
    match before {
        mdast::Node::Image(_) => {
            let attrs = DivAttrs::parse(attrs);
            if let Some(image) = paragraph.children.iter_mut().rev().find_map(|n| n.image.as_mut()) {
                if let Some(width) = attrs.value("width").and_then(length) {
                    image.width = Some(width);
                }
                if let Some(height) = attrs.value("height").and_then(length) {
                    image.height = Some(height);
                }
                image.align = match attrs.value("fig-align") {
                    Some("center") => Some(ImageAlign::Center),
                    Some("right") => Some(ImageAlign::Right),
                    Some("left") => Some(ImageAlign::Left),
                    _ => None,
                };
                if let Some(alt) = attrs.value("fig-alt") {
                    if image.alt.as_ref().is_none_or(|a| a.is_empty()) {
                        image.title = Some(alt.to_string().into());
                    }
                }
            }
        }
        mdast::Node::InlineCode(code) if attrs == "{=html}" => {
            // The code node pushed for it goes; the HTML takes its place.
            if paragraph.children.last().is_some_and(|n| n.marks.iter().any(|(_, m)| m.code)) {
                paragraph.children.pop();
            }
            match super::html::parse(&code.value, cx) {
                Ok(el) => {
                    if super::markdown::append_inline_html_blocks(paragraph, el.blocks).is_none() {
                        paragraph.push_str(&code.value);
                    }
                }
                Err(_) => paragraph.push_str(&code.value),
            }
        }
        _ => {}
    }
}

/// A length as Quarto writes one: pixels (bare or `px`), a share (`%`),
/// inches, centimeters, millimeters, ems.
fn length(text: &str) -> Option<DefiniteLength> {
    let text = text.trim();
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let n: f32 = text[..split].parse().ok()?;
    Some(match text[split..].trim() {
        "" | "px" => px(n).into(),
        "%" => relative(n / 100.),
        "in" => px(n * 96.).into(),
        "cm" => px(n * 96. / 2.54).into(),
        "mm" => px(n * 96. / 25.4).into(),
        "em" | "rem" => rems(n).into(),
        _ => return None,
    })
}

/// A text run of a paragraph, with Quarto's inline syntax read: a span's
/// words without its attributes, an empty span gone, and a shortcode as
/// a quiet chip. What was pushed is returned as plain text.
pub(super) fn push_text(paragraph: &mut Paragraph, text: &str) -> String {
    use crate::text::style::CODE_PAD;
    static SPAN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"\[([^\[\]]*)\]\{[^{}]*\}").unwrap());
    static CODE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"\{\{<\s*(.*?)\s*>\}\}").unwrap());
    let mut text = text.to_string();
    // A span cut by a mark inside it (`[a *b*]{.c}`): its end is the
    // start of this run, its start in a run before.
    if text.starts_with("]{") {
        if let Some(end) = attrs_end(&text[1..]) {
            text.replace_range(..1 + end, "");
            if let Some(node) = paragraph.children.iter_mut().rev().filter(|n| n.image.is_none()).find(|n| n.text.rfind('[').is_some_and(|at| !n.text[at..].contains(']'))) {
                let at = node.text.rfind('[').unwrap();
                let mut words = node.text.to_string();
                words.remove(at);
                node.text = words.into();
                for (range, _) in node.marks.iter_mut() {
                    if range.end > at {
                        range.end -= 1;
                    }
                    if range.start > at {
                        range.start -= 1;
                    }
                }
            }
        }
    }
    let text = SPAN.replace_all(&text, "$1").into_owned();
    let mut out = String::new();
    let mut at = 0;
    for cap in CODE.captures_iter(&text) {
        let whole = cap.get(0).unwrap();
        let plain = &text[at..whole.start()];
        if !plain.is_empty() {
            paragraph.push_str(plain);
            out.push_str(plain);
        }
        let words = cap.get(1).map_or("", |m| m.as_str());
        let lead = CODE_PAD.len_utf8() * 2;
        let padded = format!("{CODE_PAD}{CODE_PAD}{words}{CODE_PAD}{CODE_PAD}");
        paragraph.push(InlineNode::new(&padded).marks(vec![(lead..padded.len() - lead, TextMark::default().attr())]));
        out.push_str(&padded);
        at = whole.end();
    }
    // Nothing left (the attributes were all of the run) pushes nothing:
    // an empty run after a picture made it no longer stand alone.
    if at < text.len() {
        paragraph.push_str(&text[at..]);
        out.push_str(&text[at..]);
    }
    out
}

/// The attributes a heading ends with (`## Title {.class}`), taken off
/// its words.
pub(super) fn take_heading_attrs(paragraph: &mut Paragraph) -> Option<Paragraph> {
    let last = paragraph.children.last_mut()?;
    // Plain words only: a run `push_str` made carries one mark of nothing.
    if last.image.is_some() || last.marks.iter().any(|(_, m)| *m != TextMark::default()) {
        return None;
    }
    let trimmed = last.text.trim_end();
    if !trimmed.ends_with('}') {
        return None;
    }
    let open = trimmed.rfind('{')?;
    if open > 0 && !trimmed[..open].ends_with(char::is_whitespace) {
        return None;
    }
    if attrs_end(&trimmed[open..]) != Some(trimmed.len() - open) {
        return None;
    }
    let attrs = DivAttrs::parse(&trimmed[open..]).text;
    last.text = trimmed[..open].trim_end().to_string().into();
    let len = last.text.len();
    for (range, _) in last.marks.iter_mut() {
        range.end = range.end.min(len);
        range.start = range.start.min(len);
    }
    last.marks.retain(|(r, _)| !r.is_empty());
    if last.text.is_empty() {
        paragraph.children.pop();
    }
    Some(Paragraph::new(format!("{{{attrs}}}")))
}

/// A picture standing alone in its paragraph is a figure: its alt text
/// is the caption under it.
pub(super) fn caption_lone_figure(paragraph: &mut Paragraph) {
    let [only] = paragraph.children.as_mut_slice() else { return };
    if let Some(image) = only.image.as_mut() {
        if let Some(alt) = image.alt.as_ref().filter(|a| !a.trim().is_empty()) {
            image.caption = Some(alt.clone());
        }
    }
}

/// A block of code that is a Quarto raw block of HTML (```` ```{=html} ````).
pub(super) fn raw_html(lang: Option<&str>) -> bool {
    lang.is_some_and(|l| l.trim_start_matches('{').trim_end_matches('}').trim().eq_ignore_ascii_case("=html"))
}

/// The kinds of Quarto's theorem-like divs, by their ids' prefixes
/// (`::: {#thm-x}`): drawn as their words, with no chip.
pub(crate) const THEOREM_KINDS: &[&str] = &["thm", "lem", "cor", "prp", "cnj", "def", "exm", "exr", "sol", "rem"];

/// The class a row of the margin layout carries, which no document can
/// write: a block of the text and the margin notes beside it.
pub(crate) const MARGIN_ROW: &str = "\u{1}margin-row";

/// Whether a block is a margin note: `::: {.column-margin}` or
/// `::: {.aside}` (a span `[words]{.aside}` is one by the time the view
/// reads it).
pub(crate) fn is_margin(block: &BlockNode) -> bool {
    matches!(block, BlockNode::Div { attrs, .. } if attrs.has_class("column-margin") || attrs.has_class("aside"))
}

/// Whether a div is one of Quarto's column classes (`.column-page`,
/// `.column-screen-inset`…), which only say how wide it stands.
pub(crate) fn is_column(attrs: &DivAttrs) -> bool {
    attrs.classes.iter().any(|c| c.starts_with("column-") && c != "column-margin") || attrs.has_class("page-columns") || attrs.has_class("page-full")
}

/// A document with margin notes is set in two columns, as Quarto sets it:
/// each block of the text is a row with the notes that come just before
/// it beside it (a note stands level with what follows it in the source),
/// the last notes beside the last block, and a column-class div across
/// both. A document with none is left as it is.
pub(super) fn margin_rows(blocks: Vec<BlockNode>) -> Vec<BlockNode> {
    if !blocks.iter().any(is_margin) {
        return blocks;
    }
    let row = |children: Vec<BlockNode>| {
        let start = children.iter().filter_map(|c| c.span()).map(|s| s.start).min();
        let end = children.iter().filter_map(|c| c.span()).map(|s| s.end).max();
        let span = start.zip(end).map(|(start, end)| Span { start, end });
        BlockNode::Div { attrs: DivAttrs { classes: vec![MARGIN_ROW.into()], ..Default::default() }, chip: Paragraph::default(), children, span }
    };
    let mut out = Vec::new();
    let mut notes = Vec::new();
    for block in blocks {
        if is_margin(&block) {
            notes.push(block);
            continue;
        }
        if matches!(&block, BlockNode::Div { attrs, .. } if is_column(attrs)) {
            if !notes.is_empty() {
                out.push(row(std::iter::once(BlockNode::Unknown).chain(notes.drain(..)).collect()));
            }
            out.push(block);
            continue;
        }
        out.push(row(std::iter::once(block).chain(notes.drain(..)).collect()));
    }
    if !notes.is_empty() {
        match out.last_mut() {
            Some(BlockNode::Div { attrs, children, .. }) if attrs.has_class(MARGIN_ROW) => children.extend(notes),
            _ => out.push(row(std::iter::once(BlockNode::Unknown).chain(notes).collect())),
        }
    }
    out
}

/// The class a display equation's `Div` carries, which no document can
/// write: `render_div` centers what it holds, with no chip or rule.
pub(crate) const MATH_CLASS: &str = "\u{1}math";

/// A formula, as the view's math face sets it (`text/math.rs`): the room
/// its picture is painted over, or, where it is not set, its Unicode in
/// italics.
fn math_node(value: &str, display: bool, cx: &NodeContext) -> InlineNode {
    let (src, tag) = if display { untag(value) } else { (value.trim().to_string(), None) };
    let face = cx.markdown_extensions.math_face();
    let picture = face.and_then(|f| (f.set)(&src, display));
    let math = |picture| MathSource { src: src.clone().into(), display, tag: tag.clone(), picture };
    match (face, picture) {
        (Some(face), Some(picture)) => InlineNode::new(room(picture.width, face.glue)).with_math(math(Some(picture))),
        _ => {
            let words = tex(value.trim());
            InlineNode::new(&words).marks(vec![(0..words.len(), TextMark::default().italic())]).with_math(math(None))
        }
    }
}

/// Math in the line.
pub(super) fn inline_math(value: &str, cx: &NodeContext) -> InlineNode {
    math_node(value, false, cx)
}

/// Math on a line of its own, centered.
pub(super) fn display_math(value: &str, span: Option<Span>, cx: &NodeContext) -> BlockNode {
    let mut paragraph = Paragraph::default();
    paragraph.push(math_node(value, true, cx));
    paragraph.span = span;
    BlockNode::Div { attrs: DivAttrs { classes: vec![MATH_CLASS.into()], ..Default::default() }, chip: Paragraph::default(), children: vec![BlockNode::Paragraph(paragraph)], span }
}

/// Whether `$…$` holding `value` is math as Pandoc reads it: no space
/// just inside either dollar. "$5 and $10" is money, not math.
pub(super) fn is_math(value: &str) -> bool {
    !value.is_empty() && !value.starts_with(char::is_whitespace) && !value.ends_with(char::is_whitespace)
}

/// TeX math as Unicode, for a view with no math typesetting: Greek and
/// the operators as their characters, `^` and `_` as super- and
/// subscripts where Unicode has them all (else `^(…)`), a fraction as
/// `a/b`, a root as `√x`, `\mathbb{R}` as `ℝ`, an accent as a combining
/// mark, text and function names as they are, and the sizing and spacing
/// commands as nothing or a space. `\tag{n}` is `(n)` after a gap.
pub(crate) fn tex(src: &str) -> String {
    let mut p = Tex { s: src.chars().collect(), i: 0 };
    let out = p.seq(None);
    // TeX ignores the spaces typed in math; one is kept where any stood.
    let mut tidy = String::with_capacity(out.len());
    for c in out.chars() {
        if c == ' ' && (tidy.is_empty() || tidy.ends_with([' ', '\n'])) {
            continue;
        }
        tidy.push(c);
    }
    tidy.trim_end().to_string()
}

struct Tex {
    s: Vec<char>,
    i: usize,
}

impl Tex {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    /// Up to `until` (taken) or the end.
    fn seq(&mut self, until: Option<char>) -> String {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if Some(c) == until {
                self.i += 1;
                break;
            }
            self.i += 1;
            match c {
                '\\' => out.push_str(&self.command()),
                '{' => out.push_str(&self.seq(Some('}'))),
                '}' => {}
                '^' | '_' => {
                    let arg = self.arg();
                    out.push_str(&script(&arg, c == '^'));
                }
                '&' | '~' | '\n' | '\t' => out.push(' '),
                '-' => out.push('−'),
                '\'' => out.push('′'),
                c => out.push(c),
            }
        }
        out
    }

    /// One argument: a group, a command, or a character.
    fn arg(&mut self) -> String {
        while self.peek() == Some(' ') {
            self.i += 1;
        }
        match self.peek() {
            Some('{') => {
                self.i += 1;
                self.seq(Some('}'))
            }
            Some('\\') => {
                self.i += 1;
                self.command()
            }
            Some(c) => {
                self.i += 1;
                if c == '-' { "−".into() } else { c.to_string() }
            }
            None => String::new(),
        }
    }

    /// An optional `[…]` argument.
    fn optional(&mut self) -> Option<String> {
        if self.peek() != Some('[') {
            return None;
        }
        self.i += 1;
        let start = self.i;
        while self.peek().is_some_and(|c| c != ']') {
            self.i += 1;
        }
        let out: String = self.s[start..self.i].iter().collect();
        self.i += 1;
        Some(out)
    }

    /// After a backslash.
    fn command(&mut self) -> String {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i == start {
            let c = self.peek();
            self.i += 1;
            return match c {
                Some('\\') => "\n".into(),
                Some(',' | ';' | ':' | ' ') => " ".into(),
                Some('!') | None => String::new(),
                Some(c) => c.to_string(),
            };
        }
        let name: String = self.s[start..self.i].iter().collect();
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let (a, b) = (self.arg(), self.arg());
                format!("{}/{}", wrap(&a), wrap(&b))
            }
            "binom" => {
                let (a, b) = (self.arg(), self.arg());
                format!("C({a}, {b})")
            }
            "sqrt" => {
                let n = self.optional();
                let a = self.arg();
                let root = match n.as_deref() {
                    Some("3") => "∛",
                    Some("4") => "∜",
                    _ => "√",
                };
                format!("{root}{}", wrap(&a))
            }
            "text" | "textrm" | "textit" | "textbf" | "texttt" | "textsf" | "mathrm" | "mathit" | "mathbf" | "mathsf" | "mathtt" | "boldsymbol" | "bm" | "operatorname" | "mbox" | "mathcal" | "mathscr" | "mathfrak" => self.arg(),
            "mathbb" => self.arg().chars().map(double_struck).collect(),
            "hat" | "widehat" => accent(self.arg(), '\u{0302}'),
            "bar" | "overline" => accent(self.arg(), '\u{0305}'),
            "vec" | "overrightarrow" => accent(self.arg(), '\u{20D7}'),
            "dot" => accent(self.arg(), '\u{0307}'),
            "ddot" => accent(self.arg(), '\u{0308}'),
            "tilde" | "widetilde" => accent(self.arg(), '\u{0303}'),
            "underline" => self.arg(),
            "left" | "right" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "big" | "Big" | "bigg" | "Bigg" | "middle" => {
                if self.peek() == Some('.') {
                    self.i += 1;
                }
                String::new()
            }
            "displaystyle" | "textstyle" | "scriptstyle" | "limits" | "nolimits" | "nonumber" | "notag" => String::new(),
            "begin" | "end" => {
                let _ = self.arg();
                let _ = self.optional();
                " ".into()
            }
            "label" => {
                let _ = self.arg();
                String::new()
            }
            "tag" => format!("\u{2003}\u{2003}({})", self.arg()),
            "quad" => "\u{2003}".into(),
            "qquad" => "\u{2003}\u{2003}".into(),
            "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "arcsin" | "arccos" | "arctan" | "sinh" | "cosh" | "tanh" | "log" | "ln" | "lg" | "exp" | "lim" | "liminf" | "limsup" | "max" | "min" | "sup" | "inf" | "det" | "arg" | "deg" | "dim" | "gcd" | "ker" | "hom" | "Pr" | "mod" | "bmod" | "argmax" | "argmin" => name,
            other => symbol(other).map(str::to_string).unwrap_or(name),
        }
    }
}

/// A fraction's part as it can stand before or after a slash.
fn wrap(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 1 || s.chars().all(|c| c.is_alphanumeric() || "²³′.".contains(c)) {
        s.to_string()
    } else {
        format!("({s})")
    }
}

fn accent(arg: String, mark: char) -> String {
    let mut chars = arg.chars();
    match chars.next() {
        Some(first) => format!("{first}{mark}{}", chars.as_str()),
        None => String::new(),
    }
}

fn double_struck(c: char) -> char {
    match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        'E' => '𝔼',
        '1' => '𝟙',
        c => c,
    }
}

/// `arg` raised or lowered: in Unicode's super- or subscripts where it
/// has every character, else `^x` or `^(…)` as it is written plainly.
fn script(arg: &str, up: bool) -> String {
    let arg = arg.trim();
    let mapped: Option<String> = arg
        .chars()
        .filter(|c| *c != ' ')
        .map(|c| if up { superscript(c) } else { subscript(c) })
        .collect();
    match mapped {
        Some(m) if !m.is_empty() => m,
        _ => {
            let sign = if up { '^' } else { '_' };
            if arg.chars().count() == 1 { format!("{sign}{arg}") } else { format!("{sign}({arg})") }
        }
    }
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
        '+' => '⁺', '−' | '-' => '⁻', '=' => '⁼', '(' => '⁽', ')' => '⁾', '′' => '′', '*' | '∗' => '*',
        'a' => 'ᵃ', 'b' => 'ᵇ', 'c' => 'ᶜ', 'd' => 'ᵈ', 'e' => 'ᵉ', 'f' => 'ᶠ', 'g' => 'ᵍ', 'h' => 'ʰ', 'i' => 'ⁱ', 'j' => 'ʲ', 'k' => 'ᵏ', 'l' => 'ˡ', 'm' => 'ᵐ',
        'n' => 'ⁿ', 'o' => 'ᵒ', 'p' => 'ᵖ', 'r' => 'ʳ', 's' => 'ˢ', 't' => 'ᵗ', 'u' => 'ᵘ', 'v' => 'ᵛ', 'w' => 'ʷ', 'x' => 'ˣ', 'y' => 'ʸ', 'z' => 'ᶻ',
        'T' => 'ᵀ', 'θ' => 'ᶿ',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
        '+' => '₊', '−' | '-' => '₋', '=' => '₌', '(' => '₍', ')' => '₎',
        'a' => 'ₐ', 'e' => 'ₑ', 'h' => 'ₕ', 'i' => 'ᵢ', 'j' => 'ⱼ', 'k' => 'ₖ', 'l' => 'ₗ', 'm' => 'ₘ', 'n' => 'ₙ', 'o' => 'ₒ', 'p' => 'ₚ',
        'r' => 'ᵣ', 's' => 'ₛ', 't' => 'ₜ', 'u' => 'ᵤ', 'v' => 'ᵥ', 'x' => 'ₓ', 'β' => 'ᵦ', 'γ' => 'ᵧ', 'ρ' => 'ᵨ', 'φ' => 'ᵩ', 'χ' => 'ᵪ',
        _ => return None,
    })
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ", "epsilon" => "ϵ", "varepsilon" => "ε", "zeta" => "ζ", "eta" => "η",
        "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ", "lambda" => "λ", "mu" => "μ", "nu" => "ν", "xi" => "ξ",
        "pi" => "π", "varpi" => "ϖ", "rho" => "ρ", "varrho" => "ϱ", "sigma" => "σ", "varsigma" => "ς", "tau" => "τ", "upsilon" => "υ",
        "phi" => "ϕ", "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ", "Xi" => "Ξ", "Pi" => "Π", "Sigma" => "Σ", "Upsilon" => "Υ",
        "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω",
        "sum" => "∑", "prod" => "∏", "coprod" => "∐", "int" => "∫", "iint" => "∬", "iiint" => "∭", "oint" => "∮",
        "partial" => "∂", "nabla" => "∇", "infty" => "∞", "pm" => "±", "mp" => "∓", "times" => "×", "div" => "÷",
        "cdot" => "⋅", "ast" => "∗", "star" => "⋆", "circ" => "∘", "bullet" => "•", "oplus" => "⊕", "otimes" => "⊗", "odot" => "⊙",
        "leq" | "le" => "≤", "geq" | "ge" => "≥", "neq" | "ne" => "≠", "approx" => "≈", "equiv" => "≡", "sim" => "∼", "simeq" => "≃",
        "cong" => "≅", "propto" => "∝", "ll" => "≪", "gg" => "≫", "doteq" => "≐", "prec" => "≺", "succ" => "≻", "preceq" => "⪯", "succeq" => "⪰",
        "to" | "rightarrow" => "→", "leftarrow" | "gets" => "←", "Rightarrow" => "⇒", "Leftarrow" => "⇐", "leftrightarrow" => "↔",
        "Leftrightarrow" | "iff" => "⇔", "mapsto" => "↦", "implies" => "⟹", "impliedby" => "⟸", "longrightarrow" => "⟶", "longleftarrow" => "⟵",
        "uparrow" => "↑", "downarrow" => "↓", "Uparrow" => "⇑", "Downarrow" => "⇓", "nearrow" => "↗", "searrow" => "↘",
        "in" => "∈", "notin" => "∉", "ni" => "∋", "subset" => "⊂", "subseteq" => "⊆", "supset" => "⊃", "supseteq" => "⊇",
        "cup" => "∪", "cap" => "∩", "bigcup" => "⋃", "bigcap" => "⋂", "setminus" => "∖", "emptyset" | "varnothing" => "∅",
        "forall" => "∀", "exists" => "∃", "nexists" => "∄", "neg" | "lnot" => "¬", "land" | "wedge" => "∧", "lor" | "vee" => "∨",
        "perp" | "bot" => "⊥", "top" => "⊤", "parallel" => "∥", "angle" => "∠", "triangle" => "△", "square" => "□",
        "cdots" => "⋯", "ldots" | "dots" | "dotsc" | "dotsb" => "…", "vdots" => "⋮", "ddots" => "⋱",
        "prime" => "′", "degree" => "°", "ell" => "ℓ", "hbar" => "ℏ", "Re" => "ℜ", "Im" => "ℑ", "aleph" => "ℵ", "wp" => "℘",
        "langle" => "⟨", "rangle" => "⟩", "lfloor" => "⌊", "rfloor" => "⌋", "lceil" => "⌈", "rceil" => "⌉",
        "mid" | "vert" => "|", "Vert" | "|" => "‖", "lbrace" => "{", "rbrace" => "}", "backslash" => "\\", "colon" => ":",
        "therefore" => "∴", "because" => "∵", "dagger" => "†", "ddagger" => "‡", "checkmark" => "✓", "cdotp" => "·",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::markdown::parse;
    use crate::text::node::{BlockNode, ImageAlign, NodeContext};
    use crate::text::MarkdownExtensions;

    fn blocks(text: &str) -> Vec<BlockNode> {
        let mut cx = NodeContext { markdown_extensions: MarkdownExtensions::default().quarto().into(), ..NodeContext::default() };
        parse(text, &mut cx).unwrap().blocks
    }

    /// A face that sets every formula a picture as wide as its source is
    /// long, in ems, and the toolkit's own room maker.
    fn with_face(text: &str) -> Vec<BlockNode> {
        use crate::text::{MathFace, MathPicture};
        let face = MathFace {
            family: "Math".into(),
            glue: (0.332, 0.2),
            set: std::sync::Arc::new(|src, _| (!src.contains("bad")).then(|| MathPicture { svg: std::sync::Arc::from(&b"<svg/>"[..]), width: src.len() as f32 * 0.5, height: 1.2, baseline: 0.9 })),
        };
        let mut cx = NodeContext { markdown_extensions: MarkdownExtensions::default().quarto().math(face).into(), ..NodeContext::default() };
        parse(text, &mut cx).unwrap().blocks
    }

    #[test]
    fn a_formula_set_is_a_room_in_its_line_and_copies_as_written() {
        let b = with_face("Let $x^2$ be, and $bad$ is words.\n\n$$\na = b \\tag{3}\n$$\n");
        let BlockNode::Paragraph(p) = &b[0] else { panic!("{:?}", b[0]) };
        let math: Vec<_> = p.children.iter().filter_map(|c| c.math.as_ref().map(|m| (c.text.to_string(), m.clone()))).collect();
        assert_eq!(math.len(), 2, "{p:?}");
        // Set: its room is no-break spaces near its width (3 ems × 0.5).
        assert!(math[0].0.chars().all(|c| c == '\u{A0}' || c == '\u{202F}'), "{:?}", math[0].0);
        assert!(math[0].1.picture.is_some());
        assert_eq!(math[0].1.markdown(), "$x^2$");
        // Not set: its words.
        assert!(math[1].1.picture.is_none());
        assert_eq!(math[1].0, "bad");
        // On a line of its own: the number comes out of the formula.
        let BlockNode::Div { children, .. } = &b[1] else { panic!("{:?}", b[1]) };
        let BlockNode::Paragraph(d) = &children[0] else { panic!() };
        let m = d.children[0].math.as_ref().unwrap();
        assert!(m.display && m.picture.is_some());
        assert_eq!(m.src.as_ref(), "a = b");
        assert_eq!(m.tag.as_deref(), Some("3"));
    }

    #[test]
    fn margin_notes_stand_beside_what_follows_them() {
        use super::MARGIN_ROW;
        let b = blocks("Intro.\n\n::: {.column-margin}\nA note.\n:::\n\nBeside it.\n\n::: {.column-page}\nWide.\n:::\n\n::: {.aside}\nLast.\n:::\n");
        let rows: Vec<_> = b.iter().map(|x| match x {
            BlockNode::Div { attrs, children, .. } if attrs.has_class(MARGIN_ROW) => format!("row:{}", children.len()),
            BlockNode::Div { attrs, .. } => attrs.classes.join(","),
            _ => "other".into(),
        }).collect();
        // Intro alone, Beside it with its note, the wide div across both,
        // and the last note in a row of its own after it.
        assert_eq!(rows, ["row:1", "row:2", "column-page", "row:2"], "{b:#?}");
        // A document with no notes is left as it is.
        assert!(blocks("One.\n\nTwo.\n").iter().all(|x| matches!(x, BlockNode::Paragraph(_))));
    }

    #[test]
    fn fenced_divs_nest_and_cut_paragraphs() {
        let b = blocks("Before.\n\n::: {.about-more}\n[All research](research.qmd)\n:::\n\nAfter.\n\n:::: {#outer .wide}\n\n::: warning\nInside.\n:::\n\nTail.\n\n::::\n");
        assert_eq!(b.len(), 4, "{b:#?}");
        let BlockNode::Div { attrs, children, .. } = &b[1] else { panic!("{:?}", b[1]) };
        assert_eq!(attrs.classes, ["about-more"]);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].text().trim(), "All research");
        let BlockNode::Div { attrs, children, .. } = &b[3] else { panic!("{:?}", b[3]) };
        assert_eq!(attrs.id.as_deref(), Some("outer"));
        assert_eq!(children.len(), 2);
        let BlockNode::Div { attrs, .. } = &children[0] else { panic!("{:?}", children[0]) };
        assert_eq!(attrs.classes, ["warning"]);
        assert_eq!(children[1].text().trim(), "Tail.");
    }

    #[test]
    fn heading_attributes_come_off_the_words() {
        let b = blocks("## Research {.about-label}\n\n### Plain\n");
        let BlockNode::Heading { children, attrs, .. } = &b[0] else { panic!() };
        assert_eq!(children.text(), "Research");
        assert_eq!(attrs.as_ref().map(|a| a.text()).as_deref(), Some("{.about-label}"));
        let BlockNode::Heading { attrs, .. } = &b[1] else { panic!() };
        assert_eq!(*attrs, None);
    }

    #[test]
    fn spans_unwrap_and_shortcodes_become_chips() {
        let b = blocks("A [word]{.glyph} and []{.glyph .r aria-hidden=\"true\"}none, a split [a *b*]{.c}, then {{< video https://x >}}.");
        let BlockNode::Paragraph(p) = &b[0] else { panic!() };
        let text = p.text().replace('\u{202F}', "");
        assert_eq!(text, "A word and none, a split a b, then video https://x.");
        assert!(p.children.iter().any(|n| n.marks.iter().any(|(_, m)| m.attr)), "{p:#?}");
        // A link's attributes go; a span's text keeps its own marks.
        let b = blocks("[Quarto](https://quarto.org/){.about-tool} and [*it*]{.x}");
        let BlockNode::Paragraph(p) = &b[0] else { panic!() };
        assert_eq!(p.text().trim(), "Quarto and it");
        assert!(p.children.iter().any(|n| n.marks.iter().any(|(_, m)| m.italic)));
    }

    #[test]
    fn a_picture_takes_its_width_place_and_caption() {
        let b = blocks("![*Pinocchio*](/img/cat.png){fig-alt=\"A cat\" fig-align=\"center\" width=\"600\"}\n");
        let BlockNode::Paragraph(p) = &b[0] else { panic!() };
        assert_eq!(p.children.len(), 1, "{p:#?}");
        let image = p.children[0].image.as_ref().unwrap();
        assert_eq!(image.align, Some(ImageAlign::Center));
        assert_eq!(image.caption.as_deref(), Some("Pinocchio"));
        assert!(image.width.is_some());
        // In a line of words it is no figure: no caption.
        let b = blocks("See ![alt](a.png){width=\"50%\"} here.");
        let BlockNode::Paragraph(p) = &b[0] else { panic!() };
        let image = p.children.iter().find_map(|n| n.image.as_ref()).unwrap();
        assert_eq!(image.caption, None);
        assert_eq!(p.text().trim(), "See  here.");
    }

    #[test]
    fn raw_html_is_html_and_other_raw_stays_code() {
        let b = blocks("```{=html}\n<div class=\"x\"><a href=\"cv.pdf\">CV</a></div>\n```\n\nIn a line `<b>bold</b>`{=html} here.\n\n```{=latex}\n\\x\n```\n");
        assert!(matches!(&b[0], BlockNode::Root { .. }), "{:?}", b[0]);
        assert_eq!(b[0].text().trim(), "CV");
        let BlockNode::Paragraph(p) = &b[1] else { panic!() };
        assert_eq!(p.text().trim(), "In a line bold here.");
        assert!(p.children.iter().any(|n| n.marks.iter().any(|(_, m)| m.bold)));
        let BlockNode::CodeBlock(code) = &b[2] else { panic!() };
        assert_eq!(code.lang().as_deref(), Some("{=latex}"));
    }

    #[test]
    fn plain_markdown_is_left_alone_without_the_flag() {
        let mut cx = NodeContext::default();
        let b = parse("## Title {.x}\n\n::: {.d}\nwords\n:::\n", &mut cx).unwrap().blocks;
        let BlockNode::Heading { children, attrs, .. } = &b[0] else { panic!() };
        assert_eq!(children.text(), "Title {.x}");
        assert_eq!(*attrs, None);
        assert!(matches!(&b[1], BlockNode::Paragraph(_)));
    }

    #[test]
    fn tex_reads_as_unicode() {
        use super::{is_math, tex};
        assert_eq!(tex("E = mc^2"), "E = mc²");
        assert_eq!(tex(r"\frac{a+b}{2} \leq \sqrt{x_i^{2}}"), "(a+b)/2 ≤ √xᵢ²");
        assert_eq!(tex(r"\sum_{i=1}^{n} \alpha_i"), "∑ᵢ₌₁ⁿ αᵢ");
        assert_eq!(tex(r"\mathbb{R}^n \to \mathbb{R}"), "ℝⁿ → ℝ");
        assert_eq!(tex(r"\hat{\beta} = (X^T X)^{-1} X^T y"), "β̂ = (Xᵀ X)⁻¹ Xᵀ y");
        assert_eq!(tex(r"\lim_{x \to 0} \frac{\sin x}{x} = 1"), "lim_(x → 0) (sin x)/x = 1");
        assert_eq!(tex(r"\text{if } x > 0 \tag{2}"), "if x > 0 \u{2003}\u{2003}(2)");
        assert_eq!(tex(r"\left( \frac{1}{2} \right)"), "( 1/2 )");
        assert!(is_math("x^2") && !is_math("5 and ") && !is_math(""));
    }
}
