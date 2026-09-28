//! File watching over every agent's data roots. Events are coalesced per path
//! and delivered on a channel; the app decides what to do with them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

pub struct Watcher {
    _inner: RecommendedWatcher,
    pub events: Receiver<PathBuf>,
}

impl Watcher {
    /// Watch `roots` (those that exist) and emit changed `.jsonl` paths, each
    /// at most once per `debounce`.
    pub fn start(roots: &[PathBuf], debounce: Duration) -> notify::Result<Watcher> {
        let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut inner = notify::recommended_watcher(move |ev| {
            let _ = raw_tx.send(ev);
        })?;
        for root in roots {
            if root.is_dir() {
                let _ = inner.watch(root, RecursiveMode::Recursive);
            }
        }
        let (tx, rx): (Sender<PathBuf>, Receiver<PathBuf>) = mpsc::channel();
        thread::Builder::new()
            .name("emaki-watcher".into())
            .spawn(move || {
                let mut last: HashMap<PathBuf, Instant> = HashMap::new();
                while let Ok(ev) = raw_rx.recv() {
                    let Ok(ev) = ev else { continue };
                    for path in ev.paths {
                        if !interesting(&path) {
                            continue;
                        }
                        let now = Instant::now();
                        if let Some(t) = last.get(&path) {
                            if now.duration_since(*t) < debounce {
                                continue;
                            }
                        }
                        last.insert(path.clone(), now);
                        if tx.send(path).is_err() {
                            return;
                        }
                    }
                    if last.len() > 4096 {
                        let cutoff = Instant::now() - Duration::from_secs(60);
                        last.retain(|_, t| *t > cutoff);
                    }
                }
            })
            .ok();
        Ok(Watcher { _inner: inner, events: rx })
    }
}

fn interesting(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("jsonl") | Some("json") | Some("txt"))
}
