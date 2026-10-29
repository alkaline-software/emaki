use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    sync::{Arc, Mutex},
};

use gpui::{
    AnyElement, App, DefiniteLength, Div, ElementId, FontStyle, FontWeight, Half, HighlightStyle,
    Hsla, InteractiveElement as _, IntoElement, Length, ObjectFit, Overflow, ParentElement,
    ScrollHandle, SharedString, SharedUri, StatefulInteractiveElement, Styled, StyledImage as _,
    WhiteSpace, Window, div, img, linear_color_stop, linear_gradient,
    prelude::FluentBuilder as _, px, relative, rems,
};
use markdown::mdast;
use ropey::Rope;

use crate::{
    ActiveTheme as _, Icon, IconName, StyledExt, h_flex,
    highlighter::{HighlightTheme, LanguageRegistry, SyntaxHighlighter},
    input::{InputEdit, Point, RopeExt as _},
    scroll::{ScrollableElement as _, horizontal_scroll_area},
    text::{
        CodeBlockActionsFn, LinkClickHandlerFn, MarkdownExtensions, MarkdownNode,
        document::NodeRenderOptions,
        inline::{Inline, InlineState},
        inline_flow::{InlineFlow, InlineFlowItem},
        text_view::handle_link_click,
    },
    tooltip::Tooltip,
    v_flex,
};

use super::{
    SelectionFormat, TextViewStyle,
    utils::{image_source, list_item_prefix},
};

thread_local! {
    static CODE_BLOCK_HIGHLIGHTERS: RefCell<HashMap<SharedString, SyntaxHighlighter>> =
        RefCell::new(HashMap::new());
}

/// The block-level nodes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BlockNode {
    /// Something like a Div container in HTML.
    Root {
        children: Vec<BlockNode>,
        span: Option<Span>,
    },
    Paragraph(Paragraph),
    Heading {
        level: u8,
        children: Paragraph,
        span: Option<Span>,
        /// Quarto's attributes after the words (`## Title {.class}`), as
        /// written, braces and all: words of the document's own, so they
        /// select and copy with the rest. (Emaki addition.)
        attrs: Option<Paragraph>,
    },
    Blockquote {
        children: Vec<BlockNode>,
        span: Option<Span>,
    },
    /// A Quarto fenced div (`::: {.class}` … `:::`): what it holds, its
    /// attributes, and them as written, for the chip that shows them.
    /// (Emaki addition.)
    Div {
        attrs: DivAttrs,
        chip: Paragraph,
        children: Vec<BlockNode>,
        span: Option<Span>,
    },
    List {
        /// Only contains ListItem, others will be ignored
        children: Vec<BlockNode>,
        ordered: bool,
        span: Option<Span>,
    },
    ListItem {
        children: Vec<BlockNode>,
        spread: bool,
        /// Whether the list item is checked, if None, it's not a checkbox
        checked: Option<bool>,
        span: Option<Span>,
    },
    CodeBlock(CodeBlock),
    /// A custom Markdown node produced by [`MarkdownExtensions`].
    Custom(MarkdownNode),
    Table(Table),
    Break {
        html: bool,
        span: Option<Span>,
    },
    HorizontalRule {
        span: Option<Span>,
    },
    /// Use for to_markdown get raw definition
    Definition {
        identifier: SharedString,
        url: SharedString,
        title: Option<SharedString>,
        span: Option<Span>,
    },
    Unknown,
}

#[derive(Clone, Copy)]
enum BlockTextKind {
    All,
    Selected,
    /// Like `Selected`, but reconstructs Markdown source for the selection
    /// instead of the rendered plain text.
    SelectedSource,
}

impl BlockNode {
    pub(super) fn is_list_item(&self) -> bool {
        matches!(self, Self::ListItem { .. })
    }

    /// Combine all children, omitting the empt parent nodes.
    pub(super) fn compact(self) -> BlockNode {
        match self {
            Self::Root { mut children, .. } if children.len() == 1 => children.remove(0).compact(),
            _ => self,
        }
    }

    /// Get the span of the node.
    pub(crate) fn span(&self) -> Option<Span> {
        match self {
            BlockNode::Root { span, .. } => *span,
            BlockNode::Paragraph(paragraph) => paragraph.span,
            BlockNode::Heading { span, .. } => *span,
            BlockNode::Blockquote { span, .. } => *span,
            BlockNode::Div { span, .. } => *span,
            BlockNode::List { span, .. } => *span,
            BlockNode::ListItem { span, .. } => *span,
            BlockNode::CodeBlock(code_block) => code_block.span,
            BlockNode::Custom(el) => el.span,
            BlockNode::Table(table) => table.span,
            BlockNode::Break { span, .. } => *span,
            BlockNode::HorizontalRule { span, .. } => *span,
            BlockNode::Definition { span, .. } => *span,
            BlockNode::Unknown { .. } => None,
        }
    }

    pub(super) fn text(&self) -> String {
        self.text_by_kind(BlockTextKind::All)
    }

    /// The selected text within this block, in `format`.
    ///
    /// [`SelectionFormat::Source`] reconstructs the Markdown source of the
    /// selection instead of the rendered text.
    pub(super) fn selected_text(&self, format: SelectionFormat) -> String {
        self.text_by_kind(match format {
            SelectionFormat::Plain => BlockTextKind::Selected,
            SelectionFormat::Source => BlockTextKind::SelectedSource,
        })
    }

    fn text_by_kind(&self, kind: BlockTextKind) -> String {
        let mut text = String::new();
        match self {
            BlockNode::Root { children, .. } => {
                let block_text = Self::children_text(children, kind);
                if !block_text.is_empty() {
                    text.push_str(&block_text);
                    text.push('\n');
                }
            }
            BlockNode::Paragraph(paragraph) => {
                let block_text = match kind {
                    BlockTextKind::All => paragraph.text(),
                    BlockTextKind::Selected => paragraph.selected_text(),
                    BlockTextKind::SelectedSource => paragraph.selected_source(),
                };
                if !block_text.is_empty() {
                    text.push_str(&block_text);
                    text.push('\n');
                }
            }
            BlockNode::Heading {
                level, children, attrs, ..
            } => {
                let of = |p: &Paragraph| match kind {
                    BlockTextKind::All => p.text(),
                    BlockTextKind::Selected => p.selected_text(),
                    BlockTextKind::SelectedSource => p.selected_source(),
                };
                let mut block_text = of(children);
                // Its attributes after it, as the line is written.
                if let Some(chip) = attrs.as_ref().map(of).filter(|c| !c.is_empty()) {
                    block_text = if block_text.is_empty() { chip } else { format!("{} {chip}", block_text.trim_end()) };
                }
                if !block_text.is_empty() {
                    // In source mode, prefix the heading marker so a selected
                    // heading round-trips as Markdown (e.g. `## Title`).
                    if matches!(kind, BlockTextKind::SelectedSource) {
                        text.push_str(&"#".repeat(*level as usize));
                        text.push(' ');
                    }
                    text.push_str(&block_text);
                    text.push('\n');
                }
            }
            BlockNode::List {
                children, ordered, ..
            } => {
                if matches!(kind, BlockTextKind::SelectedSource) {
                    // Reconstruct the list source, indenting nested lists and
                    // restoring list markers and task-list checkboxes.
                    text.push_str(&list_selected_source(children, *ordered, ""));
                } else {
                    text.push_str(&Self::children_text(children, kind));
                }
            }
            BlockNode::ListItem { children, .. } => {
                text.push_str(&Self::children_text(children, kind));
            }
            BlockNode::Div { attrs, chip, children, .. } => {
                let block_text = Self::children_text(children, kind);
                let chip_text = match kind {
                    BlockTextKind::All => chip.text(),
                    BlockTextKind::Selected => chip.selected_text(),
                    BlockTextKind::SelectedSource => chip.selected_source(),
                };
                if matches!(kind, BlockTextKind::SelectedSource) && !block_text.is_empty() {
                    text.push_str(&format!("::: {{{}}}\n", attrs.text));
                    text.push_str(&block_text);
                    text.push_str(":::\n");
                } else {
                    if !chip_text.is_empty() && !attrs.has_class(crate::text::format::quarto::MATH_CLASS) {
                        text.push_str(&chip_text);
                        text.push('\n');
                    }
                    text.push_str(&block_text);
                }
            }
            BlockNode::Blockquote { children, .. } => {
                let block_text = Self::children_text(children, kind);

                if !block_text.is_empty() {
                    if matches!(kind, BlockTextKind::SelectedSource) {
                        // Prefix every line with `> ` so a selected blockquote
                        // round-trips as Markdown.
                        let quoted = block_text
                            .trim_end_matches('\n')
                            .lines()
                            .map(|line| {
                                if line.is_empty() {
                                    ">".to_string()
                                } else {
                                    format!("> {}", line)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        text.push_str(&quoted);
                    } else {
                        text.push_str(&block_text);
                    }
                    text.push('\n');
                }
            }
            BlockNode::Table(table) => {
                if matches!(kind, BlockTextKind::SelectedSource) {
                    let block_text = table_selected_source(table);
                    if !block_text.is_empty() {
                        text.push_str(&block_text);
                        text.push('\n');
                    }
                } else {
                    let mut block_text = String::new();
                    for row in table.children.iter() {
                        let mut row_texts = vec![];
                        for cell in row.children.iter() {
                            row_texts.push(match kind {
                                BlockTextKind::All => cell.children.text(),
                                // Source is handled above; only Selected reaches here.
                                _ => cell.children.selected_text(),
                            });
                        }
                        if !row_texts.is_empty() {
                            block_text.push_str(&row_texts.join(" "));
                            block_text.push('\n');
                        }
                    }

                    if !block_text.is_empty() {
                        text.push_str(&block_text);
                        text.push('\n');
                    }
                }
            }
            BlockNode::CodeBlock(code_block) => {
                let block_text = match kind {
                    BlockTextKind::All => code_block.text(),
                    BlockTextKind::Selected => code_block.selected_text(),
                    BlockTextKind::SelectedSource => code_block.selected_source(),
                };
                if !block_text.is_empty() {
                    text.push_str(&block_text);
                    text.push('\n');
                }
            }
            BlockNode::Custom(node) => {
                if let BlockTextKind::All = kind {
                    let content = node.as_text();
                    if !content.is_empty() {
                        text.push_str(content);
                        text.push('\n');
                    }
                }
            }
            BlockNode::Definition { .. }
            | BlockNode::Break { .. }
            | BlockNode::HorizontalRule { .. }
            | BlockNode::Unknown { .. } => {}
        }

        text
    }

    fn children_text(children: &[BlockNode], kind: BlockTextKind) -> String {
        let mut text = String::new();
        for child in children.iter() {
            text.push_str(&child.text_by_kind(kind));
        }

        text
    }

    /// Synchronously clear the selection stored in every inline state.
    ///
    /// Mirrors the [`selected_text`](Self::selected_text) traversal so the
    /// selection can be cleared without relying on a repaint.
    /// Whether this block carries a selection, even an empty one.
    ///
    /// A block only learns its selection when it is painted, so this doubles as
    /// "this block was on screen while the selection was made". An empty
    /// selection is the caret left by the press that started the drag, which is
    /// why it counts (see [`ParsedDocument::selected_text`]).
    pub(super) fn has_selection(&self) -> bool {
        match self {
            BlockNode::Div { chip, children, .. } => chip.has_selection() || children.iter().any(|child| child.has_selection()),
            BlockNode::Root { children, .. }
            | BlockNode::Blockquote { children, .. }
            | BlockNode::List { children, .. }
            | BlockNode::ListItem { children, .. } => {
                children.iter().any(|child| child.has_selection())
            }
            BlockNode::Paragraph(paragraph) => paragraph.has_selection(),
            BlockNode::Heading { children, attrs, .. } => children.has_selection() || attrs.as_ref().is_some_and(|a| a.has_selection()),
            BlockNode::Table(table) => table.children.iter().any(|row| {
                row.children
                    .iter()
                    .any(|cell| cell.children.has_selection())
            }),
            BlockNode::CodeBlock(code_block) => code_block.has_selection(),
            BlockNode::Custom { .. }
            | BlockNode::Definition { .. }
            | BlockNode::Break { .. }
            | BlockNode::HorizontalRule { .. }
            | BlockNode::Unknown { .. } => false,
        }
    }

    pub(super) fn clear_selection(&self) {
        match self {
            BlockNode::Div { chip, children, .. } => {
                chip.clear_selection();
                for child in children.iter() {
                    child.clear_selection();
                }
            }
            BlockNode::Root { children, .. }
            | BlockNode::Blockquote { children, .. }
            | BlockNode::List { children, .. }
            | BlockNode::ListItem { children, .. } => {
                for child in children.iter() {
                    child.clear_selection();
                }
            }
            BlockNode::Paragraph(paragraph) => paragraph.clear_selection(),
            BlockNode::Heading { children, attrs, .. } => {
                children.clear_selection();
                if let Some(attrs) = attrs {
                    attrs.clear_selection();
                }
            }
            BlockNode::Table(table) => {
                for row in table.children.iter() {
                    for cell in row.children.iter() {
                        cell.children.clear_selection();
                    }
                }
            }
            BlockNode::CodeBlock(code_block) => code_block.clear_selection(),
            BlockNode::Custom { .. }
            | BlockNode::Definition { .. }
            | BlockNode::Break { .. }
            | BlockNode::HorizontalRule { .. }
            | BlockNode::Unknown { .. } => {}
        }
    }
}

#[allow(unused)]
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LinkMark {
    pub url: SharedString,
    /// Optional identifier for footnotes.
    pub identifier: Option<SharedString>,
    pub title: Option<SharedString>,
    /// The link as written in the markdown, for a menu's Copy Markdown.
    /// (Emaki addition.)
    pub raw: Option<SharedString>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TextMark {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub code: bool,
    /// Highlight (`<mark>`) the text with this background color.
    ///
    /// `None` means the text is not highlighted.
    pub highlight: Option<Hsla>,
    pub link: Option<LinkMark>,
    /// Quarto's own syntax shown as a quiet chip in the mono face: an
    /// attribute set, a shortcode not resolved. (Emaki addition.)
    pub attr: bool,
}

impl TextMark {
    pub fn attr(mut self) -> Self {
        self.attr = true;
        self.code = true;
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn code(mut self) -> Self {
        self.code = true;
        self
    }

    /// Mark the text as highlighted (`<mark>`) with the given background color.
    pub fn highlight(mut self, color: Hsla) -> Self {
        self.highlight = Some(color);
        self
    }

    pub fn link(mut self, link: impl Into<LinkMark>) -> Self {
        self.link = Some(link.into());
        self
    }

    pub fn merge(&mut self, other: TextMark) {
        self.bold |= other.bold;
        self.italic |= other.italic;
        self.strikethrough |= other.strikethrough;
        self.underline |= other.underline;
        self.code |= other.code;
        self.attr |= other.attr;
        if other.highlight.is_some() {
            self.highlight = other.highlight;
        }
        if let Some(link) = other.link {
            self.link = Some(link);
        }
    }
}

/// The bytes
#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl From<Span> for ElementId {
    fn from(value: Span) -> Self {
        ElementId::Name(format!("md-{}:{}", value.start, value.end).into())
    }
}

/// Where a figure stands: Quarto's `fig-align`. (Emaki addition.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageAlign {
    Left,
    Center,
    Right,
}

/// The attributes of a Quarto fenced div, heading, picture or span, as
/// Pandoc writes them: `{#id .class key="value"}`. (Emaki addition.)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DivAttrs {
    pub id: Option<SharedString>,
    pub classes: Vec<SharedString>,
    pub pairs: Vec<(SharedString, SharedString)>,
    /// As written inside the braces.
    pub text: SharedString,
}

impl DivAttrs {
    /// `{#id .class key="value" key2=value2}`, or a bare class name as a
    /// fence may give one (`::: warning`).
    pub fn parse(text: &str) -> Self {
        let inner = text.trim();
        let inner = inner.strip_prefix('{').and_then(|t| t.strip_suffix('}')).unwrap_or(inner).trim();
        let mut attrs = DivAttrs { text: inner.to_string().into(), ..Default::default() };
        let mut rest = inner;
        while !rest.is_empty() {
            let end = word_end(rest);
            let word = &rest[..end];
            rest = rest[end..].trim_start();
            if let Some(id) = word.strip_prefix('#') {
                attrs.id = Some(id.to_string().into());
            } else if let Some(class) = word.strip_prefix('.') {
                attrs.classes.push(class.to_string().into());
            } else if let Some((k, v)) = word.split_once('=') {
                let v = v.trim_matches(|c| c == '"' || c == '\'');
                attrs.pairs.push((k.to_string().into(), v.to_string().into()));
            } else if !word.is_empty() {
                attrs.classes.push(word.to_string().into());
            }
        }
        attrs
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_ref())
    }
}

/// Where the first word of an attribute list ends: at a space outside
/// quotes.
fn word_end(text: &str) -> usize {
    let mut quote: Option<char> = None;
    for (ix, c) in text.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() => return ix,
            None => {}
        }
    }
    text.len()
}

#[allow(unused)]
#[derive(Debug, Default, Clone)]
pub struct ImageNode {
    pub url: SharedUri,
    pub link: Option<LinkMark>,
    pub title: Option<SharedString>,
    pub alt: Option<SharedString>,
    pub width: Option<DefiniteLength>,
    pub height: Option<DefiniteLength>,
    /// Where a figure stands in its line, from Quarto's `fig-align`.
    /// (Emaki addition.)
    pub align: Option<ImageAlign>,
    /// The words under a figure: Quarto's, the alt text of a picture
    /// standing alone. (Emaki addition.)
    pub caption: Option<SharedString>,
}

