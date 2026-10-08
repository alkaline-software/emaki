//! The background engine. No gpui here: threads, channels and the core.
//!
//! One scanner thread walks every agent, archives what it finds (copy first),
//! and hands the index to the window. A watcher wakes it early when a
//! transcript changes. A search thread keeps the FTS index in step. Drivers
//! (headless Claude Code children) live here too, so the window only ever
//! sees events.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use emaki_core::adapters;
use emaki_core::archive;
use emaki_core::config::Config;
use emaki_core::driver::{self, Driver};
use emaki_core::explain::Explainer;
use emaki_core::model::AgentId;
use emaki_core::options::Options;
use emaki_core::peer::{self, Peer};
use emaki_core::pty::Pty;
use emaki_core::search::{self, SearchIndex};
use emaki_core::transcript::SessionRef;
use emaki_core::update::{self, UpdateState};
use emaki_core::watcher::Watcher;

/// A process behind a transcript is assumed for this long after its last write
/// when nothing else says so.
pub const LIVE_GRACE_S: f64 = 600.0;
/// The same for a Claude Code session whose transcript reads as working:
/// see `Hub::is_live`.
const WORKING_GRACE_S: f64 = 90.0;
/// How long a session still at work goes between two copies into the
/// archive. One whose turn is over is copied at once; this is for a turn
/// that runs on. The archive is for a file the agent deletes or rewrites,
/// which it does not do to one it is writing, so a copy every few seconds
/// bought nothing.
const ARCHIVE_EVERY: Duration = Duration::from_secs(300);
/// How often the copies the agents no longer have are looked over for
/// packing, after the look at launch.
const PACK_EVERY: Duration = Duration::from_secs(6 * 3600);

pub enum HubEvent {
    Index(Vec<SessionRef>),
    Changed(PathBuf),
    DriverStarted { session_id: String },
    DriverFailed { session_id: String, error: String },
    Driver { session_id: String, event: driver::Event },
    /// A message left through `via` ("driver" or "inbox").
    Sent { session_id: String, queued: bool, error: String },
    /// One line for the status row: what a background request came back with.
    Note(String),
    /// An explanation landed for a tool call (empty when the model had
    /// nothing usable to say).
    Explained { call_id: String, text: String },
    /// The update check, the download and the install, as they go.
    Update(UpdateEvent),
    /// A folder's commands are in (`commands_for` answers now), and with
    /// them what the agent offers (`options_for`).
    Commands,
    /// A terminal's status line ran for this session and left what it was
    /// handed (`state/context/<session>.json`): the model, the effort or
    /// the mode there may have changed.
    Context(String),
    /// The screen of this session's hidden terminal changed.
    Screen(String),
    /// A message waits on this session's hidden terminal, which is
    /// showing something that is not its prompt: the person has to see it.
    TerminalNeeded(String),
}

#[derive(Debug, Clone)]
pub enum UpdateEvent {
    Checking,
    UpToDate { latest: String },
    Available { latest: String },
    CheckFailed(String),
    Downloading { done: u64, total: Option<u64> },
    Installing,
    /// The new app is running (or its installer is up); the window quits.
    Relaunch,
    InstallFailed(String),
}

pub struct Hub {
    tx: UnboundedSender<HubEvent>,
    wake: Mutex<mpsc::Sender<()>>,
    search_tx: Mutex<mpsc::Sender<Vec<SessionRef>>>,
    drivers: Mutex<HashMap<String, Arc<Driver>>>,
    /// The terminals of our own, with no window, each running an
    /// interactive Claude Code on one session (`emaki_core::pty`).
    terminals: Mutex<HashMap<String, Arc<Pty>>>,
    /// The session showing in the window: its hidden terminal is kept
    /// for as long as it shows, however idle.
    showing: Mutex<Option<String>>,
    /// Hidden terminals whose child went away by itself, with the last
    /// thing on its screen: said once to whoever is looking.
    lost: Mutex<HashMap<String, String>>,
    /// Sessions whose driver exited; a fresh mtime alone must not count as a
    /// process for them.
    ended: Mutex<HashMap<String, Instant>>,
    /// Claude Code's own session registry: every terminal session with an
    /// inbox, refreshed on each scan.
    peers: Mutex<HashMap<String, Peer>>,
    /// What last went to each session's inbox, as it was sent, and when:
    /// the same words again inside Claude Code's window go as a repeat
    /// (`peer::send`).
    inbox_last: Mutex<HashMap<String, (String, std::time::Instant)>>,
    pub cfg: RwLock<Config>,
    /// The slash commands known in each folder asked about, for the list
    /// over the composer on a session no driver of ours is behind; an
    /// empty entry is a folder being asked about now.
    commands: Mutex<HashMap<String, Vec<driver::CommandInfo>>>,
    /// What each agent said a session can be set to (its modes, models
    /// and effort levels), by folder as each is asked; under the empty
    /// folder, the last answer from anywhere, which at launch is the one
    /// kept from the last run.
    options: Mutex<HashMap<(AgentId, String), Options>>,
    /// Plain-English lines for opaque tool calls, asked of a small model
    /// on a thread; a permission card asks on arrival, a tool card on click.
    pub explainer: Arc<Explainer>,
}

