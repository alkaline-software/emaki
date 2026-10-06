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
use std::time::{Duration, Instant};

use chrono::Timelike;
use futures::StreamExt;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, OutdentInline, Textarea, TextareaState};
use gpui_component::popover::Popover;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Disableable as _, Sizable as _};

use crate::ui_state::{Rect, UiState};
use emaki_core::adapters;
use emaki_core::build::Phase;
use emaki_core::config::Config;
use emaki_core::driver::{self, image_block, model_label, acts_alone, slash_token_at, slash_tokens, CommandInfo, PermissionRequest, IMAGE_TYPES};
use emaki_core::find::{FindIndex, Hit};
use emaki_core::limits::{Limits, SessionContext};
use emaki_core::model::{questions_of, AgentId, Item, Session};
use emaki_core::options::{humanize, Choice, Options};
use emaki_core::search::Results;
use emaki_core::transcript::SessionRef;

use crate::format::{bucket, clock, day, elapsed_since, now_secs, plural, relative, today_line};
use crate::hub::{Hub, HubEvent, UpdateEvent};
use emaki_core::update::{self, UpdateState};
use gpui_component::checkbox::Checkbox;

actions!(emaki, [ToggleSearch, Refresh, NewSession, GoBoard, GoSessions, ToggleSidebar, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, Tab9, Escape, Send, CloseTab, OpenSettings, FindInPage, FindNext, FindPrev, TermTab, TermBackTab]);

pub const KEY_CONTEXT: &str = "Workbench";
pub const COMPOSER_CONTEXT: &str = "Composer";
pub const SEARCH_CONTEXT: &str = "SearchPalette";
pub const FIND_CONTEXT: &str = "FindBar";
/// The hidden terminal, while it is drawn: its keys are Claude Code's.
pub const TERMINAL_CONTEXT: &str = "Terminal";

pub const SIDEBAR_W: Pixels = px(268.);
/// What a right click offers: where it was made, and the choices.
#[derive(Clone)]
struct Menu {
    at: Point<Pixels>,
    items: Vec<(&'static str, MenuDo)>,
}

/// One choice on a right-click menu.
#[derive(Clone)]
enum MenuDo {
    /// Open this folder in the file manager; none when it is gone.
    OpenFolder(Option<String>),
    /// Give the session, by its key, a name of the person's own.
    Rename(String),
    /// Show the session's transcript in the file manager.
    Reveal(PathBuf),
}

/// Where the person's own names for sessions are kept, by session key.
fn titles_file() -> PathBuf {
    emaki_core::paths::state_dir().join("titles.json")
}

/// How many sessions an open folder shows in the sidebar before "N more".
const FOLDER_ROWS: usize = 5;
/// How many folders the sidebar lists before "N more".
const SIDE_FOLDERS: usize = 10;
/// How long a folder takes to unfold or fold away.
const FOLDER_ANIM: Duration = Duration::from_millis(200);
/// The sidebar floating in over the content, or back out.
const FLOAT_ANIM: Duration = Duration::from_millis(220);
/// A tab's width when there is room, the least it shrinks to, the gap
/// between two, and how long a width takes to change.
const TAB_MAX: f32 = 200.;
const TAB_MIN: f32 = 56.;
const TAB_GAP: f32 = 4.;
const TAB_ANIM: Duration = Duration::from_millis(220);

/// A tab being dragged along the row, by its key.
#[derive(Clone)]
struct DragTab(String);

/// What follows the pointer while a tab is dragged: its title on a plate.
struct TabGhost(String);

impl Render for TabGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div().h(px(30.)).max_w(px(TAB_MAX)).px(px(10.)).flex().items_center().rounded(px(8.)).bg(theme.muted).border_1().border_color(theme.border).text_size(px(12.5)).text_color(theme.foreground).truncate().opacity(0.9).child(self.0.clone())
    }
}

/// One tab's width on its way from one size to another.
#[derive(Clone, Copy)]
struct TabWidth {
    from: f32,
    to: f32,
    at: Instant,
    serial: u32,
}

impl TabWidth {
    fn now(&self) -> f32 {
        let t = (self.at.elapsed().as_secs_f32() / TAB_ANIM.as_secs_f32()).min(1.);
        self.from + (self.to - self.from) * ease_out_quint()(t)
    }
}

/// The buttons beside the traffic lights: each one's box.
const STRIP_BTN: Pixels = px(26.);
/// The sidebar's rows: an agent or a folder, and a session under a folder.
const SIDE_ROW_H: Pixels = px(26.);
const SIDE_SESSION_H: Pixels = px(24.);
pub const TITLEBAR_H: Pixels = px(48.);
/// Room for the traffic lights on a transparent title bar.
pub const TRAFFIC_W: Pixels = px(85.);
/// The reading column: conversation, composer, the sessions list, the home page.
pub const CONTENT_W: Pixels = px(768.);
/// A board column never gets narrower than this; past that the board scrolls.
pub const COL_MIN_W: Pixels = px(210.);
/// Home-page folder cards to a row.
pub const FOLDER_COLS: usize = 3;
/// The settings panel's sections, in rail order: key, label, icon.
pub const SETTINGS_SECTIONS: &[(&str, &str, &str)] = &[("appearance", "Appearance", "icons/palette.svg"), ("sessions", "New sessions", "icons/square-terminal.svg"), ("explain", "Explanations", "icons/bot.svg"), ("updates", "Updates", "icons/redo-2.svg")];
/// The panel's size: wide enough for a rail beside the rows, and a fixed
/// height so it reads as a sheet, not a second window (capped by the
/// window when that is smaller).
pub const SETTINGS_W: Pixels = px(720.);
pub const SETTINGS_H: Pixels = px(520.);
pub const SETTINGS_RAIL_W: Pixels = px(180.);
/// What the composer says when empty: on the home page, and on a session.
pub const PLACEHOLDER_NEW: &str = "Start a session…  (⌘↩ to send)";
pub const PLACEHOLDER_REPLY: &str = "Reply…  (⌘↩ to send)";
/// While a question of Claude's is waiting, what is typed answers it.
pub const PLACEHOLDER_ANSWER: &str = "Type an answer…  (⌘↩ to send)";
/// How long the window waits to come back from the terminal on its own.
const COME_BACK_SECS: f64 = 15. * 60.;

/// The slash commands offered for a session that has not started yet,
/// where no child has said what it knows: the ones that run headlessly
/// and mean something on a fresh session. A running driver lists its own.
const BUILTIN_COMMANDS: &[(&str, &str, &str)] = &[
    ("compact", "Free up context by summarising the conversation so far", "[instructions]"),
    ("effort", "Set the effort level", "low | medium | high | max"),
    ("context", "Show what is in the context window", ""),
    ("cost", "Show what this session has cost", ""),
    ("status", "Show the session's status", ""),
];
/// How many folders the home page offers.
pub const HOME_FOLDERS: usize = 6;
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

/// The two panes a trackpad gesture can belong to. Momentum events keep
/// arriving after the finger lifts, addressed to wherever the pointer is by
/// then; they belong to the pane the gesture began in. See `route_scroll`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Sidebar,
    Content,
}

/// A wheel event this long after the previous one begins a new gesture: a
/// mouse wheel sends no phases, and momentum comes at frame rate.
const SCROLL_GAP: Duration = Duration::from_millis(150);

/// Why neither the terminal nor the project folder button can do anything.
const FOLDER_GONE: &str = "The session's folder is gone";

/// How long the terminal's last word for what it is doing is kept once
/// its screen stops showing one.
const WORKING_KEPT_SECS: f64 = 3.0;
/// How long a notice stays under the composer.
pub const NOTICE_SECS: f64 = 8.0;

