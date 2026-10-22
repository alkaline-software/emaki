//! Sessions deleted in the window. A deleted session is not gone at
//! once: it is archived, so its agent no longer lists or continues it,
//! and kept out of every list but the Trash's for thirty days, a count
//! of its own from when it was deleted. Put back, it is unarchived
//! again where deleting archived it, and its count is forgotten. Past
//! its thirty days, every file of it is removed for good
//! (`docs/core.md`, The trash).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::adapters;
use crate::paths;
use crate::transcript::SessionRef;

/// How long a deleted session is kept before it is removed for good.
pub const KEEP_DAYS: f64 = 30.0;
const DAY_S: f64 = 24.0 * 3600.0;

/// One deleted session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    /// When it was deleted, in seconds since 1970.
    pub at: f64,
    /// Whether deleting it archived it: putting it back unarchives it
    /// then, and only then.
    #[serde(default)]
    pub archived_here: bool,
}

impl Entry {
    /// Whole days left before it is removed, counted up: thirty on the
    /// day it was deleted, one on its last.
    pub fn days_left(&self, now: f64) -> u32 {
        ((self.at + KEEP_DAYS * DAY_S - now) / DAY_S).ceil().max(0.) as u32
    }

    pub fn due(&self, now: f64) -> bool {
        now >= self.at + KEEP_DAYS * DAY_S
    }
}

/// The deleted sessions, by the window's key for a session
/// (`<agent>:<session id>`).
pub type Trash = BTreeMap<String, Entry>;

/// Every change is a read, a change and a write under this, so two
/// threads of one Emaki never write over each other's.
static LOCK: Mutex<()> = Mutex::new(());

fn file() -> std::path::PathBuf {
    paths::state_dir().join("trash.json")
}

pub fn key(r: &SessionRef) -> String {
    format!("{}:{}", r.agent.as_str(), r.session_id)
}

pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

pub fn load() -> Trash {
    std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn change(f: impl FnOnce(&mut Trash)) -> Result<(), String> {
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut trash = load();
    f(&mut trash);
    let text = serde_json::to_string_pretty(&trash).map_err(|e| e.to_string())?;
    paths::write_atomic(&file(), text.as_bytes()).map_err(|e| e.to_string())
}

/// Delete a session into the trash. One its agent still has is archived
/// first; when that cannot be done (a terminal of the person's still
/// writing it) it goes to the trash as it is, and is removed whole when
/// its days are up all the same.
pub fn put(r: &SessionRef) -> Result<(), String> {
    let archived_here = !r.archived && adapters::for_agent(r.agent).archive(r).is_ok();
    let entry = Entry { at: now(), archived_here };
    change(|t| {
        t.insert(key(r), entry);
    })
}

/// Take a session out of the trash, back to how it was before it was
/// deleted. It leaves the trash even when it cannot be unarchived; it
/// is then listed as archived, and the error says why.
pub fn put_back(r: &SessionRef) -> Result<(), String> {
    let entry = load().get(&key(r)).cloned();
    change(|t| {
        t.remove(&key(r));
    })?;
    match entry {
        Some(e) if e.archived_here => adapters::for_agent(r.agent).unarchive(r),
        _ => Ok(()),
    }
}

/// Remove a session for good: its agent's copy by its agent's word, and
/// every file of it Emaki has.
pub fn purge(r: &SessionRef) -> Result<(), String> {
    adapters::delete_session(r, remove)?;
    change(|t| {
        t.remove(&key(r));
    })
}

fn remove(p: &Path) -> Result<(), String> {
    let done = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
    done.map_err(|e| format!("{}: {e}", p.display()))
}

/// The sessions whose thirty days are up, removed; and the trash's
/// record of one no session answers to any more let go. Every agent's
/// sessions are asked for, the ones turned off in Settings too, so none
/// is forgotten unremoved.
pub fn purge_due() {
    let now = now();
    let due: Vec<String> = load().into_iter().filter(|(_, e)| e.due(now)).map(|(k, _)| k).collect();
    if due.is_empty() {
        return;
    }
    let refs = adapters::index_all(0, &[]);
    for k in due {
        match refs.iter().find(|r| key(r) == k) {
            Some(r) => {
                let _ = purge(r);
            }
            None => {
                let _ = change(|t| {
                    t.remove(&k);
                });
            }
        }
    }
}
