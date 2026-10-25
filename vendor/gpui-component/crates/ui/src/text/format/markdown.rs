use std::ops::Range;

use gpui::SharedString;
use markdown::mdast::{self, Node};

use crate::text::{
    document::ParsedDocument,
    markdown_ext::MarkdownParseContext,
    node::{
        self, BlockNode, CodeBlock, ImageNode, InlineNode, LinkMark, NodeContext, Paragraph, Span,
        Table, TableRow, TextMark,
    },
};

/// Parse Markdown into a tree of nodes.
pub(crate) fn parse(source: &str, cx: &mut NodeContext) -> Result<ParsedDocument, SharedString> {
    let options = cx.markdown_extensions.parse_options();
    cx.source = Some(source.into());
    markdown::to_mdast(&source, &options)
        .map(|n| ast_to_document(source, n, cx))
        .map_err(|e| e.to_string().into())
}

fn parse_table_row(table: &mut Table, node: &mdast::TableRow, cx: &mut NodeContext) {
    let mut row = TableRow::default();
    node.children.iter().for_each(|c| {
        match c {
            Node::TableCell(cell) => {
                parse_table_cell(&mut row, cell, cx);
            }
            _ => {}
        };
    });
    table.children.push(row);
}

fn parse_table_cell(row: &mut node::TableRow, node: &mdast::TableCell, cx: &mut NodeContext) {
    let mut paragraph = Paragraph::default();
    node.children.iter().for_each(|c| {
        parse_paragraph(&mut paragraph, c, cx);
    });
    let table_cell = node::TableCell {
        children: paragraph,
        ..Default::default()
    };
    row.children.push(table_cell);
}

/// Push a text run with its existing `marks` plus `new_mark` across the full
/// run.
///
/// If the last mark already covers the full run, merge into it. Otherwise add a
/// new full-run mark. Empty runs are skipped so callers can flush freely.
fn push_merged(
    paragraph: &mut Paragraph,
    text: String,
    marks: Vec<(Range<usize>, TextMark)>,
    new_mark: TextMark,
) {
    if text.is_empty() {
        return;
    }

    let mut node = InlineNode::new(text).marks(marks);
    let len = node.text.len();
    if let Some(last) = node.marks.last_mut()
        && last.0.start == 0
        && last.0.end == len
    {
        last.1.merge(new_mark);
    } else {
        node.marks.push((0..len, new_mark));
    }
    paragraph.push(node);
}

/// Parse `children` and apply `mark` across each emitted text run.
///
/// Nested child marks are kept and shifted to match the combined text for the
/// current run, which lets nested emphasis like `**_x_**` render as both bold
/// and italic. Inline images split the run and are emitted as sibling image
/// nodes. The return value is the plain text from all children, for callers that
/// need to pass text back to their parent node.
fn merge_children_with_mark(
    paragraph: &mut Paragraph,
    children: &[mdast::Node],
    mark: TextMark,
    cx: &mut NodeContext,
) -> String {
    let mut merged_text = String::new();
    let mut merged_marks = Vec::new();

    // One paragraph for all the children, so Quarto's attributes after
    // a picture or a link inside a mark are seen (`parse_inlines`).
    let mut child_paragraph = Paragraph::default();
    let text = parse_inlines(&mut child_paragraph, children, cx);
    {
        for node in child_paragraph.children {
            let merged_offset = merged_text.len();
            merged_text.push_str(&node.text);

            for (range, child_mark) in node.marks {
                merged_marks.push((
                    range.start + merged_offset..range.end + merged_offset,
                    child_mark,
                ));
            }

            if let Some(mut image) = node.image {
                if let Some(link_mark) = mark.link.clone() {
                    image.link = Some(link_mark);
                }

                // GPUI InteractiveText does not support inline images, so
                // flush the accumulated text run and emit the image as its
                // own sibling InlineNode.
                push_merged(
                    paragraph,
                    std::mem::take(&mut merged_text),
                    std::mem::take(&mut merged_marks),
                    mark.clone(),
                );
                paragraph.push(InlineNode::image(image));
            }
        }
    }

    push_merged(paragraph, merged_text, merged_marks, mark);
    text
}