/// How often the clock reads the status line's files, in seconds.
const LIMITS_SECS: f64 = 60.0;

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
    /// Every slash command the child said it knows, for the list that
    /// opens when a message starts with "/".
    pub commands: Vec<CommandInfo>,
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
    /// What the composer's placeholder says now; see `render`.
    composer_placeholder: String,
    /// Where each segment of a settings control sat on the last draw,
    /// relative to its track, by `control-key`, so the raised plate can
    /// slide from the old choice to the new one. See `segmented`.
    seg_bounds: Rc<std::cell::RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// For each control, the choice the plate slides from and the one it
    /// is on, as of the last draw.
    seg_state: std::cell::RefCell<HashMap<&'static str, (&'static str, &'static str)>>,
    pub attachments: Vec<Attachment>,
    /// What was left typed and attached in each composer the person is
    /// not looking at, by `draft_key`: every session has a composer of
    /// its own, and the new-session page has one.
    drafts: HashMap<String, (String, Vec<Attachment>)>,
    /// The sidebar's folders that show their sessions; none until the
    /// person opens or closes one (`folder_open`).
    folders_open: Option<HashSet<String>>,
    /// The folders with a live session as of the last draw, and those
    /// opened for that reason and not touched since (`sync_folders`).
    folders_live: HashSet<String>,
    folders_auto: HashSet<String>,
    /// The right-click menu showing, if one is.
    menu: Option<Menu>,
    /// The session being renamed, by its key, and the field for its name.
    renaming: Option<String>,
    rename_input: Entity<InputState>,
    /// The person's own names for sessions, by session key
    /// (`state/titles.json`); each stands in for the agent's title.
    titles: HashMap<String, String>,
    /// Names still to be given to the sessions themselves, by session
    /// key: each waits for its session to be between turns.
    renames: HashMap<String, String>,
    /// The session showing changed on disk while it was being read; a
    /// change that came then was dropped, and a prompt whose row was
    /// half written at the read (a large pasted picture is one long
    /// line) did not show until the agent's first words, seconds later.
    reload_wanted: bool,
    /// The folder last opened or closed: which, whether it was opened,
    /// when, and the click's number, for the moment its sessions take to
    /// unfold or fold away.
    folder_anim: Option<(String, bool, std::time::Instant, u32)>,
    /// Whose words the one textarea holds now.
    draft_of: Option<String>,
    /// An attachment opened large over the window.
    pub lightbox: Option<Lightbox>,
    pub search_input: Entity<InputState>,
    pub search_open: bool,
    pub settings_open: bool,
    /// Which section of the settings panel is showing; see `SETTINGS_SECTIONS`.
    pub settings_section: &'static str,
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
    /// Options picked on a question card that has not been sent yet, by
    /// request id, then by question: a card with several questions, or a
    /// question allowing several answers, is sent by its button.
    pub picks: HashMap<String, HashMap<String, Vec<String>>>,
    /// A session the person was sent to the terminal for, and what will
    /// say the interaction there is done, which brings the window back to
    /// the front.
    pub come_back: Option<ComeBack>,
    /// The mode a terminal session was left in after the person changed
    /// it there: the session, the mode as its screen showed it (none when
    /// the terminal cannot be read), and the transcript's time then. The
    /// transcript only learns the mode with its next prompt, so this is
    /// what the pill shows until a turn starts.
    mode_seen: Option<(String, Option<String>, f64)>,
    /// A terminal's screen is being read for its mode, and the session to
    /// read again once that is back, when its status line ran meanwhile.
    mode_reading: bool,
    mode_read_again: Option<String>,
    /// What a terminal session's screen says it is doing, and when that
    /// was read (`read_working`); and whether a read is out now.
    working_seen: Option<(String, emaki_core::driver::Working, f64)>,
    working_reading: bool,
    /// The dialog a terminal session is held on, read off its screen
    /// (`read_dialog`), whether a read is out now, and whether keys sent
    /// from here are still on their way.
    /// The prompt a terminal session's input suggests, read off its
    /// screen (`read_suggestion`), and whether a read is out now.
    suggestion: Option<(String, String)>,
    suggestion_reading: bool,
    dialog_seen: Option<(String, emaki_core::driver::Dialog)>,
    dialog_reading: bool,
    dialog_sending: bool,
    /// `EMAKI_GO=dialog:<keys>`, `answer:<words>` or `goto:<tab>`, done once on the
    /// first dialog read, for a check from a script.
    dialog_probe: Option<String>,
    /// What a terminal is being opened for: the session, the action, and
    /// when the opening began (`via_terminal`, `terminal_ready`).
    pending_terminal: Option<(String, TerminalAction, f64)>,
    /// The session whose freshly opened terminal has its prompt on the
    /// screen, and whether that screen is being read now.
    terminal_up: Option<String>,
    terminal_probing: bool,
    /// The session whose hidden terminal is drawn over the composer, for
    /// something only its own interface can take: the `/model` list, the
    /// `/effort` slider, a screen no card stands for.
    term_open: Option<String>,
    /// It was put there by the window, for a screen it could not read,
    /// and goes when the terminal stops waiting.
    term_auto: bool,
    /// The person put it away while the terminal still waits: it is not
    /// brought back until that wait is over.
    term_dismissed: Option<String>,
    term_focus: FocusHandle,
    /// The focus is owed to the terminal card, or back to the composer.
    term_focus_due: Option<bool>,
    /// A screen change is waiting to be read (`on_screen`).
    screen_due: bool,
    /// `EMAKI_GO`, done once the session it names is open.
    go_probe: Option<String>,
    /// `EMAKI_GO=send:<words>`: sent once the session is open.
    send_probe: Option<String>,
    /// `EMAKI_TERM_KEYS=right,enter`: pressed in the terminal card a
    /// moment after it first shows.
    term_keys_probe: Option<String>,
    /// The terminal session ⇧Tab was last sent to from here, until its
    /// screen has been read back.
    mode_pressed: Option<String>,
    /// The session whose mode was changed since its last turn, from here
    /// or in its terminal, whatever it is now.
    mode_touched: Option<(String, Vec<(String, f64)>)>,
    /// Which row of the slash-command list the keys are on, counted over
    /// every match; back to the first whenever the typed text changes.
    pub slash_sel: usize,
    /// Escape put the list away; typing brings it back.
    pub slash_closed: bool,
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
    /// What the terminal's status line last said of the session showing:
    /// its id and its context window. Read on every tick and every load.
    session_ctx: Option<(String, SessionContext)>,
    /// When the status line's files were last read on the clock.
    limits_read: f64,
    /// Until when the showing session's status-line file is read every
    /// second: a mode, model or effort was just changed in its terminal.
    ctx_watch_until: f64,
    /// The last message sent from here, with the tab it went to and what
    /// was attached, so a stopped turn can hand it back whole.
    last_sent: Option<(String, String, Vec<Attachment>)>,
    /// The session that was showing and working at the last index, to
    /// notice the moment it is stopped.
    was_working: Option<String>,
    /// A turn was just stopped: put its prompt back in the composer at
    /// the next draw, which is where a window is at hand.
    restore_due: bool,
    /// The update check and install, as the settings panel shows them.
    pub update: UpdateView,
    /// A line for the person at the right end of the row under the
    /// composer: what a send or an action did, or why it did not. It fades
    /// after `NOTICE_SECS`; the row's space stays, so nothing moves when it
    /// comes and goes.
    pub notice: Option<Notice>,
    /// The copy button that has just copied, by its id, so it can show a
    /// tick for a moment.
    pub copied: Option<SharedString>,
    pub now: f64,
    /// Who to greet on the home page.
    pub user_name: String,
    /// Sessions with a tab, in tab order. `selected` is the one showing.
    pub tabs: Vec<String>,
    /// Where each tab was scrolled when the reader left it, restored when
    /// the tab is opened again, here or on the next launch.
    window_rect: Option<Rect>,
    last_ui_save: std::time::Instant,
    /// The window is too narrow for the sidebar beside the content; it is
    /// hidden and only shows over the content, on request (`sidebar_peek`).
    narrow: bool,
    /// The content pane's width as of the last draw, for layouts that
    /// choose their column count from it (the board).
    pane_w: Pixels,
    /// The pane the current scroll gesture began in, and when its last event
    /// came; see `route_scroll`.
    scroll_owner: Option<Pane>,
    last_scroll: Instant,
    /// The scroll positions momentum may be forwarded to.
    side_scroll: ScrollHandle,
    sessions_scroll: ScrollHandle,
    home_scroll: ScrollHandle,
    /// The settings body's scroll position, for its scrollbar.
    settings_scroll: ScrollHandle,
    sidebar_peek: bool,
    /// The sidebar is folded away and showing over the content because the
    /// pointer is on its button, until the pointer leaves it.
    sidebar_float: bool,
    /// The click that folded the sidebar away left the pointer on the
    /// button: that is not a hover, and floats nothing until it has left.
    float_block: bool,
    /// The float last coming or going: which, when, and its number.
    float_anim: Option<(bool, Instant, u32)>,
    /// The tab row's width as last laid out, each tab's width (see
    /// `sync_tab_widths`), and how many tabs those were worked out for.
    tabs_row_w: Rc<std::cell::Cell<f32>>,
    tab_widths: HashMap<String, TabWidth>,
    tabs_n: usize,
    /// The tab being dragged along the row.
    drag_tab: Option<String>,
    /// A press on the top strip that may become a drag of the window, and
    /// a press something on the strip (a tab, a button) took for itself;
    /// see `drag_region`.
    win_move: bool,
    press_taken: bool,
    /// The conversations of the other open tabs, kept as they were left
    /// so that going back to one draws it at once; see `open_session`.
    stashed: HashMap<String, Detail>,
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

    /// The sidebar is beside the content, or over it because it was asked
    /// for with a click or the key (not only floating under the pointer).
    fn sidebar_pinned(&self) -> bool {
        if self.narrow { self.sidebar_peek } else { self.sidebar_open }
    }

    /// The sidebar button, or its key. On a floating sidebar it keeps the
    /// sidebar; otherwise it shows or hides it.
    fn toggle_sidebar(&mut self, clicked: bool, cx: &mut Context<Self>) {
        if self.sidebar_float {
            self.sidebar_float = false;
            self.float_anim = None;
            self.show_sidebar();
        } else if self.sidebar_pinned() {
            self.hide_sidebar();
            self.float_block = clicked;
        } else {
            self.show_sidebar();
        }
        cx.notify();
    }

    /// Float the folded sidebar in over the content, or let it go.
    fn set_float(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.sidebar_float == on || (on && self.sidebar_pinned()) {
            return;
        }
        self.sidebar_float = on;
        let serial = self.float_anim.map(|(_, _, n)| n + 1).unwrap_or(0);
        self.float_anim = Some((on, Instant::now(), serial));
        if !on {
            // Drawn until it has slid out, then dropped.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FLOAT_ANIM).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        cx.notify();
    }

    /// The pointer moved while the sidebar floats: off the sidebar, it goes.
    fn float_follow(&mut self, e: &MouseMoveEvent, window: &Window, cx: &mut Context<Self>) {
        if !self.sidebar_float {
            return;
        }
        let view = window.viewport_size();
        let p = e.position;
        if p.x < px(0.) || p.x > SIDEBAR_W || p.y < px(0.) || p.y > view.height {
            self.set_float(false, cx);
        }
    }

    /// The floating sidebar: no scrim, the content stays as it is, and it
    /// slides in from the edge. `None` once it is neither up nor on its
    /// way out.
    fn render_sidebar_float(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (opening, at, serial) = self.float_anim?;
        if self.sidebar_pinned() || (!self.sidebar_float && at.elapsed() >= FLOAT_ANIM) {
            return None;
        }
        let dark = cx.theme().mode.is_dark();
        let shadow = vec![BoxShadow { color: gpui::black().opacity(if dark { 0.5 } else { 0.14 }), offset: point(px(6.), px(0.)), blur_radius: px(28.), spread_radius: px(-6.), inset: false }];
        Some(
            div()
                .id("sidebar-float")
                .absolute()
                .top_0()
                .h_full()
                .w(SIDEBAR_W)
                .occlude()
                .shadow(shadow)
                .child(self.render_sidebar(cx))
                .with_animation(ElementId::Name(format!("sidebar-float-{serial}").into()), Animation::new(FLOAT_ANIM).with_easing(ease_out_quint()), move |d, t| {
                    let shown = if opening { t } else { 1. - t };
                    d.left(SIDEBAR_W * (shown - 1.)).opacity(0.4 + 0.6 * shown)
                })
                .into_any_element(),
        )
    }

    /// Where the strip of buttons starts: past the traffic lights on
    /// macOS, at the edge elsewhere.
    fn strip_left() -> Pixels {
        if cfg!(target_os = "macos") { TRAFFIC_W + px(2.) } else { px(10.) }
    }

    /// Where the strip ends: the sidebar button and search.
    fn strip_right() -> Pixels {
        Self::strip_left() + STRIP_BTN * 2.
    }

    /// The buttons at the window's top left, over the sidebar when it is
    /// there and over the top strip when it is not, so the sidebar button
    /// never moves: the sidebar and search.
    fn render_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let button = |id: &'static str, icon: IconName, tip: &'static str, lit: bool| {
            let hover = theme.foreground.opacity(0.07);
            div()
                .id(id)
                .size(STRIP_BTN)
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .when(lit, |d| d.bg(hover))
                .hover(move |s| s.bg(hover))
                .when(!tip.is_empty(), |d| d.tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip).build(window, cx)))
                .child(Icon::new(icon).with_size(px(16.)).text_color(theme.muted_foreground))
        };
        h_flex()
            .absolute()
            // A pixel down: the lights' centres are at 24.75, measured.
            .top(px(1.))
            .left(Self::strip_left())
            .h(TITLEBAR_H)
            .items_center()
            // No tooltip on this one: it would sit on the sidebar it floats.
            .child(
                button("strip-sidebar", IconName::PanelLeft, "", self.sidebar_float)
                    .on_hover(cx.listener(|this, over: &bool, _, cx| {
                        if !*over {
                            this.float_block = false;
                        } else if !this.float_block {
                            this.set_float(true, cx);
                        }
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.toggle_sidebar(true, cx);
                    })),
            )
            .child(button("strip-search", IconName::Search, "Search (⌘K)", false).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.open_search(window, cx);
            })))
    }

    /// Empty space along the top of the window moves the window. The app
    /// owns that drag (`app_owns_titlebar_drag`): left to AppKit, the whole
    /// strip under the title bar moved the window, tabs included, so a tab
    /// could not be dragged along the row. A press here becomes a move
    /// once the pointer moves with the button down, unless something on
    /// the strip took the press first (`press_taken`: a child's listener
    /// runs before its parent's), and a double click is the title bar's.
    fn drag_region(el: Div, cx: &mut Context<Self>) -> Div {
        el.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, window, _| {
                this.win_move = !std::mem::take(&mut this.press_taken);
                if this.win_move && ev.click_count == 2 {
                    this.win_move = false;
                    window.titlebar_double_click();
                }
            }),
        )
        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, _| {
            if !this.win_move {
                return;
            }
            this.win_move = false;
            if ev.pressed_button == Some(MouseButton::Left) {
                window.start_window_move();
            }
        }))
    }

    /// Give every tab its width. They share the row equally, `TAB_MAX`
    /// each while there is room and less once there is not, and a change
    /// in how many there are is a move, not a jump: a new tab grows in
    /// from nothing while the others give way, and a closed one's room is
    /// taken up the same way. A change in the row's own width (the window
    /// resized, the sidebar folded) is followed at once. Left to the flex
    /// row, a new tab was drawn at full width for a frame and the whole
    /// row then snapped narrower.
    fn sync_tab_widths(&mut self, cx: &mut Context<Self>) {
        if !cx.has_active_drag() {
            self.drag_tab = None;
        }
        let n = self.tabs.len();
        let first = self.tab_widths.is_empty();
        let tabs = &self.tabs;
        self.tab_widths.retain(|k, _| tabs.contains(k));
        if n == 0 {
            self.tabs_n = 0;
            return;
        }
        let row = self.tabs_row_w.get();
        let target = if row <= 0. { TAB_MAX } else { ((row - TAB_GAP * (n as f32 - 1.)) / n as f32).clamp(TAB_MIN, TAB_MAX) };
        let moved = n != self.tabs_n && !first;
        self.tabs_n = n;
        let now = Instant::now();
        for key in self.tabs.iter() {
            match self.tab_widths.get_mut(key) {
                None => {
                    self.tab_widths.insert(key.clone(), TabWidth { from: if moved { 0. } else { target }, to: target, at: now, serial: 0 });
                }
                Some(w) if (w.to - target).abs() > 0.5 => {
                    let running = w.at.elapsed() < TAB_ANIM;
                    w.from = if moved || running { w.now() } else { target };
                    w.to = target;
                    w.at = now;
                    w.serial += 1;
                }
                _ => {}
            }
        }
    }

    /// A tab dragged along the row takes the place the pointer is over.
    fn drag_tab_to(&mut self, key: &str, x: Pixels, row: Bounds<Pixels>, cx: &mut Context<Self>) {
        let n = self.tabs.len();
        let Some(from) = self.tabs.iter().position(|t| t == key) else { return };
        let w = self.tab_widths.get(key).map(|w| w.to).unwrap_or(TAB_MAX);
        let total = w * n as f32 + TAB_GAP * (n as f32 - 1.);
        let start = f32::from(row.left()) + ((f32::from(row.size.width) - total) / 2.).max(0.);
        let to = (((f32::from(x) - start) / (w + TAB_GAP)).floor().max(0.) as usize).min(n - 1);
        if self.drag_tab.as_deref() != Some(key) {
            self.drag_tab = Some(key.to_string());
            cx.notify();
        }
        if to != from {
            let tab = self.tabs.remove(from);
            self.tabs.insert(to, tab);
            self.save_ui(true);
            cx.notify();
        }
    }

    /// ⌘1 to ⌘9: that tab, or the last one when there are fewer.
    fn go_tab(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.tabs.get(n - 1).or(self.tabs.last()).cloned() else { return };
        self.open_and_focus(&key, window, cx);
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

/// What to call a mode: the agent's name for it, else its key made
/// readable.
pub(crate) fn mode_name(options: &Options, key: &str) -> String {
    options.mode(key).map(|m| m.label.clone()).unwrap_or_else(|| humanize(key))
}

/// What to call a model, however the session names it: the agent's name
/// for that key or id, else the id read for a family and a version.
pub(crate) fn model_name(options: &Options, name: &str) -> String {
    if let Some(m) = options.model(name) {
        return m.label.clone();
    }
    if name.is_empty() {
        if let Some(m) = options.model(&options.default_model) {
            return m.label.clone();
        }
    }
    match name.strip_suffix("[1m]").and_then(|base| options.model(base)) {
        Some(m) => format!("{} 1M", m.label),
        None => model_label(name),
    }
}

/// The colour of a choice's name: none, one, or several run through
/// its letters.
#[derive(Clone)]
pub(crate) enum Tint {
    Plain,
    Solid(Hsla),
    Spectrum(Vec<Hsla>),
}

/// A choice's name in bold, in its colour.
pub(crate) fn tinted(value: &str, tint: &Tint) -> Div {
    let bold = div().font_weight(FontWeight::SEMIBOLD);
    match tint {
        Tint::Plain => bold.child(value.to_string()),
        Tint::Solid(c) => bold.text_color(*c).child(value.to_string()),
        Tint::Spectrum(colors) => {
            let n = value.chars().count();
            let at = |i: usize| colors[if n > 1 { i * (colors.len() - 1) / (n - 1) } else { 0 }];
            bold.flex().children(value.chars().enumerate().map(|(i, c)| div().text_color(at(i)).child(c.to_string())))
        }
    }
}

/// What a pill says: the choice in bold, in its colour when it has one,
/// and a plain word after it ("**Plan** mode", "**High** effort").
#[derive(Clone)]
pub(crate) struct PillText {
    value: String,
    tint: Tint,
    tail: &'static str,
}

impl PillText {
    fn plain(value: String) -> Self {
        PillText { value, tint: Tint::Plain, tail: "" }
    }
}

/// The colours of a choice with none of its own, from low to high,
/// each on a light ground and then on a dark one: blue, green, amber,
/// purple, red, in the shades Claude Code's two themes use, so a choice
/// it has no colour for sits beside the ones it has.
const RAMP: [[u32; 2]; 5] = [[0x3A68B0, 0x8FB2EA], [0x2C7A39, 0x4EBA65], [0x966C1E, 0xFFC107], [0x8700FF, 0xAF87FF], [0xAB2B3F, 0xFF6B80]];

/// The rainbow on a light ground: Claude Code's seven colours are
/// pastels made for a dark one, and its yellow and green all but
/// vanish on cream, so each is taken down to a shade that reads there.
const SPECTRUM_LIGHT: [u32; 7] = [0xC8372D, 0xC8611F, 0x9A7411, 0x3F8A3A, 0x356FB5, 0x6A4FB0, 0xA8418A];

/// One of a light and dark pair, by the window's appearance.
fn shade(pair: [u32; 2], theme: &gpui_component::Theme) -> Hsla {
    rgb(if theme.mode.is_dark() { pair[1] } else { pair[0] }).into()
}

/// The colour of a choice with none of its own, by its place on a list
/// of `len`.
fn ramp(ix: usize, len: usize, theme: &gpui_component::Theme) -> Hsla {
    let t = if len > 1 { ix as f32 / (len - 1) as f32 } else { 0. };
    shade(RAMP[(t * (RAMP.len() - 1) as f32).round() as usize], theme)
}

/// The colour of the choice at `ix` of `list`: the agent's own when it
/// shows that choice in one (or in several), else one by its place.
fn tint_of(list: &[Choice], ix: usize, theme: &gpui_component::Theme) -> Tint {
    let c = &list[ix];
    if !c.spectrum.is_empty() {
        // The agent's rainbow as it lists it on a dark ground, ours
        // for a light one when it is the seven colours we know.
        if !theme.mode.is_dark() && c.spectrum.len() == SPECTRUM_LIGHT.len() {
            return Tint::Spectrum(SPECTRUM_LIGHT.iter().map(|v| rgb(*v).into()).collect());
        }
        return Tint::Spectrum(c.spectrum.iter().map(|v| rgb(*v).into()).collect());
    }
    Tint::Solid(match c.color {
        Some(pair) => shade(pair, theme),
        None => ramp(ix, list.len(), theme),
    })
}

/// The colour of a mode, by its key or its name: the agent's own for it
/// (Claude Code names each mode in a colour at the foot of its prompt),
/// else one by its place on the list.
pub(crate) fn mode_color(options: &Options, mode: &str, theme: &gpui_component::Theme) -> Tint {
    match options.modes.iter().position(|m| m.key == mode || m.label == mode) {
        Some(ix) => tint_of(&options.modes, ix, theme),
        None => Tint::Plain,
    }
}

/// The colour of an effort level, by its key or its name: the agent's
/// own for it (Claude Code's `/effort` slider has one per level), else
/// where it sits between the lowest and the highest.
pub(crate) fn effort_color(options: &Options, model: &str, effort: &str, theme: &gpui_component::Theme) -> Tint {
    let on = |list: &[Choice]| list.iter().position(|e| e.key == effort || e.label == effort).map(|ix| tint_of(list, ix, theme));
    on(options.efforts_for(model)).or_else(|| on(&options.efforts)).or_else(|| options.models.iter().find_map(|m| on(&m.efforts))).unwrap_or(Tint::Plain)
}

/// What a mode's pill says: its name, and "mode" after a name of one
/// word, so "Plan" reads as "Plan mode" beside an effort and a model.
fn mode_pill(options: &Options, mode: &str, theme: &gpui_component::Theme) -> PillText {
    let value = mode_name(options, mode);
    let tail = if value.contains(' ') { "" } else { "mode" };
    PillText { value, tint: mode_color(options, mode, theme), tail }
}

/// What the effort pill says; an empty level is the session's default.
fn effort_pill(options: &Options, model: &str, effort: &str, theme: &gpui_component::Theme) -> PillText {
    if effort.is_empty() {
        return PillText { value: "Default".into(), tint: Tint::Plain, tail: "effort" };
    }
    PillText { value: options.effort_label(model, effort), tint: effort_color(options, model, effort, theme), tail: "effort" }
}

impl Workbench {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = Config::load();
        let (hub, mut rx) = Hub::start(cfg.clone());

        let composer = cx.new(|cx| TextareaState::new(window, cx).placeholder(PLACEHOLDER_NEW).auto_grow(COMPOSER_MIN_ROWS, COMPOSER_MAX_ROWS));
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
                    // The status line's files once a minute, which is the
                    // countdown's own resolution; a session's load reads
                    // them too, and that is when they change.
                    if this.now - this.limits_read >= LIMITS_SECS {
                        this.limits_read = this.now;
                        this.limits.refresh_from_statusline();
                        if let Some(id) = this.shown_session().map(|s| s.id.clone()) {
                            this.refresh_context(&id);
                        }
                        cx.notify();
                    }
                    this.hub.show(this.selected_ref().filter(|_| this.page == Page::Session).map(|r| r.session_id.clone()));
                    this.terminal_lost(cx);
                    this.watch_terminal(cx);
                    this.terminal_ready(cx);
                    this.read_working(cx);
                    this.read_dialog(cx);
                    this.read_suggestion(cx);
                    if this.now < this.ctx_watch_until {
                        if let Some(id) = this.shown_session().map(|s| s.id.clone()) {
                            if this.refresh_context(&id) {
                                cx.notify();
                            }
                        }
                    }
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

        let titles: HashMap<String, String> = emaki_core::paths::read_json(&titles_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("A name for this session"));
        cx.subscribe_in(&rename_input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.commit_rename(window, cx);
            }
        })
        .detach();
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
            if let InputEvent::Change = ev {
                this.slash_sel = 0;
                this.slash_closed = false;
                this.mark_slash(cx);
                this.warm_terminal(cx);
            }
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
        // `EMAKI_FIND=<text>` opens the find bar on that query,
        // `EMAKI_SETTINGS=1` opens the settings panel, `EMAKI_TYPE=<text>`
        // puts that text in the composer, and `EMAKI_QUESTION=1` holds a
        // sample question of Claude's on the session opened, as a driver
        // would (nothing answers it).
        let ui = UiState::load();
        let startup_open = std::env::var("EMAKI_OPEN").ok().filter(|s| !s.is_empty());
        let startup_find = std::env::var("EMAKI_FIND").ok().filter(|s| !s.is_empty());
        if let Some(q) = &startup_find {
            find_input.update(cx, |s, cx| s.set_value(q.clone(), window, cx));
        }
        if let Some(t) = std::env::var("EMAKI_TYPE").ok().filter(|s| !s.is_empty()) {
            // With the caret after it, as typing would leave it.
            let end = gpui_component::input::Position::new(t.matches('\n').count() as u32, t.rsplit('\n').next().unwrap_or("").encode_utf16().count() as u32);
            composer.update(cx, |s, cx| {
                s.set_value(t, window, cx);
                s.set_cursor_position(end, window, cx);
            });
        }
        // `EMAKI_KEYS=down,down,tab` presses those keys in the composer a
        // few seconds in (up, down, tab, esc: the slash list's keys, less
        // the one that runs a command; shift-up and shift-down, which
        // extend the selection), as actions through the focus.
        if let Some(keys) = std::env::var("EMAKI_KEYS").ok().filter(|s| !s.is_empty()) {
            let composer = composer.clone();
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(Duration::from_secs(4)).await;
                let _ = cx.update(|window, cx| composer.update(cx, |s, cx| s.focus(window, cx)));
                for key in keys.split(',') {
                    cx.background_executor().timer(Duration::from_millis(200)).await;
                    let _ = cx.update(|window, cx| match key {
                        "up" => window.dispatch_action(Box::new(gpui_component::input::MoveUp), cx),
                        "down" => window.dispatch_action(Box::new(gpui_component::input::MoveDown), cx),
                        "tab" => window.dispatch_action(Box::new(gpui_component::input::IndentInline), cx),
                        "esc" => window.dispatch_action(Box::new(gpui_component::input::Escape), cx),
                        "shift-up" => window.dispatch_action(Box::new(gpui_base::actions::SelectUp), cx),
                        "shift-down" => window.dispatch_action(Box::new(gpui_base::actions::SelectDown), cx),
                        _ => {}
                    });
                }
            })
            .detach();
        }
        // `EMAKI_SHOT=<file.png>` writes a picture of the window there a
        // few seconds in (`EMAKI_SHOT_AFTER`, five by default) and quits:
        // with the variables above, any state is one launch away from
        // being looked at, and the app needs no access to capture itself.
        if let Some(path) = std::env::var_os("EMAKI_SHOT").filter(|p| !p.is_empty()) {
            let after = std::env::var("EMAKI_SHOT_AFTER").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(5.0);
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor().timer(Duration::from_secs_f64(after)).await;
                let _ = cx.update(|window, cx| {
                    if let Err(e) = crate::sys::shoot_window(window, std::path::Path::new(&path)) {
                        eprintln!("emaki: EMAKI_SHOT: {e}");
                    }
                    cx.quit();
                });
            })
            .detach();
        }
        let settings_open = std::env::var("EMAKI_SETTINGS").is_ok();
        // `EMAKI_SETTINGS=<section>` opens the panel on that section.
        let settings_section = std::env::var("EMAKI_SETTINGS").ok().and_then(|v| SETTINGS_SECTIONS.iter().find(|(k, _, _)| *k == v).map(|(k, _, _)| *k)).unwrap_or("appearance");
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
            drafts: HashMap::new(),
            folders_open: ui.folders_open.clone().map(|f| f.into_iter().collect()),
            folder_anim: None,
            folders_live: HashSet::new(),
            folders_auto: HashSet::new(),
            menu: None,
            renaming: None,
            rename_input,
            titles: titles.clone(),
            // A name still in our record is one its transcript does not
            // say yet: asked for again at launch.
            renames: titles,
            reload_wanted: false,
            draft_of: None,
            lightbox: None,
            search_input,
            search_open: false,
            settings_open,
            settings_section,
            search_results: None,
            search_task: None,
            find_input,
            find_open: startup_find.is_some(),
            find_hits: Vec::new(),
            find_at: 0,
            find_pending: startup_find.map(|_| 0),
            done_open: false,
            permissions: Vec::new(),
            picks: HashMap::new(),
            come_back: None,
            mode_seen: None,
            mode_reading: false,
            working_seen: None,
            working_reading: false,
            suggestion: None,
            suggestion_reading: false,
            dialog_seen: None,
            dialog_reading: false,
            dialog_sending: false,
            dialog_probe: std::env::var("EMAKI_GO").ok().filter(|g| g.starts_with("dialog:") || g.starts_with("answer:") || g.starts_with("goto:")),
            pending_terminal: None,
            terminal_up: None,
            terminal_probing: false,
            term_open: None,
            term_auto: false,
            term_dismissed: None,
            term_focus: cx.focus_handle(),
            term_focus_due: None,
            screen_due: false,
            go_probe: std::env::var("EMAKI_GO").ok(),
            send_probe: std::env::var("EMAKI_GO").ok().and_then(|g| g.strip_prefix("send:").map(str::to_string)),
            term_keys_probe: std::env::var("EMAKI_TERM_KEYS").ok(),
            mode_read_again: None,
            mode_pressed: None,
            mode_touched: None,
            slash_sel: 0,
            slash_closed: false,
            drivers: HashMap::new(),
            explanations: HashMap::new(),
            explaining: HashSet::new(),
            copied: None,
            new_cwd: String::new(),
            new_id: String::new(),
            next_mode,
            next_model,
            limits: {
                let mut l = Limits::load();
                l.refresh_from_statusline();
                // A month without a status line is a session that ended.
                emaki_core::limits::prune_contexts(now_secs(), 30.0 * 86_400.0);
                l
            },
            session_ctx: None,
            limits_read: now_secs(),
            ctx_watch_until: 0.0,
            last_sent: None,
            was_working: None,
            restore_due: false,
            update: {
                let s = UpdateState::load();
                UpdateView { last_check: s.last_check, latest: s.latest, ..Default::default() }
            },
            notice: None,
            now: now_secs(),
            user_name: crate::sys::user_first_name(),
            tabs: ui.tabs.clone(),
            window_rect: Some(rect_of(window.bounds())),
            last_ui_save: std::time::Instant::now(),
            narrow: false,
            pane_w: px(1180.),
            composer_placeholder: PLACEHOLDER_NEW.to_string(),
            settings_scroll: ScrollHandle::new(),
            seg_bounds: Rc::new(std::cell::RefCell::new(HashMap::new())),
            seg_state: std::cell::RefCell::new(HashMap::new()),
            scroll_owner: None,
            last_scroll: Instant::now(),
            side_scroll: ScrollHandle::new(),
            sessions_scroll: ScrollHandle::new(),
            home_scroll: ScrollHandle::new(),
            sidebar_peek: false,
            sidebar_float: false,
            float_block: false,
            float_anim: None,
            stashed: HashMap::new(),
            tabs_row_w: Rc::new(std::cell::Cell::new(0.)),
            tab_widths: HashMap::new(),
            tabs_n: 0,
            drag_tab: None,
            win_move: false,
            press_taken: false,
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
            HubEvent::Index(mut refs) => {
                // A session nothing was said in (what `/clear` leaves
                // behind) is listed nowhere, unless the person has it
                // showing or it is one of ours getting under way.
                let shown = self.selected.clone();
                let mut dropped: HashSet<String> = HashSet::new();
                refs.retain(|r| {
                    let keep = !r.blank
                        || shown.as_deref() == Some(key_of(r).as_str())
                        || self.driven(&r.session_id)
                        || self.hub.terminal_for(&r.session_id).is_some();
                    if !keep {
                        dropped.insert(key_of(r));
                    }
                    keep
                });
                let tabs = self.tabs.len();
                self.tabs.retain(|t| !dropped.contains(t));
                if self.tabs.len() != tabs {
                    self.save_ui(true);
                }
                // The person's own name for a session stands in for the
                // agent's title wherever the list is drawn.
                // Once the transcript says the same, the name is the
                // session's own and ours is put away.
                let mut settled = false;
                for r in refs.iter_mut() {
                    let key = key_of(r);
                    match self.titles.get(&key) {
                        Some(name) if *name == r.title => {
                            self.titles.remove(&key);
                            settled = true;
                        }
                        Some(name) => r.title = name.clone(),
                        None => {}
                    }
                }
                if settled {
                    self.save_titles();
                }
                self.refs = refs;
                self.try_renames();
                // The turn showing was working and now reads as stopped
                // (Escape in the terminal, or Stop here): its prompt goes
                // back to the composer.
                let shown = self.selected_ref().filter(|_| self.page == Page::Session).cloned();
                let working = shown.as_ref().filter(|r| self.is_working(r)).map(|r| r.session_id.clone());
                if let (Some(was), Some(r)) = (&self.was_working, &shown) {
                    if *was == r.session_id && working.is_none() && r.state.activity_kind == "stop" {
                        self.restore_due = true;
                        cx.notify();
                    }
                }
                self.was_working = working;
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
                // Sent to the terminal for a question, an approval or a
                // command: the transcript moving is that done, and the
                // window comes back. Forgotten after a while unanswered.
                if let Some(cb) = &self.come_back {
                    let moved = cb.mtime.is_some_and(|then| self.refs.iter().any(|r| r.session_id == cb.sid && r.mtime > then));
                    if moved {
                        self.back_from_terminal(cx);
                    } else if self.now - cb.at > COME_BACK_SECS {
                        self.come_back = None;
                    }
                }
                // A turn that began after the mode was changed in the
                // terminal wrote the mode with its prompt.
                if let Some((sid, _, then)) = &self.mode_seen {
                    if self.refs.iter().any(|r| r.session_id == *sid && r.mtime > *then && self.is_working(r)) {
                        if self.mode_touched.as_ref().is_some_and(|(s, _)| s == sid) {
                            self.mode_touched = None;
                        }
                        self.mode_seen = None;
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
                    if same && self.loading.is_some() {
                        // A read is under way and may have been taken
                        // before this change was whole: read once more
                        // when it is back.
                        self.reload_wanted = true;
                    } else if same {
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
            // A terminal's status line ran: what it says of the session
            // showing is read at once, and so is the mode on its screen,
            // which is what a ⇧Tab there changed.
            HubEvent::Context(sid) => self.terminal_changed(&sid, cx),
            HubEvent::Screen(sid) => self.on_screen(sid, cx),
            // A message waits on a hidden terminal that is not at its
            // prompt: the person has to see what it shows.
            HubEvent::TerminalNeeded(sid) => {
                if self.selected_ref().is_some_and(|r| self.page == Page::Session && r.session_id == sid) && self.dialog_seen.is_none() {
                    self.show_terminal(&sid, true, cx);
                }
            }
            HubEvent::Commands => {
                self.mark_slash(cx);
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
                if !caps.commands.is_empty() {
                    view.commands = caps.commands;
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
        if !self.tabs.iter().any(|t| t == key) {
            self.tabs.push(key.to_string());
        }
        self.page = Page::Session;
        self.selected = Some(key.to_string());
        self.sidebar_peek = false;
        self.save_ui(true);
        if let Some(r) = self.refs.iter().find(|r| key_of(r) == key).cloned() {
            // Another tab: the conversation left is put away as it is
            // (where it was scrolled, what was unfolded) and the one
            // arrived at is taken out again and drawn at once, as a
            // browser's tab is; the load below then brings it up to date
            // in place. Only a tab never shown before has to wait for its
            // load. It was dropped and read again at every switch, and the
            // pane drew "loading…" in between, a blink at each click.
            if self.detail.as_ref().map(|d| d.key != key).unwrap_or(true) {
                if let Some(left) = self.detail.take() {
                    self.stashed.insert(left.key.clone(), left);
                }
                self.detail = self.stashed.remove(key);
                let tabs = &self.tabs;
                self.stashed.retain(|k, _| tabs.contains(k));
            }
            if std::env::var_os("EMAKI_QUESTION").is_some() && self.permissions.is_empty() {
                self.permissions.push((r.session_id.clone(), sample_question()));
            }
            if self.term_open.as_deref() != Some(r.session_id.as_str()) {
                self.term_open = None;
            }
            self.hub.show(Some(r.session_id.clone()));
            self.read_terminal_mode(&r.session_id, cx);
            self.read_dialog(cx);
            self.load_detail(r, cx);
            // `EMAKI_GO=terminal` presses the go-to-terminal action once the
            // session is open, `EMAKI_GO=type:<text>` types that into its
            // terminal, and `EMAKI_GO=pill:mode` (or `model`, `effort`)
            // clicks that pill, for a check from a script.
            // Several, with `;` between, are done five seconds apart.
            if let Some(go) = self.go_probe.take() {
                for (ix, step) in go.split(';').map(str::to_string).enumerate() {
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(5 * ix as u64)).await;
                        let _ = this.update(cx, |this, cx| this.probe_go(&step, cx));
                    })
                    .detach();
                }
            }
        } else {
            self.pending_select = Some(key.to_string());
        }
        cx.notify();
    }

    /// One step of `EMAKI_GO`, on the session showing.
    fn probe_go(&mut self, step: &str, cx: &mut Context<Self>) {
        match Some(step) {
            Some("terminal") => self.go_to_terminal(cx),
            Some("button:terminal") => self.open_in_terminal(cx),
            Some(t) if t.starts_with("type:") => self.run_in_terminal(t["type:".len()..].to_string(), cx),
            Some("pill:mode") => self.cycle_mode(cx),
            Some("pill:model") => self.via_terminal(TerminalAction::Pick(Pill::Model), cx),
            Some("pill:effort") => self.via_terminal(TerminalAction::Pick(Pill::Effort), cx),
            Some("step:mode") => self.cycle_mode(cx),
            // A click on a folder in the sidebar, by its name.
            Some(t) if t.starts_with("folder:") => self.toggle_folder(&t["folder:".len()..], cx),
            // The right-click menu of the session showing, the field it
            // is renamed in, and a name given without the field.
            Some("menu") => {
                if let Some(r) = self.selected_ref().cloned() {
                    self.open_menu(point(px(150.), px(330.)), vec![("Rename", MenuDo::Rename(key_of(&r))), (crate::sys::REVEAL_LABEL, MenuDo::Reveal(r.path.clone()))], cx);
                }
            }
            Some("renaming") => {
                self.renaming = self.selected.clone();
                cx.notify();
            }
            Some(t) if t.starts_with("name:") => {
                if let Some(key) = self.selected.clone() {
                    self.set_title(&key, &t["name:".len()..]);
                    cx.notify();
                }
            }
            // The sidebar button, and the pointer on it.
            Some("sidebar") => self.toggle_sidebar(false, cx),
            Some("float") => self.set_float(true, cx),
            Some("page:board") => {
                self.page = Page::Board;
                cx.notify();
            }
            Some("page:new") => {
                self.page = Page::New;
                self.selected = None;
                cx.notify();
            }
            // Another session, by the start of its id: for a look at
            // what the composer holds after a switch.
            Some(t) if t.starts_with("open:") => {
                if let Some(key) = self.refs.iter().find(|r| r.session_id.starts_with(&t["open:".len()..])).map(key_of) {
                    self.open_session(&key, cx);
                }
            }
            Some(t) if t.starts_with("mode:") => self.set_mode(&t["mode:".len()..], cx),
            Some(t) if t.starts_with("model:") => self.set_model(&t["model:".len()..], cx),
            Some(t) if t.starts_with("effort:") => self.set_effort(&t["effort:".len()..], cx),
            _ => {}
        }
    }

    /// Close a tab. The session goes on without it: a driver behind it
    /// keeps running until its idle timeout, and the transcript is on disk.
    /// Closing the showing tab moves to its neighbour, or to the new-session
    /// page when it was the last one, with the caret in its composer.
    pub fn close_tab(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t == key) else { return };
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
        let page = match self.page {
            Page::Board => "board",
            Page::Sessions => "sessions",
            Page::Session => "session",
            Page::New => "new",
        };
        UiState {
            window: self.window_rect,
            sidebar_open: Some(self.sidebar_open),
            page: page.into(),
            tabs: self.tabs.clone(),
            active: self.selected.clone().filter(|_| self.page == Page::Session),
            folders_open: self.folders_open.as_ref().map(|f| {
                let mut v: Vec<String> = f.iter().cloned().collect();
                v.sort();
                v
            }),
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
        self.reload_wanted = false;
        let path = r.path.clone();
        // The folder's commands, read once per folder, so the commands a
        // prompt names are known by the time they are drawn.
        if r.agent == AgentId::ClaudeCode && !r.cwd.is_empty() && std::path::Path::new(&r.cwd).is_dir() {
            let _ = self.hub.commands_for(&r.cwd);
        }
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
                if std::mem::take(&mut this.reload_wanted) {
                    if let Some(r) = this.refs.iter().find(|r| key_of(r) == key).cloned() {
                        this.load_detail(r, cx);
                    }
                }
                if timing {
                    eprintln!("emaki: open {} — load {}ms, set_detail {}ms, {} rounds", &key, loaded.as_millis(), t.elapsed().as_millis(), rounds);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// A message waiting in the queue is known to the transcript by its
    /// words alone until the agent takes it up; the pictures and files
    /// sent with it from here are put on it meanwhile, from the message
    /// as it left the window.
    fn dress_queued(&self, key: &str, session: &mut Session) {
        let Some((sent_key, text, attached)) = &self.last_sent else { return };
        if sent_key != key || attached.is_empty() {
            return;
        }
        let Some(rnd) = session.rounds.iter_mut().rev().find(|r| r.queued) else { return };
        if !rnd.attachments.is_empty() || rnd.prompt.trim() != text.trim() {
            return;
        }
        rnd.attachments = attached
            .iter()
            .filter(|a| a.path.is_file())
            .map(|a| emaki_core::model::Attachment {
                kind: if a.image { "image".into() } else { "file".into() },
                path: a.path.to_string_lossy().to_string(),
                name: a.name.clone(),
                media_type: a.mime.clone(),
                size: std::fs::metadata(&a.path).map(|m| m.len()).unwrap_or(0),
                ..Default::default()
            })
            .collect();
    }

    fn set_detail(&mut self, key: String, path: PathBuf, mut session: Session) {
        self.dress_queued(&key, &mut session);
        self.limits.refresh_from_statusline();
        self.refresh_context(&session.id);
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
                // Bottom alignment opens every session at its end, where the
                // newest turn is; a remembered position was tried and
                // dropped, since the end is where the reader wants to be.
                // Tail following keeps it there as a reply streams in: the
                // list re-pins to the end on every layout, pauses when the
                // reader scrolls up (or a card opened at the end gives the
                // list a real top, `pin_scroll`), and resumes once the view
                // is back at the bottom. Without it the first thing that
                // set a logical top left the list anchored to the prompt
                // while the reply grew below the view.
                let list = ListState::new(n, ListAlignment::Bottom, px(512.));
                list.set_follow_mode(FollowMode::Tail);
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
        // The agent's stand-in for its own default is "no model asked for".
        let v = if choice == self.options().default_model { String::new() } else { choice.to_string() };
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

        let appearance_key = match app.appearance.as_str() {
            "light" => "light",
            "dark" => "dark",
            _ => "system",
        };
        let appearance = self.segmented("appearance", vec![("system", "System".into(), None), ("light", "Light".into(), None), ("dark", "Dark".into(), None)], appearance_key, Rc::new(|this, key, window, cx| this.set_appearance(key, window, cx)), cx);
        // A row: a title and a line on it at the left, the choices at the right.
        let row = |title: &'static str, detail: &'static str, control: AnyElement, theme: &gpui_component::Theme| {
            h_flex()
                .items_center()
                .gap(px(12.))
                .child(v_flex().flex_1().min_w_0().gap(px(2.)).child(div().text_size(px(13.5)).child(title)).child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(detail)))
                .child(control)
        };

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
        let font_key = if app.chat_font == "sans" { "sans" } else { "serif" };
        let font = self.segmented("font", vec![("serif", "Anthropic Serif".into(), serif_family), ("sans", "Anthropic Sans".into(), sans_family)], font_key, Rc::new(|this, key, _, cx| this.set_chat_font(key, cx)), cx);
        let size_key = match app.chat_size.as_str() {
            "small" => "small",
            "large" => "large",
            _ => "medium",
        };
        let size = self.segmented("size", vec![("small", "Small".into(), None), ("medium", "Medium".into(), None), ("large", "Large".into(), None)], size_key, Rc::new(|this, key, _, cx| this.set_chat_size(key, cx)), cx);
        let note = match &fonts.source {
            Some(dir) => format!("Anthropic Serif and Anthropic Sans are loaded from the Claude app at {}.", emaki_core::paths::tilde(&dir.to_string_lossy())),
            None => "Anthropic Serif and Anthropic Sans are the Claude desktop app's fonts and are loaded from it when it is installed. It was not found here, so Georgia stands in for the serif and the window's own face for the sans.".to_string(),
        };

        let default_mode = if self.cfg.driver.default_mode.is_empty() { "default".to_string() } else { self.cfg.driver.default_mode.clone() };
        // The same lists the pills under the composer open, since both
        // are whatever the agent offers.
        let options = self.options();
        let wb = cx.entity().downgrade();
        let modes = picker(
            "default-mode",
            "icons/shield.svg",
            PillText { tail: "", ..mode_pill(&options, &default_mode, &theme) },
            self.modes(&options).into_iter().map(|m| (m.key, m.label, m.detail)).collect(),
            default_mode.clone(),
            Anchor::TopRight,
            wb.clone(),
            Rc::new(|this, key, cx| this.set_default_mode(key, cx)),
            cx,
        );
        let default_model = if self.cfg.driver.default_model.is_empty() { options.default_model.clone() } else { self.cfg.driver.default_model.clone() };
        let models = picker(
            "default-model",
            "icons/box.svg",
            PillText::plain(model_name(&options, &default_model)),
            options.models.iter().map(|m| (m.key.clone(), m.label.clone(), m.detail.clone())).collect(),
            options.model(&default_model).map(|m| m.key.clone()).unwrap_or(default_model),
            Anchor::TopRight,
            wb,
            Rc::new(|this, key, cx| this.set_default_model(key, cx)),
            cx,
        );

        let scope = if self.cfg.explain.enabled { self.cfg.explain.scope.as_str() } else { "off" };
        let explain_key = match scope {
            "off" => "off",
            "all" => "all",
            _ => "permission",
        };
        let explain = self.segmented("explain", vec![("off", "Off".into(), None), ("permission", "Permission cards".into(), None), ("all", "Every new call".into(), None)], explain_key, Rc::new(|this, key, _, cx| this.set_explain_scope(key, cx)), cx);
        // Updates, as little as possible: the version on the left, one
        // button on the right that is "Check for updates" until a check
        // finds something and "Update to x" after, in the same place, and
        // a small underlined "Release notes" link under it only then. The
        // detail line says something only when there is something to say
        // (downloading, a failure, up to date after a click). Three pills
        // and two sentences used to sit here.
        let u = &self.update;
        let current = update::current_version();
        let update_line = if u.installing {
            match u.progress {
                Some((done, Some(total))) if total > 0 => format!("Downloading… {}%", done * 100 / total),
                Some(_) => "Downloading…".to_string(),
                None => "Installing… Emaki restarts by itself.".to_string(),
            }
        } else if !u.install_error.is_empty() {
            format!("The update did not go through: {}", u.install_error)
        } else if !u.error.is_empty() {
            "Could not reach GitHub.".to_string()
        } else if u.available.is_none() && u.answered && !u.automatic {
            "Up to date.".to_string()
        } else {
            String::new()
        };
        let can_install = u.available.is_some() && !u.installing && update::asset_name().is_some();
        let checking = u.checking;
        let installing = u.installing;
        let notes_url = u.available.as_deref().map(update::release_page);
        let button = if can_install {
            let label: SharedString = format!("Update to {}", u.available.as_deref().unwrap_or("")).into();
            pill_button("update-now", label, &theme, cx.listener(|this, _, _, cx| this.install_update_now(cx))).into_any_element()
        } else if installing {
            pill_button("update-busy", "Updating…", &theme, |_, _, _| {}).into_any_element()
        } else {
            pill_button("update-check", if checking { "Checking…" } else { "Check for updates" }, &theme, cx.listener(|this, _, _, cx| this.check_updates_now(cx))).into_any_element()
        };
        let link_color = theme.muted_foreground;
        let link_hover = theme.primary;
        let version_row = v_flex().items_end().gap(px(6.)).child(button).when_some(notes_url, |d, url| {
            d.child(
                div()
                    .id("update-notes")
                    .cursor_pointer()
                    .text_size(px(11.5))
                    .text_color(link_color)
                    .text_decoration_1()
                    .text_decoration_color(link_color.opacity(0.6))
                    .hover(move |s| s.text_color(link_hover).text_decoration_color(link_hover))
                    .on_click(move |_, _, _| {
                        let _ = opener::open(&url);
                    })
                    .child("Release notes"),
            )
        });
        let auto_check = Checkbox::new("update-auto").checked(self.cfg.app.check_updates).on_click(cx.listener(|this, on: &bool, _, cx| this.set_check_updates(*on, cx)));
        let version_detail: SharedString = if update_line.is_empty() { format!("Emaki {current}") } else { format!("Emaki {current} · {update_line}") }.into();

        let explain_model = if self.cfg.explain.model.is_empty() { "claude-haiku-4-5".to_string() } else { self.cfg.explain.model.clone() };
        let explain_note = format!(
            "An opaque call (a heredoc, a piped chain, anything long) gets one or two plain sentences from {} through your own Claude Code login. Simple calls explain themselves for free, answers are kept by content so a command is explained once, and every tool card has an Explain button.",
            model_label(&explain_model)
        );

        // The rows of the section showing, and the rail beside them.
        let section = self.settings_section;
        let (section_title, section_anim): (&'static str, &'static str) = match section {
            "sessions" => ("New sessions", "settings-sessions"),
            "explain" => ("Explanations", "settings-explain"),
            "updates" => ("Updates", "settings-updates"),
            _ => ("Appearance", "settings-appearance"),
        };
        let rows: Vec<AnyElement> = match section {
            "sessions" => vec![
                row("Permission mode", "What a session started here begins in.", modes.into_any_element(), &theme).into_any_element(),
                row("Model", "Which model a session started here uses.", models.into_any_element(), &theme).into_any_element(),
                div().text_size(px(12.)).text_color(theme.muted_foreground).child("A running session keeps its own choices: the pills under its composer change them for that session, and ⇧Tab in the composer steps through the modes.").into_any_element(),
            ],
            "explain" => vec![
                row("Explain tool calls", "Which calls are put into plain words without asking.", explain.into_any_element(), &theme).into_any_element(),
                div().text_size(px(12.)).text_color(theme.muted_foreground).child(explain_note).into_any_element(),
            ],
            "updates" => vec![
                h_flex()
                    .items_start()
                    .gap(px(12.))
                    .child(v_flex().flex_1().min_w_0().gap(px(2.)).child(div().text_size(px(13.5)).child("Version")).child(div().text_size(px(12.)).text_color(theme.muted_foreground).whitespace_normal().child(version_detail)))
                    .child(version_row)
                    .into_any_element(),
                h_flex().items_center().gap(px(12.)).child(div().flex_1().text_size(px(13.5)).child("Check for updates automatically")).child(auto_check).into_any_element(),
            ],
            _ => vec![
                row("Theme", "Follow the system, or keep one look.", appearance.into_any_element(), &theme).into_any_element(),
                row("Accent", "The colour of the send button, links and the mark.", accents.into_any_element(), &theme).into_any_element(),
                row("Chat font", "The face the conversation is set in.", font.into_any_element(), &theme).into_any_element(),
                row("Text size", "How large the conversation reads.", size.into_any_element(), &theme).into_any_element(),
                div().text_size(px(12.)).text_color(theme.muted_foreground).child(note).into_any_element(),
            ],
        };
        let rail = self.settings_rail(cx);

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
                    .w(SETTINGS_W)
                    .h(SETTINGS_H)
                    .max_w(gpui::relative(0.94))
                    .max_h(gpui::relative(0.9))
                    .rounded(px(18.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .px(px(22.))
                            .h(px(52.))
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
                    // A rail of sections on the left, the chosen section's
                    // rows on the right. The rows scroll under the panel's
                    // header with the toolkit's scrollbar at their edge (it
                    // fades a second after the last scroll, as every
                    // scrollbar in the window does).
                    .child(h_flex().flex_1().min_h_0().items_stretch().child(rail).child(
                        v_flex().relative().flex_1().min_w_0().min_h_0().child(
                            v_flex()
                                .id("settings-body")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .track_scroll(&self.settings_scroll)
                                .px(px(24.))
                                .py(px(20.))
                                .child(page_in(section_anim, v_flex().gap(px(16.)).child(div().pb(px(2.)).text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(section_title)).children(rows))),
                        )
                        .vertical_scrollbar(&self.settings_scroll),
                    )),
            )
    }

    /// The rail of sections at the left of the settings panel: an icon and
    /// a name per section, the chosen one on a plate.
    fn settings_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let current = self.settings_section;
        v_flex()
            .w(SETTINGS_RAIL_W)
            .flex_shrink_0()
            .p(px(10.))
            .gap(px(2.))
            .bg(if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.5) })
            .border_r_1()
            .border_color(theme.border)
            .children(SETTINGS_SECTIONS.iter().map(|(key, label, icon)| {
                let active = *key == current;
                let key: &'static str = key;
                h_flex()
                    .id(SharedString::from(format!("settings-nav-{key}")))
                    .h(px(30.))
                    .px(px(10.))
                    .gap(px(9.))
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.sidebar_accent))
                    .when(!active, |d| d.hover(|s| s.bg(theme.sidebar_accent.opacity(0.6))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_section = key;
                        this.settings_scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }))
                    .child(Icon::default().path(*icon).with_size(px(15.)).text_color(if active { theme.foreground } else { theme.muted_foreground }))
                    .child(div().text_size(px(13.)).when(active, |d| d.font_weight(FontWeight::MEDIUM)).text_color(if active { theme.foreground } else { theme.sidebar_foreground }).child(*label))
            }))
            .into_any_element()
    }

    /// A segmented control: the choices in a row on a muted track, the    /// A segmented control: the choices in a row on a muted track, the
    /// chosen one on a raised plate. The plate is one element drawn under
    /// the row, placed from where each segment sat on the last draw
    /// (`seg_bounds`, recorded as the row is prepainted), and it slides
    /// from the old choice to the new one over a moment, keyed on the new
    /// choice so each click plays it once. On the very first draw, before
    /// any segment has been measured, the chosen segment paints its own
    /// plate instead, so nothing flashes.
    #[allow(clippy::type_complexity)]
    fn segmented(&self, control: &'static str, options: Vec<(&'static str, String, Option<String>)>, current: &'static str, on: Rc<dyn Fn(&mut Self, &'static str, &mut Window, &mut Context<Self>)>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let track_bg = if dark { theme.sidebar } else { theme.muted };
        let plate_bg = if dark { theme.secondary_active } else { theme.popover };
        let (from, to) = {
            let mut st = self.seg_state.borrow_mut();
            let e = st.entry(control).or_insert((current, current));
            if e.1 != current {
                e.0 = e.1;
                e.1 = current;
            }
            *e
        };
        let (b_from, b_to) = {
            let b = self.seg_bounds.borrow();
            (b.get(&format!("{control}-{from}")).copied(), b.get(&format!("{control}-{to}")).copied())
        };
        let plate = match (b_from, b_to) {
            (Some(a), Some(b)) => Some(
                div()
                    .absolute()
                    .rounded(px(7.))
                    .bg(plate_bg)
                    .shadow_sm()
                    .with_animation(ElementId::Name(format!("{control}-plate-{to}").into()), Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()), move |d, t| {
                        let x = a.origin.x + (b.origin.x - a.origin.x) * t;
                        let w = a.size.width + (b.size.width - a.size.width) * t;
                        d.left(x).top(b.origin.y).w(w).h(b.size.height)
                    }),
            ),
            _ => None,
        };
        let measured = plate.is_some();
        let ids: Vec<String> = options.iter().map(|(k, _, _)| format!("{control}-{k}")).collect();
        let seg_bounds = self.seg_bounds.clone();
        let entity = cx.entity().downgrade();
        let row = h_flex()
            .gap(px(2.))
            // Where each segment landed, relative to the track: the row
            // sits inside the track's 3px padding. A change is noted and
            // the next draw places the plate from it.
            .on_children_prepainted(move |bounds, _, cx| {
                let Some(first) = bounds.first() else { return };
                let mut changed = false;
                let mut map = seg_bounds.borrow_mut();
                for (id, b) in ids.iter().zip(bounds.iter()) {
                    let rel = Bounds { origin: point(b.origin.x - first.origin.x + px(3.), px(3.)), size: b.size };
                    if map.get(id) != Some(&rel) {
                        map.insert(id.clone(), rel);
                        changed = true;
                    }
                }
                drop(map);
                if changed {
                    let _ = entity.update(cx, |_, cx| cx.notify());
                }
            })
            .children(options.into_iter().map(|(key, label, family)| {
                let active = key == current;
                let on = on.clone();
                h_flex()
                    .id(SharedString::from(format!("{control}-{key}")))
                    .h(px(26.))
                    .px(px(11.))
                    .items_center()
                    .rounded(px(7.))
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .when(active && !measured, |d| d.bg(plate_bg).shadow_sm())
                    .when(active, |d| d.font_weight(FontWeight::MEDIUM).text_color(theme.foreground))
                    .when(!active, |d| d.text_color(theme.muted_foreground).hover(|s| s.text_color(theme.foreground)))
                    .when_some(family, |d, f| d.font_family(f))
                    .on_click(cx.listener(move |this, _, window, cx| on(this, key, window, cx)))
                    .child(label)
            }));
        h_flex().relative().p(px(3.)).rounded(px(9.)).bg(track_bg).flex_shrink_0().children(plate).child(row).into_any_element()
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
        h_flex().w_full().justify_center().px(px(24.)).py(px(8.)).child(
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
    /// Bring the terminal this session runs in to the front, for what only
    /// it can take: a question's dialog, an approval, a slash command, the
    /// mode. When the session is waiting on the person or idle, the next
    /// change to its transcript brings the window back (`come_back`); while
    /// the agent works, the next change would be its own, so nothing is
    /// armed.
    pub fn go_to_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.selected_ref().cloned() else { return };
        let Some(peer) = self.hub.peer_for(&r.session_id).filter(|_| self.in_terminal(&r)) else {
            return self.via_terminal(TerminalAction::Go, cx);
        };
        // A terminal of our own has no app to bring forward: it is drawn here.
        if self.own_terminal(&r.session_id) {
            return self.show_terminal(&r.session_id, false, cx);
        }
        match crate::sys::focus_terminal(peer.pid) {
            Ok(app) => {
                self.come_back = (!self.is_working(&r)).then(|| ComeBack::on_transcript(&r, self.now));
                self.notice = Some(Notice::said(format!("in {app}; back here when that is done")));
            }
            Err(e) => self.notice = Some(Notice::error(e)),
        }
        cx.notify();
    }

    /// Type `text` into the terminal this session runs in and send it, the
    /// way a slash command has to go on a terminal session; the terminal
    /// comes to the front, and the window comes back once the transcript
    /// shows the command ran (`come_back`). The typing runs on a thread,
    /// since the app may take a moment to answer, and reports on the row
    /// under the composer.
    pub fn run_in_terminal(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(r) = self.selected_ref().cloned() else { return };
        if self.theirs_busy(&r, cx) {
            return;
        }
        let Some(peer) = self.hub.peer_for(&r.session_id).filter(|_| self.in_terminal(&r)) else {
            return self.via_terminal(TerminalAction::Run(text), cx);
        };
        // In a terminal of our own the command is typed and nobody goes
        // anywhere. It is watched as a pick is: a command that opens an
        // interface of its own (`/config`, `/status`) has the registry
        // say `waiting`, and the terminal is then drawn here until it
        // is closed (`watch_terminal`).
        if self.own_terminal(&r.session_id) {
            self.come_back = Some(ComeBack { mtime: None, typed: Some(now_secs()), ..ComeBack::on_transcript(&r, self.now) });
            std::thread::spawn(move || {
                let _ = crate::sys::type_in_terminal(peer.pid, &text);
            });
            cx.notify();
            return;
        }
        self.come_back = (!self.is_working(&r)).then(|| ComeBack::on_transcript(&r, self.now));
        self.notice = Some(Notice::said(format!("sending {text} to the terminal…")));
        let hub = Arc::clone(&self.hub);
        std::thread::spawn(move || match crate::sys::type_in_terminal(peer.pid, &text) {
            Ok(app) => hub.say(format!("sent to {app}; back here when it has run")),
            Err(e) => hub.say(e),
        });
        cx.notify();
    }

    /// A click on one of the three pills. On a session it is the
    /// terminal's to answer (`via_terminal`). Before a session exists
    /// there is no terminal to open: the pills show what a new session
    /// starts in, and a click opens Settings where that is chosen.
    fn pill_clicked(&mut self, pill: Pill, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Session {
            // The mode has no picker: a click on its pill is one ⇧Tab.
            if pill == Pill::Mode {
                return self.cycle_mode(cx);
            }
            return self.via_terminal(TerminalAction::Pick(pill), cx);
        }
        self.settings_section = "sessions";
        self.settings_open = true;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Do something only the session's terminal can take: a pick from
    /// `/model` or `/effort`, a typed command, ⇧Tab for the mode, or just
    /// going there. With the session in a terminal it is done at once.
    /// With none, the terminal is opened first, the way the button at the
    /// top right opens it (`open_in_terminal`: the agent's own resume
    /// command, an idle driver of ours stopped on the way), the row under
    /// the composer says so, and the action waits in `pending_terminal`
    /// until Claude Code has registered there and says it is idle
    /// (`terminal_ready`). A session a driver is mid-reply on is refused,
    /// as the button refuses it: one writer per transcript.
    pub fn via_terminal(&mut self, action: TerminalAction, cx: &mut Context<Self>) {
        let Some(r) = self.selected_ref().cloned() else { return };
        if matches!(action, TerminalAction::Pick(Pill::Mode)) {
            return self.pick_in_terminal(Pill::Mode, cx);
        }
        // Asked again while the terminal is still coming up: the newer
        // wish takes the older one's place and goes on waiting, also once
        // the terminal has registered and is not ready for keys yet. Done
        // at once here, it was done a second time when the wait ended.
        if self.pending_terminal.as_ref().is_some_and(|(sid, _, _)| *sid == r.session_id) {
            self.pending_terminal = Some((r.session_id.clone(), action, self.pending_terminal.as_ref().map(|p| p.2).unwrap_or(self.now)));
            self.notice = Some(self.opening(&r.session_id));
            cx.notify();
            return;
        }
        if self.in_terminal(&r) {
            // A terminal of our own that is still coming up: the wish
            // waits for it like any other.
            if self.hub.peer_for(&r.session_id).is_none() {
                self.pending_terminal = Some((r.session_id.clone(), action, now_secs()));
                self.notice = Some(self.opening(&r.session_id));
                cx.notify();
                return;
            }
            return self.do_in_terminal(action, cx);
        }
        if let Err(why) = self.terminal_check(&r) {
            self.notice = Some(Notice::error(why));
            cx.notify();
            return;
        }
        if self.drivers.remove(&r.session_id).is_some() {
            self.hub.stop_driver(&r.session_id);
        }
        // The terminal is one of our own, with no window, unless the
        // setting says otherwise; then theirs is opened, as the button
        // opens it.
        let opened = if r.agent == AgentId::ClaudeCode && self.hub.hidden_terminals() {
            self.hub.start_terminal(&r.session_id, &r.cwd, true, "", "")
        } else {
            let argv = emaki_core::terminal::resume_argv(r.agent, &r.session_id);
            crate::sys::open_in_terminal(&r.session_id, &r.cwd, &argv)
        };
        match opened {
            Ok(()) => {
                self.pending_terminal = Some((r.session_id.clone(), action, now_secs()));
                self.notice = Some(self.opening(&r.session_id));
                self.hub.refresh();
            }
            Err(e) => self.notice = Some(Notice::error(format!("could not open a terminal: {e}"))),
        }
        cx.notify();
    }

    fn do_in_terminal(&mut self, action: TerminalAction, cx: &mut Context<Self>) {
        match action {
            TerminalAction::Go => self.go_to_terminal(cx),
            TerminalAction::Pick(pill) => self.pick_in_terminal(pill, cx),
            TerminalAction::Run(text) => self.run_in_terminal(text, cx),
            TerminalAction::StepMode => {
                if let Some((sid, peer)) = self.terminal_peer() {
                    self.step_mode(sid, peer, cx);
                }
            }
        }
    }

    /// While a terminal is being opened for something: wait for it to be
    /// there and ready, however long that takes, then do what it was
    /// opened for. Nothing here is a length of time. Two things have to be
    /// so: Claude Code has registered in that terminal and its record
    /// says `idle` (a login or trust screen says `waiting`, and is never
    /// typed into), and its prompt is on the screen, which is the mode's
    /// footer under it (`terminal_up`, read off the terminal where it can
    /// be read; where it cannot, the registry's word stands). Looked at
    /// on the clock and whenever the terminal's status line runs. The
    /// wait ends only when the person leaves the session.
    fn terminal_ready(&mut self, cx: &mut Context<Self>) {
        let Some((sid, _, _)) = self.pending_terminal.clone() else { return };
        let showing = self.selected_ref().is_some_and(|r| self.page == Page::Session && r.session_id == sid);
        if !showing {
            self.pending_terminal = None;
            self.terminal_up = None;
            return;
        }
        self.hub.refresh_peers();
        let peer = self.hub.peer_for(&sid).filter(|p| p.status == "idle");
        let in_terminal = self.selected_ref().is_some_and(|r| self.in_terminal(r));
        match peer {
            Some(_) if in_terminal && self.terminal_up.as_deref() == Some(sid.as_str()) => {
                let (_, action, _) = self.pending_terminal.take().unwrap();
                self.terminal_up = None;
                self.notice = None;
                // Opening the terminal brought it to the front. A pick
                // keeps the person there, to choose. ⇧Tab asks nothing of
                // them, so the window takes the front back, as it never
                // left it when the terminal was already open.
                let back = matches!(action, TerminalAction::StepMode);
                self.do_in_terminal(action, cx);
                if back {
                    cx.activate(true);
                }
            }
            Some(peer) if in_terminal => {
                self.notice = Some(self.opening(&sid));
                if !self.terminal_probing {
                    self.terminal_probing = true;
                    let modes = self.hub.options_for(AgentId::ClaudeCode, &peer.cwd).modes;
                    cx.spawn(async move |this, cx| {
                        let up = cx
                            .background_spawn(async move {
                                match crate::sys::terminal_text(peer.pid) {
                                    Some(text) if !modes.is_empty() => emaki_core::driver::mode_on_screen(&text, &modes).is_some(),
                                    _ => true,
                                }
                            })
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            this.terminal_probing = false;
                            if up {
                                this.terminal_up = Some(sid);
                                this.terminal_ready(cx);
                            }
                        });
                    })
                    .detach();
                }
            }
            _ => {
                // Kept on the row while it waits, past a notice's usual life.
                self.notice = Some(self.opening(&sid));
                self.hub.refresh();
            }
        }
        cx.notify();
    }

    /// A click on the mode, model or effort pill of a terminal session.
    /// The choice is made in the terminal. The model and the effort are a
    /// bare `/model` or `/effort` typed there, which opens Claude Code's
    /// own list or slider, and the window comes back when the pick is
    /// made: done is the status line naming another model or effort, or,
    /// on an idle session, the row the command leaves in the transcript
    /// ("Kept model as …" when nothing was changed). The mode has no
    /// command, only ⇧Tab, and ⇧Tab in the composer presses it in the
    /// terminal without leaving the window (`step_mode`), so a click on
    /// the mode pill goes nowhere: it says which key to use. It used to
    /// go to the terminal for the key, and for a while came back once the
    /// presses stopped, which landed sometimes and not others.
    pub fn pick_in_terminal(&mut self, pill: Pill, cx: &mut Context<Self>) {
        let Some(r) = self.selected_ref().cloned() else { return };
        if pill == Pill::Mode {
            self.notice = Some(Notice::said("Use ⇧Tab to change the mode"));
            cx.notify();
            return;
        }
        if self.theirs_busy(&r, cx) {
            return;
        }
        let Some(peer) = self.hub.peer_for(&r.session_id).filter(|_| self.in_terminal(&r)) else {
            return self.via_terminal(TerminalAction::Pick(pill), cx);
        };
        let working = self.is_working(&r);
        let ctx = emaki_core::limits::session_context(&r.session_id);
        let hub = Arc::clone(&self.hub);
        match pill {
            Pill::Mode => {}
            Pill::Model | Pill::Effort => {
                let command = if pill == Pill::Model { "/model" } else { "/effort" };
                // What the pick is measured against. The status line's
                // word for the session when it has one, else what the
                // pills show: an empty pair would be "changed" by the
                // first run of the status line, and the window came back
                // before anything was chosen. And the transcript as of
                // this moment, not as of the last scan: a terminal just
                // opened for this has written its resume rows since, and
                // those are not the command's row.
                let before = ctx.map(|c| (c.model, c.effort)).unwrap_or_else(|| (self.current_model(), self.current_effort()));
                let _ = working;
                self.come_back = Some(ComeBack { pick: Some(before), mtime: None, typed: Some(now_secs()), ..ComeBack::on_transcript(&r, self.now) });
                // In a terminal of our own the picker is drawn here, over
                // the composer, and goes when the pick is made.
                if self.own_terminal(&r.session_id) {
                    std::thread::spawn(move || {
                        let _ = crate::sys::type_in_terminal(peer.pid, command);
                    });
                    return self.show_terminal(&r.session_id, false, cx);
                }
                self.notice = Some(Notice::said(format!("opening {command} in the terminal…")));
                std::thread::spawn(move || match crate::sys::type_in_terminal(peer.pid, command) {
                    Ok(app) => hub.say(format!("choose in {app}; back here once it is set")),
                    Err(e) => hub.say(e),
                });
            }
        }
        cx.notify();
    }

    /// While the person is in a terminal for the model or effort pill:
    /// come back when the pick is made, and not before. The sign is
    /// Claude Code's own: its registry record says `waiting` while the
    /// picker is up and something else once it is closed, chosen or
    /// cancelled, with the time of each change. So the pick is over when
    /// the record was seen `waiting` after the command was typed and no
    /// longer is, or, for a picker opened and closed between two looks,
    /// when it says `idle` as of a time after the typing. A change alone
    /// is not enough: the record says `busy` for a few milliseconds
    /// between the command and the picker (recorded on 2.1.289: busy,
    /// waiting 7 ms later, idle at the choice). Looked at on the clock
    /// and whenever the status line runs. What it was measured by before
    /// came back early after a
    /// fresh open: the status line naming another effort than a stale
    /// file had, and the transcript moving, which a resume also does. A
    /// Claude Code whose record carries no status falls back to the
    /// status line's word.
    fn watch_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(cb) = self.come_back.as_ref().filter(|cb| cb.typed.is_some()) else { return };
        let typed = cb.typed.unwrap_or(0.0);
        let (sid, picking) = (cb.sid.clone(), cb.picking);
        self.hub.refresh_peers();
        let peer = self.hub.peer_for(&sid).filter(|p| !p.status.is_empty());
        if peer.as_ref().is_some_and(|p| p.status == "waiting" && p.status_at >= typed) {
            if let Some(cb) = self.come_back.as_mut() {
                cb.picking = true;
            }
            // A command typed into our own terminal opened an interface:
            // it is drawn here for as long as it is up.
            if self.own_terminal(&sid) && self.term_open.is_none() && self.term_dismissed.as_deref() != Some(sid.as_str()) {
                self.show_terminal(&sid, false, cx);
            }
            return;
        }
        // A command in our own terminal that opened nothing and started
        // no turn (`/cost`) changes no status: it is not waited on.
        if self.own_terminal(&sid) && !picking && self.term_open.is_none() && now_secs() - typed > 3.0 && peer.as_ref().is_some_and(|p| p.status == "idle") {
            self.come_back = None;
            return;
        }
        let Some(cb) = self.come_back.as_ref() else { return };
        let done = match peer {
            Some(p) => picking || (p.status == "idle" && p.status_at > typed),
            None => match (&cb.pick, emaki_core::limits::session_context(&cb.sid)) {
                (Some((model, effort)), Some(c)) => c.model != *model || c.effort != *effort,
                _ => false,
            },
        };
        if done {
            self.back_from_terminal(cx);
        }
    }

    /// What the terminal says the agent is doing, for the row under the
    /// conversation: Claude Code's own line over its prompt, the word
    /// and the turn's figures ("Embellishing… (13s · ↓ 1.0k tokens)"),
    /// read off the screen of the session showing while its turn runs
    /// (`driver::working_on_screen`), once a second and whenever its
    /// status line runs. One read at a time, off the main thread. A
    /// driven session has no screen, and an IDE's terminal cannot be
    /// read; the row then says "<agent> is working…".
    fn read_working(&mut self, cx: &mut Context<Self>) {
        let target = self
            .selected_ref()
            .filter(|r| self.page == Page::Session && self.is_working(r) && self.in_terminal(r))
            .and_then(|r| self.hub.peer_for(&r.session_id).map(|p| (r.session_id.clone(), p.pid)));
        let Some((sid, pid)) = target else {
            if self.working_seen.take().is_some() {
                cx.notify();
            }
            return;
        };
        if self.working_reading {
            return;
        }
        self.working_reading = true;
        cx.spawn(async move |this, cx| {
            let seen = cx.background_spawn(async move { crate::sys::terminal_styled(pid).and_then(|t| emaki_core::driver::working_on_screen(&t)) }).await;
            let _ = this.update(cx, |this, cx| {
                this.working_reading = false;
                // A screen without the line for a moment (a dialog over
                // it, a redraw) keeps the last word a few seconds.
                let seen = match seen {
                    Some(w) => Some((sid, w, this.now)),
                    None => this.working_seen.take().filter(|(s, _, at)| *s == sid && this.now - at < WORKING_KEPT_SECS),
                };
                if this.working_seen != seen {
                    this.working_seen = seen;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The prompt the terminal suggests for the session showing, once
    /// its turn is over: Claude Code sets words in its input, dim, that →
    /// takes there. The composer offers the same words the same way
    /// (`suggested`): as its placeholder, taken with → or Tab while the
    /// box is empty, sent as they are with ⌘↩. Read off the screen
    /// (`driver::suggestion_on_screen`, which needs its colours, so
    /// WezTerm and Kaku only) once a second while the session is idle in
    /// its terminal and the box is empty, one read at a time.
    fn read_suggestion(&mut self, cx: &mut Context<Self>) {
        let empty = self.composer.read(cx).value().is_empty();
        let target = self
            .selected_ref()
            .filter(|r| self.page == Page::Session && empty && !self.is_working(r) && self.in_terminal(r))
            .and_then(|r| self.hub.peer_for(&r.session_id).filter(|p| p.status == "idle").map(|p| (r.session_id.clone(), p.pid)));
        let Some((sid, pid)) = target else {
            if self.suggestion.take().is_some() {
                cx.notify();
            }
            return;
        };
        if self.suggestion_reading {
            return;
        }
        self.suggestion_reading = true;
        cx.spawn(async move |this, cx| {
            let words = cx.background_spawn(async move { crate::sys::terminal_styled(pid).and_then(|t| emaki_core::driver::suggestion_on_screen(&t)) }).await;
            let _ = this.update(cx, |this, cx| {
                this.suggestion_reading = false;
                let seen = words.map(|w| (sid, w));
                if this.suggestion != seen {
                    this.suggestion = seen;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The suggested prompt for the session showing, when there is one.
    fn suggested(&self) -> Option<&str> {
        let r = self.selected_ref().filter(|_| self.page == Page::Session)?;
        self.suggestion.as_ref().filter(|(sid, _)| *sid == r.session_id).map(|(_, w)| w.as_str())
    }

    /// → or Tab in an empty composer: the suggested prompt becomes the
    /// text, caret after it, to send or to change. False when the box
    /// has words or nothing is suggested, and the key is the textarea's.
    fn accept_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.composer.read(cx).value().is_empty() {
            return false;
        }
        let Some(words) = self.suggested().map(str::to_string) else { return false };
        let col = words.encode_utf16().count() as u32;
        self.composer.update(cx, |s, cx| {
            s.set_value(words, window, cx);
            s.set_cursor_position(gpui_component::input::Position::new(0, col), window, cx);
        });
        self.suggestion = None;
        cx.notify();
        true
    }

    /// The dialog the terminal holds the session showing on, for the card
    /// over the composer: a question of Claude's, the review of the
    /// answers, an approval. Claude Code writes none of it to the
    /// transcript until it is answered, so it is read off the screen
    /// (`driver::dialog_on_screen`) while the registry record says
    /// `waiting`, once a second and whenever the status line runs, one
    /// read at a time. Not while the person was sent to the terminal for
    /// a pick of their own (`/model`, `/effort`), whose list is theirs to
    /// use there. A screen that cannot be read leaves the line that says
    /// to answer in the terminal.
    fn read_dialog(&mut self, cx: &mut Context<Self>) {
        let picking = self.come_back.as_ref().is_some_and(|cb| cb.typed.is_some());
        let target = self.selected_ref().filter(|r| self.page == Page::Session && self.in_terminal(r) && !picking).map(|r| r.session_id.clone()).and_then(|sid| {
            self.hub.refresh_peers();
            self.hub.peer_for(&sid).filter(|p| p.status == "waiting").map(|p| (sid, p.pid))
        });
        let Some((sid, pid)) = target else {
            if self.dialog_seen.take().is_some() {
                cx.notify();
            }
            // The wait the terminal was drawn for is over.
            if !picking {
                self.term_dismissed = None;
                if self.term_auto {
                    self.hide_terminal(cx);
                }
            }
            return;
        };
        if self.dialog_reading {
            return;
        }
        self.dialog_reading = true;
        cx.spawn(async move |this, cx| {
            let seen = cx.background_spawn(async move { crate::sys::terminal_styled(pid).and_then(|t| emaki_core::driver::dialog_on_screen(&t)) }).await;
            let _ = this.update(cx, |this, cx| {
                this.dialog_reading = false;
                // Our own terminal waiting on a screen that is no
                // dialog the card knows (a folder to trust, a login):
                // the screen itself is drawn. A dialog the card does
                // know takes its place.
                match &seen {
                    None if this.term_open.is_none() && this.term_dismissed.as_deref() != Some(sid.as_str()) => this.show_terminal(&sid, true, cx),
                    Some(_) if this.term_auto => this.hide_terminal(cx),
                    _ => {}
                }
                let seen = seen.map(|d| (sid, d));
                if this.dialog_seen != seen {
                    this.dialog_seen = seen;
                    cx.notify();
                }
                if this.dialog_seen.is_some() {
                    match this.dialog_probe.take() {
                        Some(g) if g.starts_with("answer:") => {
                            this.dialog_answer_typed(&g["answer:".len()..], cx);
                        }
                        Some(g) if g.starts_with("goto:") => this.dialog_go(g["goto:".len()..].parse().unwrap_or(0), cx),
                        Some(g) => this.dialog_send(vec![DialogStep { keys: g["dialog:".len()..].replace("tab", "\t"), until: None }], cx),
                        None => {}
                    }
                }
            });
        })
        .detach();
    }

    /// Answer the terminal's dialog from the card: `steps` are sent to
    /// the terminal as keys, one after another, each once the screen
    /// shows what the step before it did. A digit picks a choice; for
    /// words of the person's own, the digit of "Type something" puts the
    /// terminal's pointer there, the words go into its field, and Return
    /// sends them. Sent in one write the dialog took the digit and lost
    /// the rest, so the steps wait on the screen, not on a clock.
    fn dialog_send(&mut self, steps: Vec<DialogStep>, cx: &mut Context<Self>) {
        let Some((sid, _)) = self.dialog_seen.clone() else { return };
        let Some(peer) = self.hub.peer_for(&sid) else { return };
        if self.dialog_sending {
            return;
        }
        self.dialog_sending = true;
        let pid = peer.pid;
        cx.spawn(async move |this, cx| {
            let sent = cx
                .background_spawn(async move {
                    for step in steps {
                        crate::sys::text_in_terminal(pid, &step.keys)?;
                        let Some(until) = step.until else { continue };
                        // The screen follows the key within a frame or
                        // two; looked at until it has, and given up on
                        // when it never does.
                        let mut shown = false;
                        for _ in 0..40 {
                            std::thread::sleep(Duration::from_millis(60));
                            let dialog = crate::sys::terminal_styled(pid).and_then(|t| emaki_core::driver::dialog_on_screen(&t));
                            if dialog.is_some_and(|d| until.holds(&d)) {
                                shown = true;
                                break;
                            }
                        }
                        if !shown {
                            return Err("the terminal's dialog did not follow; finish the answer there".to_string());
                        }
                    }
                    Ok(())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.dialog_sending = false;
                if let Err(e) = sent {
                    this.notice = Some(Notice::error(e));
                }
                this.read_dialog(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Go to another of the dialog's questions, or to its review, as the
    /// arrow keys do in the terminal: one arrow a step, each once the
    /// screen shows the tab before it. `to` is the tab's place.
    fn dialog_go(&mut self, to: usize, cx: &mut Context<Self>) {
        let Some(from) = self.dialog_seen.as_ref().and_then(|(_, d)| d.current) else { return };
        let (key, range): (&str, Vec<usize>) = if to > from { ("\x1b[C", (from + 1..=to).collect()) } else { ("\x1b[D", (to..from).rev().collect()) };
        let steps = range.into_iter().map(|ix| DialogStep { keys: key.to_string(), until: Some(DialogUntil::Tab(ix)) }).collect();
        self.dialog_send(steps, cx);
    }

    /// Words typed in the composer while the terminal's dialog offers
    /// "Type something": they are the answer. False when the dialog on
    /// the screen has no such field, or takes several choices, where the
    /// field works another way and is left to the terminal.
    fn dialog_answer_typed(&mut self, words: &str, cx: &mut Context<Self>) -> bool {
        let shown = self.selected_ref().map(|r| r.session_id.clone());
        let Some((_, d)) = self.dialog_seen.as_ref().filter(|(sid, _)| Some(sid) == shown.as_ref()) else { return false };
        let Some(n) = d.options.iter().find(|o| !d.multi && o.label.starts_with("Type something")).map(|o| o.n) else { return false };
        let words: String = words.split_whitespace().collect::<Vec<_>>().join(" ");
        self.dialog_send(
            vec![
                DialogStep { keys: n.to_string(), until: Some(DialogUntil::CursorOn(n)) },
                DialogStep { keys: words.clone(), until: Some(DialogUntil::Says(n, words)) },
                DialogStep { keys: "\r".into(), until: None },
            ],
            cx,
        );
        true
    }

    /// The interaction in the terminal is done: the window comes to the
    /// front, with what the status line says now.
    fn back_from_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(cb) = self.come_back.take() else { return };
        if let Some(id) = self.shown_session().map(|s| s.id.clone()) {
            self.refresh_context(&id);
        }
        self.read_terminal_mode(&cb.sid, cx);
        if self.own_terminal(&cb.sid) {
            self.term_dismissed = None;
            self.hide_terminal(cx);
        } else {
            cx.activate(true);
        }
        cx.notify();
    }

    // -- the hidden terminal -----------------------------------------------------

    /// A terminal of our own, with no window, is behind the session
    /// (`emaki_core::pty`, held by the hub).
    fn own_terminal(&self, sid: &str) -> bool {
        self.hub.terminal_for(sid).is_some()
    }

    /// The session is mid-turn in a terminal of the person's. The window
    /// does not go to that terminal for a mode, a pick or a command, and
    /// does not start a second Claude Code on a turn that is running:
    /// it says to wait, and answers true.
    fn theirs_busy(&mut self, r: &SessionRef, cx: &mut Context<Self>) -> bool {
        let busy = self.hub.hidden_terminals() && self.reply_via_for(r).0 == "inbox" && self.hub.peer_for(&r.session_id).is_some_and(|p| p.status != "idle");
        if busy {
            self.notice = Some(Notice::said("Claude is mid-turn in another terminal; this works here once the turn is over"));
            cx.notify();
        }
        busy
    }

    /// The line for the row under the composer while a terminal comes up.
    fn opening(&self, sid: &str) -> Notice {
        Notice::said(if self.own_terminal(sid) { "one moment…" } else { "opening the terminal, one moment…" })
    }

    /// The person has begun to say something on a session with no
    /// process behind it: its hidden terminal is started now, so it is
    /// up by the time they send, press ⇧Tab or click a pill. Not on
    /// opening the session: a resumed Claude Code writes to the
    /// transcript, and a conversation only read would move to the top
    /// of every list and read as live.
    fn warm_terminal(&mut self, cx: &mut Context<Self>) {
        if self.page != Page::Session || self.composer.read(cx).value().trim().is_empty() {
            return;
        }
        let Some(r) = self.selected_ref().cloned() else { return };
        if self.reply_via_for(&r).0 == "spawn" && self.hub.hidden_terminals() {
            let _ = self.hub.start_terminal(&r.session_id, &r.cwd, true, "", "");
        }
    }

    /// Draw the session's hidden terminal over the composer and give it
    /// the keyboard. `auto` says the window did it, for a screen it has
    /// no card for, and will put it away when the terminal stops waiting.
    fn show_terminal(&mut self, sid: &str, auto: bool, cx: &mut Context<Self>) {
        if !self.own_terminal(sid) {
            return;
        }
        self.term_open = Some(sid.to_string());
        self.term_auto = auto;
        self.term_focus_due = Some(true);
        self.notice = None;
        if let Some(keys) = self.term_keys_probe.take() {
            let hub = Arc::clone(&self.hub);
            let sid = sid.to_string();
            std::thread::spawn(move || {
                for key in keys.split(',') {
                    std::thread::sleep(Duration::from_millis(1500));
                    let bytes: &[u8] = match key.trim() {
                        "up" => b"\x1b[A",
                        "down" => b"\x1b[B",
                        "right" => b"\x1b[C",
                        "left" => b"\x1b[D",
                        "enter" => b"\r",
                        "esc" => b"\x1b",
                        "tab" => b"\t",
                        other => other.as_bytes(),
                    };
                    if let Some(pty) = hub.terminal_for(&sid) {
                        pty.write(bytes);
                    }
                }
            });
        }
        cx.notify();
    }

    fn hide_terminal(&mut self, cx: &mut Context<Self>) {
        if self.term_open.take().is_some() {
            self.term_auto = false;
            self.term_focus_due = Some(false);
            cx.notify();
        }
    }

    /// The card's close button: what the terminal shows is cancelled as
    /// Escape cancels it there, and the card is not brought back for the
    /// same wait.
    fn dismiss_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(sid) = self.term_open.clone() else { return };
        if self.hub.peer_for(&sid).is_some_and(|p| p.status == "waiting") {
            if let Some(pty) = self.hub.terminal_for(&sid) {
                pty.write(b"\x1b");
            }
            self.term_dismissed = Some(sid);
        }
        self.come_back = None;
        self.hide_terminal(cx);
    }

    /// A key pressed on the terminal card goes to Claude Code.
    fn term_key(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if let Some(pty) = self.term_open.as_ref().and_then(|sid| self.hub.terminal_for(sid)) {
            pty.write(bytes);
            cx.notify();
        }
    }

    /// The hidden terminal's screen changed. The card is drawn again at
    /// once; what the window reads off the screen (the mode, the working
    /// line, a dialog) is read a moment later, once for a burst.
    fn on_screen(&mut self, sid: String, cx: &mut Context<Self>) {
        if self.term_open.as_deref() == Some(sid.as_str()) {
            cx.notify();
        }
        if self.screen_due {
            return;
        }
        self.screen_due = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(120)).await;
            let _ = this.update(cx, |this, cx| {
                this.screen_due = false;
                this.terminal_changed(&sid, cx);
            });
        })
        .detach();
    }

    /// Something changed in the terminal of `sid`, the person's or our
    /// own: its status line ran, or its screen was drawn. What the
    /// window shows of it is read again.
    fn terminal_changed(&mut self, sid: &str, cx: &mut Context<Self>) {
        self.watch_terminal(cx);
        self.terminal_ready(cx);
        if self.selected_ref().is_some_and(|r| self.page == Page::Session && r.session_id == sid) {
            if self.refresh_context(sid) {
                cx.notify();
            }
            self.read_terminal_mode(sid, cx);
            self.read_working(cx);
            self.read_dialog(cx);
        }
    }

    /// The hidden terminal of the session showing went away by itself:
    /// said once, and nothing goes on waiting for it.
    fn terminal_lost(&mut self, cx: &mut Context<Self>) {
        let Some(sid) = self.selected_ref().filter(|_| self.page == Page::Session).map(|r| r.session_id.clone()) else { return };
        let Some(why) = self.hub.take_lost(&sid) else { return };
        if self.pending_terminal.as_ref().is_some_and(|(s, _, _)| *s == sid) {
            self.pending_terminal = None;
        }
        self.hide_terminal(cx);
        self.notice = Some(Notice::error(if why.is_empty() { "claude exited".to_string() } else { format!("claude exited: {why}") }));
        cx.notify();
    }

    /// Read the mode a terminal session is in off its screen
    /// (`sys::terminal_text`, `driver::mode_on_screen`, against the modes
    /// the agent lists), for the pill: the transcript knows the mode only
    /// as of the last prompt, and ⇧Tab since then shows nowhere else.
    /// Done when a session is opened, when the window comes back from its
    /// terminal, and every time the terminal's status line runs
    /// (`HubEvent::Context`), which a ⇧Tab makes it do, so the pill
    /// follows the key within a moment. Off the main thread, one read at
    /// a time: a run of the status line during a read is read after it.
    /// A terminal that cannot be read changes nothing.
    fn read_terminal_mode(&mut self, sid: &str, cx: &mut Context<Self>) {
        if self.driven(sid) {
            return;
        }
        let Some(peer) = self.hub.peer_for(sid) else { return };
        if self.mode_reading {
            self.mode_read_again = Some(sid.to_string());
            return;
        }
        let modes = self.hub.options_for(AgentId::ClaudeCode, &peer.cwd).modes;
        if modes.is_empty() {
            return;
        }
        self.mode_reading = true;
        let sid = sid.to_string();
        cx.spawn(async move |this, cx| {
            let mode = cx.background_spawn(async move { crate::sys::terminal_text(peer.pid).and_then(|t| emaki_core::driver::mode_on_screen(&t, &modes)) }).await;
            let _ = this.update(cx, |this, cx| {
                this.mode_reading = false;
                // A key pressed from here with no screen to read it back
                // from: the mode moved and is not known.
                let pressed = this.mode_pressed.as_deref() == Some(sid.as_str());
                if mode.is_some() || pressed {
                    if mode.is_some() {
                        this.mode_pressed = None;
                    }
                    let r = this.refs.iter().find(|r| r.session_id == sid);
                    let mtime = r.map(|r| r.mtime).unwrap_or(0.0);
                    // Another mode than the pill had: a change.
                    let before = match &this.mode_seen {
                        Some((s, m, _)) if *s == sid => m.clone(),
                        _ => r.map(|r| r.state.mode.clone()).filter(|m| !m.is_empty()),
                    };
                    if mode.is_some() && before.is_some() && mode != before {
                        this.touch_mode(&sid, mode.clone().unwrap_or_default());
                    }
                    let seen = Some((sid, mode, mtime));
                    if this.mode_seen != seen {
                        this.mode_seen = seen;
                        cx.notify();
                    }
                }
                if let Some(again) = this.mode_read_again.take() {
                    this.read_terminal_mode(&again, cx);
                }
            });
        })
        .detach();
    }

    fn terminal_check(&self, r: &SessionRef) -> Result<(), &'static str> {
        if r.archived {
            return Err("Kept only: the agent no longer has this transcript");
        }
        if !Self::folder_exists(r) {
            return Err(FOLDER_GONE);
        }
        if let Some(v) = self.drivers.get(&r.session_id) {
            if v.starting || v.state == "running" {
                return Err("Wait for the running reply, then open");
            }
        }
        Ok(())
    }

    /// Whether the session runs in a terminal of the person's. The
    /// registry alone cannot say: a headless child of ours registers an
    /// inbox too, and it has no terminal to go to (its parent is this
    /// app), so a driver behind the session is asked first, as
    /// `reply_via_for` does.
    fn in_terminal(&self, r: &SessionRef) -> bool {
        matches!(self.reply_via_for(r).0, "inbox" | "pty")
    }

    /// The first of the three buttons at the top right: continue the session
    /// showing in the person's own terminal, with the agent's resume command. See
    /// `emaki_core::terminal` and `sys::open_in_terminal`.
    pub fn open_in_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.selected_ref().cloned() else { return };
        // "Your terminal" is the default one, the app the system keeps
        // for shell scripts, and only that: a session running there
        // already, however it got there, is brought forward. One in any
        // other terminal (an IDE's, say) counts as not open.
        let ours = self.hub.terminal_for(&r.session_id).map(|t| t.pid);
        let theirs: Vec<emaki_core::peer::Peer> =
            emaki_core::peer::registry_all().into_iter().filter(|p| p.session_id == r.session_id && Some(p.pid) != ours).collect();
        if let Some(there) = theirs.iter().find(|p| crate::sys::in_default_terminal(p.pid)) {
            self.notice = Some(match crate::sys::focus_terminal(there.pid) {
                Ok(app) => Notice::said(format!("already open in {app}")),
                Err(e) => Notice::error(e),
            });
            cx.notify();
            return;
        }
        if let Err(why) = self.terminal_check(&r) {
            self.notice = Some(Notice::error(why));
            cx.notify();
            return;
        }
        // Not onto a turn that is running, here or anywhere.
        let ours_busy = ours.is_some() && self.hub.peer_for(&r.session_id).is_some_and(|p| p.status != "idle");
        if ours_busy || self.is_working(&r) || theirs.iter().any(|p| p.status == "busy") {
            self.notice = Some(Notice::error("Wait for the running reply, then open"));
            cx.notify();
            return;
        }
        // The hidden terminal stays as it is: the person's terminal is
        // theirs, and opening it changes nothing in the window.
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

    /// Put `text` on the clipboard, and have the button `key` show a tick
    /// for a moment.
    pub fn copy_text(&mut self, key: SharedString, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some(key.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            let _ = this.update(cx, |this, cx| {
                if this.copied.as_ref() == Some(&key) {
                    this.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn folder_exists(r: &SessionRef) -> bool {
        !r.cwd.is_empty() && std::path::Path::new(&r.cwd).is_dir()
    }

    /// The second button at the top right: the folder the session ran in,
    /// opened in the file manager.
    pub fn open_project_folder(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.selected_ref() else { return };
        if !Self::folder_exists(r) {
            self.notice = Some(Notice::error(FOLDER_GONE));
            cx.notify();
            return;
        }
        crate::sys::open_path(std::path::Path::new(&r.cwd));
    }

    /// Keep a scroll gesture's momentum in the pane it began in. macOS goes
    /// on sending wheel events after the finger lifts, and gpui hands each
    /// to whatever is under the pointer by then, so a flick in the
    /// conversation followed by a move to the sidebar scrolled the sidebar.
    /// This runs in the capture phase, before any scroll container: the pane
    /// under the pointer at `Started` (or at the first event after a pause,
    /// which is how a mouse wheel begins) owns the gesture, and an event
    /// that lands in the other pane is applied to the owner's scroll
    /// position and stopped. With the sidebar folded away there is one pane
    /// and nothing to do.
    fn route_scroll(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if std::env::var("EMAKI_SCROLL_DEBUG").is_ok() {
            eprintln!("scroll {:?} at ({:.0},{:.0}) delta {:?} owner {:?}", e.touch_phase, f32::from(e.position.x), f32::from(e.position.y), e.delta, self.scroll_owner);
        }
        let now = Instant::now();
        let fresh = now.duration_since(self.last_scroll) > SCROLL_GAP;
        self.last_scroll = now;
        let inline_sidebar = (self.sidebar_open && !self.narrow) || self.sidebar_float;
        let here = if inline_sidebar && e.position.x < SIDEBAR_W { Pane::Sidebar } else { Pane::Content };
        match e.touch_phase {
            TouchPhase::Started => self.scroll_owner = Some(here),
            TouchPhase::Moved if fresh || self.scroll_owner.is_none() => self.scroll_owner = Some(here),
            _ => {}
        }
        let Some(owner) = self.scroll_owner else { return };
        if owner == here || !inline_sidebar {
            return;
        }
        let delta = e.delta.pixel_delta(px(20.));
        match owner {
            Pane::Sidebar => self.side_scroll.set_offset(self.side_scroll.offset() + delta),
            Pane::Content => match self.page {
                Page::Session => {
                    if let Some(d) = &self.detail {
                        d.list.scroll_by(-delta.y);
                    }
                }
                Page::Sessions => self.sessions_scroll.set_offset(self.sessions_scroll.offset() + delta),
                Page::New => self.home_scroll.set_offset(self.home_scroll.offset() + delta),
                Page::Board => {}
            },
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// A headless child of ours is behind the session: the window has
    /// been told of one, and the hub still holds it. The hub lets a
    /// driver go by itself (idle past its limit, or its child gone, as
    /// when the session was taken up in a terminal) and says nothing, so
    /// the window's record alone outlived it: the session then read as
    /// driven, every message was refused with "the driver is gone; try
    /// again", and the terminal's inbox beside it was never tried.
    fn driven(&self, sid: &str) -> bool {
        self.drivers.get(sid).is_some_and(|v| v.starting || (v.state != "exited" && self.hub.driver_for(sid).is_some()))
    }

    pub fn reply_via_for(&self, r: &SessionRef) -> (&'static str, String) {
        if r.agent != AgentId::ClaudeCode {
            return ("", format!("{} sessions are read-only here", r.agent.display_name()));
        }
        if self.drivers.get(&r.session_id).is_some_and(|v| v.starting) {
            return ("driver", "starting claude…".into());
        }
        if self.driven(&r.session_id) {
            return ("driver", String::new());
        }
        // A terminal of our own behind the session: the message is typed
        // at its prompt. Asked before the registry, where its Claude Code
        // has an inbox like any other.
        if self.own_terminal(&r.session_id) {
            return ("pty", String::new());
        }
        // A terminal session with an inbox takes the message directly; the
        // driver is checked first because its child registers an inbox too.
        // Only while a turn is running there, when the hidden terminal is
        // how sessions run here: between turns the person's terminal is
        // left alone, the session is taken up on a hidden one like any
        // session with no process (`spawn` below), and nothing is sent,
        // typed or ended in theirs.
        if self.hub.peer_for(&r.session_id).is_some_and(|p| !(self.hub.hidden_terminals() && p.status == "idle")) {
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
        let mut typed = self.composer.read(cx).value().to_string();
        // An empty box with a suggested prompt in it sends the prompt,
        // as Return does in the terminal.
        if typed.is_empty() && self.attachments.is_empty() {
            if let Some(words) = self.suggested() {
                typed = words.to_string();
                self.suggestion = None;
            }
        }
        if typed.trim().is_empty() && self.attachments.is_empty() {
            return;
        }
        let (via, why) = self.reply_via();
        // A slash command reaches a headless session as the command it is
        // (`/compact` runs; Claude Code marks it as fine without a
        // terminal). A terminal session's inbox hands everything to the
        // model as words from a peer, checked on the wire: there the
        // command is refused here rather than spent as a prompt.
        if slash_command(typed.trim()).is_some() {
            if via == "inbox" || via == "pty" {
                self.run_in_terminal(typed.trim().to_string(), cx);
                self.composer.update(cx, |s, cx| s.set_value("", window, cx));
                cx.notify();
                return;
            }
        } else if let Some(q) = self.question_pending() {
            // Words typed while Claude is asking are the answer.
            if self.attachments.is_empty() {
                self.answer_question_typed(q, typed.trim().to_string(), window, cx);
                return;
            }
        } else if self.attachments.is_empty() && self.dialog_answer_typed(typed.trim(), cx) {
            // The same for a question the terminal holds.
            self.composer.update(cx, |s, cx| s.set_value("", window, cx));
            cx.notify();
            return;
        }
        // A session with no process behind it, or none yet: the process is
        // an interactive Claude Code on a terminal of our own, and the
        // message is typed there. Only when that cannot be started (or
        // the setting says so) is it the headless child it used to be.
        let (new_id, new_cwd) = if via == "spawn" && self.page == Page::New {
            if self.new_id.is_empty() {
                self.new_id = uuid::Uuid::new_v4().to_string();
            }
            (self.new_id.clone(), self.new_cwd.clone())
        } else {
            (String::new(), String::new())
        };
        let via = if via != "spawn" {
            via
        } else {
            let (sid, cwd, resume) = match self.selected_ref().filter(|_| new_id.is_empty()) {
                Some(r) => (r.session_id.clone(), r.cwd.clone(), true),
                None => (new_id.clone(), new_cwd, false),
            };
            let started = self.hub.hidden_terminals() && self.hub.start_terminal(&sid, &cwd, resume, &self.next_mode, &self.next_model).is_ok();
            if started { "pty" } else { "spawn" }
        };
        let (text, images) = self.fold_attachments(&typed, via == "driver" || via == "spawn");
        match via {
            "inbox" => {
                let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
                self.hub.send_to_inbox(&sid, text);
            }
            "pty" => {
                let sid = if new_id.is_empty() { self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default() } else { new_id.clone() };
                // A terminal waiting on an answer would take the words
                // for its dialog.
                if self.hub.peer_for(&sid).is_some_and(|p| p.status == "waiting") {
                    self.notice = Some(Notice::error("Claude is waiting for an answer first"));
                    cx.notify();
                    return;
                }
                // Claude Code takes a picture as the path of its file,
                // pasted by itself; any other file stays a line of the
                // message, as on the inbox.
                let pictures: Vec<PathBuf> = self.attachments.iter().filter(|a| a.image).map(|a| a.path.clone()).collect();
                let words = {
                    let lines: Vec<String> = self.attachments.iter().filter(|a| !a.image).map(|a| format!("Attached file: {}", a.path.display())).collect();
                    let mut out = typed.trim_end().to_string();
                    if !lines.is_empty() {
                        if !out.trim().is_empty() {
                            out.push_str("\n\n");
                        }
                        out.push_str(&lines.join("\n"));
                    }
                    out
                };
                self.hub.send_to_terminal(&sid, words, pictures);
                if !new_id.is_empty() {
                    let key = format!("claude-code:{sid}");
                    self.pending_select = Some(key.clone());
                    self.selected = Some(key);
                    self.new_id = String::new();
                    self.page = Page::Session;
                    self.notice = Some(Notice::said("starting claude…"));
                }
                let _ = text;
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
        self.last_sent = self.selected.clone().map(|key| (key, typed.trim().to_string(), self.attachments.clone()));
        self.composer.update(cx, |s, cx| s.set_value("", window, cx));
        self.attachments.clear();
        cx.notify();
    }

    /// Hand a stopped turn's prompt back: its words in the composer with
    /// the caret after them, and what was attached on the chips again, as
    /// Claude Code's own terminal does on Escape, and only when it does:
    /// when the turn was stopped before the agent wrote or ran anything.
    /// Once the agent has started it has the message, the round stays in
    /// the conversation, and the composer is left for what comes next.
    /// The message as it left
    /// this window when it was the last thing sent to this session
    /// (pictures included, which a driver takes as blocks and the
    /// transcript keeps no path for); else what the transcript's last
    /// round holds, files and kept pictures by their paths. Nothing is
    /// touched when the composer already has something in it.
    fn restore_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_draft(window, cx);
        if !self.composer.read(cx).value().trim().is_empty() || !self.attachments.is_empty() {
            return;
        }
        // The prompt the stop withdrew; or, when the marker of the stop
        // is not in the transcript yet (a driver is stopped at the
        // click), the last round while it is still only a prompt, by the
        // rule the withdrawal goes by (`build::handle_user`).
        let untouched = |r: &&emaki_core::model::Round| !r.items.iter().any(|it| !matches!(it, Item::Notice { .. }));
        let Some(rnd) = self.shown_session().and_then(|s| s.withdrawn.as_ref().or_else(|| s.rounds.iter().rev().find(|r| !r.queued).filter(untouched))) else {
            return;
        };
        if !matches!(rnd.source, emaki_core::model::Source::User | emaki_core::model::Source::Web) {
            return;
        }
        let prompt = rnd.prompt.trim().to_string();
        // A command that acts by itself (`/compact`) is not a message to
        // hand back: it has no reply, so its round is only ever a prompt,
        // and one that ran to its end read as stopped for a scan whenever
        // the registry said idle before its rows were in the file.
        if prompt.strip_prefix('/').is_some_and(|rest| acts_alone(rest.split_whitespace().next().unwrap_or(""))) {
            return;
        }
        let from_round: Vec<PathBuf> = rnd.attachments.iter().filter(|a| !a.path.is_empty()).map(|a| PathBuf::from(&a.path)).collect();
        let sent = self.last_sent.clone().filter(|(key, text, _)| Some(key) == self.selected.as_ref() && *text == prompt);
        match sent {
            Some((_, _, attached)) => self.attachments = attached.into_iter().filter(|a| a.path.is_file()).collect(),
            None => self.attach_paths(&from_round, cx),
        }
        if !prompt.is_empty() {
            let end = prompt.lines().count().saturating_sub(1) as u32;
            let col = prompt.lines().last().map(|l| l.encode_utf16().count()).unwrap_or(0) as u32;
            self.composer.update(cx, |s, cx| {
                s.set_value(prompt.clone(), window, cx);
                s.set_cursor_position(gpui_component::input::Position::new(end, col), window, cx);
            });
            self.mark_slash(cx);
        }
        self.notice = Some(Notice::said("stopped; your message is back in the box"));
        cx.notify();
    }

    /// Whose composer is showing: the session's, or the new-session
    /// page's. None on a page with no composer, where the textarea keeps
    /// what it had.
    fn draft_key(&self) -> Option<String> {
        match self.page {
            Page::Session => self.selected.clone(),
            Page::New => Some(String::new()),
            _ => None,
        }
    }

    /// One textarea stands for every composer, so on arriving somewhere
    /// else what it holds is put away under where it was typed, and what
    /// was left typed here is brought back, caret at the end (setting the
    /// value is not a change event, so it starts no terminal). Called at
    /// every draw, and before anything that looks at the composer on
    /// behalf of a session. It was one box for the whole window: words
    /// typed for one session followed the person into the next.
    fn sync_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.draft_key() else { return };
        if self.draft_of.as_ref() == Some(&key) {
            return;
        }
        // The first composer of the launch takes what is there (`EMAKI_TYPE`).
        let Some(old) = self.draft_of.replace(key.clone()) else { return };
        let text = self.composer.read(cx).value().to_string();
        let attached = std::mem::take(&mut self.attachments);
        if text.trim().is_empty() && attached.is_empty() {
            self.drafts.remove(&old);
        } else {
            self.drafts.insert(old, (text, attached));
        }
        let (text, attached) = self.drafts.remove(&key).unwrap_or_default();
        self.attachments = attached.into_iter().filter(|a| a.path.is_file()).collect();
        let end = text.lines().count().saturating_sub(1) as u32;
        let col = text.lines().last().map(|l| l.encode_utf16().count()).unwrap_or(0) as u32;
        self.composer.update(cx, |s, cx| {
            s.set_value(text, window, cx);
            s.set_cursor_position(gpui_component::input::Position::new(end, col), window, cx);
        });
        self.mark_slash(cx);
    }

    /// Escape with nothing to close stops the turn running on the session
    /// showing, as it does in the terminal and as the Stop pill does.
    /// False when there is no turn to stop.
    fn escape_stops(&mut self, cx: &mut Context<Self>) -> bool {
        let working = self.page == Page::Session && self.selected_ref().is_some_and(|r| self.is_working(r));
        if working {
            self.interrupt(cx);
        }
        working
    }

    /// What Escape closes, nearest first: the lightbox, then the search;
    /// with nothing open, it stops the running turn.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.menu = None;
            cx.notify();
        } else if self.renaming.is_some() {
            self.close_rename(window, cx);
        } else if self.settings_open {
            self.settings_open = false;
            cx.notify();
        } else if self.lightbox.is_some() {
            self.lightbox = None;
            cx.notify();
        } else if self.search_open {
            self.close_search(window, cx);
        } else if self.find_open {
            self.close_find(window, cx);
        } else if self.term_open.is_some() {
            // The terminal card is up: Escape is Claude Code's, wherever
            // the keyboard is in the window, and cancels what it shows.
            self.term_key(b"\x1b", cx);
        } else {
            self.escape_stops(cx);
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

    /// What the agent offers the session showing, or the folder a new one
    /// would start in: its modes, its models, their effort levels
    /// (`Hub::options_for`). The lists under the pills are these.
    pub(crate) fn options(&self) -> Options {
        match self.selected_ref().filter(|_| self.page == Page::Session) {
            Some(r) => self.hub.options_for(r.agent, &r.cwd),
            None => self.hub.options_for(AgentId::ClaudeCode, &self.new_cwd),
        }
    }

    /// The modes the picker offers: every one the agent lists, the one
    /// that asks nothing only when config allows it.
    fn modes(&self, options: &Options) -> Vec<Choice> {
        let allow_bypass = self.cfg.driver.allow_bypass;
        options.modes.iter().filter(|m| allow_bypass || m.key != "bypassPermissions").cloned().collect()
    }

    /// The mode the session showing is in when its transcript does not
    /// say so yet: changed in the terminal or through the driver since the
    /// last turn, which the file only learns with the next one. The
    /// conversation says it at its foot until then (`render_round`).
    /// Every change since then, each with when it was made where that is
    /// known: the lines take their places among the round's other lines
    /// by that time, so mode, effort, mode reads in that order.
    pub(crate) fn unwritten_modes(&self) -> Vec<(String, Option<f64>)> {
        if let (Some(r), Some((sid, changes))) = (self.selected_ref(), &self.mode_touched) {
            if *sid == r.session_id && self.unwritten_mode().is_some() {
                return changes.iter().map(|(mode, at)| (mode.clone(), Some(*at))).collect();
            }
        }
        self.unwritten_mode().into_iter().collect()
    }

    /// A change of mode on `sid` seen just now, from here or in its
    /// terminal.
    fn touch_mode(&mut self, sid: &str, mode: String) {
        match &mut self.mode_touched {
            Some((s, changes)) if s == sid => changes.push((mode, now_secs())),
            _ => self.mode_touched = Some((sid.to_string(), vec![(mode, now_secs())])),
        }
    }

    fn unwritten_mode(&self) -> Option<(String, Option<f64>)> {
        let written = &self.shown_session()?.mode;
        let r = self.selected_ref()?;
        if written.is_empty() {
            return None;
        }
        let live = match self.terminal_mode_seen() {
            Some(seen) => seen?,
            None => self.drivers.get(&r.session_id).map(|v| v.mode.clone()).filter(|m| !m.is_empty()).or_else(|| Some(r.state.mode.clone()).filter(|m| !m.is_empty()))?,
        };
        // Stepped away and back to where the transcript has it is still a
        // change the person made, and its line stays until the next turn.
        let touched = self.mode_touched.as_ref().filter(|(sid, _)| *sid == r.session_id).and_then(|(_, changes)| changes.last().map(|(_, at)| *at));
        (live != *written || touched.is_some()).then_some((live, touched))
    }

    /// The session showing, once loaded.
    /// Read what the status line left about `session`'s context window,
    /// and keep its size for the model too. Returns whether it changed.
    fn refresh_context(&mut self, session: &str) -> bool {
        let next = emaki_core::limits::session_context(session).map(|c| (session.to_string(), c));
        if next == self.session_ctx {
            return false;
        }
        if let Some((_, c)) = &next {
            if self.limits.learn_window(c) {
                let _ = self.limits.save();
            }
        }
        self.session_ctx = next;
        true
    }

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
        if let Some(seen) = self.terminal_mode_seen() {
            return seen.unwrap_or_default();
        }
        let m = self
            .drivers
            .get(&sid)
            .map(|v| v.mode.clone())
            .filter(|m| !m.is_empty())
            .or_else(|| r.filter(|_| self.page == Page::Session).map(|r| r.state.mode.clone()).filter(|m| !m.is_empty()))
            .unwrap_or_else(|| self.next_mode.clone());
        if m.is_empty() { "default".into() } else { m }
    }

    /// The mode the session showing was left in by a change in its
    /// terminal, when there was one since its last turn: the mode, or
    /// none when the terminal could not be read, where the pill names no
    /// mode rather than the transcript's old one.
    fn terminal_mode_seen(&self) -> Option<Option<String>> {
        let r = self.selected_ref().filter(|_| self.page == Page::Session)?;
        if self.driven(&r.session_id) {
            return None;
        }
        self.mode_seen.as_ref().filter(|(sid, _, _)| *sid == r.session_id).map(|(_, mode, _)| mode.clone())
    }

    /// The model, as the driver reports it (a full id after its first
    /// turn), else the transcript's last, else what was asked for.
    fn current_model(&self) -> String {
        if let Some(m) = self.terminal_ctx().map(|c| c.model.clone()).filter(|m| !m.is_empty()) {
            return m;
        }
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
        if let Some(e) = self.terminal_ctx().map(|c| c.effort.clone()).filter(|e| !e.is_empty()) {
            return e;
        }
        self.shown_session().map(|s| s.effort.clone()).unwrap_or_default()
    }

    fn set_effort(&mut self, effort: &str, cx: &mut Context<Self>) {
        if self.terminal_peer().is_some() {
            self.ctx_watch_until = self.now + 20.0;
            return self.run_in_terminal(format!("/effort {effort}"), cx);
        }
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        if self.hub.set_driver_effort(&sid, effort.to_string()) {
            self.notice = Some(Notice::said(format!("setting {}…", format!("{} effort", self.options().effort_label(&self.current_model(), effort)).to_lowercase())));
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
        let own = self.session_ctx.as_ref().filter(|(id, _)| *id == s.id).map(|(_, c)| c);
        let window = self.limits.window_for(model, s.context_tokens, own);
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
        if self.terminal_peer().is_some() {
            return self.pick_in_terminal(Pill::Mode, cx);
        }
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        self.next_mode = mode.to_string();
        if self.hub.set_driver_mode(&sid, mode.to_string()) {
            self.touch_mode(&sid, mode.to_string());
            if let Some(v) = self.drivers.get_mut(&sid) {
                v.mode = mode.to_string();
            }
        }
        cx.notify();
    }

    /// ⇧Tab in the composer, as in Claude Code's own terminal. A driven
    /// session, or one not started, steps down the agent's list. A
    /// terminal session is stepped by its own key (`step_mode`).
    fn cycle_mode(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.selected_ref().filter(|_| self.page == Page::Session).cloned() {
            if self.theirs_busy(&r, cx) {
                return;
            }
        }
        if let Some((sid, peer)) = self.terminal_peer() {
            return self.step_mode(sid, peer, cx);
        }
        // A session with no process behind it: its terminal is opened,
        // and the key pressed there once it is up.
        if self.page == Page::Session && self.selected_ref().is_some_and(|r| !self.driven(&r.session_id)) {
            return self.via_terminal(TerminalAction::StepMode, cx);
        }
        let modes = self.modes(&self.options());
        if modes.is_empty() {
            return;
        }
        let current = self.current_mode();
        let i = modes.iter().position(|m| m.key == current).map(|i| (i + 1) % modes.len()).unwrap_or(0);
        self.set_mode(&modes[i].key, cx);
    }

    /// ⇧Tab from the composer, for a terminal session: the same key,
    /// pressed once in its terminal, which is the only thing that moves
    /// its mode. The order is Claude Code's, whatever it is for that
    /// session, so nothing here counts presses or aims at a mode: one
    /// press for one press, and the pill shows where it landed. WezTerm,
    /// Kaku and iTerm2 take the key for the pane without coming forward;
    /// any other host comes forward for it and the window takes the
    /// front back at once (`sys::key_in_terminal`). The key makes the
    /// status line run, which is what has the screen read; it is asked
    /// for twice more in case that run is slow or does not come. Where
    /// the screen cannot be read (an IDE's terminal) the pill names no
    /// mode until the next turn, rather than the one from before the key.
    fn step_mode(&mut self, sid: String, peer: emaki_core::peer::Peer, cx: &mut Context<Self>) {
        self.come_back = None;
        self.mode_pressed = Some(sid.clone());
        let hub = Arc::clone(&self.hub);
        std::thread::spawn(move || match crate::sys::key_in_terminal(peer.pid, crate::sys::TerminalKey::ShiftTab) {
            Ok(_) => {
                for wait in [250, 600] {
                    std::thread::sleep(Duration::from_millis(wait));
                    hub.look_at(&sid);
                }
            }
            Err(e) => hub.say(e),
        });
        cx.notify();
    }

    fn set_model(&mut self, model: &str, cx: &mut Context<Self>) {
        if self.terminal_peer().is_some() {
            self.ctx_watch_until = self.now + 20.0;
            return self.run_in_terminal(format!("/model {model}"), cx);
        }
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        self.next_model = model.to_string();
        if self.hub.set_driver_model(&sid, model.to_string()) {
            if let Some(v) = self.drivers.get_mut(&sid) {
                v.model = model.to_string();
            }
        }
        cx.notify();
    }

    /// Stop the turn running on the session showing, and hand its prompt
    /// back (`restore_prompt`). A driver of ours takes an `interrupt`
    /// request. A terminal session is stopped the way the person stops
    /// it, with Escape in its terminal (`sys::key_in_terminal`); the
    /// registry then says idle and the next scan, asked for at once,
    /// shows it stopped.
    fn interrupt(&mut self, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        if let Some(d) = self.hub.driver_for(&sid) {
            std::thread::spawn(move || {
                let _ = d.interrupt();
            });
            self.restore_due = true;
        } else if let Some(peer) = self.hub.peer_for(&sid) {
            let hub = Arc::clone(&self.hub);
            self.notice = Some(Notice::said("stopping…"));
            std::thread::spawn(move || match crate::sys::key_in_terminal(peer.pid, crate::sys::TerminalKey::Escape) {
                Ok(_) => {
                    // The registry flips a moment after the key lands.
                    for wait in [400, 1200] {
                        std::thread::sleep(Duration::from_millis(wait));
                        hub.refresh();
                    }
                }
                Err(e) => hub.say(e),
            });
        } else {
            self.notice = Some(Notice::error("nothing here can stop it: the session has no terminal and no driver"));
        }
        cx.notify();
    }

    /// What the terminal's status line last said of the session showing,
    /// when that session is in a terminal and not driven from here.
    fn terminal_ctx(&self) -> Option<&SessionContext> {
        let r = self.selected_ref().filter(|_| self.page == Page::Session)?;
        if self.driven(&r.session_id) {
            return None;
        }
        self.session_ctx.as_ref().filter(|(id, _)| *id == r.session_id).map(|(_, c)| c)
    }

    /// The terminal session behind the composer, when a message from here
    /// would go to its inbox: the one whose mode, model and effort are
    /// changed in the terminal.
    fn terminal_peer(&self) -> Option<(String, emaki_core::peer::Peer)> {
        if !matches!(self.reply_via().0, "inbox" | "pty") {
            return None;
        }
        let sid = self.selected_ref()?.session_id.clone();
        self.hub.peer_for(&sid).map(|p| (sid, p))
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
        // A question is never answered by a bare ↩: it takes a choice or words.
        let Some(id) = self.permissions.iter().find(|(s, p)| *s == sid && !p.is_question()).map(|(_, p)| p.request_id.clone()) else { return false };
        // The textarea put a new line in before saying ↩ was pressed.
        self.composer.update(cx, |s, cx| s.set_value("", window, cx));
        self.answer_permission(id, allow, cx);
        true
    }

    /// Every card waiting on this session, allowed at once.
    fn allow_all_pending(&mut self, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let ids: Vec<String> = self.permissions.iter().filter(|(s, p)| *s == sid && !p.is_question()).map(|(_, p)| p.request_id.clone()).collect();
        for id in ids {
            self.answer_permission(id, true, cx);
        }
    }

    /// The oldest question waiting on the session showing, when a driver
    /// of ours holds one. A terminal session's questions are its own.
    pub fn question_pending(&self) -> Option<PermissionRequest> {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        self.permissions.iter().find(|(s, p)| *s == sid && p.is_question()).map(|(_, p)| p.clone())
    }

    fn answer_question_with(&mut self, request_id: String, answers: serde_json::Map<String, serde_json::Value>, cx: &mut Context<Self>) {
        let Some((sid, _)) = self.permissions.iter().find(|(_, r)| r.request_id == request_id).cloned() else { return };
        if let Some(d) = self.hub.driver_for(&sid) {
            let rid = request_id.clone();
            std::thread::spawn(move || d.answer_question(&rid, answers));
        }
        self.permissions.retain(|(_, r)| r.request_id != request_id);
        self.picks.remove(&request_id);
        cx.notify();
    }

    /// A click on an option: the answer at once when the card holds one
    /// question with one choice, else a pick the card's button sends.
    fn pick_option(&mut self, request_id: String, question: String, label: String, multi: bool, cx: &mut Context<Self>) {
        let Some((_, req)) = self.permissions.iter().find(|(_, r)| r.request_id == request_id).cloned() else { return };
        let single = questions_of(&req.input).len() == 1 && !multi;
        let entry = self.picks.entry(request_id.clone()).or_default().entry(question).or_default();
        if multi {
            match entry.iter().position(|l| *l == label) {
                Some(i) => {
                    entry.remove(i);
                }
                None => entry.push(label),
            }
        } else {
            *entry = vec![label];
        }
        if single {
            self.submit_question(request_id, cx);
        } else {
            cx.notify();
        }
    }

    fn submit_question(&mut self, request_id: String, cx: &mut Context<Self>) {
        let picks = self.picks.get(&request_id).cloned().unwrap_or_default();
        let mut answers = serde_json::Map::new();
        for (q, labels) in picks {
            if !labels.is_empty() {
                answers.insert(q, serde_json::Value::String(labels.join(", ")));
            }
        }
        self.answer_question_with(request_id, answers, cx);
    }

    /// Words typed while a question waits: they answer the first question
    /// without a pick, and the picks stand for the rest.
    fn answer_question_typed(&mut self, req: PermissionRequest, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let qs = questions_of(&req.input);
        let picks = self.picks.get(&req.request_id).cloned().unwrap_or_default();
        let mut answers = serde_json::Map::new();
        let mut placed = false;
        for q in &qs {
            match picks.get(&q.question).filter(|l| !l.is_empty()) {
                Some(labels) => {
                    answers.insert(q.question.clone(), serde_json::Value::String(labels.join(", ")));
                }
                None if !placed => {
                    answers.insert(q.question.clone(), serde_json::Value::String(text.clone()));
                    placed = true;
                }
                None => {}
            }
        }
        if !placed {
            if let Some(q) = qs.first() {
                answers.insert(q.question.clone(), serde_json::Value::String(text));
            }
        }
        self.composer.update(cx, |s, cx| s.set_value("", window, cx));
        self.answer_question_with(req.request_id, answers, cx);
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
        let page = self.page;
        let scope = self.scope.clone();

        // The app's own icon, the one in the Dock, and its name in a light,
        // elegant sans at regular weight (`fonts::wordmark_family`). The PNG keeps Apple's margin
        // around the plate, so the box is larger than what shows. On macOS the pair has a row of
        // its own under the traffic lights, in line with the entries below
        // it; elsewhere nothing holds the corner, so it sits in the strip.
        let brand = h_flex()
            .h(px(36.))
            .gap(px(5.))
            .items_center()
            .child(img("icon/app.png").size(px(30.)).flex_shrink_0())
            // Optima has a regular and a bold and nothing between: the
            // regular read as thin in the accent's colour and the bold as
            // thick. The name is drawn twice, half a pixel apart, which
            // lands between the two.
            .child(
                div()
                    .relative()
                    .text_size(px(22.))
                    .text_color(theme.primary)
                    .when_some(crate::fonts::wordmark_family(cx), |d, f| d.font_family(f))
                    .child("Emaki")
                    .child(div().absolute().top_0().left(px(0.5)).child("Emaki")),
            );

        // The strip above the brand is the window's buttons (`render_strip`),
        // drawn over this, so here it is only room.
        let header = Self::drag_region(div().h(TITLEBAR_H).flex_shrink_0(), cx);
        let top = v_flex().px(px(10.)).pt(px(2.)).gap(px(2.)).child(brand.pl(px(7.)).mb(px(6.)));

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
        let top = top
            .child(new_row)
            .child(nav("nav-board", IconName::LayoutDashboard, "Board", "⌘B", page == Page::Board, cx, Box::new(|this, _, cx| {
                this.page = Page::Board;
                cx.notify();
            })))
            .child(nav("nav-sessions", IconName::Inbox, "Sessions", "⌘L", sessions_active && scope == Scope::All, cx, Box::new(|this, _, cx| this.show_sessions(Scope::All, cx))));

        let mut agents: Vec<(AgentId, usize)> = Vec::new();
        for a in AgentId::ALL {
            let n = self.refs.iter().filter(|r| r.agent == a).count();
            if n > 0 {
                agents.push((a, n));
            }
        }
        // The rows sit in a column of their own inside the scroller. As
        // the scroller's children they were flex items of a column too
        // short for them, and every row gave up height to fit, down to
        // its text: 30px rows drew at about 21, and at 24 with a folder
        // closed, so the list changed its spacing as a folder opened.
        let mut scroll = v_flex().flex_shrink_0();
        scroll = scroll.child(self.group_label("Agents", cx));
        scroll = scroll.children(agents.into_iter().map(|(a, n)| {
            let active = sessions_active && scope == Scope::Agent(a);
            let theme = cx.theme().clone();
            let live = self.refs.iter().filter(|r| r.agent == a && self.live_color(r, cx).is_some()).count();
            h_flex()
                .id(SharedString::from(format!("agent-{}", a.as_str())))
                .h(SIDE_ROW_H)
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
                    .h(SIDE_ROW_H)
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
        // Folders: every project as a row that opens onto its sessions,
        // newest folder first, the sessions under each headed by when
        // (today, yesterday, this week, this month, earlier). A folder
        // wears what its sessions wear: its icon takes the agent's colour
        // while one of them is live, and the dot at its right is the most
        // pressing of theirs (needs you, then working, then your turn).
        // A session's mark is in the muted ink unless it is live, so a
        // column of rows does not read as so many accents; a live one
        // wears its agent's colour and turns while it works.
        let mut folders: Vec<(String, Vec<SessionRef>)> = Vec::new();
        for r in &self.refs {
            let p = r.project();
            match folders.iter_mut().find(|(name, _)| *name == p) {
                Some((_, list)) => list.push(r.clone()),
                None => folders.push((p, vec![r.clone()])),
            }
        }
        if !folders.is_empty() {
            scroll = scroll.child(self.group_label("Folders", cx));
        }
        let more_folders = folders.len().saturating_sub(SIDE_FOLDERS);
        for (p, list) in folders.into_iter().take(SIDE_FOLDERS) {
            let theme = cx.theme().clone();
            let open = self.folder_open(&p);
            let live: Vec<(&SessionRef, Column)> = list.iter().filter(|r| self.live_color(r, cx).is_some()).map(|r| (r, self.card_for(r).column)).collect();
            let dot = Column::LIVE.iter().find(|c| live.iter().any(|(_, col)| col == *c)).map(|c| self.column_color(*c, cx));
            let tint = live.first().map(|(r, _)| agent_color(r.agent, &theme)).unwrap_or(theme.muted_foreground);
            let name = p.clone();
            // The folder itself, from the newest session that still has it.
            let folder_cwd = list.iter().map(|r| r.cwd.clone()).find(|c| !c.is_empty() && std::path::Path::new(c).is_dir());
            scroll = scroll.child(
                h_flex()
                    .id(SharedString::from(format!("folder-{p}")))
                    .h(SIDE_ROW_H)
                    .px(px(10.))
                    .gap(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_folder(&name, cx)))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, _, cx| this.open_menu(ev.position, vec![(crate::sys::OPEN_FOLDER_LABEL, MenuDo::OpenFolder(folder_cwd.clone()))], cx)))
                    .child(div().w(px(22.)).flex().justify_center().child(Icon::new(if open { IconName::FolderOpen } else { IconName::Folder }).with_size(px(15.)).text_color(tint)))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(p.clone()))
                    .when_some(dot, |d, c| d.child(div().size(px(7.)).rounded_full().bg(c).flex_shrink_0()))
                    .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(list.len().to_string())),
            );
            let anim = self.folder_anim.as_ref().filter(|(name, _, at, _)| *name == p && at.elapsed() < FOLDER_ANIM).map(|(_, opening, _, serial)| (*opening, *serial));
            if !open && anim.is_none() {
                continue;
            }
            // The sessions, in a box whose height is the sum of its rows,
            // so opening and closing can run that height up and down.
            let more = list.len().saturating_sub(FOLDER_ROWS);
            let mut body = v_flex().overflow_hidden();
            let mut body_h = 0.;
            let mut last_bucket = "";
            for r in list.into_iter().take(FOLDER_ROWS) {
                let b = bucket(r.mtime);
                if b != last_bucket {
                    let h = if last_bucket.is_empty() { 18. } else { 24. };
                    body = body.child(h_flex().h(px(h)).flex_shrink_0().items_end().pl(px(42.)).pb(px(2.)).text_size(px(11.)).text_color(theme.muted_foreground.opacity(0.7)).child(b));
                    body_h += h;
                    last_bucket = b;
                }
                let key = key_of(&r);
                let active = page == Page::Session && self.selected.as_deref() == Some(key.as_str());
                let dot = self.live_color(&r, cx);
                let working = self.is_working(&r);
                let glyph_id = SharedString::from(format!("recent-glyph-{key}"));
                let glyph_color = if dot.is_some() { agent_color(r.agent, &theme) } else { theme.muted_foreground.opacity(0.75) };
                let (menu_key, menu_path) = (key.clone(), r.path.clone());
                body = body.child(
                    h_flex()
                        .id(SharedString::from(format!("recent-{key}")))
                        .h(SIDE_SESSION_H)
                        .flex_shrink_0()
                        .ml(px(18.))
                        .px(px(10.))
                        .gap(px(8.))
                        .rounded(px(7.))
                        .cursor_pointer()
                        .when(active, |d| d.bg(theme.sidebar_accent))
                        .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                        .on_click(cx.listener(move |this, _, window, cx| this.open_and_focus(&key, window, cx)))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| this.open_menu(ev.position, vec![("Rename", MenuDo::Rename(menu_key.clone())), (crate::sys::REVEAL_LABEL, MenuDo::Reveal(menu_path.clone()))], cx)),
                        )
                        .child(div().w(px(18.)).flex().justify_center().child(agent_glyph(r.agent, px(13.), glyph_color, working, glyph_id)))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(if active { theme.foreground } else { theme.sidebar_foreground }).child(r.title.clone()))
                        .when_some(dot, |d, c| d.child(div().size(px(7.)).rounded_full().bg(c).flex_shrink_0()))
                        .when(r.archived && dot.is_none(), |d| d.child(Icon::new(IconName::HardDrive).with_size(px(12.)).text_color(theme.muted_foreground.opacity(0.7)))),
                );
                body_h += f32::from(SIDE_SESSION_H);
            }
            if more > 0 {
                let all = p.clone();
                body = body.child(
                    h_flex()
                        .id(SharedString::from(format!("folder-more-{p}")))
                        .h(SIDE_SESSION_H)
                        .flex_shrink_0()
                        .ml(px(18.))
                        .pl(px(36.))
                        .rounded(px(7.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                        .on_click(cx.listener(move |this, _, _, cx| this.show_sessions(Scope::Project(all.clone()), cx)))
                        .child(format!("{more} more")),
                );
                body_h += f32::from(SIDE_SESSION_H);
            }
            scroll = scroll.child(match anim {
                // Opening runs the box from nothing to its height, closing
                // back again, the rows fading with it; the id carries the
                // click's number so each one plays once.
                Some((opening, serial)) => body
                    .with_animation(ElementId::Name(format!("folder-body-{p}-{serial}").into()), Animation::new(FOLDER_ANIM).with_easing(ease_out_quint()), move |d, t| {
                        let t = if opening { t } else { 1. - t };
                        d.h(px(body_h * t)).opacity(t)
                    })
                    .into_any_element(),
                None => body.into_any_element(),
            });
        }

        if more_folders > 0 {
            scroll = scroll.child(
                h_flex()
                    .id("folders-more")
                    .h(SIDE_ROW_H)
                    .flex_shrink_0()
                    .pl(px(42.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.sidebar_accent.opacity(0.6)))
                    .on_click(cx.listener(|this, _, _, cx| this.show_sessions(Scope::All, cx)))
                    .child(format!("{more_folders} more")),
            );
        }

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
            )
            // Settings live behind ⌘, and the app menu; the gear says so
            // for anyone who looks for them where every app keeps them.
            .child(icon_button("settings-gear", IconName::Settings, "Settings (⌘,)", cx, |this, window, cx| {
                this.settings_open = true;
                window.focus(&this.focus_handle, cx);
                cx.notify();
            }));

        v_flex().w(SIDEBAR_W).h_full().flex_shrink_0().bg(theme.sidebar).text_color(theme.sidebar_foreground).border_r_1().border_color(theme.sidebar_border).child(header).child(top).child(v_flex().id("side-scroll").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.side_scroll).px(px(10.)).pb(px(8.)).child(scroll)).child(footer)
    }

    /// Whether a folder in the sidebar shows its sessions: the person's
    /// own choice, on top of what `sync_folders` opens and closes.
    fn folder_open(&self, project: &str) -> bool {
        self.folders_open.as_ref().is_some_and(|open| open.contains(project))
    }

    /// A folder with a live session is open without being asked: it
    /// opens when one of its sessions goes live (at launch, every one
    /// that has one), and closes again when the last of them stops, if
    /// it was opened here and the person has not touched it since. A
    /// click is the person's choice and stays until the folder next goes
    /// live or quiet.
    fn sync_folders(&mut self, cx: &mut Context<Self>) {
        let live: HashSet<String> = self.refs.iter().filter(|r| self.live_color(r, cx).is_some()).map(|r| r.project()).collect();
        if live == self.folders_live {
            return;
        }
        let open = self.folders_open.get_or_insert_with(HashSet::new);
        for p in live.difference(&self.folders_live) {
            if open.insert(p.clone()) {
                self.folders_auto.insert(p.clone());
            }
        }
        for p in self.folders_live.difference(&live) {
            if self.folders_auto.remove(p) {
                open.remove(p);
            }
        }
        self.folders_live = live;
        self.save_ui(false);
    }

    fn toggle_folder(&mut self, project: &str, cx: &mut Context<Self>) {
        let open = self.folders_open.get_or_insert_with(HashSet::new);
        self.folders_auto.remove(project);
        if !open.remove(project) {
            open.insert(project.to_string());
        }
        let opening = open.contains(project);
        let serial = self.folder_anim.as_ref().map(|(_, _, _, n)| n + 1).unwrap_or(0);
        self.folder_anim = Some((project.to_string(), opening, std::time::Instant::now(), serial));
        self.save_ui(true);
        cx.notify();
    }

    /// A right click: the menu opens where the pointer is.
    fn open_menu(&mut self, at: Point<Pixels>, items: Vec<(&'static str, MenuDo)>, cx: &mut Context<Self>) {
        self.menu = Some(Menu { at, items });
        cx.notify();
    }

    fn menu_pick(&mut self, what: MenuDo, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        match what {
            MenuDo::OpenFolder(Some(cwd)) => crate::sys::open_path(std::path::Path::new(&cwd)),
            MenuDo::OpenFolder(None) => self.notice = Some(Notice::error(FOLDER_GONE)),
            MenuDo::Reveal(path) => crate::sys::reveal_path(&path),
            MenuDo::Rename(key) => {
                let now = self.refs.iter().find(|r| key_of(r) == key).map(|r| r.title.clone()).unwrap_or_default();
                self.renaming = Some(key);
                self.rename_input.update(cx, |s, cx| {
                    s.set_value(now, window, cx);
                    s.focus(window, cx);
                });
            }
        }
        cx.notify();
    }

    /// The name in the field becomes the session's (`set_title`); an
    /// empty field changes nothing.
    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.renaming.clone() else { return };
        let name = self.rename_input.read(cx).value().trim().to_string();
        self.set_title(&key, &name);
        self.close_rename(window, cx);
    }

    /// The session takes a name. It shows at once, from our own record
    /// (`state/titles.json`), and the session itself is renamed as soon
    /// as it can be (`try_renames`).
    fn set_title(&mut self, key: &str, name: &str) {
        if name.is_empty() {
            return;
        }
        if let Some(r) = self.refs.iter_mut().find(|r| key_of(r) == key) {
            r.title = name.to_string();
        }
        self.titles.insert(key.to_string(), name.to_string());
        self.save_titles();
        self.renames.insert(key.to_string(), name.to_string());
        self.try_renames();
    }

    fn save_titles(&self) {
        let _ = emaki_core::paths::ensure_dirs();
        if let Ok(v) = serde_json::to_value(&self.titles) {
            let _ = emaki_core::paths::write_json(&titles_file(), &v);
        }
    }

    /// The rename itself is the agent's to make: nothing here writes to
    /// a transcript, so Claude Code is asked, with its own `/rename`
    /// typed into the session's hidden terminal (one is started when the
    /// session has none), and it writes the name where its resume list
    /// and every other reader find it. Not typed into a running turn:
    /// the name waits here and goes when the turn is over. A session
    /// that cannot be asked (another agent, one kept only, a folder
    /// that is gone) keeps the name as ours alone.
    fn try_renames(&mut self) {
        for (key, name) in self.renames.clone() {
            let Some(r) = self.refs.iter().find(|r| key_of(r) == key).cloned() else { continue };
            if r.agent != AgentId::ClaudeCode || r.archived || !Self::folder_exists(&r) || !self.hub.hidden_terminals() {
                self.renames.remove(&key);
                continue;
            }
            if self.is_working(&r) {
                continue;
            }
            self.renames.remove(&key);
            if self.hub.start_terminal(&r.session_id, &r.cwd, true, "", "").is_ok() {
                // One line, as the command takes it.
                let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
                self.hub.send_to_terminal(&r.session_id, format!("/rename {name}"), Vec::new());
            }
        }
    }

    fn close_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The right-click menu: a small card at the pointer, kept inside the
    /// window, over a clear sheet that any click puts away.
    fn render_menu(&self, menu: Menu, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (w, row_h) = (px(210.), px(30.));
        let h = row_h * menu.items.len() as f32 + px(10.);
        let view = window.viewport_size();
        let x = menu.at.x.min(view.width - w - px(8.)).max(px(8.));
        let y = menu.at.y.min(view.height - h - px(8.)).max(px(8.));
        let mut card = v_flex()
            .id("menu-card")
            .absolute()
            .left(x)
            .top(y)
            .w(w)
            .p(px(5.))
            .rounded(px(10.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow(float_shadow(&theme))
            .on_mouse_down(MouseButton::Left, |_, window, cx| swallow_click(window, cx));
        for (ix, (label, what)) in menu.items.into_iter().enumerate() {
            card = card.child(
                h_flex()
                    .id(("menu-item", ix))
                    .h(row_h)
                    .px(px(10.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .hover(|s| s.bg(theme.sidebar_accent))
                    .on_click(cx.listener(move |this, _, window, cx| this.menu_pick(what.clone(), window, cx)))
                    .child(label),
            );
        }
        let shut = |this: &mut Self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>| {
            this.menu = None;
            cx.notify();
        };
        div().id("menu-sheet").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(shut)).on_mouse_down(MouseButton::Right, cx.listener(shut)).child(card)
    }

    /// The field a session is renamed in: a small card over a scrim, ↩
    /// to keep the name, Escape or a click outside to leave it.
    fn render_rename(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let focus = self.rename_input.read(cx).focus_handle(cx);
        let value = self.rename_input.read(cx).value().to_string();
        div()
            .id("rename-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .flex_col()
            .items_center()
            .pt(px(120.))
            .on_click(cx.listener(|this, _, window, cx| this.close_rename(window, cx)))
            .child(
                v_flex()
                    .id("rename-panel")
                    .key_context(SEARCH_CONTEXT)
                    .on_action(cx.listener(|this, _: &Escape, window, cx| this.close_rename(window, cx)))
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(px(460.))
                    .max_w(gpui::relative(0.94))
                    .p(px(16.))
                    .gap(px(10.))
                    .rounded(px(16.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Rename session"))
                    .child(
                        div()
                            .id("rename-field")
                            .track_focus(&focus)
                            .role(Role::TextInput)
                            .aria_label("Session name")
                            .aria_value(value)
                            .px(px(10.))
                            .h(px(36.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .bg(theme.muted)
                            .text_size(px(14.))
                            .child(Input::new(&self.rename_input).appearance(false).bordered(false)),
                    )
                    .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child("↩ to save · Esc to cancel")),
            )
    }

    fn group_label(&self, text: &'static str, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div().pt(px(18.)).pb(px(5.)).px(px(10.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(text)
    }

    /// The strip along the top of the content pane: room for the traffic
    /// lights when the sidebar is hidden, a title in the middle, actions on
    /// the right.
    fn render_topbar(&self, title: String, right: Vec<AnyElement>, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let centre = div().flex_1().min_w_0().text_center().truncate().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(title).into_any_element();
        self.render_topbar_with(centre, right, cx)
    }

    /// One tab per open session in the top strip: the agent's mark (turning
    /// while it works), the title, and a close button. Clicking a tab shows
    /// that session; ⌘W closes the one showing.
    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let row_w = self.tabs_row_w.clone();
        let entity = cx.entity().downgrade();
        let mut row = h_flex()
            .id("tabs")
            .flex_1()
            .min_w_0()
            .h_full()
            .justify_center()
            .items_center()
            .gap(px(TAB_GAP))
            .overflow_hidden()
            .on_drag_move::<DragTab>(cx.listener(|this, e: &DragMoveEvent<DragTab>, _, cx| {
                let key = e.drag(cx).0.clone();
                this.drag_tab_to(&key, e.event.position.x, e.bounds, cx);
            }))
            // The row's width, for `sync_tab_widths` at the next draw.
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let w = f32::from(bounds.size.width);
                        if (row_w.get() - w).abs() > 0.5 {
                            row_w.set(w);
                            let _ = entity.update(cx, |_, cx| cx.notify());
                        }
                    },
                    |_, _, _, _| (),
                )
                .absolute()
                .size_full(),
            );
        for (ix, key) in self.tabs.clone().into_iter().enumerate() {
            let r = self.refs.iter().find(|r| key_of(r) == key).cloned();
            let title = r.as_ref().map(|r| r.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "untitled".into());
            let agent = r.as_ref().map(|r| r.agent).unwrap_or(AgentId::ClaudeCode);
            let working = r.as_ref().map(|r| self.is_working(r)).unwrap_or(false);
            let active = self.selected.as_deref() == Some(key.as_str());
            let dragged = self.drag_tab.as_deref() == Some(key.as_str());
            let open_key = key.clone();
            let close_key = key.clone();
            let hover_bg = theme.muted.opacity(0.6);
            let close_bg = theme.border;
            let ghost = title.clone();
            // The same menu the session has in the sidebar.
            let menu: Vec<(&'static str, MenuDo)> = match &r {
                Some(r) => vec![("Rename", MenuDo::Rename(key.clone())), (crate::sys::REVEAL_LABEL, MenuDo::Reveal(r.path.clone()))],
                None => Vec::new(),
            };
            // A width that is over is drawn as it ended, so that a row
            // drawn afresh does not play it again.
            let width = self.tab_widths.get(&key).copied().unwrap_or(TabWidth { from: TAB_MAX, to: TAB_MAX, at: Instant::now(), serial: 0 });
            let (from, to) = if width.at.elapsed() < TAB_ANIM { (width.from, width.to) } else { (width.to, width.to) };
            let tab = h_flex()
                .id(ElementId::Name(format!("tab-{key}").into()))
                .h(px(30.))
                .pl(px(10.))
                .pr(px(6.))
                .gap(px(6.))
                .items_center()
                .rounded(px(8.))
                .cursor_pointer()
                .flex_shrink_0()
                .overflow_hidden()
                .when(active, |d| d.bg(theme.muted))
                .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
                .when(dragged, |d| d.opacity(0.45))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .when(!menu.is_empty(), |d| d.on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, _, cx| this.open_menu(ev.position, menu.clone(), cx))))
                .on_drag(DragTab(key.clone()), move |_, _, _, cx| cx.new(|_| TabGhost(ghost.clone())))
                .on_click(cx.listener(move |this, _, _, cx| this.open_session(&open_key, cx)))
                .child(agent_glyph(agent, px(14.), agent_color(agent, &theme), working, format!("tab-glyph-{ix}")))
                .child(
                    div()
                        .flex_1()
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
                        .flex_shrink_0()
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
                );
            row = row.child(tab.with_animation(ElementId::Name(format!("tab-w-{key}-{}", width.serial).into()), Animation::new(TAB_ANIM).with_easing(ease_out_quint()), move |d, t| {
                let d = d.w(px(from + (to - from) * t));
                if from == 0. { d.opacity(t) } else { d }
            }));
        }
        row.into_any_element()
    }

    /// The left end is room for the window's buttons (`render_strip`, and
    /// the traffic lights) when the sidebar is not beside the content;
    /// `right` is the page's actions. Both ends are at least 120px so the
    /// centre stays centred when they are short.
    fn render_topbar_with(&self, centre: AnyElement, right: Vec<AnyElement>, cx: &mut Context<Self>) -> impl IntoElement {
        let mut left_end = h_flex().min_w(px(120.)).flex_shrink_0();
        if !self.sidebar_open || self.narrow {
            left_end = left_end.w(Self::strip_right() - px(4.));
        }
        Self::drag_region(h_flex(), cx)
            .h(TITLEBAR_H)
            .flex_shrink_0()
            .px(px(12.))
            .items_center()
            .child(left_end)
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

        // Rows headed by when, newest first. A live row carries its state
        // as a chip at the right (the column it is on, in that colour); the
        // snippet of its last message that used to sit there was cut to a
        // few words and read as noise.
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last_bucket = "";
        for r in refs {
            let b = bucket(r.mtime);
            if b != last_bucket {
                rows.push(div().pt(if last_bucket.is_empty() { px(4.) } else { px(18.) }).pb(px(6.)).px(px(12.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(b).into_any_element());
                last_bucket = b;
            }
            let key = key_of(&r);
            let theme = cx.theme().clone();
            let card = self.card_for(&r);
            let dot = self.live_color(&r, cx);
            let working = self.is_working(&r);
            let glyph_id = SharedString::from(format!("row-glyph-{key}"));
            let glyph_color = if dot.is_some() { agent_color(r.agent, &theme) } else { theme.muted_foreground.opacity(0.75) };
            let mut sub = vec![r.project()];
            if !r.git_branch.is_empty() {
                sub.push(format!("⎇ {}", r.git_branch));
            }
            sub.push(relative(r.mtime, now));
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("row-{key}")))
                    .w_full()
                    .px(px(12.))
                    .py(px(9.))
                    .gap(px(12.))
                    .items_center()
                    .rounded(px(10.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_and_focus(&key, window, cx)))
                    .child(div().w(px(24.)).flex().justify_center().child(agent_glyph(r.agent, px(15.), glyph_color, working, glyph_id)))
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
                                    .when(r.archived, |d| d.child(badge("kept", theme.muted, theme.muted_foreground)))
                                    .when(r.agent != AgentId::ClaudeCode, |d| d.child(badge(r.agent.display_name(), theme.muted, theme.muted_foreground))),
                            )
                            .child(div().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(sub.join(" · "))),
                    )
                    .when_some(dot, |d, c| d.child(state_chip(card.column, c, &theme)))
                    .into_any_element(),
            );
        }

        let title = match &scope {
            Scope::All => "Your sessions".to_string(),
            Scope::Agent(a) => format!("{} sessions", a.display_name()),
            Scope::Project(p) => p.clone(),
            Scope::Kept => "Kept sessions".to_string(),
        };
        let count = self.scoped_refs().len();
        let display = crate::fonts::display_family(cx);

        v_flex().flex_1().min_w_0().h_full().bg(theme.background).child(self.render_topbar(String::new(), Vec::new(), cx)).child(
            v_flex().id("sessions").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.sessions_scroll).px(px(24.)).items_center().child(page_in(
                "page-sessions",
                v_flex()
                    .w_full()
                    .max_w(CONTENT_W)
                    .pt(px(20.))
                    .pb(px(40.))
                    .gap(px(14.))
                    .child(div().text_size(px(30.)).font_family(display).child(title))
                    .child(filters)
                    .child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child(format!("{} on this machine, every one of them kept.", plural(count, "session", "sessions"))))
                    .child(v_flex().w_full().gap(px(2.)).children(rows)),
            )),
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
        // A tab shown for the first time, for as long as its read takes:
        // the tabs stay where they are and the pane is empty, with no
        // word to flash by.
        let Some(detail) = &self.detail else {
            let tabs = self.render_tabs(cx);
            return v_flex().flex_1().h_full().child(self.render_topbar_with(tabs, Vec::new(), cx)).into_any_element();
        };
        let session = detail.session.clone();
        let list = detail.list.clone();
        let entity = cx.entity().downgrade();

        let mut right: Vec<AnyElement> = Vec::new();
        if r.archived {
            right.push(badge("kept", theme.muted, theme.muted_foreground).into_any_element());
        }
        // The three places a session can be taken to, side by side: the
        // terminal, the project's folder, the transcript on disk.
        let terminal_tip = match self.terminal_check(&r) {
            Ok(()) => "Open in your terminal",
            Err(why) => why,
        };
        right.push(icon_button("terminal", Icon::default().path("icons/square-terminal.svg"), terminal_tip, cx, |this, _, cx| this.open_in_terminal(cx)).into_any_element());
        let folder_tip = if Self::folder_exists(&r) { "Open the project folder" } else { FOLDER_GONE };
        right.push(icon_button("project-folder", IconName::FolderOpen, folder_tip, cx, |this, _, cx| this.open_project_folder(cx)).into_any_element());
        right.push(
            icon_button("reveal", Icon::default().path("icons/file-text.svg"), crate::sys::REVEAL_TRANSCRIPT_LABEL, cx, {
                let p = r.path.clone();
                move |_, _, _| crate::sys::reveal_path(&p)
            })
            .into_any_element(),
        );
        let tabs = self.render_tabs(cx);
        let topbar = self.render_topbar_with(tabs, right, cx);

        // Under the tabs, where the session lives and nothing else: a band
        // across the pane, as a file manager's path bar is, so it reads as
        // part of the frame and not as a second tab under the first. The
        // folder's parents are dimmed, its own name is in the foreground,
        // and a click opens it, as the folder button above does. Rounds,
        // tool calls, tokens, the model and the id used to follow the path
        // on one long line nobody read.
        let cwd = session.cwd.clone();
        let name_len = std::path::Path::new(&cwd).file_name().map(|n| n.to_string_lossy().len()).filter(|n| cwd.len() >= *n).unwrap_or(0);
        let (parents, name) = cwd.split_at(cwd.len() - name_len);
        let path_line = (!cwd.is_empty()).then(|| {
            let hover_bg = theme.muted;
            h_flex().w_full().h(px(30.)).px(px(24.)).justify_center().items_center().bg(theme.muted.opacity(0.3)).border_b_1().border_color(theme.border).child(
                h_flex()
                    .id("path-bar")
                    .max_w_full()
                    .min_w_0()
                    .h(px(22.))
                    .px(px(8.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .font_family(theme.mono_font_family.clone())
                    .text_size(px(11.))
                    .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(folder_tip).build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.open_project_folder(cx)))
                    .child(Icon::new(IconName::Folder).with_size(px(12.)).text_color(theme.muted_foreground).flex_shrink_0())
                    .child(
                        h_flex()
                            .min_w_0()
                            .child(div().min_w_0().truncate().text_color(theme.muted_foreground).child(parents.to_string()))
                            .child(div().flex_shrink_0().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(name.to_string())),
                    ),
            )
        });

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
            // The terminal's own line when it could be read, in the
            // terminal's colours as the window's appearance has them:
            // the screen is in whichever theme Claude Code is set to, so
            // each colour is looked up in its two themes
            // (`driver::theme_pair`) and the window takes its own side.
            // The word is in the colour of the terminal's mark, which is
            // one colour while the agent writes and another while it
            // thinks. Read as written, a dark theme's bright yellow sat
            // on a light window. Grey is the muted ink, and a colour in
            // neither theme is only kept readable. The bracket carries
            // the turn's time, so ours is left out.
            let seen = self.working_seen.as_ref().filter(|(sid, _, _)| *sid == r.session_id).map(|(_, w, _)| w.clone());
            let ink = |c: Option<u32>| match c {
                Some(v) if (v >> 16) & 0xff == (v >> 8) & 0xff && (v >> 8) & 0xff == v & 0xff => theme.muted_foreground,
                Some(v) => match driver::theme_pair(v) {
                    Some(pair) => shade(pair, &theme),
                    None => {
                        let mut c: Hsla = rgb(v).into();
                        c.l = if theme.mode.is_dark() { c.l.max(0.6) } else { c.l.min(0.36) };
                        c
                    }
                },
                None => theme.muted_foreground,
            };
            let word = |c: Option<u32>| c.map(|v| ink(Some(v))).unwrap_or_else(|| agent_color(r.agent, &theme));
            h_flex()
                .w_full()
                .max_w(CONTENT_W)
                .px(px(6.))
                .gap(px(8.))
                .items_center()
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .child(agent_glyph(r.agent, px(14.), agent_color(r.agent, &theme), true, "status-glyph"))
                .map(|d| match seen {
                    Some(w) => d
                        .child(div().flex_shrink_0().font_weight(FontWeight::MEDIUM).text_color(word(w.color)).child(w.verb))
                        .child(h_flex().flex_1().min_w_0().overflow_hidden().gap(px(4.)).children(w.detail.into_iter().map(|(t, c)| div().flex_shrink_0().text_color(ink(c)).child(t)))),
                    None => d
                        .child(div().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(format!("{} is working…", r.agent.speaker())))
                        .child(div().flex_1().min_w_0().truncate().child(what))
                        .child(div().child(elapsed_since(from, self.now))),
                })
                .child(
                    // Stop, as Escape is in the terminal: a square on a
                    // pill that fills with the danger colour under the
                    // pointer, so it is found without shouting.
                    h_flex()
                        .id("stop-turn")
                        .occlude()
                        .h(px(22.))
                        .px(px(9.))
                        .gap(px(6.))
                        .items_center()
                        .rounded_full()
                        .border_1()
                        .border_color(theme.border)
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.danger.opacity(0.10)).border_color(theme.danger.opacity(0.45)).text_color(theme.danger))
                        .active(|s| s.bg(theme.danger.opacity(0.18)))
                        .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new("Stop this turn (Esc)").build(window, cx))
                        .on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.interrupt(cx);
                        }))
                        .child(div().size(px(7.)).rounded(px(1.5)).bg(theme.danger))
                        .child("Stop"),
                )
        });

        // The agent waiting on you, said in the same place: what it waits
        // for, and where to give it when the terminal holds the dialog and
        // nothing here can answer it.
        let waiting = (!working).then(|| self.card_for(&r)).filter(|c| c.column == Column::NeedsYou).map(|card| {
            let in_terminal = card.pending.is_none();
            let question = card.pending.as_ref().map(|p| p.is_question()).unwrap_or(card.chip_kind == "ask");
            let what = if question {
                "your answer"
            } else if card.chip_kind == "plan" {
                "your go-ahead on the plan"
            } else {
                "your approval"
            };
            let from = if !r.state.since.is_empty() { r.state.since.clone() } else { r.updated.clone() };
            let own = self.own_terminal(&r.session_id);
            let where_ = if in_terminal && own { "in its terminal".to_string() } else if in_terminal { "in your terminal".to_string() } else { "below".to_string() };
            h_flex()
                .w_full()
                .max_w(CONTENT_W)
                .px(px(6.))
                .gap(px(8.))
                .items_center()
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .child(agent_glyph(r.agent, px(14.), agent_color(r.agent, &theme), false, "status-glyph"))
                .child(div().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(format!("{} is waiting for {what}", r.agent.speaker())))
                .child(div().flex_1().min_w_0().truncate().child(where_))
                .when(in_terminal, |d| d.child(Button::new("waiting-terminal").primary().small().label(if own { "Show the terminal" } else { "Open the terminal" }).on_click(cx.listener(|this, _, _, cx| this.go_to_terminal(cx)))))
                .child(div().child(elapsed_since(&from, self.now)))
        });

        // The hidden terminal's own screen, when it has to be seen, takes
        // the place of any card: it is the same dialog, as it is.
        let terminal = self.render_terminal(&r, cx);
        let dialog = if terminal.is_some() { None } else { self.render_dialog(&r, cx) };
        let covered = dialog.is_some() || terminal.is_some();
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme.background)
            .child(topbar)
            .children(path_line)
            .when(self.find_open, |d| d.child(self.render_find_bar(cx)))
            .child(transcript)
            // The working or waiting line gets air above it: without any,
            // it sat against the conversation's clipped edge and read as
            // part of the last tool card.
            .child(
                v_flex()
                    .w_full()
                    .items_center()
                    .px(px(24.))
                    .when(status.is_some() || waiting.is_some(), |d| d.pt(px(12.)))
                    .pb(px(14.))
                    .gap(px(8.))
                    .children(status.filter(|_| !covered))
                    .children(waiting.filter(|_| !covered))
                    .children(terminal)
                    .children(dialog)
                    .child(self.render_permissions(cx))
                    .child(self.render_composer(cx)),
            )
            .into_any_element()
    }

    /// The hidden terminal's screen, drawn where the cards sit, when the
    /// person has to see it: Claude Code's own `/model` list or `/effort`
    /// slider, or a screen no card stands for. It is the screen as the
    /// terminal has it, in its colours, on the ground Claude Code's
    /// theme was made for; the keys pressed on it go to Claude Code, and
    /// it leaves when what it showed is done (`back_from_terminal`).
    fn render_terminal(&self, r: &SessionRef, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sid = self.term_open.as_ref().filter(|s| **s == r.session_id)?;
        let pty = self.hub.terminal_for(sid)?;
        let theme = cx.theme().clone();
        let (ground, ink): (u32, u32) = if claude_theme_light() { (0xfaf9f5, 0x1f1e1d) } else { (0x1f1e1d, 0xe8e6dc) };
        let hsla = |c: u32| -> Hsla { rgb(c).into() };
        let mix = |a: u32, b: u32, t: f32| {
            let ch = |sh: u32| ((a >> sh & 0xff) as f32 * (1.0 - t) + (b >> sh & 0xff) as f32 * t).round() as u32;
            ch(16) << 16 | ch(8) << 8 | ch(0)
        };
        // What the pointer can press, as the keys that do it: Claude
        // Code's own interface takes no mouse (`pty::hits`).
        let screen = pty.rows();
        // Asked for before the picker is drawn, and still up for a moment
        // after it closes: the screen is then the whole conversation, and
        // drawing that made the card jump from tall to small and back.
        // Nothing is drawn until there is something to show: a picker, or
        // a screen the terminal is waiting on.
        let waiting = self.term_auto || self.hub.peer_for(sid).is_some_and(|p| p.status == "waiting");
        if !emaki_core::pty::picker_up(&screen) && !waiting {
            return None;
        }
        let rows = emaki_core::pty::panel_rows(screen);
        let hits = emaki_core::pty::hits(&rows);
        let mut lines: Vec<AnyElement> = Vec::new();
        for (rix, row) in rows.iter().enumerate() {
            let mut text = String::new();
            let mut looks = Vec::new();
            for s in row {
                let start = text.len();
                text.push_str(&s.text);
                let (mut fg, mut bg) = (s.fg.unwrap_or(ink), s.bg);
                if s.inverse {
                    (fg, bg) = (bg.unwrap_or(ground), Some(fg));
                }
                if s.dim {
                    fg = mix(fg, bg.unwrap_or(ground), 0.45);
                }
                looks.push((
                    start..text.len(),
                    HighlightStyle {
                        color: Some(hsla(fg)),
                        background_color: bg.map(hsla),
                        font_weight: s.bold.then_some(FontWeight::BOLD),
                        font_style: s.italic.then_some(FontStyle::Italic),
                        underline: s.underline.then(|| UnderlineStyle { thickness: px(1.), ..Default::default() }),
                        ..Default::default()
                    },
                ));
            }
            let mut mine: Vec<&emaki_core::pty::Hit> = hits.iter().filter(|h| h.row == rix).collect();
            mine.sort_by_key(|h| h.start);
            if mine.is_empty() {
                if text.is_empty() {
                    text.push(' ');
                }
                lines.push(div().h(px(17.)).whitespace_nowrap().child(StyledText::new(text).with_highlights(looks)).into_any_element());
                continue;
            }
            // The row in pieces, so each stretch that takes a click is an
            // element of its own: characters to bytes, and each piece
            // with the looks that fall inside it.
            let offsets: Vec<usize> = text.char_indices().map(|(at, _)| at).chain(std::iter::once(text.len())).collect();
            let last = offsets.len() - 1;
            let piece = |from: usize, to: usize| {
                let (a, b) = (offsets[from.min(last)], offsets[to.min(last)]);
                let inside = looks.iter().filter_map(|(range, look)| {
                    let (start, end) = (range.start.max(a), range.end.min(b));
                    (start < end).then(|| (start - a..end - a, look.clone()))
                });
                StyledText::new(text[a..b].to_string()).with_highlights(inside.collect::<Vec<_>>())
            };
            let mut kids: Vec<AnyElement> = Vec::new();
            let mut at = 0;
            for hit in mine {
                if hit.start < at || hit.end <= hit.start {
                    continue;
                }
                if hit.start > at {
                    kids.push(div().child(piece(at, hit.start)).into_any_element());
                }
                let keys = hit.keys.clone();
                kids.push(
                    div()
                        .id(SharedString::from(format!("term-hit-{rix}-{}", hit.start)))
                        .rounded(px(3.))
                        .cursor_pointer()
                        .hover(|s| s.bg(hsla(ink).opacity(0.16)))
                        .active(|s| s.bg(hsla(ink).opacity(0.26)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            window.focus(&this.term_focus, cx);
                            this.term_key(&keys, cx);
                        }))
                        .child(piece(hit.start, hit.end))
                        .into_any_element(),
                );
                at = hit.end;
            }
            if at < last {
                kids.push(div().child(piece(at, last)).into_any_element());
            }
            // From the top, as a row drawn whole is: centred, a split row sat two pixels off its neighbours.
            lines.push(h_flex().items_start().h(px(17.)).whitespace_nowrap().children(kids).into_any_element());
        }
        let quiet = hsla(mix(ink, ground, 0.45));
        Some(
            v_flex()
                .id("terminal-card")
                .key_context(TERMINAL_CONTEXT)
                .track_focus(&self.term_focus)
                .w_full()
                .max_w(CONTENT_W)
                .rounded(px(14.))
                .border_1()
                .border_color(theme.primary)
                .bg(hsla(ground))
                .shadow_sm()
                .overflow_hidden()
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                    if let Some(bytes) = term_bytes(&e.keystroke) {
                        this.term_key(&bytes, cx);
                        cx.stop_propagation();
                    }
                }))
                .on_action(cx.listener(|this, _: &TermTab, _, cx| this.term_key(b"\t", cx)))
                .on_action(cx.listener(|this, _: &TermBackTab, _, cx| this.term_key(b"\x1b[Z", cx)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        window.focus(&this.term_focus, cx);
                    }),
                )
                .child(
                    h_flex()
                        .px(px(12.))
                        .pt(px(9.))
                        .pb(px(6.))
                        .gap(px(8.))
                        .items_center()
                        .text_size(px(11.5))
                        .child(Icon::default().path("icons/square-terminal.svg").with_size(px(13.)).text_color(hsla(ink)))
                        .child(div().font_weight(FontWeight::MEDIUM).text_color(hsla(ink)).child(format!("{}'s terminal", r.agent.display_name())))
                        .child(div().flex_1().min_w_0().truncate().text_color(quiet).child("click or use the keys; it closes when you are done"))
                        .child(
                            div()
                                .id("terminal-close")
                                .size(px(20.))
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(|s| s.bg(hsla(ink).opacity(0.12)))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    swallow_click(window, cx);
                                    this.dismiss_terminal(cx);
                                }))
                                .child(Icon::new(IconName::Close).with_size(px(12.)).text_color(quiet)),
                        ),
                )
                .child(div().px(px(12.)).pb(px(12.)).font_family(theme.mono_font_family.clone()).text_size(px(12.)).text_color(hsla(ink)).children(lines))
                .into_any_element(),
        )
    }

    /// The dialog a terminal session is held on, as a card where the
    /// permission cards sit: the questions as chips, ticked once
    /// answered, what is asked, and the choices as rows. A click sends
    /// the choice's digit to the terminal, whose dialog is the one that
    /// moves on; the card is its screen read again. Words typed in the
    /// composer answer a question that offers "Type something".
    fn render_dialog(&self, r: &SessionRef, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, d) = self.dialog_seen.as_ref().filter(|(sid, _)| *sid == r.session_id)?;
        let theme = cx.theme().clone();
        let approval = d.body.last().is_some_and(|l| l.starts_with("Do you want"));
        let review = d.body.first().is_some_and(|l| l.starts_with("Review your answers"));
        // What is asked is the last line over the choices; what stands
        // before it is the command, or the answers under review.
        let (title, before) = match d.body.split_last() {
            Some((last, rest)) => (last.clone(), rest.to_vec()),
            None => (String::new(), Vec::new()),
        };
        let typed = !d.multi && d.options.iter().any(|o| o.label.starts_with("Type something"));
        let busy = self.dialog_sending;
        let head = if approval { "approval" } else if review { "review" } else { "question" };
        let says = if approval { format!("{} asks to go ahead", r.agent.speaker()) } else { format!("{} asks", r.agent.speaker()) };
        let rows = d.options.iter().filter(|o| !(d.multi && o.label.starts_with("Type something"))).map(|o| {
            let on = o.checked == Some(true);
            let (n, multi, own) = (o.n, d.multi, o.label.starts_with("Type something"));
            // The two the dialog adds to every question are quieter.
            let aside = own || o.label == "Chat about this";
            h_flex()
                .id(SharedString::from(format!("dialog-opt-{n}")))
                .w_full()
                .px(px(10.))
                .py(px(7.))
                .gap(px(10.))
                .items_start()
                .rounded(px(10.))
                .border_1()
                .border_color(if on { theme.primary } else { theme.border })
                .bg(if on { theme.primary.opacity(0.08) } else { theme.transparent })
                .when(busy, |d| d.opacity(0.6))
                .cursor_pointer()
                .hover(|s| s.bg(theme.list_hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    if own {
                        let handle = this.composer.read(cx).focus_handle(cx);
                        window.focus(&handle, cx);
                        this.notice = Some(Notice::said("type your answer in the box and send it"));
                        cx.notify();
                    } else {
                        this.dialog_send(vec![DialogStep { keys: n.to_string(), until: None }], cx);
                    }
                }))
                .child(
                    div()
                        .flex_shrink_0()
                        .size(px(16.))
                        .mt(px(1.))
                        .rounded(if multi && !aside { px(4.) } else { px(8.) })
                        .border_1()
                        .border_color(if on { theme.primary } else { theme.muted_foreground.opacity(if aside { 0.5 } else { 1.0 }) })
                        .bg(if on { theme.primary } else { theme.transparent })
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(on, |d| d.child(Icon::new(IconName::Check).with_size(px(11.)).text_color(theme.primary_foreground))),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .gap(px(1.))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).when(aside, |d| d.text_color(theme.muted_foreground)).child(if own { "Type your own answer".to_string() } else { o.label.clone() }))
                        .when(!o.detail.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(o.detail.clone()))),
                )
        });
        Some(
            v_flex()
                .w_full()
                .max_w(CONTENT_W)
                .p(px(14.))
                .gap(px(12.))
                .rounded(px(14.))
                .border_1()
                .border_color(theme.primary)
                .bg(theme.popover)
                .shadow_sm()
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(badge_str(head.into(), theme.primary.opacity(0.14), theme.primary))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(says))
                        .child(div().flex_1())
                        // The questions and the review, as the terminal's
                        // tabs: the one showing ringed, an answered one
                        // ticked, and a click goes there.
                        .children(d.tabs.iter().enumerate().map(|(ix, (name, done))| {
                            let here = d.current == Some(ix);
                            let reachable = d.current.is_some() && !here && !busy;
                            h_flex()
                                .id(SharedString::from(format!("dialog-tab-{ix}")))
                                .gap(px(4.))
                                .items_center()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded(px(999.))
                                .border_1()
                                .border_color(if here { theme.primary } else { theme.transparent })
                                .bg(if *done { theme.primary.opacity(0.12) } else { theme.muted })
                                .text_size(px(11.))
                                .text_color(if *done || here { theme.primary } else { theme.muted_foreground })
                                .when(reachable, |d| d.cursor_pointer().hover(|s| s.bg(theme.list_hover)))
                                .when(reachable, |d| {
                                    d.on_click(cx.listener(move |this, _, window, cx| {
                                        swallow_click(window, cx);
                                        this.dialog_go(ix, cx);
                                    }))
                                })
                                .when(*done, |d| d.child(Icon::new(IconName::Check).with_size(px(10.))))
                                .child(if name == "Submit" { "Review".to_string() } else { name.clone() })
                        })),
                )
                .when(!before.is_empty(), |el| {
                    el.child(
                        v_flex()
                            .p(px(8.))
                            .gap(px(2.))
                            .rounded(px(8.))
                            .bg(theme.muted)
                            .text_size(px(12.))
                            .when(approval, |d| d.font_family(theme.mono_font_family.clone()))
                            .children(before.into_iter().filter(|l| !(review && l.starts_with("Review your answers"))).map(|l| match l.strip_prefix("→ ") {
                                // An answer under review, under its question.
                                Some(answer) => div().pb(px(4.)).whitespace_normal().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(theme.primary).child(answer.to_string()),
                                None => div().whitespace_normal().when(review, |d| d.text_color(theme.muted_foreground)).child(l),
                            })),
                    )
                })
                .child(div().text_size(px(14.)).child(title))
                .when(!review, |el| el.child(v_flex().gap(px(4.)).children(rows)))
                .when(review, |el| {
                    // The review's two choices are the card's buttons.
                    el.child(h_flex().gap(px(8.)).items_center().children(d.options.iter().enumerate().map(|(i, o)| {
                        let n = o.n;
                        let b = Button::new(SharedString::from(format!("dialog-review-{n}"))).small().label(o.label.clone()).disabled(busy);
                        let b = if i == 0 { b.primary() } else { b.outline() };
                        b.on_click(cx.listener(move |this, _, _, cx| this.dialog_send(vec![DialogStep { keys: n.to_string(), until: None }], cx)))
                    })))
                })
                .when(!review, |el| el.child(
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .when(d.multi, |el| {
                            el.child(Button::new("dialog-next").primary().small().label("Next").disabled(busy).on_click(cx.listener(|this, _, _, cx| this.dialog_send(vec![DialogStep { keys: "\t".into(), until: None }], cx))))
                        })
                        .child(Button::new("dialog-cancel").ghost().small().label("Cancel  esc").disabled(busy).on_click(cx.listener(|this, _, _, cx| this.dialog_send(vec![DialogStep { keys: "\x1b".into(), until: None }], cx))))
                        .child(div().flex_1())
                        .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(if typed { "or type an answer below" } else { "" })),
                ))
                .into_any_element(),
        )
    }

    fn render_permissions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let cwd = self.selected_ref().map(|r| r.cwd.clone()).unwrap_or_default();
        let cards: Vec<PermissionRequest> = self.permissions.iter().filter(|(s, _)| *s == sid).map(|(_, p)| p.clone()).collect();
        let several = cards.len() > 1;
        v_flex().w_full().max_w(CONTENT_W).gap(px(8.)).children(cards.into_iter().enumerate().map(|(i, p)| {
            if p.is_question() {
                return self.render_question(&p, cx);
            }
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
                .into_any_element()
        }))
    }

    /// A question of Claude's, where a permission card would be: each
    /// question with its options as rows to click, the chosen one filled.
    /// One question with one choice is answered by the click; several
    /// questions, or several choices, are sent by the button once every
    /// question has one. Words typed in the composer answer instead.
    fn render_question(&self, p: &PermissionRequest, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let qs = questions_of(&p.input);
        let picks = self.picks.get(&p.request_id).cloned().unwrap_or_default();
        let by_button = qs.len() > 1 || qs.iter().any(|q| q.multi);
        let ready = qs.iter().all(|q| picks.get(&q.question).map(|l| !l.is_empty()).unwrap_or(false));
        let rid = p.request_id.clone();
        v_flex()
            .p(px(14.))
            .gap(px(12.))
            .rounded(px(14.))
            .border_1()
            .border_color(theme.primary)
            .bg(theme.popover)
            .shadow_sm()
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(badge_str("question".into(), theme.primary.opacity(0.14), theme.primary))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Claude asks")),
            )
            .children(qs.iter().enumerate().map(|(qi, q)| {
                let chosen = picks.get(&q.question).cloned().unwrap_or_default();
                v_flex()
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .when(!q.header.is_empty(), |d| d.child(badge_str(q.header.clone(), theme.muted, theme.muted_foreground)))
                            .child(div().flex_1().min_w_0().text_size(px(14.)).child(q.question.clone())),
                    )
                    .child(v_flex().gap(px(4.)).children(q.options.iter().enumerate().map(|(oi, (label, detail))| {
                        let on = chosen.iter().any(|l| l == label);
                        let (rid, question, label_c, multi) = (rid.clone(), q.question.clone(), label.clone(), q.multi);
                        h_flex()
                            .id(SharedString::from(format!("opt-{}-{qi}-{oi}", p.request_id)))
                            .w_full()
                            .px(px(10.))
                            .py(px(7.))
                            .gap(px(10.))
                            .items_start()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(if on { theme.primary } else { theme.border })
                            .bg(if on { theme.primary.opacity(0.08) } else { theme.transparent })
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.list_hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.pick_option(rid.clone(), question.clone(), label_c.clone(), multi, cx);
                            }))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size(px(16.))
                                    .mt(px(1.))
                                    .rounded(if q.multi { px(4.) } else { px(8.) })
                                    .border_1()
                                    .border_color(if on { theme.primary } else { theme.muted_foreground })
                                    .bg(if on { theme.primary } else { theme.transparent })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(on, |d| d.child(Icon::new(IconName::Check).with_size(px(11.)).text_color(theme.primary_foreground))),
                            )
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .gap(px(1.))
                                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(label.clone()))
                                    .when(!detail.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(detail.clone()))),
                            )
                    })))
            }))
            .child(
                h_flex()
                    .gap(px(10.))
                    .items_center()
                    .when(by_button, |d| {
                        let rid = p.request_id.clone();
                        d.child(Button::new(SharedString::from(format!("answer-{}", p.request_id))).primary().small().label("Answer").disabled(!ready).on_click(cx.listener(move |this, _, _, cx| this.submit_question(rid.clone(), cx))))
                    })
                    .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child("or type an answer below")),
            )
            .into_any_element()
    }

    /// The commands the list over the composer offers for what is typed
    /// so far, best match first, and whether the folder's own catalogue is
    /// still being read. A driver's own list, else the folder's catalogue
    /// read from a headless child, else the built-ins until that is in.
    fn slash_rows(&self, prefix: &str, via: &str, whole: bool) -> (Vec<(String, String, String)>, bool) {
        let (sid, cwd) = self.selected_ref().map(|r| (r.session_id.clone(), r.cwd.clone())).unwrap_or_default();
        let (known, loading): (Vec<(String, String, String)>, bool) = match self.drivers.get(&sid).filter(|v| !v.commands.is_empty()) {
            Some(v) => (v.commands.iter().map(|c| (c.name.clone(), c.description.clone(), c.argument_hint.clone())).collect(), false),
            None => {
                let cwd = if via == "spawn" && self.page == Page::New { self.new_cwd.clone() } else { cwd };
                match (!cwd.is_empty()).then(|| self.hub.commands_for(&cwd)).flatten() {
                    Some(list) => (list.iter().map(|c| (c.name.clone(), c.description.clone(), c.argument_hint.clone())).collect(), false),
                    None => (BUILTIN_COMMANDS.iter().map(|(n, d, a)| (n.to_string(), d.to_string(), a.to_string())).collect(), !cwd.is_empty()),
                }
            }
        };
        // As the terminal lists them: names that start with what is typed,
        // then names holding it, then descriptions.
        let needle = prefix.to_lowercase();
        let rank = |name: &str, description: &str| {
            let name = name.to_lowercase();
            if name.starts_with(&needle) {
                Some(0u8)
            } else if name.contains(&needle) {
                Some(1)
            } else if !needle.is_empty() && description.to_lowercase().contains(&needle) {
                Some(2)
            } else {
                None
            }
        };
        // Inside a sentence only what can be named there is offered: a
        // command that acts by itself means nothing in the middle of one.
        let mut ranked: Vec<(u8, (String, String, String))> =
            known.into_iter().filter(|row| whole || !acts_alone(&row.0)).filter_map(|row| rank(&row.0, &row.1).map(|r| (r, row))).collect();
        // Within a rank the shorter name is the nearer match ("/co" is
        // /color and /config before a plugin's long name); with nothing
        // typed the catalogue's own order stands.
        ranked.sort_by_key(|(r, row)| (*r, if needle.is_empty() { 0 } else { row.0.len() }));
        (ranked.into_iter().map(|(_, row)| row).collect(), loading)
    }

    /// The slash command being typed at the caret, while the list is
    /// showing: the channel a message would take, where the token starts
    /// and ends in the text, what is typed of its name, and whether the
    /// message is that token and nothing else.
    fn slash_open(&self, cx: &App) -> Option<SlashAt> {
        if self.slash_closed {
            return None;
        }
        let (via, _) = self.reply_via();
        if !matches!(via, "driver" | "spawn" | "inbox" | "pty") {
            return None;
        }
        let state = self.composer.read(cx);
        let text = state.value().to_string();
        let (start, end, prefix) = slash_token_at(&text, state.cursor())?;
        let whole = text[..start].trim().is_empty() && text[end..].trim().is_empty();
        Some(SlashAt { via, start, end, prefix, whole, text })
    }

    /// Runs a command chosen from the list: typed into the terminal on a
    /// terminal session, through the driver otherwise.
    fn slash_run(&mut self, command: String, via: &str, window: &mut Window, cx: &mut Context<Self>) {
        if via == "inbox" || via == "pty" {
            self.run_in_terminal(command, cx);
            self.composer.update(cx, |s, cx| s.set_value("", window, cx));
        } else {
            self.composer.update(cx, |s, cx| s.set_value(command, window, cx));
            self.send_message(window, cx);
        }
        cx.notify();
    }

    /// Puts a command's name where the token being typed is, with a space
    /// and the caret after it, and leaves the message to be written on.
    fn slash_insert(&mut self, name: &str, at: &SlashAt, window: &mut Window, cx: &mut Context<Self>) {
        let tail = &at.text[at.end..];
        let word = format!("/{name}{}", if tail.starts_with(' ') { "" } else { " " });
        let caret = at.start + name.len() + 2;
        let text = format!("{}{word}{tail}", &at.text[..at.start]);
        let before = &text[..caret.min(text.len())];
        let line = before.matches('\n').count() as u32;
        let column = before.rsplit('\n').next().unwrap_or("").encode_utf16().count() as u32;
        self.composer.update(cx, |s, cx| {
            s.set_value(text, window, cx);
            s.set_cursor_position(gpui_component::input::Position::new(line, column), window, cx);
        });
        // A value set from code reports no change.
        self.slash_sel = 0;
        self.mark_slash(cx);
    }

    /// A row of the list was chosen, by ↩ or a click. A command that acts
    /// by itself (`driver::acts_alone`: `/compact`, `/model`) runs when it
    /// is the whole message; a skill, or anything chosen inside a
    /// sentence, goes into the text, since a skill is as often asked for
    /// in words ("use my /ph-image skill to…") and running one unasked
    /// cannot be taken back. ⌘↩ still sends a message that is only a
    /// command as that command.
    fn slash_pick(&mut self, name: &str, at: &SlashAt, window: &mut Window, cx: &mut Context<Self>) {
        if at.whole && acts_alone(name) {
            self.slash_run(format!("/{name}"), at.via, window, cx);
        } else {
            self.slash_insert(name, at, window, cx);
        }
    }

    /// A key pressed in the composer while the list is showing: ↑ and ↓
    /// move the choice (round the ends), ↩ picks it, ⇥ puts its name in
    /// the text whatever it is, Escape puts the list away. False when the
    /// list is not showing, and the key is the textarea's.
    fn slash_key(&mut self, key: SlashKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(at) = self.slash_open(cx) else { return false };
        let (rows, _) = self.slash_rows(&at.prefix, at.via, at.whole);
        if rows.is_empty() {
            return false;
        }
        let n = rows.len();
        let sel = self.slash_sel.min(n - 1);
        match key {
            SlashKey::Up => self.slash_sel = (sel + n - 1) % n,
            SlashKey::Down => self.slash_sel = (sel + 1) % n,
            SlashKey::Run => self.slash_pick(&rows[sel].0, &at, window, cx),
            SlashKey::Complete => self.slash_insert(&rows[sel].0, &at, window, cx),
            SlashKey::Close => self.slash_closed = true,
        }
        cx.notify();
        true
    }

    /// Colours every command the composer's text names (`/ph-image` in a
    /// sentence, `/compact` alone) in the accent, as the conversation does
    /// once it is sent. Set again on every change: the ranges are not
    /// tracked across edits.
    fn mark_slash(&mut self, cx: &mut Context<Self>) {
        let (via, _) = self.reply_via();
        let text = self.composer.read(cx).value().to_string();
        let mut marks = Vec::new();
        if text.contains('/') {
            let (known, _) = self.slash_rows("", via, true);
            let style = HighlightStyle { color: Some(cx.theme().link), ..Default::default() };
            for (a, b) in slash_tokens(&text) {
                let name = &text[a + 1..b];
                if acts_alone(name) || known.iter().any(|row| row.0 == name) {
                    marks.push(gpui_component::input::TextDecoration::new(a..b, style));
                }
            }
        }
        self.composer.update(cx, |s, cx| s.set_marks(marks, cx));
    }

    /// The list that opens over the composer while a message is a slash
    /// command being typed: the commands the session knows, filtered by
    /// what is typed so far. Each row is the name and its arguments in one
    /// column and what it does in the other, both cut with an ellipsis, so
    /// a long argument hint never pushes the row past the card. The keys
    /// are on one row (`slash_sel`), the first after every keystroke.
    fn render_slash_help(&self, at: SlashAt, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let via = at.via;
        let (rows, loading) = self.slash_rows(&at.prefix, via, at.whole);
        if rows.is_empty() && !loading {
            return div().into_any_element();
        }
        let total = rows.len();
        let sel = self.slash_sel.min(total.saturating_sub(1));
        // Eight rows show; the window slides once the choice passes them.
        let start = (sel + 1).saturating_sub(SLASH_ROWS);
        // What ↩ does to the row the keys are on: run it, or put it in
        // the message.
        let runs = at.whole && rows.get(sel).is_some_and(|row| acts_alone(&row.0));
        let to_terminal = via == "inbox" && runs;
        let at = Rc::new(at);
        let mono = theme.mono_font_family.clone();
        let keycap = |label: &'static str| {
            div()
                .h(px(16.))
                .min_w(px(16.))
                .px(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .text_size(px(10.))
                .text_color(theme.muted_foreground)
                .child(label)
        };
        let key = |cap: &'static str, does: &'static str| h_flex().flex_shrink_0().gap(px(5.)).items_center().child(keycap(cap)).child(does);
        let wash = theme.primary.opacity(if theme.is_dark() { 0.16 } else { 0.10 });
        let list = v_flex().p(px(6.)).gap(px(1.)).children(rows.into_iter().enumerate().skip(start).take(SLASH_ROWS).map(|(i, (name, description, hint))| {
            let command = format!("/{name}");
            let chosen = i == sel;
            h_flex()
                .id(SharedString::from(format!("slash-{i}")))
                .w_full()
                .h(px(32.))
                .px(px(10.))
                .gap(px(14.))
                .items_center()
                .rounded(px(8.))
                .cursor_pointer()
                .map(|d| if chosen { d.bg(wash) } else { d.hover(|s| s.bg(theme.list_hover)) })
                .on_click(cx.listener({
                    let at = Rc::clone(&at);
                    move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.slash_pick(&name, &at, window, cx);
                        cx.notify();
                    }
                }))
                .child(
                    h_flex()
                        .w(gpui::relative(0.36))
                        .flex_shrink_0()
                        .min_w_0()
                        .overflow_hidden()
                        .gap(px(8.))
                        .items_baseline()
                        .font_family(mono.clone())
                        .child(div().flex_shrink_0().max_w_full().truncate().text_size(px(12.5)).text_color(if chosen { theme.link } else { theme.foreground }).child(command))
                        .when(!hint.is_empty(), |d| d.child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(theme.muted_foreground.opacity(0.8)).child(hint))),
                )
                .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(if chosen { theme.foreground } else { theme.muted_foreground }).child(description))
                .when(chosen, |d| d.child(keycap("↩")))
        }));
        let foot = h_flex()
            .h(px(32.))
            .px(px(16.))
            .gap(px(14.))
            .items_center()
            .border_t_1()
            .border_color(theme.border)
            .text_size(px(11.))
            .text_color(theme.muted_foreground)
            .child(h_flex().flex_shrink_0().gap(px(3.)).items_center().child(keycap("↑")).child(keycap("↓")).child(div().pl(px(2.)).child("choose")))
            .child(key("↩", if runs { "run" } else { "insert" }))
            .when(runs, |d| d.child(key("⇥", "insert")))
            .child(key("esc", "close"))
            .child(div().flex_1())
            .child(div().min_w_0().truncate().child(if loading {
                "reading this folder's commands…".to_string()
            } else if total > SLASH_ROWS {
                format!("{} of {total}", sel + 1)
            } else {
                String::new()
            }))
            .when(to_terminal, |d| {
                d.child(
                    h_flex()
                        .id("slash-terminal")
                        .flex_shrink_0()
                        .gap(px(5.))
                        .items_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.foreground))
                        .on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.go_to_terminal(cx);
                        }))
                        .child(Icon::new(IconName::SquareTerminal).with_size(px(12.)))
                        .child("runs in the terminal"),
                )
            });
        v_flex()
            .w_full()
            .max_w(CONTENT_W)
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow(float_shadow(&theme))
            .overflow_hidden()
            .child(list)
            .child(foot)
            .into_any_element()
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
        let can_send = matches!(via, "driver" | "spawn" | "inbox" | "pty");
        // A terminal session's mode, model and effort are changed in the
        // terminal, which the same lists do for it (`set_mode` and the
        // rest): the inbox reads everything as prose.
        let settable = can_send;
        let effort = self.current_effort();
        let on_session = self.page == Page::Session;
        let options = self.options();
        let effort_text = effort_pill(&options, &model, &effort, &theme);
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
            .rounded(px(20.))
            .border_1()
            .border_color(if theme.mode.is_dark() { theme.secondary_active } else { theme.border })
            .bg(theme.popover)
            .shadow(float_shadow(&theme))
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
                        } else if this.slash_key(SlashKey::Run, window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    // While the list of slash commands is open, the arrows,
                    // Tab and Escape are its keys, not the textarea's.
                    .capture_action(cx.listener(|this, _: &gpui_component::input::MoveUp, window, cx| {
                        if this.slash_key(SlashKey::Up, window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &gpui_component::input::MoveDown, window, cx| {
                        if this.slash_key(SlashKey::Down, window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &gpui_component::input::IndentInline, window, cx| {
                        if this.slash_key(SlashKey::Complete, window, cx) || this.accept_suggestion(window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &gpui_component::input::MoveRight, window, cx| {
                        if this.accept_suggestion(window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                        // The slash list first, then anything else open,
                        // then the running turn: the textarea would keep
                        // the key to itself otherwise.
                        if this.slash_key(SlashKey::Close, window, cx) {
                            cx.stop_propagation();
                        } else if this.settings_open || this.lightbox.is_some() || this.search_open || this.find_open || this.term_open.is_some() {
                            this.escape(window, cx);
                            cx.stop_propagation();
                        } else if this.escape_stops(cx) {
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
                    .gap(px(8.))
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
                    // The pills are buttons, on every channel: a choice is
                    // made in the session's terminal (`pill_clicked`), which
                    // is opened first when the session has none. Before a
                    // session exists there is nothing to open, and they say
                    // what a new one starts in, set in Settings.
                    .when(settable, |d| {
                        let to = |pill: Pill| cx.listener(move |this, _: &ClickEvent, window, cx| this.pill_clicked(pill, window, cx));
                        // A mode changed in a terminal that cannot be read
                        // is not known until the next turn.
                        let mode_text = if mode.is_empty() { PillText { value: String::new(), tint: Tint::Plain, tail: "Mode" } } else { mode_pill(&options, &mode, &theme) };
                        d.child(composer_pill("mode", "icons/shield.svg", mode_text, cx).on_click(to(Pill::Mode)))
                            .when(on_session, |d| d.child(composer_pill("effort", "icons/gauge.svg", effort_text.clone(), cx).on_click(to(Pill::Effort))))
                            .child(div().flex_1())
                            .child(composer_pill("model", "icons/box.svg", PillText::plain(model_name(&options, &model)), cx).on_click(to(Pill::Model)))
                    })
                    .when(!settable, |d| d.child(div().flex_1()))
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
        // A message that is a slash command being typed opens the list of
        // commands over the card; it closes as soon as a space or a line
        // follows the name.
        let help = self.slash_open(cx).map(|at| self.render_slash_help(at, cx));
        v_flex().w_full().items_center().gap(px(8.)).children(help).child(card).child(foot)
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

        // The four live columns side by side when the pane has room for
        // them, two by two when it does not, one under another when it is
        // narrow: the board never scrolls sideways. The columns are open
        // regions under a hairline, not grey slabs, and done is a row
        // under them that opens into a grid.
        let gap = px(20.);
        let per_row = {
            let avail = self.pane_w - px(48.);
            if avail >= COL_MIN_W * 4. + gap * 3. {
                4
            } else if avail >= COL_MIN_W * 2. + gap {
                2
            } else {
                1
            }
        };
        let mut rows = v_flex().w_full().gap(px(28.));
        for chunk in Column::LIVE.chunks(per_row) {
            let mut row = h_flex().w_full().gap(gap).items_start();
            for c in chunk {
                let cards = cols.remove(c).unwrap_or_default();
                row = row.child(self.render_column(*c, cards, cx));
            }
            for _ in chunk.len()..per_row {
                row = row.child(div().flex_1());
            }
            rows = rows.child(row);
        }

        v_flex().flex_1().min_w_0().h_full().bg(theme.background).child(self.render_topbar("Board".into(), vec![stats], cx)).child(
            v_flex()
                .id("board")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(24.))
                .pt(px(8.))
                .pb(px(32.))
                .child(page_in("page-board", v_flex().w_full().gap(px(28.)).child(rows).child(self.render_done(done, per_row, cx)))),
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

    /// A column's heading: the dot, the name and the count over a hairline.
    fn column_head(&self, c: Column, count: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        h_flex()
            .w_full()
            .pb(px(10.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(self.dot(c, cx))
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(c.title()))
            .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(count.to_string()))
    }

    fn render_column(&self, c: Column, cards: Vec<Card>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let count = cards.len();
        let mut col = v_flex().flex_1().min_w_0().gap(px(10.)).child(self.column_head(c, count, cx));
        if cards.is_empty() {
            // An empty column says so inside a dashed outline the height
            // of a card, so the page keeps its shape and the words are
            // where a card would be, not floating in a tall void.
            col = col.child(
                div()
                    .w_full()
                    .h(px(84.))
                    .rounded(px(12.))
                    .border_1()
                    .border_dashed()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(16.))
                    .text_size(px(12.))
                    .text_center()
                    .text_color(theme.muted_foreground.opacity(0.8))
                    .child(c.empty()),
            );
        } else {
            let shown = cards.len().min(60);
            let more = cards.len() - shown;
            col = col
                .children(cards.into_iter().take(shown).map(|card| self.render_card(card, cx)))
                .when(more > 0, |d| d.child(div().py(px(8.)).text_size(px(11.)).text_color(theme.muted_foreground).text_center().child(format!("{more} more in the list"))));
        }
        col.into_any_element()
    }

    /// Done is a row under the live columns: the count and how many are
    /// kept, with a chevron. Opened, every finished session as a grid in
    /// as many columns as the live ones, newest first.
    fn render_done(&self, done: Vec<Card>, per_row: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let kept = done.iter().filter(|c| c.r.archived).count();
        let open = self.done_open;
        let head = h_flex()
            .id("done-head")
            .w_full()
            .pb(px(10.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.done_open = !this.done_open;
                cx.notify();
            }))
            .child(self.dot(Column::Done, cx))
            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("done"))
            .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(done.len().to_string()))
            .when(kept > 0, |d| d.child(badge_str(format!("{kept} kept"), theme.muted, theme.muted_foreground)))
            .child(div().flex_1())
            .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(if open { "hide" } else { "show" }))
            .child(Icon::new(if open { IconName::ChevronUp } else { IconName::ChevronDown }).with_size(px(13.)).text_color(theme.muted_foreground));
        let mut section = v_flex().w_full().gap(px(10.)).child(head);
        if open {
            let shown = done.len().min(60);
            let more = done.len() - shown;
            let mut grid = v_flex().w_full().gap(px(10.));
            let cards: Vec<Card> = done.into_iter().take(shown).collect();
            let mut iter = cards.into_iter().peekable();
            while iter.peek().is_some() {
                let mut row = h_flex().w_full().gap(px(10.)).items_start();
                let mut n = 0;
                for card in iter.by_ref().take(per_row) {
                    row = row.child(div().flex_1().min_w_0().child(self.render_card(card, cx)));
                    n += 1;
                }
                for _ in n..per_row {
                    row = row.child(div().flex_1());
                }
                grid = grid.child(row);
            }
            section = section.child(grid);
            if more > 0 {
                section = section.child(div().py(px(8.)).text_size(px(11.)).text_color(theme.muted_foreground).text_center().child(format!("{more} more in the list")));
            }
        }
        section.into_any_element()
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
            .rounded(px(12.))
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

    /// The greeting in the display serif, the composer, and the folders a
    /// session can start in as a grid of cards: the folder's own name in
    /// the foreground, its parents dimmed, ringed in the accent when
    /// chosen. The folders used to be a cloud of full-path pills of every
    /// width, which read as clutter.
    fn render_new(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let cwds: Vec<String> = self.recent_cwds().into_iter().take(HOME_FOLDERS).collect();
        let chosen = self.new_cwd.clone();
        let display = crate::fonts::display_family(cx);
        let live = self.refs.iter().filter(|r| self.live_color(r, cx).is_some()).count();
        let line = format!("{} · {} live · {} kept", today_line(), live, self.refs.len());
        // Three cards to a row, each a third of the column, so the grid
        // fills the width exactly whatever the window; a wrapping row of
        // fixed widths fell to two per row with the sidebar open.
        let folder_card = |c: String, cx: &mut Context<Self>| {
            let active = c == chosen;
            let theme = cx.theme().clone();
            let path = std::path::Path::new(&c);
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| c.clone());
            let parent = path.parent().map(|p| emaki_core::paths::tilde(&p.to_string_lossy())).filter(|p| !p.is_empty()).unwrap_or_else(|| "/".into());
            let hover_border = theme.muted_foreground.opacity(0.45);
            let menu_cwd = c.clone();
            h_flex()
                .id(SharedString::from(format!("cwd-{c}")))
                .flex_1()
                .min_w_0()
                .h(px(58.))
                .px(px(12.))
                .gap(px(10.))
                .items_center()
                .rounded(px(12.))
                .border_1()
                .bg(theme.popover)
                .border_color(if active { theme.primary } else { theme.border })
                .when(active, |d| d.bg(theme.primary.opacity(if theme.mode.is_dark() { 0.12 } else { 0.06 })))
                .when(!active, |d| d.hover(move |s| s.border_color(hover_border)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.new_cwd = c.clone();
                    cx.notify();
                }))
                .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, _, cx| this.open_menu(ev.position, vec![(crate::sys::OPEN_FOLDER_LABEL, MenuDo::OpenFolder(Some(menu_cwd.clone())))], cx)))
                .child(
                    div()
                        .size(px(32.))
                        .rounded(px(9.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(if active { theme.primary.opacity(0.16) } else { theme.muted })
                        .child(Icon::new(IconName::Folder).with_size(px(15.)).text_color(if active { theme.primary } else { theme.muted_foreground })),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(1.))
                        .child(div().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(name))
                        .child(div().truncate().text_size(px(11.)).text_color(theme.muted_foreground).child(parent)),
                )
        };
        let mut folders = v_flex().w_full().gap(px(10.));
        for row in cwds.chunks(FOLDER_COLS) {
            let mut line = h_flex().w_full().gap(px(10.)).items_center();
            for c in row {
                line = line.child(folder_card(c.clone(), cx));
            }
            for _ in row.len()..FOLDER_COLS {
                line = line.child(div().flex_1());
            }
            folders = folders.child(line);
        }

        v_flex().flex_1().min_w_0().h_full().bg(theme.background).child(self.render_topbar(String::new(), Vec::new(), cx)).child(
            v_flex().id("home").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.home_scroll).px(px(24.)).child(
                page_in(
                    "page-home",
                    v_flex()
                        .w_full()
                        .min_h_full()
                        .items_center()
                        .justify_center()
                        .pb(px(48.))
                        .gap(px(22.))
                        .child(
                            v_flex()
                                .items_center()
                                .gap(px(10.))
                                .child(h_flex().gap(px(14.)).items_center().child(img("icon/app.png").size(px(44.)).flex_shrink_0()).child(div().text_size(px(36.)).font_family(display).child(greeting(&self.user_name))))
                                .child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child(line)),
                        )
                        .child(self.render_composer(cx))
                        .child(v_flex().w_full().max_w(CONTENT_W).gap(px(10.)).pt(px(10.)).child(div().px(px(2.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("START IN")).child(folders)),
                ),
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
        // The scrim is a flex box and the palette its child, so the
        // palette sits in the middle of the window (an absolute panel with
        // auto margins stayed at the left edge).
        div()
            .id("search-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .flex_col()
            .items_center()
            .pt(px(80.))
            .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))
            .child(
                v_flex()
                    .id("search-panel")
                    .key_context(SEARCH_CONTEXT)
                    .on_action(cx.listener(|this, _: &Escape, window, cx| this.close_search(window, cx)))
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(px(680.))
                    .max_w(gpui::relative(0.94))
                    .max_h(px(520.))
                    .rounded(px(18.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
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

/// A page's content as it arrives: it fades and settles in over a moment,
/// once, keyed on the page so switching back plays it again. Pages only;
/// the conversation's list items are never animated.
pub fn page_in(id: &'static str, el: Div) -> AnyElement {
    el.with_animation(ElementId::Name(id.into()), Animation::new(Duration::from_millis(240)).with_easing(ease_out_quint()), |d, t| d.opacity(t).pt(px(8. * (1. - t)))).into_any_element()
}

/// The shadow under a floating card (the composer, a palette): a wide soft
/// drop in the ink's own hue and a hairline of contact under it, so the
/// card lifts off the page rather than sitting in a grey halo.
pub fn float_shadow(theme: &gpui_component::Theme) -> Vec<BoxShadow> {
    let dark = theme.mode.is_dark();
    let ink = if dark { gpui::black() } else { theme.foreground };
    vec![
        BoxShadow { color: ink.opacity(if dark { 0.45 } else { 0.08 }), offset: point(px(0.), px(12.)), blur_radius: px(32.), spread_radius: px(-8.), inset: false },
        BoxShadow { color: ink.opacity(if dark { 0.3 } else { 0.05 }), offset: point(px(0.), px(1.)), blur_radius: px(3.), spread_radius: px(0.), inset: false },
    ]
}

/// A session's state as a small chip: a dot in the column's colour and
/// the column's name, for rows that are live.
pub fn state_chip(column: Column, color: Hsla, theme: &gpui_component::Theme) -> impl IntoElement {
    h_flex()
        .flex_shrink_0()
        .h(px(22.))
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .rounded_full()
        .bg(color.opacity(if theme.mode.is_dark() { 0.16 } else { 0.10 }))
        .child(div().size(px(6.)).rounded_full().bg(color))
        .child(div().text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(color).child(column.title()))
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

/// Claude's own mark, the starburst, for the agent's glyph.
pub fn claude_icon(size: Pixels, color: Hsla) -> Icon {
    Icon::default().path("icons/claude.svg").with_size(size).text_color(color)
}

/// The glyph that says which agent a session belongs to.
pub fn agent_icon(agent: AgentId, size: Pixels, color: Hsla) -> Icon {
    match agent {
        AgentId::ClaudeCode => claude_icon(size, color),
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
        // Claude's mark turns once every 2.8 s and breathes twice a turn
        // (down to 82% of its size and 55% opacity), what the Python
        // viewer's `spark` did; a fixed box keeps the breathing from
        // moving anything around it.
        AgentId::ClaudeCode => div()
            .size(size)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(agent_icon(agent, size, color).with_animation(ElementId::Name(id), Animation::new(Duration::from_millis(2800)).repeat(), move |icon, t| {
                let breath = 0.5 - 0.5 * (t * 4.0 * std::f32::consts::PI).cos();
                icon.rotate(gpui::Radians(t * std::f32::consts::TAU)).with_size(size * (1.0 - 0.18 * breath)).opacity(1.0 - 0.45 * breath)
            }))
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
/// The question `EMAKI_QUESTION=1` holds on the session opened, for a
/// look at the card from a script.
fn sample_question() -> PermissionRequest {
    let input = serde_json::json!({"questions": [{
        "question": "Commit the compaction and mid-turn message fixes and push to origin, no version bump?",
        "header": "Commit",
        "options": [
            {"label": "Yes, commit and push (Recommended)", "description": "One commit with the core builder changes, tests, AGENTS.md and CHANGELOG, pushed to origin."},
            {"label": "Commit only", "description": "Make the commit but leave the push to you."},
            {"label": "Not yet", "description": "Leave the working tree as it is."}
        ],
        "multiSelect": false
    }]});
    PermissionRequest { request_id: "probe-q".into(), tool_name: "AskUserQuestion".into(), tool_use_id: "probe-q".into(), input: input.as_object().cloned().unwrap_or_default(), description: String::new(), asked_at: 0. }
}

/// How many rows of the slash-command list show at once.
const SLASH_ROWS: usize = 8;

/// The slash command being typed at the caret (`Workbench::slash_open`).
struct SlashAt {
    via: &'static str,
    start: usize,
    end: usize,
    prefix: String,
    whole: bool,
    text: String,
}

/// A key the slash-command list takes from the composer.
#[derive(Clone, Copy)]
enum SlashKey {
    Up,
    Down,
    Run,
    Complete,
    Close,
}

/// The name of the slash command `text` is, when it is one: a first line
/// starting with "/" and a name after it.
pub fn slash_command(text: &str) -> Option<String> {
    let first = text.lines().next().unwrap_or("").trim();
    let name = first.strip_prefix('/')?.split_whitespace().next()?;
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ':')).then(|| name.to_string())
}


/// A key pressed on the terminal card, as a terminal sends it. None for
/// a key that is the app's own (anything with ⌘) or sends nothing.
fn term_bytes(k: &Keystroke) -> Option<Vec<u8>> {
    let m = &k.modifiers;
    if m.platform {
        return None;
    }
    let named: Option<&[u8]> = match k.key.as_str() {
        "enter" => Some(b"\r"),
        "escape" => Some(b"\x1b"),
        "backspace" => Some(b"\x7f"),
        "tab" if m.shift => Some(b"\x1b[Z"),
        "tab" => Some(b"\t"),
        "space" => Some(b" "),
        "up" => Some(b"\x1b[A"),
        "down" => Some(b"\x1b[B"),
        "right" => Some(b"\x1b[C"),
        "left" => Some(b"\x1b[D"),
        "home" => Some(b"\x1b[H"),
        "end" => Some(b"\x1b[F"),
        "delete" => Some(b"\x1b[3~"),
        "pageup" => Some(b"\x1b[5~"),
        "pagedown" => Some(b"\x1b[6~"),
        _ => None,
    };
    if let Some(bytes) = named {
        return Some(bytes.to_vec());
    }
    if m.control {
        let c = k.key.chars().next().filter(|c| k.key.len() == 1 && c.is_ascii_alphabetic())?;
        return Some(vec![c.to_ascii_lowercase() as u8 & 0x1f]);
    }
    let typed = k.key_char.clone().or_else(|| (k.key.chars().count() == 1).then(|| k.key.clone()))?;
    Some(typed.into_bytes())
}

/// Whether Claude Code is set to one of its light themes (`theme` in
/// `~/.claude.json`, dark when it says nothing): the colours it writes
/// are made for that ground, so the terminal card is drawn on it.
fn claude_theme_light() -> bool {
    static LIGHT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LIGHT.get_or_init(|| {
        emaki_core::paths::read_json(&emaki_core::paths::home().join(".claude.json"))
            .and_then(|v| v.get("theme").and_then(|t| t.as_str()).map(|t| t.starts_with("light")))
            .unwrap_or(false)
    })
}

/// What a session's terminal is wanted for (`Workbench::via_terminal`).
#[derive(Debug, Clone)]
pub enum TerminalAction {
    /// Only to be there: a question's dialog, an approval.
    Go,
    /// The model or effort pill: `/model` or `/effort`, Claude Code's own picker.
    Pick(Pill),
    /// A command typed and sent.
    Run(String),
    /// ⇧Tab, for the next mode.
    StepMode,
}

/// Which of the three pills under the composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pill {
    Mode,
    Model,
    Effort,
}

/// One write of keys to the terminal's dialog, and what its screen has
/// to show before the next one goes.
struct DialogStep {
    keys: String,
    until: Option<DialogUntil>,
}

enum DialogUntil {
    /// The terminal's pointer is on that choice.
    CursorOn(u32),
    /// That choice reads as these words: its field has taken them.
    Says(u32, String),
    /// That tab is the one showing.
    Tab(usize),
}

impl DialogUntil {
    fn holds(&self, d: &emaki_core::driver::Dialog) -> bool {
        match self {
            DialogUntil::CursorOn(n) => d.options.iter().any(|o| o.n == *n && o.cursor),
            DialogUntil::Tab(ix) => d.current == Some(*ix),
            // Long words wrap onto the lines under the choice.
            DialogUntil::Says(n, words) => {
                let bare = |t: &str| t.split_whitespace().collect::<String>();
                d.options.iter().any(|o| o.n == *n && bare(&format!("{}{}", o.label, o.detail)) == bare(words))
            }
        }
    }
}

/// What brings the window back from a terminal the person was sent to
/// (`Workbench::come_back`): any one of these, whichever shows first.
pub struct ComeBack {
    sid: String,
    /// When the person left, to forget it after `COME_BACK_SECS`.
    at: f64,
    /// The transcript's time then, when a change to it is the sign: a
    /// question answered, a command run. None while the agent works,
    /// since the next change would be its own.
    mtime: Option<f64>,
    /// The model and effort the status line named then, when another of
    /// either is the sign: a pick from `/model` or `/effort`.
    pick: Option<(String, String)>,
    /// When `/model` or `/effort` was typed there: the pick is over once
    /// the registry's record has gone to `waiting` since and left it.
    typed: Option<f64>,
    /// The picker has been seen up: the record said `waiting` after the
    /// command was typed.
    picking: bool,
}

impl ComeBack {
    fn on_transcript(r: &SessionRef, now: f64) -> Self {
        Self { sid: r.session_id.clone(), at: now, mtime: Some(r.mtime), pick: None, typed: None, picking: false }
    }
}

/// The pill itself: light grey with its icon in front, darker under the
/// pointer, darker again while pressed or open. No tooltip and no caret.
/// (The toolkit draws a custom colour at a fifth of its strength, so the
/// resting grey is the ink's own, thinned.)
fn composer_pill(id: &'static str, icon: &'static str, label: PillText, cx: &App) -> Button {
    let theme = cx.theme();
    let look = ButtonCustomVariant::new(cx).color(theme.muted_foreground.opacity(0.65)).foreground(theme.muted_foreground).hover(theme.muted_foreground.opacity(0.22)).active(theme.muted_foreground.opacity(0.34));
    Button::new(SharedString::from(format!("{id}-trigger"))).custom(look).small().rounded(ButtonRounded::Size(px(999.)))
        .icon(Icon::default().path(icon))
        .child(
            h_flex()
                .gap(px(4.))
                .when(!label.value.is_empty(), |d| d.child(tinted(&label.value, &label.tint)))
                .when(!label.tail.is_empty(), |d| d.child(label.tail)),
        )
}

/// A pill that opens a list of choices, in the settings panel: the mode
/// and the model a new session starts in. (The pills under the composer
/// had these lists too, until every choice on a session came to be made
/// in its terminal.) Every choice is on the list with a line on what it
/// does, the current one ticked, so all of them can be reached (a pill that
/// cycled on click hid the fourth mode behind three clicks and a label that
/// did not know it). Picking one calls `on` with its key.
#[allow(clippy::too_many_arguments)]
fn picker(
    id: &'static str,
    icon: &'static str,
    label: PillText,
    options: Vec<(String, String, String)>,
    current: String,
    anchor: Anchor,
    wb: WeakEntity<Workbench>,
    on: Rc<dyn Fn(&mut Workbench, &str, &mut Context<Workbench>)>,
    cx: &App,
) -> impl IntoElement {
    let options = Rc::new(options);
    let trigger = composer_pill(id, icon, label, cx);
    Popover::new(id).anchor(anchor).trigger(trigger).content(move |_, _, cx| {
        let theme = cx.theme().clone();
        let popover = cx.entity();
        // The list is as long as the agent makes it (a dozen models), so
        // it scrolls past a height the smallest window still has room for.
        v_flex().id(SharedString::from(format!("{id}-list"))).min_w(px(250.)).max_h(px(336.)).overflow_y_scroll().gap(px(2.)).children(options.iter().map(|(key, name, detail)| {
            let active = *key == current;
            let (wb, on, popover, key) = (wb.clone(), on.clone(), popover.clone(), key.clone());
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
                    let _ = wb.update(cx, |this, cx| on(this, &key, cx));
                    popover.update(cx, |s, cx| s.dismiss(window, cx));
                })
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(name.clone()))
                        .when(active, |d| d.child(Icon::new(IconName::Check).with_size(px(12.)).text_color(theme.primary))),
                )
                .when(!detail.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(detail.clone())))
        }))
    })
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_draft(window, cx);
        self.sync_folders(cx);
        // The focused element must be one this page draws. gpui dispatches a
        // keystroke from the focused node, or from the window root when that
        // node is not in the frame, and the root is above every handler here:
        // with the caret left in a composer the board does not draw, ⌘W and
        // every other shortcut went nowhere.
        if matches!(self.page, Page::Board | Page::Sessions) && self.composer.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.focus_handle, cx);
        }
        match self.term_focus_due.take() {
            Some(true) => window.focus(&self.term_focus, cx),
            Some(false) if self.page == Page::Session => {
                let handle = self.composer.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            _ => {}
        }
        if let Some(words) = self.send_probe.take_if(|_| (self.page == Page::Session && self.detail.is_some()) || (self.page == Page::New && !self.new_cwd.is_empty())) {
            self.composer.update(cx, |s, cx| s.set_value(words, window, cx));
            self.send_message(window, cx);
        }
        if std::mem::take(&mut self.restore_due) {
            let this = cx.entity();
            window.defer(cx, move |window, cx| this.update(cx, |this, cx| this.restore_prompt(window, cx)));
        }
        // The composer is one field for both pages; its placeholder says
        // what a message here does. Set only when it differs, since the
        // setter notifies.
        // A prompt the terminal suggests stands there instead, in the
        // placeholder's own lighter ink: words offered, not yet said.
        let want = if self.question_pending().is_some() {
            PLACEHOLDER_ANSWER.to_string()
        } else if self.page == Page::New {
            PLACEHOLDER_NEW.to_string()
        } else if let Some(words) = self.suggested() {
            format!("{words}   (→ to accept, ⌘↩ to send)")
        } else {
            PLACEHOLDER_REPLY.to_string()
        };
        if self.composer_placeholder != want {
            self.composer_placeholder = want.clone();
            self.composer.update(cx, |s, cx| s.set_placeholder(want, window, cx));
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
        self.sync_tab_widths(cx);
        self.pane_w = window.viewport_size().width - if sidebar_open { SIDEBAR_W } else { px(0.) };
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
            // A raw wheel listener in the capture phase, registered at paint
            // (nothing is drawn): see `route_scroll`.
            .child({
                let this = cx.entity().downgrade();
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let scroll = this.clone();
                        window.on_mouse_event(move |e: &ScrollWheelEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = scroll.update(cx, |w, cx| w.route_scroll(e, cx));
                            }
                        });
                        // A floating sidebar goes when the pointer leaves it.
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = this.update(cx, |w, cx| w.float_follow(e, window, cx));
                            }
                        });
                    },
                )
                .absolute()
                .size_0()
            })
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
                this.page = Page::Board;
                this.save_ui(true);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| match this.selected.clone() {
                Some(key) if this.page == Page::Session && this.tabs.contains(&key) => this.close_tab(&key, window, cx),
                _ => window.remove_window(),
            }))
            .on_action(cx.listener(|this, _: &GoSessions, _, cx| this.show_sessions(Scope::All, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(false, cx)))
            .on_action(cx.listener(|this, _: &Tab1, window, cx| this.go_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &Tab2, window, cx| this.go_tab(2, window, cx)))
            .on_action(cx.listener(|this, _: &Tab3, window, cx| this.go_tab(3, window, cx)))
            .on_action(cx.listener(|this, _: &Tab4, window, cx| this.go_tab(4, window, cx)))
            .on_action(cx.listener(|this, _: &Tab5, window, cx| this.go_tab(5, window, cx)))
            .on_action(cx.listener(|this, _: &Tab6, window, cx| this.go_tab(6, window, cx)))
            .on_action(cx.listener(|this, _: &Tab7, window, cx| this.go_tab(7, window, cx)))
            .on_action(cx.listener(|this, _: &Tab8, window, cx| this.go_tab(8, window, cx)))
            .on_action(cx.listener(|this, _: &Tab9, window, cx| this.go_tab(9, window, cx)))
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
            .children(self.render_sidebar_float(cx))
            .child(self.render_strip(cx))
            .when(search_open, |d| d.child(self.render_search(cx)))
            .when(self.settings_open, |d| d.child(self.render_settings(cx)))
            .when_some(self.lightbox.clone(), |d, lb| d.child(self.render_lightbox(lb, cx)))
            .when(self.renaming.is_some(), |d| d.child(self.render_rename(cx)))
            .when_some(self.menu.clone(), |d, m| d.child(self.render_menu(m, window, cx)))
    }
}
