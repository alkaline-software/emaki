//! Where a project's folder went.
//!
//! A folder renamed or moved leaves every session recorded in it naming a
//! path that is gone: Claude Code and Codex keep the folder a session ran
//! in, in its rows, for good. Nothing of theirs is rewritten. A move is
//! kept here (`state/moves.json`), and a session's folder is read through
//! it (`SessionRef::moved`); both agents continue a session from whatever
//! folder they are started in.
//!
//! Where a gone folder went is found (`survey`) by its identity, which a
//! rename keeps, or by its name where the move of a parent says it would
//! be, and the scanner keeps each move it finds. One the person takes back
//! is kept too, as going nowhere, and is not found again.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::paths;

static LOCK: Mutex<()> = Mutex::new(());

fn moves_file() -> PathBuf {
    paths::state_dir().join("moves.json")
}

/// Each folder's identity (device and file number) as last seen while it
/// was there: a rename on the same disk keeps it.
fn ids_file() -> PathBuf {
    paths::state_dir().join("folders.json")
}

/// The moves kept, from the old path to the new; an empty new path is a
/// move the person took back.
#[derive(Debug, Clone, Default)]
pub struct Moves(Vec<(PathBuf, PathBuf)>);

pub fn load() -> Moves {
    let map: BTreeMap<String, String> = paths::read_json(&moves_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    Moves(map.into_iter().map(|(from, to)| (PathBuf::from(from), PathBuf::from(to))).collect())
}

impl Moves {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The move nearest a folder: the one kept for it or for the folder
    /// nearest it that it is in.
    fn nearest(&self, at: &Path) -> Option<&(PathBuf, PathBuf)> {
        self.0.iter().filter(|(from, _)| at.starts_with(from)).max_by_key(|(from, _)| from.components().count())
    }

    /// Whether the person took back the move nearest this folder.
    fn taken_back(&self, at: &Path) -> bool {
        self.nearest(at).is_some_and(|(_, to)| to.as_os_str().is_empty())
    }

    /// Where a folder recorded as `cwd` is now, when it has moved: the
    /// confirmed move nearest it applied, a folder inside the one moved
    /// going with it, and a move of where that lands after it. A folder
    /// that is there is where it is, so one made again at an old path is
    /// its own.
    pub fn now(&self, cwd: &str) -> Option<String> {
        if self.0.is_empty() || cwd.is_empty() {
            return None;
        }
        let mut at = PathBuf::from(cwd);
        let mut moved = false;
        for _ in 0..8 {
            let Some((from, to)) = self.nearest(&at) else { break };
            if to.as_os_str().is_empty() || at.exists() {
                break;
            }
            let rest = at.strip_prefix(from).map(Path::to_path_buf).unwrap_or_default();
            let next = if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) };
            if next == at {
                break;
            }
            at = next;
            moved = true;
        }
        moved.then(|| at.to_string_lossy().to_string())
    }
}

/// The folder at `from` is now at `to`.
pub fn record(from: &str, to: &str) -> std::io::Result<()> {
    if from.is_empty() || to.is_empty() || from == to {
        return Ok(());
    }
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map: BTreeMap<String, String> = paths::read_json(&moves_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    map.insert(from.to_string(), to.to_string());
    paths::write_json(&moves_file(), &serde_json::to_value(&map).unwrap_or_default())
}

/// The person took back the move kept for `from`: its sessions are listed
/// where their transcripts say again, and it is not found again.
pub fn take_back(from: &str) -> std::io::Result<()> {
    if from.is_empty() {
        return Ok(());
    }
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map: BTreeMap<String, String> = paths::read_json(&moves_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    map.insert(from.to_string(), String::new());
    paths::write_json(&moves_file(), &serde_json::to_value(&map).unwrap_or_default())
}

/// The folders the sessions name, as one look finds them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Survey {
    /// The folders that are not there.
    pub gone: HashSet<String>,
    /// Where each of those is now, where that was found.
    pub to: HashMap<String, String>,
}

/// Look at every folder the sessions name: which are gone, and where each
/// of those may have gone. The identity of each that is there is kept for
/// a later pass, when it may not be. The caller passes temporary folders
/// by (`paths::is_temp_dir`): the system clears them, and nobody goes back
/// to one.
pub fn survey<'a>(folders: impl IntoIterator<Item = &'a str>) -> Survey {
    let mut seen: HashSet<&str> = HashSet::new();
    let (mut gone, mut live): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for f in folders {
        if f.is_empty() || !seen.insert(f) {
            continue;
        }
        if Path::new(f).is_dir() {
            live.push(f.to_string());
        } else {
            gone.push(f.to_string());
        }
    }
    note(&live);
    let to = whereabouts(&gone, &live);
    Survey { gone: gone.into_iter().collect(), to }
}

