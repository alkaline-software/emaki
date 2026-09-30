//! The window: sidebar, sessions, board, search, transcript and composer.
//!
//! State lives here; everything shown comes from the transcript through the
//! core, and everything typed goes out through the hub. The board is the home
//! page: the question the window answers on arrival is "what needs me".
//!
//! The layout follows the Claude desktop app: one collapsible sidebar with a
//! "new" entry, a few destinations and a list of recents; one content column
//! centred in the rest of the window; a floating composer card at the foot
//! of a conversation and in the middle of the home page.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use chrono::Timelike;
use futures::StreamExt;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, OutdentInline, Textarea, TextareaState};
use gpui_component::popover::Popover;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use crate::ui_state::{Rect, Scroll, UiState};
use emaki_core::adapters;
use emaki_core::build::Phase;
use emaki_core::config::Config;
use emaki_core::driver::{self, effort_detail, effort_label, image_block, mode_detail, mode_label, model_label, PermissionRequest, EFFORTS, IMAGE_TYPES, MODELS, MODES};
use emaki_core::find::{FindIndex, Hit};
use emaki_core::limits::Limits;
use emaki_core::model::{AgentId, Item, Session};
use emaki_core::search::Results;
use emaki_core::transcript::SessionRef;

use crate::format::{clock, day, elapsed_since, now_secs, plural, relative, short_id};
use crate::hub::{Hub, HubEvent, UpdateEvent};
use emaki_core::update::{self, UpdateState};
use gpui_component::checkbox::Checkbox;

actions!(emaki, [ToggleSearch, Refresh, NewSession, GoBoard, GoSessions, ToggleSidebar, Escape, Send, CloseTab, OpenSettings, FindInPage, FindNext, FindPrev]);

pub const KEY_CONTEXT: &str = "Workbench";
pub const COMPOSER_CONTEXT: &str = "Composer";
pub const SEARCH_CONTEXT: &str = "SearchPalette";
pub const FIND_CONTEXT: &str = "FindBar";

pub const SIDEBAR_W: Pixels = px(268.);
pub const TITLEBAR_H: Pixels = px(48.);
/// Room for the traffic lights on a transparent title bar.
pub const TRAFFIC_W: Pixels = px(80.);
/// The reading column: conversation, composer, the sessions list, the home page.
pub const CONTENT_W: Pixels = px(768.);
/// A board column never gets narrower than this; past that the board scrolls.
pub const COL_MIN_W: Pixels = px(210.);
/// The serif used for greetings and page titles.
pub const SERIF: &str = "Georgia";
/// The composer grows with its text between these row counts.
pub const COMPOSER_MIN_ROWS: usize = 3;
pub const COMPOSER_MAX_ROWS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    Agent(AgentId),
    Project(String),
    Kept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Board,
    Sessions,
    Session,
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    NeedsYou,
    Planning,
    Working,
    YourTurn,
    Done,
}

impl Column {
    pub const LIVE: [Column; 4] = [Column::NeedsYou, Column::Planning, Column::Working, Column::YourTurn];
    pub fn title(self) -> &'static str {
        match self {
            Column::NeedsYou => "needs you",
            Column::Planning => "planning",
            Column::Working => "working",
            Column::YourTurn => "your turn",
            Column::Done => "done",
        }
    }
    pub fn empty(self) -> &'static str {
        match self {
            Column::NeedsYou => "nothing is waiting on you",
            Column::Planning => "no session is in plan mode",
            Column::Working => "nothing is running",
            Column::YourTurn => "no replies waiting to be read",
            Column::Done => "nothing finished yet",
        }
    }
}

/// What the row under the composer says on its right, and since when.
#[derive(Clone, Debug)]
pub struct Notice {
    pub text: String,
    pub error: bool,
    pub at: f64,
}

/// How long a notice stays under the composer.
pub const NOTICE_SECS: f64 = 8.0;

/// The height of the row under the composer (limits on the left, the
/// notice on the right), taken whether or not either has words.
pub const NOTICE_H: Pixels = px(18.);

impl Notice {
    pub fn said(text: impl Into<String>) -> Self {
        Notice { text: text.into(), error: false, at: now_secs() }
    }
    pub fn error(text: impl Into<String>) -> Self {
        Notice { text: text.into(), error: true, at: now_secs() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DriverView {
    pub state: String,
    pub mode: String,
    pub model: String,
    pub error: String,
    pub queued: usize,
    pub starting: bool,
}

pub struct Detail {
    pub key: String,
    pub path: PathBuf,
    pub session: Rc<Session>,
    pub list: ListState,
    /// Thumbnails of images pasted into prompts, read back out of the
    /// transcript on demand. `None` while a load is in flight.
    pub thumbs: HashMap<String, Option<Thumb>>,
    /// Pixel sizes of kept picture files, read from their headers once.
    pub sizes: HashMap<String, Option<(u32, u32)>>,
    pub open_tools: HashSet<(usize, usize)>,
    pub open_thoughts: HashSet<(usize, usize)>,
    pub open_subagents: HashSet<(usize, usize)>,
    /// Runs of tool calls unfolded by hand, keyed by (round, first item).
    pub open_runs: HashSet<(usize, usize)>,
    /// Tool calls whose plain-words line is showing, by call id.
    pub open_explanations: HashSet<String>,
    /// Where each opened tool body is scrolled to inside its card, and
    /// whose the current wheel stroke is; both must outlive the frame or
    /// the bar and the wheel forget them.
    pub body_scrolls: HashMap<(usize, usize), BodyScroll>,
    /// Everything in the session lowered for the find bar, built the first
    /// time it is needed and dropped whenever the session is reloaded.
    pub find: Option<Rc<FindIndex>>,
}

/// What the settings panel says about updates.
#[derive(Clone, Default)]
pub struct UpdateView {
    pub checking: bool,
    /// A newer version, once a check found one.
    pub available: Option<String>,
    /// When the last check ran (Unix seconds; 0 = never) and what it found.
    pub last_check: f64,
    pub latest: String,
    /// The last check was the daily one, not a click.
    pub automatic: bool,
    /// Whether a check has answered since the panel asked (so "up to
    /// date" is said only after a click).
    pub answered: bool,
    pub error: String,
    /// Bytes fetched and the total, while an installer downloads.
    pub progress: Option<(u64, Option<u64>)>,
    pub installing: bool,
    pub install_error: String,
}

/// A tool body's scroll position and the wheel stroke in progress over it.
/// A stroke is one gesture: on a trackpad it begins when a finger touches
/// (the event's `TouchPhase::Started`; macOS reports the finger's phase,
/// and the momentum that follows a lift comes as phase-less `Moved`
/// events, so a long tail never counts as a new stroke and a fresh touch
/// always does); a mouse wheel has no phases, so a pause of `STROKE_GAP`
/// separates its strokes. Whose a stroke is gets decided on its first
/// event with any travel: the body's when the body has room in that
/// direction, the conversation's when the body is already at that edge. A
/// stroke that reaches the edge midway stops there, tail included, rather
/// than spilling into the conversation, so a long pull never overshoots;
/// the next touch goes on.
#[derive(Clone, Default)]
pub struct BodyScroll {
    pub handle: ScrollHandle,
    pub stroke: Rc<std::cell::RefCell<Stroke>>,
}

#[derive(Clone, Copy, Default)]
pub struct Stroke {
    pub last: Option<std::time::Instant>,
    /// Whether this stroke's owner has been decided yet (a touch may
    /// begin with events that carry no travel).
    pub decided: bool,
    /// True while the stroke belongs to the conversation.
    pub handed_over: bool,
}

/// A pause longer than this between phase-less wheel events (a mouse)
/// begins a new stroke.
pub const STROKE_GAP: Duration = Duration::from_millis(160);

/// A file waiting in the composer. Images go to a driver as content blocks
/// so the model sees the picture; everything else, and everything on the
/// inbox channel, is named by path so Claude can read it with its own tools.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub path: PathBuf,
    pub name: String,
    pub mime: String,
    pub image: bool,
    /// Pixel size of a picture, from its header, so the chip can keep its
    /// shape. `None` when unknown.
    pub size: Option<(u32, u32)>,
}

/// A picture read back out of a transcript, with its size when the header
/// gave one.
#[derive(Clone)]
pub struct Thumb {
    pub image: Arc<gpui::Image>,
    pub size: Option<(u32, u32)>,
}

/// Width and height from the first bytes of a PNG, JPEG, GIF or WebP.
/// A thumbnail is sized from this so the whole picture shows, never a crop.
pub fn image_dims(b: &[u8]) -> Option<(u32, u32)> {
    let be32 = |i: usize| -> Option<u32> { b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])) };
    let be16 = |i: usize| -> Option<u32> { b.get(i..i + 2).map(|s| u16::from_be_bytes([s[0], s[1]]) as u32) };
    let le16 = |i: usize| -> Option<u32> { b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as u32) };
    let le24 = |i: usize| -> Option<u32> { b.get(i..i + 3).map(|s| s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16) };
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    if b.starts_with(b"GIF8") {
        return Some((le16(6)?, le16(8)?));
    }
    if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        return match b.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let (b1, b2, b3, b4) = (*b.get(21)? as u32, *b.get(22)? as u32, *b.get(23)? as u32, *b.get(24)? as u32);
                Some((1 + ((b1 | b2 << 8) & 0x3fff), 1 + ((b2 >> 6 | b3 << 2 | (b4 & 0xf) << 10) & 0x3fff)))
            }
            b"VP8X" => Some((1 + le24(24)?, 1 + le24(27)?)),
            _ => None,
        };
    }
    if b.starts_with(b"\xff\xd8") {
        // Walk the segments to the first start-of-frame.
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            if marker == 0xff {
                i += 1;
                continue;
            }
            if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                return Some((be16(i + 7)?, be16(i + 5)?));
            }
            if matches!(marker, 0xd0..=0xd9) || marker == 0x01 {
                i += 2;
                continue;
            }
            i += 2 + be16(i + 2)? as usize;
        }
    }
    None
}

/// `image_dims` over the head of a file: enough for any header, including a
/// JPEG behind a large EXIF block.
pub fn file_image_dims(path: &std::path::Path) -> Option<(u32, u32)> {
    use std::io::Read as _;
    let mut head = Vec::with_capacity(1 << 18);
    std::fs::File::open(path).ok()?.take(1 << 18).read_to_end(&mut head).ok()?;
    image_dims(&head)
}

/// The box a thumbnail gets: the picture's own shape scaled to fit
/// `max_w` by `max_h`, or `fallback` when its size is unknown.
pub fn fit_thumb(size: Option<(u32, u32)>, max_w: f32, max_h: f32, fallback: (f32, f32)) -> (f32, f32) {
    match size {
        Some((w, h)) if w > 0 && h > 0 => {
            let scale = (max_w / w as f32).min(max_h / h as f32).min(1.0);
            ((w as f32 * scale).max(24.), (h as f32 * scale).max(24.))
        }
        _ => fallback,
    }
}

fn mime_of(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("md") | Some("txt") => "text/plain",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

/// A picture shown large over the window, with the file it came from when
/// there is one.
#[derive(Clone)]
pub struct Lightbox {
    pub title: String,
    pub source: ImageSource,
    pub path: Option<PathBuf>,
}

/// One session as the board sees it.
pub struct Card {
    pub r: SessionRef,
    pub column: Column,
    pub chip: String,
    pub chip_kind: &'static str,
    pub text: String,
    pub pending: Option<PermissionRequest>,
    pub queued: usize,
}

pub struct Workbench {
    pub hub: Arc<Hub>,
    pub cfg: Config,
    pub refs: Vec<SessionRef>,
    pub scope: Scope,
    pub page: Page,
    pub sidebar_open: bool,
    pub selected: Option<String>,
    /// A session to show once the index knows it (opened before the first
    /// scan, or a draft that has just got its file).
    pending_select: Option<String>,
    pub detail: Option<Detail>,
    loading: Option<String>,
    load_task: Option<Task<()>>,
    /// The message being written: plain text, on purpose. Markdown in it
    /// renders once sent, like any prompt.
    pub composer: Entity<TextareaState>,
    pub attachments: Vec<Attachment>,
    /// An attachment opened large over the window.
    pub lightbox: Option<Lightbox>,
    pub search_input: Entity<InputState>,
    pub search_open: bool,
    pub settings_open: bool,
    pub search_results: Option<Results>,
    search_task: Option<Task<()>>,
    /// Find inside the conversation showing (⌘F): the field, whether the
    /// bar is up, every hit in reading order and the one the bar is on.
    pub find_input: Entity<InputState>,
    pub find_open: bool,
    pub find_hits: Vec<Hit>,
    pub find_at: usize,
    /// Once the session being opened has loaded, land on the first hit at
    /// or after this round: how a hit in the search palette opens on its
    /// match.
    find_pending: Option<usize>,
    pub done_open: bool,
    pub permissions: Vec<(String, PermissionRequest)>,
    pub drivers: HashMap<String, DriverView>,
    /// Explanations that landed since the session showing was loaded, by
    /// call id; a reload folds them into the model from the cache.
    pub explanations: HashMap<String, String>,
    /// Calls a model is explaining right now.
    pub explaining: HashSet<String>,
    pub new_cwd: String,
    pub new_id: String,
    pub next_mode: String,
    pub next_model: String,
    /// The account's usage windows and the models' context sizes, as the
    /// last driver turn reported them; kept in `state/limits.json`.
    pub limits: Limits,
    /// The update check and install, as the settings panel shows them.
    pub update: UpdateView,
    /// A line for the person at the right end of the row under the
    /// composer: what a send or an action did, or why it did not. It fades
    /// after `NOTICE_SECS`; the row's space stays, so nothing moves when it
    /// comes and goes.
    pub notice: Option<Notice>,
    pub now: f64,
    /// Who to greet on the home page.
    pub user_name: String,
    /// Sessions with a tab, in tab order. `selected` is the one showing.
    pub tabs: Vec<String>,
    /// Where each tab was scrolled when the reader left it, restored when
    /// the tab is opened again, here or on the next launch.
    scroll_memory: HashMap<String, Scroll>,
    window_rect: Option<Rect>,
    last_ui_save: std::time::Instant,
    /// The window is too narrow for the sidebar beside the content; it is
    /// hidden and only shows over the content, on request (`sidebar_peek`).
    narrow: bool,
    sidebar_peek: bool,
    startup_open: Option<String>,
    focus_handle: FocusHandle,
    _tasks: Vec<Task<()>>,
}

pub fn key_of(r: &SessionRef) -> String {
    format!("{}:{}", r.agent.as_str(), r.session_id)
}

/// Swallow a click without leaving the toolkit's text selection mid-drag.
/// gpui-component's window selection layer begins a drag on every left
/// mouse-down and ends it on the bubble-phase mouse-up; a click handler that
/// only stops propagation eats that mouse-up, and every later mouse move
/// then extends a selection nobody is making.
pub fn swallow_click(window: &mut Window, cx: &mut App) {
    gpui_base::TextSelection::end(window, cx);
    cx.stop_propagation();
}

/// Narrower than this, the sidebar no longer sits beside the content.
const NARROW_W: Pixels = px(880.);

impl Workbench {
    /// Show the sidebar: beside the content when there is room, over it
    /// when the window is narrow.
    fn show_sidebar(&mut self) {
        if self.narrow {
            self.sidebar_peek = true;
        } else {
            self.sidebar_open = true;
            self.save_ui(true);
        }
    }

    fn hide_sidebar(&mut self) {
        if self.narrow {
            self.sidebar_peek = false;
        } else {
            self.sidebar_open = false;
            self.save_ui(true);
        }
    }

    /// The sidebar over the content, for a narrow window: a scrim that any
    /// click closes, so choosing a session in it also puts it away.
    fn render_sidebar_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("sidebar-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .bg(gpui::black().opacity(0.25))
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_peek = false;
                cx.notify();
            }))
            .child(div().absolute().left_0().top_0().h_full().w(SIDEBAR_W).shadow_lg().child(self.render_sidebar(cx)))
    }
}

fn rect_of(b: Bounds<Pixels>) -> Rect {
    Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) }
}



fn greeting(name: &str) -> String {
    let hour = chrono::Local::now().hour();
    let part = if hour < 5 { "Still up" } else if hour < 12 { "Good morning" } else if hour < 18 { "Good afternoon" } else { "Good evening" };
    if name.is_empty() { part.to_string() } else { format!("{part}, {name}") }
}

/// The alias on the wire (`opus`) for a model however it is named: the
/// alias itself, or the id Claude Code reports (`claude-opus-5-5`).
fn model_key(model: &str) -> &'static str {
    MODELS.iter().skip(1).find(|a| model == **a || model.contains(**a)).copied().unwrap_or("default")
}

/// One line on a model, under its name in the picker.
fn model_detail(model: &str) -> &'static str {
    match model {
        "fable" => "The most capable; slowest.",
        "opus" => "Deep work on hard problems.",
        "sonnet" => "The everyday balance of speed and depth.",
        "haiku" => "Fastest and cheapest.",
        _ => "Whatever this account uses by default.",
    }
}

impl Workbench {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = Config::load();
        let (hub, mut rx) = Hub::start(cfg.clone());

        let composer = cx.new(|cx| TextareaState::new(window, cx).placeholder("Reply…  (⌘↩ to send)").auto_grow(COMPOSER_MIN_ROWS, COMPOSER_MAX_ROWS));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search every conversation, live and kept"));
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in this conversation"));

        // Every headless child is ours to close: a driver left running after
        // the window is gone would keep writing to a transcript nobody reads.
        let hub_for_quit = Arc::clone(&hub);
        cx.on_app_quit(move |_, _| {
            hub_for_quit.stop_all();
            async {}
        })
        .detach();
        // The window's state outlives it: tabs, page, sidebar, bounds.
        cx.on_app_quit(|this, _| {
            this.save_ui(true);
            async {}
        })
        .detach();
        cx.observe_window_bounds(window, |this, window, _| {
            this.window_rect = Some(rect_of(window.bounds()));
            this.save_ui(false);
        })
        .detach();

