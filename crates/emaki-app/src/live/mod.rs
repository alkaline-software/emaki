//! A live-preview markdown editor, brought over from Onyx: the text is
//! drawn as it reads while it is typed. A span's marks (`**`, `*`,
//! backticks, `#`, a link's address) hide unless the caret or the
//! selection touches them; a heading is larger; code is monospace on a
//! plate; a list has its bullets, numbers and boxes with their lines
//! hung under them; a quote has its bar; a table is a grid of cells.
//!
//! `Editor` is a view as tall as its text: the view around it puts it in
//! a scroller (`Editor::scroll`), gives it its look (`Look`, `set_look`),
//! binds its keys (`key_bindings`, under `LIVE_CONTEXT`), hears what it
//! says (`EditorEvent`), and draws a placeholder of its own, since an
//! empty editor draws its caret and nothing else. Typed text arrives
//! through gpui's input handler, never a binding; a key that moves or
//! changes is an action here, and Copy, Cut, Paste, Select All, Undo and
//! Redo are the toolkit's own (`gpui_component::input`).
//!
//! What a note has and a field does not is left in Onyx: pictures, files
//! in frames, cards, PDF pages, the vault and its `[[links]]`. A picture
//! and a block of HTML show as the text they are written as, unless the
//! view around draws them over their lines (`Piece`: a markdown file
//! written in as it reads).
#![allow(dead_code)]

mod editor;
mod field;
mod html;
mod langs;
mod layout;
mod lines;
mod markdown;
mod rhythm;
mod tables;

#[allow(unused_imports)]
pub(crate) use editor::{
    key_bindings, Backspace, Bold, CellAt, Colors, Delete, DeleteLineBack, DeleteWordBack, DocEnd, DocStart, Down, Editor, EditorEvent, Enter, Escape, Italic, Left, LineEnd, LineStart, Look, Mark, MenuAt, MenuIssue, MenuLink, Mode, NewLine, Outdent, Piece, PageDown, PageUp, Right, Scope, SelectDocEnd, SelectDocStart, SelectDown, SelectLeft, SelectLineEnd,
    SelectLineStart, SelectRight, SelectUp, SelectWordLeft, SelectWordRight, Tab, TableFacts, ToggleTask, Up, WordLeft, WordRight, LIVE_CONTEXT,
};
#[allow(unused_imports)]
pub(crate) use markdown::{code_around, Align, Prose};
pub(crate) use rhythm::Heading;