pub(super) fn append_inline_html_blocks(paragraph: &mut Paragraph, blocks: Vec<BlockNode>) -> Option<String> {
    let mut text = String::new();

    for block in blocks {
        match block {
            BlockNode::Root { children, .. } => {
                text.push_str(&append_inline_html_blocks(paragraph, children)?);
            }
            BlockNode::Paragraph(html_paragraph) => {
                text.push_str(&html_paragraph.text());
                for child in html_paragraph.children {
                    paragraph.push(child);
                }
            }
            BlockNode::Break { .. } => {
                text.push('\n');
                paragraph.push(InlineNode::new("\n"));
            }
            _ => return None,
        }
    }

    Some(text)
}

/// The inline nodes of one run (a paragraph's, a heading's, a mark's)
/// into `paragraph`, with Quarto's syntax read when it is on: a text
/// run after a picture, a link or inline code that opens with `{` is an
/// attribute set, applied and taken out (`quarto::apply_inline_attrs`),
/// and the words of a run are read for spans and shortcodes
/// (`quarto::push_text`). (Emaki addition.)
fn parse_inlines(paragraph: &mut Paragraph, children: &[mdast::Node], cx: &mut NodeContext) -> String {
    if !cx.markdown_extensions.is_quarto() {
        let mut text = String::new();
        for child in children {
            text.push_str(&parse_paragraph(paragraph, child, cx));
        }
        return text;
    }
    let mut text = String::new();
    let mut before: Option<&mdast::Node> = None;
    let mut ix = 0;
    while ix < children.len() {
        let child = &children[ix];
        match child {
            // Math, as Unicode and in italics; what Pandoc would not
            // read as math is the words it was, dollars and all.
            Node::InlineMath(val) if super::quarto::is_math(&val.value) => {
                let node = super::quarto::inline_math(&val.value, cx);
                text.push_str(&node.text);
                paragraph.push(node);
            }
            Node::InlineMath(val) => {
                let words = format!("${}$", val.value);
                paragraph.push(InlineNode::new(&words).marks(vec![(0..words.len(), TextMark::default())]));
                text.push_str(&words);
            }
            Node::Text(val) => {
                let mut value = val.value.as_str();
                if let Some(prev) = before.filter(|p| matches!(p, Node::Image(_) | Node::Link(_) | Node::InlineCode(_))) {
                    if let Some(end) = super::quarto::attrs_end(value) {
                        super::quarto::apply_inline_attrs(paragraph, prev, &value[..end], cx);
                        value = &value[end..];
                    }
                }
                // A shortcode an autolink cut in pieces (`{{< video
                // https://x >}}`): the pieces up to its close, joined
                // back as the words they are.
                if let Some(open) = value.rfind("{{<").filter(|at| !value[*at..].contains(">}}")) {
                    let close = children[ix + 1..].iter().take(6).position(|c| matches!(c, Node::Text(t) if t.value.contains(">}}")));
                    if let Some(j) = close.filter(|j| children[ix + 1..ix + 1 + j].iter().all(|c| matches!(c, Node::Link(_) | Node::Text(_)))) {
                        let end = ix + 1 + j;
                        let mut joined = value[..open].to_string();
                        joined.push_str(&value[open..]);
                        for c in &children[ix + 1..=end] {
                            joined.push_str(&c.to_string());
                        }
                        text.push_str(&super::quarto::push_text(paragraph, &joined));
                        before = Some(&children[end]);
                        ix = end + 1;
                        continue;
                    }
                }
                text.push_str(&super::quarto::push_text(paragraph, value));
            }
            other => text.push_str(&parse_paragraph(paragraph, other, cx)),
        }
        before = Some(child);
        ix += 1;
    }
    text
}

