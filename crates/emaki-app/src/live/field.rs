//! The editor as Emaki's composer knows its field: the names the
//! toolkit's field answers to, over the editor's own (`docs/composer.md`,
//! Live markdown), so what reads and writes the composer's text is the
//! same code whichever stands there.

use std::ops::Range;

use gpui::*;

use super::Editor;

impl Editor {
    pub(crate) fn value(&self) -> SharedString {
        self.text().into()
    }

    pub(crate) fn cursor(&self) -> usize {
        self.caret_byte()
    }

    /// What is selected, its start first whichever way it was made.
    pub(crate) fn selected_range(&self) -> Range<usize> {
        let r = self.selection_bytes();
        r.start.min(r.end)..r.start.max(r.end)
    }

    pub(crate) fn set_selected_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.set_selection_bytes(range, cx);
    }

    /// Another text in the field, its history begun again and the caret
    /// at its start.
    pub(crate) fn set_value(&mut self, text: impl Into<SharedString>, _: &mut Window, cx: &mut Context<Self>) {
        let text: SharedString = text.into();
        self.set_text(&text, cx);
    }

    /// The caret at a line and a place along it, counted as an input
    /// method counts (UTF-16).
    pub(crate) fn set_cursor_position(&mut self, at: gpui_component::input::Position, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.text();
        let mut start = 0;
        for (ix, line) in text.split('\n').enumerate() {
            if ix == at.line as usize {
                let mut units = 0;
                let along = line.char_indices().find(|(_, c)| {
                    let here = units >= at.character as usize;
                    units += c.len_utf16();
                    here
                }).map_or(line.len(), |(byte, _)| byte);
                let caret = start + along;
                self.set_selection_bytes(caret..caret, cx);
                return;
            }
            start += line.len() + 1;
        }
        self.set_selection_bytes(text.len()..text.len(), cx);
    }

    /// These bytes replaced as typing over them would, one step of undo,
    /// and the caret left at `caret`, a byte of the new text.
    pub(crate) fn replace_bytes(&mut self, range: Range<usize>, with: &str, caret: usize, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_edits(vec![(range, with.to_string())], cx);
        let caret = caret.min(self.text().len());
        self.set_selection_bytes(caret..caret, cx);
    }

    /// Words put where the caret is, over what is selected.
    pub(crate) fn put(&mut self, text: impl Into<SharedString>, _: &mut Window, cx: &mut Context<Self>) {
        let text: SharedString = text.into();
        let range = self.selected_range();
        let caret = range.start + text.len();
        self.apply_edits(vec![(range, text.to_string())], cx);
        self.set_selection_bytes(caret..caret, cx);
    }

    pub(crate) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle(cx), cx);
    }

    /// Where the field stands in the window: its scroller's room.
    pub(crate) fn input_bounds(&self) -> Bounds<Pixels> {
        self.scroll.bounds()
    }
}
