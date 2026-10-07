//! A terminal at the conversation's right, under the top strip, opened
//! with the strip's terminal button. Two things show in it, one at a
//! time: a shell of the person's in the session's folder, and the
//! session's own agent as it runs in the hidden terminal
//! (`emaki_core::pty`), the same screen the window reads and types into.
//!
//! Both are a `Pty`: a child on a pty this process owns and a screen
//! model that draws nothing. Here the screen is drawn, a row a line in
//! the mono face, the keyboard is written to the child, and the pty is
//! kept the size of the panel.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use emaki_core::model::AgentId;
use emaki_core::transcript::SessionRef;
use emaki_core::pty::{self, Pty};

use crate::panels::CONVERSATION_MIN;
use crate::workbench::{pill_button, swallow_click, term_bytes, Page, TabGhost, TermBackTab, TermClear, TermCloseTab, TermNewTab, TermTab, Workbench, TERMINAL_CONTEXT};

/// How wide the panel is to begin with, and the least it is dragged to.
pub(crate) const TERM_W: Pixels = px(520.);
pub(crate) const TERM_MIN: Pixels = px(300.);
/// The strip over the panel's edge that takes the drag.
const TERM_GRIP: Pixels = px(8.);
/// A row's height and the letters' size, as the terminal card draws them.
const LINE_H: f32 = 17.;
const TEXT: f32 = 12.;
/// The room around the screen.
const PAD_X: f32 = 10.;
const PAD_Y: f32 = 8.;
/// How long the panel takes to come and to go.
const TERM_PANEL_ANIM: Duration = Duration::from_millis(200);
/// How often a panel that shows looks for something new on its screen.
const POLL: Duration = Duration::from_millis(33);
/// How long the panel says a selection was copied.
const COPIED_FOR: Duration = Duration::from_millis(1200);
/// The head over a side, as tall as the path bar over the conversation.
const HEAD_H: f32 = 31.;
/// A shell's tab in the head: its height, the most and the least it is
/// wide, and the room between two.
const TAB_H: f32 = 22.;
const TAB_MAX: f32 = 116.;
const TAB_MIN: f32 = 86.;
const TAB_GAP: f32 = 3.;
/// How long a new tab takes to grow in, and one screen to take another's place.
const TAB_ANIM: Duration = Duration::from_millis(180);

/// One shell: a tab in the head and the pty behind it.
pub(crate) struct ShellTab {
    /// Which tab it is, never given twice.
    id: u64,
    /// The number in its name: one more than the largest there was when
    /// it was made, so a closed tab's number is given again.
    n: u64,
    pty: Arc<Pty>,
    born: Instant,
}

/// A session's shells, in the order of their tabs.
#[derive(Default)]
pub(crate) struct Shells {
    tabs: Vec<ShellTab>,
    /// The tab that shows.
    on: u64,
    /// The last `id` given.
    last: u64,
    /// The first tab has been made: an empty row after that is the
    /// person's doing and stays empty.
    begun: bool,
    /// Why the last shell did not start.
    failed: Option<String>,
}

impl Shells {
    fn current(&self) -> Option<&ShellTab> {
        self.tabs.iter().find(|t| t.id == self.on)
    }
}

/// What is selected on the screen: where the press was and where the
/// pointer is, each (row, column), and whether by the letter, the word
/// (a double click) or the row (a triple). A press that never moved
/// selects nothing by the letter.
#[derive(Clone, Copy)]
pub(crate) struct Sel {
    anchor: (u16, u16),
    head: (u16, u16),
    unit: usize,
}

impl Sel {
    /// The columns selected in each row, from and up to, given what the
    /// rows hold.
    fn spans(&self, cells: &[Vec<char>]) -> Vec<Option<(usize, usize)>> {
        let (a, b) = if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) };
        let mut out = vec![None; cells.len()];
        if self.unit <= 1 && a == b {
            return out;
        }
        let len = |r: u16| cells.get(r as usize).map(|c| c.len()).unwrap_or(0);
        let word = |c: char| !c.is_whitespace() && !"{}[]()\"'`".contains(c);
        let (mut from, mut to) = (a.1 as usize, b.1 as usize + 1);
        match self.unit {
            2 => {
                if let Some(row) = cells.get(a.0 as usize) {
                    while from > 0 && from <= row.len() && row.get(from).is_some_and(|c| word(*c)) && word(row[from - 1]) {
                        from -= 1;
                    }
                }
                if let Some(row) = cells.get(b.0 as usize) {
                    while to < row.len() && word(row[to - 1]) && word(row[to]) {
                        to += 1;
                    }
                }
            }
            3 => (from, to) = (0, usize::MAX),
            _ => {}
        }
        for r in a.0..=b.0 {
            let (f, t) = (if r == a.0 { from } else { 0 }, if r == b.0 { to } else { usize::MAX });
            // A row with nothing on it still shows that it is selected.
            let t = t.min(len(r).max(1));
            if let Some(slot) = out.get_mut(r as usize).filter(|_| f < t) {
                *slot = Some((f, t));
            }
        }
        out
    }
}

/// A row as its cells hold it: a character a cell, and a wide one's
/// second cell a NUL.
fn row_cells(row: &[pty::Span]) -> Vec<char> {
    let mut out = Vec::new();
    for c in row.iter().flat_map(|s| s.text.chars()) {
        out.push(c);
        if wide(c) {
            out.push('\0');
        }
    }
    out
}

/// A shell's tab being dragged along the row.
#[derive(Clone)]
struct DragShell(u64);

impl Workbench {
    /// Whether the panel is drawn: asked for, on a conversation.
    pub(crate) fn term_panel_shown(&self) -> bool {
        self.side_term && self.page == Page::Session && self.detail.is_some()
    }

    /// Which side is asked for: the agent's is `true`, the shell's
    /// `false`, none when the panel is away.
    pub(crate) fn term_on(&self) -> Option<bool> {
        self.side_term.then_some(self.side_term_agent)
    }

    /// One of the strip's two terminal buttons was pressed: the side
    /// showing goes, and any other takes the place of the one that was.
    pub(crate) fn toggle_term(&mut self, agent: bool, window: &mut Window, cx: &mut Context<Self>) {
        let to = (self.term_on() != Some(agent)).then_some(agent);
        self.term_go(to, cx);
        window.focus(if to.is_some() { &self.side_term_focus } else { &self.focus_handle }, cx);
    }

    /// The panel goes to that side, or away, and the change is drawn.
    fn term_go(&mut self, to: Option<bool>, cx: &mut Context<Self>) {
        let from = self.term_on();
        if from == to {
            return;
        }
        self.side_term = to.is_some();
        if let Some(agent) = to {
            self.side_term_agent = agent;
        }
        self.side_term_anim = Some((from, to, Instant::now(), self.side_term_anim.map(|(_, _, _, n)| n + 1).unwrap_or(0)));
        self.side_back = 0;
        self.side_sel = None;
        if to.is_some() {
            self.term_panel_watch(cx);
            self.warm_now();
        }
        if from.is_some() {
            // The side left is drawn while it goes: once more when it has.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TERM_PANEL_ANIM + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        self.save_ui(true);
        cx.notify();
    }

    /// "Open in Terminal" on a session's menu: the agent's side comes.
    pub(crate) fn open_term_panel_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.term_go(Some(true), cx);
        window.focus(&self.side_term_focus, cx);
    }

