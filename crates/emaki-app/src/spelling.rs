//! A file of prose in its editor, checked as the composer is: markdown,
//! Quarto and plain text files get the composer's spelling and grammar
//! (`emaki_core::check`, `docs/composer.md`), in the language Settings
//! names, marked with the editor's own wavy underlines and corrected
//! from the right click's menu (`docs/panels.md`, the file's editor).
//! Code is not checked: a comment's prose is for another day.

use std::path::Path;

use gpui::*;
use gpui_component::input::{EditorState, TextDecoration};
use gpui_component::ActiveTheme as _;

use emaki_core::check::{self, Kind};

use crate::workbench::{MenuDo, Workbench, CHECK_AFTER, CHECK_WORD_AFTER};

/// Past this many bytes a file is not checked: Harper reads the whole
/// text at each pause in the typing.
const CHECK_MOST: usize = 200_000;

/// Whether a file is prose the check reads: markdown, Quarto, R
/// Markdown and plain text, by its extension.
pub(crate) fn checked(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "md" | "markdown" | "qmd" | "rmd" | "txt" | "text")
}

impl Workbench {
    /// Whether the editor showing holds a file the check reads, and the
    /// check is on.
    fn file_checking(&self) -> bool {
        self.cfg.app.check_writing && self.file_editor.as_ref().is_some_and(|ed| ed.saved.is_some() && checked(&ed.path))
    }

    /// One change of the file's text by the person's hand: the marks
    /// move with the words until the text is checked again.
    pub(crate) fn file_writing_changed(&mut self, state: &Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(ed) = self.file_editor.as_mut().filter(|ed| ed.state == *state) else { return };
        let new = state.read(cx).value().to_string();
        ed.issues = check::carry(&ed.issues_for, &new, &ed.issues);
        ed.issues_for = new;
        self.file_marks(cx);
        self.file_check_soon(cx);
    }

    /// The file's text was set from outside a keystroke (formatted at a
    /// save, a draft back): what was marked was another text's. Run at
    /// every draw, as the composer's is.
    pub(crate) fn file_writing_sync(&mut self, cx: &mut Context<Self>) {
        if !self.file_checking() {
            return;
        }
        let Some(ed) = self.file_editor.as_mut() else { return };
        let now = ed.state.read(cx).value().to_string();
        if ed.issues_for == now {
            return;
        }
        // This text has no marks until its check answers. Left as the old
        // text's, every draw till then began the check again and dropped
        // the one waiting: a window that drew on (a GIF, a turn at work)
        // was never checked.
        ed.issues.clear();
        ed.issues_for = now;
        self.file_check_soon(cx);
    }