fn parse_paragraph(paragraph: &mut Paragraph, node: &mdast::Node, cx: &mut NodeContext) -> String {
    let span = node.position().map(|pos| Span {
        start: cx.offset + pos.start.offset,
        end: cx.offset + pos.end.offset,
    });
    if let Some(span) = span {
        paragraph.set_span(span);
    }

    let mut text = String::new();

    match node {
        Node::Paragraph(val) => {
            text = parse_inlines(paragraph, &val.children, cx);
        }
        Node::Text(val) => {
            text = val.value.clone();
            paragraph.push_str(&val.value)
        }
        Node::Emphasis(val) => {
            text = merge_children_with_mark(
                paragraph,
                &val.children,
                TextMark::default().italic(),
                cx,
            );
        }
        Node::Strong(val) => {
            text =
                merge_children_with_mark(paragraph, &val.children, TextMark::default().bold(), cx);
        }
        Node::Delete(val) => {
            text = merge_children_with_mark(
                paragraph,
                &val.children,
                TextMark::default().strikethrough(),
                cx,
            );
        }
        Node::InlineCode(val) => {
            // The span is set between spaces of its own, two at each end:
            // padding and margin for the plate drawn behind it (see
            // `CODE_PAD`). They are taken out again when a selection is
            // copied. The parser runs before the view's style is known, so
            // this does not depend on the plate being asked for.
            // (Emaki addition.)
            use crate::text::style::CODE_PAD;
            let lead = CODE_PAD.len_utf8() * 2;
            text = format!("{CODE_PAD}{CODE_PAD}{}{CODE_PAD}{CODE_PAD}", val.value);
            paragraph.push(InlineNode::new(&text).marks(vec![(lead..text.len() - lead, TextMark::default().code())]));
        }
        Node::Link(val) => {
            // The link as written, for Copy Markdown. (Emaki addition.)
            let raw = val.position.as_ref().zip(cx.source.as_ref()).and_then(|(p, s)| s.get(p.start.offset..p.end.offset)).map(|r| SharedString::from(r.to_string()));
            let link_mark = Some(LinkMark {
                url: val.url.clone().into(),
                title: val.title.clone().map(|s| s.into()),
                raw,
                ..Default::default()
            });

            text = merge_children_with_mark(
                paragraph,
                &val.children,
                TextMark {
                    link: link_mark,
                    ..Default::default()
                },
                cx,
            );
        }
        Node::Image(raw) => {
            paragraph.push_image(ImageNode {
                url: raw.url.clone().into(),
                title: raw.title.clone().map(|t| t.into()),
                alt: Some(raw.alt.clone().into()),
                ..Default::default()
            });
        }
        Node::InlineMath(raw) => {
            text = raw.value.clone();
            paragraph.push(
                InlineNode::new(&text).marks(vec![(0..text.len(), TextMark::default().code())]),
            );
        }
        Node::MdxTextExpression(raw) => {
            text = raw.value.clone();
            paragraph
                .push(InlineNode::new(&text).marks(vec![(0..text.len(), TextMark::default())]));
        }
        Node::Html(val) => match super::html::parse(&val.value, cx) {
            Ok(el) => {
                if let Some(inline_text) = append_inline_html_blocks(paragraph, el.blocks) {
                    text = inline_text;
                } else {
                    if cfg!(debug_assertions) {
                        tracing::warn!("unsupported inline html tag: {:#?}", val.value);
                    }
                }
            }
            Err(err) => {
                if cfg!(debug_assertions) {
                    tracing::warn!("failed parsing html: {:#?}", err);
                }

                text.push_str(&val.value);
            }
        },
        Node::FootnoteReference(foot) => {
            let prefix = format!("[{}]", foot.identifier);
            paragraph.push(InlineNode::new(&prefix).marks(vec![(
                0..prefix.len(),
                TextMark {
                    italic: true,
                    ..Default::default()
                },
            )]));
        }
        Node::LinkReference(link) => {
            let link_mark = LinkMark {
                url: "".into(),
                title: link.label.clone().map(Into::into),
                identifier: Some(link.identifier.clone().into()),
                raw: link.position.as_ref().zip(cx.source.as_ref()).and_then(|(p, s)| s.get(p.start.offset..p.end.offset)).map(|r| SharedString::from(r.to_string())),
            };

            text = merge_children_with_mark(
                paragraph,
                &link.children,
                TextMark {
                    link: Some(link_mark),
                    ..Default::default()
                },
                cx,
            );
        }
        _ => {
            if cfg!(debug_assertions) {
                tracing::warn!("unsupported inline node: {:#?}", node);
            }
        }
    }

    text
}