#[cfg(unix)]
fn id_of(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).ok()?;
    m.is_dir().then(|| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn id_of(_path: &Path) -> Option<(u64, u64)> {
    None
}

fn saved_ids() -> BTreeMap<String, (u64, u64)> {
    paths::read_json(&ids_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// Keep the identity of each folder that is there. Written only when one
/// is new or changed. Off Unix there is none to keep, and a move is
/// guessed from names alone.
fn note(live: &[String]) {
    if cfg!(not(unix)) {
        return;
    }
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ids = saved_ids();
    let mut changed = false;
    for f in live {
        if let Some(id) = id_of(Path::new(f)) {
            if ids.get(f) != Some(&id) {
                ids.insert(f.clone(), id);
                changed = true;
            }
        }
    }
    if changed {
        let _ = paths::write_json(&ids_file(), &serde_json::to_value(&ids).unwrap_or_default());
    }
}

/// Where the system says a folder of this identity is now, wherever on
/// its disk it went. A Mac finds a file by its number (`/.vol`) and says
/// its path (`F_GETPATH`).
#[cfg(target_os = "macos")]
fn path_of_id(dev: u64, ino: u64) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;
    let f = std::fs::File::open(format!("/.vol/{dev}/{ino}")).ok()?;
    let mut buf = vec![0u8; libc::PATH_MAX as usize + 1];
    // SAFETY: F_GETPATH writes at most PATH_MAX bytes and a nul into the
    // buffer, which is longer than that; the descriptor is open.
    let r = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) };
    if r == -1 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0)?;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..end])))
}

#[cfg(not(target_os = "macos"))]
fn path_of_id(_dev: u64, _ino: u64) -> Option<PathBuf> {
    None
}

/// A folder worth offering: there, not in a trash (a folder put in the
/// Trash keeps its identity too), and not a temporary one for a folder
/// that was not.
fn plausible(p: &Path, from: &str) -> bool {
    p != Path::new(from)
        && p.is_dir()
        && !p.components().any(|c| c.as_os_str() == ".Trash" || c.as_os_str() == ".Trashes")
        && (paths::is_temp_dir(from) || !paths::is_temp_dir(&p.to_string_lossy()))
}

/// Where each of `gone` may be now. In order:
/// - the folder itself by its identity: one of `live` that is it, or, on
///   a Mac, wherever the system says it is;
/// - a move carried to the parents it says moved: once `a/old/x` went to
///   `a/new/x`, `a/old/y` is looked for at `a/new/y`;
/// - the one folder of `live` with its name, when its parent is gone too.
///   The last two are where the parent's move says, and so are tried
///   together, the nearest first.
fn whereabouts(gone: &[String], live: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if gone.is_empty() {
        return out;
    }
    let ids = saved_ids();
    let live_ids: HashMap<(u64, u64), &String> = live.iter().filter_map(|l| id_of(Path::new(l)).map(|id| (id, l))).collect();
    // The pairs of old and new: each move, and the parents it says moved.
    let mut pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut add = |from: &Path, to: &Path| {
        pairs.push((from.to_path_buf(), to.to_path_buf()));
        let (mut f, mut t) = (from.to_path_buf(), to.to_path_buf());
        while f.file_name().is_some() && f.file_name() == t.file_name() {
            f.pop();
            t.pop();
        }
        if f.as_path() != from && f.file_name().is_some() && t.file_name().is_some() {
            pairs.push((f, t));
        }
    };
    let moves = load();
    for (from, to) in moves.0.iter().filter(|(_, to)| !to.as_os_str().is_empty()) {
        add(from, to);
    }
    for g in gone {
        let parent_gone = Path::new(g).parent().is_some_and(|p| p.file_name().is_some() && !p.exists());
        if !parent_gone || moves.taken_back(Path::new(g)) {
            continue;
        }
        let name = Path::new(g).file_name();
        let mut same = live.iter().filter(|l| Path::new(l.as_str()).file_name() == name);
        if let (Some(l), None) = (same.next(), same.next()) {
            add(Path::new(g), Path::new(l));
        }
    }
    for g in gone.iter().filter(|g| !moves.taken_back(Path::new(g.as_str()))) {
        let by_id = ids.get(g).and_then(|id| match live_ids.get(id) {
            Some(l) => Some(PathBuf::from(l.as_str())),
            None => path_of_id(id.0, id.1),
        });
        let by_parent = || {
            let at = Path::new(g);
            pairs.iter().filter(|(from, _)| at.starts_with(from)).max_by_key(|(from, _)| from.components().count()).map(|(from, to)| {
                let rest = at.strip_prefix(from).unwrap_or(Path::new(""));
                if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) }
            })
        };
        let found = by_id.filter(|p| plausible(p, g)).or_else(|| by_parent().filter(|p| plausible(p, g)));
        if let Some(p) = found {
            out.insert(g.clone(), p.to_string_lossy().to_string());
        }
    }
    out
}