impl Hub {
    pub fn start(cfg: Config) -> (Arc<Hub>, UnboundedReceiver<HubEvent>) {
        let (tx, rx) = unbounded();
        let (wake_tx, wake_rx) = mpsc::channel::<()>();
        let (search_tx, search_rx) = mpsc::channel::<Vec<SessionRef>>();
        let etx = tx.clone();
        let explainer = Explainer::new(
            cfg.explain.clone(),
            Arc::new(move |call_id: &str, text: &str| {
                let _ = etx.unbounded_send(HubEvent::Explained { call_id: call_id.to_string(), text: text.to_string() });
            }),
        );
        let hub = Arc::new(Hub {
            tx,
            wake: Mutex::new(wake_tx),
            search_tx: Mutex::new(search_tx),
            drivers: Mutex::new(HashMap::new()),
            terminals: Mutex::new(HashMap::new()),
            showing: Mutex::new(None),
            lost: Mutex::new(HashMap::new()),
            ended: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            inbox_last: Mutex::new(HashMap::new()),
            cfg: RwLock::new(cfg),
            commands: Mutex::new(HashMap::new()),
            options: Mutex::new(AgentId::ALL.iter().map(|a| ((*a, String::new()), Options::cached(*a))).filter(|(_, o)| !o.is_empty()).collect()),
            explainer,
        });
        let _ = emaki_core::paths::ensure_dirs();
        // Claude Code's status line is ours from the first launch on; the
        // limits row under the composer reads what it leaves.
        if let Err(e) = emaki_core::statusline::ensure() {
            eprintln!("emaki: could not install the status line: {e}");
        }
        hub.spawn_scanner(wake_rx);
        hub.spawn_search(search_rx);
        hub.spawn_watcher();
        (hub, rx)
    }

    fn send(&self, ev: HubEvent) {
        let _ = self.tx.unbounded_send(ev);
    }

    pub fn refresh(&self) {
        if let Ok(w) = self.wake.lock() {
            let _ = w.send(());
        }
    }

    fn spawn_scanner(self: &Arc<Self>, wake_rx: mpsc::Receiver<()>) {
        let hub = Arc::clone(self);
        thread::Builder::new()
            .name("emaki-scan".into())
            .spawn(move || {
                let mut last_sig: Vec<(String, u64, f64)> = Vec::new();
                // What of each session the archive has, by its size and
                // time, and when it was taken.
                let mut taken: HashMap<String, (u64, f64, Instant)> = HashMap::new();
                let mut packed_at: Option<Instant> = None;
                loop {
                    let (interval, agents) = {
                        let cfg = hub.cfg.read().unwrap();
                        (cfg.scan_interval_ms, cfg.agents.clone())
                    };
                    // The registry first, the transcripts after: an idle
                    // read before the rows it follows would settle a turn
                    // that ended by itself as a stopped one.
                    hub.refresh_peers();
                    let mut refs = adapters::index_all(200, &agents);
                    hub.settle_stopped(&mut refs);
                    let sig: Vec<(String, u64, f64)> =
                        refs.iter().map(|r| (format!("{}:{}", r.agent.as_str(), r.session_id), r.size, r.mtime)).collect();
                    let changed = sig != last_sig;
                    // The window redraws the board from this; send it every
                    // pass so the elapsed clocks tick, cheap because peek is
                    // cached on (size, mtime).
                    hub.send(HubEvent::Index(refs.clone()));
                    // Copy first, render second: at launch every session,
                    // after that a session when it has changed and its
                    // turn is over, and one still at work now and then.
                    // The counts are bookkeeping and go nowhere the
                    // person looks.
                    let due: Vec<SessionRef> = refs
                        .iter()
                        .filter(|r| !r.archived)
                        .filter(|r| match taken.get(&format!("{}:{}", r.agent.as_str(), r.session_id)) {
                            None => true,
                            Some((size, mtime, _)) if *size == r.size && *mtime == r.mtime => false,
                            Some((_, _, at)) => r.state.phase != emaki_core::build::Phase::Working || at.elapsed() >= ARCHIVE_EVERY,
                        })
                        .cloned()
                        .collect();
                    if !due.is_empty() {
                        let _ = archive::sweep(&due);
                        for r in &due {
                            taken.insert(format!("{}:{}", r.agent.as_str(), r.session_id), (r.size, r.mtime, Instant::now()));
                        }
                    }
                    // The copies the agents no longer have are packed, in
                    // their own time: once after launch, then a few times
                    // a day.
                    if packed_at.is_none_or(|at| at.elapsed() >= PACK_EVERY) {
                        packed_at = Some(Instant::now());
                        let stale: Vec<SessionRef> = refs.iter().filter(|r| r.archived).cloned().collect();
                        let _ = thread::Builder::new().name("emaki-pack".into()).spawn(move || {
                            let _ = archive::pack_stale(&stale);
                        });
                    }
                    if changed {
                        if let Ok(s) = hub.search_tx.lock() {
                            let _ = s.send(refs.clone());
                        }
                        last_sig = sig;
                    }
                    hub.reap_drivers();
                    hub.reap_terminals();
                    match wake_rx.recv_timeout(Duration::from_millis(interval.max(500))) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    // Coalesce a burst of wakeups.
                    while wake_rx.try_recv().is_ok() {}
                }
            })
            .ok();
    }