fn ast_to_document(source: &str, root: mdast::Node, cx: &mut NodeContext) -> ParsedDocument {
    let root = match root {
        Node::Root(r) => r,
        _ => panic!("expected root node"),
    };

    let mut blocks = blocks_of(source, root.children, cx);
    if cx.markdown_extensions.is_quarto() {
        blocks = super::quarto::margin_rows(blocks);
    }
    ParsedDocument {
        source: source.to_string().into(),
        blocks,
    }
}

/// The blocks of a list of sibling nodes. With Quarto's syntax on, a
/// paragraph holding fence lines (`::: {.class}`, `:::`) is cut at them,
/// the pieces between read again on their own, and what stands between
/// an open and its close nested into a `Div` (`quarto.rs`). (Emaki
/// addition.)
fn blocks_of(source: &str, children: Vec<mdast::Node>, cx: &mut NodeContext) -> Vec<BlockNode> {
    if !cx.markdown_extensions.is_quarto() {
        return children.into_iter().map(|c| ast_to_node(source, c, cx)).collect();
    }
    let mut pieces = Vec::new();
    for child in children {
        match child {
            Node::Paragraph(val) if val.position.is_some() => {
                let pos = val.position.clone().unwrap();
                let (start, end) = (pos.start.offset, pos.end.offset.min(source.len()));
                let slice = &source[start..end];
                let fences = super::quarto::fences(slice);
                if fences.is_empty() {
                    pieces.push(super::quarto::Piece::Block(ast_to_node(source, Node::Paragraph(val), cx)));
                    continue;
                }
                let mut at = 0;
                for fence in fences {
                    pieces.extend(reparse(source, start + at, start + fence.range.start, cx));
                    let span = Some(Span { start: cx.offset + start + fence.range.start, end: cx.offset + start + fence.range.end });
                    pieces.push(match fence.open {
                        Some(attrs) => super::quarto::Piece::Open(attrs, span),
                        None => super::quarto::Piece::Close(span),
                    });
                    at = (fence.range.end + 1).min(slice.len());
                }
                pieces.extend(reparse(source, start + at, end, cx));
            }
            other => pieces.push(super::quarto::Piece::Block(ast_to_node(source, other, cx))),
        }
    }
    super::quarto::nest(pieces)
}

/// The piece of `source` between two fences read as markdown of its own,
/// its spans where the piece stands.
fn reparse(source: &str, start: usize, end: usize, cx: &mut NodeContext) -> Vec<super::quarto::Piece> {
    let Some(piece) = source.get(start..end) else { return Vec::new() };
    if piece.trim().is_empty() {
        return Vec::new();
    }
    let options = cx.markdown_extensions.parse_options();
    let Ok(Node::Root(root)) = markdown::to_mdast(piece, &options) else { return Vec::new() };
    let was = cx.offset;
    let was_source = cx.source.replace(piece.into());
    cx.offset += start;
    let blocks: Vec<_> = root.children.into_iter().map(|c| ast_to_node(piece, c, cx)).collect();
    cx.offset = was;
    cx.source = was_source;
    blocks.into_iter().map(super::quarto::Piece::Block).collect()
}

fn new_span(pos: Option<markdown::unist::Position>, cx: &NodeContext) -> Option<Span> {
    let pos = pos?;

    Some(Span {
        start: cx.offset + pos.start.offset,
        end: cx.offset + pos.end.offset,
    })
}