impl ImageNode {
    pub fn title(&self) -> String {
        self.title
            .clone()
            .unwrap_or_else(|| self.alt.clone().unwrap_or_default())
            .to_string()
    }
}

impl PartialEq for ImageNode {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.link == other.link
            && self.title == other.title
            && self.alt == other.alt
            && self.width == other.width
            && self.align == other.align
            && self.caption == other.caption
            && self.height == other.height
    }
}

#[derive(Default, Clone, Debug)]
pub(crate) struct InlineNode {
    /// The text content.
    pub(crate) text: SharedString,
    pub(crate) image: Option<ImageNode>,
    /// The text styles, each tuple contains the range of the text and the style.
    pub(crate) marks: Vec<(Range<usize>, TextMark)>,
    /// A formula: its text is the room its picture is painted over, or
    /// its words where it has none (`text/math.rs`). (Emaki addition.)
    pub(crate) math: Option<Arc<super::math::MathSource>>,

    state: Arc<Mutex<InlineState>>,
}

impl PartialEq for InlineNode {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.image == other.image && self.marks == other.marks && self.math == other.math
    }
}

/// Wrap `text` with the Markdown syntax implied by `mark`.
///
/// This mirrors the per-mark formatting in [`Paragraph::to_markdown`] but
/// operates on an already-sliced run, so it can reconstruct the Markdown
/// source for a *partial* text selection. Applied inside-out (innermost markup
/// first) so nested emphasis like `**_x_**` round-trips.
pub(crate) fn wrap_with_mark(text: &str, mark: &TextMark) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut out = text.to_string();
    if mark.code {
        out = format!("`{}`", out);
    }
    if mark.italic {
        out = format!("*{}*", out);
    }
    if mark.bold {
        out = format!("**{}**", out);
    }
    if mark.strikethrough {
        out = format!("~~{}~~", out);
    }
    if mark.underline {
        // Markdown has no underline syntax, and `__` reads as bold to most
        // parsers, so fall back to the inline HTML `<u>` parses from.
        out = format!("<u>{}</u>", out);
    }
    if mark.highlight.is_some() {
        out = format!("=={}==", out);
    }
    if let Some(link) = &mark.link {
        out = match &link.title {
            Some(title) => format!("[{}]({} \"{}\")", out, link.url, title),
            None => format!("[{}]({})", out, link.url),
        };
    }
    out
}

/// How a selection covers one rendered run, so the caller can tell whether it
/// continues into an adjacent inline image.
#[derive(Default)]
struct RunSelection {
    emitted: bool,
    at_start: bool,
    at_end: bool,
}

/// Emit the selected part of one rendered run, preceded by the images the
/// selection has run into. `run` holds the run's children with their offset
/// into the run's concatenated text.
fn emit_run(
    state: &Arc<Mutex<InlineState>>,
    run: &[(usize, &InlineNode)],
    pending_images: &mut Vec<String>,
    out: &mut String,
) -> RunSelection {
    let mut selected = RunSelection::default();
    let Ok(state) = state.lock() else {
        return selected;
    };
    let Some(selection) = &state.selection else {
        return selected;
    };
    if selection.start >= selection.end {
        return selected;
    }

    selected.at_start = selection.start == 0;
    selected.at_end = selection.end >= state.text.len();

    for (start, child) in run {
        let end = start + child.text.len();
        let lo = selection.start.max(*start);
        let hi = selection.end.min(end);
        if lo >= hi {
            continue;
        }

        if !selected.emitted {
            if selected.at_start {
                out.push_str(&pending_images.join(""));
            }
            pending_images.clear();
        }
        selected.emitted = true;

        // A formula is copied as it was written. (Emaki addition.)
        if let Some(math) = &child.math {
            out.push_str(&math.markdown());
            continue;
        }
        out.push_str(&reconstruct_markdown(
            &child.text,
            &child.marks,
            (lo - start)..(hi - start),
        ));
    }

    selected
}

/// The selected words of one rendered run, a formula as it was written.
/// (Emaki addition.)
fn selected_run_text(state: &Arc<Mutex<InlineState>>, run: &[(usize, &InlineNode)], out: &mut String) {
    let Ok(state) = state.lock() else { return };
    let Some(selection) = &state.selection else { return };
    let (lo, hi) = (selection.start.min(state.text.len()), selection.end.min(state.text.len()));
    if lo >= hi {
        return;
    }
    if run.iter().all(|(_, c)| c.math.is_none()) {
        out.push_str(&state.text[lo..hi]);
        return;
    }
    for (start, child) in run {
        let end = start + child.text.len();
        let (a, b) = (lo.max(*start), hi.min(end));
        if a >= b {
            continue;
        }
        match &child.math {
            Some(math) => out.push_str(&math.markdown()),
            None => out.push_str(state.text.get(a..b).unwrap_or_default()),
        }
    }
}

/// The Markdown source for an inline image, e.g. `![alt](url "title")`.
fn image_markdown(image: &ImageNode) -> String {
    let alt = image.alt.clone().unwrap_or_default();
    let title = image
        .title
        .clone()
        .map_or(String::new(), |title| format!(" \"{}\"", title));
    format!("![{}]({}{})", alt, image.url, title)
}

/// Reconstruct the Markdown source for the `selection` sub-range of a text run
/// carrying `marks`.
///
/// `selection` is a byte range into `text`. For each mark that overlaps the
/// selection, the overlapping slice is wrapped in the mark's Markdown syntax
/// (see [`wrap_with_mark`]); slices not covered by any mark are emitted
/// verbatim. This lets a rendered-offset selection be copied back as Markdown
/// source (e.g. selecting inside a `**bold**` run yields `**bold**`).
pub(crate) fn reconstruct_markdown(
    text: &str,
    marks: &[(Range<usize>, TextMark)],
    selection: Range<usize>,
) -> String {
    let start = selection.start.min(text.len());
    let end = selection.end.min(text.len());
    if start >= end {
        return String::new();
    }

    let mut out = String::new();
    let mut cursor = start;
    // Marks are stored in ascending, non-overlapping order by the parser.
    for (range, mark) in marks.iter() {
        let seg_start = range.start.max(start);
        let seg_end = range.end.min(end);
        if seg_start >= seg_end {
            continue;
        }
        // Emit any unmarked text before this mark verbatim.
        if cursor < seg_start {
            out.push_str(&text[cursor..seg_start]);
        }
        out.push_str(&wrap_with_mark(&text[seg_start..seg_end], mark));
        cursor = seg_end;
    }
    // Trailing unmarked text.
    if cursor < end {
        out.push_str(&text[cursor..end]);
    }
    out
}

/// Reconstruct the Markdown source of the selected cells of `table`.
///
/// Cells emit their own selected source; rows are piped (`| a | b |`) and the
/// delimiter/alignment row is inserted after the first row, so a selected
/// table round-trips as a Markdown table. Returns an empty string when no cell
/// is selected.
fn table_selected_source(table: &Table) -> String {
    let cell_source = |cell: &TableCell| cell.children.selected_source().replace('\n', " ");

    let any_selected = table.children.iter().any(|row| {
        row.children
            .iter()
            .any(|cell| !cell_source(cell).trim().is_empty())
    });
    if !any_selected {
        return String::new();
    }

    let mut lines: Vec<String> = Vec::new();
    for (row_ix, row) in table.children.iter().enumerate() {
        let cells: Vec<String> = row
            .children
            .iter()
            .map(|cell| cell_source(cell).trim().to_string())
            .collect();
        lines.push(format!("| {} |", cells.join(" | ")));

        // The Markdown delimiter row carries the column alignments and must
        // follow the header row.
        if row_ix == 0 {
            let aligns: Vec<String> = (0..row.children.len())
                .map(|ix| {
                    match table.column_align(ix) {
                        ColumnumnAlign::Left => ":--",
                        ColumnumnAlign::Center => ":-:",
                        ColumnumnAlign::Right => "--:",
                    }
                    .to_string()
                })
                .collect();
            lines.push(format!("| {} |", aligns.join(" | ")));
        }
    }

    lines.join("\n")
}

/// Reconstruct the Markdown source of the selected items of a list.
///
/// Restores the list marker (`- ` / `N. `) and task-list checkbox (`[x] ` /
/// `[ ] `) of each item, and recurses into nested lists with a deeper `indent`
/// so nesting is preserved. `indent` is the leading whitespace for this level;
/// nested levels are indented by the width of the parent marker so continuation
/// and sub-list lines align under the item text. Items with no selected content
/// are skipped but still consume an ordered number, so the remaining items keep
/// their original numbering.
fn list_selected_source(children: &[BlockNode], ordered: bool, indent: &str) -> String {
    let mut out = String::new();
    let mut item_ix = 0usize;

    for child in children {
        let BlockNode::ListItem {
            children: item_children,
            checked,
            ..
        } = child
        else {
            continue;
        };

        let marker = if ordered {
            format!("{}. ", item_ix + 1)
        } else {
            "- ".to_string()
        };
        let checkbox = match checked {
            Some(true) => "[x] ",
            Some(false) => "[ ] ",
            None => "",
        };
        let child_indent = format!("{}{}", indent, " ".repeat(marker.len()));

        // Split the item into its own content and any nested lists, so the
        // nested lists can be indented under the content.
        let mut content = String::new();
        let mut nested = String::new();
        for sub in item_children {
            if let BlockNode::List {
                children: sub_children,
                ordered: sub_ordered,
                ..
            } = sub
            {
                nested.push_str(&list_selected_source(
                    sub_children,
                    *sub_ordered,
                    &child_indent,
                ));
            } else {
                content.push_str(&sub.text_by_kind(BlockTextKind::SelectedSource));
            }
        }
        let content = content.trim_end_matches('\n');

        if content.is_empty() && nested.is_empty() {
            item_ix += 1;
            continue;
        }

        if content.is_empty() {
            // An item whose only selected content is a nested list.
            out.push_str(indent);
            out.push_str(&marker);
            out.push_str(checkbox.trim_end());
            out.push('\n');
        } else {
            // The first line carries the marker and checkbox; continuation
            // lines are indented to align under the item text.
            let mut lines = content.lines();
            if let Some(first) = lines.next() {
                out.push_str(indent);
                out.push_str(&marker);
                out.push_str(checkbox);
                out.push_str(first);
                out.push('\n');
            }
            for line in lines {
                out.push_str(&child_indent);
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str(&nested);
        item_ix += 1;
    }

    out
}

impl InlineNode {
    pub(crate) fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            image: None,
            marks: vec![],
            math: None,
            state: Arc::new(Mutex::new(InlineState::default())),
        }
    }

    /// The node is this formula. (Emaki addition.)
    pub(crate) fn with_math(mut self, math: super::math::MathSource) -> Self {
        self.math = Some(Arc::new(math));
        self
    }

    pub(crate) fn image(image: ImageNode) -> Self {
        let mut this = Self::new("");
        this.image = Some(image);
        this
    }

    pub(crate) fn marks(mut self, marks: Vec<(Range<usize>, TextMark)>) -> Self {
        self.marks = marks;
        self
    }
}

/// The paragraph element, contains multiple text nodes.
///
/// Unlike other Element, this is cloneable, because it is used in the Node AST.
/// We are keep the selection state inside this AST Nodes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Paragraph {
    pub(super) span: Option<Span>,
    pub(super) children: Vec<InlineNode>,
    /// The link references in this paragraph, used for reference links.
    ///
    /// The key is the identifier, the value is the url.
    pub(super) link_refs: HashMap<SharedString, SharedString>,

    pub(crate) state: Arc<Mutex<InlineState>>,
}

impl PartialEq for Paragraph {
    fn eq(&self, other: &Self) -> bool {
        self.span == other.span
            && self.children == other.children
            && self.link_refs == other.link_refs
    }
}

impl Paragraph {
    pub(crate) fn new(text: String) -> Self {
        Self {
            span: None,
            children: vec![InlineNode::new(&text)],
            link_refs: HashMap::new(),
            state: Arc::new(Mutex::new(InlineState::default())),
        }
    }

    pub(super) fn selected_text(&self) -> String {
        let mut text = String::new();
        // Walked in the runs [`Paragraph::render`] draws, as
        // `selected_source` walks them, so a formula's room is read as the
        // formula. (Emaki: the runs.)
        let mut run: Vec<(usize, &InlineNode)> = Vec::new();
        let mut offset = 0;
        for c in self.children.iter() {
            if c.image.is_none() {
                run.push((offset, c));
                offset += c.text.len();
                continue;
            }
            selected_run_text(&c.state, &run, &mut text);
            run.clear();
            offset = 0;
        }
        selected_run_text(&self.state, &run, &mut text);
        text
    }

    /// Reconstruct the Markdown source for the current selection.
    ///
    /// Mirrors [`selected_text`](Self::selected_text), but emits Markdown
    /// instead of the rendered text, using each inline node's `marks` (see
    /// [`reconstruct_markdown`]).
    ///
    /// Selection offsets index an `InlineState.text`, and one such state spans
    /// *several* children: [`Paragraph::render`] concatenates children until it
    /// hits an inline image, stores that run in the image child's state, then
    /// starts over; whatever follows the last image is stored in `self.state`.
    /// So walk the children in the same runs and map each selected byte back to
    /// the child it was rendered from — mapping against a single child's text
    /// would attribute the same offsets to children in other runs.
    ///
    /// An image has no selection of its own, so it is emitted when the
    /// selection runs into it: reaching the end of the run before it, and
    /// starting at the beginning of the run after it. A paragraph that begins
    /// or ends with an image has no run on that side, which counts as reaching
    /// it.
    pub(super) fn selected_source(&self) -> String {
        let mut source = String::new();
        let mut pending_images: Vec<String> = Vec::new();
        let mut run: Vec<(usize, &InlineNode)> = Vec::new();
        let mut offset = 0;
        let mut enters_image = true;

        for child in self.children.iter() {
            let Some(image) = &child.image else {
                run.push((offset, child));
                offset += child.text.len();
                continue;
            };

            // The run before an image is stored in that image's own state.
            let run_before = !run.is_empty();
            let selected = emit_run(&child.state, &run, &mut pending_images, &mut source);
            if run_before {
                enters_image = selected.emitted && selected.at_end;
            }
            if enters_image {
                pending_images.push(image_markdown(image));
            } else {
                pending_images.clear();
            }

            run.clear();
            offset = 0;
        }

        let trailing = emit_run(&self.state, &run, &mut pending_images, &mut source);
        // Trailing images have no run after them to flush them.
        if !trailing.emitted && enters_image && !source.is_empty() {
            source.push_str(&pending_images.join(""));
        }

        source
    }

    pub(super) fn text(&self) -> String {
        let mut text = String::new();
        for node in self.children.iter() {
            text.push_str(&node.text);
        }
        text
    }

    /// Synchronously clear the selection stored in every inline state.
    ///
    /// Mirrors the [`selected_text`](Self::selected_text) traversal.
    pub(super) fn has_selection(&self) -> bool {
        self.children
            .iter()
            .any(|c| c.state.lock().is_ok_and(|state| state.selection.is_some()))
            || self
                .state
                .lock()
                .is_ok_and(|state| state.selection.is_some())
    }

