use ropey::Rope;
use std::ops::Range;
use std::time::Instant;

use gpui::{
    Animation, AnimationExt as _, App, AppContext as _, Context, ElementId, Empty, Entity,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _,
    Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    WeakEntity, Window, actions, div, percentage, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme, ElementExt as _, Icon, IconName, Sizable,
    animation::ease_out_cubic,
    h_flex,
    input::{
        Enter, Escape, IndentInline, Input, InputBaseState, InputEvent, InputState, OutdentInline,
        Replace, Search,
    },
    tooltip::ManagedTooltipExt as _,
    v_flex,
};
use gpui_base::input::{SEARCH_ANIM, SearchOptions};

const CONTEXT: &'static str = "SearchPanel";

actions!(input, [Tab, ToggleMatchCase, ToggleWholeWord, ToggleRegex]);

/// Emaki: the panel's own keys for its three toggles, as VS Code has them.
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-alt-c", ToggleMatchCase, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-alt-w", ToggleWholeWord, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-alt-r", ToggleRegex, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("alt-c", ToggleMatchCase, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("alt-w", ToggleWholeWord, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("alt-r", ToggleRegex, Some(CONTEXT)),
    ]);
}

#[cfg(test)]
use gpui_base::input::SearchMatcher;

#[derive(Clone, Copy)]
#[cfg(test)]
enum MoveDirection {
    Up,
    Down,
}

#[cfg(test)]
fn next_scroll_direction(
    previous_match_ix: usize,
    current_match_ix: usize,
) -> Option<MoveDirection> {
    if current_match_ix <= previous_match_ix {
        None
    } else {
        Some(MoveDirection::Down)
    }
}

#[cfg(test)]
fn prev_scroll_direction(
    previous_match_ix: usize,
    current_match_ix: usize,
) -> Option<MoveDirection> {
    if current_match_ix >= previous_match_ix {
        None
    } else {
        Some(MoveDirection::Up)
    }
}

/// One row of the panel, and a button in it.
const ROW_H: f32 = 28.;
const TOOL: f32 = 22.;
/// Narrower than this, the toggles, the count and the steps go to a
/// row of their own under the field, which keeps the room for the words.
const ONE_ROW_W: f32 = 380.;

/// Emaki: drawn as the window's find bar is drawn, a card with the
/// field, the match's place and the steps, after VS Code's: a chevron
/// that opens the replace row, and the case, whole word and regular
/// expression toggles in the field. It comes and goes in motion, and so
/// does the replace row. Upstream had a bar of the toolkit's buttons.
pub(super) struct SearchPanel<M: crate::input::overlay::OverlayMode> {
    editor: WeakEntity<InputBaseState<M>>,
    search_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    session: gpui_base::input::SearchSession,
    input_width: Pixels,
    /// The card's own size as last drawn: its width for the layout of
    /// its rows, its height for the motion that opens and closes it.
    width: Pixels,
    height: Pixels,
    /// The replace row's last turn: on or off, when, and a serial for
    /// its motion.
    replace_turn: Option<(bool, Instant, u64)>,

    _subscriptions: Vec<Subscription>,
}

impl<M: crate::input::overlay::OverlayMode> SearchPanel<M> {
    pub(super) fn sync_session(&mut self, session: &gpui_base::input::SearchSession) {
        self.session = session.clone();
    }

    pub(crate) fn new(
        editor: Entity<InputBaseState<M>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("Replace"));

