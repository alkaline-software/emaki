//! The window: sidebar, session list, board, search, transcript and composer.
//!
//! State lives here; everything shown comes from the transcript through the
//! core, and everything typed goes out through the hub. The board is the home
//! page: the question the window answers on arrival is "what needs me".

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::Disableable as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};
use scribe_core::adapters;
use scribe_core::build::Phase;
use scribe_core::config::Config;
use scribe_core::driver::{self, PermissionRequest, MODELS, MODES};
use scribe_core::model::{AgentId, Session};
use scribe_core::search::Results;
use scribe_core::transcript::SessionRef;

use crate::format::{clock, day, elapsed_since, now_secs, plural, relative, short_id};
use crate::hub::{Hub, HubEvent};

actions!(scribe, [ToggleSearch, Refresh, NewSession, GoBoard, Escape, Send]);

pub const KEY_CONTEXT: &str = "Workbench";
pub const COMPOSER_CONTEXT: &str = "Composer";
pub const SEARCH_CONTEXT: &str = "SearchPalette";

pub const SIDEBAR_W: Pixels = px(224.);
pub const LIST_W: Pixels = px(336.);
pub const TITLEBAR_H: Pixels = px(44.);
/// A board column never gets narrower than this; past that the board scrolls.
pub const COL_MIN_W: Pixels = px(210.);

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
    pub open_tools: HashSet<(usize, usize)>,
    pub open_thoughts: HashSet<(usize, usize)>,
    pub open_subagents: HashSet<(usize, usize)>,
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
    pub selected: Option<String>,
    pending_select: Option<String>,
    pub detail: Option<Detail>,
    loading: Option<String>,
    load_task: Option<Task<()>>,
    pub composer: Entity<TextareaState>,
    pub search_input: Entity<InputState>,
    pub search_open: bool,
    pub search_results: Option<Results>,
    search_task: Option<Task<()>>,
    pub done_open: bool,
    pub permissions: Vec<(String, PermissionRequest)>,
    pub drivers: HashMap<String, DriverView>,
    pub new_cwd: String,
    pub new_id: String,
    pub next_mode: String,
    pub next_model: String,
    pub status: String,
    pub now: f64,
    startup_open: Option<String>,
    focus_handle: FocusHandle,
    _tasks: Vec<Task<()>>,
}

pub fn key_of(r: &SessionRef) -> String {
    format!("{}:{}", r.agent.as_str(), r.session_id)
}

impl Workbench {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = Config::load();
        let (hub, mut rx) = Hub::start(cfg.clone());

        let composer = cx.new(|cx| TextareaState::new(window, cx).placeholder("Message this session…  (⌘↩ to send)"));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search every conversation, live and kept"));