    pub(super) fn clear_selection(&self) {
        for c in self.children.iter() {
            if let Ok(mut state) = c.state.lock() {
                state.selection = None;
            }
        }

        if let Ok(mut state) = self.state.lock() {
            state.selection = None;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Table {
    pub(crate) children: Vec<TableRow>,
    pub(crate) column_aligns: Vec<ColumnumnAlign>,
    pub(crate) span: Option<Span>,
}

impl Table {
    pub(crate) fn column_align(&self, index: usize) -> ColumnumnAlign {
        self.column_aligns.get(index).copied().unwrap_or_default()
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub(crate) enum ColumnumnAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl From<mdast::AlignKind> for ColumnumnAlign {
    fn from(value: mdast::AlignKind) -> Self {
        match value {
            mdast::AlignKind::None => ColumnumnAlign::Left,
            mdast::AlignKind::Left => ColumnumnAlign::Left,
            mdast::AlignKind::Center => ColumnumnAlign::Center,
            mdast::AlignKind::Right => ColumnumnAlign::Right,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TableRow {
    pub children: Vec<TableCell>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TableCell {
    pub children: Paragraph,
    pub width: Option<DefiniteLength>,
}

impl Paragraph {
    pub(crate) fn take(&mut self) -> Paragraph {
        std::mem::replace(
            self,
            Paragraph {
                span: None,
                children: vec![],
                link_refs: Default::default(),
                state: Arc::new(Mutex::new(InlineState::default())),
            },
        )
    }

    pub(crate) fn is_image(&self) -> bool {
        false
    }

    pub(crate) fn set_span(&mut self, span: Span) {
        self.span = Some(span);
    }

    pub(crate) fn push_str(&mut self, text: &str) {
        self.children.push(
            InlineNode::new(text.to_string()).marks(vec![(0..text.len(), TextMark::default())]),
        );
    }

    pub(crate) fn push(&mut self, text: InlineNode) {
        self.children.push(text);
    }

    pub(crate) fn push_image(&mut self, image: ImageNode) {
        self.children.push(InlineNode::image(image));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.children.is_empty()
            || self
                .children
                .iter()
                .all(|node| node.text.is_empty() && node.image.is_none())
    }

    /// Return length of children text.
    pub(crate) fn text_len(&self) -> usize {
        self.children
            .iter()
            .map(|node| node.text.len())
            .sum::<usize>()
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.children.extend(other.children);
    }
}

#[derive(Debug, Clone)]
struct CachedCodeBlockStyles {
    /// The active theme used to compute `styles`.
    highlight_theme: Arc<HighlightTheme>,
    styles: Vec<(Range<usize>, HighlightStyle)>,
}

#[derive(Debug, Clone)]
pub struct CodeBlock {
    lang: Option<SharedString>,
    styles: Arc<Mutex<Option<CachedCodeBlockStyles>>>,
    state: Arc<Mutex<InlineState>>,
    pub span: Option<Span>,
}

/// The icon of a chunk's language, by its file's name in the caller's
/// set (`TextViewStyle::chunk_icons`). (Emaki addition.)
fn chunk_icon(lang: Option<&str>) -> Option<&'static str> {
    let inner = lang?.strip_prefix('{')?.trim_end_matches('}');
    let word = inner.trim_start_matches('=').split(|c: char| c == ',' || c.is_whitespace()).next()?.trim().to_ascii_lowercase();
    Some(match word.as_str() {
        "r" => "r",
        "python" | "py" => "python",
        "julia" | "jl" => "julia",
        "ojs" | "js" | "javascript" | "node" => "javascript",
        "mermaid" => "mermaid",
        "dot" | "graphviz" => "graphviz",
        "bash" | "sh" | "zsh" | "shell" => "bash",
        "sql" => "sql",
        "html" => "html",
        "css" => "css",
        "latex" | "tex" => "latex",
        "typst" => "typst",
        "stata" => "stata",
        "rust" => "rust",
        "c" => "c",
        "cpp" | "rcpp" => "cpp",
        "go" => "go",
        "java" => "java",
        "swift" => "swift",
        "md" | "markdown" | "asis" => "markdown",
        _ => "code",
    })
}

/// The name a Quarto chunk's language is shown under: `{r}` R,
/// `{python}` Python, a raw block (`{=latex}`) its format. None for a
/// block of code that is not a chunk. (Emaki addition.)
fn chunk_label(lang: Option<&str>) -> Option<String> {
    let inner = lang?.strip_prefix('{')?.trim_end_matches('}');
    let word = inner.trim_start_matches('=').split(|c: char| c == ',' || c.is_whitespace()).next()?.trim();
    if word.is_empty() {
        return None;
    }
    let name = match word.to_ascii_lowercase().as_str() {
        "r" => "R",
        "python" | "py" => "Python",
        "julia" | "jl" => "Julia",
        "ojs" => "OJS",
        "mermaid" => "Mermaid",
        "dot" => "Graphviz",
        "bash" | "sh" | "zsh" => "Bash",
        "sql" => "SQL",
        "html" => "HTML",
        "latex" | "tex" => "LaTeX",
        "typst" => "Typst",
        "openxml" => "OpenXML",
        "rst" => "reStructuredText",
        "css" => "CSS",
        "js" | "javascript" => "JavaScript",
        "stata" => "Stata",
        "sas" => "SAS",
        _ => {
            let mut chars = word.chars();
            let first = chars.next()?.to_uppercase().collect::<String>();
            return Some(format!("{first}{}", chars.as_str()));
        }
    };
    Some(name.to_string())
}

impl PartialEq for CodeBlock {
    fn eq(&self, other: &Self) -> bool {
        self.lang == other.lang && self.code() == other.code() && self.span == other.span
    }
}

impl CodeBlock {
    /// Get the language of the code block.
    pub fn lang(&self) -> Option<SharedString> {
        self.lang.clone()
    }

    /// Get the code content of the code block.
    pub fn code(&self) -> SharedString {
        self.state
            .lock()
            .map(|state| state.text.clone())
            .unwrap_or_default()
    }

    pub(crate) fn new(
        code: SharedString,
        lang: Option<SharedString>,
        span: Option<impl Into<Span>>,
    ) -> Self {
        let state = Arc::new(Mutex::new(InlineState::default()));
        if let Ok(mut state) = state.lock() {
            state.set_text(code);
        }

        Self {
            lang,
            styles: Arc::new(Mutex::new(None)),
            state,
            span: span.map(|s| s.into()),
        }
    }

    pub(crate) fn styles(
        &self,
        highlight_theme: &Arc<HighlightTheme>,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let Some(lang) = &self.lang else {
            return Vec::new();
        };

        let Ok(mut styles) = self.styles.lock() else {
            return Vec::new();
        };

        // Pointer identity is the common render-path fast check. If an
        // equivalent theme is reallocated, adopt its Arc while preserving the
        // computed styles so subsequent renders also use the fast path.
        if let Some(cached) = styles.as_mut() {
            if Arc::ptr_eq(&cached.highlight_theme, highlight_theme) {
                return cached.styles.clone();
            }

            if cached.highlight_theme.as_ref() == highlight_theme.as_ref() {
                cached.highlight_theme = highlight_theme.clone();
                return cached.styles.clone();
            }
        }

        let code = self.code();
        let computed_styles = CODE_BLOCK_HIGHLIGHTERS.with(|cache| {
            let mut cache = cache.borrow_mut();
            let highlighter = cache
                .entry(lang.clone())
                .or_insert_with(|| SyntaxHighlighter::new(lang));

            if let Some(config) = LanguageRegistry::singleton().language(lang)
                && highlighter.language() != &config.name
            {
                *highlighter = SyntaxHighlighter::new(lang);
            }

            let old_end_byte = highlighter.text().len();
            let old_end_position = highlighter.text().offset_to_point(old_end_byte);
            let code_rope = Rope::from_str(code.as_str());

            let edit = InputEdit {
                start_byte: 0,
                old_end_byte,
                new_end_byte: code.len(),
                start_position: Point::new(0, 0),
                old_end_position,
                new_end_position: code_rope.offset_to_point(code.len()),
            };

            #[cfg(feature = "tree-sitter")]
            highlighter.update_input(Some(edit), &code_rope, None);
            #[cfg(not(feature = "tree-sitter"))]
            highlighter.update(Some(edit), &code_rope, None);
            highlighter.styles(&(0..code.len()), highlight_theme.as_ref())
        });
        *styles = Some(CachedCodeBlockStyles {
            highlight_theme: highlight_theme.clone(),
            styles: computed_styles.clone(),
        });
        computed_styles
    }

    pub(super) fn selected_text(&self) -> String {
        let mut text = String::new();
        if let Ok(state) = self.state.lock()
            && let Some(selection) = &state.selection
        {
            text.push_str(&state.text[selection.start..selection.end]);
        }
        text
    }

    /// Markdown source for the current selection.
    ///
    /// The selected code is wrapped in a fenced code block carrying the block's
    /// language, so a selected code block round-trips as Markdown (e.g.
    /// ```` ```rust\n…\n``` ````) instead of pasting as bare, unfenced text.
    /// A partial selection is still emitted as a valid fenced block.
    pub(super) fn selected_source(&self) -> String {
        let code = self.selected_text();
        if code.is_empty() {
            return String::new();
        }
        let lang = self.lang.clone().unwrap_or_default();
        // Trim trailing newlines so the closing fence sits on its own line
        // directly after the last code line (no blank line before it).
        let code = code.trim_end_matches('\n');
        format!("```{}\n{}\n```", lang, code)
    }

    pub(super) fn text(&self) -> String {
        self.state
            .lock()
            .map(|state| state.text.to_string())
            .unwrap_or_default()
    }

    /// Synchronously clear the selection stored in the inline state.
    ///
    /// Mirrors the [`selected_text`](Self::selected_text) traversal.
    pub(super) fn has_selection(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.selection.is_some())
    }

    pub(super) fn clear_selection(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.selection = None;
        }
    }

    fn render(
        &self,
        options: &NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let style = &node_cx.style;
        // Where the block is scrolled sideways, kept by the block as
        // parsed: the same block has the same place while its text
        // stands. (Emaki addition.)
        let key = SharedString::from(format!("codeblock-scroll-{:p}", Arc::as_ptr(&self.state)));
        let scroll = window.use_keyed_state(key, cx, |_, _| ScrollHandle::default()).read(cx).clone();
        // A selection dragged to an edge of the block scrolls it that
        // way, as a text field does. (Emaki addition.)
        let drag_key = SharedString::from(format!("codeblock-drag-{:p}", Arc::as_ptr(&self.state)));
        let drag = window.use_keyed_state(drag_key, cx, |_, _| CodeDrag::default());
        // The actions are made for the block under the pointer only: made
        // for every block, a document of a hundred chunks built a button
        // for each of them every frame, a quarter of the frame's time.
        // (Emaki addition.)
        let pointed = node_cx.code_block_actions.is_some().then(|| window.use_keyed_state(SharedString::from(format!("codeblock-hover-{:p}", Arc::as_ptr(&self.state))), cx, |_, _| false));
        // A Quarto chunk folds by itself (Emaki addition): a head row,
        // its chevron and its language, stays, and what is under it
        // closes up to it over a moment, off the chunk's own clock. The
        // fold is kept by what the chunk says, so a file read again keeps
        // its folds.
        let label = chunk_label(self.lang.as_deref());
        let fold = label.as_ref().map(|_| {
            use std::hash::{Hash as _, Hasher as _};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            self.lang.hash(&mut h);
            self.code().hash(&mut h);
            let key = SharedString::from(format!("chunk-fold-{:x}", h.finish()));
            window.use_keyed_state(key, cx, |_, _| ChunkFold::default())
        });
        let (open, shown, body_h) = match &fold {
            Some(f) => {
                let f = f.read(cx);
                if f.at.is_some_and(|at| at.elapsed() < CHUNK_FOLD) {
                    window.request_animation_frame();
                }
                (f.open, f.shown(), f.height.clone())
            }
            None => (true, 1., std::rc::Rc::new(std::cell::Cell::new(0.))),
        };
        let head = label.map(|label| {
            let [fold_tip, unfold_tip] = style.chunk_words.clone();
            let icon = (!style.chunk_icons.is_empty()).then(|| chunk_icon(self.lang.as_deref())).flatten().map(|id| SharedString::from(format!("{}/{id}.svg", style.chunk_icons)));
            let tip = if open { fold_tip } else { unfold_tip };
            let fold = fold.clone().unwrap();
            div()
                .id("chunk-head")
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.))
                .cursor_pointer()
                .font_family(cx.theme().font_family.clone())
                .text_size(px(11.5))
                .line_height(px(16.))
                .text_color(cx.theme().muted_foreground)
                .hover(|s| s.text_color(cx.theme().foreground))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    gpui_base::TextSelection::end(window, cx);
                    cx.stop_propagation();
                    fold.update(cx, |f, cx| {
                        f.toggle();
                        cx.notify();
                    });
                    window.refresh();
                })
                .child(
                    crate::Sizable::with_size(crate::Icon::new(crate::IconName::ChevronDown), px(12.))
                        .rotate(gpui::Radians(-std::f32::consts::FRAC_PI_2 * (1. - shown))),
                )
                .children(icon.map(|path| gpui::img(path).size(px(13.)).flex_shrink_0()))
                .child(label)
        });

        let code = {
                        let scroll = scroll.clone();
                        let (at, most) = (-scroll.offset().x, scroll.max_offset().x);
                        let ground = cx.theme().muted;
                        let fade = |left: bool| {
                            let (from, to) = if left { (ground, ground.opacity(0.)) } else { (ground.opacity(0.), ground) };
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .w(px(36.))
                                .map(|d| if left { d.left_0() } else { d.right_0() })
                                .bg(linear_gradient(90., linear_color_stop(from, 0.), linear_color_stop(to, 1.)))
                        };
                        div()
                            .relative()
                            .w_full()
                            .min_w_0()
                            .child(horizontal_scroll_area(
                                ("code-scroll", options.ix),
                                &scroll,
                                // A row, and the text an item of it that
                                // does not shrink: as a block's child it
                                // was as wide as the block, its lines ran
                                // past it, and there was nothing to scroll.
                                &gpui::StyleRefinement::default().flex(),
                                div().flex_none().child(Inline::new(
                                    "code",
                                    self.state.clone(),
                                    vec![],
                                    self.styles(&cx.theme().highlight_theme),
                                    node_cx.link_click_handler.clone(),
                                )),
                            ))
                            .when(at > px(1.), |d| d.child(fade(true)))
                            .when(most - at > px(1.), |d| d.child(fade(false)))
                            .child(code_drag(drag.clone(), scroll.clone()).absolute().inset_0())
                    };
        // What folds: the code, and the room between it and the head.
        // Measured as it is drawn, so a fold closes from the height it had.
        let body = {
            let height = body_h.clone();
            div()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .relative()
                .when(fold.is_some(), |d| d.pt_2())
                .child(code)
                .child(gpui::canvas(move |bounds, _, _| height.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().inset_0())
        };
        let body = if shown >= 1. {
            body.into_any_element()
        } else {
            div().w_full().min_w_0().h(px(body_h.get() * shown)).overflow_hidden().opacity(shown).child(body).into_any_element()
        };

        let block = div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .when(!options.is_last, |this| this.pb(style.paragraph_gap))
            .child(
                div()
                    .id(("codeblock", options.ix))
                    // EMAKI: the actions show while the pointer is on the
                    // block, and not all the time.
                    .group("codeblock")
                    .w_full()
                    .min_w_0()
                    .p_3()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().tokens.muted)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .relative()
                    .refine_style(&style.code_block)
                    // A line of code is not wrapped: what is set out in
                    // columns (a table, a drawing, a mock of a screen)
                    // came apart where a line was broken. A block wider
                    // than its room scrolls sideways, and only when the
                    // gesture is sideways, so a wheel over it still moves
                    // what it is in (`horizontal_scroll_area`, as a table
                    // has). Where more lies past an edge the
                    // text fades into the block's ground there, and the
                    // scrollbar shows under the pointer: cut off clean,
                    // a line read as all there was. (Emaki addition.)
                    .whitespace_nowrap()
                    .children(head)
                    .when(shown > 0., |d| d.child(body))
                    // On the block and not on the text's own box: there
                    // it lay over the last line.
                    .when(open, |d| d.horizontal_scrollbar(&scroll))
                    .when_some(pointed.clone(), |d, pointed| {
                        d.on_hover(move |on, _, cx| {
                            pointed.update(cx, |was, cx| {
                                if *was != *on {
                                    *was = *on;
                                    cx.notify();
                                }
                            })
                        })
                    })
                    .when_some(node_cx.code_block_actions.clone().filter(|_| pointed.as_ref().is_some_and(|p| *p.read(cx))), |this, actions| {
                        this.child(
                            div()
                                .id("actions")
                                .absolute()
                                .top_2()
                                .right_2()
                                .bg(cx.theme().tokens.muted)
                                .rounded(cx.theme().radius)
                                .opacity(0.)
                                .group_hover("codeblock", |s| s.opacity(1.))
                                .child(actions(&self, window, cx)),
                        )
                    }),
            );
        block.into_any_element()
    }
}

/// How long a tabset's plate and content take to change. (Emaki addition.)
const TABSET_ANIM: std::time::Duration = std::time::Duration::from_millis(220);

/// A tabset's choice: the tab shown, the one before it and when that
/// changed, the content's height then and as last drawn, and where each
/// tab stands in its track (left and width). (Emaki addition.)
#[derive(Default)]
struct TabSet {
    at: usize,
    from: usize,
    when: Option<std::time::Instant>,
    from_h: f32,
    height: std::rc::Rc<std::cell::Cell<f32>>,
    spots: std::rc::Rc<RefCell<Vec<(f32, f32)>>>,
    track_x: std::rc::Rc<std::cell::Cell<f32>>,
}

/// How long a chunk takes to fold or open. (Emaki addition.)
const CHUNK_FOLD: std::time::Duration = std::time::Duration::from_millis(220);

/// A chunk's fold: whether it is open, when that last changed, and the
/// height of what folds as last drawn. Open at first. (Emaki addition.)
struct ChunkFold {
    open: bool,
    at: Option<std::time::Instant>,
    height: std::rc::Rc<std::cell::Cell<f32>>,
}

impl Default for ChunkFold {
    fn default() -> Self {
        Self { open: true, at: None, height: Default::default() }
    }
}

impl ChunkFold {
    /// How much of what folds shows now: 1 all, 0 none.
    fn shown(&self) -> f32 {
        let t = self.at.map(|at| (at.elapsed().as_secs_f32() / CHUNK_FOLD.as_secs_f32()).min(1.)).unwrap_or(1.);
        let e = gpui::ease_out_quint()(t);
        if self.open { e } else { 1. - e }
    }

    /// Fold or open. Turned back on the way, it goes on from where it is:
    /// its clock set back to the moment that share was reached.
    fn toggle(&mut self) {
        let was = self.shown();
        self.open = !self.open;
        let left = if self.open { was } else { 1. - was };
        // ease_out_quint is 1 - (1 - t)^5; this is the t it gives `left` at.
        let back = CHUNK_FOLD.mul_f32(1. - (1. - left.clamp(0., 1.)).powf(0.2));
        let now = std::time::Instant::now();
        self.at = Some(now.checked_sub(back).unwrap_or(now));
    }
}

/// A drag that began on a code block's text: where the pointer is while
/// the button that began it is held, and whether it has moved since, so
/// a press alone (the copy button, near the right edge) scrolls nothing.
/// (Emaki addition.)
#[derive(Default)]
struct CodeDrag {
    at: Option<gpui::Point<gpui::Pixels>>,
    moved: bool,
}

/// Laid over a code block's text, it scrolls the block sideways while a
/// drag that began on the text is held near or past its left or right
/// edge: a step a frame, longer the further out the pointer is, so a
/// selection reaches what lies past the edge. A press on the block's
/// scrollbar is under the text's box and begins nothing, so dragging the
/// bar is the bar's alone. The selection follows by itself: it is kept
/// as points in the window, read against the text where it now stands.
/// (Emaki addition.)
fn code_drag(drag: gpui::Entity<CodeDrag>, scroll: ScrollHandle) -> gpui::Canvas<()> {
    use gpui::{DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point};
    /// How near an edge the pointer starts the scroll.
    const EDGE: f32 = 24.;
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            {
                let drag = drag.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && e.button == MouseButton::Left && bounds.contains(&e.position) {
                        drag.update(cx, |d, _| *d = CodeDrag { at: Some(e.position), moved: false });
                    }
                });
            }
            {
                let drag = drag.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || drag.read(cx).at.is_none() {
                        return;
                    }
                    let held = e.pressed_button == Some(MouseButton::Left);
                    drag.update(cx, |d, _| *d = CodeDrag { at: held.then_some(e.position), moved: held });
                    if held {
                        window.refresh();
                    }
                });
            }
            {
                let drag = drag.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && drag.read(cx).at.is_some() {
                        drag.update(cx, |d, _| d.at = None);
                    }
                });
            }
            let Some(at) = drag.read(cx).at.filter(|_| drag.read(cx).moved) else { return };
            let (x, left, right) = (f32::from(at.x), f32::from(bounds.left()) + EDGE, f32::from(bounds.right()) - EDGE);
            let past = if x > right { x - right } else if x < left { x - left } else { 0. };
            if past == 0. {
                return;
            }
            let step = (past.abs() / 3.).clamp(2., 28.) * past.signum();
            let (now, most) = (f32::from(scroll.offset().x), f32::from(scroll.max_offset().x));
            let to = (now - step).clamp(-most, 0.);
            if (to - now).abs() > 0.01 {
                scroll.set_offset(point(px(to), scroll.offset().y));
                window.request_animation_frame();
            }
        },
    )
}