    /// A probe's step (`EMAKI_GO`), which has no window to hand.
    pub(crate) fn term_panel_probe(&mut self, step: &str, cx: &mut Context<Self>) {
        match step {
            "term" => self.term_go(Some(self.side_term_agent), cx),
            "term:shell" => self.term_go(Some(false), cx),
            "term:agent" => self.term_go(Some(true), cx),
            "term:off" => self.term_go(None, cx),
            "term:start" => {
                self.term_go(Some(true), cx);
                self.start_hidden_terminal(cx);
            }
            "term:tab+" => self.shell_new(None, cx),
            "term:fit" => self.side_term_w = TERM_W,
            "term:clear" => self.term_clear(),
            _ if step.starts_with("term:sel:") => {
                // term:sel:<unit>,<row>,<col>,<row>,<col>
                let n: Vec<u16> = step["term:sel:".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
                if let [unit, r1, c1, r2, c2] = n[..] {
                    self.side_sel = Some(Sel { anchor: (r1, c1), head: (r2, c2), unit: unit as usize });
                    eprintln!("emaki: selected {:?}", self.term_sel_text());
                }
            }
            _ if step.starts_with("term:w:") => self.side_term_w = px(step["term:w:".len()..].parse().unwrap_or(520.)),
            "term:tab-" => {
                if let Some(id) = self.selected_ref().and_then(|r| self.shells.get(&r.session_id).map(|s| s.on)) {
                    self.shell_close(id, cx);
                }
            }
            _ if step.starts_with("term:tab:") => {
                let ix = step["term:tab:".len()..].parse::<usize>().unwrap_or(0);
                if let Some(id) = self.selected_ref().and_then(|r| self.shells.get(&r.session_id).and_then(|s| s.tabs.get(ix).map(|t| t.id))) {
                    self.shell_pick(id, cx);
                }
            }
            _ => {
                if let Some(pty) = step.strip_prefix("termtype:").and_then(|_| self.term_panel_pty()) {
                    pty.write(format!("{}\r", &step["termtype:".len()..]).as_bytes());
                }
            }
        }
        cx.notify();
    }

    /// The two buttons at the top strip's right end, as one control, the
    /// way the files and the outline have theirs at its left: a track
    /// with a segment each, the shell's and the agent's, and a raised
    /// plate under the one whose side shows. The plate slides from one to
    /// the other, comes in under the first pressed and fades under the
    /// one pressed again.
    pub(crate) fn term_buttons(&self, agent: AgentId, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let track_bg = if dark { theme.sidebar } else { theme.muted };
        let plate_bg = if dark { theme.secondary_active } else { theme.popover };
        let (seg_w, seg_h, pad, gap) = (30., 24., 3., 2.);
        let x_of = |agent: bool| pad + if agent { seg_w + gap } else { 0. };
        let on = self.term_on();
        // Without a press since launch there is nothing to move from.
        let (from, to, serial) = self.side_term_anim.map(|(from, to, _, n)| (from, to, n + 1)).unwrap_or((on, on, 0));
        let plate = (from.is_some() || to.is_some()).then(|| {
            let (a, b) = (x_of(from.or(to).unwrap_or(false)), x_of(to.or(from).unwrap_or(false)));
            let (o_a, o_b) = (if from.is_some() { 1. } else { 0. }, if to.is_some() { 1. } else { 0. });
            div().absolute().top(px(pad)).w(px(seg_w)).h(px(seg_h)).rounded(px(7.)).bg(plate_bg).shadow_sm().with_animation(
                ElementId::Name(format!("term-plate-{serial}").into()),
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                move |d, t| d.left(px(a + (b - a) * t)).opacity(o_a + (o_b - o_a) * t),
            )
        });
        // The agent's mark is in the agent's own colour under the pointer
        // and while its side shows.
        let own = crate::workbench::agent_color(agent, &theme);
        let segment = |id: &'static str, icon: &'static str, tip: String, which: bool, cx: &mut Context<Self>| {
            let active = on == Some(which);
            let lit = if which { own } else { theme.foreground };
            h_flex()
                .id(id)
                .w(px(seg_w))
                .h(px(seg_h))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .cursor_pointer()
                .text_color(if active { lit } else { theme.muted_foreground })
                // On the active one too (`panels.rs`, the same control).
                .hover(move |s| s.text_color(lit))
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .on_click(cx.listener(move |this, _, window, cx| this.toggle_term(which, window, cx)))
                .child(Icon::default().path(icon).with_size(px(15.)))
        };
        h_flex()
            .relative()
            .flex_shrink_0()
            .p(px(pad))
            .gap(px(gap))
            .rounded(px(9.))
            .bg(track_bg)
            .children(plate)
            .child(segment("term-shell", "icons/terminal.svg", "Shell".to_string(), false, cx))
            // The agent's side wears the agent's own mark.
            .child(segment("term-agent", crate::workbench::agent_icon_path(agent), format!("{} in a terminal", agent.display_name()), true, cx))
            .into_any_element()
    }

    /// While the panel shows, the window is drawn again whenever the
    /// screen it shows has changed: a pty writes from a thread of its
    /// own and says nothing to the window.
    fn term_panel_watch(&mut self, cx: &mut Context<Self>) {
        self.side_term_poll = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let on = this.update(cx, |this, cx| {
                if !this.side_term {
                    return false;
                }
                if this.term_panel_pty().is_some_and(|p| p.quiet_for() < POLL * 4 || !p.alive()) {
                    cx.notify();
                }
                true
            });
            if !on.unwrap_or(false) {
                break;
            }
        }));
    }

    /// The pty the panel shows now, when there is one.
    fn term_panel_pty(&self) -> Option<Arc<Pty>> {
        self.term_pty_of(self.side_term_agent)
    }

    /// The pty of one side, when there is one.
    fn term_pty_of(&self, agent: bool) -> Option<Arc<Pty>> {
        let r = self.selected_ref()?;
        if agent {
            self.hub.terminal_for(&r.session_id)
        } else {
            self.shells.get(&r.session_id).and_then(|s| s.current()).map(|t| t.pty.clone())
        }
    }

    /// A new shell for the session, in a tab of its own that then shows:
    /// the person's login shell in the session's folder (their home when
    /// the folder is gone), with no `CLAUDE*` variable of ours in it.
    /// `at` is where in the row, the end when none.
    pub(crate) fn shell_new(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        let Some(r) = self.selected_ref() else { return };
        let cwd = if Self::folder_exists(&r) { r.cwd.clone() } else { emaki_core::paths::home().to_string_lossy().to_string() };
        let made = Pty::spawn(&pty::shell_argv(), &cwd, Arc::new(|| {}));
        let set = self.shells.entry(r.session_id.clone()).or_default();
        set.begun = true;
        match made {
            Ok(pty) => {
                set.failed = None;
                set.last += 1;
                let (id, from) = (set.last, set.on);
                let n = set.tabs.iter().map(|t| t.n).max().unwrap_or(0) + 1;
                let at = at.unwrap_or(set.tabs.len()).min(set.tabs.len());
                set.tabs.insert(at, ShellTab { id, n, pty, born: Instant::now() });
                set.on = id;
                self.shell_swapped(from, cx);
            }
            Err(e) => set.failed = Some(e),
        }
        cx.notify();
    }

    /// Another tab shows.
    fn shell_pick(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(sid) = self.selected_ref().map(|r| r.session_id.clone()) else { return };
        let Some(set) = self.shells.get_mut(&sid) else { return };
        if set.on == id || !set.tabs.iter().any(|t| t.id == id) {
            return;
        }
        let from = std::mem::replace(&mut set.on, id);
        self.shell_swapped(from, cx);
        cx.notify();
    }

    /// A tab is closed and its shell let go. The one after it shows in
    /// its place, the one before when it was the last.
    fn shell_close(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(sid) = self.selected_ref().map(|r| r.session_id.clone()) else { return };
        let Some(set) = self.shells.get_mut(&sid) else { return };
        let Some(ix) = set.tabs.iter().position(|t| t.id == id) else { return };
        set.tabs.remove(ix).pty.kill();
        if set.on == id {
            set.on = set.tabs.get(ix).or(set.tabs.last()).map(|t| t.id).unwrap_or(0);
            self.shell_swapped(id, cx);
        }
        cx.notify();
    }

    /// The screen that shows changed: the one before goes as the new one comes.
    /// The tab that shows is brought into view in its row, and kept there
    /// while it grows in.
    fn shell_swapped(&mut self, from: u64, cx: &mut Context<Self>) {
        self.shell_swap = Some((from, Instant::now(), self.shell_swap.map(|(_, _, n)| n + 1).unwrap_or(0)));
        let until = TAB_ANIM + Duration::from_millis(80);
        self.shell_reveal = Some(Instant::now() + until);
        self.side_sel = None;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(until).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        self.side_back = 0;
        self.side_wheel = 0.;
    }

    /// A tab dragged along the row takes the place the pointer is over.
    fn shell_drag_to(&mut self, id: u64, x: Pixels, row: Bounds<Pixels>, tab_w: f32, cx: &mut Context<Self>) {
        if self.shell_drag != Some(id) {
            self.shell_drag = Some(id);
            cx.notify();
        }
        let Some(sid) = self.selected_ref().map(|r| r.session_id.clone()) else { return };
        let Some(set) = self.shells.get_mut(&sid) else { return };
        let Some(from) = set.tabs.iter().position(|t| t.id == id) else { return };
        let to = ((f32::from(x - row.left() - self.shell_scroll.offset().x) / (tab_w + TAB_GAP)).floor().max(0.) as usize).min(set.tabs.len() - 1);
        if to != from {
            let tab = set.tabs.remove(from);
            set.tabs.insert(to, tab);
            cx.notify();
        }
    }

    /// The shell's head: a tab a shell, then the button that makes one.
    /// A tab is pressed to show it, dragged to move it, and closed with
    /// its own button. `room` is what the row may take: tabs share it
    /// down to `TAB_MIN` each, and past that the row scrolls with the
    /// button held at its right end.
    fn render_shell_tabs(&mut self, r: &SessionRef, room: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if !cx.has_active_drag() {
            self.shell_drag = None;
        }
        let name = pty::shell_argv().first().and_then(|s| std::path::Path::new(s).file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "shell".into());
        let empty = Shells::default();
        let set = self.shells.get(&r.session_id).unwrap_or(&empty);
        let n = set.tabs.len().max(1) as f32;
        let tab_w = ((room - TAB_H - TAB_GAP * n) / n).clamp(TAB_MIN, TAB_MAX);
        if self.shell_reveal.is_some_and(|until| Instant::now() < until) {
            if let Some(ix) = set.tabs.iter().position(|t| t.id == set.on) {
                self.shell_scroll.scroll_to_item(ix);
            }
        } else {
            self.shell_reveal = None;
        }
        let mut row = h_flex().id("shell-tabs").min_w_0().h_full().items_center().gap(px(TAB_GAP)).overflow_x_scroll().track_scroll(&self.shell_scroll).on_drag_move::<DragShell>(cx.listener(move |this, e: &DragMoveEvent<DragShell>, _, cx| {
            let id = e.drag(cx).0;
            this.shell_drag_to(id, e.event.position.x, e.bounds, tab_w, cx);
        }));
        for tab in &set.tabs {
            let id = tab.id;
            let active = set.on == id;
            let label: SharedString = format!("{name} {}", tab.n).into();
            let ghost = label.to_string();
            let group: SharedString = format!("shell-tab-{id}").into();
            let fresh = tab.born.elapsed() < TAB_ANIM;
            let (hover_bg, press_bg, close_bg) = (theme.muted, theme.border, theme.border);
            let el = h_flex()
                .id(("shell-tab", id as usize))
                .group(group.clone())
                .w(px(tab_w))
                .h(px(TAB_H))
                .pl(px(7.))
                .pr(px(3.))
                .gap(px(5.))
                .items_center()
                .rounded(px(6.))
                .border_1()
                .cursor_pointer()
                .overflow_hidden()
                .text_size(px(11.5))
                .map(|d| {
                    if active {
                        d.bg(theme.background).border_color(theme.border).shadow_xs().text_color(theme.foreground).font_weight(FontWeight::MEDIUM)
                    } else {
                        d.border_color(gpui::transparent_black()).text_color(theme.muted_foreground).hover(move |s| s.bg(hover_bg)).active(move |s| s.bg(press_bg))
                    }
                })
                .when(self.shell_drag == Some(id), |d| d.opacity(0.45))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .on_drag(DragShell(id), move |_, _, _, cx| cx.new(|_| TabGhost(ghost.clone())))
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.shell_pick(id, cx);
                    window.focus(&this.side_term_focus, cx);
                }))
                .child(Icon::default().path("icons/terminal.svg").with_size(px(12.)).flex_shrink_0())
                .child(div().flex_1().min_w_0().truncate().child(label))
                .child(
                    div()
                        .id(("shell-tab-close", id as usize))
                        .size(px(16.))
                        .flex_shrink_0()
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(move |s| s.bg(close_bg))
                        // On the tab that shows, and on any under the pointer.
                        .when(!active, |d| d.invisible().group_hover(group, |s| s.visible()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.shell_close(id, cx);
                        }))
                        .child(Icon::new(IconName::Close).with_size(px(10.)).text_color(theme.muted_foreground)),
                );
            // A new tab grows in from nothing. The wrapper is there at
            // rest too, under the same name.
            row = row.child(div().flex_shrink_0().overflow_hidden().child(el).with_animation(ElementId::Name(format!("shell-tab-w-{id}").into()), Animation::new(TAB_ANIM).with_easing(ease_out_quint()), move |d, t| {
                if fresh { d.w(px(tab_w * t)).opacity(t) } else { d }
            }));
        }
        let (hover_bg, press_bg) = (theme.muted, theme.border);
        let plus = div()
            .id("shell-tab-new")
            .size(px(TAB_H))
            .flex_shrink_0()
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(theme.muted_foreground)
            .hover(move |s| s.bg(hover_bg).text_color(theme.foreground))
            .active(move |s| s.bg(press_bg))
            .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("New shell").build(window, cx))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.shell_new(None, cx);
                window.focus(&this.side_term_focus, cx);
            }))
            .child(Icon::default().path("icons/plus.svg").with_size(px(12.)));
        h_flex().min_w_0().h_full().items_center().gap(px(TAB_GAP)).child(row).child(plus)
        .into_any_element()
    }

    /// The panel's width as drawn: what it was dragged to, less when the
    /// conversation would be left with too little.
    fn term_panel_w(&self) -> Pixels {
        let (files, outline) = self.panels_shown();
        let beside = if files || outline { self.panel_w_now() } else { px(0.) };
        self.side_term_w.min(self.pane_w - beside - CONVERSATION_MIN).max(TERM_MIN)
    }

    /// The pointer moved with the panel's edge held.
    pub(crate) fn term_drag_to(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            if std::mem::take(&mut self.side_term_drag) {
                self.save_ui(true);
            }
            return;
        }
        let (files, outline) = self.panels_shown();
        let beside = if files || outline { self.panel_w_now() } else { px(0.) };
        let w = (self.view_w - e.position.x).clamp(TERM_MIN, (self.pane_w - beside - CONVERSATION_MIN).max(TERM_MIN));
        if w != self.side_term_w {
            self.side_term_w = w;
            cx.notify();
        }
    }

    /// The strip over the panel's left edge, lit under the pointer.
    pub(crate) fn render_term_grip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.term_panel_shown() {
            return None;
        }
        let line = cx.theme().primary.opacity(0.55);
        let held = self.side_term_drag;
        Some(
            div()
                .id("term-grip")
                .group("term-grip")
                .absolute()
                .top(crate::workbench::TITLEBAR_H)
                .bottom_0()
                .right(self.term_panel_w() - TERM_GRIP / 2.)
                .w(TERM_GRIP)
                .occlude()
                .cursor(CursorStyle::ResizeLeftRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                        // A double click puts the edge back where it began.
                        if ev.click_count == 2 {
                            this.side_term_drag = false;
                            this.side_term_w = TERM_W;
                            this.save_ui(true);
                        } else {
                            this.side_term_drag = true;
                        }
                        swallow_click(window, cx);
                        cx.notify();
                    }),
                )
                .child(div().absolute().top_0().h_full().left(TERM_GRIP / 2. - px(1.5)).w(px(2.)).when(held, |d| d.bg(line)).group_hover("term-grip", move |s| s.bg(line))),
        )
    }

    /// The wheel over the panel: back through what has left the screen,
    /// or, where a program has the screen to itself, its arrow keys.
    pub(crate) fn term_panel_wheel(&mut self, delta_y: Pixels, at: Point<Pixels>, mods: &Modifiers) {
        let Some(pty) = self.term_panel_pty() else { return };
        self.side_wheel += f32::from(delta_y) / LINE_H;
        let lines = self.side_wheel.trunc() as i64;
        if lines == 0 {
            return;
        }
        self.side_wheel -= lines as f32;
        // A program that hears the mouse hears the wheel as its own.
        if let Some(want) = pty.mouse().filter(|_| self.side_back == 0) {
            let (col, row) = self.term_cell(at, &pty);
            let code = if lines > 0 { 64 } else { 65 } | mod_bits(mods);
            pty.write(&pty::mouse_bytes(code, col, row, true, want.form).repeat(lines.unsigned_abs().min(12) as usize));
            return;
        }
        if pty.alt_screen() {
            let key: &[u8] = match (lines > 0, pty.app_cursor()) {
                (true, false) => b"\x1b[A",
                (true, true) => b"\x1bOA",
                (false, false) => b"\x1b[B",
                (false, true) => b"\x1bOB",
            };
            pty.write(&key.repeat(lines.unsigned_abs() as usize));
        } else {
            self.side_back = (self.side_back as i64 + lines).max(0) as usize;
            self.side_sel = None;
        }
    }

    /// The cell of the screen a point of the window is over: column, row,
    /// held inside the screen.
    fn term_cell(&self, at: Point<Pixels>, pty: &Pty) -> (u16, u16) {
        let b = self.side_term_bounds.get();
        let (rows, cols) = pty.size();
        let col = (f32::from(at.x - b.left()) - PAD_X) / self.side_cell_w.max(1.);
        let row = (f32::from(at.y - b.top()) - PAD_Y) / LINE_H;
        (col.floor().clamp(0., cols.saturating_sub(1) as f32) as u16, row.floor().clamp(0., rows.saturating_sub(1) as f32) as u16)
    }

    /// A button pressed in the panel. A program that asked to hear the
    /// mouse is told, unless Shift is held, which keeps the press for
    /// the panel as terminals do. Otherwise the left button begins a
    /// selection (by the letter, and by the word or the row on a double
    /// or triple click), opens the link under it with ⌘, and with ⌥ at
    /// a prompt takes the cursor to the column pressed.
    fn term_mouse_down(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(pty) = self.term_panel_pty() else { return };
        let (col, row) = self.term_cell(e.position, &pty);
        let want = pty.mouse().filter(|_| pty.alive() && self.side_back == 0);
        if let (Some(want), false, Some(button)) = (want, e.modifiers.shift, button_code(e.button)) {
            self.side_sel = None;
            self.side_mouse = Some(button);
            self.side_mouse_cell = (col, row);
            pty.write(&pty::mouse_bytes(button | mod_bits(&e.modifiers), col, row, true, want.form));
            return;
        }
        if e.button != MouseButton::Left {
            return;
        }
        if e.modifiers.platform {
            let (rows, _) = pty.rows_back(self.side_back);
            if let Some(url) = rows.get(row as usize).and_then(|r| link_at(&row_cells(r), col as usize)) {
                cx.open_url(&url);
            }
            return;
        }
        if e.modifiers.alt && want.is_none() && self.side_back == 0 {
            if let Some((at_row, at_col)) = pty.cursor().filter(|(r, _)| *r == row) {
                let _ = at_row;
                let key: &[u8] = match (col > at_col, pty.app_cursor()) {
                    (true, false) => b"\x1b[C",
                    (true, true) => b"\x1bOC",
                    (false, false) => b"\x1b[D",
                    (false, true) => b"\x1bOD",
                };
                pty.write(&key.repeat(col.abs_diff(at_col) as usize));
            }
            return;
        }
        self.side_sel = Some(Sel { anchor: (row, col), head: (row, col), unit: e.click_count.clamp(1, 3) });
        self.side_selecting = true;
    }

    /// The button let go, in the panel or outside it. A selection just
    /// made is copied, as it is in a terminal.
    fn term_mouse_up(&mut self, e: &MouseUpEvent, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.side_selecting) {
            if !self.term_copy(cx) {
                self.side_sel = None;
            }
            cx.notify();
            return;
        }
        let Some(button) = self.side_mouse.take() else { return };
        let Some(pty) = self.term_panel_pty() else { return };
        if let Some(want) = pty.mouse().filter(|w| w.release) {
            let (col, row) = self.term_cell(e.position, &pty);
            pty.write(&pty::mouse_bytes(button | mod_bits(&e.modifiers), col, row, false, want.form));
        }
    }

    /// The pointer moved over the panel: the selection follows it, or it
    /// is told to the program a cell at a time, with a button held, or
    /// without one where the program asked for that too.
    fn term_mouse_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button.is_none() {
            self.side_mouse = None;
            self.side_selecting = false;
        }
        let Some(pty) = self.term_panel_pty() else { return };
        if self.side_selecting {
            let (col, row) = self.term_cell(e.position, &pty);
            if let Some(sel) = self.side_sel.as_mut().filter(|s| s.head != (row, col)) {
                sel.head = (row, col);
                cx.notify();
            }
            return;
        }
        let Some(want) = pty.mouse().filter(|_| pty.alive() && self.side_back == 0) else { return };
        let held = self.side_mouse;
        if !(want.motion || want.drag && held.is_some()) {
            return;
        }
        let cell = self.term_cell(e.position, &pty);
        if cell == self.side_mouse_cell {
            return;
        }
        self.side_mouse_cell = cell;
        // No button held is button 3 on the move.
        pty.write(&pty::mouse_bytes(held.unwrap_or(3) | 32 | mod_bits(&e.modifiers), cell.0, cell.1, true, want.form));
    }

    /// The words selected: each row's stretch without the blank at its
    /// end, a new line between rows unless one runs on into the next.
    fn term_sel_text(&self) -> String {
        let (Some(sel), Some(pty)) = (self.side_sel, self.term_panel_pty()) else { return String::new() };
        let (rows, _) = pty.rows_back(self.side_back);
        let wraps = pty.wraps_back(self.side_back);
        let cells: Vec<Vec<char>> = rows.iter().map(|r| row_cells(r)).collect();
        let spans = sel.spans(&cells);
        let last = spans.iter().rposition(|s| s.is_some());
        let mut out = String::new();
        for (r, span) in spans.iter().enumerate() {
            let Some((from, to)) = *span else { continue };
            let piece: String = cells[r].iter().skip(from).take(to.saturating_sub(from)).filter(|c| **c != '\0').collect();
            let runs_on = wraps.get(r).copied().unwrap_or(false) && Some(r) != last;
            out.push_str(if runs_on { &piece } else { piece.trim_end() });
            if Some(r) != last && !runs_on {
                out.push('\n');
            }
        }
        out
    }

    /// The selection goes to the clipboard, and the panel says so for a
    /// moment. Whether there was anything to copy.
    fn term_copy(&mut self, cx: &mut Context<Self>) -> bool {
        let text = self.term_sel_text();
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.side_copied = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR + Duration::from_millis(30)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        true
    }

    /// ⌘K: the screen and what has left it are forgotten, and the
    /// program is asked to draw again (^L), which a shell answers with a
    /// fresh prompt.
    fn term_clear(&mut self) {
        let Some(pty) = self.term_panel_pty() else { return };
        if pty.clear_all() {
            pty.write(b"\x0c");
            self.side_back = 0;
            self.side_sel = None;
        }
    }

    /// The shell tab beside the one that shows, round the row's end.
    fn shell_step(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(set) = self.selected_ref().and_then(|r| self.shells.get(&r.session_id)) else { return };
        let Some(ix) = set.tabs.iter().position(|t| t.id == set.on) else { return };
        let n = set.tabs.len() as isize;
        let id = set.tabs[(ix as isize + by).rem_euclid(n) as usize].id;
        self.shell_pick(id, cx);
    }

    /// Where the panel's screen is in the window, for the wheel.
    pub(crate) fn term_panel_bounds(&self) -> Option<Bounds<Pixels>> {
        self.term_panel_shown().then(|| self.side_term_bounds.get())
    }

    /// A key in the panel, as the bytes a terminal sends for it. ⌘V is a
    /// paste; any other key with ⌘ is the window's.
    fn term_panel_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(pty) = self.term_panel_pty().filter(|p| p.alive()) else { return };
        let k = &e.keystroke;
        if k.modifiers.platform {
            // ⌘ with the keys of a line is the line's start, its end, all
            // of it before the cursor, and a new line that sends nothing,
            // as a Mac's terminals have them; ⌘⇧[ and ⌘⇧] are the shell
            // tab before and after.
            let line: Option<&[u8]> = match k.key.as_str() {
                "left" => Some(b"\x01"),
                "right" => Some(b"\x05"),
                "backspace" => Some(b"\x15"),
                "enter" => Some(b"\x1b\r"),
                _ => None,
            };
            let handled = match k.key.as_str() {
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        pty.paste(&text);
                        self.side_back = 0;
                        self.side_sel = None;
                    }
                    true
                }
                "c" => self.term_copy(cx),
                "[" | "{" if k.modifiers.shift && !self.side_term_agent => {
                    self.shell_step(-1, cx);
                    true
                }
                "]" | "}" if k.modifiers.shift && !self.side_term_agent => {
                    self.shell_step(1, cx);
                    true
                }
                _ => match line {
                    Some(bytes) => {
                        pty.write(bytes);
                        self.side_back = 0;
                        self.side_sel = None;
                        true
                    }
                    None => false,
                },
            };
            if handled {
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        // ⌥ with an arrow or Backspace is a word, as a Mac's terminals
        // have it.
        let word: Option<&[u8]> = match (k.modifiers.alt, k.key.as_str()) {
            (true, "left") => Some(b"\x1bb"),
            (true, "right") => Some(b"\x1bf"),
            (true, "backspace") => Some(b"\x1b\x7f"),
            // ⇧↩ is a new line that sends nothing.
            (false, "enter") if k.modifiers.shift => Some(b"\x1b\r"),
            _ => None,
        };
        let Some(mut bytes) = word.map(<[u8]>::to_vec).or_else(|| term_bytes(k)) else { return };
        if pty.app_cursor() && bytes.len() == 3 && bytes.starts_with(b"\x1b[") && matches!(bytes[2], b'A'..=b'D' | b'H' | b'F') {
            bytes[1] = b'O';
        }
        pty.write(&bytes);
        self.side_back = 0;
        self.side_sel = None;
        cx.stop_propagation();
        cx.notify();
    }

    /// The agent's hidden terminal is kept the size of the panel while
    /// the panel shows it, and its own size otherwise: the terminal card
    /// and the screen's readers were made for that one.
    pub(crate) fn fit_agent_pty(&self, session_id: &str) {
        let Some(pty) = self.hub.terminal_for(session_id) else { return };
        let want = if self.term_panel_shown() && self.side_term_agent { self.side_term_size.get() } else { (pty::ROWS, pty::COLS) };
        if want.0 > 0 && want.1 > 0 && pty.size() != want {
            pty.resize(want.0, want.1);
        }
    }

    /// The panel as it is drawn: the side that shows, and for a moment
    /// the side that was showing. Beside nothing a side widens from the
    /// window's edge and fades in, and put away it narrows and fades out;
    /// in the other's place it fades in while the other fades out over
    /// it, as the files and the outline do. The wrapper is there at rest
    /// too, under the same name (`docs/panels.md`).
    pub(crate) fn render_term_panel(&mut self, r: &SessionRef, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.page != Page::Session || self.detail.is_none() {
            return Vec::new();
        }
        let live = self.side_term_anim.filter(|(_, _, at, _)| at.elapsed() < TERM_PANEL_ANIM);
        let serial = self.side_term_anim.map(|(_, _, _, n)| n + 1).unwrap_or(0);
        let w = self.term_panel_w();
        let mut out = Vec::new();
        let going = live.and_then(|(from, to, _, _)| from.filter(|f| to != Some(*f)).map(|f| (f, to.is_some())));
        let coming = self.term_on();
        for (agent, ghost, over) in going.map(|(f, over)| (f, true, over)).into_iter().chain(coming.map(|a| (a, false, false))) {
            let panel = self.render_term_side(agent, ghost, r, window, cx);
            // (widens or narrows, comes or goes)
            let play = match live {
                Some((from, _, _, _)) if !ghost => Some((from.is_none(), true)),
                Some((_, to, _, _)) if ghost => Some((to.is_none(), false)),
                _ => None,
            };
            out.push(
                div()
                    .h_full()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .when(over, |d| d.absolute().top_0().right_0())
                    .child(panel)
                    .with_animation(ElementId::Name(format!("term-panel-{agent}-{serial}").into()), Animation::new(TERM_PANEL_ANIM).with_easing(ease_out_quint()), move |d, t| match play {
                        Some((wide, comes)) => {
                            let t = if comes { t } else { 1. - t };
                            d.w(if wide { (w * t).round() } else { w }).opacity(t)
                        }
                        None => d,
                    })
                    .into_any_element(),
            );
        }
        out
    }

    /// One side of the panel: its head, saying what it is, with a close
    /// button, over the screen. `leaving` is the side on its way out,
    /// drawn as it was: nothing is started, sized or measured for it.
    fn render_term_side(&mut self, agent: bool, leaving: bool, r: &SessionRef, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let w = self.term_panel_w();

        // The screen's size in cells, from the room it was last given.
        let cell_w = {
            let font = font(theme.mono_font_family.clone());
            let probe = "MMMMMMMMMM";
            let run = TextRun { len: probe.len(), font, color: gpui::black(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(probe.into(), px(TEXT), &[run], None).width) / probe.len() as f32
        };
        let room = self.side_term_bounds.get().size;
        let size = (((f32::from(room.height) - 2. * PAD_Y) / LINE_H).floor().max(4.) as u16, ((f32::from(room.width) - 2. * PAD_X) / cell_w.max(1.)).floor().max(20.) as u16);
        if room.height > px(0.) && !leaving {
            self.side_term_size.set(size);
            self.side_cell_w = cell_w;
        }

        // What shows: the shell, started when first looked at, or the
        // agent's hidden terminal when it has one.
        let shown: Result<Arc<Pty>, AnyElement> = if leaving {
            self.term_pty_of(agent).ok_or_else(|| div().into_any_element())
        } else if agent {
            // Looked at with none behind the session, the agent is
            // started, once: one that ends by itself straight away is not
            // started over and over, and the button is there for that.
            if self.hub.terminal_for(&r.session_id).is_none() && self.agent_startable(r) && self.side_term_tried.insert(r.session_id.clone()) {
                self.start_hidden_terminal(cx);
            }
            self.hub.terminal_for(&r.session_id).ok_or_else(|| self.render_agent_absent(r, cx))
        } else {
            // The first tab is made when the side is first looked at.
            if !self.shells.get(&r.session_id).is_some_and(|s| s.begun) {
                self.shell_new(None, cx);
                self.shell_swap = None;
            }
            self.term_pty_of(false).ok_or_else(|| {
                let failed = self.shells.get(&r.session_id).and_then(|s| s.failed.clone());
                v_flex()
                    .absolute()
                    .inset_0()
                    .items_center()
                    .justify_center()
                    .px(px(28.))
                    .text_size(px(12.5))
                    .text_center()
                    .map(|d| match failed {
                        Some(e) => d.text_color(theme.danger).child(format!("Could not start a shell: {e}")),
                        None => d.text_color(theme.muted_foreground).child("No shell open. Press + for a new one."),
                    })
                    .into_any_element()
            })
        };
        // The shell tab just left, drawn once more as it goes.
        let swap = self.shell_swap.filter(|(_, at, _)| !agent && at.elapsed() < TAB_ANIM);
        let swap_serial = if agent { 0 } else { self.shell_swap.map(|(_, _, n)| n + 1).unwrap_or(0) };
        let was = swap.filter(|_| !leaving).and_then(|(from, _, _)| self.shells.get(&r.session_id)?.tabs.iter().find(|t| t.id == from).map(|t| t.pty.clone()));
        if let Ok(pty) = &shown {
            if !agent && !leaving && room.height > px(0.) && pty.size() != size {
                pty.resize(size.0, size.1);
            }
        }

        // The ground is the agent's own for its screen, whose colours
        // were chosen for it, and the window's for a shell.
        let light = if agent { crate::workbench::claude_theme_light() } else { !theme.mode.is_dark() };
        let (ground, ink) = grounds(light);
        let focused = self.side_term_focus.is_focused(window);
        // With no screen to show, the words stand on the window's own ground.
        let panel_bg = if shown.is_ok() { hsla(ground) } else { theme.background };
        let body: AnyElement = match shown {
            Err(el) => el,
            Ok(pty) => {
                let (rows, back) = pty.rows_back(self.side_back);
                self.side_back = back;
                let picked = match self.side_sel {
                    Some(sel) if !leaving => sel.spans(&rows.iter().map(|r| row_cells(r)).collect::<Vec<_>>()),
                    _ => Vec::new(),
                };
                let lines = screen_lines(&rows, cell_w, light, agent, &picked, theme.primary.opacity(0.3));
                let copied = self.side_copied.filter(|at| at.elapsed() < COPIED_FOR && !leaving).map(|_| {
                    div()
                        .absolute()
                        .right(px(12.))
                        .bottom(px(12.))
                        .h(px(22.))
                        .px(px(9.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .bg(theme.primary)
                        .text_color(theme.primary_foreground)
                        .font_family(theme.font_family.clone())
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Copied")
                        .with_animation("term-copied", Animation::new(COPIED_FOR), |d, t| d.opacity(if t < 0.08 { t / 0.08 } else if t > 0.75 { (1. - t) / 0.25 } else { 1. }))
                });
                // The cursor, where the next letter goes: a line in the
                // accent while the panel has the keyboard, the outline
                // of a block otherwise.
                let cursor = pty.cursor().filter(|_| back == 0 && pty.alive()).map(|(row, col)| {
                    div()
                        .absolute()
                        .top(px(PAD_Y + row as f32 * LINE_H))
                        .left(px(PAD_X + col as f32 * cell_w))
                        .h(px(LINE_H))
                        .map(|d| if focused { d.w(px(2.)).bg(theme.primary) } else { d.w(px(cell_w)).border_1().border_color(hsla(ink).opacity(0.55)) })
                });
                // One that has run a while may be started by itself again.
                if agent && pty.age() > Duration::from_secs(30) {
                    self.side_term_tried.remove(&r.session_id);
                }
                // Until the agent has drawn anything, say what is happening.
                let starting = (agent && pty.alive() && rows.iter().all(|row| row.iter().all(|s| s.text.trim().is_empty()))).then(|| {
                    h_flex()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .gap(px(8.))
                        .font_family(theme.font_family.clone())
                        .text_size(px(12.5))
                        .text_color(hsla(mix(ink, ground, 0.35)))
                        .child(crate::workbench::agent_glyph(r.agent, px(13.), hsla(mix(ink, ground, 0.35)), true, "term-panel-starting"))
                        .child(format!("Starting {}, one moment…", r.agent.display_name()))
                });
                let ended = (!pty.alive()).then(|| {
                    let sid = r.session_id.clone();
                    h_flex()
                        .absolute()
                        .bottom(px(12.))
                        .left_0()
                        .right_0()
                        .justify_center()
                        .gap(px(10.))
                        .items_center()
                        .text_size(px(12.))
                        .text_color(hsla(mix(ink, ground, 0.4)))
                        .child(if agent { "The agent's terminal has ended." } else { "The shell has ended." })
                        .when(!agent, |d| {
                            d.child(pill_button("term-again", "New shell", &theme, cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                // A new one in the ended one's tab's place.
                                if let Some((ix, id)) = this.shells.get(&sid).and_then(|s| s.tabs.iter().position(|t| t.id == s.on).map(|ix| (ix, s.on))) {
                                    this.shell_close(id, cx);
                                    this.shell_new(Some(ix), cx);
                                }
                                window.focus(&this.side_term_focus, cx);
                            })))
                        })
                });
                let mono = theme.mono_font_family.clone();
                let screen = move |lines: Vec<AnyElement>| div().absolute().inset_0().overflow_hidden().px(px(PAD_X)).py(px(PAD_Y)).font_family(mono.clone()).text_size(px(TEXT)).text_color(hsla(ink)).children(lines);
                let live = swap.is_some();
                div()
                    .absolute()
                    .inset_0()
                    .overflow_hidden()
                    // One shell tab in another's place: it fades in while
                    // the other fades out over it.
                    .child(screen(lines).children(cursor.filter(|_| starting.is_none())).with_animation(
                        ElementId::Name(format!("term-screen-{swap_serial}").into()),
                        Animation::new(TAB_ANIM).with_easing(ease_out_quint()),
                        move |d, t| if live { d.opacity(t) } else { d },
                    ))
                    .children(was.map(|old| {
                        screen(screen_lines(&old.rows(), cell_w, light, false, &[], gpui::transparent_black())).with_animation(
                            ElementId::Name(format!("term-screen-was-{swap_serial}").into()),
                            Animation::new(TAB_ANIM).with_easing(ease_out_quint()),
                            |d, t| d.opacity(1. - t),
                        )
                    }))
                    .children(starting)
                    .children(ended)
                    .children(copied)
                    .into_any_element()
            }
        };

        let back = (self.side_back > 0).then(|| div().flex_shrink_0().text_size(px(11.)).text_color(theme.muted_foreground).child(format!("{} lines back", self.side_back)));
        // The agent's head says whose screen it is; the shell's is its tabs.
        let head_row: AnyElement = if agent {
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(Icon::default().path(crate::workbench::agent_icon_path(r.agent)).with_size(px(13.)).text_color(crate::workbench::agent_color(r.agent, &theme)))
                .child(div().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(r.agent.display_name()))
                .into_any_element()
        } else {
            self.render_shell_tabs(r, f32::from(w) - 2. * 8. - if back.is_some() { 96. } else { 0. }, cx)
        };
        let bounds = self.side_term_bounds.clone();
        let entity = cx.entity().downgrade();
        let panel = v_flex()
            .w(w)
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(
                // The path bar's own height, ground and rules, so the two
                // heads read as one line across the window.
                h_flex()
                    .h(px(HEAD_H))
                    .flex_shrink_0()
                    .px(px(if agent { 12. } else { 8. }))
                    .gap(px(8.))
                    .items_center()
                    .overflow_hidden()
                    .bg(theme.muted.opacity(0.3))
                    .border_t_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(head_row)
                    .child(div().flex_1())
                    .children(back),
            )
            .child(
                div()
                    .id(if agent { "term-panel-agent" } else { "term-panel-shell" })
                    .key_context(TERMINAL_CONTEXT)
                    .when(!leaving, |d| d.track_focus(&self.side_term_focus))
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .bg(panel_bg)
                    .cursor(CursorStyle::IBeam)
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.term_panel_key(e, cx)))
                    .on_action(cx.listener(|this, _: &TermTab, _, cx| {
                        if let Some(pty) = this.term_panel_pty() {
                            pty.write(b"\t");
                        }
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &TermBackTab, _, cx| {
                        if let Some(pty) = this.term_panel_pty() {
                            pty.write(b"\x1b[Z");
                        }
                        cx.notify();
                    }))
                    .on_any_mouse_down(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                        window.focus(&this.side_term_focus, cx);
                        this.term_mouse_down(e, cx);
                        swallow_click(window, cx);
                        cx.notify();
                    }))
                    .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.term_mouse_move(e, cx)))
                    .on_action(cx.listener(|this, _: &TermClear, _, cx| {
                        this.term_clear();
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &TermNewTab, window, cx| {
                        if this.side_term_agent {
                            return cx.propagate();
                        }
                        this.shell_new(None, cx);
                        window.focus(&this.side_term_focus, cx);
                    }))
                    .on_action(cx.listener(|this, _: &TermCloseTab, _, cx| {
                        // With no shell tab to close, ⌘W is the window's.
                        match this.selected_ref().and_then(|r| this.shells.get(&r.session_id)).and_then(|s| s.current()).map(|t| t.id).filter(|_| !this.side_term_agent) {
                            Some(id) => this.shell_close(id, cx),
                            None => cx.propagate(),
                        }
                    }))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, _, cx| this.term_mouse_up(e, cx)))
                    .on_mouse_up(MouseButton::Right, cx.listener(|this, e: &MouseUpEvent, _, cx| this.term_mouse_up(e, cx)))
                    .on_mouse_up(MouseButton::Middle, cx.listener(|this, e: &MouseUpEvent, _, cx| this.term_mouse_up(e, cx)))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, _, cx| this.term_mouse_up(e, cx)))
                    // Where the screen is and how large, for the next draw.
                    .when(!leaving, |d| d.child(
                        canvas(
                            move |b, _, cx| {
                                if bounds.get() != b {
                                    bounds.set(b);
                                    if let Some(e) = entity.upgrade() {
                                        cx.defer(move |cx| e.update(cx, |_, cx| cx.notify()));
                                    }
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    ))
                    .child(body),
            );
        panel.into_any_element()
    }


    /// The agent's side with no hidden terminal behind the session: why,
    /// and a button that starts one where that is ours to do.
    /// Whether the agent's hidden terminal can be started for the
    /// session now, by the window itself.
    fn agent_startable(&self, r: &SessionRef) -> bool {
        r.agent == AgentId::ClaudeCode && self.hub.hidden_terminals() && !self.in_own_terminal(r) && self.terminal_ok(r).is_ok()
    }

    fn render_agent_absent(&self, r: &SessionRef, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let name = r.agent.display_name();
        let (line, can): (String, bool) = if r.agent != AgentId::ClaudeCode || !self.hub.hidden_terminals() {
            (format!("{name} runs here only for Claude Code sessions, with the hidden terminal on."), false)
        } else if self.in_own_terminal(r) {
            (format!("A turn is running in a terminal of yours. When it is over, {name} can be started here."), false)
        } else if let Err(why) = self.terminal_ok(r) {
            (why.to_string(), false)
        } else {
            (format!("{name} has stopped for this session."), true)
        };
        v_flex()
            .absolute()
            .inset_0()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .px(px(28.))
            .child(div().text_size(px(12.5)).text_center().text_color(theme.muted_foreground).child(line))
            .when(can, |d| {
                d.child(pill_button("term-agent-start", SharedString::from(format!("Start {name}")), &theme, cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.start_hidden_terminal(cx);
                    window.focus(&this.side_term_focus, cx);
                    cx.notify();
                })))
            })
            .into_any_element()
    }
}