    fn spawn_search(&self, rx: mpsc::Receiver<Vec<SessionRef>>) {
        thread::Builder::new()
            .name("emaki-search".into())
            .spawn(move || {
                let Ok(mut index) = SearchIndex::open() else { return };
                while let Ok(mut refs) = rx.recv() {
                    // Only the newest request matters.
                    while let Ok(newer) = rx.try_recv() {
                        refs = newer;
                    }
                    let _ = index.sync(&refs, false);
                }
            })
            .ok();
    }

    fn spawn_watcher(self: &Arc<Self>) {
        let hub = Arc::clone(self);
        thread::Builder::new()
            .name("emaki-watch".into())
            .spawn(move || {
                let mut roots: Vec<PathBuf> = Vec::new();
                for a in adapters::all() {
                    roots.extend(a.data_roots());
                }
                // What the status line leaves per session is watched too:
                // a model, an effort or a mode changed in a terminal reruns
                // it, and that file is the only word of it until the next
                // turn. It says nothing about any transcript, so no rescan.
                let context = emaki_core::paths::state_dir().join("context");
                roots.push(context.clone());
                let Ok(watcher) = Watcher::start(&roots, Duration::from_millis(250)) else { return };
                while let Ok(path) = watcher.events.recv() {
                    if path.starts_with(&context) {
                        if let Some(sid) = path.file_stem().filter(|_| path.extension().is_some_and(|e| e == "json")) {
                            hub.send(HubEvent::Context(sid.to_string_lossy().to_string()));
                        }
                        continue;
                    }
                    hub.send(HubEvent::Changed(path));
                    hub.refresh();
                }
            })
            .ok();
    }

    // -- presence ---------------------------------------------------------

    pub fn driver_for(&self, session_id: &str) -> Option<Arc<Driver>> {
        self.drivers.lock().unwrap().get(session_id).cloned()
    }

    pub fn has_ended(&self, session_id: &str) -> bool {
        self.ended.lock().unwrap().contains_key(session_id)
    }

    /// A session the registry calls idle is not working, whatever its
    /// transcript's tail reads as: see `TurnState::settle_idle`.
    fn settle_stopped(&self, refs: &mut [SessionRef]) {
        let peers = self.peers.lock().unwrap();
        for r in refs.iter_mut() {
            if let Some(p) = peers.get(&r.session_id).filter(|p| p.status == "idle" && p.status_at > 0.0) {
                r.state.settle_idle(p.status_at);
            }
        }
    }

    pub fn refresh_peers(&self) {
        // A session may be in a hidden terminal of ours and in a terminal
        // of the person's at once, which is theirs to do: ours is the
        // record the window keeps.
        let ours: Vec<i32> = self.terminals.lock().unwrap().values().map(|p| p.pid).collect();
        let mut fresh: HashMap<String, Peer> = HashMap::new();
        for p in peer::registry_all() {
            if ours.contains(&p.pid) || !fresh.contains_key(&p.session_id) {
                fresh.insert(p.session_id.clone(), p);
            }
        }
        // A process that was registered and no longer is has ended,
        // whatever its transcript's tail reads as: killed part way
        // through a turn, it left a prompt with no reply, which read as
        // working for as long as a recent write is taken for a process.
        let mut peers = self.peers.lock().unwrap();
        let mut ended = self.ended.lock().unwrap();
        for sid in peers.keys().filter(|sid| !fresh.contains_key(*sid)) {
            ended.insert(sid.clone(), Instant::now());
        }
        for sid in fresh.keys() {
            ended.remove(sid);
        }
        *peers = fresh;
    }