        cx.new(|cx| {
            let _subscriptions =
                vec![
                    cx.subscribe(&search_input, |this: &mut Self, _, ev: &InputEvent, cx| {
                        // Handle search input changes
                        match ev {
                            InputEvent::Change => {
                                this.update_search_query(None, cx);
                            }
                            _ => {}
                        }
                    }),
                ];

            Self {
                editor: editor.downgrade(),
                search_input,
                replace_input,
                session: gpui_base::input::SearchSession::default(),
                input_width: Pixels::ZERO,
                width: Pixels::ZERO,
                height: Pixels::ZERO,
                replace_turn: None,
                _subscriptions,
            }
        })
    }

    pub(super) fn show_with_focus(
        &mut self,
        selected_text: &Rope,
        replace_mode: bool,
        visible_range_offset: Option<Range<usize>>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.open = true;
        self.session.replace_mode = replace_mode;
        if focus {
            let field = if replace_mode { &self.replace_input } else { &self.search_input };
            field.read(cx).focus_handle(cx).focus(window, cx);
        }

        self.search_input.update(cx, |this, cx| {
            if selected_text.len() > 0 {
                this.set_value(selected_text.to_string(), window, cx);
            }
            this.select_all(window, cx);
        });

        // The `set_value` does not emit `InputEvent::Change`, so update the query
        // here to match the value of the search input.
        self.update_search_query(visible_range_offset, cx);
    }

    /// Update the matcher by the value of the search input.
    ///
    /// The `visible_range_offset` is to select the nearest match of the visible range,
    /// it is passed in, because the editor may be borrowed by the caller.
    fn update_search_query(
        &mut self,
        visible_range_offset: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let query = self.search_input.read(cx).value();
        let options = self.session.options();
        let editor = self.editor.clone();
        let _ = editor.update(cx, |state, cx| {
            state.set_search_options(query.clone(), options, cx);
        });
        if let Ok(session) = editor.read_with(cx, |state, _| state.search_session().clone()) {
            self.session = session;
        }
        if let Some(visible_range_offset) = visible_range_offset {
            self.session
                .matcher
                .update_cursor_by_offset(visible_range_offset.start);
        }
        cx.notify();
    }

    fn set_options(&mut self, options: SearchOptions, cx: &mut Context<Self>) {
        self.session.case_insensitive = options.case_insensitive;
        self.session.whole_word = options.whole_word;
        self.session.regex = options.regex;
        self.update_search_query(None, cx);
    }

    fn toggle_case(&mut self, cx: &mut Context<Self>) {
        let mut options = self.session.options();
        options.case_insensitive = !options.case_insensitive;
        self.set_options(options, cx);
    }

    fn toggle_word(&mut self, cx: &mut Context<Self>) {
        let mut options = self.session.options();
        options.whole_word = !options.whole_word;
        self.set_options(options, cx);
    }

    fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        let mut options = self.session.options();
        options.regex = !options.regex;
        self.set_options(options, cx);
    }

    fn replaceable(&self, cx: &App) -> bool {
        self.editor
            .read_with(cx, |editor, _| editor.is_replaceable())
            .unwrap_or(false)
    }

    pub(super) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.hide_with_focus(true, window, cx);
    }

    pub(super) fn hide_with_focus(
        &mut self,
        focus_editor: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_open = self.session.open;
        self.session.open = false;
        let _ = self.editor.update(cx, |state, cx| state.close_search(cx));
        if let Ok(session) = self.editor.read_with(cx, |state, _| state.search_session().clone()) {
            self.session = session;
        }
        if focus_editor {
            if let Some(editor) = self.editor.upgrade() {
                editor.read(cx).focus_handle(cx).focus(window, cx);
            }
        }
        if was_open {
            // Drawn while it goes: the editor once more when it has.
            let editor = self.editor.clone();
            cx.spawn(async move |_, cx| {
                cx.background_executor().timer(SEARCH_ANIM + std::time::Duration::from_millis(20)).await;
                let _ = editor.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        cx.notify();
    }

    fn on_action_enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        // In the replace field ↩ replaces the match and ⌘↩ every one.
        if self.session.replace_mode && self.replace_input.read(cx).focus_handle(cx).is_focused(window) {
            if action.secondary {
                self.replace_all(window, cx);
            } else {
                self.replace_next(window, cx);
            }
        } else if action.shift {
            self.prev(window, cx);
        } else {
            self.next(window, cx);
        }
    }

    fn on_action_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(window, cx);
    }

    /// ⌘F in the panel puts it away, as ⌘F in the window's find bar does.
    fn on_action_search(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(window, cx);
    }

    fn on_action_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(window, cx);
    }

    fn on_action_tab_prev(
        &mut self,
        _: &OutdentInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_focus(window, cx);
    }

    /// Cycle focus between the search and the replace input, to keep the Tab key
    /// staying in the panel.
    ///
    /// There are only 2 inputs, so the forward and the backward are the same.
    fn cycle_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session.replace_mode || !self.replaceable(cx) {
            return;
        }

        let search_focus_handle = self.search_input.read(cx).focus_handle(cx);
        let focus_handle = if search_focus_handle.is_focused(window) {
            self.replace_input.read(cx).focus_handle(cx)
        } else {
            search_focus_handle
        };
        focus_handle.focus(window, cx);
    }

    fn on_action_replace(&mut self, _: &Replace, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_replace_mode(window, cx);
    }

    /// Toggle the replace field, and move focus to the field that is going to be used.
    fn toggle_replace_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            return;
        }

        self.session.replace_mode = !self.session.replace_mode;
        let replace_mode = self.session.replace_mode;
        let serial = self.replace_turn.map(|(_, _, n)| n + 1).unwrap_or(1);
        self.replace_turn = Some((replace_mode, Instant::now(), serial));
        let _ = self.editor.update(cx, |state, cx| {
            state.set_search_replace_mode(replace_mode, cx);
        });
        let focus_handle = if self.session.replace_mode {
            self.replace_input.read(cx).focus_handle(cx)
        } else {
            self.search_input.read(cx).focus_handle(cx)
        };
        focus_handle.focus(window, cx);
        if !replace_mode {
            // The row is drawn while it goes: once more when it has.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SEARCH_ANIM + std::time::Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        cx.notify();
    }

    fn prev(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.previous_search_match(cx);
        });
    }

    fn next(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.next_search_match(cx);
        });
    }

    fn replace_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            self.session.replace_mode = false;
            cx.notify();
            return;
        }

        let replacement = self.replace_input.read(cx).value();
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.replace_current_search_match(&replacement, window, cx);
        });
    }

    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replaceable(cx) {
            self.session.replace_mode = false;
            cx.notify();
            return;
        }

        let replacement = self.replace_input.read(cx).value();
        let _ = self.editor.update(cx, |state, cx| {
            _ = state.replace_all_search_matches(&replacement, window, cx);
        });
    }

    /// A small button of the panel: an icon, its label on hover, lit in
    /// the accent while `on`, quiet while it can do nothing.
    fn tool(
        &self,
        id: &'static str,
        icon: Icon,
        tip: &'static str,
        on: bool,
        enabled: bool,
        size: f32,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> gpui::AnyElement {
        let theme = cx.theme();
        let lit = theme.primary;
        let ink = if !enabled {
            theme.muted_foreground.opacity(0.45)
        } else if on {
            lit
        } else {
            theme.muted_foreground
        };
        let hover = if on { lit.opacity(0.2) } else { theme.foreground.opacity(0.07) };
        div()
            .id(id)
            .size(px(size))
            .flex_shrink_0()
            .rounded(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .when(on, |d| d.bg(lit.opacity(0.13)))
            .when(enabled, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, window, cx| act(this, window, cx)))
            })
            .managed_tooltip(move |window, cx| crate::tooltip::label(tip, window, cx))
            .child(icon.with_size(px(14.)).text_color(ink))
            .into_any_element()
    }
}