    /// Checks the file's text once the typing has paused, as the
    /// composer's `check_soon` does: English is Harper's, off the main
    /// thread; another language's spelling is the system's. The word
    /// being typed is marked a little later than the rest.
    pub(crate) fn file_check_soon(&mut self, cx: &mut Context<Self>) {
        let checking = self.file_checking();
        let Some(ed) = self.file_editor.as_mut() else { return };
        ed.check_serial += 1;
        let serial = ed.check_serial;
        let (state, text) = (ed.state.clone(), ed.state.read(cx).value().to_string());
        if !checking || text.trim().is_empty() || text.len() > CHECK_MOST {
            ed.issues.clear();
            ed.issues_for = text;
            self.file_marks(cx);
            return;
        }
        let language = self.cfg.app.writing_language.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHECK_AFTER).await;
            let same = |this: &Self| this.file_editor.as_ref().is_some_and(|ed| ed.state == state && ed.check_serial == serial);
            if !this.read_with(cx, |this, _| same(this)).unwrap_or(false) {
                return;
            }
            let mut found = if check::is_english(&language) {
                let (text, british) = (text.clone(), language == "en-GB");
                cx.background_spawn(async move { check::english(&text, british) }).await
            } else {
                crate::sys::spelling(&text, &language).await
            };
            found.extend(check::capitals(&text));
            let shown = this.update(cx, |this, cx| {
                if !same(this) {
                    return false;
                }
                let kept = check::keep(&text, found, &this.learned, &this.ignored);
                if let Some(ed) = this.file_editor.as_mut() {
                    ed.issues = kept;
                    ed.issues_for = text;
                    ed.check_at_caret = false;
                }
                this.file_marks(cx);
                true
            });
            if !shown.unwrap_or(false) {
                return;
            }
            cx.background_executor().timer(CHECK_WORD_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                if same(this) {
                    if let Some(ed) = this.file_editor.as_mut() {
                        ed.check_at_caret = true;
                    }
                    this.file_marks(cx);
                }
            });
        })
        .detach();
    }

    /// The marks are the composer's: a wavy underline, red for a
    /// spelling and blue for the grammar, with nothing on hover, and the
    /// corrections on the right click. They are the editor's decorations,
    /// not its diagnostics: a diagnostic brought up a card at the pointer
    /// that said Harper's words, went as the pointer moved to it, and
    /// narrowed the editor while it was up. They are set again from the
    /// issues at every change, since the editor keeps them as bytes of
    /// the text they were set on.
    fn file_marks(&mut self, cx: &mut Context<Self>) {
        let (danger, info) = (cx.theme().danger, cx.theme().info);
        let Some(ed) = self.file_editor.as_mut() else { return };
        let marks: Vec<TextDecoration> = if ed.issues_for.as_str() == ed.state.read(cx).value().as_ref() {
            let caret = ed.state.read(cx).cursor();
            ed.issues
                .iter()
                .filter(|i| i.kind == Kind::Grammar || ed.check_at_caret || i.range.end != caret)
                .filter(|i| i.kind == Kind::Spelling || !i.fixes.is_empty())
                .map(|i| {
                    let color = if i.kind == Kind::Spelling { danger } else { info };
                    TextDecoration::new(i.range.clone(), HighlightStyle { underline: Some(UnderlineStyle { thickness: px(1.), color: Some(color), wavy: true }), ..Default::default() })
                })
                .collect()
        } else {
            Vec::new()
        };
        match &ed.spell_marks {
            Some(set) => set.set(marks, cx),
            None if marks.is_empty() => {}
            None => ed.spell_marks = Some(ed.state.update(cx, |s, cx| s.create_decorations_collection(marks, cx))),
        }
    }

    /// What could stand in place of the marked words under a point of
    /// the editor, as its menu's first choices.
    pub(crate) fn file_corrections(&self, at: Point<Pixels>, cx: &App) -> Vec<(&'static str, MenuDo)> {
        let Some(ed) = self.file_editor.as_ref() else { return Vec::new() };
        let state = ed.state.read(cx);
        if !self.file_checking() || ed.issues_for.as_str() != state.value().as_ref() {
            return Vec::new();
        }
        let offset = state.offset_at(at);
        ed.issues.iter().find(|i| i.range.start <= offset && offset <= i.range.end).map(|issue| Self::issue_items(issue, &ed.issues_for, true, &self.cfg.app.writing_language)).unwrap_or_default()
    }

    /// A correction chosen from the menu goes where the marked words
    /// were, as one step for undo.
    pub(crate) fn file_fix(&mut self, range: std::ops::Range<usize>, with: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.file_editor.as_ref() else { return };
        if ed.state.read(cx).value().as_ref() != ed.issues_for.as_str() || ed.issues_for.get(range.clone()).is_none() {
            return;
        }
        let caret = range.start + with.len();
        ed.state.update(cx, |s, cx| {
            s.replace_bytes(range, &with, caret, window, cx);
            s.focus(window, cx);
        });
    }

    /// After Learn Spelling or Ignore: the file's marks less the ones
    /// those took, as the composer's are.
    pub(crate) fn file_issues_keep(&mut self, cx: &mut Context<Self>) {
        let (learned, ignored) = (self.learned.clone(), self.ignored.clone());
        let Some(ed) = self.file_editor.as_mut() else { return };
        let text = std::mem::take(&mut ed.issues_for);
        ed.issues = check::keep(&text, std::mem::take(&mut ed.issues), &learned, &ignored);
        ed.issues_for = text;
        self.file_marks(cx);
    }
}