    /// The inbox of a terminal session, if Claude Code has one registered.
    /// The commands known in `cwd`, once read; asking starts the read on a
    /// thread the first time, and `HubEvent::Commands` says when it is in.
    pub fn commands_for(self: &Arc<Self>, cwd: &str) -> Option<Vec<driver::CommandInfo>> {
        let mut g = self.commands.lock().unwrap();
        if let Some(list) = g.get(cwd) {
            return (!list.is_empty()).then(|| list.clone());
        }
        g.insert(cwd.to_string(), Vec::new());
        drop(g);
        let hub = Arc::clone(self);
        let cwd = cwd.to_string();
        thread::spawn(move || {
            let catalogue = adapters::for_agent(AgentId::ClaudeCode).catalogue(&cwd);
            hub.learn_options(AgentId::ClaudeCode, &cwd, catalogue.options);
            if catalogue.commands.is_empty() {
                // Ask again next time rather than remember a failure.
                hub.commands.lock().unwrap().remove(&cwd);
                return;
            }
            hub.commands.lock().unwrap().insert(cwd, catalogue.commands);
            hub.send(HubEvent::Commands);
        });
        None
    }

    /// The modes, models and effort levels `agent` offers a session in
    /// `cwd`: what it said for that folder when it has been asked
    /// (`commands_for` asks, and so does starting a driver there), else
    /// what it last said anywhere. Never starts a read, so the composer
    /// can ask on every draw. Empty for an agent that was never heard.
    pub fn options_for(&self, agent: AgentId, cwd: &str) -> Options {
        let g = self.options.lock().unwrap();
        g.get(&(agent, cwd.to_string())).or_else(|| g.get(&(agent, String::new()))).cloned().unwrap_or_default()
    }

    /// Keep what `agent` just said it offers, for this folder, as the
    /// last word from anywhere, and on disk for the next launch. An empty
    /// answer is an agent that did not answer.
    fn learn_options(&self, agent: AgentId, cwd: &str, options: Options) {
        if options.is_empty() {
            return;
        }
        {
            let mut g = self.options.lock().unwrap();
            g.insert((agent, cwd.to_string()), options.clone());
            g.insert((agent, String::new()), options.clone());
        }
        options.remember(agent);
        self.send(HubEvent::Commands);
    }

    /// Whether `/name` is a command: one that acts by itself, or one the
    /// folder's catalogue lists, when it has been read. Never starts a
    /// read, so the conversation can ask on every draw.
    pub fn knows_command(&self, cwd: &str, name: &str) -> bool {
        driver::acts_alone(name) || self.commands.lock().unwrap().get(cwd).is_some_and(|list| list.iter().any(|c| c.name == name))
    }

    /// Have the window read a terminal session again, as when its status
    /// line runs: after a key sent there from here.
    pub fn look_at(&self, session_id: &str) {
        self.send(HubEvent::Context(session_id.to_string()));
    }

    /// One line for the row under the composer, from a thread.
    pub fn say(&self, text: String) {
        self.send(HubEvent::Note(text));
    }

    /// The session's record in Claude Code's registry. With a hidden
    /// terminal of ours behind the session it is that terminal's record
    /// or none yet, never the record of a terminal of the person's on
    /// the same session: nothing the window does goes there.
    pub fn peer_for(&self, session_id: &str) -> Option<Peer> {
        let peer = self.peers.lock().unwrap().get(session_id).cloned();
        match self.terminal_for(session_id) {
            Some(pty) => peer.filter(|p| p.pid == pty.pid),
            None => peer,
        }
    }

    /// Whether something is behind this session: our driver, a registered
    /// inbox, or a transcript written recently enough that a process is the
    /// likely explanation.
    pub fn is_live(&self, r: &SessionRef, now: f64) -> bool {
        if self.driver_for(&r.session_id).is_some() || self.terminal_for(&r.session_id).is_some() || self.peer_for(&r.session_id).is_some() {
            return true;
        }
        if self.has_ended(&r.session_id) {
            return false;
        }
        // An interactive Claude Code always registers, so one that reads
        // as working with no process registered is a headless run of
        // someone's, which writes as it goes, or a process that died part
        // way through a turn. A short quiet tells the two apart; the long
        // grace kept a killed session "working" with its clock running.
        let working = r.agent == AgentId::ClaudeCode && matches!(r.state.phase, emaki_core::build::Phase::Working);
        now - r.mtime < if working { WORKING_GRACE_S } else { LIVE_GRACE_S }
    }

