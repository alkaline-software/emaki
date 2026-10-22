//! A file of prose in its editor, checked as the composer is: markdown,
//! Quarto and plain text files get the composer's spelling and grammar
//! (`emaki_core::check`, `docs/composer.md`), in the language Settings
//! names, marked with the editor's own wavy underlines and corrected
//! from the right click's menu (`docs/panels.md`, the file's editor).
//! Code is not checked: a comment's prose is for another day.

use std::path::Path;

use gpui::*;
use gpui_base::input::{Diagnostic, DiagnosticSeverity, RopeExt as _};
use gpui_component::input::EditorState;

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
        if ed.issues_for.as_str() == ed.state.read(cx).value().as_ref() {
            return;
        }
        ed.issues.clear();
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
                crate::sys::spelling(&text, &language)
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

    /// The marks are the editor's diagnostics: a wavy underline, red
    /// for a spelling and blue for the grammar, with the message on
    /// hover. They are set again from the issues at every change, since
    /// the editor keeps them as bytes of the text they were set on.
    fn file_marks(&mut self, cx: &mut Context<Self>) {
        let Some(ed) = self.file_editor.as_ref() else { return };
        let issues: Vec<(std::ops::Range<usize>, bool, String)> = if ed.issues_for.as_str() == ed.state.read(cx).value().as_ref() {
            let caret = ed.state.read(cx).cursor();
            ed.issues
                .iter()
                .filter(|i| i.kind == Kind::Grammar || ed.check_at_caret || i.range.end != caret)
                .filter(|i| i.kind == Kind::Spelling || !i.fixes.is_empty())
                .map(|i| (i.range.clone(), i.kind == Kind::Spelling, i.message.clone()))
                .collect()
        } else {
            Vec::new()
        };
        ed.state.update(cx, |s, cx| {
            let text = s.text().clone();
            if let Some(set) = s.diagnostics_mut() {
                set.reset(&text);
                for (range, spelling, message) in issues {
                    let (a, b) = (text.offset_to_position(range.start), text.offset_to_position(range.end));
                    set.push(Diagnostic::new(a..b, message).with_severity(if spelling { DiagnosticSeverity::Error } else { DiagnosticSeverity::Info }));
                }
            }
            cx.notify();
        });
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
        ed.issues.iter().find(|i| i.range.start <= offset && offset <= i.range.end).map(|issue| Self::issue_items(issue, &ed.issues_for, true)).unwrap_or_default()
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