        let mut tasks = Vec::new();
        tasks.push(cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                if this.update(cx, |this, cx| this.on_hub_event(ev, cx)).is_err() {
                    break;
                }
            }
        }));
        tasks.push(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this
                .update(cx, |this, cx| {
                    this.now = now_secs();
                    this.limits.refresh_from_statusline();
                    this.check_updates_daily();
                    if this.notice.as_ref().is_some_and(|n| this.now - n.at > NOTICE_SECS) {
                        this.notice = None;
                        cx.notify();
                    }
                    if this.page == Page::Board || this.drivers.values().any(|d| d.state == "running" || d.starting) {
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
        }));

        cx.subscribe_in(&search_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => this.run_search(window, cx),
            InputEvent::PressEnter { .. } => {
                if let Some(first) = this.search_results.as_ref().and_then(|r| r.sessions.first()) {
                    let key = format!("{}:{}", first.agent, first.id);
                    this.close_search(window, cx);
                    this.open_session(&key, cx);
                }
            }
            _ => {}
        })
        .detach();
        // ⌘↩ never gets here: the composer wrapper captures it and sends
        // (see `render_composer`), so what arrives is a bare or shifted ↩.
        cx.subscribe_in(&composer, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { secondary: false, shift } = ev {
                // A bare ↩ on an empty composer answers the oldest
                // permission card, ⇧↩ turns it down; with words typed
                // they stay what they are, a new line.
                this.answer_pending_by_key(!*shift, window, cx);
            }
        })
        .detach();
        cx.subscribe_in(&find_input, window, |this, _, ev: &InputEvent, _window, cx| match ev {
            InputEvent::Change => this.run_find(cx),
            InputEvent::PressEnter { shift, .. } => this.find_step(if *shift { -1 } else { 1 }, cx),
            _ => {}
        })
        .detach();

        let next_mode = cfg.driver.default_mode.clone();
        let next_model = cfg.driver.default_model.clone();
        // Last time's tabs and sidebar come back; the window opens on the
        // new-session page, as the Claude app opens on a new chat.
        // `EMAKI_OPEN=<session-id prefix>` opens a session on launch instead
        // and `EMAKI_PAGE=new|sessions|board` picks the page. For probing
        // the window from a script with no accessibility access,
        // `EMAKI_FIND=<text>` opens the find bar on that query and
        // `EMAKI_SETTINGS=1` opens the settings panel.
        let ui = UiState::load();
        let startup_open = std::env::var("EMAKI_OPEN").ok().filter(|s| !s.is_empty());
        let startup_find = std::env::var("EMAKI_FIND").ok().filter(|s| !s.is_empty());
        if let Some(q) = &startup_find {
            find_input.update(cx, |s, cx| s.set_value(q.clone(), window, cx));
        }
        let settings_open = std::env::var("EMAKI_SETTINGS").is_ok();
        let page = match std::env::var("EMAKI_PAGE").as_deref() {
            Ok("sessions") => Page::Sessions,
            Ok("board") => Page::Board,
            _ => Page::New,
        };
        Self {
            hub,
            cfg,
            refs: Vec::new(),
            scope: Scope::All,
            page,
            sidebar_open: ui.sidebar_open.unwrap_or(true),
            selected: None,
            pending_select: None,
            detail: None,
            loading: None,
            load_task: None,
            composer,
            attachments: Vec::new(),
            lightbox: None,
            search_input,
            search_open: false,
            settings_open,
            search_results: None,
            search_task: None,
            find_input,
            find_open: startup_find.is_some(),
            find_hits: Vec::new(),
            find_at: 0,
            find_pending: startup_find.map(|_| 0),
            done_open: false,
            permissions: Vec::new(),
            drivers: HashMap::new(),
            explanations: HashMap::new(),
            explaining: HashSet::new(),
            new_cwd: String::new(),
            new_id: String::new(),
            next_mode,
            next_model,
            limits: {
                let mut l = Limits::load();
                l.refresh_from_statusline();
                l
            },
            update: {
                let s = UpdateState::load();
                UpdateView { last_check: s.last_check, latest: s.latest, ..Default::default() }
            },
            notice: None,
            now: now_secs(),
            user_name: crate::sys::user_first_name(),
            tabs: ui.tabs.clone(),
            scroll_memory: ui.scroll.clone(),
            window_rect: Some(rect_of(window.bounds())),
            last_ui_save: std::time::Instant::now(),
            narrow: false,
            sidebar_peek: false,
            startup_open,
            focus_handle: cx.focus_handle(),
            _tasks: tasks,
        }
    }

    pub fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }

    // -- events -----------------------------------------------------------

    fn on_hub_event(&mut self, ev: HubEvent, cx: &mut Context<Self>) {
        match ev {
            HubEvent::Index(refs) => {
                self.refs = refs;
                if self.page == Page::New && self.new_cwd.is_empty() {
                    self.new_cwd = self.recent_cwds().first().cloned().unwrap_or_default();
                }
                if let Some(prefix) = self.startup_open.take() {
                    if let Some(r) = self.refs.iter().find(|r| r.session_id.starts_with(&prefix)) {
                        let key = key_of(r);
                        self.open_session(&key, cx);
                    }
                }
                if let Some(key) = self.pending_select.clone() {
                    if self.refs.iter().any(|r| key_of(r) == key) {
                        self.pending_select = None;
                        self.open_session(&key, cx);
                    }
                }
                // A selected session that grew is reloaded even when the
                // watcher missed it.
                if let Some(d) = &self.detail {
                    if let Some(r) = self.refs.iter().find(|r| key_of(r) == d.key) {
                        let changed = r.path != d.path || r.updated > d.session.updated;
                        if changed && self.loading.is_none() {
                            self.load_detail(r.clone(), cx);
                        }
                    }
                }
                cx.notify();
            }
            HubEvent::Changed(path) => {
                if let Some(d) = &self.detail {
                    let same = d.path == path || path.starts_with(d.path.with_extension(""));
                    if same && self.loading.is_none() {
                        if let Some(r) = self.refs.iter().find(|r| r.path == d.path).cloned() {
                            self.load_detail(r, cx);
                        }
                    }
                }
            }
            HubEvent::DriverStarted { session_id } => {
                let view = self.drivers.entry(session_id.clone()).or_default();
                view.starting = false;
                view.state = "idle".into();
                if let Some(d) = self.hub.driver_for(&session_id) {
                    view.mode = d.mode();
                    view.model = d.model();
                }
                cx.notify();
            }
            HubEvent::DriverFailed { session_id, error } => {
                let view = self.drivers.entry(session_id).or_default();
                view.starting = false;
                view.state = "exited".into();
                view.error = error.clone();
                self.notice = Some(Notice::error(format!("could not start claude: {error}")));
                cx.notify();
            }
            HubEvent::Sent { session_id, queued, error } => {
                // A message that went through shows up in the transcript,
                // which says it better than a "sent" would; only what did
                // not go, or is waiting, is worth a line.
                if !error.is_empty() {
                    self.notice = Some(Notice::error(format!("not sent: {error}")));
                } else if queued {
                    self.notice = Some(Notice::said("queued behind the running turn"));
                }
                if let Some(v) = self.drivers.get_mut(&session_id) {
                    if error.is_empty() && !queued {
                        v.state = "running".into();
                    }
                }
                cx.notify();
            }
            HubEvent::Driver { session_id, event } => self.on_driver_event(session_id, event, cx),
            HubEvent::Note(text) => {
                self.notice = Some(Notice::error(text));
                cx.notify();
            }
            HubEvent::Explained { call_id, text } => {
                self.explaining.remove(&call_id);
                if !text.is_empty() {
                    self.explanations.insert(call_id, text);
                }
                cx.notify();
            }
            HubEvent::Update(ev) => self.on_update_event(ev, cx),
        }
    }

    // -- updates ------------------------------------------------------------

    /// Once a day, when the setting allows: the same check the button
    /// makes, but quiet unless it finds something.
    fn check_updates_daily(&mut self) {
        if !self.cfg.app.check_updates || self.update.checking || self.update.installing {
            return;
        }
        if self.now - self.update.last_check < 86_400.0 {
            return;
        }
        self.update.last_check = self.now;
        self.update.automatic = true;
        self.hub.check_updates();
    }

    fn check_updates_now(&mut self, cx: &mut Context<Self>) {
        if self.update.checking {
            return;
        }
        self.update.automatic = false;
        self.update.answered = false;
        self.update.error.clear();
        self.hub.check_updates();
        cx.notify();
    }

    fn install_update_now(&mut self, cx: &mut Context<Self>) {
        let Some(v) = self.update.available.clone() else { return };
        if self.update.installing {
            return;
        }
        self.update.installing = true;
        self.update.install_error.clear();
        self.update.progress = Some((0, None));
        self.hub.install_update(v);
        cx.notify();
    }

    fn set_check_updates(&mut self, on: bool, cx: &mut Context<Self>) {
        self.cfg.app.check_updates = on;
        if let Err(e) = Config::edit(move |c| c.app.check_updates = on) {
            self.notice = Some(Notice::error(format!("could not save settings: {e}")));
        }
        cx.notify();
    }

    fn on_update_event(&mut self, ev: UpdateEvent, cx: &mut Context<Self>) {
        match ev {
            UpdateEvent::Checking => self.update.checking = true,
            UpdateEvent::UpToDate { latest } => {
                self.update.checking = false;
                self.update.answered = true;
                self.update.last_check = self.now;
                self.update.latest = latest;
                self.update.available = None;
            }
            UpdateEvent::Available { latest } => {
                self.update.checking = false;
                self.update.answered = true;
                self.update.last_check = self.now;
                self.update.latest = latest.clone();
                self.update.available = Some(latest.clone());
                if self.update.automatic {
                    self.notice = Some(Notice::said(format!("Emaki {latest} is available. Update from Settings (⌘,).")));
                }
            }
            UpdateEvent::CheckFailed(e) => {
                self.update.checking = false;
                self.update.answered = true;
                if !self.update.automatic {
                    self.update.error = e;
                }
            }
            UpdateEvent::Downloading { done, total } => self.update.progress = Some((done, total)),
            UpdateEvent::Installing => self.update.progress = None,
            UpdateEvent::Relaunch => {
                // The new app is already running; this one leaves.
                self.save_ui(true);
                cx.quit();
            }
            UpdateEvent::InstallFailed(e) => {
                self.update.installing = false;
                self.update.progress = None;
                self.update.install_error = e;
            }
        }
        cx.notify();
    }

    /// The line under a tool call: what the transcript carries (a cached
    /// answer folded in at load), else what landed since.
    pub fn explanation_for(&self, call: &emaki_core::model::ToolCall) -> String {
        if !call.explanation.is_empty() {
            return call.explanation.clone();
        }
        self.explanations.get(&call.id).cloned().unwrap_or_default()
    }

    /// The Explain pill on a tool row: show the plain-words line, or hide
    /// it again. A line already known (in the model, the overlay or the
    /// cache) shows at once; only a call never explained asks the model.
    pub fn toggle_explain(&mut self, call_id: &str, name: &str, input: &serde_json::Map<String, serde_json::Value>, has_text: bool, cx: &mut Context<Self>) {
        self.pin_scroll();
        let Some(d) = self.detail.as_mut() else { return };
        if d.open_explanations.remove(call_id) {
            cx.notify();
            return;
        }
        d.open_explanations.insert(call_id.to_string());
        if !has_text && !self.explaining.contains(call_id) {
            let cached = self.hub.explainer.lookup(name, input);
            if !cached.is_empty() {
                self.explanations.insert(call_id.to_string(), cached);
            } else {
                self.explaining.insert(call_id.to_string());
                self.hub.explainer.request(call_id, name, input, true);
            }
        }
        cx.notify();
    }

    /// Keep the row just clicked where it is when what it opens makes its
    /// item taller. Scrolled to the very end, the list has no logical top
    /// (it hangs from its bottom and grows upward), so an opened card would
    /// push its own row up the screen. Giving the list a real top first
    /// makes the new content extend below the row instead.
    pub fn pin_scroll(&self) {
        let Some(d) = &self.detail else { return };
        if d.list.logical_scroll_top().item_ix >= d.session.rounds.len() {
            let h = d.list.viewport_bounds().size.height;
            if h > px(0.) {
                d.list.scroll_by(-h);
            }
        }
    }

    /// With the scope on `all`, every opaque call in the tail of a live
    /// session is explained as it appears. Only the last two rounds are
    /// looked at, and only on a reload of the session already showing, so
    /// opening an old session never spends anything.
    fn request_live_explanations(&mut self) {
        if self.cfg.explain.scope != "all" {
            return;
        }
        let Some(d) = &self.detail else { return };
        let session = Rc::clone(&d.session);
        let start = session.rounds.len().saturating_sub(2);
        for rnd in &session.rounds[start..] {
            for item in &rnd.items {
                if let Item::Tool(call) = item {
                    let known = !call.explanation.is_empty() || self.explanations.contains_key(&call.id) || self.explaining.contains(&call.id);
                    if !known && self.hub.explainer.needs_model(&call.name, &call.input) {
                        self.explaining.insert(call.id.clone());
                        self.hub.explainer.request(&call.id, &call.name, &call.input, false);
                    }
                }
            }
        }
    }

    fn on_driver_event(&mut self, session_id: String, event: driver::Event, cx: &mut Context<Self>) {
        let view = self.drivers.entry(session_id.clone()).or_default();
        match event {
            driver::Event::Init(caps) => {
                if !caps.mode.is_empty() {
                    view.mode = caps.mode;
                }
                if !caps.model.is_empty() {
                    view.model = caps.model;
                }
            }
            driver::Event::Turn { queued, .. } => {
                view.state = "running".into();
                view.queued = queued;
            }
            driver::Event::Result(r) => {
                view.queued = r.queued;
                view.state = if r.queued > 0 { "running".into() } else { "idle".into() };
                if r.is_error {
                    self.notice = Some(Notice::error(format!("turn ended: {}", r.subtype)));
                }
                if self.limits.absorb_model_usage(&r.model_usage) {
                    let _ = self.limits.save();
                }
            }
            driver::Event::RateLimit(info) => {
                if self.limits.absorb_rate_limit(&info, self.now) {
                    let _ = self.limits.save();
                }
            }
            driver::Event::Mode(m) => view.mode = m,
            driver::Event::Permission(req) => self.permissions.push((session_id, req)),
            driver::Event::PermissionSettled(id) => self.permissions.retain(|(_, r)| r.request_id != id),
            driver::Event::Exit { error, .. } => {
                view.state = "exited".into();
                view.error = error.clone();
                view.starting = false;
                self.permissions.retain(|(s, _)| s != &session_id);
                if !error.is_empty() {
                    self.notice = Some(Notice::error(format!("claude exited: {error}")));
                }
            }
        }
        cx.notify();
    }

    // -- navigation ----------------------------------------------------------

    pub fn open_session(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() != Some(key) {
            self.remember_scroll();
        }
        if !self.tabs.iter().any(|t| t == key) {
            self.tabs.push(key.to_string());
        }
        self.page = Page::Session;
        self.selected = Some(key.to_string());
        self.sidebar_peek = false;
        self.save_ui(true);
        if let Some(r) = self.refs.iter().find(|r| key_of(r) == key).cloned() {
            if self.detail.as_ref().map(|d| d.key != key).unwrap_or(true) {
                self.detail = None;
            }
            self.load_detail(r, cx);
        } else {
            self.pending_select = Some(key.to_string());
        }
        cx.notify();
    }

    /// Note where the reader is in the session showing, so the same spot
    /// comes back when its tab is opened again.
    fn remember_scroll(&mut self) {
        if let Some(d) = &self.detail {
            let top = d.list.logical_scroll_top();
            self.scroll_memory.insert(d.key.clone(), Scroll { item: top.item_ix, offset: f32::from(top.offset_in_item) });
        }
    }

    /// Close a tab. The session goes on without it: a driver behind it
    /// keeps running until its idle timeout, and the transcript is on disk.
    /// Closing the showing tab moves to its neighbour, or to the new-session
    /// page when it was the last one, with the caret in its composer.
    pub fn close_tab(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t == key) else { return };
        if self.selected.as_deref() == Some(key) {
            self.remember_scroll();
        }
        self.tabs.remove(ix);
        if self.selected.as_deref() == Some(key) {
            self.selected = None;
            self.detail = None;
            self.loading = None;
            self.load_task = None;
            match self.tabs.get(ix.min(self.tabs.len().saturating_sub(1))).cloned() {
                Some(next) if !self.tabs.is_empty() => self.open_session(&next, cx),
                _ => self.show_new(None, window, cx),
            }
        }
        self.save_ui(true);
        cx.notify();
    }

    /// Write the window's state to `state/ui.json`. `now` forces it; otherwise
    /// writes are spaced two seconds apart, which is enough for a window being
    /// dragged.
    fn save_ui(&mut self, now: bool) {
        if !now && self.last_ui_save.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.remember_scroll();
        let page = match self.page {
            Page::Board => "board",
            Page::Sessions => "sessions",
            Page::Session => "session",
            Page::New => "new",
        };
        let scroll = self.scroll_memory.iter().filter(|(k, _)| self.tabs.contains(k)).map(|(k, v)| (k.clone(), *v)).collect();
        UiState {
            window: self.window_rect,
            sidebar_open: Some(self.sidebar_open),
            page: page.into(),
            tabs: self.tabs.clone(),
            active: self.selected.clone().filter(|_| self.page == Page::Session),
            scroll,
        }
        .save();
        self.last_ui_save = std::time::Instant::now();
    }

    /// Open a session and put the caret in the composer, as a click on a
    /// recent does.
    pub fn open_and_focus(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_session(key, cx);
        self.focus_composer(window, cx);
    }

    pub fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |s, cx| s.focus(window, cx));
    }

    fn show_sessions(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.scope = scope;
        self.page = Page::Sessions;
        self.sidebar_peek = false;
        self.save_ui(true);
        cx.notify();
    }

    fn load_detail(&mut self, r: SessionRef, cx: &mut Context<Self>) {
        let key = key_of(&r);
        self.loading = Some(key.clone());
        let path = r.path.clone();
        // EMAKI_TIMING=1 prints how long the load and the hand-over to the
        // list took, per open, so a slow session can be measured, not guessed.
        let timing = std::env::var_os("EMAKI_TIMING").is_some();
        let started = std::time::Instant::now();
        let task = cx.background_spawn(async move { adapters::for_agent(r.agent).load(&r) });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let mut session = task.await;
            let loaded = started.elapsed();
            this.update(cx, |this, cx| {
                this.loading = None;
                if this.selected.as_deref() != Some(key.as_str()) {
                    return;
                }
                let rounds = session.rounds.len();
                let t = std::time::Instant::now();
                // Cached explanations are free and belong to every session
                // they match, however old.
                this.hub.explainer.attach(&mut session);
                let reload = this.detail.as_ref().map(|d| d.key == key).unwrap_or(false);
                this.set_detail(key.clone(), path, session);
                if reload {
                    this.request_live_explanations();
                }
                this.after_detail_loaded(cx);
                if timing {
                    eprintln!("emaki: open {} — load {}ms, set_detail {}ms, {} rounds", &key, loaded.as_millis(), t.elapsed().as_millis(), rounds);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn set_detail(&mut self, key: String, path: PathBuf, session: Session) {
        let n = session.rounds.len();
        match self.detail.as_mut() {
            Some(d) if d.key == key => {
                let old = d.session.rounds.len();
                d.session = Rc::new(session);
                d.path = path;
                d.find = None;
                // A splice that covers the item the reader is scrolled into
                // moves the scroll anchor to that item's top: for a live
                // session whose last round is one tall item, that is a jump
                // to the middle of the conversation on every reload. Items
                // re-render from the new session anyway, so only the count
                // is told to the list: new rounds are appended, a rewrite
                // (fewer rounds) resets.
                if n > old {
                    d.list.splice(old..old, n - old);
                } else if n < old {
                    d.list.reset(n);
                }
            }
            _ => {
                let list = ListState::new(n, ListAlignment::Bottom, px(512.));
                if let Some(s) = self.scroll_memory.get(&key) {
                    if s.item < n {
                        list.scroll_to(ListOffset { item_ix: s.item, offset_in_item: px(s.offset) });
                    }
                }
                self.detail = Some(Detail {
                    key,
                    path,
                    session: Rc::new(session),
                    list,
                    thumbs: HashMap::new(),
                    sizes: HashMap::new(),
                    open_tools: HashSet::new(),
                    open_thoughts: HashSet::new(),
                    open_subagents: HashSet::new(),
                    open_runs: HashSet::new(),
                    open_explanations: HashSet::new(),
                    body_scrolls: HashMap::new(),
                    find: None,
                });
            }
        }
    }

    pub fn selected_ref(&self) -> Option<&SessionRef> {
        let key = self.selected.as_deref()?;
        self.refs.iter().find(|r| key_of(r) == key)
    }

    fn show_new(&mut self, cwd: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_scroll();
        self.page = Page::New;
        self.selected = None;
        self.sidebar_peek = false;
        self.save_ui(true);
        if let Some(c) = cwd {
            self.new_cwd = c;
        } else if self.new_cwd.is_empty() {
            self.new_cwd = self.recent_cwds().first().cloned().unwrap_or_default();
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    // -- settings ----------------------------------------------------------

    /// Write the look section of `config.json` as this window now has it.
    fn save_app_config(&mut self) {
        let app = self.cfg.app.clone();
        if let Err(e) = Config::edit(move |c| c.app = app) {
            self.notice = Some(Notice::error(format!("could not save settings: {e}")));
        }
    }

    fn set_chat_font(&mut self, choice: &str, cx: &mut Context<Self>) {
        self.cfg.app.chat_font = choice.to_string();
        self.save_app_config();
        cx.notify();
    }

    fn set_chat_size(&mut self, choice: &str, cx: &mut Context<Self>) {
        self.cfg.app.chat_size = choice.to_string();
        self.save_app_config();
        cx.notify();
    }

    /// System, light or dark: drawn now and kept for the next launch.
    fn set_appearance(&mut self, choice: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.cfg.app.appearance = choice.to_string();
        self.save_app_config();
        crate::look::apply(choice, Some(window), cx);
        cx.notify();
    }

    /// A new accent is painted into both palettes, then the current
    /// appearance is drawn again with it.
    fn set_accent(&mut self, choice: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.cfg.app.accent = choice.to_string();
        self.save_app_config();
        crate::look::install(choice, cx);
        crate::look::apply(&self.cfg.app.appearance, Some(window), cx);
        cx.notify();
    }

    /// What a session started from this window begins in. A running
    /// session's own pills under the composer change it for that session.
    fn set_default_mode(&mut self, choice: &str, cx: &mut Context<Self>) {
        let v = if choice == "default" { String::new() } else { choice.to_string() };
        self.cfg.driver.default_mode = v.clone();
        self.next_mode = v.clone();
        if let Err(e) = Config::edit(move |c| c.driver.default_mode = v) {
            self.notice = Some(Notice::error(format!("could not save settings: {e}")));
        }
        cx.notify();
    }

    fn set_default_model(&mut self, choice: &str, cx: &mut Context<Self>) {
        let v = if choice == "default" { String::new() } else { choice.to_string() };
        self.cfg.driver.default_model = v.clone();
        self.next_model = v.clone();
        if let Err(e) = Config::edit(move |c| c.driver.default_model = v) {
            self.notice = Some(Notice::error(format!("could not save settings: {e}")));
        }
        cx.notify();
    }

    fn set_explain_scope(&mut self, choice: &'static str, cx: &mut Context<Self>) {
        self.cfg.explain.scope = choice.to_string();
        self.hub.set_explain(self.cfg.explain.clone());
        if let Err(e) = Config::edit(move |c| c.explain.scope = choice.to_string()) {
            self.notice = Some(Notice::error(format!("could not save settings: {e}")));
        }
        cx.notify();
    }

    /// The settings panel (⌘,): how the window looks (appearance, accent,
    /// the conversation's face and size) and what a new session starts
    /// with (permission mode, model). Every choice is a row of pills, and
    /// every change is drawn at once and written to `config.json`.
    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let fonts = cx.global::<crate::fonts::ChatFonts>().clone();
        let app = self.cfg.app.clone();
        let dark = theme.mode.is_dark();

        // One pill: bordered, filled with the ink when it is the choice.
        let pill = |id: String, label: String, active: bool, family: Option<String>, cx: &mut Context<Self>, on: Box<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>| {
            let theme = cx.theme().clone();
            h_flex()
                .id(SharedString::from(id))
                .h(px(30.))
                .px(px(12.))
                .items_center()
                .rounded_full()
                .cursor_pointer()
                .text_size(px(13.))
                .border_1()
                .border_color(if active { theme.foreground } else { theme.border })
                .when(active, |d| d.bg(theme.foreground).text_color(theme.background))
                .when(!active, |d| d.hover(|s| s.bg(theme.muted)))
                .when_some(family, |d, f| d.font_family(f))
                .on_click(cx.listener(move |this, _, window, cx| on(this, window, cx)))
                .child(label)
        };
        // A row: a title and a line on it at the left, the choices at the right.
        let row = |title: &'static str, detail: &'static str, control: AnyElement, theme: &gpui_component::Theme| {
            h_flex()
                .items_center()
                .gap(px(12.))
                .child(v_flex().flex_1().min_w_0().gap(px(2.)).child(div().text_size(px(13.5)).child(title)).child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(detail)))
                .child(control)
        };
        let heading = |text: &'static str, theme: &gpui_component::Theme| div().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(text);

        let appearance = h_flex().gap(px(6.)).children([("system", "System"), ("light", "Light"), ("dark", "Dark")].into_iter().map(|(key, label)| {
            pill(format!("appearance-{key}"), label.into(), app.appearance == key, None, cx, Box::new(move |this, window, cx| this.set_appearance(key, window, cx)))
        }));

        // Accents are swatches: a disc of the colour, ringed when chosen.
        let accents = h_flex().gap(px(8.)).children(crate::look::ACCENTS.iter().map(|a| {
            let active = app.accent == a.key;
            let color: Hsla = gpui::Rgba::try_from(crate::look::accent_hex(a.key, dark)).map(Hsla::from).unwrap_or(theme.primary);
            let key = a.key;
            div()
                .id(SharedString::from(format!("accent-{}", a.key)))
                .size(px(26.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .border_2()
                .border_color(if active { theme.foreground } else { theme.transparent })
                .hover(|s| s.border_color(theme.muted_foreground))
                .tooltip({
                    let name = a.name;
                    move |window, cx| gpui_component::tooltip::Tooltip::new(name).build(window, cx)
                })
                .on_click(cx.listener(move |this, _, window, cx| this.set_accent(key, window, cx)))
                .child(div().size(px(18.)).rounded_full().bg(color))
        }));

        let serif_family = Some(fonts.serif.clone().unwrap_or_else(|| crate::fonts::SERIF_FALLBACK.into()));
        let sans_family = fonts.sans.clone();
        let font = h_flex()
            .gap(px(6.))
            .child(pill("font-serif".into(), "Anthropic Serif".into(), app.chat_font == "serif", serif_family, cx, Box::new(|this, _, cx| this.set_chat_font("serif", cx))))
            .child(pill("font-sans".into(), "Anthropic Sans".into(), app.chat_font == "sans", sans_family, cx, Box::new(|this, _, cx| this.set_chat_font("sans", cx))));
        let size = h_flex().gap(px(6.)).children([("small", "Small"), ("medium", "Medium"), ("large", "Large")].into_iter().map(|(key, label)| {
            pill(format!("size-{key}"), label.into(), app.chat_size == key, None, cx, Box::new(move |this, _, cx| this.set_chat_size(key, cx)))
        }));
        let note = match &fonts.source {
            Some(dir) => format!("Anthropic Serif and Anthropic Sans are loaded from the Claude app at {}.", emaki_core::paths::tilde(&dir.to_string_lossy())),
            None => "Anthropic Serif and Anthropic Sans are the Claude desktop app's fonts and are loaded from it when it is installed. It was not found here, so Georgia stands in for the serif and the window's own face for the sans.".to_string(),
        };

        let default_mode = if self.cfg.driver.default_mode.is_empty() { "default".to_string() } else { self.cfg.driver.default_mode.clone() };
        // Short names here; the picker under the composer has the long ones
        // with a line on each.
        let short_mode = |m: &str| match m {
            "acceptEdits" => "Accept edits",
            "plan" => "Plan",
            "auto" => "Auto",
            "bypassPermissions" => "Bypass",
            _ => "Default",
        };
        let modes = h_flex().gap(px(6.)).flex_wrap().justify_end().max_w(px(360.)).children(self.modes().into_iter().map(|m| {
            pill(format!("default-mode-{m}"), short_mode(m).into(), default_mode == m, None, cx, Box::new(move |this, _, cx| this.set_default_mode(m, cx)))
        }));
        let default_model = if self.cfg.driver.default_model.is_empty() { "default".to_string() } else { self.cfg.driver.default_model.clone() };
        let models = h_flex().gap(px(6.)).flex_wrap().justify_end().max_w(px(360.)).children(MODELS.iter().map(|m| {
            let label = if *m == "default" { "Default".to_string() } else { model_label(m) };
            pill(format!("default-model-{m}"), label, default_model == *m, None, cx, Box::new(move |this, _, cx| this.set_default_model(m, cx)))
        }));

        let scope = if self.cfg.explain.enabled { self.cfg.explain.scope.as_str() } else { "off" };
        let explain = h_flex().gap(px(6.)).children([("off", "Off"), ("permission", "Permission cards"), ("all", "Every new call")].into_iter().map(|(key, label)| {
            pill(format!("explain-{key}"), label.into(), scope == key, None, cx, Box::new(move |this, _, cx| this.set_explain_scope(key, cx)))
        }));
        // Updates: the version, what the last check said, and the buttons.
        let u = &self.update;
        let current = update::current_version();
        let update_line = if u.installing {
            match u.progress {
                Some((done, Some(total))) if total > 0 => format!("Downloading {}… {}%", u.available.as_deref().unwrap_or(""), done * 100 / total),
                Some(_) => format!("Downloading {}…", u.available.as_deref().unwrap_or("")),
                None => "Installing… Emaki restarts by itself.".to_string(),
            }
        } else if !u.install_error.is_empty() {
            format!("The update did not go through: {}", u.install_error)
        } else if u.checking {
            "Looking for a newer version…".to_string()
        } else if !u.error.is_empty() {
            format!("Could not check: {}.", u.error)
        } else if let Some(v) = &u.available {
            format!("Emaki {v} is available.")
        } else if u.answered && !u.automatic {
            "This is the newest version.".to_string()
        } else if u.last_check > 0.0 {
            format!("Last checked {}.", relative(u.last_check, self.now))
        } else {
            "Not checked yet.".to_string()
        };
        let can_install = u.available.is_some() && !u.installing && update::asset_name().is_some();
        let checking = u.checking;
        let installing = u.installing;
        let notes_url = u.available.as_deref().map(update::release_page);
        let version_row = h_flex()
            .gap(px(6.))
            .items_center()
            .when(can_install, |d| {
                let label: SharedString = format!("Update to {}", u.available.as_deref().unwrap_or("")).into();
                d.child(pill_button("update-now", label, &theme, cx.listener(|this, _, _, cx| this.install_update_now(cx))))
            })
            .when_some(notes_url, |d, url| d.child(pill_button("update-notes", "Release notes", &theme, move |_, _, _| { let _ = opener::open(&url); })))
            .when(!installing, |d| d.child(pill_button("update-check", if checking { "Checking…" } else { "Check for updates" }, &theme, cx.listener(|this, _, _, cx| this.check_updates_now(cx)))));
        let auto_check = Checkbox::new("update-auto").checked(self.cfg.app.check_updates).label("Once a day").on_click(cx.listener(|this, on: &bool, _, cx| this.set_check_updates(*on, cx)));
        let version_detail: SharedString = format!("Emaki {current}. {update_line}").into();

        let explain_model = if self.cfg.explain.model.is_empty() { "claude-haiku-4-5".to_string() } else { self.cfg.explain.model.clone() };
        let explain_note = format!(
            "An opaque call (a heredoc, a piped chain, anything long) gets one or two plain sentences from {} through your own Claude Code login. Simple calls explain themselves for free, answers are kept by content so a command is explained once, and every tool card has an Explain button.",
            model_label(&explain_model)
        );

        // The scrim is a flex box, so the panel sits in the middle of the
        // window both ways (an absolute panel with auto margins did not).
        div()
            .id("settings-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_open = false;
                cx.notify();
            }))
            .child(
                v_flex()
                    .id("settings-panel")
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(px(600.))
                    .max_w(gpui::relative(0.94))
                    .max_h(gpui::relative(0.9))
                    .rounded(px(16.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .px(px(18.))
                            .h(px(50.))
                            .flex_shrink_0()
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(div().flex_1().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
                            .child(Button::new("settings-close").ghost().small().icon(Icon::new(IconName::Close)).tooltip("Close (esc)").on_click(cx.listener(|this, _, _, cx| {
                                this.settings_open = false;
                                cx.notify();
                            }))),
                    )
                    .child(
                        v_flex()
                            .id("settings-body")
                            .min_h_0()
                            .overflow_y_scroll()
                            .p(px(18.))
                            .gap(px(14.))
                            .child(heading("APPEARANCE", &theme))
                            .child(row("Theme", "Follow the system, or keep one look.", appearance.into_any_element(), &theme))
                            .child(row("Accent", "The colour of the send button, links and the mark.", accents.into_any_element(), &theme))
                            .child(row("Chat font", "The face the conversation is set in.", font.into_any_element(), &theme))
                            .child(row("Text size", "How large the conversation reads.", size.into_any_element(), &theme))
                            .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(note))
                            .child(div().h(px(4.)))
                            .child(heading("NEW SESSIONS", &theme))
                            .child(row("Permission mode", "What a session started here begins in.", modes.into_any_element(), &theme))
                            .child(row("Model", "Which model a session started here uses.", models.into_any_element(), &theme))
                            .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("A running session keeps its own choices: the pills under its composer change them for that session, and ⇧Tab in the composer steps through the modes."))
                            .child(div().h(px(4.)))
                            .child(heading("EXPLANATIONS", &theme))
                            .child(row("Explain tool calls", "Which calls are put into plain words without asking.", explain.into_any_element(), &theme))
                            .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(explain_note))
                            .child(div().h(px(4.)))
                            .child(heading("UPDATES", &theme))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(v_flex().flex_1().min_w_0().gap(px(2.)).child(div().text_size(px(13.5)).child("Version")).child(div().text_size(px(12.)).text_color(theme.muted_foreground).whitespace_normal().child(version_detail)))
                                    .child(version_row),
                            )
                            .child(row("Check for updates automatically", "Asks GitHub for the newest release once a day and says so here. Nothing is installed unasked.", auto_check.into_any_element(), &theme))
                            .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("Updating fetches the installer for this machine, puts it in place and restarts Emaki. Tabs and the page come back as they were.")),
                    ),
            )
    }

    // -- find in the conversation ------------------------------------------

    /// ⌘F: put the find bar over the conversation showing, with the caret
    /// in it. Nothing to find in on any other page.
    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != Page::Session {
            return;
        }
        self.find_open = true;
        self.find_input.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.run_find(cx);
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        self.find_hits.clear();
        self.find_at = 0;
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Open a session on a match from the search palette: the query goes
    /// into the find bar and the first hit in `round` (numbered from one,
    /// as the index numbers them) is the one the bar lands on once the
    /// session has loaded.
    fn open_with_find(&mut self, key: &str, query: &str, round: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.find_input.update(cx, |s, cx| s.set_value(query.to_string(), window, cx));
        self.find_open = true;
        self.find_pending = round.map(|r| r.saturating_sub(1));
        self.open_session(key, cx);
        self.find_input.update(cx, |s, cx| s.focus(window, cx));
    }

    /// The session just loaded (or reloaded): keep the find bar's answer
    /// current without moving the reader, unless a palette hit asked to.
    fn after_detail_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        self.compute_hits(cx);
        if let Some(round) = self.find_pending.take() {
            if let Some(i) = self.find_hits.iter().position(|h| h.round >= round) {
                self.find_at = i;
            }
            self.reveal_current(cx);
        }
    }

    /// Scan the session for the query, keeping the current hit where it
    /// was when it still exists.
    fn compute_hits(&mut self, cx: &mut Context<Self>) {
        let q = self.find_input.read(cx).value().to_string();
        let Some(d) = self.detail.as_mut() else {
            self.find_hits.clear();
            return;
        };
        if d.find.is_none() {
            d.find = Some(Rc::new(FindIndex::build(&d.session)));
        }
        let idx = d.find.clone().unwrap();
        let was = self.find_hits.get(self.find_at).copied();
        self.find_hits = idx.find(&q);
        self.find_at = was.and_then(|w| self.find_hits.iter().position(|h| *h == w)).unwrap_or(0).min(self.find_hits.len().saturating_sub(1));
    }

    /// The query changed: scan again and land on the first hit at or after
    /// the round in view, so typing takes the reader to the nearest match.
    fn run_find(&mut self, cx: &mut Context<Self>) {
        self.compute_hits(cx);
        let top = self.detail.as_ref().map(|d| d.list.logical_scroll_top().item_ix).unwrap_or(0);
        self.find_at = self.find_hits.iter().position(|h| h.round >= top).unwrap_or(0);
        if self.find_hits.is_empty() {
            cx.notify();
        } else {
            self.reveal_current(cx);
        }
    }

    /// ↩ / ⌘G forward, ⇧↩ / ⌘⇧G back, wrapping at either end.
    fn find_step(&mut self, delta: i64, cx: &mut Context<Self>) {
        let n = self.find_hits.len() as i64;
        if n == 0 {
            return;
        }
        self.find_at = (self.find_at as i64 + delta).rem_euclid(n) as usize;
        self.reveal_current(cx);
    }

    /// Scroll the current hit's round into view and unfold whatever hides
    /// the item: a tool card, a thought, a folded run of tool calls.
    fn reveal_current(&mut self, cx: &mut Context<Self>) {
        let Some(h) = self.find_hits.get(self.find_at).copied() else { return };
        if let Some(d) = self.detail.as_mut() {
            if let Some(jx) = h.item {
                if let Some(rnd) = d.session.rounds.get(h.round) {
                    match rnd.items.get(jx) {
                        Some(emaki_core::model::Item::Tool(_)) => {
                            d.open_tools.insert((h.round, jx));
                        }
                        Some(emaki_core::model::Item::Thinking { .. }) => {
                            d.open_thoughts.insert((h.round, jx));
                        }
                        _ => {}
                    }
                    if let Some(start) = crate::transcript::run_start(rnd, jx) {
                        d.open_runs.insert((h.round, start));
                    }
                }
            }
            d.list.scroll_to(ListOffset { item_ix: h.round, offset_in_item: px(0.) });
        }
        cx.notify();
    }

    /// How the transcript should draw a prompt (`item` is `None`) or an
    /// item: 0 plain, 1 a hit, 2 the hit the find bar is on.
    pub(crate) fn find_mark(&self, round: usize, item: Option<usize>) -> u8 {
        if !self.find_open || self.find_hits.is_empty() {
            return 0;
        }
        let here = Hit { round, item };
        if self.find_hits.get(self.find_at) == Some(&here) {
            return 2;
        }
        if self.find_hits.iter().any(|h| *h == here) { 1 } else { 0 }
    }

    /// The find bar: the field, "n of m", up, down, close. It sits between
    /// the title strip and the conversation, in the reading column.
    fn render_find_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let focus = self.find_input.read(cx).focus_handle(cx);
        let value = self.find_input.read(cx).value().to_string();
        let n = self.find_hits.len();
        let empty = value.trim().is_empty();
        let count = if empty {
            String::new()
        } else if n == 0 {
            "no matches".into()
        } else {
            format!("{} of {}", self.find_at + 1, n)
        };
        h_flex().w_full().justify_center().px(px(24.)).pb(px(8.)).child(
            h_flex()
                .id("find-bar")
                .key_context(FIND_CONTEXT)
                .w_full()
                .max_w(CONTENT_W)
                .h(px(36.))
                .px(px(10.))
                .gap(px(8.))
                .items_center()
                .rounded(px(10.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .shadow_sm()
                .child(Icon::new(IconName::Search).with_size(px(14.)).text_color(theme.muted_foreground))
                .child(
                    div()
                        .id("find-field")
                        .track_focus(&focus)
                        .role(Role::TextInput)
                        .aria_label("Find in conversation")
                        .aria_value(value)
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .child(Input::new(&self.find_input).appearance(false).bordered(false)),
                )
                .child(div().text_size(px(11.5)).flex_shrink_0().text_color(if n == 0 && !empty { theme.danger } else { theme.muted_foreground }).child(count))
                .child(icon_button("find-prev", IconName::ChevronUp, "Previous (⇧↩, ⌘⇧G)", cx, |this, _, cx| this.find_step(-1, cx)))
                .child(icon_button("find-next", IconName::ChevronDown, "Next (↩, ⌘G)", cx, |this, _, cx| this.find_step(1, cx)))
                .child(icon_button("find-close", IconName::Close, "Close (esc)", cx, |this, window, cx| this.close_find(window, cx))),
        )
    }

    // -- search ------------------------------------------------------------

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = true;
        self.search_input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn run_search(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let q = self.search_input.read(cx).value().to_string();
        if q.trim().is_empty() {
            self.search_results = None;
            cx.notify();
            return;
        }
        let hub = Arc::clone(&self.hub);
        let task = cx.background_spawn(async move { hub.search(&q) });
        self.search_task = Some(cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                this.search_results = Some(res);
                cx.notify();
            })
            .ok();
        }));
    }

    // -- composer ------------------------------------------------------------

    /// How a message typed here would reach the session, and why not.
    pub fn reply_via(&self) -> (&'static str, String) {
        if self.page == Page::New {
            return if self.new_cwd.is_empty() { ("", "pick a folder first".into()) } else { ("spawn", String::new()) };
        }
        let Some(r) = self.selected_ref() else { return ("", "no session selected".into()) };
        self.reply_via_for(r)
    }

    /// Whether the session showing can be continued in a terminal, and in a
    /// few words why not. The rule is one writer per transcript: a
    /// terminal session with an inbox already has one, and a driver of ours
    /// mid-reply is one too (an idle driver is stopped on the way out).
    fn terminal_check(&self, r: &SessionRef) -> Result<(), &'static str> {
        if r.archived {
            return Err("Kept only: the agent no longer has this transcript");
        }
        if r.cwd.is_empty() || !std::path::Path::new(&r.cwd).is_dir() {
            return Err("The session's folder is gone");
        }
        if let Some(v) = self.drivers.get(&r.session_id) {
            if v.starting || v.state == "running" {
                return Err("Wait for the running reply, then open");
            }
        }
        if self.hub.peer_for(&r.session_id).is_some() {
            return Err("Already open in a terminal");
        }
        Ok(())
    }

    /// The button at the top left: continue the session showing in the
    /// person's own terminal, with the agent's resume command. See
    /// `emaki_core::terminal` and `sys::open_in_terminal`.
    pub fn open_in_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.selected_ref().cloned() else { return };
        if let Err(why) = self.terminal_check(&r) {
            self.notice = Some(Notice::error(why));
            cx.notify();
            return;
        }
        if self.drivers.remove(&r.session_id).is_some() {
            self.hub.stop_driver(&r.session_id);
        }
        let argv = emaki_core::terminal::resume_argv(r.agent, &r.session_id);
        self.notice = Some(match crate::sys::open_in_terminal(&r.session_id, &r.cwd, &argv) {
            Ok(()) => Notice::said("opened in your terminal"),
            Err(e) => Notice::error(format!("could not open a terminal: {e}")),
        });
        cx.notify();
    }

    pub fn reply_via_for(&self, r: &SessionRef) -> (&'static str, String) {
        if r.agent != AgentId::ClaudeCode {
            return ("", format!("{} sessions are read-only here", r.agent.display_name()));
        }
        if let Some(v) = self.drivers.get(&r.session_id) {
            if v.starting {
                return ("driver", "starting claude…".into());
            }
            if v.state != "exited" {
                return ("driver", String::new());
            }
        }
        // A terminal session with an inbox takes the message directly; the
        // driver is checked first because its child registers an inbox too.
        if self.hub.peer_for(&r.session_id).is_some() {
            return ("inbox", String::new());
        }
        if r.archived {
            return ("", "kept only: Claude Code no longer has this transcript, so it cannot be resumed".into());
        }
        if !self.cfg.driver.enabled {
            return ("", "the driver is off in config".into());
        }
        if r.cwd.is_empty() || !std::path::Path::new(&r.cwd).is_dir() {
            return ("", "the session's folder is gone".into());
        }
        // A fresh file with neither a driver nor an inbox: an interactive
        // Claude Code always registers an inbox, so this is a session that
        // just ended, or a headless one we cannot reach. Sending resumes it.
        ("spawn", String::new())
    }

    pub fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.composer.read(cx).value().to_string();
        if typed.trim().is_empty() && self.attachments.is_empty() {
            return;
        }
        let (via, why) = self.reply_via();
        let (text, images) = self.fold_attachments(&typed, via == "driver" || via == "spawn");
        match via {
            "inbox" => {
                let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
                self.hub.send_to_inbox(&sid, text);
            }
            "driver" => {
                let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
                if !self.hub.send_to_driver(&sid, text.clone(), images) {
                    self.notice = Some(Notice::error("the driver is gone; try again"));
                    cx.notify();
                    return;
                }
                if let Some(v) = self.drivers.get_mut(&sid) {
                    v.state = "running".into();
                }
            }
            "spawn" => {
                let (sid, cwd, resume) = if self.page == Page::New {
                    if self.new_id.is_empty() {
                        self.new_id = uuid::Uuid::new_v4().to_string();
                    }
                    (self.new_id.clone(), self.new_cwd.clone(), false)
                } else {
                    let r = self.selected_ref().unwrap();
                    (r.session_id.clone(), r.cwd.clone(), true)
                };
                self.drivers.insert(sid.clone(), DriverView { starting: true, state: "starting".into(), mode: self.next_mode.clone(), model: self.next_model.clone(), ..Default::default() });
                self.hub.spawn_driver_and_send(sid.clone(), cwd, resume, self.next_mode.clone(), self.next_model.clone(), Some((text.clone(), images)));
                if self.page == Page::New {
                    let key = format!("claude-code:{sid}");
                    self.pending_select = Some(key.clone());
                    self.selected = Some(key);
                    self.new_id = String::new();
                    self.page = Page::Session;
                }
                self.notice = Some(Notice::said("starting claude…"));
            }
            _ => {
                self.notice = Some(Notice::error(why));
                cx.notify();
                return;
            }
        }
        self.composer.update(cx, |s, cx| s.set_value("", window, cx));
        self.attachments.clear();
        cx.notify();
    }

    /// What Escape closes, nearest first: the lightbox, then the search.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            self.settings_open = false;
            cx.notify();
        } else if self.lightbox.is_some() {
            self.lightbox = None;
            cx.notify();
        } else if self.search_open {
            self.close_search(window, cx);
        } else if self.find_open {
            self.close_find(window, cx);
        }
    }

    fn open_lightbox(&mut self, lb: Lightbox, window: &mut Window, cx: &mut Context<Self>) {
        self.lightbox = Some(lb);
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// What a click on an attachment does: a picture opens large here, any
    /// other file opens in the app the system keeps for it.
    pub fn preview_attachment(&mut self, title: String, path: Option<PathBuf>, image: Option<ImageSource>, window: &mut Window, cx: &mut Context<Self>) {
        match image {
            Some(source) => self.open_lightbox(Lightbox { title, source, path }, window, cx),
            None => {
                if let Some(p) = path {
                    crate::sys::open_path(&p);
                }
            }
        }
    }

    // -- attachments ----------------------------------------------------------

    /// Where a pasted image lands: under the session's upload folder, or the
    /// folder of the session about to be started.
    fn upload_dir(&mut self) -> PathBuf {
        let sid = if self.page == Page::New {
            if self.new_id.is_empty() {
                self.new_id = uuid::Uuid::new_v4().to_string();
            }
            self.new_id.clone()
        } else {
            self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_else(|| "new".into())
        };
        emaki_core::paths::uploads_dir(&sid)
    }

    pub fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for p in paths {
            if !p.is_file() || self.attachments.iter().any(|a| &a.path == p) {
                continue;
            }
            let mime = mime_of(p);
            let image = IMAGE_TYPES.contains(&mime);
            let size = if image { file_image_dims(p) } else { None };
            self.attachments.push(Attachment { path: p.clone(), name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), mime: mime.into(), image, size });
        }
        cx.notify();
    }

    /// Keep a pasted image as a file first, like an upload; the message then
    /// carries it as a block or a path depending on the channel.
    pub fn attach_image_bytes(&mut self, mime: &str, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let dir = self.upload_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            self.notice = Some(Notice::error("could not create the uploads folder"));
            cx.notify();
            return;
        }
        let ext = match mime {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            "image/tiff" => "tiff",
            "image/bmp" => "bmp",
            _ => "img",
        };
        let name = format!("{}-pasted.{ext}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let path = dir.join(&name);
        if let Err(e) = emaki_core::paths::write_atomic(&path, &bytes) {
            self.notice = Some(Notice::error(format!("could not keep the pasted image: {e}")));
            cx.notify();
            return;
        }
        self.attachments.push(Attachment { path, name, mime: mime.into(), image: IMAGE_TYPES.contains(&mime), size: image_dims(&bytes) });
        cx.notify();
    }

    /// Images and files on the clipboard become attachments; text is left
    /// for the textarea. Returns whether anything was taken.
    fn paste_attachments(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else { return false };
        let mut took = false;
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(img) => {
                    self.attach_image_bytes(img.format.mime_type(), img.bytes.clone(), cx);
                    took = true;
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    self.attach_paths(paths.paths(), cx);
                    took = true;
                }
                ClipboardEntry::String(_) => {}
            }
        }
        took
    }

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Attach".into()) });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |this, cx| this.attach_paths(&paths, cx)).ok();
            }
        })
        .detach();
    }

    fn fold_attachments(&self, text: &str, as_blocks: bool) -> (String, Vec<serde_json::Value>) {
        let mut images = Vec::new();
        let mut lines = Vec::new();
        for a in &self.attachments {
            let block = if as_blocks && a.image { image_block(&a.path, &a.mime) } else { None };
            match block {
                Some(b) => images.push(b),
                None => lines.push(format!("Attached file: {}", a.path.display())),
            }
        }
        let mut out = text.trim_end().to_string();
        if !lines.is_empty() {
            if !out.trim().is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&lines.join("\n"));
        }
        (out, images)
    }

    /// The thumbnail of the `index`-th image block of row `uuid`, if it has
    /// been read yet; the first call starts the read and redraws round `ix`
    /// when it lands.
    pub fn thumb(&mut self, ix: usize, uuid: &str, index: usize, cx: &mut Context<Self>) -> Option<Thumb> {
        let key = format!("{uuid}:{index}");
        let d = self.detail.as_mut()?;
        if let Some(t) = d.thumbs.get(&key) {
            return t.clone();
        }
        d.thumbs.insert(key.clone(), None);
        let path = d.path.clone();
        let dkey = d.key.clone();
        let uuid = uuid.to_string();
        let task = cx.background_spawn(async move { emaki_core::transcript::image_block_bytes(&path, &uuid, index) });
        cx.spawn(async move |this, cx| {
            let loaded = task.await.and_then(|(mime, bytes)| {
                let size = image_dims(&bytes);
                Some(Thumb { image: Arc::new(gpui::Image::from_bytes(image_format(&mime)?, bytes)), size })
            });
            this.update(cx, |this, cx| {
                if let Some(d) = this.detail.as_mut() {
                    if d.key == dkey {
                        match loaded {
                            Some(img) => {
                                d.thumbs.insert(key, Some(img));
                            }
                            None => {
                                d.thumbs.remove(&key);
                            }
                        }
                        let _ = ix;
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
        None
    }

    /// The pixel size of a kept picture file, read from its header the first
    /// time and remembered for as long as the session is open.
    pub fn kept_image_size(&mut self, path: &std::path::Path) -> Option<(u32, u32)> {
        let d = self.detail.as_mut()?;
        let key = path.to_string_lossy().to_string();
        if let Some(s) = d.sizes.get(&key) {
            return *s;
        }
        let s = file_image_dims(path);
        d.sizes.insert(key, s);
        s
    }

    /// The modes the picker offers: every one Claude Code has, bypass only
    /// when config allows it.
    fn modes(&self) -> Vec<&'static str> {
        let allow_bypass = self.cfg.driver.allow_bypass;
        MODES.iter().copied().filter(|m| allow_bypass || *m != "bypassPermissions").collect()
    }

    /// The session showing, once loaded.
    fn shown_session(&self) -> Option<&Session> {
        let d = self.detail.as_ref()?;
        (self.page == Page::Session && self.selected.as_deref() == Some(d.key.as_str())).then_some(&*d.session)
    }

    /// The mode the composer's pill shows: the driver's own when one is
    /// behind the session, else the transcript's for a terminal session,
    /// else what the next session will start in.
    fn current_mode(&self) -> String {
        let r = self.selected_ref();
        let sid = r.map(|r| r.session_id.clone()).unwrap_or_default();
        let m = self
            .drivers
            .get(&sid)
            .map(|v| v.mode.clone())
            .filter(|m| !m.is_empty())
            .or_else(|| r.filter(|_| self.page == Page::Session).map(|r| r.state.mode.clone()).filter(|m| !m.is_empty()))
            .unwrap_or_else(|| self.next_mode.clone());
        if m.is_empty() { "default".into() } else { m }
    }

    /// The model, as the driver reports it (a full id after its first
    /// turn), else the transcript's last, else what was asked for.
    fn current_model(&self) -> String {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let m = self
            .drivers
            .get(&sid)
            .map(|v| v.model.clone())
            .filter(|m| !m.is_empty())
            .or_else(|| self.shown_session().and_then(|s| s.models.last().cloned()))
            .unwrap_or_else(|| self.next_model.clone());
        if m.is_empty() { "default".into() } else { m }
    }

    /// The effort level the transcript last recorded; empty when none.
    fn current_effort(&self) -> String {
        self.shown_session().map(|s| s.effort.clone()).unwrap_or_default()
    }

    fn set_effort(&mut self, effort: &str, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        if self.hub.set_driver_effort(&sid, effort.to_string()) {
            self.notice = Some(Notice::said(format!("setting {}…", effort_label(effort).to_lowercase())));
        } else {
            self.notice = Some(Notice::error("no driver behind this session"));
        }
        cx.notify();
    }

    /// `Context 37% (386k of 1M) · 5h 3% (4h26m) · 7d 6% (6d7h)`, as the
    /// terminal's status line has it, above the composer. The context is
    /// the transcript's; the windows are what the last driver turn saw,
    /// which a terminal turn does not refresh, so the row says how old
    /// they are.
    /// The left of the row under the composer, on a Claude conversation.
    fn render_limits(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let theme = cx.theme().clone();
        let s = self.shown_session()?;
        if s.agent != AgentId::ClaudeCode {
            return None;
        }
        let now = self.now;
        let muted = theme.muted_foreground;
        // The terminal's line, word for word: `Context 32% | 5h: 5% (4h45m)
        // | 7d: 11% (6d2h)`, the same colours at the same thresholds as
        // scripts/statusline.sh. No tooltip: the line is the whole story.
        let part = |label: &'static str, pct: u64, color: Hsla, tail: String| {
            h_flex()
                .gap(px(4.))
                .items_center()
                .child(div().text_color(muted).child(label))
                .child(div().font_weight(FontWeight::MEDIUM).text_color(color).child(format!("{pct}%")))
                .when(!tail.is_empty(), |d| d.child(div().text_color(muted).child(tail)))
        };
        let bar = || div().text_color(muted.opacity(0.6)).child("|");
        let mut row = h_flex().flex_shrink_0().gap(px(8.)).items_center();
        let model = s.models.last().map(String::as_str).unwrap_or("");
        let window = self.limits.context_window(model);
        let ctx_pct = if s.context_tokens > 0 { (s.context_tokens * 100 / window.max(1)).min(999) } else { 0 };
        let ctx_color = if ctx_pct >= 85 { theme.danger } else if ctx_pct >= 70 { theme.warning } else { theme.success };
        row = row.child(part("Context", ctx_pct, ctx_color, String::new()));
        let usage_color = |pct: u64| if pct >= 90 { theme.danger } else if pct >= 70 { theme.magenta } else { theme.blue };
        for (label, w) in [("5h:", self.limits.five_hour), ("7d:", self.limits.seven_day)] {
            row = row.child(bar());
            match w {
                Some(w) => {
                    let pct = (w.utilization * 100.0).round().max(0.0) as u64;
                    let left = emaki_core::limits::until(w.resets_at, now);
                    row = row.child(part(label, pct, usage_color(pct), if left.is_empty() { String::new() } else { format!("({left})") }));
                }
                None => {
                    row = row.child(h_flex().gap(px(4.)).text_color(muted).child(label).child("--"));
                }
            }
        }
        Some(row)
    }

    /// Switch the permission mode: for the driver behind this session, and
    /// for the next session started here. The hub answers with the mode
    /// Claude Code actually holds, so a refused switch shows on the status
    /// row and the pill goes back.
    fn set_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        self.next_mode = mode.to_string();
        if self.hub.set_driver_mode(&sid, mode.to_string()) {
            if let Some(v) = self.drivers.get_mut(&sid) {
                v.mode = mode.to_string();
            }
        }
        cx.notify();
    }

    /// ⇧Tab in the composer, as in Claude Code's own terminal.
    fn cycle_mode(&mut self, cx: &mut Context<Self>) {
        let modes = self.modes();
        let current = self.current_mode();
        let i = modes.iter().position(|m| *m == current).map(|i| (i + 1) % modes.len()).unwrap_or(0);
        self.set_mode(modes[i], cx);
    }

    fn set_model(&mut self, model: &str, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        self.next_model = model.to_string();
        if self.hub.set_driver_model(&sid, model.to_string()) {
            if let Some(v) = self.drivers.get_mut(&sid) {
                v.model = model.to_string();
            }
        }
        cx.notify();
    }

    fn interrupt(&mut self, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        if let Some(d) = self.hub.driver_for(&sid) {
            std::thread::spawn(move || {
                let _ = d.interrupt();
            });
            self.notice = Some(Notice::said("interrupting…"));
            cx.notify();
        }
    }

    fn answer_permission(&mut self, request_id: String, allow: bool, cx: &mut Context<Self>) {
        let Some((sid, _)) = self.permissions.iter().find(|(_, r)| r.request_id == request_id).cloned() else { return };
        if let Some(d) = self.hub.driver_for(&sid) {
            let rid = request_id.clone();
            std::thread::spawn(move || d.answer_permission(&rid, allow, ""));
        }
        self.permissions.retain(|(_, r)| r.request_id != request_id);
        cx.notify();
    }

    /// ↩ on an empty composer allows the oldest card waiting on this
    /// session, ⇧↩ denies it. Says whether a card was answered.
    fn answer_pending_by_key(&mut self, allow: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.composer.read(cx).value().trim().is_empty() {
            return false;
        }
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let Some(id) = self.permissions.iter().find(|(s, _)| *s == sid).map(|(_, p)| p.request_id.clone()) else { return false };
        // The textarea put a new line in before saying ↩ was pressed.
        self.composer.update(cx, |s, cx| s.set_value("", window, cx));
        self.answer_permission(id, allow, cx);
        true
    }

    /// Every card waiting on this session, allowed at once.
    fn allow_all_pending(&mut self, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let ids: Vec<String> = self.permissions.iter().filter(|(s, _)| *s == sid).map(|(_, p)| p.request_id.clone()).collect();
        for id in ids {
            self.answer_permission(id, true, cx);
        }
    }

    fn reply_from_board(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_session(key, cx);
        self.focus_composer(window, cx);
    }

    // -- derived ------------------------------------------------------------

    /// The transcript decides the phase; what the window adds is whether a
    /// process is behind the session, whether an approval is held here, and
    /// what our own driver is doing. Those refine the answer, never replace it.
    pub fn card_for(&self, r: &SessionRef) -> Card {
        let alive = self.hub.is_live(r, self.now);
        let pending = self.permissions.iter().filter(|(s, _)| s == &r.session_id).map(|(_, p)| p.clone()).last();
        let drv = self.drivers.get(&r.session_id);
        let st = &r.state;
        let mut card = Card { r: r.clone(), column: Column::Done, chip: String::new(), chip_kind: "", text: String::new(), pending: None, queued: drv.map(|v| v.queued).unwrap_or(0) };
        if let Some(p) = pending {
            card.column = Column::NeedsYou;
            card.chip = p.tool_name.clone();
            card.chip_kind = "approve";
            card.text = emaki_core::build::tool_subject(&p.tool_name, &p.input, &r.cwd);
            card.pending = Some(p);
            return card;
        }
        if !alive {
            return card;
        }
        let mut phase = st.phase;
        let mut activity = st.activity.clone();
        let mut tool = st.tool.clone();
        if let Some(v) = drv {
            if (v.state == "running" || v.starting) && matches!(phase, Phase::YourTurn | Phase::Idle) {
                phase = Phase::Working;
                activity = "working on your message".into();
                tool = String::new();
            }
        }
        let mode = drv.map(|v| v.mode.clone()).filter(|m| !m.is_empty()).unwrap_or_else(|| st.mode.clone());
        match phase {
            Phase::NeedsYou => {
                card.column = Column::NeedsYou;
                card.chip_kind = if st.activity_kind == "plan" { "plan" } else { "ask" };
                card.chip = card.chip_kind.into();
                card.text = activity;
            }
            Phase::Working => {
                card.column = if mode == "plan" { Column::Planning } else { Column::Working };
                card.chip_kind = kind_static(&st.activity_kind);
                card.chip = if !tool.is_empty() { tool } else if mode == "plan" { "plan".into() } else { "…".into() };
                card.text = activity;
            }
            Phase::YourTurn => {
                card.column = Column::YourTurn;
                let stopped = st.activity_kind == "stop";
                card.chip_kind = if stopped { "stop" } else { "reply" };
                card.chip = if stopped { "interrupted".into() } else if !st.reply.is_empty() { "replied".into() } else { "idle".into() };
                card.text = if st.reply.is_empty() { activity } else { st.reply.clone() };
            }
            Phase::Idle => {
                card.column = Column::YourTurn;
                card.chip_kind = "reply";
                card.chip = "idle".into();
                card.text = "waiting for the first prompt".into();
            }
        }
        card
    }

    fn scoped_refs(&self) -> Vec<&SessionRef> {
        self.refs
            .iter()
            .filter(|r| match &self.scope {
                Scope::All => true,
                Scope::Agent(a) => r.agent == *a,
                Scope::Project(p) => r.project() == *p,
                Scope::Kept => r.archived,
            })
            .collect()
    }

    fn projects(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for r in &self.refs {
            *counts.entry(r.project()).or_default() += 1;
        }
        let mut v: Vec<(String, usize)> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    fn recent_cwds(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let own = emaki_core::paths::root().to_string_lossy().to_string();
        let legacy = emaki_core::paths::legacy_root().to_string_lossy().to_string();
        for r in &self.refs {
            if r.cwd.is_empty() || !std::path::Path::new(&r.cwd).is_dir() || r.cwd.starts_with(&own) || r.cwd.starts_with(&legacy) {
                continue;
            }
            if seen.insert(r.cwd.clone()) {
                out.push(r.cwd.clone());
            }
            if out.len() >= 12 {
                break;
            }
        }
        out
    }

    /// Whether an agent is busy on this session right now.
    pub fn is_working(&self, r: &SessionRef) -> bool {
        matches!(self.card_for(r).column, Column::Working | Column::Planning)
    }

    /// The colour a live session's dot takes, or none when nothing is behind it.
    fn live_color(&self, r: &SessionRef, cx: &App) -> Option<Hsla> {
        if !self.hub.is_live(r, self.now) {
            return None;
        }
        let col = self.card_for(r).column;
        (col != Column::Done).then(|| self.column_color(col, cx))
    }

    // -- sidebar --------------------------------------------------------------

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let projects = self.projects();
        let page = self.page;
        let scope = self.scope.clone();

        let header = h_flex()
            .h(TITLEBAR_H)
            .flex_shrink_0()
            .pl(if cfg!(target_os = "macos") { TRAFFIC_W } else { px(16.) })
            .pr(px(10.))
            .items_center()
            .gap(px(8.))
            .child(mark_icon(px(16.), theme.primary))
            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).font_family(SERIF).child("Emaki"))
            .child(div().flex_1())
            .child(icon_button("sidebar-close", IconName::PanelLeftClose, "Hide sidebar (⌘⇧S)", cx, |this, _, cx| {
                this.hide_sidebar();
                cx.notify();
            }));

        let new_row = h_flex()
            .id("nav-new")
            .h(px(36.))
            .px(px(10.))
            .gap(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(page == Page::New, |d| d.bg(theme.sidebar_accent))
            .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
            .on_click(cx.listener(|this, _, window, cx| this.show_new(None, window, cx)))
            .child(div().size(px(22.)).rounded_full().bg(theme.primary).flex().items_center().justify_center().child(Icon::new(IconName::Plus).with_size(px(13.)).text_color(theme.primary_foreground)))
            .child(div().flex_1().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child("New session"))
            .child(kbd_hint("⌘N", &theme));

        let nav = |id: &'static str, icon: IconName, label: &'static str, hint: &'static str, active: bool, cx: &mut Context<Self>, on: Box<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>| {
            let theme = cx.theme().clone();
            h_flex()
                .id(id)
                .h(px(32.))
                .px(px(10.))
                .gap(px(10.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(active, |d| d.bg(theme.sidebar_accent))
                .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                .on_click(cx.listener(move |this, _, window, cx| on(this, window, cx)))
                .child(div().w(px(22.)).flex().justify_center().child(Icon::new(icon).with_size(px(16.)).text_color(if active { theme.foreground } else { theme.muted_foreground })))
                .child(div().flex_1().min_w_0().truncate().text_size(px(13.5)).child(label))
                .when(!hint.is_empty(), |d| d.child(kbd_hint(hint, &theme)))
        };

        let sessions_active = page == Page::Sessions;
        let top = v_flex()
            .px(px(10.))
            .pt(px(2.))
            .gap(px(2.))
            .child(new_row)
            .child(nav("nav-board", IconName::LayoutDashboard, "Board", "⌘B", page == Page::Board, cx, Box::new(|this, _, cx| {
                this.page = Page::Board;
                cx.notify();
            })))
            .child(nav("nav-sessions", IconName::Inbox, "Sessions", "⌘L", sessions_active && scope == Scope::All, cx, Box::new(|this, _, cx| this.show_sessions(Scope::All, cx))))
            .child(nav("nav-search", IconName::Search, "Search", "⌘K", false, cx, Box::new(|this, window, cx| this.open_search(window, cx))));

        let mut agents: Vec<(AgentId, usize)> = Vec::new();
        for a in AgentId::ALL {
            let n = self.refs.iter().filter(|r| r.agent == a).count();
            if n > 0 {
                agents.push((a, n));
            }
        }
        let shown_projects = projects.iter().take(8).cloned().collect::<Vec<_>>();
        let more_projects = projects.len().saturating_sub(shown_projects.len());
        let mut scroll = v_flex().id("side-scroll").flex_1().min_h_0().overflow_y_scroll().px(px(10.)).pb(px(8.));
        scroll = scroll.child(self.group_label("Agents", cx));
        scroll = scroll.children(agents.into_iter().map(|(a, n)| {
            let active = sessions_active && scope == Scope::Agent(a);
            let theme = cx.theme().clone();
            let live = self.refs.iter().filter(|r| r.agent == a && self.live_color(r, cx).is_some()).count();
            h_flex()
                .id(SharedString::from(format!("agent-{}", a.as_str())))
                .h(px(30.))
                .px(px(10.))
                .gap(px(10.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(active, |d| d.bg(theme.sidebar_accent))
                .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                .on_click(cx.listener(move |this, _, _, cx| this.show_sessions(Scope::Agent(a), cx)))
                .child(div().w(px(22.)).flex().justify_center().child(agent_icon(a, px(15.), agent_color(a, &theme))))
                .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(a.display_name()))
                .when(live > 0, |d| d.child(div().size(px(7.)).rounded_full().bg(theme.green).flex_shrink_0()))
                .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(n.to_string()))
        }));
        let kept = self.refs.iter().filter(|r| r.archived).count();
        if kept > 0 {
            let active = sessions_active && scope == Scope::Kept;
            scroll = scroll.child(
                h_flex()
                    .id("agent-kept")
                    .h(px(30.))
                    .px(px(10.))
                    .gap(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.sidebar_accent))
                    .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                    .on_click(cx.listener(|this, _, _, cx| this.show_sessions(Scope::Kept, cx)))
                    .child(div().w(px(22.)).flex().justify_center().child(Icon::new(IconName::HardDrive).with_size(px(15.)).text_color(theme.muted_foreground)))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child("Kept only"))
                    .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(kept.to_string())),
            );
        }
        if !shown_projects.is_empty() {
            scroll = scroll.child(self.group_label("Projects", cx));
            scroll = scroll.children(shown_projects.into_iter().map(|(p, _n)| {
                let active = sessions_active && scope == Scope::Project(p.clone());
                let theme = cx.theme().clone();
                let label = p.clone();
                h_flex()
                    .id(SharedString::from(format!("proj-{p}")))
                    .h(px(30.))
                    .px(px(10.))
                    .gap(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.sidebar_accent))
                    .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                    .on_click(cx.listener(move |this, _, _, cx| this.show_sessions(Scope::Project(p.clone()), cx)))
                    .child(div().w(px(22.)).flex().justify_center().child(Icon::new(IconName::Folder).with_size(px(15.)).text_color(theme.muted_foreground)))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(label))
            }));
            if more_projects > 0 {
                scroll = scroll.child(
                    h_flex()
                        .id("proj-more")
                        .h(px(28.))
                        .px(px(10.))
                        .pl(px(42.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .text_size(px(12.5))
                        .text_color(theme.muted_foreground)
                        .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                        .on_click(cx.listener(|this, _, _, cx| this.show_sessions(Scope::All, cx)))
                        .child(format!("{more_projects} more")),
                );
            }
        }
        scroll = scroll.child(self.group_label("Recents", cx));
        let recents: Vec<SessionRef> = self.refs.iter().take(40).cloned().collect();
        scroll = scroll.children(recents.into_iter().map(|r| {
            let key = key_of(&r);
            let active = page == Page::Session && self.selected.as_deref() == Some(key.as_str());
            let theme = cx.theme().clone();
            let dot = self.live_color(&r, cx);
            let working = self.is_working(&r);
            let glyph_id = SharedString::from(format!("recent-glyph-{key}"));
            h_flex()
                .id(SharedString::from(format!("recent-{key}")))
                .h(px(30.))
                .px(px(10.))
                .gap(px(10.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(active, |d| d.bg(theme.sidebar_accent))
                .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                .on_click(cx.listener(move |this, _, window, cx| this.open_and_focus(&key, window, cx)))
                .child(div().w(px(22.)).flex().justify_center().child(agent_glyph(r.agent, px(14.), agent_color(r.agent, &theme), working, glyph_id)))
                .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(r.title.clone()))
                .when(r.agent != AgentId::ClaudeCode, |d| d.child(badge(r.agent.display_name(), theme.muted, theme.muted_foreground)))
                .when_some(dot, |d, c| d.child(div().size(px(7.)).rounded_full().bg(c).flex_shrink_0()))
                .when(r.archived && dot.is_none(), |d| d.child(Icon::new(IconName::HardDrive).with_size(px(12.)).text_color(theme.muted_foreground)))
        }));

        let initial = self.user_name.chars().next().map(|c| c.to_string()).unwrap_or_else(|| "S".into());
        let footer = h_flex()
            .flex_shrink_0()
            .px(px(14.))
            .py(px(10.))
            .gap(px(10.))
            .items_center()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(div().size(px(28.)).rounded_full().bg(theme.primary).flex().items_center().justify_center().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.primary_foreground).child(initial))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(if self.user_name.is_empty() { "Emaki".to_string() } else { self.user_name.clone() })),
            );

        v_flex().w(SIDEBAR_W).h_full().flex_shrink_0().bg(theme.sidebar).text_color(theme.sidebar_foreground).border_r_1().border_color(theme.sidebar_border).child(header).child(top).child(scroll).child(footer)
    }

    fn group_label(&self, text: &'static str, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div().pt(px(16.)).pb(px(4.)).px(px(10.)).text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(text)
    }

    /// The strip along the top of the content pane: room for the traffic
    /// lights when the sidebar is hidden, a title in the middle, actions on
    /// the right.
    fn render_topbar(&self, title: String, right: Vec<AnyElement>, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let centre = div().flex_1().min_w_0().text_center().truncate().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(title).into_any_element();
        self.render_topbar_with(centre, Vec::new(), right, cx)
    }

    /// One tab per open session in the top strip: the agent's mark (turning
    /// while it works), the title, and a close button. Clicking a tab shows
    /// that session; ⌘W closes the one showing.
    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut row = h_flex().flex_1().min_w_0().justify_center().items_center().gap(px(4.)).overflow_hidden();
        for (ix, key) in self.tabs.clone().into_iter().enumerate() {
            let r = self.refs.iter().find(|r| key_of(r) == key).cloned();
            let title = r.as_ref().map(|r| r.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "untitled".into());
            let agent = r.as_ref().map(|r| r.agent).unwrap_or(AgentId::ClaudeCode);
            let working = r.as_ref().map(|r| self.is_working(r)).unwrap_or(false);
            let active = self.selected.as_deref() == Some(key.as_str());
            let open_key = key.clone();
            let close_key = key.clone();
            let hover_bg = theme.muted.opacity(0.6);
            let close_bg = theme.border;
            row = row.child(
                h_flex()
                    .id(("tab", ix))
                    .h(px(30.))
                    .pl(px(10.))
                    .pr(px(6.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .min_w_0()
                    .max_w(px(220.))
                    .flex_shrink(1.)
                    .when(active, |d| d.bg(theme.muted))
                    .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_session(&open_key, cx)))
                    .child(agent_glyph(agent, px(14.), agent_color(agent, &theme), working, format!("tab-glyph-{ix}")))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.5))
                            .font_weight(if active { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                            .text_color(if active { theme.foreground } else { theme.muted_foreground })
                            .child(title),
                    )
                    .child(
                        div()
                            .id(("tab-close", ix))
                            .size(px(18.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(move |s| s.bg(close_bg))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.close_tab(&close_key, window, cx);
                            }))
                            .child(Icon::new(IconName::Close).xsmall().text_color(theme.muted_foreground)),
                    ),
            );
        }
        row.into_any_element()
    }

    /// `left` sits after the sidebar button (and the traffic lights when
    /// the sidebar is hidden); `right` is the page's actions. Both ends
    /// are at least 120px so the centre stays centred when they are short.
    fn render_topbar_with(&self, centre: AnyElement, left: Vec<AnyElement>, right: Vec<AnyElement>, cx: &mut Context<Self>) -> impl IntoElement {
        let mac = cfg!(target_os = "macos");
        let mut left_end = h_flex().min_w(px(120.)).flex_shrink_0().items_center().gap(px(4.));
        if !self.sidebar_open || self.narrow {
            left_end = left_end
                .when(mac, |d| d.pl(TRAFFIC_W - px(12.)))
                .child(icon_button("sidebar-open", IconName::PanelLeftOpen, "Show sidebar (⌘⇧S)", cx, |this, _, cx| {
                    this.show_sidebar();
                    cx.notify();
                }));
        }
        h_flex()
            .h(TITLEBAR_H)
            .flex_shrink_0()
            .px(px(12.))
            .items_center()
            .child(left_end.children(left))
            .child(centre)
            .child(h_flex().min_w(px(120.)).flex_shrink_0().justify_end().items_center().gap(px(6.)).children(right))
    }

    // -- the sessions page ----------------------------------------------------

    fn render_sessions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let refs: Vec<SessionRef> = self.scoped_refs().into_iter().cloned().collect();
        let now = self.now;
        let scope = self.scope.clone();
        let kept = self.refs.iter().filter(|r| r.archived).count();
        let claude = self.refs.iter().filter(|r| r.agent == AgentId::ClaudeCode).count();
        let codex = self.refs.iter().filter(|r| r.agent == AgentId::Codex).count();

        let filter = |id: &'static str, label: String, active: bool, cx: &mut Context<Self>, on: Box<dyn Fn(&mut Self, &mut Context<Self>)>| {
            let theme = cx.theme().clone();
            div()
                .id(id)
                .h(px(28.))
                .px(px(12.))
                .rounded_full()
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(12.5))
                .border_1()
                .border_color(if active { theme.foreground } else { theme.border })
                .when(active, |d| d.bg(theme.foreground).text_color(theme.background))
                .when(!active, |d| d.hover(|s| s.bg(theme.muted)))
                .on_click(cx.listener(move |this, _, _, cx| on(this, cx)))
                .child(label)
        };
        let mut filters = h_flex()
            .gap(px(6.))
            .flex_wrap()
            .child(filter("f-all", format!("All · {}", self.refs.len()), scope == Scope::All, cx, Box::new(|this, cx| this.show_sessions(Scope::All, cx))))
            .child(filter("f-claude", format!("Claude Code · {claude}"), scope == Scope::Agent(AgentId::ClaudeCode), cx, Box::new(|this, cx| this.show_sessions(Scope::Agent(AgentId::ClaudeCode), cx))));
        if codex > 0 {
            filters = filters.child(filter("f-codex", format!("Codex · {codex}"), scope == Scope::Agent(AgentId::Codex), cx, Box::new(|this, cx| this.show_sessions(Scope::Agent(AgentId::Codex), cx))));
        }
        filters = filters.child(filter("f-kept", format!("Kept only · {kept}"), scope == Scope::Kept, cx, Box::new(|this, cx| this.show_sessions(Scope::Kept, cx))));
        if let Scope::Project(p) = &scope {
            filters = filters.child(filter("f-project", format!("{p}  ×"), true, cx, Box::new(|this, cx| this.show_sessions(Scope::All, cx))));
        }

        let rows: Vec<AnyElement> = refs.into_iter().map(|r| {
            let key = key_of(&r);
            let theme = cx.theme().clone();
            let card = self.card_for(&r);
            let dot = self.live_color(&r, cx);
            let working = self.is_working(&r);
            let glyph_id = SharedString::from(format!("row-glyph-{key}"));
            let mut sub = vec![r.project()];
            if !r.git_branch.is_empty() {
                sub.push(format!("⎇ {}", r.git_branch));
            }
            sub.push(relative(r.mtime, now));
            h_flex()
                .id(SharedString::from(format!("row-{key}")))
                .w_full()
                .px(px(12.))
                .py(px(11.))
                .gap(px(12.))
                .items_center()
                .rounded(px(10.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted))
                .on_click(cx.listener(move |this, _, window, cx| this.open_and_focus(&key, window, cx)))
                .child(div().w(px(24.)).flex().justify_center().child(agent_glyph(r.agent, px(16.), agent_color(r.agent, &theme), working, glyph_id)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .items_center()
                                .child(div().min_w_0().truncate().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(r.title.clone()))
                                .when_some(dot, |d, c| d.child(div().size(px(7.)).rounded_full().bg(c).flex_shrink_0()))
                                .when(r.archived, |d| d.child(badge("kept", theme.muted, theme.muted_foreground)))
                                .when(r.agent != AgentId::ClaudeCode, |d| d.child(badge(r.agent.display_name(), theme.muted, theme.muted_foreground))),
                        )
                        .child(div().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(sub.join(" · "))),
                )
                .when(dot.is_some() && !card.text.is_empty(), |d| d.child(div().max_w(px(220.)).truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(card.text.clone())))
                .into_any_element()
        }).collect();

        let title = match &scope {
            Scope::All => "Your sessions".to_string(),
            Scope::Agent(a) => format!("{} sessions", a.display_name()),
            Scope::Project(p) => p.clone(),
            Scope::Kept => "Kept sessions".to_string(),
        };
        let count = self.scoped_refs().len();

        v_flex().flex_1().min_w_0().h_full().bg(theme.background).child(self.render_topbar(String::new(), Vec::new(), cx)).child(
            v_flex().id("sessions").flex_1().min_h_0().overflow_y_scroll().px(px(24.)).items_center().child(
                v_flex()
                    .w_full()
                    .max_w(CONTENT_W)
                    .pt(px(16.))
                    .pb(px(40.))
                    .gap(px(14.))
                    .child(div().text_size(px(28.)).font_family(SERIF).child(title))
                    .child(filters)
                    .child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child(format!("{} on this machine, every one of them kept.", plural(count, "session", "sessions"))))
                    .child(v_flex().w_full().gap(px(2.)).children(rows)),
            ),
        )
    }

    // -- one conversation ----------------------------------------------------

    fn render_detail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(r) = self.selected_ref().cloned() else {
            return v_flex()
                .flex_1()
                .h_full()
                .child(self.render_topbar(String::new(), Vec::new(), cx))
                .child(div().flex_1().flex().items_center().justify_center().text_color(theme.muted_foreground).child("Pick a session, or press ⌘K to search everything."))
                .into_any_element();
        };
        let Some(detail) = &self.detail else {
            return v_flex().flex_1().h_full().child(self.render_topbar(r.title.clone(), Vec::new(), cx)).child(div().flex_1().flex().items_center().justify_center().text_color(theme.muted_foreground).child("loading…")).into_any_element();
        };
        let session = detail.session.clone();
        let list = detail.list.clone();
        let entity = cx.entity().downgrade();
        let tokens = emaki_core::render_md::human_tokens(session.usage.total());

        let mut right: Vec<AnyElement> = Vec::new();
        if r.archived {
            right.push(badge("kept", theme.muted, theme.muted_foreground).into_any_element());
        }
        right.push(agent_badge(r.agent, &theme, self.is_working(&r), "top-glyph").into_any_element());
        right.push(
            icon_button("reveal", IconName::FolderOpen, crate::sys::REVEAL_LABEL, cx, {
                let p = r.path.clone();
                move |_, _, _| crate::sys::reveal_path(&p)
            })
            .into_any_element(),
        );
        let terminal_tip = match self.terminal_check(&r) {
            Ok(()) => "Open in your terminal",
            Err(why) => why,
        };
        let left = vec![icon_button("terminal", Icon::default().path("icons/square-terminal.svg"), terminal_tip, cx, |this, _, cx| this.open_in_terminal(cx)).into_any_element()];
        let tabs = self.render_tabs(cx);
        let topbar = self.render_topbar_with(tabs, left, right, cx);

        let mut meta = vec![emaki_core::paths::tilde(&session.cwd)];
        if !session.git_branch.is_empty() {
            meta.push(format!("⎇ {}", session.git_branch));
        }
        meta.push(plural(session.rounds.len(), "round", "rounds"));
        meta.push(plural(session.tool_count(), "tool call", "tool calls"));
        meta.push(format!("{tokens} tokens"));
        if let Some(m) = session.models.last() {
            meta.push(m.clone());
        }
        meta.push(short_id(&session.id));
        let meta_line = div().w_full().px(px(24.)).pb(px(4.)).text_center().truncate().text_size(px(11.5)).text_color(theme.muted_foreground).child(meta.join("  ·  "));

        let chat_font = crate::fonts::chat_family(&self.cfg.app.chat_font, cx);
        let transcript = div()
            .flex_1()
            .min_h_0()
            .relative()
            .when_some(chat_font, |d, f| d.font_family(f))
            .child(
                gpui::list(list.clone(), move |ix, window, cx| {
                    entity.upgrade().map(|e| e.update(cx, |this, cx| this.render_round(ix, window, cx))).unwrap_or_else(|| div().into_any_element())
                })
                .size_full(),
            )
            .vertical_scrollbar(&list);

        // The agent at work, said under the transcript so it is seen without
        // scrolling: the mark turns while a turn is running.
        let working = self.is_working(&r);
        let status = working.then(|| {
            let st = &r.state;
            let from = if !st.turn_started.is_empty() { &st.turn_started } else if !st.since.is_empty() { &st.since } else { &r.updated };
            let card = self.card_for(&r);
            let what = if card.text.is_empty() { "working".to_string() } else { card.text.clone() };
            h_flex()
                .w_full()
                .max_w(CONTENT_W)
                .px(px(6.))
                .gap(px(8.))
                .items_center()
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .child(agent_glyph(r.agent, px(14.), agent_color(r.agent, &theme), true, "status-glyph"))
                .child(div().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(format!("{} is working", r.agent.speaker())))
                .child(div().flex_1().min_w_0().truncate().child(what))
                .child(div().child(elapsed_since(from, self.now)))
        });

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme.background)
            .child(topbar)
            .child(meta_line)
            .when(self.find_open, |d| d.child(self.render_find_bar(cx)))
            .child(transcript)
            .child(v_flex().w_full().items_center().px(px(24.)).pb(px(14.)).gap(px(8.)).children(status).child(self.render_permissions(cx)).child(self.render_composer(cx)))
            .into_any_element()
    }

    fn render_permissions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let cwd = self.selected_ref().map(|r| r.cwd.clone()).unwrap_or_default();
        let cards: Vec<PermissionRequest> = self.permissions.iter().filter(|(s, _)| *s == sid).map(|(_, p)| p.clone()).collect();
        let several = cards.len() > 1;
        v_flex().w_full().max_w(CONTENT_W).gap(px(8.)).children(cards.into_iter().enumerate().map(|(i, p)| {
            // The oldest card is the one ↩ answers, and says so.
            let first = i == 0;
            let subject = emaki_core::build::tool_subject(&p.tool_name, &p.input, &cwd);
            let detail = match p.tool_name.as_str() {
                "Bash" => p.input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                _ => emaki_core::render_md::pretty_args(&p.input, 800),
            };
            let id_allow = p.request_id.clone();
            let id_deny = p.request_id.clone();
            // What it does, in words: the model's answer when one has
            // landed, else the free line a simple call carries in its
            // arguments; "Explaining…" while a model is on it.
            let call_id = if p.tool_use_id.is_empty() { p.request_id.clone() } else { p.tool_use_id.clone() };
            let mut explanation = self.explanations.get(&call_id).cloned().unwrap_or_default();
            if explanation.is_empty() {
                explanation = self.hub.explainer.lookup(&p.tool_name, &p.input);
            }
            if explanation.is_empty() {
                explanation = emaki_core::explain::canned(&p.tool_name, &p.input, &cwd);
            }
            let explaining = explanation.is_empty() && self.hub.explainer.in_flight(&p.tool_name, &p.input);
            v_flex()
                .p(px(14.))
                .gap(px(8.))
                .rounded(px(14.))
                .border_1()
                .border_color(theme.primary)
                .bg(theme.popover)
                .shadow_sm()
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(Icon::new(IconName::TriangleAlert).with_size(px(14.)).text_color(theme.primary))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(format!("Claude wants to run {}", p.tool_name)))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(theme.muted_foreground).child(subject)),
                )
                .when(!detail.is_empty(), |d| d.child(div().p(px(8.)).rounded(px(8.)).bg(theme.muted).font_family(theme.mono_font_family.clone()).text_size(px(12.)).whitespace_normal().child(detail)))
                .when(!explanation.is_empty() || explaining, |d| d.child(crate::transcript::explain_card(format!("pex-{}", p.request_id), explanation.clone(), explaining, px(13.5), cx)))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(Button::new(SharedString::from(format!("allow-{}", p.request_id))).primary().small().label(if first { "Allow  ↩" } else { "Allow" }).on_click(cx.listener(move |this, _, _, cx| this.answer_permission(id_allow.clone(), true, cx))))
                        .child(Button::new(SharedString::from(format!("deny-{}", p.request_id))).outline().small().label(if first { "Deny  ⇧↩" } else { "Deny" }).on_click(cx.listener(move |this, _, _, cx| this.answer_permission(id_deny.clone(), false, cx))))
                        .when(first && several, |d| d.child(div().flex_1()).child(Button::new("allow-all").ghost().small().label("Allow all").on_click(cx.listener(|this, _, _, cx| this.allow_all_pending(cx))))),
                )
        }))
    }

    /// The composer card: a textarea, the mode and model chips, and a round
    /// send button. It floats at the foot of a conversation and in the middle
    /// of the home page.
    fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (via, why) = self.reply_via();
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let drv = self.drivers.get(&sid).cloned();
        let mode = self.current_mode();
        let model = self.current_model();
        let running = drv.as_ref().map(|v| v.state == "running" || v.starting).unwrap_or(false);
        let can_send = via == "driver" || via == "spawn" || via == "inbox";
        let settable = via == "driver" || via == "spawn";
        // A terminal session shows its mode, model and effort but cannot
        // take a change: the inbox reads everything as prose.
        let readonly = via == "inbox";
        let effort = self.current_effort();
        let on_session = self.page == Page::Session;
        // The right of the row under the composer: a notice while one is
        // showing (what the last send or action did, an error in red), else
        // why nothing can send, else empty. The row always takes `NOTICE_H`,
        // so the composer stays put as a notice comes and goes, and it is
        // the only thing under the card: the limits sit on its left.
        let (hint, hint_color) = if let Some(n) = &self.notice {
            (n.text.clone(), if n.error { theme.danger } else { theme.muted_foreground })
        } else if can_send {
            (String::new(), theme.muted_foreground)
        } else {
            let mut w = why.clone();
            if let Some(f) = w.get(..1) {
                w = f.to_uppercase() + &w[1..];
            }
            (w, theme.muted_foreground)
        };
        let send = div()
            .id("send")
            .size(px(32.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .map(|d| {
                if running {
                    d.bg(theme.primary).cursor_pointer().hover(|s| s.bg(theme.primary_hover)).on_click(cx.listener(|this, _, _, cx| this.interrupt(cx))).child(Icon::new(IconName::Pause).with_size(px(15.)).text_color(theme.primary_foreground))
                } else if can_send {
                    d.bg(theme.primary).cursor_pointer().hover(|s| s.bg(theme.primary_hover)).on_click(cx.listener(|this, _, window, cx| this.send_message(window, cx))).child(Icon::new(IconName::ArrowUp).with_size(px(16.)).text_color(theme.primary_foreground))
                } else {
                    d.bg(theme.muted).child(Icon::new(IconName::ArrowUp).with_size(px(16.)).text_color(theme.muted_foreground))
                }
            });

        let attachments = self.attachments.clone();
        // The toolkit's frame tracks a focus handle of its own and only asks
        // whether it *contains* the focus, so gpui never marks the editor as
        // the focused accessibility node and assistive apps are handed the
        // window instead. This wrapper tracks the editor's real handle and
        // carries the text-area role, so the focused element is a text area
        // with the typed value.
        let field_focus = self.composer.read(cx).focus_handle(cx);
        let field_value = self.composer.read(cx).value().to_string();
        let card = v_flex()
            .id("composer-card")
            .w_full()
            .max_w(CONTENT_W)
            .rounded(px(18.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .px(px(14.))
            .pt(px(10.))
            .pb(px(10.))
            .gap(px(6.))
            .drag_over::<ExternalPaths>({
                let accent = theme.primary;
                move |s, _, _, _| s.border_color(accent)
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.attach_paths(paths.paths(), cx)))
            .on_click(cx.listener(|this, _, window, cx| this.focus_composer(window, cx)))
            .when(!attachments.is_empty(), |d| {
                d.child(h_flex().flex_wrap().gap(px(6.)).pb(px(2.)).children(attachments.into_iter().enumerate().map(|(i, a)| {
                    let theme = cx.theme().clone();
                    let size = std::fs::metadata(&a.path).map(|m| m.len()).unwrap_or(0);
                    let (a_title, a_path, a_image) = (a.name.clone(), a.path.clone(), a.image);
                    h_flex()
                        .id(SharedString::from(format!("att-{i}")))
                        .h(px(48.))
                        .pl(px(4.))
                        .pr(px(6.))
                        .gap(px(8.))
                        .items_center()
                        .rounded(px(12.))
                        .bg(theme.muted)
                        .border_1()
                        .border_color(theme.border)
                        .text_size(px(12.))
                        .cursor_pointer()
                        .hover(|s| s.border_color(theme.primary))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let image = a_image.then(|| ImageSource::from(a_path.clone()));
                            this.preview_attachment(a_title.clone(), Some(a_path.clone()), image, window, cx);
                        }))
                        .child(if a.image {
                            // The whole picture, at its own shape, 40px tall.
                            let (w, h) = fit_thumb(a.size, 96., 40., (40., 40.));
                            img(a.path.clone()).w(px(w)).h(px(h)).rounded(px(8.)).object_fit(ObjectFit::Contain).bg(theme.border).into_any_element()
                        } else {
                            div().size(px(40.)).rounded(px(8.)).bg(theme.popover).flex().items_center().justify_center().child(file_icon(&a.name, px(20.), theme.muted_foreground)).into_any_element()
                        })
                        .child(
                            v_flex()
                                .gap(px(1.))
                                .child(div().max_w(px(200.)).truncate().font_weight(FontWeight::MEDIUM).child(a.name.clone()))
                                .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(format!("{} · {}", file_kind(&a.name), human_size(size)))),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("att-x-{i}")))
                                .size(px(18.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.border))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    swallow_click(window, cx);
                                    if i < this.attachments.len() {
                                        this.attachments.remove(i);
                                    }
                                    cx.notify();
                                }))
                                .child(Icon::new(IconName::Close).with_size(px(11.)).text_color(theme.muted_foreground)),
                        )
                })))
            })
            .child(
                div()
                    .id("composer")
                    .key_context(COMPOSER_CONTEXT)
                    .on_action(cx.listener(|this, _: &Send, window, cx| this.send_message(window, cx)))
                    .capture_action(cx.listener(|this, _: &gpui_component::input::Paste, _, cx| {
                        if this.paste_attachments(cx) {
                            cx.stop_propagation();
                        }
                    }))
                    // ⇧Tab cycles the permission mode, as in Claude Code's
                    // terminal; the textarea would otherwise outdent.
                    .capture_action(cx.listener(|this, _: &OutdentInline, _, cx| {
                        this.cycle_mode(cx);
                        cx.stop_propagation();
                    }))
                    // ⌘↩ sends. Taken here, before the textarea sees it,
                    // because the textarea puts a newline at the caret for
                    // every Enter and only then reports the key, which left
                    // a line break wherever the caret stood in a sent
                    // message. A bare ↩ still reaches the textarea.
                    .capture_action(cx.listener(|this, a: &gpui_component::input::Enter, window, cx| {
                        if a.secondary {
                            this.send_message(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .role(Role::MultilineTextInput)
                    .track_focus(&field_focus)
                    .aria_label("Message")
                    .aria_placeholder("Reply")
                    .aria_value(field_value)
                    .w_full()
                    .text_size(px(14.))
                    .child(Textarea::new(&self.composer).appearance(false).bordered(false)),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .id("attach")
                            .size(px(30.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_1()
                            .border_color(theme.border)
                            .text_color(theme.muted_foreground)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted).text_color(theme.foreground))
                            .on_click(cx.listener(|this, _, window, cx| {
                                swallow_click(window, cx);
                                this.pick_files(cx)
                            }))
                            .child(Icon::new(IconName::Plus).with_size(px(15.))),
                    )
                    .when(settable, |d| {
                        let options = self.modes().into_iter().map(|m| (m, mode_label(m).to_string(), mode_detail(m).to_string())).collect();
                        d.child(picker("mode", mode_label(&mode).to_string(), options, mode.clone(), Anchor::BottomLeft, cx.entity().downgrade(), Rc::new(|this, key, cx| this.set_mode(key, cx)), cx))
                    })
                    .when(settable && on_session, |d| {
                        let options = EFFORTS.iter().map(|e| (*e, effort_label(e), effort_detail(e).to_string())).collect();
                        d.child(picker("effort", effort_label(&effort), options, effort.clone(), Anchor::BottomLeft, cx.entity().downgrade(), Rc::new(|this, key, cx| this.set_effort(key, cx)), cx))
                    })
                    .when(readonly, |d| d.child(chip_static("ro-mode", mode_label(&mode).to_string(), cx)).child(chip_static("ro-effort", effort_label(&effort), cx)))
                    .child(div().flex_1())
                    .when(settable, |d| {
                        let options = MODELS.iter().map(|m| (*m, model_label(m), model_detail(m).to_string())).collect();
                        d.child(picker("model", model_label(&model), options, model_key(&model).to_string(), Anchor::BottomRight, cx.entity().downgrade(), Rc::new(|this, key, cx| this.set_model(key, cx)), cx))
                    })
                    .when(readonly, |d| d.child(chip_static("ro-model", model_label(&model), cx)))
                    .child(send),
            );

        let foot = h_flex()
            .w_full()
            .max_w(CONTENT_W)
            .h(NOTICE_H)
            .flex_shrink_0()
            .px(px(6.))
            .gap(px(8.))
            .items_center()
            .text_size(px(11.5))
            .children(self.render_limits(cx))
            .child(div().flex_1())
            .child(div().min_w_0().truncate().text_color(hint_color).child(hint));
        v_flex().w_full().items_center().gap(px(8.)).child(card).child(foot)
    }

    // -- the board -----------------------------------------------------------

    fn render_board(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut cols: HashMap<Column, Vec<Card>> = HashMap::new();
        for r in &self.refs {
            let card = self.card_for(r);
            cols.entry(card.column).or_default().push(card);
        }
        // Whoever has waited longest on you comes first; everything else newest first.
        let since_of = |c: &Card| emaki_core::build::parse_ts(if c.r.state.since.is_empty() { &c.r.updated } else { &c.r.state.since }).map(|d| d.timestamp()).unwrap_or(0);
        if let Some(v) = cols.get_mut(&Column::NeedsYou) {
            v.sort_by_key(since_of);
        }
        for col in [Column::Planning, Column::Working, Column::YourTurn] {
            if let Some(v) = cols.get_mut(&col) {
                v.sort_by_key(|c| std::cmp::Reverse(since_of(c)));
            }
        }
        let needs = cols.get(&Column::NeedsYou).map(Vec::len).unwrap_or(0);
        let done = cols.remove(&Column::Done).unwrap_or_default();
        let live: usize = cols.values().map(Vec::len).sum();
        let projects: HashSet<String> = cols.values().flatten().map(|c| c.r.project()).collect();

        let stats = div().text_size(px(12.)).text_color(theme.muted_foreground).whitespace_nowrap().child(format!("{live} live · {needs} need you · {}", plural(projects.len(), "project", "projects"))).into_any_element();

        v_flex().flex_1().min_w_0().h_full().bg(theme.background).child(self.render_topbar("Board".into(), vec![stats], cx)).child(
            h_flex()
                .id("board")
                .flex_1()
                .min_h_0()
                .px(px(16.))
                .pt(px(4.))
                .pb(px(16.))
                .gap(px(12.))
                .items_stretch()
                .overflow_x_scroll()
                .children(Column::LIVE.into_iter().map(|c| {
                    let cards = cols.remove(&c).unwrap_or_default();
                    self.render_column(c, cards, cx)
                }))
                .child(self.render_done(done, cx)),
        )
    }

    fn column_color(&self, c: Column, cx: &App) -> Hsla {
        let theme = cx.theme();
        match c {
            Column::NeedsYou => theme.primary,
            Column::Planning | Column::Working => theme.blue,
            Column::YourTurn => theme.green,
            Column::Done => theme.muted_foreground,
        }
    }

    fn dot(&self, c: Column, cx: &App) -> Div {
        let color = self.column_color(c, cx);
        let d = div().size(px(8.)).rounded_full().flex_shrink_0();
        match c {
            Column::Planning => d.border_2().border_color(color),
            Column::NeedsYou => d.bg(color).shadow(vec![BoxShadow { color: color.opacity(0.25), offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(3.), inset: false }]),
            _ => d.bg(color),
        }
    }

    fn render_column(&self, c: Column, cards: Vec<Card>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let count = cards.len();
        let mut col = v_flex()
            .flex_1()
            .min_w(COL_MIN_W)
            .h_full()
            .min_h_0()
            .bg(theme.sidebar)
            .border_1()
            .border_color(theme.border)
            .rounded(px(14.))
            .child(
                h_flex()
                    .px(px(14.))
                    .pt(px(12.))
                    .pb(px(8.))
                    .gap(px(8.))
                    .items_center()
                    .child(self.dot(c, cx))
                    .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).child(c.title()))
                    .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(count.to_string()))
                    .when(c == Column::Done, |d| {
                        d.child(div().flex_1()).child(icon_button("done-fold", IconName::PanelRightClose, "Collapse", cx, |this, _, cx| {
                            this.done_open = false;
                            cx.notify();
                        }))
                    }),
            );
        if cards.is_empty() {
            col = col.child(div().flex_1().flex().items_center().justify_center().px(px(16.)).text_size(px(12.)).text_color(theme.muted_foreground).child(c.empty()));
        } else {
            let shown = cards.len().min(60);
            let more = cards.len() - shown;
            col = col.child(
                v_flex()
                    .id(SharedString::from(format!("cards-{}", c.title())))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(8.))
                    .pb(px(10.))
                    .gap(px(8.))
                    .children(cards.into_iter().take(shown).map(|card| self.render_card(card, cx)))
                    .when(more > 0, |d| d.child(div().py(px(8.)).text_size(px(11.)).text_color(theme.muted_foreground).text_center().child(format!("{more} more in the list")))),
            );
        }
        col.into_any_element()
    }

    /// Done is a strip with a count until opened: every session with no
    /// process behind it, newest first.
    fn render_done(&self, done: Vec<Card>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let kept = done.iter().filter(|c| c.r.archived).count();
        if self.done_open {
            return self.render_column(Column::Done, done, cx);
        }
        let mut strip = v_flex()
            .id("done-strip")
            .w(px(52.))
            .flex_shrink_0()
            .h_full()
            .items_center()
            .pt(px(13.))
            .pb(px(12.))
            .gap(px(6.))
            .bg(theme.sidebar)
            .border_1()
            .border_color(theme.border)
            .rounded(px(14.))
            .cursor_pointer()
            .hover(|s| s.bg(theme.list_hover))
            .on_click(cx.listener(|this, _, _, cx| {
                this.done_open = true;
                cx.notify();
            }))
            .child(self.dot(Column::Done, cx))
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(done.len().to_string()))
            .child(v_flex().pt(px(6.)).items_center().text_size(px(10.)).text_color(theme.muted_foreground).children("done".chars().map(|ch| div().h(px(12.)).child(ch.to_string()))));
        if kept > 0 {
            strip = strip.child(div().flex_1()).child(badge_str(format!("{kept} kept"), theme.muted, theme.muted_foreground));
        }
        strip.into_any_element()
    }

    fn render_card(&self, card: Card, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let r = card.r.clone();
        let key = key_of(&r);
        let done = card.column == Column::Done;
        let accent = self.column_color(card.column, cx);
        let open_key = key.clone();
        let mut el = v_flex()
            .id(SharedString::from(format!("card-{key}")))
            .relative()
            .w_full()
            .pl(px(14.))
            .pr(px(12.))
            .pt(px(10.))
            .pb(px(9.))
            .gap(px(5.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .rounded(px(10.))
            .when(!done, |d| d.shadow_sm())
            .cursor_pointer()
            .hover(|s| s.border_color(accent))
            .on_click(cx.listener(move |this, _, _, cx| this.open_session(&open_key, cx)))
            .child(div().absolute().left(px(0.)).top(px(8.)).bottom(px(8.)).w(px(3.)).rounded_r(px(2.)).bg(if done { theme.border } else { accent }))
            .child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .text_size(px(10.5))
                    .text_color(theme.muted_foreground)
                    .child(div().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(r.project()))
                    .when(!r.git_branch.is_empty(), |d| d.child(div().min_w_0().truncate().child(format!("· {}", r.git_branch))))
                    .child(div().flex_1())
                    .child(agent_badge(r.agent, &theme, matches!(card.column, Column::Working | Column::Planning), SharedString::from(format!("card-glyph-{key}")))),
            )
            .child(div().text_size(if done { px(12.5) } else { px(13.5) }).font_weight(FontWeight::MEDIUM).line_height(gpui::relative(1.3)).line_clamp(2).child(r.title.clone()));

        if !done {
            let (chip_bg, chip_fg) = match card.chip_kind {
                "bash" | "task" | "web" | "mcp" | "plan" | "search" | "read" => (theme.blue.opacity(0.14), theme.blue),
                "edit" | "write" | "approve" | "terminal" | "ask" => (theme.primary.opacity(0.14), theme.primary),
                "reply" => (theme.green.opacity(0.16), theme.green),
                "stop" => (theme.red.opacity(0.16), theme.red),
                _ => (theme.muted, theme.muted_foreground),
            };
            let working = matches!(card.column, Column::Working | Column::Planning);
            let clock_text = if working {
                let from = if !r.state.turn_started.is_empty() { &r.state.turn_started } else if !r.state.since.is_empty() { &r.state.since } else { &r.updated };
                elapsed_since(from, self.now)
            } else {
                ago(if r.state.since.is_empty() { &r.updated } else { &r.state.since }, self.now)
            };
            el = el.child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .text_size(px(11.5))
                    .child(div().flex_shrink_0().px(px(6.)).py(px(1.)).rounded(px(5.)).bg(chip_bg).text_color(chip_fg).text_size(px(10.5)).font_weight(FontWeight::MEDIUM).child(card.chip.clone()))
                    .child(div().flex_1().min_w_0().truncate().child(card.text.clone()))
                    .child(div().flex_shrink_0().text_size(px(11.)).text_color(theme.muted_foreground).child(clock_text)),
            );
        }

        let mut foot = h_flex().gap(px(6.)).items_center().pt(px(2.)).text_size(px(10.5)).text_color(theme.muted_foreground);
        foot = foot.child(div().child(if done { format!("{} {}", day(&r.updated), clock(&r.updated)) } else { format!("since {}", clock(if r.started.is_empty() { &r.updated } else { &r.started })) }));
        if card.queued > 0 {
            foot = foot.child(badge_str(format!("{} queued", card.queued), theme.blue.opacity(0.14), theme.blue));
        }
        if r.archived {
            foot = foot.child(badge("kept", theme.muted, theme.muted_foreground));
        }
        foot = foot.child(div().flex_1());
        let (via, _) = self.reply_via_for(&r);
        let small = |id: String, label: &'static str, primary: bool, cx: &mut Context<Self>, on: Box<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>| {
            let b = Button::new(SharedString::from(id)).small().compact().label(label).on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                on(this, window, cx)
            }));
            if primary { b.primary() } else { b.outline() }
        };
        match (card.pending.clone(), card.column) {
            (Some(pend), _) => {
                let deny_id = pend.request_id.clone();
                let allow_id = pend.request_id.clone();
                foot = foot
                    .child(small(format!("deny-{key}"), "deny", false, cx, Box::new(move |this, _, cx| this.answer_permission(deny_id.clone(), false, cx))))
                    .child(small(format!("allow-{key}"), "approve", true, cx, Box::new(move |this, _, cx| this.answer_permission(allow_id.clone(), true, cx))));
            }
            (None, Column::NeedsYou) => {
                let k = key.clone();
                foot = foot.child(small(format!("answer-{key}"), "answer", true, cx, Box::new(move |this, _, cx| this.open_session(&k, cx))));
            }
            (None, Column::YourTurn) => {
                if !via.is_empty() && via != "wait" {
                    let k = key.clone();
                    foot = foot.child(small(format!("reply-{key}"), "reply", false, cx, Box::new(move |this, window, cx| this.reply_from_board(&k, window, cx))));
                }
                let k = key.clone();
                foot = foot.child(small(format!("read-{key}"), "read", true, cx, Box::new(move |this, _, cx| this.open_session(&k, cx))));
            }
            (None, Column::Done) => {
                if via == "spawn" {
                    let k = key.clone();
                    foot = foot.child(small(format!("cont-{key}"), "continue", false, cx, Box::new(move |this, window, cx| this.reply_from_board(&k, window, cx))));
                }
            }
            _ => {
                let k = key.clone();
                foot = foot.child(small(format!("open-{key}"), "open", false, cx, Box::new(move |this, _, cx| this.open_session(&k, cx))));
            }
        }
        el.child(foot).into_any_element()
    }

    // -- new session: the home page -------------------------------------------

    fn render_new(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let cwds = self.recent_cwds();
        let chosen = self.new_cwd.clone();
        let folders = h_flex().w_full().max_w(CONTENT_W).flex_wrap().gap(px(6.)).justify_center().children(cwds.into_iter().map(|c| {
            let active = c == chosen;
            let theme = cx.theme().clone();
            let label = emaki_core::paths::tilde(&c);
            h_flex()
                .id(SharedString::from(format!("cwd-{c}")))
                .h(px(30.))
                .px(px(12.))
                .gap(px(6.))
                .items_center()
                .rounded_full()
                .border_1()
                .border_color(if active { theme.primary } else { theme.border })
                .when(active, |d| d.bg(theme.primary.opacity(0.10)))
                .when(!active, |d| d.hover(|s| s.bg(theme.muted)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.new_cwd = c.clone();
                    cx.notify();
                }))
                .child(Icon::new(IconName::Folder).with_size(px(13.)).text_color(if active { theme.primary } else { theme.muted_foreground }))
                .child(div().text_size(px(12.5)).child(label))
        }));

        v_flex().flex_1().min_w_0().h_full().bg(theme.background).child(self.render_topbar(String::new(), Vec::new(), cx)).child(
            v_flex().id("home").flex_1().min_h_0().overflow_y_scroll().px(px(24.)).child(
                v_flex()
                    .w_full()
                    .min_h_full()
                    .items_center()
                    .justify_center()
                    .pb(px(48.))
                    .gap(px(22.))
                    .child(h_flex().gap(px(14.)).items_center().child(mark_icon(px(30.), theme.primary)).child(div().text_size(px(34.)).font_family(SERIF).child(greeting(&self.user_name))))
                    .child(self.render_composer(cx))
                    .child(v_flex().w_full().items_center().gap(px(8.)).pt(px(6.)).child(div().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("START IN")).child(folders)),
            ),
        )
    }

    // -- lightbox --------------------------------------------------------------

    fn render_lightbox(&self, lb: Lightbox, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let path_open = lb.path.clone();
        let path_reveal = lb.path.clone();
        div()
            .id("lightbox")
            .absolute()
            .inset_0()
            // Nothing under the lightbox hears the mouse while it is up:
            // without this, moving over the backdrop reached the selectable
            // text beneath and dragged a selection across it.
            .occlude()
            .bg(gpui::black().opacity(0.72))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.lightbox = None;
                cx.notify();
            }))
            .child(div().id("lightbox-img").on_click(|_, window, cx| swallow_click(window, cx)).max_w(gpui::relative(0.88)).max_h(gpui::relative(0.8)).rounded(px(12.)).overflow_hidden().shadow_lg().child(img(lb.source.clone()).max_w(gpui::relative(1.0)).max_h(gpui::relative(1.0)).object_fit(ObjectFit::Contain)))
            // The bar under the picture: the whole name or path, in the mono
            // face, selectable and wrapping rather than cut short, with Open
            // and Reveal when there is a file behind it.
            .child(
                h_flex()
                    .id("lightbox-bar")
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .gap(px(12.))
                    .items_center()
                    .max_w(gpui::relative(0.88))
                    .px(px(16.))
                    .py(px(10.))
                    .rounded(px(12.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow_md()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(theme.mono_font_family.clone())
                            .text_size(px(12.5))
                            .whitespace_normal()
                            .child(crate::transcript::md_view("lightbox-title".into(), lb.title.clone(), cx)),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .gap(px(4.))
                            .items_center()
                            .when_some(path_open, |d, p| d.child(pill_button("lb-open", "Open", &theme, move |_, _, _| crate::sys::open_path(&p))))
                            .when_some(path_reveal, |d, p| d.child(pill_button("lb-reveal", crate::sys::REVEAL_LABEL, &theme, move |_, _, _| crate::sys::reveal_path(&p))))
                            .child(Button::new("lb-close").ghost().small().icon(Icon::new(IconName::Close)).tooltip("Close (esc)").on_click(cx.listener(|this, _, _, cx| {
                                this.lightbox = None;
                                cx.notify();
                            }))),
                    ),
            )
    }

    // -- search palette -------------------------------------------------------

    fn render_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let results = self.search_results.clone();
        let search_focus = self.search_input.read(cx).focus_handle(cx);
        let search_value = self.search_input.read(cx).value().to_string();
        div()
            .id("search-overlay")
            .absolute()
            .inset_0()
            .bg(theme.overlay)
            .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))
            .child(
                v_flex()
                    .id("search-panel")
                    .key_context(SEARCH_CONTEXT)
                    .on_action(cx.listener(|this, _: &Escape, window, cx| this.close_search(window, cx)))
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .absolute()
                    .top(px(80.))
                    .left_0()
                    .right_0()
                    .mx_auto()
                    .w(px(680.))
                    .max_h(px(520.))
                    .rounded(px(16.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .child(
                        h_flex()
                            .px(px(16.))
                            .h(px(50.))
                            .gap(px(10.))
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(Icon::new(IconName::Search).with_size(px(16.)).text_color(theme.muted_foreground))
                            // Tracks the editor's own focus handle, as the composer
                            // does, so keyboard focus set from code lands on a node
                            // the key bindings and assistive apps can see.
                            .child(
                                div()
                                    .id("search-field")
                                    .track_focus(&search_focus)
                                    .role(Role::TextInput)
                                    .aria_label("Search")
                                    .aria_value(search_value)
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .child(Input::new(&self.search_input).appearance(false).bordered(false)),
                            ),
                    )
                    .child(v_flex().id("search-results").flex_1().min_h_0().overflow_y_scroll().p(px(8.)).map(|d| match results {
                        None => d.child(div().p(px(12.)).text_size(px(12.5)).text_color(theme.muted_foreground).child("Type to search prompts, replies, thoughts and tool calls across every session.")),
                        Some(res) if !res.error.is_empty() => d.child(div().p(px(12.)).text_size(px(12.5)).text_color(theme.danger).child(res.error)),
                        Some(res) if res.sessions.is_empty() => d.child(div().p(px(12.)).text_size(px(12.5)).text_color(theme.muted_foreground).child("No matches.")),
                        Some(res) => d.children(res.sessions.into_iter().take(40).map(|s| {
                            let key = format!("{}:{}", s.agent, s.id);
                            let query = res.query.clone();
                            let round = s.matches.first().map(|m| m.round.max(0) as usize);
                            let theme = cx.theme().clone();
                            v_flex()
                                .id(SharedString::from(format!("hit-{key}")))
                                .px(px(12.))
                                .py(px(8.))
                                .gap(px(3.))
                                .rounded(px(10.))
                                .cursor_pointer()
                                .hover(|st| st.bg(theme.list_hover))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.close_search(window, cx);
                                    this.open_with_find(&key, &query, round, window, cx);
                                }))
                                .child(
                                    h_flex()
                                        .gap(px(8.))
                                        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(s.title.clone()))
                                        .when(s.archived, |d| d.child(badge("kept", theme.muted, theme.muted_foreground)))
                                        .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(format!("{} · {}", s.project, plural(s.hits, "hit", "hits")))),
                                )
                                .children(s.matches.into_iter().take(3).map(|m| {
                                    let theme = cx.theme().clone();
                                    h_flex()
                                        .gap(px(6.))
                                        .text_size(px(11.5))
                                        .text_color(theme.muted_foreground)
                                        .child(div().flex_shrink_0().child(format!("r{} {}", m.round, m.kind)))
                                        .child(div().flex_1().min_w_0().truncate().child(m.snippet.replace(['\x02', '\x03'], "").replace('\n', " ")))
                                }))
                        })),
                    })),
            )
    }
}