    // -- hidden terminals ---------------------------------------------------

    /// The hidden terminal behind a session, while its child lives.
    pub fn terminal_for(&self, session_id: &str) -> Option<Arc<Pty>> {
        self.terminals.lock().unwrap().get(session_id).filter(|p| p.alive()).cloned()
    }

    /// Which session the window shows, or none.
    pub fn show(&self, session_id: Option<String>) {
        *self.showing.lock().unwrap() = session_id;
    }

    /// Start an interactive Claude Code on the session, on a terminal
    /// with no window: resumed, or begun under that id in `mode` with
    /// `model`. Nothing happens when one is already there. The caller
    /// has made sure no other process is behind the session. The screen
    /// changing arrives as `HubEvent::Screen`, a moment after it does,
    /// so a burst of drawing is one event.
    pub fn start_terminal(self: &Arc<Self>, session_id: &str, cwd: &str, resume: bool, mode: &str, model: &str) -> Result<(), String> {
        if self.terminal_for(session_id).is_some() {
            return Ok(());
        }
        let (tx, rx) = mpsc::channel::<()>();
        let tx = Mutex::new(tx);
        let argv = emaki_core::pty::claude_argv(session_id, resume, mode, model);
        let pty = Pty::spawn(&argv, cwd, Arc::new(move || {
            let _ = tx.lock().map(|t| t.send(()));
        }))?;
        self.terminals.lock().unwrap().insert(session_id.to_string(), pty);
        self.ended.lock().unwrap().remove(session_id);
        let hub = Arc::clone(self);
        let sid = session_id.to_string();
        thread::spawn(move || {
            while rx.recv().is_ok() {
                thread::sleep(Duration::from_millis(40));
                while rx.try_recv().is_ok() {}
                hub.send(HubEvent::Screen(sid.clone()));
            }
            // The child is gone. Still held here, nobody let it go: it
            // went by itself, and the foot of its screen says why.
            if let Some(pty) = hub.terminals.lock().unwrap().remove(&sid) {
                let said = pty.text().lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
                hub.lost.lock().unwrap().insert(sid.clone(), said);
            }
            hub.ended.lock().unwrap().insert(sid.clone(), Instant::now());
            hub.refresh_peers();
            hub.send(HubEvent::Screen(sid));
            hub.refresh();
        });
        self.refresh();
        Ok(())
    }

    /// Put a message in the hidden terminal's prompt and send it, as the
    /// person typing there would: each picture's path pasted first, which
    /// Claude Code takes as the picture, then the words, then Return.
    /// Nothing is typed before Claude Code has registered and its prompt
    /// is on the screen, and each step waits on the screen showing the
    /// one before it. A terminal that shows something else for a while
    /// (a login, a folder to trust) is put in front of the person
    /// (`HubEvent::TerminalNeeded`) and the message goes once they are
    /// through it. A turn that is running takes the message into its
    /// queue, as it does in any terminal.
    pub fn send_to_terminal(self: &Arc<Self>, session_id: &str, text: String, images: Vec<PathBuf>) {
        let hub = Arc::clone(self);
        let sid = session_id.to_string();
        thread::spawn(move || {
            let sent = hub.type_message(&sid, &text, &images);
            hub.send(HubEvent::Sent { session_id: sid, queued: false, error: sent.err().unwrap_or_default() });
            hub.refresh();
        });
    }