/// A context for rendering nodes, contains link references.
#[derive(Default, Clone)]
pub(crate) struct NodeContext {
    /// The byte offset of the node in the original markdown text.
    /// Used for incremental updates.
    pub(crate) offset: usize,
    pub(crate) link_refs: HashMap<SharedString, LinkMark>,
    pub(crate) style: TextViewStyle,
    pub(crate) code_block_actions: Option<Arc<CodeBlockActionsFn>>,
    pub(crate) link_click_handler: Option<Arc<LinkClickHandlerFn>>,
    pub(crate) markdown_extensions: Arc<MarkdownExtensions>,
    /// The markdown being parsed, which node positions count in: a link
    /// keeps what it was written as. (Emaki addition.)
    pub(crate) source: Option<Arc<str>>,
}

impl NodeContext {
    pub(super) fn add_ref(&mut self, identifier: SharedString, link: LinkMark) {
        self.link_refs.insert(identifier, link);
    }
}

impl PartialEq for NodeContext {
    fn eq(&self, other: &Self) -> bool {
        self.link_refs == other.link_refs && self.style == other.style
        // Note: code_block_actions and markdown_extensions are intentionally
        // not compared (closures can't be compared)
    }
}

impl Paragraph {
    fn render(&self, node_cx: &NodeContext, _window: &mut Window, cx: &mut App) -> AnyElement {
        let span = self.span;
        let children = &self.children;

        if self.should_render_inline_flow() {
            return InlineFlow::new(
                span.unwrap_or_default(),
                self.inline_flow_items(node_cx, cx),
                node_cx.link_click_handler.clone(),
            )
            .with_math_family(node_cx.markdown_extensions.math_face().map(|f| f.family.clone()))
            .into_any_element();
        }

        let mut child_nodes: Vec<AnyElement> = vec![];

        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = vec![];
        // The inline code spans, in the same offsets as `highlights`, for
        // the mono face `Inline` gives them. (Emaki addition.)
        let mut code_ranges: Vec<Range<usize>> = vec![];
        // Quarto's attribute chips, apart: their plate is the quiet one.
        // (Emaki addition.)
        let mut attr_ranges: Vec<Range<usize>> = vec![];
        let attr_chip = (cx.theme().muted, cx.theme().border);
        // Formulas: their rooms and pictures, and the face the rooms are
        // set in. (Emaki addition.)
        let math_family = node_cx.markdown_extensions.math_face().map(|f| f.family.clone());
        let mut math_items: Vec<(Range<usize>, crate::text::math::MathPicture, bool)> = vec![];
        let mut links: Vec<(Range<usize>, LinkMark)> = vec![];
        let mut offset = 0;

        // A formula alone on a line of its own: the line as tall as its
        // picture, centered, its number at the right. (Emaki addition.)
        if let [only] = children.as_slice() {
            if let Some(math) = only.math.as_ref().filter(|m| m.display) {
                if let Some(picture) = math.picture.clone() {
                    let text: SharedString = only.text.clone();
                    if let Ok(mut state) = self.state.lock() {
                        state.set_text(text.clone());
                    }
                    let room = 0..text.len();
                    let clear = HighlightStyle { color: Some(gpui::transparent_black()), ..Default::default() };
                    let line = Inline::new(0, self.state.clone(), vec![], vec![(room.clone(), clear)], node_cx.link_click_handler.clone())
                        .with_math(math_family, vec![(room, picture.clone(), true)]);
                    let tag = math.tag.clone().map(|t| div().pl_4().flex_shrink_0().whitespace_nowrap().child(format!("({t})")));
                    return div()
                        .id(span.unwrap_or_default())
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("math-room")
                                .min_w_0()
                                .overflow_x_scroll()
                                .line_height(relative(picture.height + 0.5))
                                .whitespace_nowrap()
                                .child(line),
                        )
                        .child(div().flex_1().flex().flex_row().justify_end().children(tag))
                        .into_any_element();
                }
            }
        }

        let mut ix = 0;
        for inline_node in children {
            let text_len = inline_node.text.len();
            text.push_str(&inline_node.text);

            if let Some(image) = &inline_node.image {
                if text.len() > 0 {
                    if let Ok(mut state) = inline_node.state.lock() {
                        state.set_text(text.clone().into());
                    }
                    child_nodes.push(
                        Inline::new(
                            ix,
                            inline_node.state.clone(),
                            links.clone(),
                            highlights.clone(),
                            node_cx.link_click_handler.clone(),
                        )
                        .with_mono(node_cx.style.inline_code_font_family.clone(), code_ranges.iter().chain(attr_ranges.iter()).cloned().collect())
                        .with_chip(node_cx.style.inline_code_chip, code_ranges.clone())
                        .with_attr_chip(attr_chip, attr_ranges.clone())
                        .with_math(math_family.clone(), std::mem::take(&mut math_items))
                        .into_any_element(),
                    );
                }
                let link_click_handler = node_cx.link_click_handler.clone();
                child_nodes.push(
                    img(image_source(&image.url))
                        .id(ix)
                        .object_fit(ObjectFit::Contain)
                        .max_w(relative(1.))
                        .when_some(image.width, |this, width| this.w(width))
                        // Where a figure stands (Quarto's `fig-align`).
                        .when(image.align == Some(ImageAlign::Center), |this| this.mx_auto())
                        .when(image.align == Some(ImageAlign::Right), |this| this.ml_auto())
                        .when_some(image.link.clone(), |this, link| {
                            let title = image.title();
                            let link_click_handler = link_click_handler.clone();
                            let aux_link = link.clone();
                            let aux_link_click_handler = link_click_handler.clone();
                            this.cursor_pointer()
                                .tooltip(move |window, cx| {
                                    Tooltip::new(title.clone()).build(window, cx)
                                })
                                .on_click(move |event, window, cx| {
                                    gpui_base::TextSelection::end(window, cx);
                                    cx.stop_propagation();
                                    handle_link_click(
                                        &link_click_handler,
                                        link.url.clone(),
                                        event.clone(),
                                        window,
                                        cx,
                                    );
                                })
                                .on_aux_click(move |event, window, cx| {
                                    gpui_base::TextSelection::end(window, cx);
                                    cx.stop_propagation();
                                    handle_link_click(
                                        &aux_link_click_handler,
                                        aux_link.url.clone(),
                                        event.clone(),
                                        window,
                                        cx,
                                    );
                                })
                        })
                        .into_any_element(),
                );

                text.clear();
                links.clear();
                highlights.clear();
                code_ranges.clear();
                offset = 0;
            } else {
                let mut node_highlights = vec![];
                for (range, style) in &inline_node.marks {
                    let inner_range = (offset + range.start)..(offset + range.end);

                    let mut highlight = HighlightStyle::default();
                    if style.bold {
                        highlight.font_weight =
                            Some(node_cx.style.strong_font_weight.unwrap_or(FontWeight::BOLD));
                    }
                    if style.italic {
                        highlight.font_style = Some(FontStyle::Italic);
                    }
                    if style.strikethrough {
                        highlight.strikethrough = Some(gpui::StrikethroughStyle {
                            thickness: gpui::px(1.),
                            ..Default::default()
                        });
                    }
                    if style.underline {
                        highlight.underline = Some(gpui::UnderlineStyle {
                            thickness: gpui::px(1.),
                            ..Default::default()
                        });
                    }
                    if style.attr {
                        highlight.color = Some(cx.theme().muted_foreground);
                        attr_ranges.push(inner_range.clone());
                    } else if style.code {
                        highlight = highlight.highlight(node_cx.style.inline_code_highlight(cx));
                        code_ranges.push(inner_range.clone());
                    }
                    if let Some(color) = style.highlight {
                        highlight.background_color = Some(color);
                    }

                    if let Some(mut link_mark) = style.link.clone() {
                        highlight.color = Some(cx.theme().link);
                        highlight.underline = Some(gpui::UnderlineStyle {
                            thickness: gpui::px(1.),
                            ..Default::default()
                        });

                        // convert link references, replace link
                        if let Some(identifier) = link_mark.identifier.as_ref() {
                            if let Some(mark) = node_cx.link_refs.get(identifier) {
                                link_mark = LinkMark { raw: link_mark.raw.take(), ..mark.clone() };
                            }
                        }

                        links.push((inner_range.clone(), link_mark));
                    }

                    node_highlights.push((inner_range, highlight));
                }
                // A formula's room: its own run, clear, its picture over it.
                if let Some(picture) = inline_node.math.as_ref().and_then(|m| m.picture.clone().map(|p| (p, m.display))) {
                    let room = offset..offset + text_len;
                    node_highlights.push((room.clone(), HighlightStyle { color: Some(gpui::transparent_black()), ..Default::default() }));
                    math_items.push((room, picture.0, picture.1));
                }

                highlights = gpui::combine_highlights(highlights, node_highlights).collect();
                offset += text_len;
            }
            ix += 1;
        }

        // Add the last text node
        if text.len() > 0 {
            if let Ok(mut state) = self.state.lock() {
                state.set_text(text.into());
            }
            child_nodes.push(
                Inline::new(
                    ix,
                    self.state.clone(),
                    links,
                    highlights,
                    node_cx.link_click_handler.clone(),
                )
                .with_mono(node_cx.style.inline_code_font_family.clone(), code_ranges.iter().chain(attr_ranges.iter()).cloned().collect())
                .with_chip(node_cx.style.inline_code_chip, code_ranges)
                .with_attr_chip(attr_chip, attr_ranges)
                .with_math(math_family, math_items)
                .into_any_element(),
            );
        }

        // A figure (Quarto): its caption under the picture, quiet and
        // centered. The paragraph stays a block: the picture is put where
        // `fig-align` says by its margins (below), since in a column of
        // flex the picture kept the height of its written width while the
        // pane cut its width, and stood in a box too tall. (Emaki addition.)
        let figure = children.iter().find_map(|n| n.image.as_ref()).filter(|_| children.len() == 1);
        let caption = figure.and_then(|i| i.caption.clone());
        div()
            .id(span.unwrap_or_default())
            .children(child_nodes)
            .when_some(caption, |d, caption| {
                d.child(div().w_full().pt(rems(0.4)).text_center().text_size(rems(0.875)).text_color(cx.theme().muted_foreground).child(caption))
            })
            .into_any_element()
    }

    fn should_render_inline_flow(&self) -> bool {
        let has_image = self.children.iter().any(|child| child.image.is_some());
        let has_text = self.children.iter().any(|child| !child.text.is_empty());
        has_image && has_text
    }

    fn inline_flow_items(&self, node_cx: &NodeContext, cx: &mut App) -> Vec<InlineFlowItem> {
        let mut items = Vec::new();
        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = vec![];
        let mut links: Vec<(Range<usize>, LinkMark)> = vec![];
        let mut math: crate::text::inline_flow::MathRooms = vec![];
        let mut offset = 0;

        for inline_node in &self.children {
            let text_len = inline_node.text.len();
            text.push_str(&inline_node.text);

            if let Some(image) = &inline_node.image {
                if !text.is_empty() {
                    if let Ok(mut state) = inline_node.state.lock() {
                        state.set_text(text.clone().into());
                    }
                    items.push(InlineFlowItem::Text {
                        state: inline_node.state.clone(),
                        text: text.clone().into(),
                        links: links.clone(),
                        highlights: highlights.clone(),
                        math: std::mem::take(&mut math),
                    });
                }

                items.push(InlineFlowItem::Image {
                    url: image.url.clone(),
                    link: image.link.clone(),
                    title: image.title(),
                    width: image.width,
                    height: image.height,
                });

                text.clear();
                links.clear();
                highlights.clear();
                offset = 0;
            } else {
                let mut node_highlights = vec![];
                for (range, style) in &inline_node.marks {
                    let inner_range = (offset + range.start)..(offset + range.end);

                    let mut highlight = HighlightStyle::default();
                    if style.bold {
                        highlight.font_weight =
                            Some(node_cx.style.strong_font_weight.unwrap_or(FontWeight::BOLD));
                    }
                    if style.italic {
                        highlight.font_style = Some(FontStyle::Italic);
                    }
                    if style.strikethrough {
                        highlight.strikethrough = Some(gpui::StrikethroughStyle {
                            thickness: gpui::px(1.),
                            ..Default::default()
                        });
                    }
                    if style.underline {
                        highlight.underline = Some(gpui::UnderlineStyle {
                            thickness: gpui::px(1.),
                            ..Default::default()
                        });
                    }
                    if style.attr {
                        highlight.color = Some(cx.theme().muted_foreground);
                    } else if style.code {
                        highlight = highlight.highlight(node_cx.style.inline_code_highlight(cx));
                    }
                    if let Some(color) = style.highlight {
                        highlight.background_color = Some(color);
                    }

                    if let Some(mut link_mark) = style.link.clone() {
                        highlight.color = Some(cx.theme().link);
                        highlight.underline = Some(gpui::UnderlineStyle {
                            thickness: gpui::px(1.),
                            ..Default::default()
                        });

                        if let Some(identifier) = link_mark.identifier.as_ref()
                            && let Some(mark) = node_cx.link_refs.get(identifier)
                        {
                            link_mark = LinkMark { raw: link_mark.raw.take(), ..mark.clone() };
                        }

                        links.push((inner_range.clone(), link_mark));
                    }

                    node_highlights.push((inner_range, highlight));
                }
                if let Some(m) = inline_node.math.as_ref() {
                    if let Some(picture) = m.picture.clone() {
                        let room = offset..offset + text_len;
                        node_highlights.push((room.clone(), HighlightStyle { color: Some(gpui::transparent_black()), ..Default::default() }));
                        math.push((room, picture, m.display));
                    }
                }

                highlights = gpui::combine_highlights(highlights, node_highlights).collect();
                offset += text_len;
            }
        }

        if !text.is_empty() {
            if let Ok(mut state) = self.state.lock() {
                state.set_text(text.clone().into());
            }
            items.push(InlineFlowItem::Text {
                state: self.state.clone(),
                text: text.into(),
                links,
                highlights,
                math,
            });
        }

        items
    }
}

impl Paragraph {
    fn to_markdown(&self) -> String {
        let mut text = self
            .children
            .iter()
            .map(|text_node| {
                let mut text = text_node.text.to_string();
                for (range, style) in &text_node.marks {
                    if style.bold {
                        text = format!("**{}**", &text_node.text[range.clone()]);
                    }
                    if style.italic {
                        text = format!("*{}*", &text_node.text[range.clone()]);
                    }
                    if style.strikethrough {
                        text = format!("~~{}~~", &text_node.text[range.clone()]);
                    }
                    if style.code {
                        text = format!("`{}`", &text_node.text[range.clone()]);
                    }
                    if style.highlight.is_some() {
                        text = format!("=={}==", &text_node.text[range.clone()]);
                    }
                    if let Some(link) = &style.link {
                        text = format!("[{}]({})", &text_node.text[range.clone()], link.url);
                    }
                }

                if let Some(image) = &text_node.image {
                    let alt = image.alt.clone().unwrap_or_default();
                    let title = image
                        .title
                        .clone()
                        .map_or(String::new(), |t| format!(" \"{}\"", t));
                    text.push_str(&format!("![{}]({}{})", alt, image.url, title))
                }

                text
            })
            .collect::<Vec<_>>()
            .join("");

        text.push_str("\n\n");
        text
    }
}