impl<M: crate::input::overlay::OverlayMode> Focusable for SearchPanel<M> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search_input.read(cx).focus_handle(cx)
    }
}

impl<M: crate::input::overlay::OverlayMode> Render for SearchPanel<M> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.session.open;
        if !open && !self.session.closing() {
            return Empty.into_any_element();
        }

        let theme = cx.theme().clone();
        let matcher = &self.session.matcher;
        let has_matches = !matcher.is_empty();
        let allow_replace = self.replaceable(cx);
        if !allow_replace {
            self.session.replace_mode = false;
        }
        let replace_mode = self.session.replace_mode;
        let options = self.session.options();
        let empty = self.search_input.read(cx).value().is_empty();
        let narrow = self.width > Pixels::ZERO && self.width < px(ONE_ROW_W);
        let (n, at) = (matcher.len(), matcher.current_match_index() + 1);
        let (count, bad): (SharedString, bool) = if empty {
            ("".into(), false)
        } else if matcher.is_invalid() {
            ("invalid".into(), true)
        } else if n == 0 {
            ("no matches".into(), true)
        } else {
            (format!("{at} of {n}").into(), false)
        };

        // The replace row: drawn while it is on, and while it goes.
        let (row_on, row_at, row_serial) =
            self.replace_turn.unwrap_or((replace_mode, Instant::now() - SEARCH_ANIM, 0));
        let row_shown = allow_replace && (replace_mode || (!row_on && row_at.elapsed() < SEARCH_ANIM));
        let turned_at_open = self.replace_turn.is_none();

        let chevron = allow_replace.then(|| {
            let (from, to) = if replace_mode { (0., 0.25) } else { (0.25, 0.) };
            let icon = Icon::new(IconName::ChevronRight);
            let still = turned_at_open;
            div()
                .id("search-replace-toggle")
                .size(px(TOOL))
                .flex_shrink_0()
                .rounded(px(5.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(theme.foreground.opacity(0.07)))
                .on_click(cx.listener(|this, _, window, cx| this.toggle_replace_mode(window, cx)))
                .managed_tooltip(|window, cx| crate::tooltip::label("Replace (⌥⌘F)", window, cx))
                .child(
                    icon.with_size(px(14.))
                        .text_color(theme.muted_foreground)
                        .with_animation(
                            ElementId::Name(format!("search-chevron-{row_serial}").into()),
                            Animation::new(SEARCH_ANIM).with_easing(ease_out_cubic),
                            move |icon, t| icon.rotate(percentage(if still { to } else { from + (to - from) * t })),
                        ),
                )
        });

        let mut toggles = Some(h_flex()
            .gap(px(1.))
            .child(self.tool("search-case", Icon::new(IconName::CaseSensitive), "Match Case (⌥⌘C)", !options.case_insensitive, true, 20., cx, |this, _, cx| this.toggle_case(cx)))
            .child(self.tool("search-word", Icon::default().path("icons/whole-word.svg"), "Whole Word (⌥⌘W)", options.whole_word, true, 20., cx, |this, _, cx| this.toggle_word(cx)))
            .child(self.tool("search-regex", Icon::default().path("icons/regex.svg"), "Regular Expression (⌥⌘R)", options.regex, true, 20., cx, |this, _, cx| this.toggle_regex(cx))));

        let field = div()
            .flex_1()
            .min_w(px(64.))
            .child({
                let input = Input::new(&self.search_input).focus_bordered(false).small().w_full().shadow_none();
                match toggles.take_if(|_| !narrow) {
                    Some(toggles) => input.suffix(toggles),
                    None => input,
                }
            })
            .on_prepaint({
                let view = cx.entity();
                move |bounds, _, cx| view.update(cx, |r, _| r.input_width = bounds.size.width)
            });
        let count = (!count.is_empty()).then(|| {
            div()
                .flex_shrink_0()
                .px(px(2.))
                .text_size(px(11.5))
                .text_color(if bad { theme.danger } else { theme.muted_foreground })
                .child(count)
        });
        let prev = self.tool("search-prev", Icon::new(IconName::ChevronUp), "Previous (⇧↩)", false, has_matches, TOOL, cx, |this, window, cx| this.prev(window, cx));
        let next = self.tool("search-next", Icon::new(IconName::ChevronDown), "Next (↩)", false, has_matches, TOOL, cx, |this, window, cx| this.next(window, cx));
        let close = self.tool("search-close", Icon::new(IconName::Close), "Close (esc)", false, true, TOOL, cx, |this, window, cx| this.hide(window, cx));
        let mut find_row = h_flex().h(px(ROW_H)).gap(px(4.)).items_center().children(chevron).child(field);
        // Narrow, the field has the row with the close button, and what
        // stands beside it in a wide panel goes under it.
        let mut meta_row = None;
        if narrow {
            find_row = find_row.child(close);
            meta_row = Some(
                h_flex()
                    .pt(px(4.))
                    .h(px(ROW_H + 4.))
                    .gap(px(4.))
                    .items_center()
                    .when(allow_replace, |d| d.child(div().w(px(TOOL)).flex_shrink_0()))
                    .children(toggles.take())
                    .child(div().flex_1())
                    .children(count)
                    .child(prev)
                    .child(next),
            );
        } else {
            find_row = find_row.children(count).child(prev).child(next).child(close);
        }

        let replace_row = row_shown.then(|| {
            let row = h_flex()
                .pt(px(4.))
                .h(px(ROW_H + 4.))
                .gap(px(4.))
                .items_center()
                .child(div().w(px(TOOL)).flex_shrink_0())
                .child({
                    // As wide as the find field, so the two line up; narrow,
                    // the field takes what the buttons leave.
                    let input = Input::new(&self.replace_input).focus_bordered(false).small().shadow_none();
                    if narrow { div().flex_1().min_w(px(64.)).child(input.w_full()) } else { div().child(input.w(self.input_width)) }
                })
                .child(self.tool("search-replace-one", Icon::new(IconName::Replace), "Replace (↩)", false, has_matches, TOOL, cx, |this, window, cx| this.replace_next(window, cx)))
                .child(self.tool("search-replace-all", Icon::default().path("icons/replace-all.svg"), "Replace All (⌘↩)", false, has_matches, TOOL, cx, |this, window, cx| this.replace_all(window, cx)));
            div().overflow_hidden().child(row).with_animation(
                ElementId::Name(format!("search-replace-row-{row_serial}").into()),
                Animation::new(SEARCH_ANIM).with_easing(ease_out_cubic),
                move |d, t| {
                    if turned_at_open || (row_on && t >= 1.) {
                        return d;
                    }
                    let k = if row_on { t } else { 1. - t };
                    d.max_h(px((ROW_H + 4.) * k)).opacity(k)
                },
            )
        });

        let card = v_flex()
            .id("search-panel")
            .occlude()
            .track_focus(&self.focus_handle(cx))
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_action_enter))
            .on_action(cx.listener(Self::on_action_escape))
            .on_action(cx.listener(Self::on_action_search))
            .on_action(cx.listener(Self::on_action_tab))
            .on_action(cx.listener(Self::on_action_tab_prev))
            .on_action(cx.listener(Self::on_action_replace))
            .on_action(cx.listener(|this, _: &ToggleMatchCase, _, cx| this.toggle_case(cx)))
            .on_action(cx.listener(|this, _: &ToggleWholeWord, _, cx| this.toggle_word(cx)))
            .on_action(cx.listener(|this, _: &ToggleRegex, _, cx| this.toggle_regex(cx)))
            .font_family(theme.font_family.clone())
            .px(px(6.))
            .py(px(5.))
            .rounded(px(10.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_sm()
            // Its own height whatever the room the motion gives it, so the
            // height measured is the card's.
            .flex_shrink_0()
            .on_prepaint({
                let view = cx.entity();
                move |bounds, _, cx| {
                    view.update(cx, |r, _| {
                        r.width = bounds.size.width;
                        r.height = bounds.size.height;
                    })
                }
            })
            .child(find_row)
            .children(meta_row)
            .children(replace_row);

        // The panel opens and closes over a moment, the text under it
        // moving with it. The motion is over the card's height as
        // measured, and its room around it: a height guessed over the
        // card's left the close standing still before it moved.
        let rows = 1. + if narrow { 1. } else { 0. } + if replace_mode { 1. } else { 0. };
        let guess = 5. * 2. + 2. + ROW_H * rows + 4. * (rows - 1.);
        let most = 6. + 4. + if self.height > Pixels::ZERO { f32::from(self.height) } else { guess };
        let opened = self.session.opened;
        let (key, going) = if open { ("search-panel-in", false) } else { ("search-panel-out", true) };
        // The room around the card is inside what is cut: on the box
        // that closes, padding is the least it shrinks to, and the close
        // stood still at it before the panel went.
        div()
            .w_full()
            .flex_shrink_0()
            .overflow_hidden()
            .child(div().w_full().flex_shrink_0().px(px(8.)).pt(px(6.)).pb(px(4.)).child(card))
            .with_animation(
                ElementId::Name(format!("{key}-{opened}").into()),
                Animation::new(SEARCH_ANIM).with_easing(ease_out_cubic),
                move |d, t| {
                    if !going && t >= 1. {
                        return d;
                    }
                    let k = if going { 1. - t } else { t };
                    d.max_h(px(most * k)).opacity(k)
                },
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    #[test]
    fn test_search() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
        matcher.update_query("Is", true);

        assert_eq!(matcher.len(), 3);
        let mut matches = matcher.clone();
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next(), Some(18..20));
        assert_eq!(matches.next(), Some(23..25));
        assert_eq!(matches.current_match_index(), 2);
        assert_eq!(matches.next(), Some(15..17));
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next_back(), Some(23..25));
        assert_eq!(matches.current_match_index(), 2);
        assert_eq!(matches.next_back(), Some(18..20));
        assert_eq!(matches.current_match_index(), 1);
        assert_eq!(matches.next_back(), Some(15..17));
        assert_eq!(matches.current_match_index(), 0);
        assert_eq!(matches.next_back(), Some(23..25));

        matcher.update_query("IS", false);
        assert_eq!(matcher.len(), 0);
        assert_eq!(matcher.next(), None);
        assert_eq!(matcher.next_back(), None);
    }

    #[test]
    fn test_search_label() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
        matcher.update_query("Is", true);
        assert_eq!(matcher.label(), "1/3");
        matcher.next();
        assert_eq!(matcher.label(), "2/3");
        matcher.next();
        assert_eq!(matcher.label(), "3/3");
        matcher.next();
        assert_eq!(matcher.label(), "1/3");

        matcher.update_query("IS", false);
        assert_eq!(matcher.label(), "0/0");
    }

    #[test]
    fn test_select_range_start() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from(".....aaaaa.....aaaaa.....aaaaa"));
        matcher.update_query("aaaaa", false);
        matcher.update_cursor_by_offset(0);
        assert_eq!(matcher.current_match_index(), 0);

        matcher.update_cursor_by_offset(5);
        assert_eq!(matcher.current_match_index(), 0);

        matcher.update_cursor_by_offset(12);
        assert_eq!(matcher.current_match_index(), 1);

        matcher.update_cursor_by_offset(16);
        assert_eq!(matcher.current_match_index(), 1);

        matcher.update_cursor_by_offset(30);
        assert_eq!(matcher.current_match_index(), 2);

        matcher.update_cursor_by_offset(31);
        assert_eq!(matcher.current_match_index(), 2);
    }

    #[test]
    fn test_next_scroll_direction_returns_down_without_wrap() {
        assert!(matches!(
            next_scroll_direction(0, 1),
            Some(MoveDirection::Down)
        ));
    }

    #[test]
    fn test_next_scroll_direction_returns_none_on_wrap() {
        assert!(next_scroll_direction(2, 0).is_none());
    }

    #[test]
    fn test_next_scroll_direction_returns_none_for_single_match() {
        assert!(next_scroll_direction(0, 0).is_none());
    }

    #[test]
    fn test_prev_scroll_direction_returns_up_without_wrap() {
        assert!(matches!(
            prev_scroll_direction(2, 1),
            Some(MoveDirection::Up)
        ));
    }

    #[test]
    fn test_prev_scroll_direction_returns_none_on_wrap() {
        assert!(prev_scroll_direction(0, 2).is_none());
    }

    #[test]
    fn test_prev_scroll_direction_returns_none_for_single_match() {
        assert!(prev_scroll_direction(0, 0).is_none());
    }
}