    fn type_message(&self, sid: &str, text: &str, images: &[PathBuf]) -> Result<(), String> {
        let Some(pty) = self.terminal_for(sid) else { return Err("claude is not running; try again".into()) };
        let look = Duration::from_millis(60);
        let started = Instant::now();
        let mut asked = false;
        loop {
            if !pty.alive() {
                return Err("claude exited before it took the message".into());
            }
            let registered = !peer::available() || peer::registry_all().iter().any(|p| p.pid == pty.pid);
            if registered && driver::prompt_on_screen(&pty.styled()).is_some() {
                break;
            }
            if !asked && started.elapsed() > Duration::from_secs(5) {
                asked = true;
                self.send(HubEvent::TerminalNeeded(sid.to_string()));
            }
            if started.elapsed() > Duration::from_secs(600) {
                return Err("claude never came to its prompt".into());
            }
            thread::sleep(look);
        }
        // The screen follows a paste within a frame or two; looked at
        // until it has, and gone on from when it never does.
        let until = |shown: &dyn Fn(&str) -> bool| {
            for _ in 0..80 {
                if shown(&pty.styled()) || !pty.alive() {
                    break;
                }
                thread::sleep(look);
            }
        };
        // Each picture is on the screen before the next is pasted: one
        // more "[Image #" than there was. Counted from what the screen
        // already shows, since the conversation above the prompt names
        // the pictures of earlier messages; counted from nothing, the
        // wait was over before it began whenever one was in view, the
        // second path went in on top of the first, and one picture of
        // the two never arrived.
        // The prompt may hold words already: Claude Code puts a prompt
        // stopped at once back into its input, as the window puts it
        // back into the composer, and the message typed after it went
        // out with the old words in front, twice over after two stops.
        // What is there is taken out a line at a time (to the line's end,
        // back to its start, and the break before it) until the prompt
        // is empty, each step once the screen shows the one before.
        for _ in 0..400 {
            if driver::prompt_on_screen(&pty.styled()) != Some(false) || !pty.alive() {
                break;
            }
            let before = pty.styled();
            pty.write(b"\x05\x15\x7f");
            for _ in 0..20 {
                if pty.styled() != before {
                    break;
                }
                thread::sleep(look);
            }
        }
        let marks = |screen: &str| screen.matches("[Image #").count();
        for path in images.iter() {
            let before = marks(&pty.styled());
            pty.paste(&path.to_string_lossy());
            until(&|screen| marks(screen) > before);
        }
        let words = text.trim();
        if !words.is_empty() {
            pty.paste(words);
        }
        until(&|screen| driver::prompt_on_screen(screen) == Some(false));
        pty.write(b"\r");
        Ok(())
    }

    /// Why the session's hidden terminal went away by itself, once.
    pub fn take_lost(&self, session_id: &str) -> Option<String> {
        self.lost.lock().unwrap().remove(session_id)
    }

    /// Whether the hidden terminal is how sessions run here.
    pub fn hidden_terminals(&self) -> bool {
        self.cfg.read().unwrap().driver.hidden_terminal
    }

    /// Let a session's hidden terminal go.
    pub fn stop_terminal(&self, session_id: &str) {
        if let Some(pty) = self.terminals.lock().unwrap().remove(session_id) {
            let pid = pty.pid;
            pty.kill();
            self.peers.lock().unwrap().retain(|_, p| p.pid != pid);
        }
    }

    /// Hidden terminals nobody needs: one whose child is gone, and one
    /// that is not showing and has been idle past the driver's limit.
    fn reap_terminals(&self) {
        let limit = Duration::from_secs(self.cfg.read().unwrap().driver.idle_min.max(1) * 60);
        let showing = self.showing.lock().unwrap().clone();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let stale: Vec<String> = {
            let peers = self.peers.lock().unwrap();
            self.terminals
                .lock()
                .unwrap()
                .iter()
                .filter(|(sid, pty)| {
                    let idle = peers.get(*sid).is_some_and(|p| p.pid == pty.pid && p.status == "idle" && now - p.status_at > limit.as_secs_f64());
                    !pty.alive() || (idle && showing.as_ref() != Some(*sid) && pty.quiet_for() > limit)
                })
                .map(|(sid, _)| sid.clone())
                .collect()
        };
        for sid in stale {
            self.stop_terminal(&sid);
        }
    }

    // -- drivers ----------------------------------------------------------

    /// Start a driver on a thread (initialisation blocks for up to 15s), then
    /// deliver `first` through it. Events arrive as `HubEvent::Driver`.
    pub fn spawn_driver_and_send(
        self: &Arc<Self>,
        session_id: String,
        cwd: String,
        resume: bool,
        mode: String,
        model: String,
        first: Option<(String, Vec<serde_json::Value>)>,
    ) {
        let hub = Arc::clone(self);
        thread::spawn(move || {
            let (ev_tx, ev_rx) = mpsc::channel::<driver::Event>();
            let sid = session_id.clone();
            let hub2 = Arc::clone(&hub);
            thread::spawn(move || {
                while let Ok(ev) = ev_rx.recv() {
                    let exit = matches!(ev, driver::Event::Exit { .. });
                    // The explanation is asked for the moment the card
                    // exists: reading it while you decide is the point.
                    if let driver::Event::Permission(p) = &ev {
                        let id = if p.tool_use_id.is_empty() { p.request_id.clone() } else { p.tool_use_id.clone() };
                        hub2.explainer.request(&id, &p.tool_name, &p.input, false);
                    }
                    hub2.send(HubEvent::Driver { session_id: sid.clone(), event: ev });
                    if exit {
                        hub2.drivers.lock().unwrap().remove(&sid);
                        hub2.ended.lock().unwrap().insert(sid.clone(), Instant::now());
                        hub2.refresh();
                        break;
                    }
                }
            });
            match Driver::start(&session_id, &cwd, resume, &mode, &model, ev_tx) {
                Ok(d) => {
                    hub.learn_options(AgentId::ClaudeCode, &cwd, d.caps().options);
                    hub.drivers.lock().unwrap().insert(session_id.clone(), Arc::clone(&d));
                    hub.ended.lock().unwrap().remove(&session_id);
                    hub.send(HubEvent::DriverStarted { session_id: session_id.clone() });
                    if let Some((text, images)) = first {
                        let r = d.send(&text, images);
                        hub.send(HubEvent::Sent {
                            session_id: session_id.clone(),
                            queued: r.as_ref().map(|q| *q).unwrap_or(false),
                            error: r.err().map(|e| e.0).unwrap_or_default(),
                        });
                    }
                    hub.refresh();
                }
                Err(e) => hub.send(HubEvent::DriverFailed { session_id, error: e.0 }),
            }
        });
    }