fn ast_to_node(source: &str, value: mdast::Node, cx: &mut NodeContext) -> BlockNode {
    let span = new_span(value.position().cloned(), cx);
    let parse_cx = MarkdownParseContext::new(source, cx.offset);
    if let Some(mut node) = cx.markdown_extensions.parse_block(&value, &parse_cx) {
        node.set_span(span);
        return BlockNode::Custom(node);
    }

    match value {
        Node::Root(_) => unreachable!("node::Root should be handled separately"),
        // A paragraph that is one piece of math is math on its own line
        // (`$$x$$` written in a line), as Pandoc draws it.
        Node::Paragraph(val) if cx.markdown_extensions.is_quarto() && matches!(val.children.as_slice(), [Node::InlineMath(m)] if super::quarto::is_math(&m.value)) => {
            let Some(Node::InlineMath(m)) = val.children.first() else { unreachable!() };
            super::quarto::display_math(&m.value, new_span(val.position.clone(), cx), cx)
        }
        Node::Math(val) if cx.markdown_extensions.is_quarto() => super::quarto::display_math(&val.value, new_span(val.position.clone(), cx), cx),
        Node::Paragraph(val) => {
            let mut paragraph = Paragraph::default();
            parse_inlines(&mut paragraph, &val.children, cx);
            if cx.markdown_extensions.is_quarto() {
                super::quarto::caption_lone_figure(&mut paragraph);
            }
            paragraph.span = new_span(val.position, cx);
            BlockNode::Paragraph(paragraph)
        }
        Node::Blockquote(val) => {
            let children = blocks_of(source, val.children, cx);
            BlockNode::Blockquote {
                children,
                span: new_span(val.position, cx),
            }
        }
        Node::List(list) => {
            let children = list
                .children
                .into_iter()
                .map(|c| ast_to_node(source, c, cx))
                .collect();
            BlockNode::List {
                ordered: list.ordered,
                children,
                span: new_span(list.position, cx),
            }
        }
        Node::ListItem(val) => {
            let children = blocks_of(source, val.children, cx);
            BlockNode::ListItem {
                children,
                spread: val.spread,
                checked: val.checked,
                span: new_span(val.position, cx),
            }
        }
        Node::Break(val) => BlockNode::Break {
            html: false,
            span: new_span(val.position, cx),
        },
        Node::Code(raw) if cx.markdown_extensions.is_quarto() && super::quarto::raw_html(raw.lang.as_deref()) => {
            // A raw block of HTML is the HTML, drawn as the view draws it.
            let span = new_span(raw.position.clone(), cx);
            match super::html::parse(&raw.value, cx) {
                Ok(el) => BlockNode::Root { children: el.blocks, span },
                Err(_) => BlockNode::CodeBlock(CodeBlock::new(raw.value.into(), raw.lang.map(|s| s.into()), span)),
            }
        }
        Node::Code(raw) => BlockNode::CodeBlock(CodeBlock::new(
            raw.value.into(),
            raw.lang.map(|s| s.into()),
            new_span(raw.position, cx),
        )),
        Node::Heading(val) => {
            let mut paragraph = Paragraph::default();
            parse_inlines(&mut paragraph, &val.children, cx);
            let attrs = if cx.markdown_extensions.is_quarto() { super::quarto::take_heading_attrs(&mut paragraph) } else { None };

            BlockNode::Heading {
                attrs,
                level: val.depth,
                children: paragraph,
                span: new_span(val.position, cx),
            }
        }
        Node::Math(val) => BlockNode::CodeBlock(CodeBlock::new(
            val.value.into(),
            None,
            new_span(val.position, cx),
        )),
        Node::Html(val) => match super::html::parse(&val.value, cx) {
            Ok(el) => BlockNode::Root {
                children: el.blocks,
                span: new_span(val.position, cx),
            },
            Err(err) => {
                if cfg!(debug_assertions) {
                    tracing::warn!("error parsing html: {:#?}", err);
                }

                BlockNode::Paragraph(Paragraph::new(val.value))
            }
        },
        Node::MdxFlowExpression(val) => BlockNode::CodeBlock(CodeBlock::new(
            val.value.into(),
            Some("mdx".into()),
            new_span(val.position, cx),
        )),
        Node::Yaml(val) => BlockNode::CodeBlock(CodeBlock::new(
            val.value.into(),
            Some("yml".into()),
            new_span(val.position, cx),
        )),
        Node::Toml(val) => BlockNode::CodeBlock(CodeBlock::new(
            val.value.into(),
            Some("toml".into()),
            new_span(val.position, cx),
        )),
        Node::MdxJsxTextElement(val) => {
            let mut paragraph = Paragraph::default();
            val.children.iter().for_each(|c| {
                parse_paragraph(&mut paragraph, c, cx);
            });
            paragraph.span = new_span(val.position, cx);
            BlockNode::Paragraph(paragraph)
        }
        Node::MdxJsxFlowElement(val) => {
            let mut paragraph = Paragraph::default();
            val.children.iter().for_each(|c| {
                parse_paragraph(&mut paragraph, c, cx);
            });
            paragraph.span = new_span(val.position, cx);
            BlockNode::Paragraph(paragraph)
        }
        Node::ThematicBreak(val) => BlockNode::HorizontalRule {
            span: new_span(val.position, cx),
        },
        Node::Table(val) => {
            let mut table = Table::default();
            table.column_aligns = val
                .align
                .clone()
                .into_iter()
                .map(|align| align.into())
                .collect();
            val.children.iter().for_each(|c| {
                if let Node::TableRow(row) = c {
                    parse_table_row(&mut table, row, cx);
                }
            });
            table.span = new_span(val.position, cx);

            BlockNode::Table(table)
        }
        Node::FootnoteDefinition(def) => {
            let mut paragraph = Paragraph::default();
            let prefix = format!("[{}]: ", def.identifier);
            paragraph.push(InlineNode::new(&prefix).marks(vec![(
                0..prefix.len(),
                TextMark {
                    italic: true,
                    ..Default::default()
                },
            )]));

            def.children.iter().for_each(|c| {
                parse_paragraph(&mut paragraph, c, cx);
            });
            paragraph.span = new_span(def.position, cx);
            BlockNode::Paragraph(paragraph)
        }
        Node::Definition(def) => {
            cx.add_ref(
                def.identifier.clone().into(),
                LinkMark {
                    url: def.url.clone().into(),
                    identifier: Some(def.identifier.clone().into()),
                    title: def.title.clone().map(Into::into),
                    raw: None,
                },
            );

            BlockNode::Definition {
                identifier: def.identifier.clone().into(),
                url: def.url.clone().into(),
                title: def.title.clone().map(|s| s.into()),
                span: new_span(def.position, cx),
            }
        }
        _ => {
            if cfg!(debug_assertions) {
                tracing::warn!("unsupported node: {:#?}", value);
            }
            BlockNode::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::ParentElement;

    use crate::text::{MarkdownExtensions, MarkdownNode, MarkdownPlugin};

    #[test]
    fn test_nested_emphasis_merges_text_marks() {
        let mut cx = NodeContext::default();
        let document = parse("This has **_bold and italic_** text.", &mut cx).unwrap();

        let BlockNode::Paragraph(paragraph) = &document.blocks[0] else {
            panic!("expected paragraph");
        };

        let bold_italic = paragraph
            .children
            .iter()
            .find(|child| child.text.as_ref() == "bold and italic")
            .expect("expected emphasized text");

        assert!(
            bold_italic
                .marks
                .iter()
                .any(|(_, mark)| mark.bold && mark.italic),
            "nested emphasis should produce a bold and italic mark"
        );
    }

    #[test]
    fn test_inline_html_image_stays_in_markdown_paragraph() {
        let mut cx = NodeContext::default();
        let document = parse(
            r#"Before <img src="https://example.com/avatar.png" alt="Avatar" width="32" height="32" /> after."#,
            &mut cx,
        )
        .unwrap();

        let BlockNode::Paragraph(paragraph) = &document.blocks[0] else {
            panic!("expected paragraph");
        };

        assert_eq!(paragraph.children.len(), 3);
        assert_eq!(paragraph.children[0].text.as_ref(), "Before ");
        assert_eq!(paragraph.children[2].text.as_ref(), " after.");

        let image = paragraph.children[1]
            .image
            .as_ref()
            .expect("expected inline html image");
        assert_eq!(image.url.as_ref(), "https://example.com/avatar.png");
        assert_eq!(image.width, Some(gpui::px(32.).into()));
        assert_eq!(image.height, Some(gpui::px(32.).into()));
    }

    #[test]
    fn test_inline_html_image_without_size_stays_in_markdown_paragraph() {
        let mut cx = NodeContext::default();
        let document = parse(
            r#"Before <img src="https://avatars.githubusercontent.com/u/5518"> after."#,
            &mut cx,
        )
        .unwrap();

        let BlockNode::Paragraph(paragraph) = &document.blocks[0] else {
            panic!("expected paragraph");
        };

        assert_eq!(paragraph.children.len(), 3);
        assert_eq!(paragraph.children[0].text.as_ref(), "Before ");
        assert_eq!(paragraph.children[2].text.as_ref(), " after.");

        let image = paragraph.children[1]
            .image
            .as_ref()
            .expect("expected inline html image");
        assert_eq!(
            image.url.as_ref(),
            "https://avatars.githubusercontent.com/u/5518"
        );
        assert_eq!(image.width, None);
        assert_eq!(image.height, None);
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Ticker {
        symbol: String,
    }

    fn parse_ticker_block(node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let Node::Paragraph(paragraph) = node else {
            return None;
        };
        let [Node::Text(text)] = paragraph.children.as_slice() else {
            return None;
        };
        let symbol = text.value.strip_prefix('$')?.to_string();
        let node_text = format!("${symbol}");

        Some(
            MarkdownNode::new("ticker", Ticker { symbol })
                .text(node_text)
                .markdown(cx.node_source(node).unwrap_or_default()),
        )
    }

    #[test]
    fn custom_block_parser_converts_ticker_syntax_to_custom_node() {
        let extensions = MarkdownExtensions::default().block_parser(parse_ticker_block);

        let mut cx = NodeContext {
            markdown_extensions: extensions.into(),
            ..NodeContext::default()
        };
        let document = parse("$TSLA.US", &mut cx).unwrap();

        let BlockNode::Custom(node) = &document.blocks[0] else {
            panic!("expected custom markdown node");
        };
        assert_eq!(node.name(), "ticker");
        assert_eq!(node.as_text(), "$TSLA.US");
        assert_eq!(node.as_markdown(), "$TSLA.US");
        assert_eq!(
            node.data::<Ticker>(),
            Some(&Ticker {
                symbol: "TSLA.US".to_string()
            })
        );
        assert_eq!(document.text(), "$TSLA.US\n");
        assert_eq!(document.to_markdown(), "$TSLA.US");
    }

    struct TickerPlugin {
        name: &'static str,
    }

    impl TickerPlugin {
        fn new(name: &'static str) -> Self {
            Self { name }
        }
    }

    impl MarkdownPlugin for TickerPlugin {
        fn is_block(&self) -> bool {
            true
        }

        fn name(&self) -> &str {
            self.name
        }

        fn parse(&self, node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
            parse_ticker_block(node, cx)
        }

        fn render(
            &self,
            node: &MarkdownNode,
            _window: &mut gpui::Window,
            _cx: &mut gpui::App,
        ) -> impl gpui::IntoElement {
            gpui::div().child(node.as_text().to_string())
        }
    }

    #[test]
    fn custom_block_plugin_registers_parser_and_renderer() {
        let extensions = MarkdownExtensions::default().plugin(TickerPlugin::new("ticker"));

        let mut cx = NodeContext {
            markdown_extensions: extensions.into(),
            ..NodeContext::default()
        };
        let document = parse("$TSLA.US", &mut cx).unwrap();

        let BlockNode::Custom(node) = &document.blocks[0] else {
            panic!("expected custom markdown node");
        };
        assert_eq!(node.name(), "ticker");
        assert_eq!(
            node.data::<Ticker>(),
            Some(&Ticker {
                symbol: "TSLA.US".to_string()
            })
        );
    }
}
