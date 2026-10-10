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
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use emaki_core::model::AgentId;
use emaki_core::transcript::SessionRef;
use emaki_core::pty::{self, Pty};

use crate::panels::CONVERSATION_MIN;
use crate::workbench::{pill_button, swallow_click, term_bytes, MenuDo, Page, TabGhost, CloseTab, TermBackTab, TermClear, TermNewTab, TermTab, Workbench, TERMINAL_CONTEXT};

/// How wide the panel is to begin with, and the least it is dragged to.
pub(crate) const TERM_W: Pixels = px(520.);
pub(crate) const TERM_MIN: Pixels = px(300.);
/// The strip over the panel's edge that takes the drag.
const TERM_GRIP: Pixels = px(8.);
/// A font's own line over the letters' size, as JetBrains Mono has it.
const LINE_RATIO: f32 = 1.32;
/// The wheel's pixels to a row, as Kaku counts a trackpad's.
const WHEEL_ROW: f32 = 15.;
/// The cursor's half blink.
const BLINK: Duration = Duration::from_millis(500);
/// How long the panel takes to come and to go.
const TERM_PANEL_ANIM: Duration = Duration::from_millis(200);
/// How often a panel that shows looks for something new on its screen.
const POLL: Duration = Duration::from_millis(33);
/// How long the panel says a selection was copied.
const COPIED_FOR: Duration = Duration::from_millis(2500);
/// The head over a side, as tall as the path bar over the conversation.
const HEAD_H: f32 = 31.;
/// A shell's tab in the head: its height, the most and the least it is
/// wide, and the room between two.
const TAB_H: f32 = 22.;
const TAB_MAX: f32 = 132.;
const TAB_MIN: f32 = 92.;
const TAB_GAP: f32 = 0.;
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
    pub(crate) tabs: Vec<ShellTab>,
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
    /// The same columns of every row (a drag with ⌥), not a run of text.
    block: bool,
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
        if self.block {
            let (from, to) = (self.anchor.1.min(self.head.1) as usize, self.anchor.1.max(self.head.1) as usize + 1);
            for slot in out.iter_mut().take(b.0 as usize + 1).skip(a.0 as usize) {
                *slot = Some((from, to));
            }
            return out;
        }
        let len = |r: u16| cells.get(r as usize).map(|c| c.len()).unwrap_or(0);
        // Kaku's `selection_word_boundary`.
        let word = |c: char| !c.is_whitespace() && c != '\0' && !"{}[]()\"'-".contains(c);
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

/// A choice of the panel's own menu, opened with a right click.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TermDo {
    Copy,
    Paste,
    Clear,
    NewTab,
    CloseTab,
}

impl TermDo {
    pub(crate) fn icon(self) -> &'static str {
        match self {
            TermDo::Copy => "icons/copy.svg",
            TermDo::Paste => "icons/file-plus.svg",
            TermDo::Clear => "icons/trash.svg",
            TermDo::NewTab => "icons/plus.svg",
            TermDo::CloseTab => "icons/close.svg",
        }
    }
}

/// A link on the screen: its row, the cells it takes, and what it opens.
#[derive(Clone, PartialEq)]
pub(crate) struct Link {
    row: u16,
    from: usize,
    to: usize,
    target: Target,
}

#[derive(Clone, PartialEq)]
enum Target {
    Url(String),
    File(std::path::PathBuf),
}

/// What the terminal card is drawn with: its rows, its ground and inks,
/// its face, and the size of a cell.
pub(crate) struct CardLook {
    pub(crate) lines: Vec<AnyElement>,
    pub(crate) ground: Hsla,
    pub(crate) ink: Hsla,
    pub(crate) quiet: Hsla,
    pub(crate) font: Font,
    pub(crate) text: f32,
    pub(crate) line_h: f32,
    pub(crate) cell_w: f32,
}