    pub fn send_to_driver(self: &Arc<Self>, session_id: &str, text: String, images: Vec<serde_json::Value>) -> bool {
        let Some(d) = self.driver_for(session_id) else { return false };
        let hub = Arc::clone(self);
        let sid = session_id.to_string();
        thread::spawn(move || {
            let r = d.send(&text, images);
            hub.send(HubEvent::Sent {
                session_id: sid,
                queued: r.as_ref().map(|q| *q).unwrap_or(false),
                error: r.err().map(|e| e.0).unwrap_or_default(),
            });
        });
        true
    }

    /// Deliver into a terminal session's inbox on a thread. The registry is
    /// re-read first so a session that just ended is refused, not forked.
    pub fn send_to_inbox(self: &Arc<Self>, session_id: &str, text: String) {
        let hub = Arc::clone(self);
        let sid = session_id.to_string();
        thread::spawn(move || {
            hub.refresh_peers();
            let error = match hub.peer_for(&sid) {
                None => "this session has no inbox any more".to_string(),
                Some(p) => {
                    let words = text.trim().to_string();
                    let again = hub.inbox_last.lock().unwrap().get(&sid).is_some_and(|(last, at)| *last == words && at.elapsed() < peer::REPEAT_WINDOW);
                    let sent = if again { format!("{words} ") } else { words };
                    hub.inbox_last.lock().unwrap().insert(sid.clone(), (sent, std::time::Instant::now()));
                    peer::send(&p, &text, again).err().unwrap_or_default()
                }
            };
            hub.send(HubEvent::Sent { session_id: sid, queued: false, error });
            hub.refresh();
        });
    }

    /// Ask a driver to change its permission mode, on a thread. The answer
    /// comes back as a `Driver` event either way: the mode Claude Code now
    /// holds, so a refused switch (auto mode not enabled here, bypass without
    /// the flag) puts the pill back and says why on the status row.
    pub fn set_driver_mode(self: &Arc<Self>, session_id: &str, mode: String) -> bool {
        let Some(d) = self.driver_for(session_id) else { return false };
        let hub = Arc::clone(self);
        let sid = session_id.to_string();
        thread::spawn(move || {
            let now = match d.set_mode(&mode) {
                Ok(now) => now,
                Err(e) => {
                    hub.send(HubEvent::Note(format!("could not switch to {}: {}", hub.options_for(AgentId::ClaudeCode, &d.cwd).mode(&mode).map(|m| m.label.clone()).unwrap_or_else(|| mode.clone()), e.0)));
                    d.mode()
                }
            };
            hub.send(HubEvent::Driver { session_id: sid, event: driver::Event::Mode(now) });
        });
        true
    }

    /// The same for the model. The child's own `system/init` on the next
    /// turn confirms what it is actually running.
    pub fn set_driver_model(self: &Arc<Self>, session_id: &str, model: String) -> bool {
        let Some(d) = self.driver_for(session_id) else { return false };
        let hub = Arc::clone(self);
        let sid = session_id.to_string();
        thread::spawn(move || {
            if let Err(e) = d.set_model(&model) {
                hub.send(HubEvent::Note(format!("could not switch to {}: {}", hub.options_for(AgentId::ClaudeCode, &d.cwd).model(&model).map(|m| m.label.clone()).unwrap_or_else(|| driver::model_label(&model)), e.0)));
            }
            hub.send(HubEvent::Driver { session_id: sid, event: driver::Event::Init(d.caps()) });
        });
        true
    }