fn kind_static(k: &str) -> &'static str {
    match k {
        "bash" => "bash",
        "edit" => "edit",
        "write" => "write",
        "read" => "read",
        "search" => "search",
        "web" => "web",
        "task" => "task",
        "todo" => "todo",
        "ask" => "ask",
        "plan" => "plan",
        "mcp" => "mcp",
        "reply" => "reply",
        "stop" => "stop",
        "approve" => "approve",
        "terminal" => "terminal",
        _ => "wait",
    }
}

fn ago(ts: &str, now: f64) -> String {
    let Some(t) = emaki_core::build::parse_ts(ts) else { return String::new() };
    let d = (now - t.timestamp() as f64).max(0.0);
    if d < 60.0 {
        format!("{}s", d as u64)
    } else if d < 3600.0 {
        format!("{}m", (d / 60.0) as u64)
    } else if d < 86_400.0 {
        format!("{}h", (d / 3600.0) as u64)
    } else {
        format!("{}d", (d / 86_400.0) as u64)
    }
}

pub fn badge(text: &'static str, bg: Hsla, fg: Hsla) -> impl IntoElement {
    div().px(px(6.)).py(px(1.)).rounded(px(5.)).bg(bg).text_color(fg).text_size(px(10.5)).font_weight(FontWeight::MEDIUM).child(text)
}

