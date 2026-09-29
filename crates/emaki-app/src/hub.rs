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
use emaki_core::peer::{self, Peer};
use emaki_core::search::{self, SearchIndex};
use emaki_core::transcript::SessionRef;
use emaki_core::update::{self, UpdateState};
use emaki_core::watcher::Watcher;

/// A process behind a transcript is assumed for this long after its last write
/// when nothing else says so.
pub const LIVE_GRACE_S: f64 = 600.0;

pub enum HubEvent {
    Index(Vec<SessionRef>),
    Changed(PathBuf),
    Archived(archive::Stats),
    SearchSynced(search::SyncReport),
    DriverStarted { session_id: String },
    DriverFailed { session_id: String, error: String },
    Driver { session_id: String, event: driver::Event },
    /// A message left through `via` ("driver" or "inbox").
    Sent { session_id: String, via: &'static str, queued: bool, error: String },
    /// One line for the status row: what a background request came back with.
    Note(String),
    /// An explanation landed for a tool call (empty when the model had
    /// nothing usable to say).
    Explained { call_id: String, text: String },
    /// The update check, the download and the install, as they go.
    Update(UpdateEvent),
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
    /// Sessions whose driver exited; a fresh mtime alone must not count as a
    /// process for them.
    ended: Mutex<HashMap<String, Instant>>,
    /// Claude Code's own session registry: every terminal session with an
    /// inbox, refreshed on each scan.
    peers: Mutex<HashMap<String, Peer>>,
    pub cfg: RwLock<Config>,
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
            ended: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            cfg: RwLock::new(cfg),
            explainer,
        });
        let _ = emaki_core::paths::ensure_dirs();
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
                loop {
                    let (interval, agents) = {
                        let cfg = hub.cfg.read().unwrap();
                        (cfg.scan_interval_ms, cfg.agents.clone())
                    };
                    let refs = adapters::index_all(200, &agents);
                    hub.refresh_peers();
                    let sig: Vec<(String, u64, f64)> =
                        refs.iter().map(|r| (format!("{}:{}", r.agent.as_str(), r.session_id), r.size, r.mtime)).collect();
                    let changed = sig != last_sig;
                    // The window redraws the board from this; send it every
                    // pass so the elapsed clocks tick, cheap because peek is
                    // cached on (size, mtime).
                    hub.send(HubEvent::Index(refs.clone()));
                    if changed {
                        // Copy first, render second.
                        let stats = archive::sweep(&refs);
                        if stats.files > 0 || stats.rotated > 0 || stats.errors > 0 {
                            hub.send(HubEvent::Archived(stats));
                        }
                        if let Ok(s) = hub.search_tx.lock() {
                            let _ = s.send(refs.clone());
                        }
                        last_sig = sig;
                    }
                    hub.reap_drivers();
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

    fn spawn_search(self: &Arc<Self>, rx: mpsc::Receiver<Vec<SessionRef>>) {
        let hub = Arc::clone(self);
        thread::Builder::new()
            .name("emaki-search".into())
            .spawn(move || {
                let Ok(mut index) = SearchIndex::open() else { return };
                while let Ok(mut refs) = rx.recv() {
                    // Only the newest request matters.
                    while let Ok(newer) = rx.try_recv() {
                        refs = newer;
                    }
                    let report = index.sync(&refs, false);
                    if report.indexed > 0 || report.failed > 0 {
                        hub.send(HubEvent::SearchSynced(report));
                    }
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
                let Ok(watcher) = Watcher::start(&roots, Duration::from_millis(250)) else { return };
                while let Ok(path) = watcher.events.recv() {
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

    pub fn refresh_peers(&self) {
        let fresh = peer::registry();
        *self.peers.lock().unwrap() = fresh;
    }

    /// The inbox of a terminal session, if Claude Code has one registered.
    pub fn peer_for(&self, session_id: &str) -> Option<Peer> {
        self.peers.lock().unwrap().get(session_id).cloned()
    }

    /// Whether something is behind this session: our driver, a registered
    /// inbox, or a transcript written recently enough that a process is the
    /// likely explanation.
    pub fn is_live(&self, r: &SessionRef, now: f64) -> bool {
        if self.driver_for(&r.session_id).is_some() || self.peer_for(&r.session_id).is_some() {
            return true;
        }
        if self.has_ended(&r.session_id) {
            return false;
        }
        now - r.mtime < LIVE_GRACE_S
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
                    hub.drivers.lock().unwrap().insert(session_id.clone(), Arc::clone(&d));
                    hub.ended.lock().unwrap().remove(&session_id);
                    hub.send(HubEvent::DriverStarted { session_id: session_id.clone() });
                    if let Some((text, images)) = first {
                        let r = d.send(&text, images);
                        hub.send(HubEvent::Sent {
                            session_id: session_id.clone(),
                            via: "driver",
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
                via: "driver",
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
                Some(p) => peer::send(&p, &text).err().unwrap_or_default(),
            };
            hub.send(HubEvent::Sent { session_id: sid, via: "inbox", queued: false, error });
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
                    hub.send(HubEvent::Note(format!("could not switch to {}: {}", driver::mode_label(&mode), e.0)));
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
                hub.send(HubEvent::Note(format!("could not switch to {}: {}", driver::model_label(&model), e.0)));
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
                via: "driver",
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

    pub fn stop_all(&self) {
        let all: Vec<Arc<Driver>> = self.drivers.lock().unwrap().drain().map(|(_, d)| d).collect();
        for d in all {
            d.stop();
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
