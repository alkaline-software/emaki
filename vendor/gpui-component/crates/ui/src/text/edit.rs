//! A document written in where it reads. (Studio addition.)
//!
//! The view draws markdown; it does not edit it. What it gives a caller
//! that does: the blocks a caret can stand in (`EditUnit`, by where each
//! is in the text), a press on one (`UnitClick`), and a slot, the place
//! of one block where the caller's own element is drawn instead of the
//! block, or after it (`EditSlot`). The caller puts a field there holding
//! that block's markdown, and gives the view the changed text when the
//! field is left.

use std::{cell::Cell, sync::Arc};

use gpui::{AnyElement, App, Pixels, Point, SharedString, Window};

use super::node::{BlockNode, Span};

/// What a block a caret can stand in is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Paragraph,
    Heading(u8),
    /// A block of code, its fences included.
    Code,
    Table,
    Rule,
    /// An item of a list with nothing in it yet.
    Item,
    /// Anything else drawn as one piece: a formula on its own line, a
    /// block of HTML.
    Other,
}

/// A block a caret can stand in: where it is in the text the view was
/// given, what it is, and whether it stands in a quote.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditUnit {
    pub span: Span,
    pub kind: UnitKind,
    pub quoted: bool,
    /// The block of the document's top level it is in: itself, or the
    /// list, quote or div around it.
    pub top: Span,
}

/// The caller's element in a block's place (`after` false), or after the
/// block of that span (`after` true): a new paragraph under it.
#[derive(Clone)]
pub struct EditSlot {
    pub span: Span,
    pub after: bool,
    /// The block is still drawn, the element laid out over it and not
    /// seen: a field takes a frame to know how its words wrap, and shown
    /// at once it stood a line tall for that frame.
    pub warm: bool,
    /// The element stands for every block of the document's top level
    /// inside `span`, not for one block: a selection that runs through
    /// them.
    pub run: bool,
    pub render: Arc<dyn Fn(&mut Window, &mut App) -> AnyElement + Send + Sync>,
}

impl EditSlot {
    /// Where a block of the top level stands to a run: its first block,
    /// one after it, or none of it.
    pub(super) fn run_part(&self, span: Option<Span>) -> Option<bool> {
        let span = span?;
        (self.run && !self.after && span.start >= self.span.start && span.end <= self.span.end).then_some(span.start == self.span.start)
    }
}

impl PartialEq for EditSlot {
    fn eq(&self, other: &Self) -> bool {
        self.span == other.span && self.after == other.after && self.warm == other.warm && self.run == other.run && Arc::ptr_eq(&self.render, &other.render)
    }
}

/// A press on a block: the block, where the pointer was, and, when it
/// was on words, the run of words and the byte of it. `press` when the
/// button went down, which a drag may follow; else a click, the button
/// let go with no drag.
#[derive(Clone)]
pub struct UnitClick {
    pub span: Span,
    pub kind: UnitKind,
    pub position: Point<Pixels>,
    pub text: Option<(SharedString, usize)>,
    pub press: bool,
}

pub type UnitClickFn = dyn Fn(&UnitClick, &mut Window, &mut App) + Send + Sync;

thread_local! {
    /// The run of words the pointer was let go over, and the byte of it,
    /// kept by the run for the block's own press, which comes after it.
    static WORDS_PRESSED: Cell<Option<(Point<Pixels>, SharedString, usize)>> = const { Cell::new(None) };
}

pub(super) fn note_words_pressed(position: Point<Pixels>, text: SharedString, index: usize) {
    WORDS_PRESSED.with(|w| w.set(Some((position, text, index))));
}

pub(super) fn take_words_pressed(position: Point<Pixels>) -> Option<(SharedString, usize)> {
    WORDS_PRESSED.with(|w| w.take()).filter(|(at, _, _)| *at == position).map(|(_, text, index)| (text, index))
}

impl BlockNode {
    /// What this block is to a caret, when it is one a caret stands in.
    pub(super) fn unit_kind(&self) -> Option<UnitKind> {
        self.span()?;
        Some(match self {
            BlockNode::Paragraph(_) => UnitKind::Paragraph,
            BlockNode::Heading { level, .. } => UnitKind::Heading(*level),
            BlockNode::CodeBlock(_) => UnitKind::Code,
            BlockNode::Table(_) => UnitKind::Table,
            BlockNode::HorizontalRule { .. } => UnitKind::Rule,
            BlockNode::Custom(_) => UnitKind::Other,
            // A block of HTML, read into blocks of its own.
            BlockNode::Root { .. } => UnitKind::Other,
            BlockNode::ListItem { children, .. } if children.is_empty() => UnitKind::Item,
            _ => return None,
        })
    }

    fn units_into(&self, quoted: bool, top: Option<Span>, out: &mut Vec<EditUnit>) {
        let top = top.or(self.span());
        if let (Some(kind), Some(span)) = (self.unit_kind(), self.span()) {
            out.push(EditUnit { span, kind, quoted, top: top.unwrap_or(span) });
            return;
        }
        match self {
            BlockNode::Root { children, .. } | BlockNode::Div { children, .. } | BlockNode::List { children, .. } | BlockNode::ListItem { children, .. } => {
                for c in children {
                    c.units_into(quoted, top, out);
                }
            }
            BlockNode::Blockquote { children, .. } => {
                for c in children {
                    c.units_into(true, top, out);
                }
            }
            _ => {}
        }
    }
}

/// The blocks a caret can stand in, in the order the text has them.
pub(super) fn units_of(blocks: &[BlockNode]) -> Vec<EditUnit> {
    let mut out = Vec::new();
    for b in blocks {
        b.units_into(false, None, &mut out);
    }
    out.sort_by_key(|u| u.span.start);
    out
}
