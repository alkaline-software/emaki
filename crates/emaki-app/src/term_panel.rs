//! The window's two terminals. The shell terminal stands at the
//! conversation's right, under the top strip, opened with the strip's
//! button: a shell of the person's in the session's folder, a tab each.
//! The agent terminal is the CLI layout, in the conversation's place:
//! the session's own agent as it runs in the hidden terminal
//! (`emaki_core::pty`), the same screen the window reads and types into.
//!
//! Both are a `Pty`: a child on a pty this process owns and a screen
//! model that draws nothing. Here the screen is drawn, a row a line in
//! the mono face, the keyboard is written to the child, and the pty is
//! kept the size of its room. Each keeps its own `TermView`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use emaki_core::model::AgentId;
use emaki_core::transcript::SessionRef;
use emaki_core::pty::{self, Pty};

use crate::i18n::{phrase, t, tf};
use crate::workbench::{swallow_click, Press, term_bytes, MenuDo, Page, TabGhost, CloseTab, TermBackTab, TermClear, TermNewTab, TermTab, Workbench, TERMINAL_CONTEXT};

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
/// How long the two layouts take to trade places, and how far each card
/// slides as it comes or goes: the CLI layout stands to the rendered
/// one's right.
pub(crate) const CLI_ANIM: Duration = Duration::from_millis(320);
pub(crate) const CLI_SHUFFLE: f32 = 80.;
/// How often a panel that shows looks for something new on its screen.
const POLL: Duration = Duration::from_millis(33);
/// How often the agents' page looks again while the shell is beside it.
const LOOK_AGAIN: Duration = Duration::from_secs(4);
/// A drag held past the screen's edge scrolls it this often, by so many
/// rows a second at the edge, so many more for each pixel further out,
/// and no more than so many.
const EDGE_TICK: Duration = Duration::from_millis(16);
const EDGE_ROWS: f32 = 10.;
const EDGE_MORE: f32 = 1.2;
const EDGE_MOST: f32 = 120.;
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
    /// person's doing and stays empty while the side shows. Brought up
    /// again empty, the side makes one as it did at first (`term_go`).
    begun: bool,
    /// Why the last shell did not start.
    failed: Option<String>,
}

impl Shells {
    fn current(&self) -> Option<&ShellTab> {
        self.tabs.iter().find(|t| t.id == self.on)
    }
}

/// What is selected: where it began and where it runs to, each (line,
/// column). A line is counted from the screen's top row with nothing
/// scrolled back, and is less than 0 for what has left the screen, so a
/// selection keeps its place while the screen scrolls under it. By the
/// letter, the word (a double click) or the row (a triple), each end a
/// cell; or from the keyboard, each end a place between two cells, as a
/// caret is (`unit` 0). A press that never moved selects nothing.
#[derive(Clone, Copy)]
pub(crate) struct Sel {
    anchor: (i64, u16),
    head: (i64, u16),
    unit: usize,
    /// The same columns of every row (a drag with ⌥), not a run of text.
    block: bool,
}

/// Kaku's `selection_word_boundary`: what a word is made of.
fn word_char(c: char) -> bool {
    !c.is_whitespace() && c != '\0' && !"{}[]()\"'-".contains(c)
}

impl Sel {
    /// The two ends, the earlier first.
    fn ends(&self) -> ((i64, u16), (i64, u16)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// The columns selected in each of `cells`, the lines from `top` on,
    /// from and up to.
    fn spans(&self, top: i64, cells: &[Vec<char>]) -> Vec<Option<(usize, usize)>> {
        let (a, b) = self.ends();
        let mut out = vec![None; cells.len()];
        if self.unit <= 1 && a == b {
            return out;
        }
        let at = |l: i64| usize::try_from(l - top).ok().filter(|i| *i < cells.len());
        let last = top + cells.len() as i64 - 1;
        if self.block {
            let (from, to) = (self.anchor.1.min(self.head.1) as usize, self.anchor.1.max(self.head.1) as usize + 1);
            for l in a.0.max(top)..=b.0.min(last) {
                out[at(l).unwrap()] = Some((from, to));
            }
            return out;
        }
        let (mut from, mut to) = (a.1 as usize, b.1 as usize + (self.unit > 0) as usize);
        match self.unit {
            2 => {
                if let Some(row) = at(a.0).map(|i| &cells[i]) {
                    while from > 0 && from <= row.len() && row.get(from).is_some_and(|c| word_char(*c)) && word_char(row[from - 1]) {
                        from -= 1;
                    }
                }
                if let Some(row) = at(b.0).map(|i| &cells[i]) {
                    while to < row.len() && word_char(row[to - 1]) && word_char(row[to]) {
                        to += 1;
                    }
                }
            }
            3 => (from, to) = (0, usize::MAX),
            _ => {}
        }
        for l in a.0.max(top)..=b.0.min(last) {
            let i = at(l).unwrap();
            let (f, t) = (if l == a.0 { from } else { 0 }, if l == b.0 { to } else { usize::MAX });
            // A row with nothing on it still shows that it is selected.
            let t = t.min(cells[i].len().max(1));
            if f < t {
                out[i] = Some((f, t));
            }
        }
        out
    }
}

/// What is being typed, as the composer's text is to its caret: the
/// rows from where it begins to where it ends, every line of it, where
/// the typing begins on the first (after the prompt), and which rows run
/// on into the next as one paragraph. Rows are lines counted from the
/// screen's top row. Claude Code's input is one paragraph: its rows
/// carry no sign of which were wrapped.
struct Typing {
    first: i64,
    rows: Vec<Vec<char>>,
    start: (i64, u16),
    runs: Vec<bool>,
    boxed: bool,
}

impl Typing {
    /// Whether a place of the screen is in what is being typed.
    fn holds(&self, at: (i64, u16)) -> bool {
        at.0 >= self.first && at.0 < self.first + self.rows.len() as i64 && at >= self.start
    }

    /// The paragraph a line is in, from its start to its end.
    fn para(&self, line: i64) -> ((i64, u16), (i64, u16)) {
        let at = (line - self.first).clamp(0, self.rows.len() as i64 - 1).max(0) as usize;
        let (mut i, mut j) = (at, at);
        while i > 0 && self.runs[i - 1] {
            i -= 1;
        }
        while j + 1 < self.rows.len() && self.runs[j] {
            j += 1;
        }
        let ends = |k: usize| (self.first + k as i64, self.rows.get(k).map_or(0, Vec::len) as u16);
        ((ends(i).0, 0).max(self.start), ends(j))
    }
}

/// Where the cursor is once the program has drawn it after what was
/// sent: looked at until it has moved and holds still, or for long enough
/// that it will not move.
fn settled(pty: &Pty, was: (u16, u16)) -> (u16, u16) {
    let began = Instant::now();
    let (mut last, mut still) = (was, 0);
    while began.elapsed() < Duration::from_millis(250) {
        std::thread::sleep(Duration::from_millis(6));
        let now = pty.cursor().unwrap_or(last);
        if now != last {
            (last, still) = (now, 0);
        } else if now != was || began.elapsed() > Duration::from_millis(80) {
            still += 1;
            if still >= 3 {
                break;
            }
        }
    }
    last
}

/// The cursor steered to a place with arrow keys, as the composer's caret
/// goes to a click: sent, looked at, and sent again until it is there or
/// goes no further. A letter is an arrow, but the program says where its
/// letters are (a new line, a wrapped row, an indented one), so where the
/// cursor goes is read, not worked out. Left and Right only: Up at the
/// first line is the shell's history. Where it ended.
fn steer(pty: &Pty, to: (u16, u16)) -> (u16, u16) {
    let Some(mut at) = pty.cursor() else { return to };
    let row_len = |r: u16| pty.rows_back(0).0.get(r as usize).map(|row| row_cells(row).len() as u16).unwrap_or(0);
    for _ in 0..64 {
        if at == to {
            break;
        }
        let on = at < to;
        // Along a row by its cells; to another row, past the end of this
        // one, or back past its start.
        let n = if at.0 == to.0 { at.1.abs_diff(to.1) } else if on { row_len(at.0).saturating_sub(at.1) + 1 } else { at.1 + 1 };
        let key: &[u8] = match (on, pty.app_cursor()) {
            (true, false) => b"\x1b[C",
            (true, true) => b"\x1bOC",
            (false, false) => b"\x1b[D",
            (false, true) => b"\x1bOD",
        };
        pty.write(&key.repeat(n.max(1) as usize));
        let now = settled(pty, at);
        if now == at {
            break;
        }
        at = now;
    }
    at
}

/// How many letters lie from the cursor to a place after it, counted
/// by walking there with Right, in steps that stay inside the text: half
/// the cells to it on its row (a letter takes one or two), else half of
/// what is left of the cursor's row, and one at a time near it. Each step
/// that moved is that many letters, a new line or a wrapped row's end
/// one. The cursor ends there, or where the text ends before it.
fn letters_to(pty: &Pty, to: (u16, u16)) -> usize {
    let Some(mut at) = pty.cursor() else { return 0 };
    let row_len = |r: u16| pty.rows_back(0).0.get(r as usize).map(|row| row_cells(row).len() as u16).unwrap_or(0);
    let mut letters = 0;
    for _ in 0..2000 {
        if at >= to {
            break;
        }
        let n = if at.0 == to.0 { (to.1 - at.1) / 2 } else { row_len(at.0).saturating_sub(at.1) / 2 }.max(1);
        let key: &[u8] = if pty.app_cursor() { b"\x1bOC" } else { b"\x1b[C" };
        pty.write(&key.repeat(n as usize));
        let now = settled(pty, at);
        if now == at || now > to && n > 1 {
            break;
        }
        letters += n as usize;
        at = now;
    }
    letters
}

/// What one terminal on the screen keeps of its own: the shell terminal
/// at the right and the agent terminal in the CLI layout are one each,
/// and both can show at once.
pub(crate) struct TermView {
    pub(crate) focus: FocusHandle,
    /// Where its screen was last drawn, and the rows and columns that fit.
    pub(crate) bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) size: Rc<Cell<(u16, u16)>>,
    /// The CLI layout's whole pane, the room at the screen's sides with
    /// it: the wheel over that room is the screen's.
    room: Rc<Cell<Bounds<Pixels>>>,
    /// How many rows back it is scrolled, and what of the wheel is left
    /// over from the last whole row.
    back: usize,
    wheel: f32,
    /// A cell's width, as last drawn.
    cell_w: f32,
    /// What was typed while the cursor was being steered (`term_edit`),
    /// sent once it is there; None while nothing is.
    edit: Arc<Mutex<Option<Vec<u8>>>>,
    /// The button held on a program that hears the mouse, and the cell
    /// last told to it.
    mouse: Option<u8>,
    mouse_cell: (u16, u16),
    /// What is selected, whether the drag that selects is still held,
    /// and when a selection was last copied.
    pub(crate) sel: Option<Sel>,
    selecting: bool,
    copied: Option<Instant>,
    /// The room around the screen as last drawn: at the sides and the
    /// top, and at the foot.
    pad: (f32, f32),
    /// Where the cursor was at the last draw and since when, which is
    /// when its blinking began again; whether it was drawn; and whether
    /// it had the keyboard.
    blink: ((u16, u16), Instant),
    blink_on: bool,
    focused: bool,
    /// The link under the pointer.
    link: Option<Link>,
    /// The cell the pointer was last over, column and row.
    hover: (u16, u16),
    /// A drag that selects, held past the screen's top (less than 0) or
    /// foot, by so many pixels, and the column it is over: the screen
    /// scrolls on under it while it is there. What of a row is left over
    /// from the last step, and whether the steps are running.
    edge: f32,
    edge_col: u16,
    edge_rows: f32,
    edge_run: bool,
}