        // Every headless child is ours to close: a driver left running after
        // the window is gone would keep writing to a transcript nobody reads.
        let hub_for_quit = Arc::clone(&hub);
        cx.on_app_quit(move |_, _| {
            hub_for_quit.stop_all();
            async {}
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
        cx.subscribe_in(&composer, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { secondary: true, .. } = ev {
                this.send_message(window, cx);
            }
        })
        .detach();

        let next_mode = cfg.driver.default_mode.clone();
        let next_model = cfg.driver.default_model.clone();
        // `SCRIBE_OPEN=<session-id prefix>` opens a session on launch.
        let startup_open = std::env::var("SCRIBE_OPEN").ok().filter(|s| !s.is_empty());
        Self {
            hub,
            cfg,
            refs: Vec::new(),
            scope: Scope::All,
            page: Page::Board,
            selected: None,
            pending_select: None,
            detail: None,
            loading: None,
            load_task: None,
            composer,
            search_input,
            search_open: false,
            search_results: None,
            search_task: None,
            done_open: false,
            permissions: Vec::new(),
            drivers: HashMap::new(),
            new_cwd: String::new(),
            new_id: String::new(),
            next_mode,
            next_model,
            status: "scanning…".into(),
            now: now_secs(),
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
                if self.status == "scanning…" {
                    self.status = format!("{} sessions", self.refs.len());
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
            HubEvent::Archived(stats) => {
                self.status = format!("archived {} file{}", stats.files, if stats.files == 1 { "" } else { "s" });
                cx.notify();
            }
            HubEvent::SearchSynced(report) => {
                self.status = format!("indexed {} session{}", report.indexed, if report.indexed == 1 { "" } else { "s" });
                cx.notify();
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
                self.status = format!("could not start claude: {error}");
                cx.notify();
            }
            HubEvent::Sent { session_id, queued, error } => {
                if !error.is_empty() {
                    self.status = format!("not sent: {error}");
                } else if queued {
                    self.status = "queued behind the running turn".into();
                } else {
                    self.status = "sent".into();
                }
                if let Some(v) = self.drivers.get_mut(&session_id) {
                    if error.is_empty() && !queued {
                        v.state = "running".into();
                    }
                }
                cx.notify();
            }
            HubEvent::Driver { session_id, event } => self.on_driver_event(session_id, event, cx),
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
                    self.status = format!("turn ended: {}", r.subtype);
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
                    self.status = format!("claude exited: {error}");
                }
            }
        }
        cx.notify();
    }

    // -- selection ----------------------------------------------------------

    pub fn open_session(&mut self, key: &str, cx: &mut Context<Self>) {
        self.page = Page::Sessions;
        self.selected = Some(key.to_string());
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

    fn load_detail(&mut self, r: SessionRef, cx: &mut Context<Self>) {
        let key = key_of(&r);
        self.loading = Some(key.clone());
        let path = r.path.clone();
        let task = cx.background_spawn(async move { adapters::for_agent(r.agent).load(&r) });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let session = task.await;
            this.update(cx, |this, cx| {
                this.loading = None;
                if this.selected.as_deref() != Some(key.as_str()) {
                    return;
                }
                this.set_detail(key, path, session);
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
                if n >= old && old > 0 {
                    d.list.splice(old - 1..old, n - old + 1);
                } else {
                    d.list.reset(n);
                }
            }
            _ => {
                let list = ListState::new(n, ListAlignment::Bottom, px(512.));
                self.detail = Some(Detail {
                    key,
                    path,
                    session: Rc::new(session),
                    list,
                    open_tools: HashSet::new(),
                    open_thoughts: HashSet::new(),
                    open_subagents: HashSet::new(),
                });
            }
        }
    }

    pub fn selected_ref(&self) -> Option<&SessionRef> {
        let key = self.selected.as_deref()?;
        self.refs.iter().find(|r| key_of(r) == key)
    }

    fn show_new(&mut self, cwd: Option<String>, cx: &mut Context<Self>) {
        self.page = Page::New;
        self.selected = None;
        if let Some(c) = cwd {
            self.new_cwd = c;
        } else if self.new_cwd.is_empty() {
            self.new_cwd = self.recent_cwds().first().cloned().unwrap_or_default();
        }
        cx.notify();
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
        if r.archived {
            return ("", "kept only: Claude Code no longer has this transcript, so it cannot be resumed".into());
        }
        if !self.cfg.driver.enabled {
            return ("", "the driver is off in config".into());
        }
        if r.cwd.is_empty() || !std::path::Path::new(&r.cwd).is_dir() {
            return ("", "the session's folder is gone".into());
        }
        if self.hub.is_live(r, self.now) && !self.hub.has_ended(&r.session_id) {
            // A fresh file with no driver: most likely a terminal session that
            // just wrote. Sending would fork it, so the composer waits.
            return ("wait", "this session is running in a terminal; when it finishes, messages here resume it".into());
        }
        ("spawn", String::new())
    }

    pub fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        let (via, why) = self.reply_via();
        match via {
            "driver" => {
                let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
                if !self.hub.send_to_driver(&sid, text.clone(), Vec::new()) {
                    self.status = "the driver is gone; try again".into();
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
                self.hub.spawn_driver_and_send(sid.clone(), cwd, resume, self.next_mode.clone(), self.next_model.clone(), Some((text.clone(), Vec::new())));
                if self.page == Page::New {
                    let key = format!("claude-code:{sid}");
                    self.pending_select = Some(key.clone());
                    self.selected = Some(key);
                    self.new_id = String::new();
                    self.page = Page::Sessions;
                }
                self.status = "starting claude…".into();
            }
            _ => {
                self.status = why;
                cx.notify();
                return;
            }
        }
        self.composer.update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    fn cycle_mode(&mut self, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let allow_bypass = self.cfg.driver.allow_bypass;
        let current = self.drivers.get(&sid).map(|v| v.mode.clone()).filter(|m| !m.is_empty()).unwrap_or_else(|| self.next_mode.clone());
        let modes: Vec<&str> = MODES.iter().copied().filter(|m| allow_bypass || *m != "bypassPermissions").collect();
        let i = modes.iter().position(|m| *m == current).map(|i| (i + 1) % modes.len()).unwrap_or(0);
        let next = modes[i].to_string();
        self.next_mode = next.clone();
        if let Some(d) = self.hub.driver_for(&sid) {
            let n = next.clone();
            std::thread::spawn(move || {
                let _ = d.set_mode(&n);
            });
            if let Some(v) = self.drivers.get_mut(&sid) {
                v.mode = next;
            }
        }
        cx.notify();
    }

    fn cycle_model(&mut self, cx: &mut Context<Self>) {
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let current = self.next_model.clone();
        let i = MODELS.iter().position(|m| *m == current).map(|i| (i + 1) % MODELS.len()).unwrap_or(1);
        let next = MODELS[i].to_string();
        self.next_model = next.clone();
        if let Some(d) = self.hub.driver_for(&sid) {
            let n = next.clone();
            std::thread::spawn(move || {
                let _ = d.set_model(&n);
            });
            if let Some(v) = self.drivers.get_mut(&sid) {
                v.model = next;
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
            self.status = "interrupting…".into();
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

    fn reply_from_board(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_session(key, cx);
        self.composer.update(cx, |s, cx| s.focus(window, cx));
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
            card.text = scribe_core::build::tool_subject(&p.tool_name, &p.input, &r.cwd);
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
        for r in &self.refs {
            if r.cwd.is_empty() || !std::path::Path::new(&r.cwd).is_dir() {
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

    // -- sidebar --------------------------------------------------------------

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut agents: Vec<(AgentId, usize)> = Vec::new();
        for a in AgentId::ALL {
            let n = self.refs.iter().filter(|r| r.agent == a).count();
            if n > 0 {
                agents.push((a, n));
            }
        }
        let kept = self.refs.iter().filter(|r| r.archived).count();
        let projects = self.projects();
        let is_board = self.page == Page::Board;
        let is_new = self.page == Page::New;

        let nav = |id: &'static str, icon: IconName, label: String, count: Option<usize>, active: bool, cx: &mut Context<Self>, on: Box<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>| {
            let theme = cx.theme().clone();
            h_flex()
                .id(id)
                .h(px(28.))
                .px(px(10.))
                .mx(px(8.))
                .gap(px(8.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(active, |d| d.bg(theme.list_active))
                .hover(|s| s.bg(theme.list_hover))
                .on_click(cx.listener(move |this, _, window, cx| on(this, window, cx)))
                .child(Icon::new(icon).with_size(px(15.)).text_color(if active { theme.foreground } else { theme.muted_foreground }))
                .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(label))
                .when_some(count, |d, n| d.child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(n.to_string())))
        };

        v_flex()
            .w(SIDEBAR_W)
            .h_full()
            .flex_shrink_0()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(div().h(TITLEBAR_H).w_full().flex_shrink_0())
            .child(nav("nav-board", IconName::LayoutDashboard, "Board".into(), None, is_board, cx, Box::new(|this, _, cx| {
                this.page = Page::Board;
                cx.notify();
            })))
            .child(nav("nav-new", IconName::Plus, "New session".into(), None, is_new, cx, Box::new(|this, _, cx| this.show_new(None, cx))))
            .child(nav("nav-search", IconName::Search, "Search".into(), None, false, cx, Box::new(|this, window, cx| this.open_search(window, cx))))
            .child(self.group_label("Sessions", cx))
            .child(nav("nav-all", IconName::Inbox, "All".into(), Some(self.refs.len()), !is_board && !is_new && self.scope == Scope::All, cx, Box::new(|this, _, cx| {
                this.scope = Scope::All;
                this.page = Page::Sessions;
                cx.notify();
            })))
            .children(agents.into_iter().map(|(a, n)| {
                let active = !is_board && !is_new && self.scope == Scope::Agent(a);
                let icon = match a {
                    AgentId::ClaudeCode => IconName::Bot,
                    AgentId::Codex => IconName::SquareTerminal,
                };
                nav(if a == AgentId::ClaudeCode { "nav-claude" } else { "nav-codex" }, icon, a.display_name().into(), Some(n), active, cx, Box::new(move |this, _, cx| {
                    this.scope = Scope::Agent(a);
                    this.page = Page::Sessions;
                    cx.notify();
                }))
            }))
            .child(nav("nav-kept", IconName::HardDrive, "Kept only".into(), Some(kept), !is_board && !is_new && self.scope == Scope::Kept, cx, Box::new(|this, _, cx| {
                this.scope = Scope::Kept;
                this.page = Page::Sessions;
                cx.notify();
            })))
            .child(self.group_label("Projects", cx))
            .child(
                v_flex().id("projects").flex_1().min_h_0().overflow_y_scroll().children(projects.into_iter().map(|(p, n)| {
                    let active = !is_board && !is_new && self.scope == Scope::Project(p.clone());
                    let theme = cx.theme().clone();
                    let label = p.clone();
                    h_flex()
                        .id(SharedString::from(format!("proj-{p}")))
                        .h(px(26.))
                        .px(px(10.))
                        .mx(px(8.))
                        .gap(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .when(active, |d| d.bg(theme.list_active))
                        .hover(|s| s.bg(theme.list_hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.scope = Scope::Project(p.clone());
                            this.page = Page::Sessions;
                            cx.notify();
                        }))
                        .child(Icon::new(IconName::Folder).with_size(px(14.)).text_color(theme.muted_foreground))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).child(label))
                        .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(n.to_string()))
                })),
            )
            .child(div().px(px(16.)).py(px(10.)).text_size(px(11.)).text_color(theme.muted_foreground).truncate().child(self.status.clone()))
    }

    fn group_label(&self, text: &'static str, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div().pt(px(14.)).pb(px(4.)).px(px(18.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(text)
    }

    // -- session list --------------------------------------------------------

    fn render_session_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let refs: Vec<SessionRef> = self.scoped_refs().into_iter().cloned().collect();
        let selected = self.selected.clone();
        let now = self.now;
        let title = match &self.scope {
            Scope::All => "All sessions".to_string(),
            Scope::Agent(a) => a.display_name().to_string(),
            Scope::Project(p) => p.clone(),
            Scope::Kept => "Kept only".to_string(),
        };
        v_flex()
            .w(LIST_W)
            .h_full()
            .flex_shrink_0()
            .bg(theme.colors.list)
            .border_r_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .h(TITLEBAR_H)
                    .px(px(16.))
                    .items_end()
                    .pb(px(10.))
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(plural(refs.len(), "session", "sessions"))),
            )
            .child(v_flex().id("session-list").flex_1().min_h_0().overflow_y_scroll().children(refs.into_iter().map(|r| {
                let key = key_of(&r);
                let active = selected.as_deref() == Some(key.as_str());
                let card = self.card_for(&r);
                let theme = cx.theme().clone();
                let live = self.hub.is_live(&r, now);
                let dot = match card.column {
                    Column::NeedsYou => Some(theme.warning),
                    Column::Working | Column::Planning => Some(theme.primary),
                    Column::YourTurn if live => Some(theme.success),
                    _ => None,
                };
                v_flex()
                    .id(SharedString::from(format!("row-{key}")))
                    .px(px(14.))
                    .py(px(9.))
                    .mx(px(8.))
                    .my(px(1.))
                    .gap(px(3.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.list_active))
                    .hover(|s| s.bg(theme.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_session(&key, cx)))
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .items_center()
                            .when_some(dot, |d, c| d.child(div().size(px(7.)).rounded_full().bg(c).flex_shrink_0()))
                            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(r.title.clone()))
                            .when(r.archived, |d| d.child(badge("kept", theme.muted, theme.muted_foreground))),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(theme.muted_foreground)
                            .child(div().truncate().child(r.project()))
                            .when(r.agent != AgentId::ClaudeCode, |d| d.child(div().child(r.agent.display_name())))
                            .child(div().child("·"))
                            .child(div().child(relative(r.mtime, now))),
                    )
                    .when(live && !card.text.is_empty(), |d| d.child(div().truncate().text_size(px(11.5)).text_color(theme.muted_foreground).child(card.text.clone())))
            })))
    }

    // -- one conversation ----------------------------------------------------

    fn render_detail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(r) = self.selected_ref().cloned() else {
            return v_flex().flex_1().h_full().items_center().justify_center().text_color(theme.muted_foreground).child("Pick a session, or press ⌘K to search everything.").into_any_element();
        };
        let Some(detail) = &self.detail else {
            return v_flex().flex_1().h_full().items_center().justify_center().text_color(theme.muted_foreground).child("loading…").into_any_element();
        };
        let session = detail.session.clone();
        let list = detail.list.clone();
        let entity = cx.entity().downgrade();
        let tokens = scribe_core::render_md::human_tokens(session.usage.total());
        let header = v_flex()
            .px(px(24.))
            .pt(px(12.))
            .pb(px(10.))
            .gap(px(4.))
            .border_b_1()
            .border_color(theme.border)
            .child(div().h(TITLEBAR_H - px(24.)))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(div().flex_1().min_w_0().truncate().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(session.title.clone()))
                    .when(r.archived, |d| d.child(badge("kept", theme.muted, theme.muted_foreground)))
                    .child(
                        Button::new("reveal").ghost().small().icon(Icon::new(IconName::Folder)).tooltip("Reveal transcript in Finder").on_click({
                            let p = r.path.clone();
                            move |_, _, _| {
                                let _ = std::process::Command::new("open").arg("-R").arg(&p).spawn();
                            }
                        }),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(10.))
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .child(div().child(scribe_core::paths::tilde(&session.cwd)))
                    .when(!session.git_branch.is_empty(), |d| d.child(div().child(format!("⎇ {}", session.git_branch))))
                    .child(div().child(format!("{} · {} · {} tokens", plural(session.rounds.len(), "round", "rounds"), plural(session.tool_count(), "tool call", "tool calls"), tokens)))
                    .when(!session.models.is_empty(), |d| d.child(div().child(session.models.last().cloned().unwrap_or_default())))
                    .child(div().child(short_id(&session.id))),
            );

        let transcript = div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(
                gpui::list(list.clone(), move |ix, window, cx| {
                    entity.upgrade().map(|e| e.update(cx, |this, cx| this.render_round(ix, window, cx))).unwrap_or_else(|| div().into_any_element())
                })
                .size_full(),
            )
            .vertical_scrollbar(&list);

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme.background)
            .child(header)
            .child(transcript)
            .child(self.render_permissions(cx))
            .child(self.render_composer(cx))
            .into_any_element()
    }

    fn render_permissions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let cwd = self.selected_ref().map(|r| r.cwd.clone()).unwrap_or_default();
        let cards: Vec<PermissionRequest> = self.permissions.iter().filter(|(s, _)| *s == sid).map(|(_, p)| p.clone()).collect();
        v_flex().w_full().px(px(24.)).gap(px(8.)).when(!cards.is_empty(), |d| d.pt(px(8.))).children(cards.into_iter().map(|p| {
            let subject = scribe_core::build::tool_subject(&p.tool_name, &p.input, &cwd);
            let detail = match p.tool_name.as_str() {
                "Bash" => p.input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                _ => scribe_core::render_md::pretty_args(&p.input, 800),
            };
            let id_allow = p.request_id.clone();
            let id_deny = p.request_id.clone();
            v_flex()
                .p(px(12.))
                .gap(px(8.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme.warning)
                .bg(theme.popover)
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(Icon::new(IconName::TriangleAlert).with_size(px(14.)).text_color(theme.warning))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(format!("Claude wants to run {}", p.tool_name)))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(theme.muted_foreground).child(subject)),
                )
                .when(!detail.is_empty(), |d| d.child(div().p(px(8.)).rounded(px(6.)).bg(theme.muted).font_family(theme.mono_font_family.clone()).text_size(px(12.)).whitespace_normal().child(detail)))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(Button::new(SharedString::from(format!("allow-{}", p.request_id))).primary().small().label("Allow").on_click(cx.listener(move |this, _, _, cx| this.answer_permission(id_allow.clone(), true, cx))))
                        .child(Button::new(SharedString::from(format!("deny-{}", p.request_id))).outline().small().label("Deny").on_click(cx.listener(move |this, _, _, cx| this.answer_permission(id_deny.clone(), false, cx)))),
                )
        }))
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (via, why) = self.reply_via();
        let sid = self.selected_ref().map(|r| r.session_id.clone()).unwrap_or_default();
        let drv = self.drivers.get(&sid).cloned();
        let mode = drv.as_ref().map(|v| v.mode.clone()).filter(|m| !m.is_empty()).unwrap_or_else(|| if self.next_mode.is_empty() { "default".into() } else { self.next_mode.clone() });
        let model = drv.as_ref().map(|v| v.model.clone()).filter(|m| !m.is_empty()).unwrap_or_else(|| if self.next_model.is_empty() { "default".into() } else { self.next_model.clone() });
        let running = drv.as_ref().map(|v| v.state == "running" || v.starting).unwrap_or(false);
        let can_send = via == "driver" || via == "spawn";
        let channel = match via {
            "driver" => match drv.as_ref().map(|v| v.state.as_str()) {
                Some("running") => "claude is working".to_string(),
                Some("starting") => "starting claude…".to_string(),
                _ => "claude is listening".to_string(),
            },
            "spawn" => "sending starts claude here and resumes this session".to_string(),
            _ => why.clone(),
        };
        v_flex()
            .w_full()
            .px(px(24.))
            .pt(px(8.))
            .pb(px(14.))
            .gap(px(6.))
            .child(
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .text_size(px(11.5))
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().min_w_0().truncate().child(channel))
                    .when(can_send, |d| {
                        d.child(chip_button("mode", format!("mode: {mode}"), cx, |this, cx| this.cycle_mode(cx)))
                            .child(chip_button("model", format!("model: {model}"), cx, |this, cx| this.cycle_model(cx)))
                    })
                    .when(running, |d| d.child(Button::new("stop").danger().small().icon(Icon::new(IconName::Pause)).label("Stop").on_click(cx.listener(|this, _, _, cx| this.interrupt(cx))))),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_end()
                    .child(
                        div()
                            .id("composer")
                            .key_context(COMPOSER_CONTEXT)
                            .on_action(cx.listener(|this, _: &Send, window, cx| this.send_message(window, cx)))
                            .flex_1()
                            .min_w_0()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.popover)
                            .child(Textarea::new(&self.composer).h(px(84.)).appearance(false).bordered(false)),
                    )
                    .child(Button::new("send").primary().icon(Icon::new(IconName::ArrowUp)).disabled(!can_send).tooltip("Send (⌘↩)").on_click(cx.listener(|this, _, window, cx| this.send_message(window, cx)))),
            )
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
        let since_of = |c: &Card| scribe_core::build::parse_ts(if c.r.state.since.is_empty() { &c.r.updated } else { &c.r.state.since }).map(|d| d.timestamp()).unwrap_or(0);
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

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme.background)
            .child(
                h_flex()
                    .h(TITLEBAR_H)
                    .px(px(24.))
                    .items_end()
                    .pb(px(10.))
                    .gap(px(12.))
                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Board"))
                    .child(
                        h_flex().gap(px(10.)).text_size(px(12.)).text_color(theme.muted_foreground).child(div().child(format!("{live} live"))).child(div().child(format!("{needs} need you"))).child(div().child(plural(projects.len(), "project", "projects"))),
                    ),
            )
            .child(
                h_flex()
                    .id("board")
                    .flex_1()
                    .min_h_0()
                    .px(px(16.))
                    .pt(px(6.))
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
            Column::NeedsYou => theme.warning,
            Column::Planning | Column::Working => theme.primary,
            Column::YourTurn => theme.success,
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
            .rounded(px(10.))
            .child(
                h_flex()
                    .px(px(12.))
                    .pt(px(10.))
                    .pb(px(8.))
                    .gap(px(8.))
                    .items_center()
                    .child(self.dot(c, cx))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(c.title()))
                    .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(count.to_string()))
                    .when(c == Column::Done, |d| {
                        d.child(div().flex_1()).child(
                            Button::new("done-fold").ghost().small().icon(Icon::new(IconName::PanelRightClose)).tooltip("Collapse").on_click(cx.listener(|this, _, _, cx| {
                                this.done_open = false;
                                cx.notify();
                            })),
                        )
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
            .pt(px(11.))
            .pb(px(12.))
            .gap(px(6.))
            .bg(theme.sidebar)
            .border_1()
            .border_color(theme.border)
            .rounded(px(10.))
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
            .rounded(px(8.))
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
                    .child(agent_badge(r.agent, &theme)),
            )
            .child(div().text_size(if done { px(12.5) } else { px(13.5) }).font_weight(FontWeight::MEDIUM).line_height(gpui::relative(1.3)).line_clamp(2).child(r.title.clone()));

        if !done {
            let (chip_bg, chip_fg) = match card.chip_kind {
                "bash" | "task" | "web" | "mcp" | "plan" | "search" | "read" => (theme.primary.opacity(0.12), theme.primary),
                "edit" | "write" | "approve" | "terminal" | "ask" => (theme.warning.opacity(0.15), theme.warning),
                "reply" => (theme.success.opacity(0.15), theme.success),
                "stop" => (theme.danger.opacity(0.15), theme.danger),
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
                    .child(div().flex_shrink_0().px(px(5.)).py(px(1.)).rounded(px(4.)).bg(chip_bg).text_color(chip_fg).text_size(px(10.5)).font_weight(FontWeight::MEDIUM).child(card.chip.clone()))
                    .child(div().flex_1().min_w_0().truncate().child(card.text.clone()))
                    .child(div().flex_shrink_0().text_size(px(11.)).text_color(theme.muted_foreground).child(clock_text)),
            );
        }

        let mut foot = h_flex().gap(px(6.)).items_center().pt(px(2.)).text_size(px(10.5)).text_color(theme.muted_foreground);
        foot = foot.child(div().child(if done { format!("{} {}", day(&r.updated), clock(&r.updated)) } else { format!("since {}", clock(if r.started.is_empty() { &r.updated } else { &r.started })) }));
        if card.queued > 0 {
            foot = foot.child(badge_str(format!("{} queued", card.queued), theme.primary.opacity(0.12), theme.primary));
        }
        if r.archived {
            foot = foot.child(badge("kept", theme.muted, theme.muted_foreground));
        }
        foot = foot.child(div().flex_1());
        let (via, _) = self.reply_via_for(&r);
        let small = |id: String, label: &'static str, primary: bool, cx: &mut Context<Self>, on: Box<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>| {
            let b = Button::new(SharedString::from(id)).small().compact().label(label).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
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

    // -- new session ---------------------------------------------------------

    fn render_new(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let cwds = self.recent_cwds();
        let chosen = self.new_cwd.clone();
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme.background)
            .child(h_flex().h(TITLEBAR_H).px(px(24.)).items_end().pb(px(10.)).child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("New session")))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(24.))
                    .gap(px(10.))
                    .child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child("Where should Claude work? Recent folders:"))
                    .child(v_flex().id("cwds").gap(px(4.)).overflow_y_scroll().children(cwds.into_iter().map(|c| {
                        let active = c == chosen;
                        let theme = cx.theme().clone();
                        let label = scribe_core::paths::tilde(&c);
                        h_flex()
                            .id(SharedString::from(format!("cwd-{c}")))
                            .px(px(10.))
                            .py(px(6.))
                            .gap(px(8.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .when(active, |d| d.bg(theme.list_active))
                            .hover(|s| s.bg(theme.list_hover))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.new_cwd = c.clone();
                                cx.notify();
                            }))
                            .child(Icon::new(IconName::Folder).with_size(px(14.)).text_color(theme.muted_foreground))
                            .child(div().text_size(px(13.)).child(label))
                    })))
                    .when(!chosen.is_empty(), |d| d.child(div().pt(px(6.)).text_size(px(12.)).text_color(theme.muted_foreground).child(format!("The first message starts Claude Code in {}.", scribe_core::paths::tilde(&chosen))))),
            )
            .child(self.render_composer(cx))
    }

    // -- search palette -------------------------------------------------------

    fn render_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let results = self.search_results.clone();
        div()
            .id("search-overlay")
            .absolute()
            .inset_0()
            .bg(theme.background.opacity(0.55))
            .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))
            .child(
                v_flex()
                    .id("search-panel")
                    .key_context(SEARCH_CONTEXT)
                    .on_action(cx.listener(|this, _: &Escape, window, cx| this.close_search(window, cx)))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .absolute()
                    .top(px(80.))
                    .left_0()
                    .right_0()
                    .mx_auto()
                    .w(px(680.))
                    .max_h(px(520.))
                    .rounded(px(12.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .child(
                        h_flex()
                            .px(px(14.))
                            .h(px(46.))
                            .gap(px(10.))
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(Icon::new(IconName::Search).with_size(px(15.)).text_color(theme.muted_foreground))
                            .child(div().flex_1().min_w_0().child(Input::new(&self.search_input).appearance(false).bordered(false))),
                    )
                    .child(v_flex().id("search-results").flex_1().min_h_0().overflow_y_scroll().p(px(8.)).map(|d| match results {
                        None => d.child(div().p(px(12.)).text_size(px(12.5)).text_color(theme.muted_foreground).child("Type to search prompts, replies, thoughts and tool calls across every session.")),
                        Some(res) if !res.error.is_empty() => d.child(div().p(px(12.)).text_size(px(12.5)).text_color(theme.danger).child(res.error)),
                        Some(res) if res.sessions.is_empty() => d.child(div().p(px(12.)).text_size(px(12.5)).text_color(theme.muted_foreground).child("No matches.")),
                        Some(res) => d.children(res.sessions.into_iter().take(40).map(|s| {
                            let key = format!("{}:{}", s.agent, s.id);
                            let theme = cx.theme().clone();
                            v_flex()
                                .id(SharedString::from(format!("hit-{key}")))
                                .px(px(10.))
                                .py(px(8.))
                                .gap(px(3.))
                                .rounded(px(8.))
                                .cursor_pointer()
                                .hover(|st| st.bg(theme.list_hover))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.close_search(window, cx);
                                    this.open_session(&key, cx);
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
    let Some(t) = scribe_core::build::parse_ts(ts) else { return String::new() };
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
    div().px(px(6.)).py(px(1.)).rounded(px(4.)).bg(bg).text_color(fg).text_size(px(10.5)).font_weight(FontWeight::MEDIUM).child(text)
}

pub fn badge_str(text: String, bg: Hsla, fg: Hsla) -> impl IntoElement {
    div().px(px(6.)).py(px(1.)).rounded(px(4.)).bg(bg).text_color(fg).text_size(px(10.5)).font_weight(FontWeight::MEDIUM).child(text)
}

/// Which agent a card belongs to, said on every card so the board never
/// needs a legend.
fn agent_badge(agent: AgentId, theme: &gpui_component::Theme) -> impl IntoElement {
    let (icon, label) = match agent {
        AgentId::ClaudeCode => (IconName::Bot, "Claude"),
        AgentId::Codex => (IconName::SquareTerminal, "Codex"),
    };
    h_flex().gap(px(3.)).items_center().flex_shrink_0().child(Icon::new(icon).with_size(px(11.)).text_color(theme.muted_foreground)).child(div().text_size(px(10.5)).text_color(theme.muted_foreground).child(label))
}

fn chip_button(id: &'static str, label: String, cx: &mut Context<Workbench>, on: impl Fn(&mut Workbench, &mut Context<Workbench>) + 'static) -> impl IntoElement {
    let theme = cx.theme().clone();
    div()
        .id(id)
        .px(px(8.))
        .py(px(2.))
        .rounded(px(6.))
        .bg(theme.muted)
        .text_color(theme.foreground)
        .cursor_pointer()
        .hover(|s| s.bg(theme.list_hover))
        .on_click(cx.listener(move |this, _, _, cx| on(this, cx)))
        .child(label)
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let search_open = self.search_open;
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
            .on_action(cx.listener(|this, _: &Refresh, _, cx| {
                this.hub.refresh();
                this.status = "refreshing…".into();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NewSession, _, cx| this.show_new(None, cx)))
            .on_action(cx.listener(|this, _: &GoBoard, _, cx| {
                this.page = Page::Board;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.search_open {
                    this.close_search(window, cx);
                }
            }))
            .size_full()
            .relative()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(px(13.))
            .child(
                h_flex().size_full().child(self.render_sidebar(cx)).map(|this| match self.page {
                    Page::Board => this.child(self.render_board(cx)),
                    Page::New => this.child(self.render_new(cx)),
                    Page::Sessions => this.child(self.render_session_list(cx)).child(self.render_detail(window, cx)),
                }),
            )
            .when(search_open, |d| d.child(self.render_search(cx)))
    }
}