    /// Set a driver's effort level: a `/effort` turn, queued behind a
    /// running one like any message. The transcript records the result and
    /// the pill reads it from there.
    pub fn set_driver_effort(self: &Arc<Self>, session_id: &str, effort: String) -> bool {
        let Some(d) = self.driver_for(session_id) else { return false };
        let hub = Arc::clone(self);
        let sid = session_id.to_string();
        thread::spawn(move || {
            let r = d.set_effort(&effort);
            hub.send(HubEvent::Sent {
                session_id: sid,
                queued: r.as_ref().map(|q| *q).unwrap_or(false),
                error: r.err().map(|e| e.0).unwrap_or_default(),
            });
        });
        true
    }

    /// Ask GitHub for the newest release, on a thread. What it says comes
    /// back as `HubEvent::Update`, and the time and answer are kept in
    /// `state/update.json` so the daily check knows when it last ran.
    pub fn check_updates(self: &Arc<Self>) {
        let hub = Arc::clone(self);
        thread::spawn(move || {
            hub.send(HubEvent::Update(UpdateEvent::Checking));
            match update::latest() {
                Ok(latest) => {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                    let _ = UpdateState { last_check: now, latest: latest.clone() }.save();
                    if update::is_newer(&latest, update::current_version()) {
                        hub.send(HubEvent::Update(UpdateEvent::Available { latest }));
                    } else {
                        hub.send(HubEvent::Update(UpdateEvent::UpToDate { latest }));
                    }
                }
                Err(e) => hub.send(HubEvent::Update(UpdateEvent::CheckFailed(e))),
            }
        });
    }

    /// Fetch `version`'s installer for this machine and put it in place, on
    /// a thread, reporting as it goes. Ends in `Relaunch` or `InstallFailed`.
    pub fn install_update(self: &Arc<Self>, version: String) {
        let hub = Arc::clone(self);
        thread::spawn(move || {
            let progress = |done: u64, total: Option<u64>| hub.send(HubEvent::Update(UpdateEvent::Downloading { done, total }));
            let file = match update::download(&version, &progress) {
                Ok(f) => f,
                Err(e) => {
                    hub.send(HubEvent::Update(UpdateEvent::InstallFailed(e)));
                    return;
                }
            };
            hub.send(HubEvent::Update(UpdateEvent::Installing));
            match update::install(&file) {
                Ok(()) => hub.send(HubEvent::Update(UpdateEvent::Relaunch)),
                Err(e) => hub.send(HubEvent::Update(UpdateEvent::InstallFailed(e))),
            }
        });
    }

    /// A settings change for the explainer, applied to the running one.
    pub fn set_explain(&self, cfg: emaki_core::config::Explain) {
        if let Ok(mut c) = self.cfg.write() {
            c.explain = cfg.clone();
        }
        self.explainer.set_cfg(cfg);
    }

    pub fn stop_driver(&self, session_id: &str) {
        let d = self.drivers.lock().unwrap().remove(session_id);
        if let Some(d) = d {
            thread::spawn(move || d.stop());
        }
    }

    /// Every session as it is now goes into the archive: at quit, so what
    /// a turn still at work had written since its last copy is not left
    /// for the next launch.
    pub fn archive_all(&self) {
        let agents = self.cfg.read().unwrap().agents.clone();
        let _ = archive::sweep(&adapters::index_all(200, &agents));
    }

    pub fn stop_all(&self) {
        let all: Vec<Arc<Driver>> = self.drivers.lock().unwrap().drain().map(|(_, d)| d).collect();
        for d in all {
            d.stop();
        }
        for (_, pty) in self.terminals.lock().unwrap().drain() {
            pty.kill();
        }
    }

    fn reap_drivers(&self) {
        let idle_min = self.cfg.read().unwrap().driver.idle_min;
        let limit = Duration::from_secs(idle_min.max(1) * 60);
        let stale: Vec<String> = self
            .drivers
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, d)| d.idle_for() > limit || !d.alive())
            .map(|(k, _)| k.clone())
            .collect();
        for k in stale {
            self.stop_driver(&k);
        }
        let cutoff = Instant::now() - Duration::from_secs(1800);
        self.ended.lock().unwrap().retain(|_, t| *t > cutoff);
    }

    // -- search -----------------------------------------------------------

    pub fn search(&self, query: &str) -> search::Results {
        match SearchIndex::open() {
            Ok(idx) => idx.search(query, 300, 4),
            Err(e) => search::Results { query: query.into(), error: e.to_string(), ..Default::default() },
        }
    }
}
