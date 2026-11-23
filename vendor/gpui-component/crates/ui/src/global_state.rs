use gpui::{App, Entity, Global};

use crate::text::TextViewState;

pub use gpui_base::GlobalState;

pub(crate) fn init(cx: &mut App) {
    // Preserve the legacy initialization point while `gpui_base::init` remains
    // after Root initialization for focus-trap ordering compatibility.
    GlobalState::init(cx);
    cx.set_global(UiGlobalState::new());
}

/// UI-only global state whose types cannot cross into `gpui-base`.
pub(crate) struct UiGlobalState {
    pub(crate) text_view_state_stack: Vec<Entity<TextViewState>>,
    /// Emaki: the tables being painted, the innermost last, each with the
    /// cell of it being painted (`text/cells.rs`).
    pub(crate) table_spots: Vec<crate::text::TableSpot>,
    selection_document_order: u64,
}

impl Global for UiGlobalState {}

impl UiGlobalState {
    fn new() -> Self {
        Self {
            text_view_state_stack: Vec::new(),
            table_spots: Vec::new(),
            selection_document_order: 1,
        }
    }

    pub(crate) fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub(crate) fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }

    pub(crate) fn text_view_state(&self) -> Option<&Entity<TextViewState>> {
        self.text_view_state_stack.last()
    }

    /// Emaki: the table a run of text is being painted in, if it is in one.
    pub(crate) fn table_spot(&self) -> Option<crate::text::TableSpot> {
        self.table_spots.last().copied()
    }

    pub(crate) fn begin_selection_frame(&mut self) {
        self.selection_document_order = 1;
    }

    pub(crate) fn next_selection_document_order(&mut self) -> u64 {
        let order = self.selection_document_order;
        self.selection_document_order = self.selection_document_order.wrapping_add(1);
        order
    }
}