pub fn badge_str(text: String, bg: Hsla, fg: Hsla) -> impl IntoElement {
    div().px(px(6.)).py(px(1.)).rounded(px(5.)).bg(bg).text_color(fg).text_size(px(10.5)).font_weight(FontWeight::MEDIUM).child(text)
}

/// The image format gpui should decode `mime` as, when it can.
pub fn image_format(mime: &str) -> Option<ImageFormat> {
    match mime {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::Webp),
        "image/svg+xml" => Some(ImageFormat::Svg),
        "image/bmp" => Some(ImageFormat::Bmp),
        "image/tiff" => Some(ImageFormat::Tiff),
        _ => None,
    }
}

/// An icon for a file with no picture to show.
pub fn file_icon(name: &str, size: Pixels, color: Hsla) -> Icon {
    Icon::default().path(crate::assets::file_icon_path(name)).with_size(size).text_color(color)
}

/// What to call a file in a chip: its extension, upper case, or "file".
pub fn file_kind(name: &str) -> String {
    match std::path::Path::new(name).extension().and_then(|e| e.to_str()) {
        Some(e) if !e.is_empty() => e.to_ascii_uppercase(),
        _ => "file".into(),
    }
}

pub fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Our mark: the four-armed burst that stands for Claude in this window.
pub fn mark_icon(size: Pixels, color: Hsla) -> Icon {
    Icon::default().path("icons/mark.svg").with_size(size).text_color(color)
}