impl BlockNode {
    /// Converts the node to markdown format.
    ///
    /// This is used to generate markdown for test.
    #[allow(dead_code)]
    pub(crate) fn to_markdown(&self) -> String {
        match self {
            BlockNode::Root { children, .. } => children
                .iter()
                .map(|child| child.to_markdown())
                .collect::<Vec<_>>()
                .join("\n\n"),
            BlockNode::Paragraph(paragraph) => paragraph.to_markdown(),
            BlockNode::Heading {
                level, children, ..
            } => {
                let hashes = "#".repeat(*level as usize);
                format!("{} {}", hashes, children.to_markdown())
            }
            BlockNode::Div { attrs, children, .. } => {
                let content = children
                    .iter()
                    .map(|child| child.to_markdown())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                format!("::: {{{}}}\n{content}\n:::", attrs.text)
            }
            BlockNode::Blockquote { children, .. } => {
                let content = children
                    .iter()
                    .map(|child| child.to_markdown())
                    .collect::<Vec<_>>()
                    .join("\n\n");

                content
                    .lines()
                    .map(|line| format!("> {}", line))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            BlockNode::List {
                children, ordered, ..
            } => children
                .iter()
                .enumerate()
                .map(|(i, child)| {
                    let prefix = if *ordered {
                        format!("{}. ", i + 1)
                    } else {
                        "- ".to_string()
                    };
                    format!("{}{}", prefix, child.to_markdown())
                })
                .collect::<Vec<_>>()
                .join("\n"),
            BlockNode::ListItem {
                children, checked, ..
            } => {
                let checkbox = if let Some(checked) = checked {
                    if *checked { "[x] " } else { "[ ] " }
                } else {
                    ""
                };
                format!(
                    "{}{}",
                    checkbox,
                    children
                        .iter()
                        .map(|child| child.to_markdown())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
            BlockNode::CodeBlock(code_block) => {
                format!(
                    "```{}\n{}\n```",
                    code_block.lang.clone().unwrap_or_default(),
                    code_block.code()
                )
            }
            BlockNode::Table(table) => {
                let header = table
                    .children
                    .first()
                    .map(|row| {
                        row.children
                            .iter()
                            .map(|cell| cell.children.to_markdown())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    })
                    .unwrap_or_default();
                let alignments = table
                    .column_aligns
                    .iter()
                    .map(|align| {
                        match align {
                            ColumnumnAlign::Left => ":--",
                            ColumnumnAlign::Center => ":-:",
                            ColumnumnAlign::Right => "--:",
                        }
                        .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                let rows = table
                    .children
                    .iter()
                    .skip(1)
                    .map(|row| {
                        row.children
                            .iter()
                            .map(|cell| cell.children.to_markdown())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{}\n{}\n{}", header, alignments, rows)
            }
            BlockNode::Break { html, .. } => {
                if *html {
                    "<br>".to_string()
                } else {
                    "\n".to_string()
                }
            }
            BlockNode::HorizontalRule { .. } => "---".to_string(),
            BlockNode::Custom(node) => node.to_markdown(),
            BlockNode::Definition {
                identifier,
                url,
                title,
                ..
            } => {
                if let Some(title) = title {
                    format!("[{}]: {} \"{}\"", identifier, url, title)
                } else {
                    format!("[{}]: {}", identifier, url)
                }
            }
            BlockNode::Unknown { .. } => "".to_string(),
        }
        .trim()
        .to_string()
    }
}

impl BlockNode {
    fn render_list_item_row(
        content: AnyElement,
        ix: usize,
        options: NodeRenderOptions,
        checked: Option<bool>,
        cx: &mut App,
    ) -> Div {
        h_flex()
            .w_full()
            .flex_1()
            .min_w_0()
            .relative()
            .items_start()
            .content_start()
            .when(!options.todo && checked.is_none() && options.ordered, |this| {
                this.child(list_item_prefix(ix, options.ordered, options.depth))
            })
            // Emaki: an item of a list that is not numbered is marked by a
            // hairline, a short thin line at the middle of its first line,
            // shorter at each depth, in the quiet ink: lines only, as the
            // window's own drawings are. The empty character makes the
            // mark's box one line of the item's text tall. Upstream drew
            // "•", "◦" and "▪".
            .when(!options.todo && checked.is_none() && !options.ordered, |this| {
                let long = if options.depth == 0 { 0.55 } else { 0.35 };
                this.child(
                    div()
                        .relative()
                        .flex_shrink_0()
                        .w(rems(1.05))
                        .child("\u{200B}")
                        .child(
                            div()
                                .absolute()
                                .left(rems(0.1))
                                .top(relative(0.54))
                                .w(rems(long))
                                .h(px(1.))
                                .bg(cx.theme().muted_foreground),
                        ),
                )
            })
            .when_some(checked, |this, checked| {
                // Todo list checkbox
                this.child(
                    div()
                        .flex()
                        .mt(rems(0.4))
                        .mr_1p5()
                        .size(rems(0.875))
                        .items_center()
                        .justify_center()
                        .rounded(cx.theme().radius.half())
                        .border_1()
                        .border_color(cx.theme().primary)
                        .text_color(cx.theme().primary_foreground)
                        .when(checked, |this| {
                            this.bg(cx.theme().tokens.primary)
                                .child(Icon::new(IconName::Check).size_2().text_xs())
                        }),
                )
            })
            .child(div().flex_1().min_w_0().overflow_hidden().child(content))
    }

    fn render_list_item(
        item: &BlockNode,
        ix: usize,
        options: NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        match item {
            BlockNode::ListItem {
                children,
                spread,
                checked,
                ..
            } => v_flex()
                .id(("li", options.ix))
                .w_full()
                .min_w_0()
                .when(*spread, |this| this.child(div()))
                .children({
                    let mut items: Vec<Div> = Vec::with_capacity(children.len());

                    for (child_ix, child) in children.iter().enumerate() {
                        match child {
                            BlockNode::Paragraph { .. } => {
                                let last_not_list = child_ix > 0
                                    && !matches!(children[child_ix - 1], BlockNode::List { .. });

                                let text = child.render_block(
                                    NodeRenderOptions {
                                        depth: options.depth + 1,
                                        todo: checked.is_some(),
                                        is_last: true,
                                        ..options
                                    },
                                    node_cx,
                                    window,
                                    cx,
                                );

                                // Continuation paragraph — stack vertically below
                                // the previous row, indented to align with the text
                                // column (past bullet/number prefix).
                                if last_not_list {
                                    if let Some(preceding_row) = items.pop() {
                                        items.push(
                                            v_flex().child(preceding_row).child(
                                                div()
                                                    .w_full()
                                                    .pl(rems(1.))
                                                    .overflow_hidden()
                                                    .child(text),
                                            ),
                                        );
                                        continue;
                                    }
                                }

                                items.push(Self::render_list_item_row(
                                    text, ix, options, *checked, cx,
                                ));
                            }
                            BlockNode::List { .. } => {
                                items.push(div().ml(rems(1.)).child(child.render_block(
                                    NodeRenderOptions {
                                        depth: options.depth + 1,
                                        todo: checked.is_some(),
                                        is_last: true,
                                        ..options
                                    },
                                    node_cx,
                                    window,
                                    cx,
                                )));
                            }
                            BlockNode::Root { .. }
                            | BlockNode::Div { .. }
                            | BlockNode::Heading { .. }
                            | BlockNode::Blockquote { .. }
                            | BlockNode::CodeBlock(_)
                            | BlockNode::Custom(_)
                            | BlockNode::Table(_)
                            | BlockNode::HorizontalRule { .. } => {
                                let block = child.render_block(
                                    NodeRenderOptions {
                                        depth: options.depth + 1,
                                        todo: checked.is_some(),
                                        is_last: true,
                                        ..options
                                    },
                                    node_cx,
                                    window,
                                    cx,
                                );

                                if child_ix == 0 {
                                    items.push(Self::render_list_item_row(
                                        block, ix, options, *checked, cx,
                                    ));
                                } else {
                                    // Indent continuation blocks to align with a
                                    // nested sub-list (`ml(rems(1.))`) and with
                                    // continuation paragraphs.
                                    items.push(
                                        div()
                                            .w_full()
                                            .min_w_0()
                                            .pl(rems(1.))
                                            .overflow_hidden()
                                            .child(block),
                                    );
                                }
                            }
                            BlockNode::ListItem { .. }
                            | BlockNode::Break { .. }
                            | BlockNode::Definition { .. }
                            | BlockNode::Unknown => {}
                        }
                    }
                    items
                })
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }

    /// Render a Markdown table. Dispatches to a horizontally scrollable layout
    /// when `style.table` opts in with overflow-x: scroll, otherwise to the
    /// default layout that fits the container width and wraps cell content.
    fn render_table(
        item: &BlockNode,
        options: &NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        const DEFAULT_LENGTH: usize = 5;

        let table = match item {
            BlockNode::Table(table) => table,
            _ => return div().into_any_element(),
        };

        // Per-column max text length (in chars), used to proportion the columns
        // in the default (wrap) layout.
        let mut col_lens: Vec<usize> = vec![];
        for row in table.children.iter() {
            for (ix, cell) in row.children.iter().enumerate() {
                if col_lens.len() <= ix {
                    col_lens.push(DEFAULT_LENGTH);
                }
                col_lens[ix] = col_lens[ix].max(cell.children.text_len());
            }
        }

        // Scroll mode is opted in via `style.table` overflow-x: scroll.
        if matches!(node_cx.style.table.overflow.x, Some(Overflow::Scroll)) {
            Self::render_scroll_table(table, col_lens.len(), options, node_cx, window, cx)
        } else {
            Self::render_wrap_table(table, &col_lens, options, node_cx, window, cx)
        }
    }

    /// Horizontally scrollable table layout (opt-in via `style.table`
    /// overflow-x: scroll).
    ///
    /// Column widths come from the **measured** shaped text of each cell (the
    /// widest per column across all rows), so columns line up and fit their
    /// content exactly — char-count heuristics are inaccurate on proportional
    /// fonts. The layout adapts to the frame like CSS auto table layout:
    ///
    /// - Wider than the content: cells `flex_grow` proportionally to fill.
    /// - Narrower: columns shrink and their text wraps, but not below a
    ///   per-column floor.
    /// - Narrower than the floors: the table keeps the floor widths and
    ///   scrolls horizontally, so no content ever becomes unreachable.
    ///
    /// `white_space: nowrap` on `style.table_cell` composes like in CSS: the
    /// refinement keeps cell text on a single line, and the floors are raised
    /// to the full content widths so the single-line columns never shrink —
    /// the table scrolls as soon as the content is wider than the frame.
    fn render_scroll_table(
        table: &Table,
        col_count: usize,
        options: &NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        const CELL_PAD_PX: f32 = 16.0; // px_2 horizontal padding
        const CELL_MIN_PX: f32 = 48.0;
        // Shrinking columns stop (and the table starts to scroll) at a floor
        // scaled to their content: roughly the width at which the text wraps
        // to `CELL_WRAP_MAX_LINES` lines, clamped between the two bounds so
        // moderate columns can still wrap meaningfully while one huge column
        // cannot push the scroll threshold arbitrarily high.
        const CELL_WRAP_MAX_LINES: f32 = 2.0;
        const CELL_WRAP_MIN_PX: f32 = 160.0;
        const CELL_WRAP_MAX_PX: f32 = 480.0;
        const CELL_BORDER_PX: f32 = 1.0; // border_r_1 drawn by every column but the last
        const TABLE_BORDER_PX: f32 = 2.0; // the track's border_1, left + right

        // Measure the widest text per column (max-content width). Never
        // capped: a cap would clip overflowing text *and* leave it outside
        // the scrollable width, making it unreachable.
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let mut col_w = vec![CELL_MIN_PX; col_count];
        for row in table.children.iter() {
            for (ix, cell) in row.children.iter().enumerate() {
                let Some(slot) = col_w.get_mut(ix) else {
                    continue;
                };
                let mut w = 0.0_f32;
                for line in cell.children.text().split('\n') {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let run = text_style.to_run(line.len());
                    let line_w = window
                        .text_system()
                        .layout_line(line, font_size, &[run], None)
                        .width;
                    w = w.max(f32::from(line_w));
                }
                // Border-box widths, so the padding and border the cell draws
                // must leave the measured text its full width.
                let border = if ix + 1 < col_count {
                    CELL_BORDER_PX
                } else {
                    0.
                };
                *slot = slot.max(w + CELL_PAD_PX + border);
            }
        }
        let style = &node_cx.style;
        // Nowrap cells (via the `table_cell` refinement, which cascades to
        // the cell text) must never shrink below their single-line content,
        // so their floor is the content width itself.
        let nowrap = style.table_cell.text.white_space == Some(WhiteSpace::Nowrap);
        let col_min_w: Vec<f32> = if nowrap {
            col_w.clone()
        } else {
            col_w
                .iter()
                .map(|w| {
                    (w / CELL_WRAP_MAX_LINES)
                        .clamp(CELL_WRAP_MIN_PX, CELL_WRAP_MAX_PX)
                        .min(*w)
                })
                .collect()
        };
        let min_total_w: f32 = col_min_w.iter().sum::<f32>() + TABLE_BORDER_PX;

        let table_scroll_key = if let Some(span) = table.span {
            SharedString::from(format!(
                "{}-table-scroll-{}:{}",
                window.current_view(),
                span.start,
                span.end
            ))
        } else {
            SharedString::from(format!(
                "{}-table-scroll-{}",
                window.current_view(),
                options.ix
            ))
        };
        let scroll_handle = window
            .use_keyed_state(table_scroll_key, cx, |_, _| ScrollHandle::default())
            .read(cx)
            .clone();
        let row_count = table.children.len();
        let mut rows = Vec::with_capacity(row_count);
        for (row_ix, row) in table.children.iter().enumerate() {
            let mut cells = Vec::with_capacity(row.children.len());
            for (ix, cell) in row.children.iter().enumerate() {
                let align = table.column_align(ix);
                let is_last_col = ix == row.children.len() - 1;
                let width = col_w.get(ix).copied().unwrap_or(CELL_MIN_PX);
                let min_width = col_min_w.get(ix).copied().unwrap_or(CELL_MIN_PX);
                cells.push(
                    div()
                        .id(("cell", ix))
                        // Measured max-content width is the flex-basis;
                        // `flex_grow` (proportional to it) distributes extra
                        // space so a narrow table still fills the frame, while
                        // shrinking is clamped at `min_w` — the flex engine
                        // squeezes columns (their text wraps) down to the
                        // floors before the track starts to scroll.
                        .flex_basis(px(width))
                        .flex_grow(width)
                        .flex_shrink(1.)
                        .min_w(px(min_width))
                        .overflow_hidden()
                        .when(align == ColumnumnAlign::Center, |this| this.text_center())
                        .when(align == ColumnumnAlign::Right, |this| this.text_right())
                        .px_2()
                        .py_1()
                        .when(!is_last_col, |this| {
                            this.border_r_1().border_color(cx.theme().border)
                        })
                        .refine_style(&style.table_cell)
                        .child(cell.children.render(node_cx, window, cx)),
                );
            }
            rows.push(
                div()
                    .id("row")
                    .w_full()
                    .when(row_ix < row_count - 1, |this| this.border_b_1())
                    .border_color(cx.theme().border)
                    .flex()
                    .flex_row()
                    .children(cells),
            );
        }

        div()
            .pb(rems(1.))
            .w_full()
            .child(
                // Scroll viewport: clips and scrolls horizontally (overflow-x
                // is handled by `ScrollableMask`, so vertical wheel events keep
                // bubbling to the parent TextView). No border — the frame is on
                // the inner track so it wraps the table tightly.
                horizontal_scroll_area(
                    ("table", options.ix),
                    &scroll_handle,
                    &style.table,
                    // Bordered track sized to `max(viewport, column floors)`:
                    // `min_w_full` fills the frame while the columns can still
                    // shrink-to-fit (their text wrapping), the definite
                    // `w(min_total_w)` keeps the floors once they are reached,
                    // letting the track exceed the viewport and scroll.
                    div()
                        .min_w_full()
                        .w(px(min_total_w))
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius)
                        .children(rows),
                ),
            )
            .into_any_element()
    }

    /// A file's header of fields, written as a table of two columns: a
    /// field a row, the key in a quiet column as wide as the longest, the
    /// value beside it, both in the mono face as the header is written,
    /// a hairline between rows. The band around them is the caller's.
    /// Its cells are the table's, so a selection runs through them.
    /// (Emaki addition.)
    fn render_fields(
        table: &Table,
        options: &NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let font_size = f32::from(window.text_style().font_size.to_pixels(window.rem_size())) * 0.875;
        // The mono face's letters are one width, about 0.6 of its size.
        let longest = table.children.iter().filter_map(|row| row.children.first()).map(|cell| cell.children.text().chars().count()).max().unwrap_or(0);
        let key_w = longest as f32 * font_size * 0.6 + 2.;
        let rows = table.children.len();
        div()
            .id(("fields", options.ix))
            .w_full()
            .font_family(theme.mono_font_family.clone())
            .text_size(px(font_size))
            .line_height(relative(1.55))
            .children(table.children.iter().enumerate().map(|(ix, row)| {
                let mut cells = row.children.iter();
                let key = cells.next();
                let value = cells.next();
                div()
                    .id(("field", ix))
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(18.))
                    .py(px(4.))
                    .when(ix + 1 < rows, |d| d.border_b_1().border_color(theme.border.opacity(0.6)))
                    .child(
                        div()
                            .flex_shrink_0()
                            .w(px(key_w))
                            .max_w(relative(0.4))
                            .overflow_hidden()
                            .text_color(theme.muted_foreground)
                            .children(key.map(|c| c.children.render(node_cx, window, cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(theme.foreground)
                            .children(value.map(|c| c.children.render(node_cx, window, cx))),
                    )
            }))
            .into_any_element()
    }

    /// Default table layout: a flex grid whose columns are proportioned by
    /// content length and shrink to fit the container width (cell text wraps).
    fn render_wrap_table(
        table: &Table,
        col_lens: &[usize],
        options: &NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        const MAX_LENGTH: usize = 150;

        let style = &node_cx.style;
        let row_count = table.children.len();
        // A column is never narrower than its longest word, up to a
        // point: sized by the length of its text alone, a column of short
        // words beside one of long text was given so little that a word
        // broke in its middle. The most a word may ask for is less with
        // more columns, so the floors together stay inside a narrow pane.
        // (Emaki addition.)
        let font_size = f32::from(window.text_style().font_size.to_pixels(window.rem_size()));
        let word_cap = (60 / col_lens.len().max(1)).clamp(6, 20);
        let mut col_word: Vec<usize> = vec![0; col_lens.len()];
        for row in table.children.iter() {
            for (ix, cell) in row.children.iter().enumerate() {
                let longest = cell.children.text().split_whitespace().map(|w| w.chars().count()).max().unwrap_or(0);
                if let Some(slot) = col_word.get_mut(ix) {
                    *slot = (*slot).max(longest.min(word_cap));
                }
            }
        }
        let mut rows = Vec::with_capacity(row_count);
        for (row_ix, row) in table.children.iter().enumerate() {
            let mut cells = Vec::with_capacity(row.children.len());
            for (ix, cell) in row.children.iter().enumerate() {
                let align = table.column_align(ix);
                let is_last_col = ix == row.children.len() - 1;
                let len = col_lens
                    .get(ix)
                    .copied()
                    .unwrap_or(MAX_LENGTH)
                    .min(MAX_LENGTH);
                let word_w = col_word.get(ix).copied().unwrap_or(0) as f32 * font_size * 0.62 + 18.;

                cells.push(
                    div()
                        .id(("cell", ix))
                        .overflow_hidden()
                        .when(align == ColumnumnAlign::Center, |this| this.text_center())
                        .when(align == ColumnumnAlign::Right, |this| this.text_right())
                        .min_w(px(word_w.max(64.)))
                        .w(Length::Definite(relative(len as f32)))
                        .px_2()
                        .py_1()
                        .when(!is_last_col, |this| {
                            this.border_r_1().border_color(cx.theme().border)
                        })
                        .refine_style(&style.table_cell)
                        .child(cell.children.render(node_cx, window, cx)),
                );
            }

            rows.push(
                div()
                    .id("row")
                    .w_full()
                    .when(row_ix < row_count - 1, |this| this.border_b_1())
                    .border_color(cx.theme().border)
                    .flex()
                    .flex_row()
                    .children(cells),
            );
        }

        div()
            .pb(rems(1.))
            .w_full()
            .child(
                div()
                    .id(("table", options.ix))
                    .w_full()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius)
                    .overflow_hidden()
                    .children(rows)
                    .refine_style(&style.table),
            )
            .into_any_element()
    }

    /// A Quarto fenced div as its class says (Emaki addition):
    /// a callout (`.callout-note`, `-tip`, `-warning`, `-caution`,
    /// `-important`) is a card with a title, its kind's color at the
    /// left; `.hidden` is not drawn; `.columns` sets its `.column`
    /// children side by side, each as wide as its `width` says;
    /// `layout-ncol=N` sets its children in rows of N; a `style` with
    /// `text-align: center` centers what it holds. Any other is what it
    /// holds, with its attributes in a quiet chip at its top and a
    /// hairline at its left, so its extent can be seen.
    fn render_div(
        attrs: &DivAttrs,
        chip_words: &Paragraph,
        children: &[BlockNode],
        options: NodeRenderOptions,
        mb: gpui::Rems,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let ix = options.ix;
        if attrs.has_class("hidden") {
            return div().into_any_element();
        }
        if attrs.has_class(crate::text::format::quarto::MATH_CLASS) {
            let n = children.len();
            return div()
                .id(("math", ix))
                .w_full()
                .pb(mb)
                .text_center()
                .children(children.iter().enumerate().map(|(i, c)| c.render_block(NodeRenderOptions { ix: i, is_last: i + 1 == n, ..options }, node_cx, window, cx)))
                .into_any_element();
        }
        let theme = cx.theme().clone();
        let render_children = |children: &[BlockNode], window: &mut Window, cx: &mut App| -> Vec<AnyElement> {
            let n = children.len();
            children.iter().enumerate().map(|(i, c)| c.render_block(NodeRenderOptions { ix: i, is_last: i + 1 == n, ..options }, node_cx, window, cx)).collect()
        };
        let chip = |chip: &Paragraph, window: &mut Window, cx: &mut App| {
            div()
                .flex_shrink_0()
                .px(px(6.))
                .py(px(1.))
                .rounded(px(5.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().muted)
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(cx.theme().muted_foreground)
                .child(chip.render(node_cx, window, cx))
        };
        let callout = attrs.classes.iter().find_map(|c| c.strip_prefix("callout-")).map(str::to_string).or_else(|| attrs.has_class("callout").then(|| "note".to_string()));
        if let Some(kind) = callout {
            return Self::render_callout(&kind, attrs, children, options, mb, node_cx, window, cx);
        }
        // A theorem, a proof, an abstract and the like read as the words
        // they hold, the bold lead the file was read with first: no chip,
        // no rule. (Emaki addition.)
        // The margin layout's row: the block of the text, and beside it
        // the notes that stand level with it. The two share the width
        // three to one; where the pane is too narrow for both, the notes
        // wrap under the block, across its width. (Emaki addition.)
        if attrs.has_class(crate::text::format::quarto::MARGIN_ROW) {
            let grow = |d: gpui::Div, by: f32, basis: f32| {
                let mut d = d.min_w_0().flex_basis(px(basis));
                d.style().flex_grow = Some(by);
                d.style().flex_shrink = Some(1.);
                d
            };
            let (main, notes) = children.split_first().map(|(m, n)| (Some(m), n)).unwrap_or((None, &[]));
            let main = main.map(|m| m.render_block(NodeRenderOptions { ix: 0, ..options }, node_cx, window, cx));
            let n = notes.len();
            let notes = notes.iter().enumerate().map(|(i, c)| c.render_block(NodeRenderOptions { ix: i + 1, is_last: i + 1 == n, ..options }, node_cx, window, cx)).collect::<Vec<_>>();
            return div()
                .id(("margin-row", ix))
                .w_full()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_start()
                .gap_x(px(28.))
                .child(grow(div(), 3., 380.).children(main))
                .child(grow(div(), 1., 170.).pb(if n > 0 { mb } else { rems(0.) }).children(notes))
                .into_any_element();
        }
        // A margin note: smaller and quieter than the text.
        if attrs.has_class("column-margin") || attrs.has_class("aside") {
            return div()
                .id(("margin", ix))
                .w_full()
                .when(!options.is_last, |d| d.pb(mb))
                .text_size(rems(0.85))
                .line_height(relative(1.5))
                .text_color(cx.theme().muted_foreground)
                .children(render_children(children, window, cx))
                .into_any_element();
        }
        let plain = crate::text::format::quarto::is_column(attrs)
            || attrs.id.as_ref().is_some_and(|id| crate::text::format::quarto::THEOREM_KINDS.iter().any(|k| id.starts_with(&format!("{k}-"))))
            || ["proof", "remark", "abstract", "theorem", "lemma", "corollary", "proposition", "conjecture", "definition", "example", "exercise", "solution"].iter().any(|c| attrs.has_class(c));
        if plain {
            return div().id(("plain", ix)).w_full().pb(mb).children(render_children(children, window, cx)).into_any_element();
        }
        if attrs.has_class("panel-tabset") {
            if let Some(tabs) = Self::render_tabset(children, options, mb, node_cx, window, cx) {
                return tabs;
            }
        }
        if attrs.has_class("columns") {
            let columns = children.iter().enumerate().map(|(i, c)| {
                let (inner, width) = match c {
                    BlockNode::Div { attrs, children, .. } if attrs.has_class("column") => (children.as_slice(), attrs.value("width").and_then(|w| w.trim_end_matches('%').parse::<f32>().ok()).map(|p| p / 100.)),
                    _ => (std::slice::from_ref(c), None),
                };
                div()
                    .id(("column", i))
                    .min_w_0()
                    .map(|d| match width {
                        Some(w) => d.w(relative(w)),
                        None => d.flex_1(),
                    })
                    .children(render_children(inner, window, cx))
            });
            return div().w_full().pb(mb).child(div().id(("columns", ix)).w_full().flex().flex_row().gap_4().items_start().children(columns)).into_any_element();
        }
        if let Some(ncol) = attrs.value("layout-ncol").and_then(|v| v.parse::<usize>().ok()).filter(|n| *n > 1) {
            let share = 1. / ncol as f32;
            let cells = children.iter().enumerate().map(|(i, c)| div().id(("cell", i)).w(relative(share)).min_w_0().px_1().child(c.render_block(NodeRenderOptions { ix: i, is_last: true, ..options }, node_cx, window, cx)));
            return div().w_full().pb(mb).child(div().id(("layout", ix)).w_full().flex().flex_row().flex_wrap().items_start().children(cells)).into_any_element();
        }
        // Centered: the words by their alignment, a figure standing alone
        // in a paragraph by its own (`ImageAlign`), the paragraph still as
        // wide as the div. Shrunk to its content in a centering column, a
        // picture's "no wider than the paragraph" had no width to be
        // measured against and ran past the pane.
        let centered = attrs.value("style").is_some_and(|s| s.replace(' ', "").contains("text-align:center")) || attrs.has_class("text-center");
        let centered_children: Vec<BlockNode>;
        let children = if centered {
            centered_children = children
                .iter()
                .cloned()
                .map(|c| match c {
                    BlockNode::Paragraph(mut p) if p.children.len() == 1 && p.children[0].image.is_some() => {
                        let image = p.children[0].image.as_mut().unwrap();
                        if image.align.is_none() {
                            image.align = Some(ImageAlign::Center);
                        }
                        BlockNode::Paragraph(p)
                    }
                    other => other,
                })
                .collect();
            centered_children.as_slice()
        } else {
            children
        };
        let body = div()
            .id(("div", ix))
            .w_full()
            .when(centered, |d| d.text_center())
            .children(render_children(children, window, cx));
        div()
            .w_full()
            .pb(mb)
            .child(
                div()
                    .w_full()
                    .border_l_2()
                    .border_color(theme.border)
                    .pl_3()
                    .child(div().flex().flex_row().pb(rems(0.4)).child(chip(chip_words, window, cx)))
                    .child(body),
            )
            .into_any_element()
    }

    /// A Quarto callout (`::: {.callout-note}`): a card with its kind's
    /// color at the left. Its options are Quarto's: `appearance`
    /// (`default` a head on a tint with the kind's icon, `simple` the head
    /// on the card's ground, `minimal` no icon and a head only for a title
    /// given), `icon=false`, and `collapse` (`true` folded at first,
    /// `false` open), which makes the head a fold that closes the body up
    /// to it as a chunk's does. The title is `title=`, else the first
    /// heading inside, else the kind's name, Quarto's own word.
    /// (Emaki addition.)
    #[allow(clippy::too_many_arguments)]
    fn render_callout(
        kind: &str,
        attrs: &DivAttrs,
        children: &[BlockNode],
        options: NodeRenderOptions,
        mb: gpui::Rems,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let ix = options.ix;
        let theme = cx.theme().clone();
        let (color, name) = match kind {
            "tip" => (theme.success, "Tip"),
            "warning" => (theme.warning, "Warning"),
            // Quarto's caution is an orange, between its warning and its
            // important.
            "caution" => (Hsla { h: theme.warning.h * 0.6, ..theme.warning }, "Caution"),
            "important" => (theme.danger, "Important"),
            _ => (theme.info, "Note"),
        };
        let icon_name = match kind {
            "tip" | "warning" | "caution" | "important" => kind,
            _ => "note",
        };
        let appearance = attrs.value("appearance").unwrap_or("default");
        let minimal = appearance == "minimal";
        let tinted = appearance != "simple" && !minimal;
        let style = &node_cx.style;
        let icon = (!minimal && attrs.value("icon") != Some("false") && !style.callout_icons.is_empty()).then(|| SharedString::from(format!("{}/{icon_name}.svg", style.callout_icons)));
        let mut body: Vec<&BlockNode> = children.iter().collect();
        let title: Option<AnyElement> = match attrs.value("title") {
            Some(t) => Some(div().child(t.to_string()).into_any_element()),
            None => match body.first() {
                Some(BlockNode::Heading { children, .. }) => {
                    let heading = children.clone();
                    body.remove(0);
                    Some(heading.render(node_cx, window, cx))
                }
                _ => (!minimal).then(|| div().child(name).into_any_element()),
            },
        };
        let body: Vec<BlockNode> = body.into_iter().cloned().collect();
        let collapse = attrs.value("collapse").filter(|v| *v == "true" || *v == "false");
        let fold = collapse.map(|v| {
            use std::hash::{Hash as _, Hasher as _};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            attrs.text.hash(&mut h);
            Self::children_text(children, BlockTextKind::All).hash(&mut h);
            let key = SharedString::from(format!("callout-fold-{:x}", h.finish()));
            let open = v == "false";
            window.use_keyed_state(key, cx, move |_, _| ChunkFold { open, ..ChunkFold::default() })
        });
        let (open, shown, body_h) = match &fold {
            Some(f) => {
                let f = f.read(cx);
                if f.at.is_some_and(|at| at.elapsed() < CHUNK_FOLD) {
                    window.request_animation_frame();
                }
                (f.open, f.shown(), f.height.clone())
            }
            None => (true, 1., std::rc::Rc::new(std::cell::Cell::new(0.))),
        };
        let head = (title.is_some() || fold.is_some()).then(|| {
            let [fold_tip, unfold_tip] = style.chunk_words.clone();
            let tip = if open { fold_tip } else { unfold_tip };
            div()
                .id(("callout-head", ix))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .px_4()
                .py_2()
                .font_weight(FontWeight::SEMIBOLD)
                .when(tinted, |d| d.bg(color.opacity(0.08)))
                .children(icon.map(|path| gpui::svg().path(path).size(px(16.)).flex_shrink_0().text_color(color)))
                .child(div().flex_1().min_w_0().children(title))
                .when_some(fold.clone(), |d, fold| {
                    d.cursor_pointer()
                        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, window, cx| {
                            gpui_base::TextSelection::end(window, cx);
                            cx.stop_propagation();
                            fold.update(cx, |f, cx| {
                                f.toggle();
                                cx.notify();
                            });
                            window.refresh();
                        })
                        .child(
                            crate::Sizable::with_size(crate::Icon::new(crate::IconName::ChevronDown), px(14.))
                                .text_color(theme.muted_foreground)
                                .rotate(gpui::Radians(-std::f32::consts::FRAC_PI_2 * (1. - shown))),
                        )
                })
        });
        let n = body.len();
        let content = div()
            .w_full()
            .px_4()
            .when(head.is_some(), |d| d.pt_2())
            .when(head.is_none(), |d| d.pt_3())
            .children(body.iter().enumerate().map(|(i, c)| c.render_block(NodeRenderOptions { ix: i, is_last: i + 1 == n, ..options }, node_cx, window, cx)))
            .child(div().h(px(12.)));
        // What folds is measured as it is drawn, so a fold closes from the
        // height it had.
        let content = {
            let height = body_h.clone();
            div().w_full().relative().child(content).child(gpui::canvas(move |bounds, _, _| height.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().inset_0())
        };
        let content = if shown >= 1. {
            Some(content.into_any_element())
        } else if shown > 0. {
            Some(div().w_full().h(px(body_h.get() * shown)).overflow_hidden().opacity(shown).child(content).into_any_element())
        } else {
            None
        };
        div()
            .w_full()
            .pb(mb)
            .child(
                div()
                    .id(("callout", ix))
                    .w_full()
                    .overflow_hidden()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .border_l_3()
                    .border_color(color)
                    .when(tinted, |d| d.bg(color.opacity(0.04)))
                    .children(head)
                    .children(content),
            )
            .into_any_element()
    }

    /// A Quarto tabset (`::: {.panel-tabset}`): a tab for each heading at
    /// the highest level inside, its blocks under it the tab's content.
    /// The tabs are a segmented control whose plate slides to the tab
    /// chosen, and the content fades in while its box's height runs from
    /// the last tab's to this one's. None when it holds no heading, and
    /// it is drawn as a plain div. (Emaki addition.)
    fn render_tabset(
        children: &[BlockNode],
        options: NodeRenderOptions,
        mb: gpui::Rems,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let level = children.iter().filter_map(|c| if let BlockNode::Heading { level, .. } = c { Some(*level) } else { None }).min()?;
        let mut before: Vec<BlockNode> = Vec::new();
        let mut tabs: Vec<(String, Vec<BlockNode>)> = Vec::new();
        for c in children {
            match c {
                BlockNode::Heading { level: l, children: words, .. } if *l == level => tabs.push((words.text().trim().to_string(), Vec::new())),
                c => match tabs.last_mut() {
                    Some((_, blocks)) => blocks.push(c.clone()),
                    None => before.push(c.clone()),
                },
            }
        }
        let key = children.iter().find_map(|c| c.span()).map(|s| s.start).unwrap_or(options.ix);
        let state = window.use_keyed_state(SharedString::from(format!("tabset-{key}")), cx, |_, _| TabSet::default());
        let (at, from, t, from_h, height, spots, track_x) = {
            let st = state.read(cx);
            let t = st.when.map(|w| gpui::ease_out_quint()((w.elapsed().as_secs_f32() / TABSET_ANIM.as_secs_f32()).min(1.))).unwrap_or(1.);
            if t < 1. {
                window.request_animation_frame();
            }
            st.spots.borrow_mut().truncate(tabs.len());
            (st.at.min(tabs.len() - 1), st.from.min(tabs.len() - 1), t, st.from_h, st.height.clone(), st.spots.clone(), st.track_x.clone())
        };
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        // The plate, from where the last tab is to where this one is.
        let plate = {
            let spots = spots.borrow();
            let origin = track_x.get();
            match (spots.get(from), spots.get(at)) {
                (Some(a), Some(b)) if b.1 > 0. => {
                    let (x, w) = (a.0 + (b.0 - a.0) * t - origin, a.1 + (b.1 - a.1) * t);
                    Some(div().absolute().top(px(2.)).bottom(px(2.)).left(px(x)).w(px(w)).rounded(px(6.)).bg(if dark { theme.secondary_active } else { theme.popover }).shadow_sm())
                }
                _ => None,
            }
        };
        let track = {
            let spots = spots.clone();
            div()
                .id(("tabset-track", key))
                .relative()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(2.))
                .p(px(2.))
                .rounded(px(8.))
                .bg(if dark { theme.tab_bar_segmented } else { theme.muted })
                .children(plate)
                .children(tabs.iter().enumerate().map(|(i, (title, _))| {
                    let state = state.clone();
                    let spots = spots.clone();
                    div()
                        .id(("tab", i))
                        .relative()
                        .px(px(12.))
                        .py(px(3.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_size(rems(0.875))
                        .text_color(if i == at { theme.foreground } else { theme.muted_foreground })
                        .hover(|s| s.text_color(theme.foreground))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, window, cx| {
                            gpui_base::TextSelection::end(window, cx);
                            state.update(cx, |st, cx| {
                                if st.at != i {
                                    st.from = st.at;
                                    st.at = i;
                                    st.from_h = st.height.get();
                                    st.when = Some(std::time::Instant::now());
                                    cx.notify();
                                }
                            });
                            window.refresh();
                        })
                        .child(title.clone())
                        // Where each tab is, for the plate.
                        .child(
                            gpui::canvas(
                                move |bounds, _, _| {
                                    let mut spots = spots.borrow_mut();
                                    if spots.len() <= i {
                                        spots.resize(i + 1, (0., 0.));
                                    }
                                    spots[i] = (f32::from(bounds.origin.x), f32::from(bounds.size.width));
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .inset_0(),
                        )
                }))
                .child({
                    // The track's own place: the tabs' are the window's.
                    let track_x = track_x.clone();
                    gpui::canvas(move |bounds, _, _| track_x.set(f32::from(bounds.origin.x)), |_, _, _, _| {}).absolute().inset_0()
                })
        };
        let content = tabs.get(at).map(|(_, blocks)| blocks.clone()).unwrap_or_default();
        let n = content.len();
        let body = {
            let height = height.clone();
            div()
                .w_full()
                .flex_shrink_0()
                .relative()
                .pt_3()
                .opacity(t)
                .children(content.iter().enumerate().map(|(i, c)| c.render_block(NodeRenderOptions { ix: i, is_last: i + 1 == n, ..options }, node_cx, window, cx)))
                .child(gpui::canvas(move |bounds, _, _| height.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().inset_0())
        };
        let new_h = height.get();
        let body = if t < 1. && from_h > 0. && new_h > 0. {
            div().w_full().h(px(from_h + (new_h - from_h) * t)).overflow_hidden().child(body).into_any_element()
        } else {
            body.into_any_element()
        };
        let nb = before.len();
        Some(
            div()
                .id(("tabset", key))
                .w_full()
                .pb(mb)
                .children(before.iter().enumerate().map(|(i, c)| c.render_block(NodeRenderOptions { ix: i, is_last: i + 1 == nb, ..options }, node_cx, window, cx)))
                .child(div().flex().flex_row().child(track))
                .child(body)
                .into_any_element(),
        )
    }

    pub(crate) fn render_block(
        &self,
        options: NodeRenderOptions,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let ix = options.ix;
        let mb = if options.in_list || options.is_last {
            rems(0.)
        } else {
            node_cx.style.paragraph_gap
        };

        match self {
            BlockNode::Root { children, .. } => div()
                .id(("div", ix))
                .children(children.into_iter().enumerate().map(move |(ix, node)| {
                    node.render_block(NodeRenderOptions { ix, ..options }, node_cx, window, cx)
                }))
                .into_any_element(),
            BlockNode::Paragraph(paragraph) => div()
                .id(("p", ix))
                .pb(mb)
                .child(paragraph.render(node_cx, window, cx))
                .into_any_element(),
            BlockNode::Heading {
                level, children, attrs, ..
            } => {
                let (text_size, font_weight) = match level {
                    1 => (rems(2.), FontWeight::BOLD),
                    2 => (rems(1.5), FontWeight::SEMIBOLD),
                    3 => (rems(1.25), FontWeight::SEMIBOLD),
                    4 => (rems(1.125), FontWeight::SEMIBOLD),
                    5 => (rems(1.), FontWeight::SEMIBOLD),
                    6 => (rems(1.), FontWeight::MEDIUM),
                    _ => (rems(1.), FontWeight::NORMAL),
                };

                // Emaki: a base of none is the text's own size where the
                // view stands, so a heading grows with the words under it.
                let base = if node_cx.style.heading_base_font_size == gpui::Pixels::ZERO {
                    window.text_style().font_size.to_pixels(window.rem_size())
                } else {
                    node_cx.style.heading_base_font_size
                };
                let mut text_size = text_size.to_pixels(base);
                if let Some(f) = node_cx.style.heading_font_size.as_ref() {
                    text_size = (f)(*level, base);
                }

                // Emaki: where it is painted, for an outline of the
                // document (`HeadingSpots`).
                let level = *level;
                // Quarto's attributes, in a quiet chip after the words,
                // small whatever the heading's size. (Emaki addition.)
                let chip = attrs.as_ref().map(|attrs| {
                    div()
                        .flex_shrink_0()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(5.))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().muted)
                        .font_family(cx.theme().mono_font_family.clone())
                        .font_weight(FontWeight::NORMAL)
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(cx.theme().muted_foreground)
                        .child(attrs.render(node_cx, window, cx))
                });
                div()
                    .id(SharedString::from(format!("h{}-{}", level, ix)))
                    .relative()
                    .pb(rems(0.3))
                    .whitespace_normal()
                    .text_size(text_size)
                    .font_weight(font_weight)
                    .map(|d| match chip {
                        // Centered on the words' first line: a box of that
                        // line's height. By baseline, gpui set the chip's
                        // box on the line's foot, and it stood low.
                        Some(chip) => {
                            let line = window.text_style().line_height.to_pixels(gpui::AbsoluteLength::Pixels(text_size), window.rem_size());
                            d.flex().flex_row().flex_wrap().items_start().gap_x(px(8.)).child(children.render(node_cx, window, cx)).child(div().h(line).flex().items_center().child(chip))
                        }
                        None => d.child(children.render(node_cx, window, cx)),
                    })
                    .child(
                        gpui::canvas(
                            |_, _, _| {},
                            move |bounds, _, _, cx| {
                                let spots = crate::global_state::UiGlobalState::global(cx)
                                    .text_view_state()
                                    .and_then(|state| state.read(cx).headings.clone());
                                if let Some(spots) = spots {
                                    spots.painted(level, bounds);
                                }
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .into_any_element()
            }
            BlockNode::Div { attrs, chip, children, .. } => {
                Self::render_div(attrs, chip, children, options, mb, node_cx, window, cx)
            }
            BlockNode::Blockquote { children, .. } => div()
                .w_full()
                .pb(mb)
                .child(
                    div()
                        .id(("blockquote", ix))
                        .w_full()
                        .text_color(cx.theme().muted_foreground)
                        .border_l_3()
                        .border_color(cx.theme().secondary_active)
                        .px_4()
                        .children({
                            let children_len = children.len();
                            children.into_iter().enumerate().map(move |(index, c)| {
                                let is_last = index == children_len - 1;
                                c.render_block(options.is_last(is_last), node_cx, window, cx)
                            })
                        }),
                )
                .into_any_element(),
            BlockNode::List {
                children, ordered, ..
            } => v_flex()
                .id((if *ordered { "ol" } else { "ul" }, ix))
                .w_full()
                .min_w_0()
                .pb(mb)
                .children({
                    let mut items = Vec::with_capacity(children.len());
                    let mut item_index = 0;
                    for (ix, item) in children.into_iter().enumerate() {
                        let is_item = item.is_list_item();

                        items.push(Self::render_list_item(
                            item,
                            item_index,
                            NodeRenderOptions {
                                ix,
                                ordered: *ordered,
                                ..options
                            },
                            node_cx,
                            window,
                            cx,
                        ));

                        if is_item {
                            item_index += 1;
                        }
                    }
                    items
                })
                .into_any_element(),
            BlockNode::CodeBlock(code_block) => code_block.render(&options, node_cx, window, cx),
            BlockNode::Custom(node) => {
                let inner = match node_cx.markdown_extensions.render_block(node, window, cx) {
                    Some(rendered) => rendered,
                    None => div().child(node.as_text().to_string()).into_any_element(),
                };

                div().pb(mb).child(inner).into_any_element()
            }
            BlockNode::Table(table)
                if node_cx.style.front_matter && table.span.is_some_and(|s| s.start == 0) =>
            {
                Self::render_fields(table, &options, node_cx, window, cx)
            }
            BlockNode::Table { .. } => {
                Self::render_table(self, &options, node_cx, window, cx).into_any_element()
            }
            BlockNode::HorizontalRule { .. } => div()
                .pb(mb)
                .child(div().id("horizontal-rule").bg(cx.theme().border).h(px(2.)))
                .into_any_element(),
            BlockNode::Break { .. } => div().id("break").into_any_element(),
            BlockNode::Unknown { .. } | BlockNode::Definition { .. } => div().into_any_element(),
            _ => {
                if cfg!(debug_assertions) {
                    tracing::warn!("unknown implementation: {:?}", self);
                }

                div().into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstruct_markdown_wraps_marked_runs() {
        // "bold" fully covered by a bold mark.
        let marks = vec![(0..4, TextMark::default().bold())];
        assert_eq!(reconstruct_markdown("bold", &marks, 0..4), "**bold**");
        // Partial selection inside the bold run still wraps the slice.
        assert_eq!(reconstruct_markdown("bold", &marks, 1..3), "**ol**");
    }

    #[test]
    fn reconstruct_markdown_emits_unmarked_text_verbatim() {
        // "a b c": plain, code, plain across three runs concatenated.
        let text = "a b c";
        let marks = vec![(2..3, TextMark::default().code())];
        assert_eq!(reconstruct_markdown(text, &marks, 0..5), "a `b` c");
        // Selecting only the plain tail.
        assert_eq!(reconstruct_markdown(text, &marks, 3..5), " c");
    }

    #[test]
    fn reconstruct_markdown_handles_code_italic_strike_link() {
        assert_eq!(
            reconstruct_markdown("x", &[(0..1, TextMark::default().code())], 0..1),
            "`x`"
        );
        assert_eq!(
            reconstruct_markdown("x", &[(0..1, TextMark::default().italic())], 0..1),
            "*x*"
        );
        assert_eq!(
            reconstruct_markdown("x", &[(0..1, TextMark::default().strikethrough())], 0..1),
            "~~x~~"
        );
        let link = TextMark::default().link(LinkMark {
            url: "https://example.com".into(),
            ..Default::default()
        });
        assert_eq!(
            reconstruct_markdown("x", &[(0..1, link)], 0..1),
            "[x](https://example.com)"
        );
    }

    #[test]
    fn reconstruct_markdown_nested_bold_italic() {
        // A single run marked both bold and italic (as produced by `**_x_**`).
        let mark = TextMark::default().bold().italic();
        // Inner (italic) is applied first, then bold: `***x***`.
        assert_eq!(reconstruct_markdown("x", &[(0..1, mark)], 0..1), "***x***");
    }

    /// Build a paragraph whose combined `state.text` is the concatenation of
    /// its children (mirroring `Paragraph::render`), then set the paragraph
    /// selection so `selected_source` can be exercised without a real paint.
    fn paragraph_with_children(children: Vec<InlineNode>) -> Paragraph {
        let combined: String = children.iter().map(|c| c.text.to_string()).collect();
        let paragraph = Paragraph {
            span: None,
            children,
            link_refs: HashMap::new(),
            state: Arc::new(Mutex::new(InlineState::default())),
        };
        if let Ok(mut state) = paragraph.state.lock() {
            state.set_text(combined.into());
        }
        paragraph
    }

    fn set_paragraph_selection(paragraph: &Paragraph, range: Range<usize>) {
        if let Ok(mut state) = paragraph.state.lock() {
            state.selection = Some(range.into());
        }
    }

    #[test]
    fn paragraph_selected_source_maps_partial_selection_across_runs() {
        // "This has **bold** text." rendered as ["This has ", "bold", " text."].
        let children = vec![
            InlineNode::new("This has ").marks(vec![(0..9, TextMark::default())]),
            InlineNode::new("bold").marks(vec![(0..4, TextMark::default().bold())]),
            InlineNode::new(" text.").marks(vec![(0..6, TextMark::default())]),
        ];
        let paragraph = paragraph_with_children(children);

        // Select the whole paragraph: "This has bold text." -> source with **.
        set_paragraph_selection(&paragraph, 0..(9 + 4 + 6));
        assert_eq!(paragraph.selected_source(), "This has **bold** text.");

        // Select only across the boundary "has **bold** te".
        // Rendered offsets: "has " starts at 5, "bold" at 9..13, " te" 13..16.
        set_paragraph_selection(&paragraph, 5..16);
        assert_eq!(paragraph.selected_source(), "has **bold** te");

        // Select entirely inside the bold run -> still wrapped.
        set_paragraph_selection(&paragraph, 10..12);
        assert_eq!(paragraph.selected_source(), "**ol**");
    }

    #[test]
    fn paragraph_selected_source_matches_text_when_no_marks() {
        let children =
            vec![InlineNode::new("plain words").marks(vec![(0..11, TextMark::default())])];
        let paragraph = paragraph_with_children(children);
        set_paragraph_selection(&paragraph, 0..11);
        assert_eq!(paragraph.selected_source(), "plain words");
        assert_eq!(paragraph.selected_text(), "plain words");
    }

    #[test]
    fn a_formula_selected_copies_as_its_source() {
        use crate::text::math::{MathPicture, MathSource};
        let picture = MathPicture { svg: Arc::from(&b"<svg/>"[..]), width: 1., height: 1., baseline: 0.8 };
        let math = MathSource { src: "x^2".into(), display: false, tag: None, picture: Some(picture) };
        let children = vec![InlineNode::new("Let "), InlineNode::new("\u{A0}\u{A0}\u{202F}").with_math(math), InlineNode::new(" be.")];
        let paragraph = paragraph_with_children(children);
        let len = paragraph.state.lock().unwrap().text.len();
        set_paragraph_selection(&paragraph, 0..len);
        assert_eq!(paragraph.selected_text(), "Let $x^2$ be.");
        assert_eq!(paragraph.selected_source(), "Let $x^2$ be.");
        // A selection that only runs into the formula's room takes it whole.
        set_paragraph_selection(&paragraph, 2..6);
        assert_eq!(paragraph.selected_text(), "t $x^2$");
    }

    fn selected_paragraph(text: &str) -> Paragraph {
        let len = text.len();
        let paragraph = paragraph_with_children(vec![
            InlineNode::new(text).marks(vec![(0..len, TextMark::default())]),
        ]);
        set_paragraph_selection(&paragraph, 0..len);
        paragraph
    }

    #[test]
    fn heading_selected_source_prefixes_hashes() {
        let heading = BlockNode::Heading {
            attrs: None,
            level: 2,
            children: selected_paragraph("Title"),
            span: None,
        };
        assert_eq!(heading.selected_text(SelectionFormat::Source), "## Title\n");
        // Rendered text keeps no marker.
        assert_eq!(heading.selected_text(SelectionFormat::Plain), "Title\n");
    }

    #[test]
    fn unordered_list_selected_source_prefixes_dash() {
        let list = BlockNode::List {
            ordered: false,
            span: None,
            children: vec![
                BlockNode::ListItem {
                    children: vec![BlockNode::Paragraph(selected_paragraph("one"))],
                    spread: false,
                    checked: None,
                    span: None,
                },
                BlockNode::ListItem {
                    children: vec![BlockNode::Paragraph(selected_paragraph("two"))],
                    spread: false,
                    checked: None,
                    span: None,
                },
            ],
        };
        assert_eq!(
            list.selected_text(SelectionFormat::Source),
            "- one\n- two\n"
        );
    }

    #[test]
    fn ordered_list_selected_source_prefixes_numbers() {
        let list = BlockNode::List {
            ordered: true,
            span: None,
            children: vec![
                BlockNode::ListItem {
                    children: vec![BlockNode::Paragraph(selected_paragraph("first"))],
                    spread: false,
                    checked: None,
                    span: None,
                },
                BlockNode::ListItem {
                    children: vec![BlockNode::Paragraph(selected_paragraph("second"))],
                    spread: false,
                    checked: None,
                    span: None,
                },
            ],
        };
        assert_eq!(
            list.selected_text(SelectionFormat::Source),
            "1. first\n2. second\n"
        );
    }

    #[test]
    fn nested_list_selected_source_indents_sublists() {
        // - one
        //   - nested
        // - two
        let nested = BlockNode::List {
            ordered: false,
            span: None,
            children: vec![BlockNode::ListItem {
                children: vec![BlockNode::Paragraph(selected_paragraph("nested"))],
                spread: false,
                checked: None,
                span: None,
            }],
        };
        let list = BlockNode::List {
            ordered: false,
            span: None,
            children: vec![
                BlockNode::ListItem {
                    children: vec![BlockNode::Paragraph(selected_paragraph("one")), nested],
                    spread: false,
                    checked: None,
                    span: None,
                },
                BlockNode::ListItem {
                    children: vec![BlockNode::Paragraph(selected_paragraph("two"))],
                    spread: false,
                    checked: None,
                    span: None,
                },
            ],
        };
        assert_eq!(
            list.selected_text(SelectionFormat::Source),
            "- one\n  - nested\n- two\n"
        );
    }

    #[test]
    fn task_list_selected_source_restores_checkboxes() {
        let list = BlockNode::List {
            ordered: false,
            span: None,
            children: vec![
                BlockNode::ListItem {
                    children: vec![BlockNode::Paragraph(selected_paragraph("done"))],
                    spread: false,
                    checked: Some(true),
                    span: None,
                },
                BlockNode::ListItem {
                    children: vec![BlockNode::Paragraph(selected_paragraph("todo"))],
                    spread: false,
                    checked: Some(false),
                    span: None,
                },
            ],
        };
        assert_eq!(
            list.selected_text(SelectionFormat::Source),
            "- [x] done\n- [ ] todo\n"
        );
    }

    #[test]
    fn blockquote_selected_source_prefixes_gt() {
        let quote = BlockNode::Blockquote {
            span: None,
            children: vec![BlockNode::Paragraph(selected_paragraph("quoted text"))],
        };
        assert_eq!(
            quote.selected_text(SelectionFormat::Source),
            "> quoted text\n"
        );
    }

    #[test]
    fn table_selected_source_pipes_cells_with_alignment_row() {
        let cell = |text: &str| TableCell {
            children: selected_paragraph(text),
            width: None,
        };
        let table = Table {
            children: vec![
                TableRow {
                    children: vec![cell("Name"), cell("Age")],
                },
                TableRow {
                    children: vec![cell("Alice"), cell("30")],
                },
            ],
            column_aligns: vec![ColumnumnAlign::Left, ColumnumnAlign::Right],
            span: None,
        };
        let block = BlockNode::Table(table);
        assert_eq!(
            block.selected_text(SelectionFormat::Source),
            "| Name | Age |\n| :-- | --: |\n| Alice | 30 |\n"
        );
    }

    fn image_paragraph(alt: &str, url: &str) -> Paragraph {
        let image = ImageNode {
            url: url.into(),
            alt: Some(alt.into()),
            ..Default::default()
        };
        Paragraph {
            span: None,
            children: vec![InlineNode::image(image)],
            link_refs: HashMap::new(),
            state: Arc::new(Mutex::new(InlineState::default())),
        }
    }

    /// Every mark round-trips, including the two Markdown has no plain syntax
    /// for.
    #[test]
    fn marks_round_trip_through_reconstruction() {
        let wrap = |mark: TextMark| reconstruct_markdown("x", &[(0..1, mark)], 0..1);

        assert_eq!(wrap(TextMark::default().bold()), "**x**");
        assert_eq!(wrap(TextMark::default().italic()), "*x*");
        assert_eq!(wrap(TextMark::default().code()), "`x`");
        assert_eq!(wrap(TextMark::default().strikethrough()), "~~x~~");
        assert_eq!(
            wrap(TextMark::default().highlight(crate::yellow(200))),
            "==x=="
        );
        // No Markdown syntax for underline, so it keeps the tag it came from.
        assert_eq!(wrap(TextMark::default().underline()), "<u>x</u>");

        // A link keeps its title, which Markdown carries after the URL.
        assert_eq!(
            wrap(TextMark::default().link(LinkMark {
                url: "https://example.com".into(),
                title: Some("Tip".into()),
                ..Default::default()
            })),
            "[x](https://example.com \"Tip\")"
        );
    }

    /// A block the selection covers whole comes straight from the source, so it
    /// keeps what the author wrote instead of a normalized reconstruction.
    #[test]
    fn document_selected_source_slices_covered_blocks_from_the_source() {
        use crate::text::document::ParsedDocument;

        // `_italic_`, the `3.` start and the column padding all survive only
        // because the block is copied, not rebuilt.
        let source = "start\n\n3. _one_\n4. two\n\n---\n\nend";
        let list = "3. _one_\n4. two";
        let list_start = source.find(list).unwrap();
        let rule_start = source.find("---").unwrap();

        let document = ParsedDocument {
            source: source.into(),
            blocks: vec![
                BlockNode::Paragraph(selected_paragraph("start")),
                BlockNode::List {
                    ordered: true,
                    children: vec![],
                    span: Some(Span {
                        start: list_start,
                        end: list_start + list.len(),
                    }),
                },
                BlockNode::HorizontalRule {
                    span: Some(Span {
                        start: rule_start,
                        end: rule_start + 3,
                    }),
                },
                BlockNode::Paragraph(selected_paragraph("end")),
            ],
        };

        assert_eq!(
            document.selected_text(SelectionFormat::Source, None),
            "start\n\n3. _one_\n4. two\n\n---\n\nend"
        );
    }

    #[test]
    fn document_selected_source_includes_enclosed_image() {
        use crate::text::document::ParsedDocument;

        // A standalone image between two selected paragraphs is covered by the
        // selection, so it is copied whole even though it holds no selection of
        // its own — straight out of the source the parser located it in.
        let source = "before\n\n![alt](https://example.com/i.png)\n\nafter";
        let image_markdown = "![alt](https://example.com/i.png)";
        let start = source.find(image_markdown).unwrap();
        let mut image = image_paragraph("alt", "https://example.com/i.png");
        image.span = Some(Span {
            start,
            end: start + image_markdown.len(),
        });

        let document = ParsedDocument {
            source: source.into(),
            blocks: vec![
                BlockNode::Paragraph(selected_paragraph("before")),
                BlockNode::Paragraph(image),
                BlockNode::Paragraph(selected_paragraph("after")),
            ],
        };
        assert_eq!(
            document.selected_text(SelectionFormat::Source, None),
            "before\n\n![alt](https://example.com/i.png)\n\nafter"
        );
    }

    #[test]
    fn document_selected_source_drops_unenclosed_image() {
        use crate::text::document::ParsedDocument;

        // An image after the only selected block, with nothing selected after
        // it, is not enclosed and is dropped.
        let document = ParsedDocument {
            source: String::new().into(),
            blocks: vec![
                BlockNode::Paragraph(selected_paragraph("before")),
                BlockNode::Paragraph(image_paragraph("alt", "u")),
            ],
        };
        assert_eq!(
            document.selected_text(SelectionFormat::Source, None),
            "before"
        );
    }

    fn selected_code_block(code: &str, lang: Option<&str>) -> BlockNode {
        let block = CodeBlock::new(
            code.to_string().into(),
            lang.map(|l| l.to_string().into()),
            None::<Span>,
        );
        if let Ok(mut state) = block.state.lock() {
            let len = state.text.len();
            state.selection = Some((0..len).into());
        }
        BlockNode::CodeBlock(block)
    }

    #[test]
    fn code_block_selected_source_wraps_in_fence_with_lang() {
        let block = selected_code_block("let x = 1;\n", Some("rust"));
        let code = block.selected_text(SelectionFormat::Plain);
        let code_trimmed = code.trim_end_matches('\n');
        // The source wraps the (trailing-newline-trimmed) selected code in a
        // fenced block carrying the language; the block arm adds one trailing
        // newline.
        assert_eq!(
            block.selected_text(SelectionFormat::Source),
            format!("```rust\n{}\n```\n", code_trimmed)
        );
        assert!(
            block
                .selected_text(SelectionFormat::Source)
                .starts_with("```rust\n")
        );
        assert!(
            block
                .selected_text(SelectionFormat::Source)
                .trim_end()
                .ends_with("\n```")
        );
    }

    #[test]
    fn code_block_selected_source_without_lang() {
        let block = selected_code_block("plain\n", None);
        let code_trimmed = block.selected_text(SelectionFormat::Plain);
        let code_trimmed = code_trimmed.trim_end_matches('\n');
        assert_eq!(
            block.selected_text(SelectionFormat::Source),
            format!("```\n{}\n```\n", code_trimmed)
        );
    }

    #[test]
    fn document_selected_source_joins_blocks_with_blank_line() {
        use crate::text::document::ParsedDocument;

        // A heading, a paragraph, and a two-item ordered list, each fully
        // selected. Top-level blocks must be separated by a blank line so the
        // copied Markdown re-renders with the same structure.
        let document = ParsedDocument {
            source: String::new().into(),
            blocks: vec![
                BlockNode::Heading {
                    attrs: None,
                    level: 1,
                    children: selected_paragraph("Title"),
                    span: None,
                },
                BlockNode::Paragraph(selected_paragraph("A paragraph.")),
                selected_code_block("let x = 1;\n", Some("rust")),
                BlockNode::List {
                    ordered: true,
                    span: None,
                    children: vec![
                        BlockNode::ListItem {
                            children: vec![BlockNode::Paragraph(selected_paragraph("one"))],
                            spread: false,
                            checked: None,
                            span: None,
                        },
                        BlockNode::ListItem {
                            children: vec![BlockNode::Paragraph(selected_paragraph("two"))],
                            spread: false,
                            checked: None,
                            span: None,
                        },
                    ],
                },
            ],
        };

        assert_eq!(
            document.selected_text(SelectionFormat::Source, None),
            "# Title\n\nA paragraph.\n\n```rust\nlet x = 1;\n```\n\n1. one\n2. two"
        );
    }

    #[cfg(feature = "tree-sitter")]
    use crate::{
        Theme, ThemeMode,
        text::{TextView, TextViewState},
    };
    #[cfg(feature = "tree-sitter")]
    use gpui::{AppContext as _, Context, Entity, Render, TestAppContext, VisualTestContext};

    #[cfg(feature = "tree-sitter")]
    fn cached_highlight_theme(block: &CodeBlock) -> Option<Arc<HighlightTheme>> {
        block
            .styles
            .lock()
            .ok()
            .and_then(|styles| styles.as_ref().map(|styles| styles.highlight_theme.clone()))
    }

    #[test]
    fn code_block_equality_includes_code_content() {
        let first = CodeBlock::new("let value = 1;".into(), Some("rust".into()), None::<Span>);
        let second = CodeBlock::new("let value = 2;".into(), Some("rust".into()), None::<Span>);

        assert_ne!(first, second);
    }

    #[cfg(feature = "tree-sitter")]
    #[test]
    fn code_block_highlighter_cache_refreshes_after_language_registration() {
        let lang = SharedString::from("json-cache-test");
        let theme = HighlightTheme::default_light();

        CODE_BLOCK_HIGHLIGHTERS.with(|cache| {
            cache.borrow_mut().remove(&lang);
        });

        let unknown_block =
            CodeBlock::new("{\"value\": 1}".into(), Some(lang.clone()), None::<Span>);
        _ = unknown_block.styles(&theme);

        let cached_language = CODE_BLOCK_HIGHLIGHTERS.with(|cache| {
            cache
                .borrow()
                .get(&lang)
                .map(|highlighter| highlighter.language().clone())
        });
        assert_eq!(cached_language.as_deref(), Some("text"));

        LanguageRegistry::singleton().register(
            lang.as_ref(),
            &crate::highlighter::LanguageConfig::new(
                lang.clone(),
                tree_sitter_json::LANGUAGE.into(),
                vec![],
                r#"
                    (string) @string
                    (number) @number
                    (pair key: (string) @property)
                "#,
                "",
                "",
            ),
        );

        let registered_block =
            CodeBlock::new("{\"value\": 2}".into(), Some(lang.clone()), None::<Span>);
        _ = registered_block.styles(&theme);

        let cached_language = CODE_BLOCK_HIGHLIGHTERS.with(|cache| {
            cache
                .borrow()
                .get(&lang)
                .map(|highlighter| highlighter.language().clone())
        });
        assert_eq!(cached_language.as_deref(), Some(lang.as_ref()));
    }

    #[cfg(feature = "tree-sitter")]
    #[test]
    fn code_block_styles_follow_the_current_highlight_theme() {
        let lang = SharedString::from("json-theme-cache-test");
        let light_theme = HighlightTheme::default_light();
        let dark_theme = HighlightTheme::default_dark();
        let code = SharedString::from(r#"{"value": 42}"#);
        let number_range = code.find("42").unwrap()..code.find("42").unwrap() + 2;

        let light_number = light_theme.style("number").and_then(|style| style.color);
        let dark_number = dark_theme.style("number").and_then(|style| style.color);
        assert_ne!(
            light_number, dark_number,
            "the test themes must use different number colors"
        );

        CODE_BLOCK_HIGHLIGHTERS.with(|cache| {
            cache.borrow_mut().remove(&lang);
        });
        LanguageRegistry::singleton().register(
            lang.as_ref(),
            &crate::highlighter::LanguageConfig::new(
                lang.clone(),
                tree_sitter_json::LANGUAGE.into(),
                vec![],
                "(number) @number",
                "",
                "",
            ),
        );

        let block = CodeBlock::new(code.clone(), Some(lang), None::<Span>);
        let light_styles = block.styles(&light_theme);
        let cached_light_theme = cached_highlight_theme(&block).unwrap();
        assert!(Arc::ptr_eq(&cached_light_theme, &light_theme));

        let equivalent_light_theme = Arc::new(light_theme.as_ref().clone());
        let repeated_light_styles = block.styles(&equivalent_light_theme);
        assert_eq!(repeated_light_styles, light_styles);
        assert!(
            Arc::ptr_eq(
                &cached_highlight_theme(&block).unwrap(),
                &equivalent_light_theme
            ),
            "an equivalent replacement should become the cache identity"
        );
        assert_eq!(block.styles(&equivalent_light_theme), light_styles);

        let dark_styles = block.styles(&dark_theme);
        assert_eq!(
            cached_highlight_theme(&block).as_deref(),
            Some(dark_theme.as_ref())
        );

        let color_for_number = |styles: &[(Range<usize>, HighlightStyle)]| -> Option<Hsla> {
            styles
                .iter()
                .find(|(range, _)| {
                    range.start <= number_range.start && range.end >= number_range.end
                })
                .and_then(|(_, style)| style.color)
        };

        assert_eq!(color_for_number(&light_styles), light_number);
        assert_eq!(
            color_for_number(&dark_styles),
            dark_number,
            "a theme change must not reuse syntax styles from the previous theme"
        );
    }

    #[cfg(feature = "tree-sitter")]
    #[gpui::test]
    fn rendered_markdown_code_block_follows_theme_without_reparsing(cx: &mut TestAppContext) {
        struct CodeBlockThemeRoot {
            text_view: Entity<TextViewState>,
        }

        impl Render for CodeBlockThemeRoot {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div().w(px(480.)).child(TextView::new(&self.text_view))
            }
        }

        let lang = SharedString::from("json-theme-render-test");
        LanguageRegistry::singleton().register(
            lang.as_ref(),
            &crate::highlighter::LanguageConfig::new(
                lang.clone(),
                tree_sitter_json::LANGUAGE.into(),
                vec![],
                "(number) @number",
                "",
                "",
            ),
        );

        cx.update(crate::init);
        let markdown = format!("```{lang}\n{{\"value\": 42}}\n```");
        let (view, cx) = cx.add_window_view(|_, cx| CodeBlockThemeRoot {
            text_view: cx.new(|cx| TextViewState::markdown(&markdown, cx)),
        });
        let cx: &mut VisualTestContext = cx;

        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let light_theme = cx.update(|_, cx| cx.theme().highlight_theme.clone());
        let light_block = view.read_with(cx, |root, cx| {
            let state = root.text_view.read(cx);
            let BlockNode::CodeBlock(block) = &state.parsed_content.document.blocks[0] else {
                panic!("expected a code block");
            };

            block.clone()
        });
        let cached_light_theme = cached_highlight_theme(&light_block)
            .expect("initial render should populate the highlight cache");
        assert_eq!(cached_light_theme.as_ref(), light_theme.as_ref());

        cx.update(|window, cx| {
            Theme::change(ThemeMode::Dark, Some(&mut *window), cx);
            let _ = window.draw(cx);
        });

        let dark_theme = cx.update(|_, cx| cx.theme().highlight_theme.clone());
        let dark_block = view.read_with(cx, |root, cx| {
            let state = root.text_view.read(cx);
            let BlockNode::CodeBlock(block) = &state.parsed_content.document.blocks[0] else {
                panic!("expected a code block");
            };

            block.clone()
        });
        let cached_dark_theme = cached_highlight_theme(&dark_block)
            .expect("theme-change render should refresh the highlight cache");

        assert_ne!(
            light_theme.as_ref(),
            dark_theme.as_ref(),
            "the test themes must have distinct highlight palettes"
        );
        assert!(
            Arc::ptr_eq(&dark_block.styles, &light_block.styles),
            "changing the theme must not require reparsing the Markdown document"
        );
        assert_eq!(cached_dark_theme.as_ref(), dark_theme.as_ref());
    }
}