/// The link a cell is part of: the stretch between blanks and quotes
/// around it, when that is an http address.
fn link_at(cells: &[char], col: usize) -> Option<String> {
    let edge = |c: char| c.is_whitespace() || c == '\0' || "\"'<>`".contains(c);
    if cells.get(col).is_none_or(|c| edge(*c)) {
        return None;
    }
    let from = cells[..col].iter().rposition(|c| edge(*c)).map(|i| i + 1).unwrap_or(0);
    let to = cells[col..].iter().position(|c| edge(*c)).map(|i| col + i).unwrap_or(cells.len());
    let word: String = cells[from..to].iter().collect();
    let at = word.find("https://").or_else(|| word.find("http://"))?;
    Some(word[at..].trim_end_matches(['.', ',', ')', ']', ';', ':']).to_string())
}

/// A button as a mouse report names it.
fn button_code(b: MouseButton) -> Option<u8> {
    match b {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

/// The keys held with the mouse, as a report adds them.
fn mod_bits(m: &Modifiers) -> u8 {
    (if m.shift { 4 } else { 0 }) | (if m.alt { 8 } else { 0 }) | (if m.control { 16 } else { 0 })
}

/// The ground and the ink of a screen, light or dark.
fn grounds(light: bool) -> (u32, u32) {
    if light { (0xfaf9f5, 0x1f1e1d) } else { (0x1f1e1d, 0xe8e6dc) }
}

fn hsla(c: u32) -> Hsla {
    rgb(c).into()
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |sh: u32| ((a >> sh & 0xff) as f32 * (1.0 - t) + (b >> sh & 0xff) as f32 * t).round() as u32;
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// A screen's rows as they are drawn, a row a line.
fn screen_lines(rows: &[Vec<pty::Span>], cell_w: f32, light: bool, agent: bool, picked: &[Option<(usize, usize)>], picked_bg: Hsla) -> Vec<AnyElement> {
    let (ground, ink) = grounds(light);
    let mut lines: Vec<AnyElement> = Vec::with_capacity(rows.len());
    for (ix, row) in rows.iter().enumerate() {
        let mut text = String::new();
        let mut looks = Vec::new();
        // What is drawn under the letters, a cell at a time:
        // each stretch's ground, the whole height of the row,
        // and the block characters as the shapes they are.
        let mut under: Vec<AnyElement> = Vec::new();
        let mut col = 0usize;
        for s in row {
            let start = text.len();
            let (mut fg, mut bg) = (s.fg.unwrap_or(ink), s.bg);
            if s.inverse {
                (fg, bg) = (bg.unwrap_or(ground), Some(fg));
            }
            if s.dim {
                fg = mix(fg, bg.unwrap_or(ground), 0.45);
            }
            // A shell's colours are a dark terminal's: on a
            // light ground the pale ones are brought toward
            // the ink, or yellow and white cannot be read.
            if light && !agent && bg.is_none() && s.fg.is_some() {
                let lum = 0.2126 * (fg >> 16 & 0xff) as f32 + 0.7152 * (fg >> 8 & 0xff) as f32 + 0.0722 * (fg & 0xff) as f32;
                if lum > 140. {
                    fg = mix(fg, ink, ((lum - 140.) / 115. * 0.6 + 0.25).min(0.8));
                }
            }
            let (from, mark) = (col, under.len());
            for c in s.text.chars() {
                match block_shape(c) {
                    Some((shapes, strength)) => {
                        for (x, y, bw, bh) in shapes {
                            under.push(
                                div()
                                    .absolute()
                                    .left(px((col as f32 + x) * cell_w))
                                    .top(px(y * LINE_H))
                                    .w(px(bw * cell_w + 0.5))
                                    .h(px(bh * LINE_H))
                                    .bg(hsla(fg).opacity(strength))
                                    .into_any_element(),
                            );
                        }
                        text.push(' ');
                    }
                    None => text.push(c),
                }
                col += if wide(c) { 2 } else { 1 };
            }
            if let Some(bg) = bg {
                // Before the shapes of its own cells.
                under.insert(
                    mark,
                    div().absolute().left(px(from as f32 * cell_w)).top_0().w(px((col - from) as f32 * cell_w + 0.5)).h(px(LINE_H)).bg(hsla(bg)).into_any_element(),
                );
            }
            looks.push((
                start..text.len(),
                HighlightStyle {
                    color: Some(hsla(fg)),
                    font_weight: s.bold.then_some(FontWeight::BOLD),
                    font_style: s.italic.then_some(FontStyle::Italic),
                    underline: s.underline.then(|| UnderlineStyle { thickness: px(1.), ..Default::default() }),
                    ..Default::default()
                },
            ));
        }
        // What is selected of the row, over its grounds and under its letters.
        if let Some((from, to)) = picked.get(ix).copied().flatten() {
            under.push(div().absolute().left(px(from as f32 * cell_w)).top_0().w(px((to - from) as f32 * cell_w)).h(px(LINE_H)).bg(picked_bg).into_any_element());
        }
        if text.is_empty() {
            text.push(' ');
        }
        lines.push(div().h(px(LINE_H)).relative().flex_shrink_0().whitespace_nowrap().children(under).child(div().relative().child(StyledText::new(text).with_highlights(looks))).into_any_element());
    }
    lines
}

/// Whether a character takes two cells, as East Asian wide ones do.
fn wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x115f | 0x2e80..=0x303e | 0x3041..=0x33ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xa000..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff | 0xfe30..=0xfe4f | 0xff00..=0xff60 | 0xffe0..=0xffe6 | 0x1f300..=0x1f64f | 0x1f900..=0x1f9ff | 0x20000..=0x3fffd)
}

/// A block character (U+2580 to U+259F) as the parts of its cell it
/// fills, each (left, top, width, height) in cells, and how strong the
/// ink is. A font draws these to its own letter height, which is less
/// than a row: pictures made of them (Claude Code's own mark at the top
/// of its screen) came out in stripes with gaps between the rows.
fn block_shape(c: char) -> Option<(Vec<(f32, f32, f32, f32)>, f32)> {
    let code = c as u32;
    if !(0x2580..=0x259f).contains(&code) {
        return None;
    }
    let (ul, ur, ll, lr) = ((0., 0., 0.5, 0.5), (0.5, 0., 0.5, 0.5), (0., 0.5, 0.5, 0.5), (0.5, 0.5, 0.5, 0.5));
    let full = vec![(0., 0., 1., 1.)];
    Some(match code {
        0x2580 => (vec![(0., 0., 1., 0.5)], 1.),
        0x2581..=0x2587 => {
            let h = (code - 0x2580) as f32 / 8.;
            (vec![(0., 1. - h, 1., h)], 1.)
        }
        0x2588 => (full, 1.),
        0x2589..=0x258f => (vec![(0., 0., (0x2590 - code) as f32 / 8., 1.)], 1.),
        0x2590 => (vec![(0.5, 0., 0.5, 1.)], 1.),
        0x2591 => (full, 0.25),
        0x2592 => (full, 0.5),
        0x2593 => (full, 0.75),
        0x2594 => (vec![(0., 0., 1., 0.125)], 1.),
        0x2595 => (vec![(0.875, 0., 0.125, 1.)], 1.),
        0x2596 => (vec![ll], 1.),
        0x2597 => (vec![lr], 1.),
        0x2598 => (vec![ul], 1.),
        0x2599 => (vec![ul, ll, lr], 1.),
        0x259a => (vec![ul, lr], 1.),
        0x259b => (vec![ul, ur, ll], 1.),
        0x259c => (vec![ul, ur, lr], 1.),
        0x259d => (vec![ur], 1.),
        0x259e => (vec![ur, ll], 1.),
        _ => (vec![ur, ll, lr], 1.),
    })
}
