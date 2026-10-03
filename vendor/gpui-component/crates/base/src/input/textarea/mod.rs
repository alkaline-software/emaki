use gpui::{App, Entity, IntoElement, RenderOnce, Window};

use super::{InputBaseState, TextareaMode};

/// State for editing ordinary multi-line text.
///
/// This is the shared editing engine in its multi-line kind. Code-editor
/// facilities such as languages, diagnostics, folding, and LSP do not exist on
/// this type — those methods live on [`super::EditorState`].
pub type TextareaState = InputBaseState<TextareaMode>;

impl InputBaseState<TextareaMode> {
    /// Colour these ranges (UTF-8 byte offsets into the value), replacing
    /// whatever was set before. Call it again after the text changes.
    /// (Emaki addition.)
    pub fn set_marks(&mut self, marks: Vec<super::TextDecoration>, cx: &mut gpui::Context<Self>) {
        self.extras.marks = marks.into_iter().filter(|m| !m.range.is_empty()).collect();
        cx.notify();
    }
}

/// An unstyled ordinary multi-line text input.
#[derive(IntoElement)]
pub struct Textarea {
    state: Entity<TextareaState>,
}

impl Textarea {
    pub fn new(state: &Entity<TextareaState>) -> Self {
        Self {
            state: state.clone(),
        }
    }
}

impl RenderOnce for Textarea {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.state
    }
}