/// How many cells of a row stand before its `chars`-th character.
pub(crate) fn cells_before(row: &[pty::Span], chars: usize) -> usize {
    row.iter().flat_map(|s| s.text.chars()).take(chars).map(|c| if wide(c) { 2 } else { 1 }).sum()
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
        self.side_term && self.page == Page::Session && self.detail.is_some() && !self.fold_term
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

    /// ⌘⇧T and ⌘⇧A, the buttons' keys: beside a conversation only.
    pub(crate) fn toggle_term_key(&mut self, agent: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Session && self.detail.is_some() {
            self.toggle_term(agent, window, cx);
        }
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
            "term:copied" => self.side_copied = Some(Instant::now()),
            _ if step.starts_with("term:hover:") => {
                // term:hover:<row>,<col>
                let n: Vec<u16> = step["term:hover:".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
                if let ([row, col], Some(pty)) = (&n[..], self.term_panel_pty()) {
                    self.side_link = self.term_link_at(&pty, *row, *col);
                }
            }
            _ if step.starts_with("term:text:") => self.cfg.terminal.size = step["term:text:".len()..].parse().unwrap_or(13.),
            "term:clear" => self.term_clear(),
            _ if step.starts_with("term:sel:") => {
                // term:sel:<unit>,<row>,<col>,<row>,<col>
                let n: Vec<u16> = step["term:sel:".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
                if let [unit, r1, c1, r2, c2] = n[..] {
                    self.side_sel = Some(Sel { anchor: (r1, c1), head: (r2, c2), unit: unit as usize % 4, block: unit >= 4 });
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
                .managed_tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
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
            .child(segment("term-shell", "icons/terminal.svg", "Shell (⌘⇧T)".to_string(), false, cx))
            // The agent's side wears the agent's own mark.
            .child(segment("term-agent", crate::workbench::agent_icon_path(agent), format!("{} in a terminal (⌘⇧A)", agent.display_name()), true, cx))
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
                // The cursor's blink is a draw too, while the panel has the keyboard.
                let blink = this.side_focused && this.cfg.terminal.cursor_blink && this.blink_on() != this.side_blink_on;
                if blink || this.term_panel_pty().is_some_and(|p| p.quiet_for() < POLL * 4 || !p.alive()) {
                    cx.notify();
                }
                true
            });
            if !on.unwrap_or(false) {
                break;
            }
        }));
    }

    /// Whether the cursor is in the lit half of its blink: on and off by
    /// turns, with no fade, from when it last moved.
    fn blink_on(&self) -> bool {
        (self.side_blink.1.elapsed().as_millis() / BLINK.as_millis()) % 2 == 0
    }

    /// A row's height at the letters' size now.
    fn term_line_h(&self) -> f32 {
        (self.cfg.terminal.size * LINE_RATIO * self.cfg.terminal.line_scale()).round()
    }

    /// The scheme the settings ask for: the window's own appearance, or
    /// one of the two kept whatever the window is.
    fn term_scheme(&self, cx: &App) -> &'static Scheme {
        scheme(match self.cfg.terminal.theme.as_str() {
            "dark" => false,
            "light" => true,
            _ => !cx.theme().mode.is_dark(),
        })
    }

    /// The face the settings ask for, in the scheme's plain weight, with
    /// Kaku's list of faces to fall back on.
    fn term_font(&self, k: &Scheme, cx: &App) -> Font {
        let t = &self.cfg.terminal;
        Font {
            family: crate::fonts::term_family(&t.font, cx),
            features: FontFeatures(Arc::new(if t.ligatures { Vec::new() } else { vec![("calt".into(), 0), ("clig".into(), 0), ("liga".into(), 0)] })),
            fallbacks: Some(FontFallbacks::from_fonts(crate::fonts::TERM_FALLBACKS.iter().map(|f| f.to_string()).collect())),
            weight: k.plain,
            style: FontStyle::Normal,
        }
    }

    /// The terminal card's rows (`Workbench::render_terminal`), drawn as
    /// the panel draws a screen: the same face, rows, shapes and colour
    /// rules. The scheme is the window's own appearance, light or dark,
    /// whatever the panel's is set to: the card sits in the conversation,
    /// over the composer.
    pub(crate) fn term_card_look(&self, rows: &[Vec<pty::Span>], cx: &App) -> CardLook {
        let k = scheme(!cx.theme().mode.is_dark());
        let font = self.term_font(k, cx);
        let text = self.cfg.terminal.size;
        // A cell is a letter's advance in the face: there is no window
        // here to lay a line out with.
        let ts = cx.text_system();
        let cell_w = ts.advance(ts.resolve_font(&font), px(text), 'M').map(|a| f32::from(a.width)).unwrap_or(text * 0.6);
        // The hidden terminal's screen is `pty::COLS` wide and the card
        // no wider than the conversation's column: letters too large for
        // every column to fit are drawn smaller, so no row is cut.
        let fit = ((f32::from(crate::workbench::CONTENT_W) - 26.) / (cell_w * pty::COLS as f32)).min(1.);
        let (text, cell_w) = (text * fit, cell_w * fit);
        let lh = (text * LINE_RATIO * self.cfg.terminal.line_scale()).round();
        CardLook { lines: screen_lines(rows, cell_w, lh, k, &[], None), ground: hsla(k.bg), ink: hsla(k.fg), quiet: hsla(mix(k.fg, k.bg, 0.45)), font, text, line_h: lh, cell_w }
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
        let k = self.term_scheme(cx);
        let face = self.term_font(k, cx);
        if !cx.has_active_drag() {
            self.shell_drag = None;
        }
        let name = pty::shell_argv().first().and_then(|s| std::path::Path::new(s).file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "shell".into());
        let empty = Shells::default();
        let set = self.shells.get(&r.session_id).unwrap_or(&empty);
        let n = set.tabs.len().max(1) as f32;
        let tab_w = ((room - TAB_H - 6. - TAB_GAP * n) / n).clamp(TAB_MIN, TAB_MAX);
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
            // Kaku's tabs: flat, the one showing on a raised ground in
                // the bold weight, the others grey until the pointer is
                // over them.
            let (hover_bg, press_bg, close_bg, lit) = (hsla(k.tab_hover), hsla(k.tab_on), hsla(k.split), hsla(k.fg));
            let el = h_flex()
                .id(("shell-tab", id as usize))
                .group(group.clone())
                .w(px(tab_w))
                .h(px(HEAD_H - 2.))
                .pl(px(10.))
                .pr(px(5.))
                .gap(px(6.))
                .items_center()
                .cursor_pointer()
                .overflow_hidden()
                .font(face.clone())
                .text_size(px(11.5))
                .map(|d| {
                    if active {
                        d.bg(hsla(k.tab_on)).text_color(lit).font_weight(k.bold)
                    } else {
                        d.text_color(hsla(k.tab_off)).hover(move |s| s.bg(hover_bg).text_color(lit)).active(move |s| s.bg(press_bg))
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
                        .child(Icon::new(IconName::Close).with_size(px(10.)).text_color(hsla(k.tab_off))),
                );
            // A new tab grows in from nothing. The wrapper is there at
            // rest too, under the same name.
            row = row.child(div().flex_shrink_0().overflow_hidden().child(el).with_animation(ElementId::Name(format!("shell-tab-w-{id}").into()), Animation::new(TAB_ANIM).with_easing(ease_out_quint()), move |d, t| {
                if fresh { d.w(px(tab_w * t)).opacity(t) } else { d }
            }));
        }
        let (hover_bg, press_bg, lit) = (hsla(k.tab_hover), hsla(k.tab_on), hsla(k.fg));
        let plus = div()
            .id("shell-tab-new")
            .w(px(TAB_H + 6.))
            .h(px(HEAD_H - 2.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(hsla(k.tab_off))
            .hover(move |s| s.bg(hover_bg).text_color(lit))
            .active(move |s| s.bg(press_bg))
            .managed_tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("New shell (⌘T)").build(window, cx))
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
    pub(crate) fn term_panel_w(&self) -> Pixels {
        let (files, outline) = self.panels_shown();
        let beside = if files || outline { self.panel_w_now() } else { px(0.) } + self.file_pane_least();
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
        let beside = if files || outline { self.panel_w_now() } else { px(0.) } + self.file_pane_least();
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
        self.side_wheel += f32::from(delta_y) / WHEEL_ROW;
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
            self.term_unhover();
        }
    }

    /// The cell of the screen a point of the window is over: column, row,
    /// held inside the screen.
    fn term_cell(&self, at: Point<Pixels>, pty: &Pty) -> (u16, u16) {
        let b = self.side_term_bounds.get();
        let (rows, cols) = pty.size();
        let col = (f32::from(at.x - b.left()) - self.side_pad.0) / self.side_cell_w.max(1.);
        let row = (f32::from(at.y - b.top()) - self.side_pad.0) / self.term_line_h();
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
        // ⌘ and a click opens the link under the pointer, in a program
        // that hears the mouse too.
        if e.button == MouseButton::Left && e.modifiers.platform {
            if let Some(link) = self.term_link_at(&pty, row, col) {
                match link.target {
                    Target::Url(url) => cx.open_url(&url),
                    Target::File(path) => crate::sys::open_path(&path),
                }
            }
            return;
        }
        let want = pty.mouse().filter(|_| pty.alive() && self.side_back == 0);
        if let (Some(want), false, Some(button)) = (want, e.modifiers.shift, button_code(e.button)) {
            self.side_sel = None;
            self.side_mouse = Some(button);
            self.side_mouse_cell = (col, row);
            pty.write(&pty::mouse_bytes(button | mod_bits(&e.modifiers), col, row, true, want.form));
            return;
        }
        match e.button {
            MouseButton::Left => {}
            MouseButton::Right => {
                let mut items = Vec::new();
                if !self.term_sel_text().is_empty() {
                    items.push(("Copy", MenuDo::Term(TermDo::Copy)));
                }
                items.push(("Paste", MenuDo::Term(TermDo::Paste)));
                items.push(("Clear", MenuDo::Term(TermDo::Clear)));
                if !self.side_term_agent {
                    items.push(("", MenuDo::Rule));
                    items.push(("New Tab", MenuDo::Term(TermDo::NewTab)));
                    if self.term_panel_pty().is_some() {
                        items.push(("Close Tab", MenuDo::Term(TermDo::CloseTab)));
                    }
                }
                self.open_menu(e.position, items, cx);
                return;
            }
            MouseButton::Middle => return self.term_paste(cx),
            _ => return,
        }
        // With Shift the selection there is runs on to the press.
        if let Some(sel) = self.side_sel.as_mut().filter(|_| e.modifiers.shift) {
            sel.head = (row, col);
            self.side_selecting = true;
            return;
        }
        // ⌥ and a drag takes the same columns of every row; ⌥ and a
        // click at a prompt takes the cursor there, when the button is
        // let go where it was pressed.
        self.side_alt = e.modifiers.alt && want.is_none() && self.side_back == 0 && !pty.alt_screen();
        self.side_sel = Some(Sel { anchor: (row, col), head: (row, col), unit: e.click_count.clamp(1, 3), block: e.modifiers.alt });
        self.side_selecting = true;
    }

    /// The cursor is taken along its row to a column, with arrow keys.
    fn term_cursor_to(&self, pty: &Pty, row: u16, col: u16) {
        let Some((_, at)) = pty.cursor().filter(|(r, _)| *r == row) else { return };
        let key: &[u8] = match (col > at, pty.app_cursor()) {
            (true, false) => b"\x1b[C",
            (true, true) => b"\x1bOC",
            (false, false) => b"\x1b[D",
            (false, true) => b"\x1bOD",
        };
        pty.write(&key.repeat(col.abs_diff(at) as usize));
    }

    /// The link the cell is part of, when it is part of one.
    fn term_link_at(&self, pty: &Pty, row: u16, col: u16) -> Option<Link> {
        let (rows, _) = pty.rows_back(self.side_back);
        let cwd = self.selected_ref().map(|r| std::path::PathBuf::from(&r.cwd)).unwrap_or_default();
        let (from, to, target) = link_at(&row_cells(rows.get(row as usize)?), col as usize, &cwd)?;
        Some(Link { row, from, to, target })
    }

    /// What is on the clipboard is pasted at the prompt.
    fn term_paste(&mut self, cx: &mut Context<Self>) {
        let Some(pty) = self.term_panel_pty().filter(|p| p.alive()) else { return };
        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
            pty.paste(&text);
            self.side_back = 0;
            self.side_sel = None;
        }
    }

    /// A choice of the panel's menu.
    pub(crate) fn term_do(&mut self, what: TermDo, window: &mut Window, cx: &mut Context<Self>) {
        match what {
            TermDo::Copy => {
                self.term_copy(cx);
            }
            TermDo::Paste => self.term_paste(cx),
            TermDo::Clear => self.term_clear(),
            TermDo::NewTab => self.shell_new(None, cx),
            TermDo::CloseTab => {
                if let Some(id) = self.selected_ref().and_then(|r| self.shells.get(&r.session_id)).and_then(|s| s.current()).map(|t| t.id) {
                    self.shell_close(id, cx);
                }
            }
        }
        window.focus(&self.side_term_focus, cx);
    }

    /// The button let go, in the panel or outside it. A selection just
    /// made is copied, as it is in a terminal.
    fn term_mouse_up(&mut self, e: &MouseUpEvent, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.side_selecting) {
            let alt = std::mem::take(&mut self.side_alt);
            // With copying on selection off, what is selected stays for ⌘C.
            let kept = !self.cfg.terminal.copy_on_select && !self.term_sel_text().is_empty();
            if !kept && !self.term_copy(cx) {
                let at = self.side_sel.take().map(|s| s.anchor);
                if let (true, Some((row, col)), Some(pty)) = (alt, at, self.term_panel_pty()) {
                    self.term_cursor_to(&pty, row, col);
                }
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
        // The link under the pointer is underlined, looked for a cell at a time.
        let cell = self.term_cell(e.position, &pty);
        if cell != self.side_hover {
            self.side_hover = cell;
            let link = self.term_link_at(&pty, cell.1, cell.0);
            if link != self.side_link {
                self.side_link = link;
                cx.notify();
            }
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
            let runs_on = !sel.block && wraps.get(r).copied().unwrap_or(false) && Some(r) != last;
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

    /// What was under the pointer is looked for again when it next moves:
    /// the rows have changed under it.
    fn term_unhover(&mut self) {
        self.side_link = None;
        self.side_hover = (u16::MAX, u16::MAX);
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
                    self.term_paste(cx);
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
                self.term_unhover();
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
        self.term_unhover();
        // A key begins the cursor's blink again, lit.
        self.side_blink.1 = Instant::now();
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
        if self.page != Page::Session || self.detail.is_none() || self.fold_term {
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
        // One look for both sides, Kaku's: its scheme for the window's
        // appearance, its face, and its room around the screen, which it
        // counts in the screen's own pixels.
        let k = self.term_scheme(cx);
        let face = self.term_font(k, cx);
        let (text, lh) = (self.cfg.terminal.size, self.term_line_h());
        let scale = window.scale_factor().max(1.);
        let (pad, pad_foot) = match self.cfg.terminal.padding.as_str() {
            "compact" => (10., 8.),
            "roomy" => (if scale >= 2. { 20. } else { 26. }, 16. / scale),
            _ => (15., 8.),
        };
        let cell_w = {
            let probe = "MMMMMMMMMM";
            let run = TextRun { len: probe.len(), font: face.clone(), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(probe.into(), px(text), &[run], None).width) / probe.len() as f32
        };
        let room = self.side_term_bounds.get().size;
        let size = (((f32::from(room.height) - pad - pad_foot) / lh).floor().max(4.) as u16, ((f32::from(room.width) - 2. * pad) / cell_w.max(1.)).floor().max(20.) as u16);
        if room.height > px(0.) && !leaving {
            self.side_term_size.set(size);
            self.side_cell_w = cell_w;
            self.side_pad = (pad, pad_foot);
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
                        Some(e) => d.text_color(hsla(k.ansi[1])).child(format!("Could not start a shell: {e}")),
                        None => d.text_color(hsla(k.tab_off)).child("No shell open. Press + for a new one."),
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

        let (ground, ink) = (k.bg, k.fg);
        let focused = self.side_term_focus.is_focused(window);
        if !leaving {
            self.side_focused = focused;
        }
        // A program that hears the mouse has the arrow, a link the hand.
        let grabbed = shown.as_ref().is_ok_and(|p| p.alive() && p.mouse().is_some()) && self.side_back == 0;
        let pointer = if self.side_link.is_some() { CursorStyle::PointingHand } else if grabbed { CursorStyle::Arrow } else { CursorStyle::IBeam };
        let body: AnyElement = match shown {
            Err(el) => el,
            Ok(pty) => {
                let (rows, back) = pty.rows_back(self.side_back);
                self.side_back = back;
                let picked = match self.side_sel {
                    Some(sel) if !leaving => sel.spans(&rows.iter().map(|r| row_cells(r)).collect::<Vec<_>>()),
                    _ => Vec::new(),
                };
                let lines = screen_lines(&rows, cell_w, lh, k, &picked, self.side_link.as_ref().filter(|_| !leaving));
                let copied = self.side_copied.filter(|at| at.elapsed() < COPIED_FOR && !leaving).map(|_| {
                    // Kaku's toast: two cells in from the corner, a cell
                    // and a half tall, gone over its last half second.
                    div()
                        .absolute()
                        .right(px(2. * cell_w))
                        .bottom(px(2. * lh))
                        .h(px(1.5 * lh))
                        .px(px(0.75 * cell_w * 2.))
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .bg(hsla(k.toast.0).opacity(0.9))
                        .text_color(hsla(k.toast.1))
                        .font(face.clone())
                        .text_size(px(text))
                        .child("Copied")
                        .with_animation("term-copied", Animation::new(COPIED_FOR), |d, t| d.opacity(if t > 0.8 { (1. - t) / 0.2 } else { 1. }))
                });
                // The cursor, where the next letter goes: Kaku's bar, two
                // of the screen's pixels wide and the row's height, on and
                // off by turns from when it last moved while the panel has
                // the keyboard, and still otherwise.
                let at = pty.cursor().filter(|_| back == 0 && pty.alive());
                if let Some(at) = at.filter(|at| !leaving && *at != self.side_blink.0) {
                    self.side_blink = (at, Instant::now());
                }
                if !leaving {
                    self.side_blink_on = self.blink_on();
                }
                let (shape, blinks) = (self.cfg.terminal.cursor.clone(), self.cfg.terminal.cursor_blink);
                let cursor = at.filter(|_| !focused || !blinks || self.side_blink_on).map(|(row, col)| {
                    let (thin, ink) = (px((2. / scale).max(1.)), hsla(k.cursor));
                    let d = div().absolute().top(px(pad + row as f32 * lh)).left(px(pad + col as f32 * cell_w)).h(px(lh));
                    match shape.as_str() {
                        // The letter under a block shows through it; without
                        // the keyboard the block is its outline.
                        "block" if focused => d.w(px(cell_w)).bg(ink.opacity(0.55)),
                        "block" => d.w(px(cell_w)).border_1().border_color(ink),
                        "underline" => d.w(px(cell_w)).border_b(thin * 2.).border_color(ink),
                        _ => d.w(thin).bg(ink),
                    }
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
                let screen_face = face.clone();
                let screen = move |lines: Vec<AnyElement>| div().absolute().inset_0().overflow_hidden().px(px(pad)).pt(px(pad)).font(screen_face.clone()).text_size(px(text)).text_color(hsla(ink)).children(lines);
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
                        screen(screen_lines(&old.rows(), cell_w, lh, k, &[], None)).with_animation(
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

        let back = (self.side_back > 0).then(|| div().flex_shrink_0().pr(px(10.)).font(face.clone()).text_size(px(11.)).text_color(hsla(k.tab_off)).child(format!("{} lines back", self.side_back)));
        // Both heads are a row of tabs: the agent's is its one screen,
        // the shell's a tab a shell.
        let head_row: AnyElement = if agent {
            h_flex()
                .h_full()
                .px(px(10.))
                .gap(px(6.))
                .items_center()
                .bg(hsla(k.tab_on))
                .font(face.clone())
                .font_weight(k.bold)
                .text_size(px(11.5))
                .text_color(hsla(k.fg))
                .child(Icon::default().path(crate::workbench::agent_icon_path(r.agent)).with_size(px(12.)).text_color(crate::workbench::agent_color(r.agent, &theme)))
                .child(r.agent.display_name())
                .into_any_element()
        } else {
            self.render_shell_tabs(r, f32::from(w) - if back.is_some() { 110. } else { 0. }, cx)
        };
        let bounds = self.side_term_bounds.clone();
        let entity = cx.entity().downgrade();
        let panel = v_flex()
            .w(w)
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(theme.border)
            .bg(hsla(k.bg))
            .child(
                // The path bar's own height, so the two heads read as
                // one line across the window, on the terminal's ground
                // as Kaku's tab bar is.
                h_flex()
                    .h(px(HEAD_H))
                    .flex_shrink_0()
                    .items_center()
                    .overflow_hidden()
                    .bg(hsla(k.bg))
                    .border_t_1()
                    .border_b_1()
                    .border_color(hsla(k.split))
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
                    .bg(hsla(ground))
                    .cursor(pointer)
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
                    .on_hover(cx.listener(|this, over: &bool, _, cx| {
                        if !over && this.side_link.is_some() {
                            this.term_unhover();
                            cx.notify();
                        }
                    }))
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
                    .on_action(cx.listener(|this, _: &CloseTab, _, cx| {
                        // ⌘W, the window's own action, heard here first
                        // while the panel has the keyboard. On the shell's
                        // side it is the shell's and nothing else's: it
                        // closes the tab showing, and with none to close
                        // does nothing, so it never reaches a file shown or
                        // the session's tab. On the agent's side it is the
                        // window's.
                        if this.side_term_agent {
                            cx.propagate();
                        } else if let Some(id) = this.selected_ref().and_then(|r| this.shells.get(&r.session_id)).and_then(|s| s.current()).map(|t| t.id) {
                            this.shell_close(id, cx);
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
        let k = self.term_scheme(cx);
        let name = r.agent.display_name();
        let (line, can): (String, bool) = if r.agent != AgentId::ClaudeCode || !self.hub.hidden_terminals() {
            (format!("{name} runs here only for Claude Code sessions, with the hidden terminal on."), false)
        } else if self.in_own_terminal(r) {
            (format!("A turn is running in a terminal of yours. When it is over, {name} can be started here."), false)
        } else if self.is_draft(&r.session_id) {
            (format!("{name} starts with your first message."), false)
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
            .child(div().text_size(px(12.5)).text_center().text_color(hsla(k.tab_off)).child(line))
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

/// The link a cell is part of, by Kaku's rules: the cells it takes and
/// what it opens. An address with a scheme, one that begins "www.", a
/// mail address, a bare domain under a well-known ending, and a path to
/// a file that is there (`cwd` is where a relative one starts; a line and
/// column after it are let go).
fn link_at(cells: &[char], col: usize, cwd: &std::path::Path) -> Option<(usize, usize, Target)> {
    const ENDINGS: &[&str] = &[
        "com", "net", "org", "edu", "gov", "io", "dev", "ai", "fun", "xyz", "me", "im", "tv", "to", "co", "info", "biz", "tech", "site", "online", "cloud", "blog", "store", "link", "live", "news", "cn", "jp", "kr", "uk", "de", "fr",
        "us", "ca", "au", "br", "ru", "nl", "se", "ch", "hk", "tw", "sg",
    ];
    let edge = |c: char| c.is_whitespace() || c == '\0' || "\"'<>`".contains(c);
    if cells.get(col).is_none_or(|c| edge(*c)) {
        return None;
    }
    let mut from = cells[..col].iter().rposition(|c| edge(*c)).map(|i| i + 1).unwrap_or(0);
    let mut to = cells[col..].iter().position(|c| edge(*c)).map(|i| col + i).unwrap_or(cells.len());
    // The brackets around it and the stop after it are not part of it.
    while from < to && "([{".contains(cells[from]) {
        from += 1;
    }
    while to > from && ".,;:!?)]}".contains(cells[to - 1]) && !(cells[to - 1] == ')' && cells[from..to].contains(&'(')) {
        to -= 1;
    }
    let word: String = cells[from..to].iter().collect();
    let target = if let Some(at) = word.find("://") {
        // From where the scheme's name begins.
        let start = word[..at].rfind(|c: char| !c.is_alphanumeric() && c != '_').map(|i| i + 1).unwrap_or(0);
        if start == at {
            return None;
        }
        from += word[..start].chars().count();
        Target::Url(word[start..].to_string())
    } else if word.to_lowercase().starts_with("www.") && word.len() > 4 {
        Target::Url(format!("https://{word}"))
    } else if let Some((name, host)) = word.split_once('@').filter(|(name, host)| {
        let plain = |s: &str, more: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || more.contains(c));
        plain(name, "_.+-") && host.contains('.') && host.split('.').all(|part| plain(part, "_-"))
    }) {
        Target::Url(format!("mailto:{name}@{host}"))
    } else {
        let host = word.split(['/', ':']).next().unwrap_or("").to_lowercase();
        let parts: Vec<&str> = host.split('.').collect();
        let domain = parts.len() > 1 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) && ENDINGS.contains(parts.last()?);
        if domain {
            Target::Url(format!("https://{word}"))
        } else if word.contains('/') {
            // A path, with the line and the column a compiler adds let go.
            let mut path = word.as_str();
            for _ in 0..2 {
                if let Some((head, tail)) = path.rsplit_once(':').filter(|(_, tail)| !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit())) {
                    let _ = tail;
                    path = head;
                }
            }
            let path = match path.strip_prefix("~/") {
                Some(rest) => emaki_core::paths::home().join(rest),
                None => cwd.join(path),
            };
            if !path.exists() {
                return None;
            }
            Target::File(path)
        } else {
            return None;
        }
    };
    (from..to).contains(&col).then_some((from, to, target))
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

/// One of Kaku's two colour schemes (tw93/kaku, MIT): the screen's
/// ground and ink, the cursor, the selection's ground (with how strong)
/// and ink, the rule under the head, a tab's grounds and the ink of one
/// not showing, the toast's ground and ink, the sixteen named colours,
/// and the grounds and inks it draws as others because Claude Code's
/// screen reads badly with them as they are.
struct Scheme {
    bg: u32,
    fg: u32,
    cursor: u32,
    sel: (u32, f32),
    sel_fg: u32,
    split: u32,
    tab_on: u32,
    tab_hover: u32,
    tab_off: u32,
    toast: (u32, u32),
    ansi: [u32; 16],
    grounds: &'static [(u32, u32)],
    inks: &'static [(u32, u32)],
    /// The ground a grey from the other side of the scale is drawn as: a
    /// program's quiet band on a dark terminal is a quiet band here.
    wash: u32,
    /// The weights of plain and of bold letters: one step heavier on a
    /// light ground, where thin strokes fade.
    plain: FontWeight,
    bold: FontWeight,
}

static KAKU_DARK: Scheme = Scheme {
    bg: 0x15141b,
    fg: 0xd5d4d6,
    cursor: 0x8e6ad9,
    sel: (0x8e6ad9, 0.55),
    sel_fg: 0xd5d4d6,
    split: 0x29263c,
    tab_on: 0x29263c,
    tab_hover: 0x1f1d28,
    tab_off: 0x6d6d6d,
    toast: (0x8e6ad9, 0xffffff),
    ansi: {
        let k = &crate::look::INKS_DARK;
        [0xc8c6cc, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0xd5d4d6, 0x6d6d6d, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0xd5d4d6]
    },
    grounds: &[(0xc8c6cc, 0x15141b), (0x6d6d6d, 0x3a3942), (0x6e6e6e, 0x3a3942), (0x8ec3ff, 0x3a3942), (0xd5d4d6, 0x4a4954)],
    inks: &[(0x000000, 0xd5d4d6), (0x110f18, 0xd5d4d6), (0x15141b, 0xd5d4d6), (0x1a1a1a, 0xd5d4d6), (0x1c1c1c, 0xd5d4d6)],
    wash: 0x3a3942,
    plain: FontWeight::NORMAL,
    bold: FontWeight::MEDIUM,
};

static KAKU_LIGHT: Scheme = Scheme {
    bg: 0xfffcf0,
    fg: 0x100f0f,
    cursor: 0x343331,
    sel: (0xe8e6db, 1.),
    sel_fg: 0x100f0f,
    split: 0xdddbcf,
    tab_on: 0xe8e6db,
    tab_hover: 0xe8e6db,
    tab_off: 0x4a4946,
    toast: (0x8e6b02, 0x1a1a1a),
    ansi: {
        let k = &crate::look::INKS_LIGHT;
        [0x100f0f, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0x575653, 0x6f6e69, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0x403e3c]
    },
    grounds: &[(0x575653, 0xf2f0eb), (0x585754, 0xf2f0eb), (0x225fa6, 0xf2f0eb), (crate::look::INKS_LIGHT.teal, 0xf2f0eb), (crate::look::INKS_LIGHT.green, 0xf2f0eb), (crate::look::INKS_LIGHT.gold, 0xf2f0eb), (crate::look::INKS_LIGHT.blue, 0xd3e3ea), (0x403e3c, 0xe8e6db)],
    inks: &[(0xffffdb, 0x575653), (0xffffdc, 0x575653)],
    wash: 0xf2f0eb,
    plain: FontWeight::MEDIUM,
    bold: FontWeight::SEMIBOLD,
};

fn scheme(light: bool) -> &'static Scheme {
    if light { &KAKU_LIGHT } else { &KAKU_DARK }
}

fn hsla(c: u32) -> Hsla {
    rgb(c).into()
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |sh: u32| ((a >> sh & 0xff) as f32 * (1.0 - t) + (b >> sh & 0xff) as f32 * t).round() as u32;
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// The colour a scheme draws in place of `c`, when it has one.
fn swapped(table: &[(u32, u32)], c: u32) -> u32 {
    table.iter().find(|(from, _)| *from == c).map(|(_, to)| *to).unwrap_or(c)
}

/// Whether a colour is a grey, or near enough to one.
fn grey(c: u32) -> bool {
    let ch = [c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff];
    ch.iter().max().unwrap() - ch.iter().min().unwrap() < 28
}

/// How bright a colour is to the eye, 0 to 1.
fn brightness(c: u32) -> f32 {
    let ch = |sh: u32| {
        let v = (c >> sh & 0xff) as f32 / 255.;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

/// An ink that can be read on its ground: Kaku's `text_min_contrast_ratio`
/// of 3. An ink too close to the ground is taken toward black or white,
/// whichever is the farther from it, as far as it takes. This is what
/// makes colours a program chose for a dark terminal readable on the
/// light scheme, and the other way round.
fn legible(fg: u32, bg: u32) -> u32 {
    let ratio = |a: u32, b: u32| {
        let (x, y) = (brightness(a), brightness(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    if ratio(fg, bg) >= 3. {
        return fg;
    }
    let to = if brightness(bg) > 0.18 { 0x000000 } else { 0xffffff };
    (1..=10).map(|t| mix(fg, to, t as f32 / 10.)).find(|c| ratio(*c, bg) >= 3.).unwrap_or(to)
}

/// A screen's rows as they are drawn, a row a line.
fn screen_lines(rows: &[Vec<pty::Span>], cell_w: f32, lh: f32, k: &Scheme, picked: &[Option<(usize, usize)>], link: Option<&Link>) -> Vec<AnyElement> {
    let mut lines: Vec<AnyElement> = Vec::with_capacity(rows.len());
    for (ix, row) in rows.iter().enumerate() {
        let sel = picked.get(ix).copied().flatten();
        let linked = link.filter(|l| l.row as usize == ix).map(|l| (l.from, l.to));
        // Whether a cell is selected, and whether it is part of the link
        // under the pointer: either changes how its letter is drawn.
        let marks = |col: usize| (sel.is_some_and(|(f, t)| (f..t).contains(&col)), linked.is_some_and(|(f, t)| (f..t).contains(&col)));
        let mut text = String::new();
        let mut looks = Vec::new();
        // What is drawn under the letters, a cell at a time: each
        // stretch's ground, the whole height of the row, then what is
        // selected, then the block and line characters as the shapes
        // they are.
        let mut grounds: Vec<AnyElement> = Vec::new();
        let mut under: Vec<AnyElement> = Vec::new();
        let mut col = 0usize;
        for s in row {
            // The sixteen named colours are the scheme's own.
            let mut fg = match s.fg_ix {
                Some(i) => k.ansi[i as usize],
                None => s.fg.map(|c| swapped(k.inks, c)).unwrap_or(k.fg),
            };
            let mut bg = match s.bg_ix {
                Some(i) => Some(swapped(k.grounds, k.ansi[i as usize])),
                None => s.bg.map(|c| swapped(k.grounds, c)),
            };
            // A grey ground a program chose for the other kind of terminal
            // (Claude Code's band behind a prompt, near black for a dark
            // one) is the scheme's own quiet ground, and a grey ink that
            // was to stand out on it is the scheme's ink. Kaku swaps the
            // greys it knows by value; this is the same for any grey.
            let light = brightness(k.bg) > 0.5;
            if bg.is_some_and(|c| s.bg_ix.is_none() && grey(c) && if light { brightness(c) < 0.2 } else { brightness(c) > 0.5 }) {
                bg = Some(k.wash);
                if s.fg_ix.is_none() && grey(fg) && if light { brightness(fg) > 0.4 } else { brightness(fg) < 0.2 } {
                    fg = k.fg;
                }
            }
            if s.inverse {
                (fg, bg) = (bg.unwrap_or(k.bg), Some(fg));
            }
            if s.dim {
                fg = mix(fg, bg.unwrap_or(k.bg), 0.45);
            }
            // Shapes keep the colour asked for; letters must be readable.
            let shape = hsla(fg);
            let fg = legible(fg, bg.unwrap_or(k.bg));
            let look = |(picked, linked): (bool, bool)| HighlightStyle {
                color: Some(hsla(if picked { k.sel_fg } else { fg })),
                // Kaku has no italic: slanted text is drawn upright.
                font_weight: s.bold.then_some(k.bold),
                underline: (s.underline || linked).then(|| UnderlineStyle { thickness: px(1.), ..Default::default() }),
                ..Default::default()
            };
            let from = col;
            let mut run = (text.len(), marks(col));
            for c in s.text.chars() {
                let now = marks(col);
                if now != run.1 {
                    if run.0 < text.len() {
                        looks.push((run.0..text.len(), look(run.1)));
                    }
                    run = (text.len(), now);
                }
                let x0 = col as f32 * cell_w;
                if let Some((shapes, strength)) = block_shape(c) {
                    for (x, y, bw, bh) in shapes {
                        under.push(div().absolute().left(px(x0 + x * cell_w)).top(px(y * lh)).w(px(bw * cell_w + 0.5)).h(px(bh * lh)).bg(shape.opacity(strength)).into_any_element());
                    }
                    text.push(' ');
                } else if let Some(parts) = line_shape(c) {
                    under.extend(line_parts(parts, x0, cell_w, lh, shape));
                    text.push(' ');
                } else {
                    text.push(c);
                }
                col += if wide(c) { 2 } else { 1 };
            }
            if run.0 < text.len() {
                looks.push((run.0..text.len(), look(run.1)));
            }
            if let Some(bg) = bg {
                grounds.push(div().absolute().left(px(from as f32 * cell_w)).top_0().w(px((col - from) as f32 * cell_w + 0.5)).h(px(lh)).bg(hsla(bg)).into_any_element());
            }
        }
        if let Some((from, to)) = sel {
            grounds.push(div().absolute().left(px(from as f32 * cell_w)).top_0().w(px((to - from) as f32 * cell_w)).h(px(lh)).bg(hsla(k.sel.0).opacity(k.sel.1)).into_any_element());
        }
        if text.is_empty() {
            text.push(' ');
        }
        lines.push(div().h(px(lh)).relative().flex_shrink_0().whitespace_nowrap().children(grounds).children(under).child(div().relative().child(StyledText::new(text).with_highlights(looks))).into_any_element());
    }
    lines
}

/// A line-drawing character (U+2500 on) as the arms it has from the
/// middle of its cell, left, right, up and down; how thick; and whether
/// its corner is round. A font draws these to its own line height, which
/// is less than a row's here, and the frames Claude Code draws came out
/// with a gap between every two rows. The ones not here are the font's.
fn line_shape(c: char) -> Option<(bool, bool, bool, bool, f32, bool)> {
    let (l, r, u, d, thick, round) = match c as u32 {
        0x2500 => (true, true, false, false, 1., false),
        0x2501 => (true, true, false, false, 2., false),
        0x2502 => (false, false, true, true, 1., false),
        0x2503 => (false, false, true, true, 2., false),
        0x250c => (false, true, false, true, 1., false),
        0x2510 => (true, false, false, true, 1., false),
        0x2514 => (false, true, true, false, 1., false),
        0x2518 => (true, false, true, false, 1., false),
        0x251c => (false, true, true, true, 1., false),
        0x2524 => (true, false, true, true, 1., false),
        0x252c => (true, true, false, true, 1., false),
        0x2534 => (true, true, true, false, 1., false),
        0x253c => (true, true, true, true, 1., false),
        0x256d => (false, true, false, true, 1., true),
        0x256e => (true, false, false, true, 1., true),
        0x256f => (true, false, true, false, 1., true),
        0x2570 => (false, true, true, false, 1., true),
        0x2574 => (true, false, false, false, 1., false),
        0x2575 => (false, false, true, false, 1., false),
        0x2576 => (false, true, false, false, 1., false),
        0x2577 => (false, false, false, true, 1., false),
        _ => return None,
    };
    Some((l, r, u, d, thick, round))
}

/// The arms of a line-drawing character, drawn in the cell that begins
/// at `x0`: each from the cell's middle to its edge.
fn line_parts((l, r, u, d, thick, round): (bool, bool, bool, bool, f32, bool), x0: f32, cell_w: f32, lh: f32, ink: Hsla) -> Vec<AnyElement> {
    let (cx, cy) = ((x0 + cell_w / 2. - thick / 2.).round(), (lh / 2. - thick / 2.).round());
    let (left, right) = (cx - x0, x0 + cell_w - cx);
    let at = |x: f32, y: f32, w: f32, h: f32| div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h));
    if round {
        // A quarter of a ring: two edges of a box with one round corner.
        let radius = px((cell_w / 2.).floor());
        let (x, w) = if r { (cx, right) } else { (x0, left + thick) };
        let (y, h) = if d { (cy, lh - cy) } else { (0., cy + thick) };
        let b = at(x, y, w, h).border_color(ink);
        let b = match (r, d) {
            (true, true) => b.border_l(px(thick)).border_t(px(thick)).rounded_tl(radius),
            (false, true) => b.border_r(px(thick)).border_t(px(thick)).rounded_tr(radius),
            (false, false) => b.border_r(px(thick)).border_b(px(thick)).rounded_br(radius),
            (true, false) => b.border_l(px(thick)).border_b(px(thick)).rounded_bl(radius),
        };
        return vec![b.into_any_element()];
    }
    let mut out = Vec::new();
    if l || r {
        let (x, w) = (if l { x0 } else { cx }, if l && r { cell_w + 0.5 } else if l { left + thick } else { right + 0.5 });
        out.push(at(x, cy, w, thick).bg(ink).into_any_element());
    }
    if u || d {
        let (y, h) = (if u { 0. } else { cy }, if u && d { lh } else if u { cy + thick } else { lh - cy });
        out.push(at(cx, y, thick, h).bg(ink).into_any_element());
    }
    out
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