/// The glyph that says which agent a session belongs to.
pub fn agent_icon(agent: AgentId, size: Pixels, color: Hsla) -> Icon {
    match agent {
        AgentId::ClaudeCode => mark_icon(size, color),
        AgentId::Codex => Icon::new(IconName::SquareTerminal).with_size(size).text_color(color),
    }
}

/// Claude wears the accent; every other agent is drawn in the muted ink, so
/// a glance down the recents tells them apart.
pub fn agent_color(agent: AgentId, theme: &gpui_component::Theme) -> Hsla {
    match agent {
        AgentId::ClaudeCode => theme.primary,
        AgentId::Codex => theme.muted_foreground,
    }
}

/// Which agent a card belongs to, said on every card so the board never
/// needs a legend.
fn agent_badge(agent: AgentId, theme: &gpui_component::Theme, working: bool, id: impl Into<SharedString>) -> impl IntoElement {
    let label = match agent {
        AgentId::ClaudeCode => "Claude",
        AgentId::Codex => "Codex",
    };
    let color = if working { agent_color(agent, theme) } else { theme.muted_foreground };
    h_flex().gap(px(4.)).items_center().flex_shrink_0().child(agent_glyph(agent, px(11.), color, working, id)).child(div().text_size(px(10.5)).text_color(theme.muted_foreground).child(label))
}