impl TermView {
    pub(crate) fn new(cx: &mut App) -> Self {
        TermView {
            focus: cx.focus_handle(),
            bounds: Rc::new(Cell::new(Bounds::default())),
            size: Rc::new(Cell::new((0, 0))),
            room: Rc::new(Cell::new(Bounds::default())),
            back: 0,
            wheel: 0.,
            cell_w: 0.,
            edit: Arc::new(Mutex::new(None)),
            mouse: None,
            mouse_cell: (0, 0),
            sel: None,
            selecting: false,
            copied: None,
            pad: (0., 0.),
            blink: ((0, 0), Instant::now()),
            blink_on: true,
            focused: false,
            link: None,
            hover: (u16::MAX, u16::MAX),
            edge: 0.,
            edge_col: 0,
            edge_rows: 0.,
            edge_run: false,
        }
    }
}

/// A choice of the panel's own menu, opened with a right click.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TermDo {
    Copy,
    Paste,
    SelectAll,
    Clear,
    NewTab,
    CloseTab,
}

impl TermDo {
    pub(crate) fn icon(self) -> &'static str {
        match self {
            TermDo::Copy => "icons/copy.svg",
            TermDo::Paste => "icons/clipboard.svg",
            TermDo::SelectAll => "icons/select-all.svg",
            TermDo::Clear => "icons/eraser.svg",
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

/// How many columns of a row stand before its `chars`-th character, by
/// the screen model's own cells.
pub(crate) fn cells_before(row: &[pty::Span], chars: usize) -> usize {
    let (mut seen, mut cols) = (0usize, 0usize);
    for (cell, w) in row.iter().flat_map(|s| s.each_cell()) {
        seen += cell.chars().count();
        if seen > chars {
            break;
        }
        cols += w as usize;
    }
    cols
}

/// A row as its cells hold it: a cell's first character, and a wide
/// one's second column a NUL.
fn row_cells(row: &[pty::Span]) -> Vec<char> {
    let mut out = Vec::new();
    for (cell, w) in row.iter().flat_map(|s| s.each_cell()) {
        out.push(cell.chars().next().unwrap_or(' '));
        if w > 1 {
            out.push('\0');
        }
    }
    out
}

/// Paths as a shell reads them, a space between: each character a shell
/// would read as its own after a backslash, as a Mac's terminals type a
/// dropped file; in double quotes for PowerShell where one needs them.
fn path_words(paths: &[std::path::PathBuf]) -> String {
    let word = |p: &std::path::PathBuf| {
        let p = p.to_string_lossy();
        if cfg!(windows) {
            return if p.contains([' ', '\'', '&', '(', ')', ';', ',']) { format!("\"{p}\"") } else { p.into_owned() };
        }
        p.chars().fold(String::new(), |mut out, c| {
            if c.is_whitespace() || "\\'\"()[]{}&;|<>*?$`!#".contains(c) {
                out.push('\\');
            }
            out.push(c);
            out
        })
    };
    paths.iter().map(word).collect::<Vec<_>>().join(" ")
}

/// A shell's tab being dragged along the row.
#[derive(Clone)]
struct DragShell(u64);

/// The name the agents page's shells are kept under, where a session's
/// are kept under its id: no session's id has a tilde in it.
const AGENTS_SHELLS: &str = "~agents";

impl Workbench {
    /// Whether the panel is drawn: asked for, on a conversation.
    pub(crate) fn term_panel_shown(&self) -> bool {
        self.term_on() && self.term_stage()
    }

    /// Whether the page showing can have the panel: a conversation, or
    /// inside an agent on the agents page.
    fn term_stage(&self) -> bool {
        (self.page == Page::Session && self.detail.is_some()) || self.term_at_agents()
    }

    /// Inside an agent on the agents page, where the panel is a shell in
    /// the home folder and nothing else: there is no session there, so
    /// no agent's terminal to show.
    fn term_at_agents(&self) -> bool {
        self.page == Page::Agents && self.agent_open.is_some()
    }

    /// Whose terminals the panel shows: the session showing, or, inside
    /// an agent on the agents page, a session that is none, in the home
    /// folder, which keeps that page's shells under a name no session
    /// has.
    pub(crate) fn term_owner(&self) -> Option<SessionRef> {
        if self.term_at_agents() {
            return Some(SessionRef { session_id: AGENTS_SHELLS.into(), cwd: emaki_core::paths::home().to_string_lossy().to_string(), ..Default::default() });
        }
        self.selected_ref().cloned()
    }

    /// Whether the shell terminal is asked for, beside a conversation or
    /// inside an agent on the agents page, which keeps its own choice.
    pub(crate) fn term_on(&self) -> bool {
        if self.term_at_agents() {
            return self.agents_shell;
        }
        self.side_term
    }

    /// Whether the CLI layout is drawn: asked for, on a conversation
    /// that has an agent to show. A session begun here with nothing sent
    /// has none yet and keeps its composer.
    pub(crate) fn cli_shown(&self) -> bool {
        self.cli_layout && self.page == Page::Session && self.detail.is_some() && self.selected_ref().is_some_and(|r| !self.is_draft(&r.session_id))
    }

    /// Whether the session showing can have the CLI layout: not one begun
    /// here with nothing sent.
    pub(crate) fn cli_possible(&self) -> bool {
        self.page == Page::Session && self.detail.is_some() && self.selected_ref().is_some_and(|r| !self.is_draft(&r.session_id))
    }

    /// One of the two terminals' own state: the agent's, or the shell's.
    pub(crate) fn view(&self, agent: bool) -> &TermView {
        if agent { &self.agent_view } else { &self.shell_view }
    }

    pub(crate) fn view_mut(&mut self, agent: bool) -> &mut TermView {
        if agent { &mut self.agent_view } else { &mut self.shell_view }
    }

    /// Which terminal has the keyboard: the agent's is `true`.
    pub(crate) fn term_focused(&self, window: &Window) -> Option<bool> {
        if self.shell_view.focus.is_focused(window) {
            Some(false)
        } else if self.agent_view.focus.is_focused(window) {
            Some(true)
        } else {
            None
        }
    }

    /// The shell terminal's button was pressed: it comes, or goes.
    pub(crate) fn toggle_term(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let to = !self.term_on();
        self.term_go(to, cx);
        window.focus(if to { &self.shell_view.focus } else { &self.focus_handle }, cx);
    }

    /// ⌘⇧S, the button's key: where the panel can be.
    pub(crate) fn toggle_term_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.term_stage() {
            self.toggle_term(window, cx);
        }
    }

    /// The panel comes or goes, and the change is drawn.
    pub(crate) fn term_go(&mut self, to: bool, cx: &mut Context<Self>) {
        let from = self.term_on();
        if from == to {
            return;
        }
        // The agents page's shell is its own choice: it neither opens
        // the panel beside a conversation nor closes it.
        if self.term_at_agents() {
            self.agents_shell = to;
        } else {
            self.side_away.retain(|s| *s != crate::panels::Side::Term);
            self.side_term = to;
            if to {
                self.side_asked(crate::panels::Side::Term, cx);
            }
        }
        // Brought up with no tab in it, it opens on a new one, as it did
        // the first time it was looked at.
        if to {
            if let Some(set) = self.term_owner().and_then(|r| self.shells.get_mut(&r.session_id)).filter(|s| s.tabs.is_empty()) {
                set.begun = false;
            }
        }
        self.side_term_anim = Some((from, to, Instant::now(), self.side_term_anim.map(|(_, _, _, n)| n + 1).unwrap_or(0)));
        self.shell_view.back = 0;
        self.shell_view.sel = None;
        if to {
            self.term_panel_watch(cx);
            self.warm_now();
        } else {
            // The panel left is drawn while it goes: once more when it has.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TERM_PANEL_ANIM + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        self.save_ui(true);
        cx.notify();
    }

    /// The layout slider was moved, or ⌘⇧L pressed: the agent terminal
    /// takes the conversation's place, or gives it back. The keyboard
    /// goes with it, to the terminal or to the composer.
    pub(crate) fn toggle_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.cli_possible() {
            return;
        }
        let to = !self.cli_layout;
        self.layout_go(to, cx);
        if to {
            window.focus(&self.agent_view.focus, cx);
        } else {
            let composer = self.composer.read(cx).focus_handle(cx);
            window.focus(&composer, cx);
        }
    }

    /// The layout goes to the CLI's or back to the rendered one. The
    /// change is drawn where it is seen (`layout_seen`).
    pub(crate) fn layout_go(&mut self, cli: bool, cx: &mut Context<Self>) {
        if self.cli_layout == cli {
            return;
        }
        self.cli_layout = cli;
        self.agent_view.back = 0;
        self.agent_view.sel = None;
        self.end_scroll_gesture();
        if cli {
            self.term_panel_watch(cx);
            self.warm_now();
        }
        self.save_ui(true);
        cx.notify();
    }

    /// Whether the layout drawn for a session changed since its last draw,
    /// by the slider or by the session itself (a first message sent from
    /// the composer): the one comes in over the other's place as the
    /// other goes, the outline inside the rendered layout, and fading
    /// with it. A change of tab plays nothing.
    pub(crate) fn layout_seen(&mut self, key: &str, cx: &mut Context<Self>) {
        let now = self.cli_shown();
        let was = match &self.cli_seen {
            Some((k, was)) if k == key => *was,
            _ => now,
        };
        self.cli_seen = Some((key.to_string(), now));
        if now {
            self.term_watch_on(cx);
        }
        if was == now {
            return;
        }
        // The outline goes and comes inside the rendered layout's fade,
        // not by its own; back, it brings the marked entry into sight.
        self.outline_at = None;
        self.cli_anim = Some((now, Instant::now(), self.cli_anim.map(|(_, _, n)| n + 1).unwrap_or(0)));
        // What goes is drawn while it goes: once more when it has.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CLI_ANIM + Duration::from_millis(20)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// "Open in CLI Layout" on a session's menu: the agent terminal takes
    /// the conversation's place, with the keyboard.
    pub(crate) fn open_term_panel_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout_go(true, cx);
        window.focus(&self.agent_view.focus, cx);
    }

    /// A probe's step (`EMAKI_GO`), which has no window to hand. One that
    /// acts on a screen acts on the agent's in the CLI layout and on the
    /// shell's otherwise.
    pub(crate) fn term_panel_probe(&mut self, step: &str, cx: &mut Context<Self>) {
        let agent = self.cli_shown();
        match step {
            "term" | "term:shell" => self.term_go(true, cx),
            "term:agent" | "layout:cli" => self.layout_go(true, cx),
            "layout:rendered" => self.layout_go(false, cx),
            "term:off" => self.term_go(false, cx),
            "term:start" => {
                self.layout_go(true, cx);
                self.start_hidden_terminal(cx);
            }
            "term:tab+" => self.shell_new(None, cx),
            "term:fit" => self.side_term_w = TERM_W,
            // The panel's edge dragged to that width, as far as it goes.
            _ if step.starts_with("term:w:") => {
                if let Ok(w) = step["term:w:".len()..].parse::<f32>() {
                    self.side_term_w = px(w).max(TERM_MIN);
                    cx.notify();
                }
            }
            "term:copied" => self.view_mut(agent).copied = Some(Instant::now()),
            _ if step.starts_with("term:hover:") => {
                // term:hover:<row>,<col>
                let n: Vec<u16> = step["term:hover:".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
                if let ([row, col], Some(pty)) = (&n[..], self.term_pty_of(agent)) {
                    self.view_mut(agent).link = self.term_link_at(agent, &pty, *row, *col);
                }
            }
            _ if step.starts_with("term:text:") => self.cfg.terminal.size = step["term:text:".len()..].parse().unwrap_or(13.),
            "term:clear" => self.term_clear(agent),
            _ if step.starts_with("term:sel:") => {
                // term:sel:<unit>,<line>,<col>,<line>,<col>
                let n: Vec<i64> = step["term:sel:".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
                if let [unit, l1, c1, l2, c2] = n[..] {
                    self.view_mut(agent).sel = Some(Sel { anchor: (l1, c1 as u16), head: (l2, c2 as u16), unit: unit as usize % 4, block: unit >= 4 });
                    eprintln!("emaki: selected {:?}", self.term_sel_text(agent));
                }
            }
            "term:tab-" => {
                if let Some(id) = self.term_owner().and_then(|r| self.shells.get(&r.session_id).map(|s| s.on)) {
                    self.shell_close(id, cx);
                }
            }
            _ if step.starts_with("term:tab:") => {
                let ix = step["term:tab:".len()..].parse::<usize>().unwrap_or(0);
                if let Some(id) = self.term_owner().and_then(|r| self.shells.get(&r.session_id).and_then(|s| s.tabs.get(ix).map(|t| t.id))) {
                    self.shell_pick(id, cx);
                }
            }
            // termput:<text> types without Return; termcursor prints where
            // the cursor is, the row it is on and where the cells are.
            _ if step.starts_with("termput:") => {
                if let Some(pty) = self.term_pty_of(agent) {
                    pty.write(step["termput:".len()..].as_bytes());
                }
            }
            "termcursor" => {
                if let Some(pty) = self.term_pty_of(agent) {
                    let row = pty.cursor().map(|(r, _)| r as usize).and_then(|r| pty.rows_back(0).0.get(r).map(|s| s.iter().map(|x| x.text.as_str()).collect::<String>()));
                    let v = self.view(agent);
                    eprintln!("emaki: term cursor {:?} row {:?} bounds {:?} pad {:?} cell_w {} line_h {} edits {} typing {:?}", pty.cursor(), row, v.bounds.get(), v.pad, v.cell_w, self.term_line_h(), pty.edits_its_line(), pty.typing_start());
                }
            }
            _ => {
                if let Some(pty) = step.strip_prefix("termtype:").and_then(|_| self.term_pty_of(agent)) {
                    // Return comes a moment after the words: Codex's
                    // terminal takes the two in one burst for a paste,
                    // and a pasted Return is a new line.
                    pty.write(step["termtype:".len()..].as_bytes());
                    cx.spawn(async move |_, cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
                        pty.write(b"\r");
                    })
                    .detach();
                }
            }
        }
        cx.notify();
    }

    /// The shell terminal's button at the top strip's right end, as the
    /// files panel's is at its left: its icon alone, with the accent's
    /// line under it while the panel shows. Inside an agent on the agents
    /// page it opens a shell in the home folder.
    pub(crate) fn term_shell_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.term_on();
        let (was, serial) = self.side_term_anim.map(|(from, _, at, n)| (crate::workbench::pressed_from(from, on, at), n + 1)).unwrap_or((on, 0));
        let tip = crate::shortcuts::keyed(if self.term_at_agents() { t("Shell Terminal in your home folder (⌘⇧S)") } else { t("Shell Terminal (⌘⇧S)") }, "terminal");
        self.toggle_button("term-shell", "icons/terminal.svg", &tip, (was, on, serial), Rc::new(|this, window, cx| this.toggle_term(window, cx)), cx)
    }

    /// While the panel shows, the window is drawn again whenever the
    /// screen it shows has changed: a pty writes from a thread of its
    /// own and says nothing to the window.
    fn term_panel_watch(&mut self, cx: &mut Context<Self>) {
        self.term_watching = true;
        self.side_term_poll = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let on = this.update(cx, |this, cx| {
                let (shell, agent) = (this.term_on(), this.cli_layout);
                if !shell && !agent {
                    this.term_watching = false;
                    return false;
                }
                // Beside an agent's page, the page looks again now and then
                // for what the commands run here have installed.
                if this.term_at_agents() && this.agent_open.is_some() && !this.agents_checking && this.agents_checked.is_none_or(|at| at.elapsed() > LOOK_AGAIN) {
                    this.look_for_agents(cx);
                }
                // The cursor's blink is a draw too, while a terminal has the
                // keyboard. A frame, not a change: the terminals are drawn
                // afresh at every frame, and what the window keeps between
                // frames is not.
                let changed = |this: &Self, agent: bool| {
                    let v = this.view(agent);
                    let blink = v.focused && this.cfg.terminal.cursor_blink && this.blink_on(agent) != v.blink_on;
                    blink || this.term_pty_of(agent).is_some_and(|p| p.quiet_for() < POLL * 4 || !p.alive())
                };
                if (shell && changed(this, false)) || (agent && this.cli_shown() && changed(this, true)) {
                    crate::ambient::redraw(&this.ambient, cx);
                }
                true
            });
            if !on.unwrap_or(false) {
                break;
            }
        }));
    }

    /// The watch, started where it is not running: a terminal drawn with
    /// nothing having asked for it, as the CLI layout at launch is.
    fn term_watch_on(&mut self, cx: &mut Context<Self>) {
        if !self.term_watching {
            self.term_panel_watch(cx);
        }
    }

    /// Whether the cursor is in the lit half of its blink: on and off by
    /// turns, with no fade, from when it last moved.
    fn blink_on(&self, agent: bool) -> bool {
        (self.view(agent).blink.1.elapsed().as_millis() / BLINK.as_millis()) % 2 == 0
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

    /// The room around a terminal's screen the settings ask for, at its
    /// sides and at its foot, in the screen's own pixels as Kaku counts it.
    pub(crate) fn term_pad(&self, window: &Window) -> (f32, f32) {
        let scale = window.scale_factor().max(1.);
        match self.cfg.terminal.padding.as_str() {
            "compact" => (10., 8.),
            "roomy" => (if scale >= 2. { 20. } else { 26. }, 16. / scale),
            _ => (15., 8.),
        }
    }

    /// The terminals' ground, as the settings' picture of the window
    /// draws the terminal panel.
    pub(crate) fn term_ground(&self, cx: &App) -> Hsla {
        hsla(self.term_scheme(cx).bg)
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
    /// the panel draws a screen: the same face, rows, shapes and color
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

    /// The pty of one side, when there is one.
    fn term_pty_of(&self, agent: bool) -> Option<Arc<Pty>> {
        let r = self.term_owner()?;
        if agent {
            self.hub.agent_screen(&r.session_id)
        } else {
            self.shells.get(&r.session_id).and_then(|s| s.current()).map(|t| t.pty.clone())
        }
    }

    /// A new shell for the session, in a tab of its own that then shows:
    /// the person's login shell in the session's folder (their home when
    /// the folder is gone), with no `CLAUDE*` variable of ours in it.
    /// `at` is where in the row, the end when none.
    pub(crate) fn shell_new(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        let Some(r) = self.term_owner() else { return };
        let cwd = if Self::folder_exists(&r) { r.cwd.clone() } else { emaki_core::paths::home().to_string_lossy().to_string() };
        // A shell whose prompt marks where what is typed begins.
        let (argv, env) = emaki_core::shell_marks::shell_command();
        let made = Pty::spawn_with(&argv, &env, &cwd, Arc::new(|| {})).inspect(|p| p.reflow_on_resize());
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
        let Some(sid) = self.term_owner().map(|r| r.session_id) else { return };
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
        let Some(sid) = self.term_owner().map(|r| r.session_id) else { return };
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
        self.shell_view.sel = None;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(until).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        self.shell_view.back = 0;
        self.shell_view.wheel = 0.;
    }

    /// A tab dragged along the row takes the place the pointer is over.
    fn shell_drag_to(&mut self, id: u64, x: Pixels, row: Bounds<Pixels>, tab_w: f32, cx: &mut Context<Self>) {
        if self.shell_drag != Some(id) {
            self.shell_drag = Some(id);
            cx.notify();
        }
        let Some(sid) = self.term_owner().map(|r| r.session_id) else { return };
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
        let name = pty::shell_argv().first().and_then(|s| std::path::Path::new(s).file_stem().map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "shell".into());
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
                // the bold weight, the others gray until the pointer is
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
                    window.focus(&this.shell_view.focus, cx);
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
            .managed_tooltip(|window, cx| crate::workbench::tip(t("New shell (⌘T)"), window, cx))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.shell_new(None, cx);
                window.focus(&this.shell_view.focus, cx);
            }))
            .child(Icon::default().path("icons/plus.svg").with_size(px(12.)));
        h_flex().min_w_0().h_full().items_center().gap(px(TAB_GAP)).child(row).child(plus)
        .into_any_element()
    }

    /// The panel's width as drawn: what it was dragged to, less when the
    /// conversation would be left with too little.
    pub(crate) fn term_panel_w(&self) -> Pixels {
        let beside = self.left_w() + self.file_pane_least() + self.outline_least();
        self.side_term_w.min(self.pane_w - beside - crate::panels::CONVERSATION_MIN).max(TERM_MIN)
    }

    /// The pointer moved with the panel's edge held.
    pub(crate) fn term_drag_to(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            if std::mem::take(&mut self.side_term_drag) {
                self.save_ui(true);
            }
            return;
        }
        let beside = self.left_w() + self.file_pane_least() + self.outline_least();
        // Its edge is at its left, or with the side panels mirrored at
        // its right.
        let from = if self.files_right() { e.position.x - self.content_left() } else { self.view_w - e.position.x };
        let w = from.clamp(TERM_MIN, (self.pane_w - beside - crate::panels::CONVERSATION_MIN).max(TERM_MIN));
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
                .bottom(crate::workbench::FRAME)
                .map(|d| if self.files_right() { d.left(self.content_left() + self.term_panel_w() - TERM_GRIP / 2.) } else { d.right(crate::workbench::FRAME + self.term_panel_w() - TERM_GRIP / 2.) })
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
    pub(crate) fn term_panel_wheel(&mut self, agent: bool, delta_y: Pixels, at: Point<Pixels>, mods: &Modifiers) {
        let Some(pty) = self.term_pty_of(agent) else { return };
        self.view_mut(agent).wheel += f32::from(delta_y) / WHEEL_ROW;
        let lines = self.view_mut(agent).wheel.trunc() as i64;
        if lines == 0 {
            return;
        }
        self.view_mut(agent).wheel -= lines as f32;
        // A program that hears the mouse hears the wheel as its own.
        if let Some(want) = pty.mouse().filter(|_| self.view_mut(agent).back == 0) {
            let (col, row) = self.term_cell(agent, at, &pty);
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
            self.view_mut(agent).back = (self.view_mut(agent).back as i64 + lines).max(0) as usize;
            self.term_unhover(agent);
        }
    }

    /// The cell of the screen a point of the window is over: column, row,
    /// held inside the screen.
    fn term_cell(&self, agent: bool, at: Point<Pixels>, pty: &Pty) -> (u16, u16) {
        let b = self.view(agent).bounds.get();
        let (rows, cols) = pty.size();
        let col = (f32::from(at.x - b.left()) - self.view(agent).pad.0) / self.view(agent).cell_w.max(1.);
        let row = (f32::from(at.y - b.top()) - self.view(agent).pad.0) / self.term_line_h();
        (col.floor().clamp(0., cols.saturating_sub(1) as f32) as u16, row.floor().clamp(0., rows.saturating_sub(1) as f32) as u16)
    }

    /// A button pressed in the panel. A program that asked to hear the
    /// mouse is told, unless Shift is held, which keeps the press for
    /// the panel as terminals do. Otherwise the left button begins a
    /// selection (by the letter, and by the word or the row on a double
    /// or triple click; a click that selects nothing puts the cursor
    /// there, `term_mouse_up`), and opens the link under it with ⌘.
    fn term_mouse_down(&mut self, agent: bool, e: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(pty) = self.term_pty_of(agent) else { return };
        let (col, row) = self.term_cell(agent, e.position, &pty);
        // ⌘ and a click opens the link under the pointer, in a program
        // that hears the mouse too; Ctrl off a Mac.
        if e.button == MouseButton::Left && e.modifiers.secondary() {
            if let Some(link) = self.term_link_at(agent, &pty, row, col) {
                match link.target {
                    Target::Url(url) => cx.open_url(&url),
                    Target::File(path) => crate::sys::open_path(&path),
                }
            }
            return;
        }
        let want = pty.mouse().filter(|_| pty.alive() && self.view_mut(agent).back == 0);
        if let (Some(want), false, Some(button)) = (want, e.modifiers.shift, button_code(e.button)) {
            self.view_mut(agent).sel = None;
            self.view_mut(agent).mouse = Some(button);
            self.view_mut(agent).mouse_cell = (col, row);
            pty.write(&pty::mouse_bytes(button | mod_bits(&e.modifiers), col, row, true, want.form));
            return;
        }
        match e.button {
            MouseButton::Left => {}
            MouseButton::Right => {
                let mut items = Vec::new();
                if !self.term_sel_text(agent).is_empty() {
                    items.push((phrase("Copy"), MenuDo::Term(TermDo::Copy)));
                }
                items.push((phrase("Paste"), MenuDo::Term(TermDo::Paste)));
                items.push((phrase("Select All"), MenuDo::Term(TermDo::SelectAll)));
                if !agent {
                    items.push(("", MenuDo::Rule));
                    items.push((phrase("New Tab"), MenuDo::Term(TermDo::NewTab)));
                    if self.term_pty_of(agent).is_some() {
                        items.push((phrase("Close Tab"), MenuDo::Term(TermDo::CloseTab)));
                    }
                }
                // What empties the screen, apart.
                items.push(("", MenuDo::Rule));
                items.push((phrase("Clear"), MenuDo::Term(TermDo::Clear)));
                self.term_menu_agent = agent;
                self.open_menu(e.position, items, cx);
                return;
            }
            MouseButton::Middle => return self.term_paste(agent, cx),
            _ => return,
        }
        let line = self.term_line(agent, row);
        // With Shift the selection there is runs on to the press; one
        // made from the keyboard is by the letter from then on.
        if let Some(sel) = self.view_mut(agent).sel.as_mut().filter(|_| e.modifiers.shift) {
            if sel.unit == 0 {
                sel.unit = 1;
                if sel.anchor > sel.head {
                    sel.anchor.1 = sel.anchor.1.saturating_sub(1);
                }
            }
            sel.head = (line, col);
            self.view_mut(agent).selecting = true;
            return;
        }
        // ⌥ and a drag takes the same columns of every row.
        self.view_mut(agent).sel = Some(Sel { anchor: (line, col), head: (line, col), unit: e.click_count.clamp(1, 3), block: e.modifiers.alt });
        self.view_mut(agent).selecting = true;
    }

    /// What is being typed, when the program reading the keys edits its
    /// own line (`Pty::edits_its_line`): not where a program has the
    /// screen to itself, a command takes whole lines or none, or the
    /// screen is scrolled back. It begins where the prompt marked it
    /// (`shell_marks`) and runs to the last row written after it, every
    /// line of it; where nothing marked it, it is what stands between
    /// the rules over and under the cursor (Claude Code's input), else
    /// the cursor's line and the rows it wraps on.
    fn term_typing(&self, agent: bool, pty: &Pty) -> Option<Typing> {
        if !pty.alive() || self.view(agent).back != 0 || !pty.edits_its_line() {
            return None;
        }
        // A program with the screen to itself (an editor, a pager) has its
        // own keys for all of this, but for an input between two rules,
        // as Claude Code draws one there.
        let own_screen = pty.alt_screen();
        let (row, _) = pty.cursor()?;
        let (rows, cols) = pty.size();
        let cur = row as i64;
        let cells = |l: i64| pty.lines(l, l).1.first().map(|(r, _)| row_cells(r)).unwrap_or_default();
        let blank = |l: i64| cells(l).iter().all(|c| c.is_whitespace() || *c == '\0');
        let rule = |l: i64| cells(l).iter().filter(|c| **c == '─').count() >= 8;
        // A row runs on into the next when its text did, not when it
        // wraps for the blanks it was filled with (zsh's mark after output
        // with no newline): as the screen model reflows it.
        let wraps = |l: i64| pty.lines(l, l).1.first().is_some_and(|(r, w)| *w && row_cells(r).len() + 1 >= cols as usize);
        let foot = |mut l: i64| {
            while l + 1 < rows as i64 && !blank(l + 1) && !rule(l + 1) {
                l += 1;
            }
            l
        };
        let mut boxed = false;
        let (first, last, start) = match pty.typing_start().filter(|(l, _)| *l <= cur && !own_screen) {
            Some((l, c)) => (l, foot(cur), (l, c)),
            None => {
                let mut top = cur;
                while top > -(pty.kept_back() as i64) && !blank(top - 1) && !rule(top - 1) {
                    top -= 1;
                }
                if top > -(pty.kept_back() as i64) && rule(top - 1) && rule(foot(cur) + 1) {
                    boxed = true;
                    (top, foot(cur), (top, 0))
                } else if own_screen {
                    return None;
                } else {
                    let (mut first, mut last) = (cur, cur);
                    while first > 0 && wraps(first - 1) {
                        first -= 1;
                    }
                    while last < rows as i64 - 1 && wraps(last) {
                        last += 1;
                    }
                    (first, last, (first, 0))
                }
            }
        };
        let (first, lines) = pty.lines(first, last);
        let rows: Vec<Vec<char>> = lines.iter().map(|(r, _)| row_cells(r)).collect();
        let runs = (0..rows.len() as i64).map(|i| boxed || wraps(first + i)).collect();
        Some(Typing { first, rows, start, runs, boxed })
    }

    /// Something done to what is being typed by steering the cursor, off
    /// the window's thread: what is typed meanwhile waits and is sent
    /// once it is done, after `then`.
    fn term_edit(&mut self, agent: bool, pty: Arc<Pty>, job: impl FnOnce(&Pty) + Send + 'static, then: Vec<u8>) {
        let queue = Arc::clone(&self.view(agent).edit);
        {
            let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
            if q.is_some() {
                // One already under way: this one follows it.
                q.get_or_insert_with(Vec::new).extend(then);
                return;
            }
            *q = Some(Vec::new());
        }
        std::thread::spawn(move || {
            job(&pty);
            pty.write(&then);
            let waiting = queue.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
            pty.write(&waiting);
        });
    }

    /// Bytes for the terminal, or for after the cursor is where it is
    /// being steered to.
    fn term_send(&self, agent: bool, pty: &Pty, bytes: &[u8]) {
        let mut q = self.view(agent).edit.lock().unwrap_or_else(|e| e.into_inner());
        match q.as_mut() {
            Some(waiting) => waiting.extend_from_slice(bytes),
            None => pty.write(bytes),
        }
    }

    /// The cursor taken to a place in what is being typed, as the
    /// composer's caret goes to a click.
    fn term_caret_to(&mut self, agent: bool, pty: Arc<Pty>, to: (u16, u16)) {
        self.term_edit(agent, pty, move |p| {
            steer(p, to);
        }, Vec::new());
    }

    /// What is selected of what is being typed goes, as Backspace takes a
    /// selection in the composer, and `then` is typed in its place: a
    /// whole paragraph by the program's own keys for one (^E, ^U), as
    /// Kaku's ⌘A and Backspace, and part of one by steering the cursor to
    /// its start and counting the letters to its end. What the program
    /// will not delete (its prompt) stays. A selection over more than one
    /// paragraph is only copied: a terminal's line editors know a line at
    /// a time. Whether it went.
    fn term_delete_sel(&mut self, agent: bool, then: Vec<u8>) -> bool {
        let (Some(sel), Some(pty)) = (self.view(agent).sel, self.term_pty_of(agent)) else { return false };
        let Some(typing) = self.term_typing(agent, &pty) else { return false };
        let spans = sel.spans(typing.first, &typing.rows);
        let (Some(i), Some(j)) = (spans.iter().position(Option::is_some), spans.iter().rposition(Option::is_some)) else { return false };
        let a = (typing.first + i as i64, spans[i].unwrap().0 as u16).max(typing.start);
        let b = (typing.first + j as i64, (spans[j].unwrap().1.min(typing.rows[j].len().max(1))) as u16);
        let (from, to) = typing.para(a.0);
        if a >= b || !typing.holds(b) || typing.para(b.0) != (from, to) {
            return false;
        }
        let on_screen = |(l, c): (i64, u16)| (l.max(0) as u16, c);
        let here = pty.cursor().is_some_and(|(r, _)| (from.0..=to.0).contains(&(r as i64)));
        let (a, b) = (on_screen(a), on_screen(b));
        self.view_mut(agent).sel = None;
        if a <= on_screen(from) && b >= on_screen(to) {
            // Claude Code's ^U takes a row: each, from the input's end,
            // and the Backspace that joins it to the one before.
            let rows = typing.rows.len();
            let boxed = typing.boxed;
            self.term_edit(agent, pty, move |p| {
                if boxed {
                    steer(p, b);
                    p.write(&b"\x15\x7f".repeat(rows));
                } else {
                    if !here {
                        steer(p, a);
                    }
                    p.write(b"\x05\x15");
                }
            }, then);
            return true;
        }
        // Counted from its start to its end, then taken back by that many:
        // a program may move what it shows as the text changes (Claude
        // Code's input grows and shrinks from its foot), and a place on
        // the screen aimed at while deleting was another letter by then.
        self.term_edit(agent, pty, move |p| {
            steer(p, a);
            let n = letters_to(p, b);
            p.write(&b"\x7f".repeat(n));
        }, then);
        true
    }

    /// The link the cell is part of, when it is part of one.
    fn term_link_at(&self, agent: bool, pty: &Pty, row: u16, col: u16) -> Option<Link> {
        let (rows, _) = pty.rows_back(self.view(agent).back);
        let cwd = self.term_owner().map(|r| std::path::PathBuf::from(&r.cwd)).unwrap_or_default();
        let (from, to, target) = link_at(&row_cells(rows.get(row as usize)?), col as usize, &cwd)?;
        Some(Link { row, from, to, target })
    }

    /// What is on the clipboard is pasted at the prompt: its words, or a
    /// picture as the path of a file it is kept in, which Claude Code
    /// takes for a picture (`[Image #1]`) as it does one the window sends,
    /// or files copied in the Finder as their paths. A terminal takes
    /// words alone, and a screenshot pasted there did nothing.
    pub(crate) fn term_paste(&mut self, agent: bool, cx: &mut Context<Self>) {
        use gpui::ClipboardEntry;
        let Some(pty) = self.term_pty_of(agent).filter(|p| p.alive()) else { return };
        let Some(item) = cx.read_from_clipboard() else { return };
        let dir = emaki_core::paths::uploads_dir(&self.term_owner().map(|r| r.session_id).unwrap_or_else(|| "terminal".into()));
        let mut said: Vec<String> = Vec::new();
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(img) => match crate::workbench::keep_image(&dir, img.format.mime_type(), &img.bytes) {
                    Ok(path) => said.push(path.to_string_lossy().to_string()),
                    Err(e) => self.notice = Some(crate::workbench::Notice::error(e)),
                },
                ClipboardEntry::ExternalPaths(paths) => said.push(path_words(paths.paths())),
                ClipboardEntry::String(s) => said.push(s.text().to_string()),
            }
        }
        if said.is_empty() {
            return cx.notify();
        }
        // A selection in what is being typed is what the paste replaces.
        let words = pty.paste_bytes(&said.join(" "));
        if !self.term_delete_sel(agent, words.clone()) {
            self.term_send(agent, &pty, &words);
        }
        self.view_mut(agent).back = 0;
        self.view_mut(agent).sel = None;
        cx.notify();
    }

    /// A choice of the panel's menu.
    pub(crate) fn term_do(&mut self, what: TermDo, window: &mut Window, cx: &mut Context<Self>) {
        let agent = self.term_menu_agent;
        match what {
            TermDo::Copy => {
                self.term_copy(agent, cx);
            }
            TermDo::Paste => self.term_paste(agent, cx),
            TermDo::SelectAll => self.term_select_all(agent),
            TermDo::Clear => self.term_clear(agent),
            TermDo::NewTab => self.shell_new(None, cx),
            TermDo::CloseTab => {
                if let Some(id) = self.term_owner().and_then(|r| self.shells.get(&r.session_id)).and_then(|s| s.current()).map(|t| t.id) {
                    self.shell_close(id, cx);
                }
            }
        }
        window.focus(&self.view(agent).focus, cx);
    }

    /// The button let go, in the panel or outside it. A selection just
    /// made is copied, as it is in a terminal.
    fn term_mouse_up(&mut self, agent: bool, e: &MouseUpEvent, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.view_mut(agent).selecting) {
            // With copying on selection off, what is selected stays for ⌘C.
            let kept = !self.cfg.terminal.copy_on_select && !self.term_sel_text(agent).is_empty();
            if !kept && !self.term_copy(agent, cx) {
                // A click that selected nothing puts the cursor where it
                // was, in the line being typed, as it does in the
                // composer. A double click's word is a selection.
                let at = self.view_mut(agent).sel.take().filter(|s| s.unit == 1).map(|s| s.anchor.0);
                if let (Some(line), Some(pty)) = (at, self.term_pty_of(agent)) {
                    // To the gap between two letters nearest the pointer,
                    // as a caret goes.
                    let b = self.view(agent).bounds.get();
                    let x = (f32::from(e.position.x - b.left()) - self.view(agent).pad.0) / self.view(agent).cell_w.max(1.);
                    let col = x.round().clamp(0., pty.size().1 as f32) as u16;
                    if let Some(typing) = self.term_typing(agent, &pty) {
                        let to = (line, col).max(typing.start);
                        if typing.holds(to) {
                            self.term_caret_to(agent, pty, (to.0.max(0) as u16, to.1));
                            self.view_mut(agent).blink.1 = Instant::now();
                        }
                    }
                }
            }
            cx.notify();
            return;
        }
        let Some(button) = self.view_mut(agent).mouse.take() else { return };
        let Some(pty) = self.term_pty_of(agent) else { return };
        if let Some(want) = pty.mouse().filter(|w| w.release) {
            let (col, row) = self.term_cell(agent, e.position, &pty);
            pty.write(&pty::mouse_bytes(button | mod_bits(&e.modifiers), col, row, false, want.form));
        }
    }

    /// The pointer moved over the panel: the selection follows it, or it
    /// is told to the program a cell at a time, with a button held, or
    /// without one where the program asked for that too.
    fn term_mouse_move(&mut self, agent: bool, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button.is_none() {
            self.view_mut(agent).mouse = None;
            self.view_mut(agent).selecting = false;
        }
        let Some(pty) = self.term_pty_of(agent) else { return };
        // A drag that selects follows the pointer anywhere in the window
        // (`term_select_to`).
        if self.view_mut(agent).selecting {
            return;
        }
        // The link under the pointer is underlined, looked for a cell at a time.
        let cell = self.term_cell(agent, e.position, &pty);
        if cell != self.view_mut(agent).hover {
            self.view_mut(agent).hover = cell;
            let link = self.term_link_at(agent, &pty, cell.1, cell.0);
            if link != self.view_mut(agent).link {
                self.view_mut(agent).link = link;
                cx.notify();
            }
        }
        let Some(want) = pty.mouse().filter(|_| pty.alive() && self.view_mut(agent).back == 0) else { return };
        let held = self.view_mut(agent).mouse;
        if !(want.motion || want.drag && held.is_some()) {
            return;
        }
        let cell = self.term_cell(agent, e.position, &pty);
        if cell == self.view_mut(agent).mouse_cell {
            return;
        }
        self.view_mut(agent).mouse_cell = cell;
        // No button held is button 3 on the move.
        pty.write(&pty::mouse_bytes(held.unwrap_or(3) | 32 | mod_bits(&e.modifiers), cell.0, cell.1, true, want.form));
    }

    /// The line of the screen's `row` as it is drawn now, scrolled back
    /// or not (`Sel`).
    fn term_line(&self, agent: bool, row: u16) -> i64 {
        row as i64 - self.view(agent).back as i64
    }

    /// Which terminal a drag is selecting in, if one is: the shell's
    /// (false) or the agent's.
    pub(crate) fn term_selecting(&self) -> Option<bool> {
        if self.shell_view.selecting {
            Some(false)
        } else if self.agent_view.selecting {
            Some(true)
        } else {
            None
        }
    }

    /// A drag that selects, wherever the pointer is in the window (the
    /// window's own listener sends every move here while it lasts): the
    /// selection runs to the cell under the pointer, and held past the
    /// screen's top or foot the screen scrolls on under it, the faster
    /// the further out (`term_edge_step`).
    pub(crate) fn term_select_to(&mut self, agent: bool, at: Point<Pixels>, held: bool, cx: &mut Context<Self>) {
        let view = self.view_mut(agent);
        if !held {
            view.selecting = false;
            view.edge = 0.;
            return;
        }
        let Some(pty) = self.term_pty_of(agent) else { return };
        let (col, row) = self.term_cell(agent, at, &pty);
        let b = self.view(agent).bounds.get();
        let (y, top, foot) = (f32::from(at.y), f32::from(b.top()), f32::from(b.bottom()));
        let edge = if y < top { y - top } else if y > foot { y - foot } else { 0. };
        let line = self.term_line(agent, row);
        let view = self.view_mut(agent);
        (view.edge, view.edge_col) = (edge, col);
        if let Some(sel) = view.sel.as_mut().filter(|s| s.head != (line, col)) {
            sel.head = (line, col);
            cx.notify();
        }
        if edge != 0. && !view.edge_run {
            view.edge_run = true;
            view.edge_rows = 0.;
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(EDGE_TICK).await;
                if !this.update(cx, |this, cx| this.term_edge_step(agent, cx)).unwrap_or(false) {
                    break;
                }
            })
            .detach();
        }
    }

    /// One step of the screen scrolling under a drag held past its edge:
    /// a row at a time at the edge, quicker further out, and the
    /// selection's end on the row that came in. Whether to go on.
    fn term_edge_step(&mut self, agent: bool, cx: &mut Context<Self>) -> bool {
        let pty = self.term_pty_of(agent);
        let view = self.view_mut(agent);
        let Some(pty) = pty.filter(|_| view.selecting && view.edge != 0.) else {
            view.edge_run = false;
            return false;
        };
        view.edge_rows += (EDGE_ROWS + view.edge.abs() * EDGE_MORE).min(EDGE_MOST) * EDGE_TICK.as_secs_f32();
        let n = view.edge_rows.trunc() as usize;
        if n == 0 {
            return true;
        }
        view.edge_rows -= n as f32;
        let rows = pty.size().0 as i64;
        let back = if view.edge < 0. { (view.back + n).min(pty.kept_back()) } else { view.back.saturating_sub(n) };
        let view = self.view_mut(agent);
        view.back = back;
        let line = if view.edge < 0. { -(back as i64) } else { rows - 1 - back as i64 };
        let col = view.edge_col;
        if let Some(sel) = view.sel.as_mut() {
            sel.head = (line, col);
        }
        self.term_unhover(agent);
        cx.notify();
        true
    }

    /// ⌘A: at a prompt, the paragraph the cursor is in, as Kaku has it;
    /// else, or with nothing typed at all, all the terminal holds, what has left
    /// the screen with it, up to the end of the last line with anything
    /// on it.
    fn term_select_all(&mut self, agent: bool) {
        let Some(pty) = self.term_pty_of(agent) else { return };
        if let (Some(typing), Some((row, _))) = (self.term_typing(agent, &pty), pty.cursor()) {
            let (from, to) = typing.para(row as i64);
            if from < to {
                self.view_mut(agent).sel = Some(Sel { anchor: from, head: to, unit: 0, block: false });
            }
            // An empty line among others has nothing to select.
            let typed = typing.rows.iter().zip(typing.first..).any(|(r, l)| r.iter().zip(0..).any(|(c, i)| (l, i) >= typing.start && !c.is_whitespace() && *c != '\0'));
            if from < to || typed {
                return;
            }
        }
        let (rows, _) = pty.rows_back(0);
        let last = rows.iter().rposition(|r| r.iter().any(|s| !s.text.trim().is_empty())).unwrap_or(0);
        let end = rows.get(last).map(|r| row_cells(r).len()).unwrap_or(0) as u16;
        self.view_mut(agent).sel = Some(Sel { anchor: (-(pty.kept_back() as i64), 0), head: (last as i64, end), unit: 0, block: false });
    }

    /// A selection by the letter, the word or the row made one between
    /// places, as the keyboard moves it, with its ends where it shows them.
    fn term_caret(sel: Sel, pty: &Pty) -> Sel {
        if sel.unit == 0 {
            return sel;
        }
        let (a, b) = sel.ends();
        let (top, lines) = pty.lines(a.0, b.0);
        let cells: Vec<Vec<char>> = lines.iter().map(|(r, _)| row_cells(r)).collect();
        let spans = Sel { block: false, ..sel }.spans(top, &cells);
        let (start, end) = match (spans.iter().position(Option::is_some), spans.iter().rposition(Option::is_some)) {
            (Some(f), Some(l)) => ((top + f as i64, spans[f].unwrap().0 as u16), (top + l as i64, spans[l].unwrap().1 as u16)),
            _ => (a, a),
        };
        let on = sel.head >= sel.anchor;
        Sel { anchor: if on { start } else { end }, head: if on { end } else { start }, unit: 0, block: false }
    }

    /// Shift with the keys that move a caret runs a selection on, from
    /// the selection there is or from the cursor: an arrow a letter or a
    /// row, with ⌥ a word, with ⌘ the line's start or end, as Home and
    /// End are. Off a Mac Ctrl is the word's, and the keys select only
    /// where something is selected already: with nothing, Shift and an
    /// arrow are the shell's (PowerShell selects with them). Where a
    /// program has the screen to itself the keys are its own. Whether
    /// the key was one of them.
    fn term_select_key(&mut self, agent: bool, k: &Keystroke) -> bool {
        enum By {
            Letter,
            Word,
            Line,
            Row,
        }
        let m = &k.modifiers;
        let mac = cfg!(target_os = "macos");
        let word = if mac { m.alt && !m.control } else { m.control && !m.alt };
        let by = match (k.key.as_str(), m.platform && mac, word, m.alt || m.control) {
            _ if !m.shift => return false,
            ("left" | "right", false, false, false) => By::Letter,
            ("left" | "right", false, true, _) => By::Word,
            ("left" | "right", true, false, false) | ("home" | "end", false, false, false) => By::Line,
            ("up" | "down", false, false, false) => By::Row,
            _ => return false,
        };
        let on = matches!(k.key.as_str(), "right" | "end" | "down");
        let Some(pty) = self.term_pty_of(agent) else { return false };
        let sel = match self.view(agent).sel {
            Some(sel) => Self::term_caret(sel, &pty),
            None if mac && !pty.alt_screen() => match pty.cursor() {
                Some((row, col)) => Sel { anchor: (row as i64, col), head: (row as i64, col), unit: 0, block: false },
                None => return false,
            },
            None => return false,
        };
        let (rows, top) = (pty.size().0 as i64, -(pty.kept_back() as i64));
        let cells = |l: i64| pty.lines(l, l).1.first().map(|(r, _)| row_cells(r)).unwrap_or_default();
        let (mut l, mut c) = (sel.head.0, sel.head.1 as usize);
        let mut row = cells(l);
        match by {
            By::Letter if on => {
                if c < row.len() {
                    c += 1;
                    // Past a wide letter's second cell too.
                    if row.get(c) == Some(&'\0') {
                        c += 1;
                    }
                } else if l < rows - 1 {
                    (l, c) = (l + 1, 0);
                }
            }
            By::Letter => {
                if c > 0 {
                    c -= 1;
                    if row.get(c) == Some(&'\0') {
                        c = c.saturating_sub(1);
                    }
                } else if l > top {
                    l -= 1;
                    c = cells(l).len();
                }
            }
            By::Word if on => {
                if c >= row.len() && l < rows - 1 {
                    (l, c) = (l + 1, 0);
                    row = cells(l);
                }
                while c < row.len() && !word_char(row[c]) {
                    c += 1;
                }
                while c < row.len() && word_char(row[c]) {
                    c += 1;
                }
            }
            By::Word => {
                if c == 0 && l > top {
                    l -= 1;
                    row = cells(l);
                    c = row.len();
                }
                c = c.min(row.len());
                while c > 0 && !word_char(row[c - 1]) {
                    c -= 1;
                }
                while c > 0 && word_char(row[c - 1]) {
                    c -= 1;
                }
            }
            By::Line => c = if on { row.len() } else { 0 },
            By::Row => l = (l + if on { 1 } else { -1 }).clamp(top, rows - 1),
        }
        let view = self.view_mut(agent);
        view.sel = Some(Sel { head: (l, c as u16), ..sel });
        // The end that moved stays in sight.
        let back = view.back as i64;
        view.back = if l < -back { -l } else if l > rows - 1 - back { rows - 1 - l } else { back }.max(0) as usize;
        true
    }

    /// The keys that scroll back through what has left the screen, as a
    /// Mac's terminals have them: ⌘↑ and ⌘↓ a row, ⇧ or ⌘ with Page Up
    /// and Page Down a screen, ⌘Home and ⌘End to the top and the foot.
    /// Off a Mac Ctrl+Shift takes ⌘'s place, and Shift with a page key is
    /// a screen too. Not where a program has the screen to itself, which
    /// scrolls its own. Whether the key was one of them.
    fn term_scroll_key(&mut self, agent: bool, k: &Keystroke) -> bool {
        let m = &k.modifiers;
        let held = if cfg!(target_os = "macos") { m.platform && !m.shift } else { m.control && m.shift };
        if m.alt || !(held || m.shift && !m.control && !m.platform) {
            return false;
        }
        let Some(pty) = self.term_pty_of(agent).filter(|p| !p.alt_screen()) else { return false };
        let page = pty.size().0.saturating_sub(1).max(1) as i64;
        let by: i64 = match (k.key.as_str(), held) {
            ("up", true) => 1,
            ("down", true) => -1,
            ("pageup", _) => page,
            ("pagedown", _) => -page,
            ("home", true) => i64::MAX / 2,
            ("end", true) => i64::MIN / 2,
            _ => return false,
        };
        let back = (self.view(agent).back as i64).saturating_add(by).clamp(0, pty.kept_back() as i64);
        self.view_mut(agent).back = back as usize;
        true
    }

    /// Files dropped on the terminal: their paths, as a shell reads them
    /// and a space after, as a Mac's terminals type them.
    fn term_drop(&mut self, agent: bool, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        let Some(pty) = self.term_pty_of(agent).filter(|p| p.alive()) else { return };
        if paths.is_empty() {
            return;
        }
        pty.paste(&format!("{} ", path_words(paths)));
        self.view_mut(agent).back = 0;
        self.view_mut(agent).sel = None;
        cx.notify();
    }

    /// The words selected: each row's stretch without the blank at its
    /// end, a new line between rows unless one runs on into the next.
    fn term_sel_text(&self, agent: bool) -> String {
        let (Some(sel), Some(pty)) = (self.view(agent).sel, self.term_pty_of(agent)) else { return String::new() };
        let (a, b) = sel.ends();
        let (top, lines) = pty.lines(a.0, b.0);
        let wraps: Vec<bool> = lines.iter().map(|(_, w)| *w).collect();
        let cells: Vec<Vec<char>> = lines.iter().map(|(r, _)| row_cells(r)).collect();
        let spans = sel.spans(top, &cells);
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
    fn term_copy(&mut self, agent: bool, cx: &mut Context<Self>) -> bool {
        let text = self.term_sel_text(agent);
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.view_mut(agent).copied = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR + Duration::from_millis(30)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        true
    }

    /// What was under the pointer is looked for again when it next moves:
    /// the rows have changed under it.
    fn term_unhover(&mut self, agent: bool) {
        self.view_mut(agent).link = None;
        self.view_mut(agent).hover = (u16::MAX, u16::MAX);
    }

    /// ⌘K: the screen and what has left it are forgotten, and the
    /// program is asked to draw again (^L), which a shell answers with a
    /// fresh prompt.
    fn term_clear(&mut self, agent: bool) {
        let Some(pty) = self.term_pty_of(agent) else { return };
        if pty.clear_all() {
            pty.write(b"\x0c");
            self.view_mut(agent).back = 0;
            self.view_mut(agent).sel = None;
        }
    }

    /// The shell tab beside the one that shows, round the row's end.
    fn shell_step(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(set) = self.term_owner().and_then(|r| self.shells.get(&r.session_id)) else { return };
        let Some(ix) = set.tabs.iter().position(|t| t.id == set.on) else { return };
        let n = set.tabs.len() as isize;
        let id = set.tabs[(ix as isize + by).rem_euclid(n) as usize].id;
        self.shell_pick(id, cx);
    }

    /// Where the panel's screen is in the window, for the wheel.
    pub(crate) fn term_panel_bounds(&self) -> Option<Bounds<Pixels>> {
        self.term_panel_shown().then(|| self.shell_view.bounds.get())
    }

    /// Where the agent terminal is in the window, in the CLI layout: its
    /// screen and the room at the screen's sides.
    pub(crate) fn agent_term_bounds(&self) -> Option<Bounds<Pixels>> {
        self.cli_shown().then(|| self.agent_view.room.get())
    }

    /// The panel's own keys off a Mac, as the system's terminal has them:
    /// Ctrl+Shift+C and V copy and paste, and so do Ctrl+Insert and
    /// Shift+Insert; on Windows Ctrl+V pastes too, and Ctrl+C copies what
    /// is selected and is the shell's Ctrl+C when nothing is. Ctrl+Tab and
    /// Ctrl+Page Down are the next shell tab. Whether it was one of them.
    fn term_pc_key(&mut self, agent: bool, k: &Keystroke, cx: &mut Context<Self>) -> bool {
        let m = &k.modifiers;
        if cfg!(target_os = "macos") || m.alt || m.platform {
            return false;
        }
        let windows = cfg!(windows);
        let tabs = !agent;
        match (m.control, m.shift, k.key.to_ascii_lowercase().as_str()) {
            (true, true, "c") | (true, false, "insert") => {
                self.term_copy(agent, cx);
            }
            (true, false, "c") if windows && !self.term_sel_text(agent).is_empty() => {
                self.term_copy(agent, cx);
                self.view_mut(agent).sel = None;
            }
            (true, true, "v") | (false, true, "insert") => self.term_paste(agent, cx),
            (true, false, "v") if windows => self.term_paste(agent, cx),
            (true, false, "tab") | (true, false, "pagedown") if tabs => self.shell_step(1, cx),
            (true, true, "tab") | (true, false, "pageup") if tabs => self.shell_step(-1, cx),
            _ => return false,
        }
        true
    }

    /// A key in the panel, as the bytes a terminal sends for it. ⌘V is a
    /// paste, ⌘A selects all, Shift with the keys that move a caret
    /// selects (`term_select_key`), and some keys scroll back
    /// (`term_scroll_key`); any other key with ⌘ is the window's. Off a
    /// Mac, `term_pc_key` has the panel's own.
    fn term_panel_key(&mut self, agent: bool, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(pty) = self.term_pty_of(agent).filter(|p| p.alive()) else { return };
        let k = &e.keystroke;
        if self.term_pc_key(agent, k, cx) || self.term_select_key(agent, k) || self.term_scroll_key(agent, k) {
            self.term_unhover(agent);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // With part of the line being typed selected, Backspace and Delete,
        // with ⌥ or ⌘ too, take the selection, as they do in the composer,
        // and a letter typed takes its place.
        let typed = k.key_char.clone().filter(|c| !k.modifiers.platform && !k.modifiers.control && !c.is_empty() && !c.chars().any(char::is_control));
        if self.view(agent).sel.is_some() && (matches!(k.key.as_str(), "backspace" | "delete") || typed.is_some()) && self.term_delete_sel(agent, typed.map(String::into_bytes).unwrap_or_default()) {
            self.term_unhover(agent);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.platform {
            // ⌘ with the keys of a line is the line's start, its end, all
            // of it before the cursor or after it, and a new line that
            // sends nothing, as a Mac's terminals have them; ⌘⇧[ and ⌘⇧]
            // are the shell tab before and after.
            let line: Option<&[u8]> = match k.key.as_str() {
                "left" => Some(b"\x01"),
                "right" => Some(b"\x05"),
                "backspace" => Some(b"\x15"),
                "delete" => Some(b"\x0b"),
                "enter" => Some(b"\x1b\r"),
                _ => None,
            };
            let handled = match k.key.as_str() {
                "v" => {
                    self.term_paste(agent, cx);
                    true
                }
                "c" => self.term_copy(agent, cx),
                // ⌘X cuts what is selected of the line being typed.
                "x" => {
                    let cut = self.term_copy(agent, cx);
                    self.term_delete_sel(agent, Vec::new()) || cut
                }
                // ⌘Z takes back the last change to the line, as the
                // shell's own undo (Ctrl-_).
                "z" if !k.modifiers.shift => {
                    pty.write(b"\x1f");
                    true
                }
                "a" if !k.modifiers.shift => {
                    self.term_select_all(agent);
                    true
                }
                "[" | "{" if k.modifiers.shift && !agent => {
                    self.shell_step(-1, cx);
                    true
                }
                "]" | "}" if k.modifiers.shift && !agent => {
                    self.shell_step(1, cx);
                    true
                }
                _ => match line {
                    Some(bytes) => {
                        pty.write(bytes);
                        self.view_mut(agent).back = 0;
                        self.view_mut(agent).sel = None;
                        true
                    }
                    None => false,
                },
            };
            if handled {
                self.term_unhover(agent);
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        // ⌥ with an arrow, Backspace or Delete is a word, as a Mac's
        // terminals have it.
        let word: Option<&[u8]> = match (k.modifiers.alt, k.key.as_str()) {
            (true, "left") => Some(b"\x1bb"),
            (true, "right") => Some(b"\x1bf"),
            (true, "backspace") => Some(b"\x1b\x7f"),
            (true, "delete") => Some(b"\x1bd"),
            // ⇧↩ is a new line that sends nothing.
            (false, "enter") if k.modifiers.shift => Some(b"\x1b\r"),
            _ => None,
        };
        let Some(mut bytes) = word.map(<[u8]>::to_vec).or_else(|| term_bytes(k)) else { return };
        if pty.app_cursor() && bytes.len() == 3 && bytes.starts_with(b"\x1b[") && matches!(bytes[2], b'A'..=b'D' | b'H' | b'F') {
            bytes[1] = b'O';
        }
        self.term_send(agent, &pty, &bytes);
        self.view_mut(agent).back = 0;
        self.view_mut(agent).sel = None;
        self.term_unhover(agent);
        // A key begins the cursor's blink again, lit.
        self.view_mut(agent).blink.1 = Instant::now();
        cx.stop_propagation();
        cx.notify();
    }

    /// The agent's hidden terminal is kept the size of the panel while
    /// the panel shows it, and its own size otherwise: the terminal card
    /// and the screen's readers were made for that one.
    pub(crate) fn fit_agent_pty(&self, session_id: &str) {
        let Some(pty) = self.hub.agent_screen(session_id) else { return };
        let want = if self.cli_shown() { self.agent_view.size.get() } else { (pty::ROWS, pty::COLS) };
        if want.0 > 0 && want.1 > 0 && pty.size() != want {
            pty.resize(want.0, want.1);
        }
    }

    /// The shell terminal's panel as it is drawn, and for a moment as it
    /// goes: it widens from the window's edge and fades in, and put away
    /// it narrows and fades out, drawn as it was. The wrapper is there at
    /// rest too, under the same name (`docs/panels.md`).
    pub(crate) fn render_term_panel(&mut self, r: &SessionRef, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.term_stage() {
            return None;
        }
        let live = self.side_term_anim.filter(|(_, _, at, _)| at.elapsed() < TERM_PANEL_ANIM);
        let serial = self.side_term_anim.map(|(_, _, _, n)| n + 1).unwrap_or(0);
        let on = self.term_on();
        let going = !on && live.is_some_and(|(from, _, _, _)| from);
        if !on && !going {
            return None;
        }
        let w = self.term_panel_w();
        let panel = self.render_term_side(false, going, r, window, cx);
        let play = live.map(|(_, to, _, _)| to);
        Some(
            div()
                .h_full()
                .flex_shrink_0()
                .overflow_hidden()
                .child(panel)
                .with_animation(ElementId::Name(format!("term-panel-{serial}").into()), Animation::new(TERM_PANEL_ANIM).with_easing(ease_out_quint()), move |d, t| match play {
                    Some(comes) => {
                        let t = if comes { t } else { 1. - t };
                        d.w((w * t).round()).opacity(t)
                    }
                    None => d,
                })
                .into_any_element(),
        )
    }

    /// The agent terminal in the conversation's place, the CLI layout's.
    /// `leaving`: on its way out, drawn as it was.
    pub(crate) fn render_cli(&mut self, r: &SessionRef, leaving: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.render_term_side(true, leaving, r, window, cx)
    }

    /// One of the two terminals: the shell's panel, its head a row of
    /// tabs over the screen, or the agent's screen in the CLI layout.
    /// `leaving` is one on its way out, drawn as it was: nothing is
    /// started, sized or measured for it.
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
        let (pad, pad_foot) = self.term_pad(window);
        let cell_w = {
            let probe = "MMMMMMMMMM";
            let run = TextRun { len: probe.len(), font: face.clone(), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(probe.into(), px(text), &[run], None).width) / probe.len() as f32
        };
        let room = self.view_mut(agent).bounds.get().size;
        let size = (((f32::from(room.height) - pad - pad_foot) / lh).floor().max(4.) as u16, ((f32::from(room.width) - 2. * pad) / cell_w.max(1.)).floor().max(20.) as u16);
        if room.height > px(0.) && !leaving {
            self.view_mut(agent).size.set(size);
            self.view_mut(agent).cell_w = cell_w;
            self.view_mut(agent).pad = (pad, pad_foot);
        }

        // What shows: the shell, started when first looked at, or the
        // agent's hidden terminal when it has one.
        let shown: Result<Arc<Pty>, AnyElement> = if leaving {
            self.term_pty_of(agent).ok_or_else(|| div().into_any_element())
        } else if agent {
            // Looked at with none behind the session, the agent is
            // started, once: one that ends by itself straight away is not
            // started over and over, and the button is there for that.
            if self.hub.agent_screen(&r.session_id).is_none() && self.agent_startable(r) && self.side_term_tried.insert(r.session_id.clone()) {
                self.start_hidden_terminal(cx);
            }
            self.hub.agent_screen(&r.session_id).ok_or_else(|| self.render_agent_absent(r, cx))
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
                        Some(e) => d.text_color(hsla(k.ansi[1])).child(tf("Could not start a shell: {error}", &[("error", &e)])),
                        None => d.text_color(hsla(k.tab_off)).child(t("No shell open. Press + for a new one.")),
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
                // The rows reflow under a selection, which no longer
                // names what it did.
                self.view_mut(agent).sel = None;
            }
        }

        let (ground, ink) = (k.bg, k.fg);
        let focused = self.view_mut(agent).focus.is_focused(window);
        if !leaving {
            self.view_mut(agent).focused = focused;
        }
        // A program that hears the mouse has the arrow, a link the hand.
        let grabbed = shown.as_ref().is_ok_and(|p| p.alive() && p.mouse().is_some()) && self.view_mut(agent).back == 0;
        let pointer = if self.view_mut(agent).link.is_some() { CursorStyle::PointingHand } else if grabbed { CursorStyle::Arrow } else { CursorStyle::IBeam };
        let body: AnyElement = match shown {
            Err(el) => el,
            Ok(pty) => {
                let (rows, back) = pty.rows_back(self.view_mut(agent).back);
                self.view_mut(agent).back = back;
                let picked = match self.view_mut(agent).sel {
                    Some(sel) if !leaving => sel.spans(-(back as i64), &rows.iter().map(|r| row_cells(r)).collect::<Vec<_>>()),
                    _ => Vec::new(),
                };
                let lines = screen_lines(&rows, cell_w, lh, k, &picked, self.view_mut(agent).link.as_ref().filter(|_| !leaving));
                let copied = self.view_mut(agent).copied.filter(|at| at.elapsed() < COPIED_FOR && !leaving).map(|_| {
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
                        .child(t("Copied"))
                        .with_animation("term-copied", Animation::new(COPIED_FOR), |d, t| d.opacity(if t > 0.8 { (1. - t) / 0.2 } else { 1. }))
                });
                // The cursor, where the next letter goes: Kaku's bar, two
                // of the screen's pixels wide and the row's height, on and
                // off by turns from when it last moved while the panel has
                // the keyboard, and still otherwise.
                let at = pty.cursor().filter(|_| back == 0 && pty.alive());
                if let Some(at) = at.filter(|at| !leaving && *at != self.view_mut(agent).blink.0) {
                    self.view_mut(agent).blink = (at, Instant::now());
                }
                if !leaving {
                    self.view_mut(agent).blink_on = self.blink_on(agent);
                }
                let (shape, blinks) = (self.cfg.terminal.cursor.clone(), self.cfg.terminal.cursor_blink);
                let cursor = at.filter(|_| !focused || !blinks || self.view_mut(agent).blink_on).map(|(row, col)| {
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
                        .child(crate::workbench::agent_glyph(r.agent, px(13.), crate::workbench::agent_color(r.agent, &theme), true))
                        .child(tf("Starting {agent}, one moment…", &[("agent", &r.agent.display_name())]))
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
                        .child(if agent { t("The agent's terminal has ended.") } else { t("The shell has ended.") })
                        .when(!agent, |d| {
                            d.child(crate::branches::small_pill("term-again", "icons/terminal.svg", t("New shell"), Press::Quiet, false, &theme).on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                // A new one in the ended one's tab's place.
                                if let Some((ix, id)) = this.shells.get(&sid).and_then(|s| s.tabs.iter().position(|t| t.id == s.on).map(|ix| (ix, s.on))) {
                                    this.shell_close(id, cx);
                                    this.shell_new(Some(ix), cx);
                                }
                                window.focus(&this.shell_view.focus, cx);
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

        let lines_back = self.view(agent).back;
        let back = (lines_back > 0).then(|| div().flex_shrink_0().pr(px(10.)).font(face.clone()).text_size(px(11.)).text_color(hsla(k.tab_off)).child(crate::i18n::count(lines_back, "{n} line back", "{n} lines back")));
        let bounds = self.view(agent).bounds.clone();
        let entity = cx.entity().downgrade();
        let screen = div()
            .id(if agent { "term-agent" } else { "term-panel-shell" })
            .key_context(TERMINAL_CONTEXT)
            .when(!leaving, |d| d.track_focus(&self.view(agent).focus))
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(hsla(ground))
            .cursor(pointer)
            // Files dragged in from the Finder are typed as their paths,
            // the edge in the accent while they are over it, as the
            // composer's is.
            .border_1()
            .border_color(transparent_black())
            .drag_over::<ExternalPaths>({
                let accent = theme.primary;
                move |s, _, _, _| s.border_color(accent)
            })
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                this.term_drop(agent, paths.paths(), cx);
                window.focus(&this.view(agent).focus, cx);
            }))
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| this.term_panel_key(agent, e, cx)))
            .on_action(cx.listener(move |this, _: &TermTab, _, cx| {
                if let Some(pty) = this.term_pty_of(agent) {
                    pty.write(b"\t");
                }
                cx.notify();
            }))
            .on_action(cx.listener(move |this, _: &TermBackTab, _, cx| {
                if let Some(pty) = this.term_pty_of(agent) {
                    pty.write(b"\x1b[Z");
                }
                cx.notify();
            }))
            .on_any_mouse_down(cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                window.focus(&this.view(agent).focus, cx);
                this.term_mouse_down(agent, e, cx);
                swallow_click(window, cx);
                cx.notify();
            }))
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| this.term_mouse_move(agent, e, cx)))
            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                if !over && this.view(agent).link.is_some() {
                    this.term_unhover(agent);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(move |this, _: &TermClear, _, cx| {
                this.term_clear(agent);
                cx.notify();
            }))
            .on_action(cx.listener(move |this, _: &TermNewTab, window, cx| {
                if agent {
                    return cx.propagate();
                }
                this.shell_new(None, cx);
                window.focus(&this.shell_view.focus, cx);
            }))
            .on_action(cx.listener(move |this, _: &CloseTab, window, cx| {
                // ⌘W, the window's own action, heard here first while the
                // terminal has the keyboard. In the shell terminal it is
                // the shell's and nothing else's: it closes the tab
                // showing, and with none left puts the panel away, as its
                // button does, so it never reaches a file shown or the
                // session's tab. In the agent terminal it is the window's.
                if agent {
                    cx.propagate();
                } else if let Some(id) = this.term_owner().and_then(|r| this.shells.get(&r.session_id)).and_then(|s| s.current()).map(|t| t.id) {
                    this.shell_close(id, cx);
                } else {
                    this.term_go(false, cx);
                    window.focus(&this.focus_handle, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(move |this, e: &MouseUpEvent, _, cx| this.term_mouse_up(agent, e, cx)))
            .on_mouse_up(MouseButton::Right, cx.listener(move |this, e: &MouseUpEvent, _, cx| this.term_mouse_up(agent, e, cx)))
            .on_mouse_up(MouseButton::Middle, cx.listener(move |this, e: &MouseUpEvent, _, cx| this.term_mouse_up(agent, e, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(move |this, e: &MouseUpEvent, _, cx| this.term_mouse_up(agent, e, cx)))
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
            .child(body);
        // The agent terminal has the conversation's room under the path
        // bar, which names its session; how far back it is scrolled is
        // said at the screen's top right.
        if agent {
            // It stands where the rendered layout's column does, as wide and
            // in the same place, on the page's own ground, so a change of
            // layout moves nothing in the margins. A press in the margins
            // is the screen's keyboard, and its wheel the screen's.
            let ground = hsla(ground);
            let room = self.agent_view.room.clone();
            let (room_l, room_r) = self.conv_room(f32::from(self.conv_bounds.get().size.width));
            return h_flex()
                .relative()
                .flex_1()
                .min_w_0()
                .h_full()
                .justify_center()
                .pl(px(room_l))
                .pr(px(room_r))
                .overflow_hidden()
                .bg(theme.background)
                .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    window.focus(&this.agent_view.focus, cx);
                    swallow_click(window, cx);
                }))
                .when(!leaving, |d| d.child(canvas(move |b, _, _| room.set(b), |_, _, _, _| {}).absolute().size_full()))
                .child(
                    v_flex()
                        .relative()
                        .w_full()
                        .max_w(crate::workbench::CONTENT_W)
                        .h_full()
                        .bg(ground)
                        .child(screen.children(back.map(|b| div().absolute().top(px(6.)).right(px(4.)).child(b)))),
                )
                .into_any_element();
        }
        // The shell's head is a row of tabs, a tab a shell.
        let head_row = self.render_shell_tabs(r, f32::from(w) - if back.is_some() { 110. } else { 0. }, cx);
        v_flex()
            .w(w)
            .h_full()
            .flex_shrink_0()
            .map(|d| if self.files_right() { d.border_r_1() } else { d.border_l_1() })
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
            .child(screen)
            .into_any_element()
    }

    /// The agent's side with no hidden terminal behind the session: why,
    /// and a button that starts one where that is ours to do.
    /// Whether the agent's hidden terminal can be started for the
    /// session now, by the window itself.
    fn agent_startable(&self, r: &SessionRef) -> bool {
        match r.agent {
            AgentId::ClaudeCode => self.hub.hidden_terminals() && !self.in_own_terminal(r) && self.terminal_ok(r).is_ok(),
            AgentId::Codex => self.codex_terminal_ok(r).is_ok(),
        }
    }

    /// Whether Codex's terminal can be started for the session, joined
    /// to its driver, and why not when it cannot. A turn under way is no
    /// bar when the driver is ours: the terminal joins it and shows it.
    fn codex_terminal_ok(&self, r: &SessionRef) -> Result<(), String> {
        if !cfg!(unix) {
            return Err(t("Codex's terminal is not shown on Windows yet. It is driven from the composer, and what it does is in the conversation.").into());
        }
        if self.is_draft(&r.session_id) {
            return Err(t("Codex starts with your first message.").into());
        }
        if self.hub.driver_for(&r.session_id).is_some() {
            return Ok(());
        }
        match self.reply_via_for(r) {
            (_, why) if !why.is_empty() => {
                let mut why = why;
                if let Some(first) = why.get(..1).map(str::to_uppercase) {
                    why.replace_range(..1, &first);
                }
                Err(tf("{why}.", &[("why", &why)]))
            }
            _ => Ok(()),
        }
    }

    fn render_agent_absent(&self, r: &SessionRef, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let k = self.term_scheme(cx);
        let name = r.agent.display_name();
        let adrift = r.agent == AgentId::ClaudeCode && self.hub.hidden_terminals() && self.in_own_terminal(r) && self.hub.adrift_for(&r.session_id).is_some();
        let (line, can): (String, bool) = if adrift {
            // Its terminal went with the Emaki that started it, and a
            // terminal cannot be joined again: the turn runs on unseen.
            (tf("This turn began before Emaki was last quit. {agent} is still running it, with no terminal left to show, and its work appears in the conversation. When the turn is over, {agent} can be started here. Stopping it now ends the turn.", &[("agent", &name)]), true)
        } else if r.agent == AgentId::Codex {
            // Codex's own terminal, joined to the app server its driver
            // runs: one session, one writer.
            if self.hub.attaching(&r.session_id) {
                (t("Starting Codex…").to_string(), false)
            } else {
                match self.codex_terminal_ok(r) {
                    Err(why) => (why, false),
                    Ok(()) => (t("Codex's terminal is closed for this session.").to_string(), true),
                }
            }
        } else if r.agent != AgentId::ClaudeCode || !self.hub.hidden_terminals() {
            (tf("{agent} runs here only for Claude Code sessions, with the hidden terminal on.", &[("agent", &name)]), false)
        } else if self.in_own_terminal(r) {
            (tf("A turn is running in a terminal of yours. When it is over, {agent} can be started here.", &[("agent", &name)]), false)
        } else if self.is_draft(&r.session_id) {
            (tf("{agent} starts with your first message.", &[("agent", &name)]), false)
        } else if let Err(why) = self.terminal_ok(r) {
            (t(why).to_string(), false)
        } else {
            (tf("{agent} has stopped for this session.", &[("agent", &name)]), true)
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
                let start = if adrift { crate::branches::small_pill("term-agent-start", "icons/circle-x.svg", t("Stop and Start Here"), crate::workbench::Press::Danger, false, &theme) } else { crate::branches::small_pill("term-agent-start", "icons/play.svg", tf("Start {agent}", &[("agent", &name)]), crate::workbench::Press::Quiet, false, &theme) };
                d.child(start.on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.start_hidden_terminal(cx);
                    window.focus(&this.agent_view.focus, cx);
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
    while from < to && "([{“‘".contains(cells[from]) {
        from += 1;
    }
    while to > from && ".,;:!?)]}”’".contains(cells[to - 1]) && !(cells[to - 1] == ')' && cells[from..to].contains(&'(')) {
        to -= 1;
    }
    let word: String = cells[from..to].iter().collect();
    let target = if let Some(at) = word.find("://") {
        // From where the scheme's name begins.
        // Past the character that is not one of its letters, which may be
        // more than a byte (“, →).
        let start = word[..at].char_indices().rfind(|(_, c)| !c.is_alphanumeric() && *c != '_').map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
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

/// One of Kaku's two color schemes (tw93/kaku, MIT): the screen's
/// ground and ink, the cursor, the selection's ground (with how strong)
/// and ink, the rule under the head, a tab's grounds and the ink of one
/// not showing, the toast's ground and ink, the sixteen named colors,
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
    /// The ground a gray from the other side of the scale is drawn as: a
    /// program's quiet band on a dark terminal is a quiet band here.
    wash: u32,
    /// The weights of plain and of bold letters: one step heavier on a
    /// light ground, where thin strokes fade.
    plain: FontWeight,
    bold: FontWeight,
}

static KAKU_DARK: Scheme = Scheme {
    bg: 0x2a2826,
    fg: 0xd5d4d6,
    cursor: 0x8e6ad9,
    sel: (0x8e6ad9, 0.55),
    sel_fg: 0xd5d4d6,
    split: 0x3f3c38,
    tab_on: 0x3f3c38,
    tab_hover: 0x33312e,
    tab_off: 0x6d6d6d,
    toast: (0x8e6ad9, 0xffffff),
    ansi: {
        let k = &crate::look::INKS_DARK;
        [0xc8c6cc, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0xd5d4d6, 0x6d6d6d, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0xd5d4d6]
    },
    grounds: &[(0xc8c6cc, 0x2a2826), (0x6d6d6d, 0x413f3b), (0x6e6e6e, 0x413f3b), (0x8ec3ff, 0x413f3b), (0xd5d4d6, 0x504d49)],
    inks: &[(0x000000, 0xd5d4d6), (0x110f18, 0xd5d4d6), (0x15141b, 0xd5d4d6), (0x1a1a1a, 0xd5d4d6), (0x1c1c1c, 0xd5d4d6)],
    wash: 0x413f3b,
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
    // The window's graphite and paper, as its frame is: Kaku's dark
    // gold read as a stain on the cream.
    toast: (0x3d3a36, 0xf3f1ea),
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

/// The color a scheme draws in place of `c`, when it has one.
fn swapped(table: &[(u32, u32)], c: u32) -> u32 {
    table.iter().find(|(from, _)| *from == c).map(|(_, to)| *to).unwrap_or(c)
}

/// Whether a color is a gray, or near enough to one.
fn gray(c: u32) -> bool {
    let ch = [c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff];
    ch.iter().max().unwrap() - ch.iter().min().unwrap() < 28
}

/// `c`'s hue, brought to the scheme's side of the scale: its brightest
/// channel full, then a little of it over the scheme's ground. A brighter
/// ground to begin with (a word marked within a marked line) keeps more,
/// so what was stronger stays stronger.
fn tint(c: u32, ground: u32, light: bool) -> u32 {
    let top = (c >> 16 & 0xff).max(c >> 8 & 0xff).max(c & 0xff).max(1) as f32;
    let ch = |sh: u32| (((c >> sh & 0xff) as f32 * 255. / top).round() as u32).min(255);
    let vivid = ch(16) << 16 | ch(8) << 8 | ch(0);
    let b = brightness(c);
    mix(ground, vivid, if light { 0.14 + 0.5 * b } else { 0.2 + 0.2 * (1. - b) })
}

/// How bright a color is to the eye, 0 to 1.
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
/// makes colors a program chose for a dark terminal readable on the
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
        // The letters, as runs of plain ones each set at the column it
        // starts at. A cell the face may draw wider or narrower than its
        // columns (an emoji, a symbol from a fallback face) stands by
        // itself at its own: laid out by the face's advances, one row of
        // text drifted off the columns the cursor, the grounds and the
        // selection are drawn at.
        let mut pieces: Vec<(usize, String, Vec<(std::ops::Range<usize>, HighlightStyle)>)> = Vec::new();
        let mut piece_col = 0usize;
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
            // The sixteen named colors are the scheme's own.
            let mut fg = match s.fg_ix {
                Some(i) => k.ansi[i as usize],
                None => s.fg.map(|c| swapped(k.inks, c)).unwrap_or(k.fg),
            };
            let mut bg = match s.bg_ix {
                Some(i) => Some(swapped(k.grounds, k.ansi[i as usize])),
                None => s.bg.map(|c| swapped(k.grounds, c)),
            };
            // A gray ground a program chose for the other kind of terminal
            // (Claude Code's band behind a prompt, near black for a dark
            // one) is the scheme's own quiet ground, and a gray ink that
            // was to stand out on it is the scheme's ink. Kaku swaps the
            // grays it knows by value; this is the same for any gray.
            let light = brightness(k.bg) > 0.5;
            if bg.is_some_and(|c| s.bg_ix.is_none() && gray(c) && if light { brightness(c) < 0.2 } else { brightness(c) > 0.5 }) {
                bg = Some(k.wash);
                if s.fg_ix.is_none() && gray(fg) && if light { brightness(fg) > 0.4 } else { brightness(fg) < 0.2 } {
                    fg = k.fg;
                }
            }
            // A colored ground from the other side of the scale (Claude
            // Code's diff, green and red, set for a dark terminal) is a
            // soft tint of its own hue on this one, and a light ink on it
            // the scheme's: as bars of deep green and red on the light
            // scheme they were the loudest thing on the screen.
            else if let Some(c) = bg.filter(|c| s.bg_ix.is_none() && !gray(*c) && if light { brightness(*c) < 0.35 } else { brightness(*c) > 0.45 }) {
                bg = Some(tint(c, k.bg, light));
                if s.fg_ix.is_none() && if light { brightness(fg) > 0.4 } else { brightness(fg) < 0.2 } {
                    fg = k.fg;
                }
            }
            if s.inverse {
                (fg, bg) = (bg.unwrap_or(k.bg), Some(fg));
            }
            if s.dim {
                fg = mix(fg, bg.unwrap_or(k.bg), 0.45);
            }
            // Shapes keep the color asked for; letters must be readable.
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
            for (cell, w) in s.each_cell() {
                let now = marks(col);
                if now != run.1 {
                    if run.0 < text.len() {
                        looks.push((run.0..text.len(), look(run.1)));
                    }
                    run = (text.len(), now);
                }
                if text.is_empty() {
                    piece_col = col;
                }
                let x0 = col as f32 * cell_w;
                let c = cell.chars().next().unwrap_or(' ');
                if let Some((shapes, strength)) = block_shape(c) {
                    for (x, y, bw, bh) in shapes {
                        under.push(div().absolute().left(px(x0 + x * cell_w)).top(px(y * lh)).w(px(bw * cell_w + 0.5)).h(px(bh * lh)).bg(shape.opacity(strength)).into_any_element());
                    }
                    text.push(' ');
                } else if let Some(parts) = line_shape(c) {
                    under.extend(line_parts(parts, x0, cell_w, lh, shape));
                    text.push(' ');
                } else if cell.len() == 1 {
                    text.push_str(cell);
                } else {
                    if run.0 < text.len() {
                        looks.push((run.0..text.len(), look(run.1)));
                    }
                    if !text.is_empty() {
                        pieces.push((piece_col, std::mem::take(&mut text), std::mem::take(&mut looks)));
                    }
                    let shown = cell_face(cell, w);
                    pieces.push((col, shown.to_string(), vec![(0..shown.len(), look(now))]));
                    run = (0, now);
                }
                col += w as usize;
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
        if !text.is_empty() {
            pieces.push((piece_col, text, looks));
        }
        let letters = pieces.into_iter().map(|(at, text, looks)| div().absolute().top_0().left(px(at as f32 * cell_w)).child(StyledText::new(text).with_highlights(looks)));
        lines.push(div().h(px(lh)).relative().flex_shrink_0().whitespace_nowrap().children(grounds).children(under).children(letters).into_any_element());
    }
    lines
}

/// What a cell draws: a character the emoji selector (U+FE0F) follows
/// in a cell of one column is drawn in its text form. The screen model
/// and the shell count it as one column, and its emoji is two wide:
/// drawn so, ☁️ covered the space after it.
fn cell_face(cell: &str, w: u8) -> std::borrow::Cow<'_, str> {
    if w == 1 && cell.contains('\u{fe0f}') {
        cell.replace('\u{fe0f}', "").into()
    } else {
        cell.into()
    }
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


#[cfg(test)]
mod tests {
    use super::{cell_face, link_at, Target};

    #[test]
    fn an_emoji_in_one_column_is_drawn_as_text() {
        assert_eq!(cell_face("\u{2601}\u{fe0f}", 1), "\u{2601}");
        assert_eq!(cell_face("1\u{fe0f}\u{20e3}", 1), "1\u{20e3}");
        // Two columns have room for the emoji.
        assert_eq!(cell_face("\u{2764}\u{fe0f}", 2), "\u{2764}\u{fe0f}");
        assert_eq!(cell_face("\u{1f680}", 2), "\u{1f680}");
    }

    #[test]
    fn a_link_after_a_quote_mark_of_more_than_a_byte() {
        for (line, url) in [("“https://example.com”", "https://example.com"), ("→https://a.io", "https://a.io"), ("see (https://b.dev).", "https://b.dev")] {
            let cells: Vec<char> = line.chars().collect();
            let col = cells.iter().position(|c| *c == ':').unwrap();
            let (from, _, target) = link_at(&cells, col, std::path::Path::new("/")).unwrap();
            assert!(target == Target::Url(url.to_string()), "{line}");
            assert_eq!(cells[from], 'h', "{line}");
        }
    }
}
