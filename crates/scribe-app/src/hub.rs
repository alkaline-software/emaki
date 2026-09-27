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
use scribe_core::adapters;
use scribe_core::archive;
use scribe_core::config::Config;
use scribe_core::driver::{self, Driver};
use scribe_core::search::{self, SearchIndex};
use scribe_core::transcript::SessionRef;
use scribe_core::watcher::Watcher;

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
    Sent { session_id: String, queued: bool, error: String },
}

pub struct Hub {
    tx: UnboundedSender<HubEvent>,
    wake: Mutex<mpsc::Sender<()>>,
    search_tx: Mutex<mpsc::Sender<Vec<SessionRef>>>,
    drivers: Mutex<HashMap<String, Arc<Driver>>>,
    /// Sessions whose driver exited; a fresh mtime alone must not count as a
    /// process for them.
    ended: Mutex<HashMap<String, Instant>>,
    pub cfg: RwLock<Config>,
}

impl Hub {
    pub fn start(cfg: Config) -> (Arc<Hub>, UnboundedReceiver<HubEvent>) {
        let (tx, rx) = unbounded();
        let (wake_tx, wake_rx) = mpsc::channel::<()>();
        let (search_tx, search_rx) = mpsc::channel::<Vec<SessionRef>>();
        let hub = Arc::new(Hub {
            tx,
            wake: Mutex::new(wake_tx),
            search_tx: Mutex::new(search_tx),
            drivers: Mutex::new(HashMap::new()),
            ended: Mutex::new(HashMap::new()),
            cfg: RwLock::new(cfg),
        });
        let _ = scribe_core::paths::ensure_dirs();
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
            .name("scribe-scan".into())
            .spawn(move || {
                let mut last_sig: Vec<(String, u64, f64)> = Vec::new();
                loop {
                    let (interval, agents) = {
                        let cfg = hub.cfg.read().unwrap();
                        (cfg.scan_interval_ms, cfg.agents.clone())
                    };
                    let refs = adapters::index_all(200, &agents);
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
            .name("scribe-search".into())
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
            .name("scribe-watch".into())
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

    /// Whether something is behind this session: our driver, or a transcript
    /// written recently enough that a process is the likely explanation.
    pub fn is_live(&self, r: &SessionRef, now: f64) -> bool {
        if self.driver_for(&r.session_id).is_some() {
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