/// The agent's glyph, turning (Claude) or breathing (Codex) while the agent
/// is at work, still otherwise. The animation needs a stable id per place.
pub fn agent_glyph(agent: AgentId, size: Pixels, color: Hsla, working: bool, id: impl Into<SharedString>) -> AnyElement {
    if !working {
        return agent_icon(agent, size, color).into_any_element();
    }
    let id: SharedString = id.into();
    match agent {
        AgentId::ClaudeCode => agent_icon(agent, size, color)
            .with_animation(ElementId::Name(id), Animation::new(Duration::from_millis(1400)).repeat().with_easing(ease_in_out), |icon, t| icon.rotate(gpui::Radians(t * std::f32::consts::FRAC_PI_2)))
            .into_any_element(),
        AgentId::Codex => div()
            .child(agent_icon(agent, size, color))
            .with_animation(ElementId::Name(id), Animation::new(Duration::from_millis(1200)).repeat().with_easing(pulsating_between(0.3, 1.0)), |d, t| d.opacity(t))
            .into_any_element(),
    }
}

/// A small outlined pill with a word on it, the same drawing as the Explain
/// pill on a tool row: the label sits in the middle of a fixed height, so
/// two of them side by side line up.
pub fn pill_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: &gpui_component::Theme, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    h_flex()
        .id(id)
        .flex_shrink_0()
        .h(px(24.))
        .px(px(10.))
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(theme.foreground)
        .hover(|s| s.border_color(theme.primary.opacity(0.6)).bg(theme.primary.opacity(0.10)))
        .active(|s| s.border_color(theme.primary).bg(theme.primary.opacity(0.22)))
        .on_click(on_click)
        .child(label.into())
}

fn kbd_hint(text: &'static str, theme: &gpui_component::Theme) -> impl IntoElement {
    div().text_size(px(11.)).text_color(theme.muted_foreground.opacity(0.8)).child(text)
}

/// A round ghost button holding one icon.
fn icon_button(id: &'static str, icon: impl Into<Icon>, tip: &'static str, cx: &mut Context<Workbench>, on: impl Fn(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static) -> impl IntoElement {
    let theme = cx.theme().clone();
    Button::new(id).ghost().small().icon(Icon::new(icon).with_size(px(16.)).text_color(theme.muted_foreground)).tooltip(tip).on_click(cx.listener(move |this, _, window, cx| on(this, window, cx)))
}

/// A pill under the composer that only says what a terminal session is
/// set to; the terminal is where it changes.
fn chip_static(id: &'static str, label: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    h_flex()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .items_center()
        .rounded_full()
        .text_size(px(12.5))
        .text_color(theme.muted_foreground)
        .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Set in the terminal: ⇧Tab for the mode, /model, /effort").build(window, cx))
        .child(label)
}

/// A pill under the composer that opens a list of choices: the permission
/// mode and the model. Every choice is on the list with a line on what it
/// does, the current one ticked, so all of them can be reached (a pill that
/// cycled on click hid the fourth mode behind three clicks and a label that
/// did not know it). Picking one calls `on` with its key.
#[allow(clippy::too_many_arguments)]
fn picker(
    id: &'static str,
    label: String,
    options: Vec<(&'static str, String, String)>,
    current: String,
    anchor: Anchor,
    wb: WeakEntity<Workbench>,
    on: Rc<dyn Fn(&mut Workbench, &str, &mut Context<Workbench>)>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let options = Rc::new(options);
    let trigger = Button::new(SharedString::from(format!("{id}-trigger"))).ghost().small().label(label).dropdown_caret(true).text_color(theme.muted_foreground);
    Popover::new(id).anchor(anchor).trigger(trigger).content(move |_, _, cx| {
        let theme = cx.theme().clone();
        let popover = cx.entity();
        v_flex().min_w(px(250.)).gap(px(2.)).children(options.iter().map(|(key, name, detail)| {
            let active = *key == current;
            let (wb, on, popover, key) = (wb.clone(), on.clone(), popover.clone(), *key);
            v_flex()
                .id(SharedString::from(format!("{id}-{key}")))
                .px(px(10.))
                .py(px(6.))
                .gap(px(1.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(active, |d| d.bg(theme.muted))
                .hover(|s| s.bg(theme.list_hover))
                .on_click(move |_, window, cx| {
                    let _ = wb.update(cx, |this, cx| on(this, key, cx));
                    popover.update(cx, |s, cx| s.dismiss(window, cx));
                })
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(name.clone()))
                        .when(active, |d| d.child(Icon::new(IconName::Check).with_size(px(12.)).text_color(theme.primary))),
                )
                .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(detail.clone()))
        }))
    })
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The focused element must be one this page draws. gpui dispatches a
        // keystroke from the focused node, or from the window root when that
        // node is not in the frame, and the root is above every handler here:
        // with the caret left in a composer the board does not draw, ⌘W and
        // every other shortcut went nowhere.
        if matches!(self.page, Page::Board | Page::Sessions) && self.composer.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.focus_handle, cx);
        }
        let theme = cx.theme().clone();
        let search_open = self.search_open;
        // Below `NARROW_W` the sidebar leaves the row and comes back only as
        // an overlay, the way the Claude app folds its sidebar away when the
        // window gets narrow. `sidebar_open` keeps the person's preference
        // for when the window is wide again.
        self.narrow = window.viewport_size().width < NARROW_W;
        if !self.narrow {
            self.sidebar_peek = false;
        }
        let sidebar_open = self.sidebar_open && !self.narrow;
        let sidebar_peek = self.narrow && self.sidebar_peek;
        // `EMAKI_A11Y=1` prints gpui's own view of the accessibility tree on
        // every draw, for checking what assistive apps are handed.
        if std::env::var("EMAKI_A11Y").is_ok() {
            if let Some(json) = window.debug_a11y_tree_json() {
                eprintln!("--- a11y {}\n{json}", chrono::Local::now().format("%H:%M:%S"));
            } else {
                eprintln!("--- a11y inactive");
            }
        }
        div()
            .id("workbench")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ToggleSearch, window, cx| {
                if this.search_open {
                    this.close_search(window, cx)
                } else {
                    this.open_search(window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &Refresh, _, _| {
                this.hub.refresh();
            }))
            .on_action(cx.listener(|this, _: &NewSession, window, cx| this.show_new(None, window, cx)))
            .on_action(cx.listener(|this, _: &GoBoard, _, cx| {
                this.remember_scroll();
                this.page = Page::Board;
                this.save_ui(true);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| match this.selected.clone() {
                Some(key) if this.page == Page::Session && this.tabs.contains(&key) => this.close_tab(&key, window, cx),
                _ => window.remove_window(),
            }))
            .on_action(cx.listener(|this, _: &GoSessions, _, cx| this.show_sessions(Scope::All, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                if this.narrow {
                    this.sidebar_peek = !this.sidebar_peek;
                } else {
                    this.sidebar_open = !this.sidebar_open;
                    this.save_ui(true);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.settings_open = !this.settings_open;
                if this.settings_open {
                    window.focus(&this.focus_handle, cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FindInPage, window, cx| this.open_find(window, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.find_step(1, cx)))
            .on_action(cx.listener(|this, _: &FindPrev, _, cx| this.find_step(-1, cx)))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.narrow && this.sidebar_peek {
                    this.sidebar_peek = false;
                    cx.notify();
                } else {
                    this.escape(window, cx)
                }
            }))
            .size_full()
            .relative()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(px(13.))
            .child(
                h_flex().size_full().when(sidebar_open, |d| d.child(self.render_sidebar(cx))).map(|this| match self.page {
                    Page::Board => this.child(self.render_board(cx)),
                    Page::New => this.child(self.render_new(cx)),
                    Page::Sessions => this.child(self.render_sessions(cx)),
                    Page::Session => this.child(self.render_detail(window, cx)),
                }),
            )
            .when(sidebar_peek, |d| d.child(self.render_sidebar_overlay(cx)))
            .when(search_open, |d| d.child(self.render_search(cx)))
            .when(self.settings_open, |d| d.child(self.render_settings(cx)))
            .when_some(self.lightbox.clone(), |d, lb| d.child(self.render_lightbox(lb, cx)))
    }
}
