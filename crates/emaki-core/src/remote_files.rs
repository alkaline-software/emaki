//! A remote computer's folders, copied here as the files panel opens
//! them and kept the same both ways, so everything that reads a file
//! here (the tree, the editor, every preview, git's marks) reads one of
//! a remote computer's as it reads its own.
//!
//! A remote's folder `ssh://<remote>/<path>` is copied to
//! `~/.emaki/remote-files/<remote>/<path>` (`local_of`, `place_of`). Not
//! all of it: a folder is copied when the tree lists it (`want`), its
//! files and the names of the folders in it, and a file larger than
//! `BIG` stands empty until it is opened (`open_file`). A session in a
//! home folder copies what is looked at, not the home.
//!
//! Each remote has one `Engine`: a thread that lists what is wanted,
//! copies what differs, and runs a watch there that every second names
//! each wanted folder something in changed, with what it holds now. What
//! is written here (an edit saved, a file or folder made) is seen by a
//! watcher of this computer's and sent there. What is moved, renamed or
//! removed in the tree is done there first (`moved`, from
//! `fsops::move_path`): a move is a move there, and what is removed goes
//! to the remote's trash, from where taking it back brings it. Nothing
//! else here removes a file there.
//!
//! What each file was when last the same both ways is kept (`Entry`, in
//! `files.json` beside the copy): a file is sent when it differs from
//! that here, and copied when it differs there. Both, and the remote's
//! wins: it is where the agent works.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths;
use crate::remote::{self, sh_quote, Remote};

/// A file larger than this is copied when it is opened, not when its
/// folder is listed.
pub const BIG: u64 = 32 << 20;

/// Names never sent: what the system leaves in a folder it shows.
const NEVER_SENT: &[&str] = &[".DS_Store"];

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where every remote's folders are copied.
pub fn root() -> PathBuf {
    paths::root().join("remote-files")
}

/// Where one remote's folders are copied.
pub fn dir_of(remote: &str) -> PathBuf {
    root().join(paths::safe_component(remote))
}

/// The copy of a remote's folder (`ssh://<remote>/<path>`), here.
pub fn local_of(folder: &str) -> Option<PathBuf> {
    let (id, path) = remote::split(folder)?;
    Some(local_in(id, path))
}

fn local_in(id: &str, path: &str) -> PathBuf {
    let mut p = dir_of(id);
    for c in path.split('/').filter(|c| !c.is_empty() && *c != "." && *c != "..") {
        p.push(c);
    }
    p
}

/// Which remote a path here is the copy of a path of, and that path
/// there. `None` for a path that is no copy.
pub fn place_of(local: &Path) -> Option<(String, String)> {
    let rest = local.strip_prefix(root()).ok()?;
    let mut parts = rest.components();
    let id = parts.next()?.as_os_str().to_str()?.to_string();
    let mut path = String::new();
    for c in parts {
        match c {
            std::path::Component::Normal(n) => {
                path.push('/');
                path.push_str(&n.to_string_lossy());
            }
            _ => return None,
        }
    }
    Some((id, if path.is_empty() { "/".into() } else { path }))
}

/// Whether a path here is the copy of a remote's.
pub fn is_copy(local: &Path) -> bool {
    local.starts_with(root())
}

/// Start copying a remote's folder, if it is not being copied, and say
/// where its copy is. Cheap: the window asks as it draws.
pub fn open(folder: &str) -> Option<PathBuf> {
    let (id, path) = remote::split(folder)?;
    let local = local_in(id, path);
    engine(id)?.want(&norm(path));
    Some(local)
}

/// A folder the tree lists. When it is a copy, it is copied from there
/// and followed from now on. Cheap.
pub fn want(local_dir: &Path) {
    if let Some((id, path)) = place_of(local_dir) {
        if let Some(e) = engine(&id) {
            e.want(&path);
        }
    }
}

/// A file the window opens. One that stands empty for its size
/// (`BIG`) is copied now; the pane reads it again when it is in.
pub fn open_file(local: &Path) {
    if let Some((id, path)) = place_of(local) {
        if let Some(e) = engine(&id) {
            e.ask_file(&path);
        }
    }
}

/// How the copy of a remote's folder stands, for the files panel when
/// the copy is not there yet: still to come, or why it cannot.
#[derive(Debug, Clone, PartialEq)]
pub enum Coming {
    Bringing,
    Failed(String),
}

/// Whether the copy of a remote's folder is on its way: `None` once it
/// has been listed, or for a path that is no copy.
pub fn coming(local_dir: &Path) -> Option<Coming> {
    let (id, path) = place_of(local_dir)?;
    let e = engine(&id)?;
    let st = lock(&e.state);
    if st.listed.contains(&path) {
        return None;
    }
    Some(match &st.failed {
        Some(why) => Coming::Failed(why.clone()),
        None => Coming::Bringing,
    })
}

fn norm(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|c| !c.is_empty() && *c != "." && *c != "..").collect();
    format!("/{}", parts.join("/"))
}

fn parent_of(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => path[..i].to_string(),
    }
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

// ------------------------------------------------------------ the engines

static ENGINES: OnceLock<Mutex<HashMap<String, Arc<Engine>>>> = OnceLock::new();

/// The engine of a remote in Settings, started the first time it is
/// asked for; `None` for one that is not there.
fn engine(id: &str) -> Option<Arc<Engine>> {
    let map = ENGINES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = lock(map);
    if let Some(e) = map.get(id).filter(|e| !e.stopped.load(Ordering::Relaxed)) {
        return Some(Arc::clone(e));
    }
    let d = remote::find(id)?;
    let e = Engine::start(d);
    map.insert(id.to_string(), Arc::clone(&e));
    Some(e)
}

/// What a file or folder was when last the same here and there: its
/// size and time there, and its length and time here.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Entry {
    dir: bool,
    size: u64,
    mtime: i64,
    len: u64,
    /// Nanoseconds since 1970 here; 0 for a folder.
    at: u128,
    /// The bytes are here: false for a file standing empty for its size.
    whole: bool,
}

#[derive(Default)]
struct State {
    /// Folders there to copy and follow.
    wanted: BTreeSet<String>,
    /// Of them, the ones listed since the engine started.
    listed: HashSet<String>,
    /// Large files asked for.
    asked: BTreeSet<String>,
    files: HashMap<String, Entry>,
    /// Removed from the tree and in the remote's trash: where each went
    /// here, and where it is there.
    held: HashMap<PathBuf, String>,
    failed: Option<String>,
    dirty: bool,
}

struct Engine {
    remote: Remote,
    state: Mutex<State>,
    poke: Mutex<Option<Sender<()>>>,
    stopped: AtomicBool,
    /// A move of the tree's is being made, which the watcher here is not
    /// to read as an edit.
    moving: Mutex<()>,
}

impl Engine {
    fn start(remote: Remote) -> Arc<Engine> {
        let files = load(&remote.id);
        // A copy with no record of what it was is not trusted to say what
        // changed here: it is made again from there. Kept, a file of it
        // the remote had since changed or removed would be sent back.
        if files.is_empty() {
            let _ = std::fs::remove_dir_all(dir_of(&remote.id));
        }
        let e = Arc::new(Engine { remote, state: Mutex::new(State { files, ..State::default() }), poke: Mutex::new(None), stopped: AtomicBool::new(false), moving: Mutex::new(()) });
        let (tx, rx) = mpsc::channel();
        *lock(&e.poke) = Some(tx);
        let me = Arc::clone(&e);
        let _ = std::thread::Builder::new().name(format!("emaki-files-{}", me.remote.id)).spawn(move || me.run(rx));
        e
    }

    fn wake(&self) {
        if let Some(tx) = lock(&self.poke).as_ref() {
            let _ = tx.send(());
        }
    }

    fn want(&self, path: &str) {
        let mut st = lock(&self.state);
        if st.wanted.insert(path.to_string()) {
            drop(st);
            self.wake();
        }
    }

    fn ask_file(&self, path: &str) {
        let mut st = lock(&self.state);
        if st.files.get(path).is_some_and(|e| !e.dir && !e.whole) && st.asked.insert(path.to_string()) {
            drop(st);
            self.wake();
        }
    }

    fn local(&self, path: &str) -> PathBuf {
        local_in(&self.remote.id, path)
    }

    fn run(self: Arc<Self>, rx: Receiver<()>) {
        let (edits_tx, edits_rx) = mpsc::channel::<PathBuf>();
        let _ = std::fs::create_dir_all(dir_of(&self.remote.id));
        let watcher = watch_here(&dir_of(&self.remote.id), edits_tx);
        let mut wait = 5u64;
        while !self.stopped.load(Ordering::Relaxed) {
            if remote::find(&self.remote.id).is_none() {
                self.stopped.store(true, Ordering::Relaxed);
                break;
            }
            if !remote::is_on(&self.remote.id) {
                let _ = rx.recv_timeout(Duration::from_secs(5));
                continue;
            }
            match self.follow(&rx, &edits_rx) {
                Ok(()) => wait = 5,
                Err(why) => {
                    lock(&self.state).failed = Some(why);
                    let _ = rx.recv_timeout(Duration::from_secs(wait));
                    wait = (wait * 2).min(60);
                }
            }
        }
        drop(watcher);
    }

    /// List what is wanted and not listed, then follow it there and here
    /// until the link breaks or more is wanted.
    fn follow(&self, rx: &Receiver<()>, edits: &Receiver<PathBuf>) -> Result<(), String> {
        let fresh: Vec<String> = {
            let st = lock(&self.state);
            st.wanted.iter().filter(|w| !st.listed.contains(*w)).cloned().collect()
        };
        if !fresh.is_empty() {
            let out = remote::run(&self.remote, &format!("{LS}\n{}", fresh.iter().map(|d| format!("emaki_dir {}\n", sh_quote(d))).collect::<String>()), Duration::from_secs(60))?;
            self.take_listing(&out)?;
        }
        lock(&self.state).failed = None;
        let set: Vec<String> = lock(&self.state).wanted.iter().cloned().collect();
        let script = format!("{LS}\n{}\n{WATCH}", format_args!("set -- {}", set.iter().map(|d| sh_quote(d)).collect::<Vec<_>>().join(" ")));
        let mut cmd = remote::command(&self.remote, &script, false);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
        let lines = read_lines(&mut child);
        let mut block = String::new();
        let mut last = Instant::now();
        let ended = loop {
            if self.stopped.load(Ordering::Relaxed) {
                break Ok(());
            }
            // More wanted, or a large file asked for: the watch is made
            // again with the new set, after this.
            if rx.try_recv().is_ok() {
                let (more, asked) = {
                    let st = lock(&self.state);
                    (st.wanted.iter().any(|w| !st.listed.contains(w)), st.asked.iter().cloned().collect::<Vec<_>>())
                };
                if !asked.is_empty() {
                    let wants: Vec<(String, u64)> = {
                        let st = lock(&self.state);
                        asked.iter().filter_map(|p| st.files.get(p).map(|e| (p.clone(), e.size))).collect()
                    };
                    lock(&self.state).asked.clear();
                    if let Err(e) = self.fetch(&wants) {
                        break Err(e);
                    }
                }
                if more {
                    break Ok(());
                }
            }
            let mut sent = Vec::new();
            while let Ok(p) = edits.try_recv() {
                sent.push(p);
            }
            if !sent.is_empty() {
                // A moment for the rest of a save to land.
                std::thread::sleep(Duration::from_millis(150));
                while let Ok(p) = edits.try_recv() {
                    sent.push(p);
                }
                if let Err(e) = self.send_edits(sent) {
                    break Err(e);
                }
            }
            match lines.recv_timeout(Duration::from_millis(200)) {
                Ok(Some(line)) => {
                    last = Instant::now();
                    if line == "." {
                        if !block.is_empty() {
                            let out = std::mem::take(&mut block);
                            if let Err(e) = self.take_listing(&out) {
                                break Err(e);
                            }
                        }
                        self.save_if_dirty();
                    } else {
                        block.push_str(&line);
                        block.push('\n');
                    }
                }
                Ok(None) => break Err("the connection closed".into()),
                Err(RecvTimeoutError::Timeout) if last.elapsed() > Duration::from_secs(15) => break Err("the remote stopped answering".into()),
                Err(_) => {}
            }
        };
        let _ = child.kill();
        let _ = child.wait();
        self.save_if_dirty();
        ended
    }

    /// What a listing says (`LS`), folder by folder: each folder made
    /// the same as it is there.
    fn take_listing(&self, out: &str) -> Result<(), String> {
        let mut at: Option<String> = None;
        let mut kids: Vec<(String, bool, u64, i64)> = Vec::new();
        for line in out.lines() {
            if let Some(d) = line.strip_prefix("D|") {
                at = Some(d.to_string());
                kids.clear();
            } else if let Some(d) = line.strip_prefix("E|") {
                if at.as_deref() == Some(d) {
                    self.settle(d, &kids)?;
                }
                at = None;
            } else if let Some(d) = line.strip_prefix("G|") {
                self.gone(d);
            } else if at.is_some() {
                let mut it = line.splitn(4, '|');
                let (kind, size, mtime, path) = (it.next().unwrap_or("").to_lowercase(), it.next().and_then(|s| s.parse().ok()), it.next().and_then(|s| s.parse().ok()), it.next());
                let (Some(size), Some(mtime), Some(path)) = (size, mtime, path) else { continue };
                let dir = kind.contains("directory");
                if dir || kind.contains("regular") {
                    kids.push((path.to_string(), dir, size, mtime));
                }
            }
        }
        Ok(())
    }

    /// A folder there, now listed: what is in it here made the same.
    fn settle(&self, dir: &str, kids: &[(String, bool, u64, i64)]) -> Result<(), String> {
        let here = self.local(dir);
        let _ = std::fs::create_dir_all(&here);
        let mut fetch: Vec<(String, u64)> = Vec::new();
        let names: HashSet<&str> = kids.iter().map(|(p, ..)| name_of(p)).collect();
        {
            let mut st = lock(&self.state);
            for (path, is_dir, size, mtime) in kids {
                let path = norm(path);
                if parent_of(&path) != dir {
                    continue;
                }
                let local = self.local(&path);
                if *is_dir {
                    let _ = std::fs::create_dir_all(&local);
                    if !st.files.get(&path).is_some_and(|e| e.dir) {
                        st.files.insert(path, Entry { dir: true, ..Entry::default() });
                        st.dirty = true;
                    }
                    continue;
                }
                let known = st.files.get(&path).cloned();
                let now = stamp(&local);
                let same_there = known.as_ref().is_some_and(|e| !e.dir && e.size == *size && e.mtime == *mtime);
                let same_here = known.as_ref().is_some_and(|e| now == Some((e.len, e.at)));
                // The same there, and here as it was or changed here (the
                // watcher here sends that): nothing to copy.
                if same_there && now.is_some() {
                    let _ = same_here;
                    continue;
                }
                // Large and never opened: it stands empty for its size
                // until it is.
                if *size > BIG && !known.as_ref().is_some_and(|e| e.whole) && !st.asked.contains(&path) {
                    let _ = std::fs::write(&local, b"");
                    set_time(&local, *mtime);
                    let (len, at) = stamp(&local).unwrap_or_default();
                    st.files.insert(path, Entry { dir: false, size: *size, mtime: *mtime, len, at, whole: false });
                    st.dirty = true;
                    continue;
                }
                fetch.push((path, *size));
            }
            // What was copied from here and is gone there went there.
            if let Ok(rd) = std::fs::read_dir(&here) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if names.contains(name.as_str()) {
                        continue;
                    }
                    let path = if dir == "/" { format!("/{name}") } else { format!("{dir}/{name}") };
                    if st.files.contains_key(&path) {
                        let p = e.path();
                        if p.is_dir() {
                            let _ = std::fs::remove_dir_all(&p);
                        } else {
                            let _ = std::fs::remove_file(&p);
                        }
                        let prefix = format!("{path}/");
                        st.files.retain(|k, _| *k != path && !k.starts_with(&prefix));
                        st.dirty = true;
                    }
                }
            }
            st.listed.insert(dir.to_string());
        }
        self.fetch(&fetch)
    }

    /// A wanted folder that is not there any more.
    fn gone(&self, dir: &str) {
        let mut st = lock(&self.state);
        if st.files.contains_key(dir) {
            let _ = std::fs::remove_dir_all(self.local(dir));
            let prefix = format!("{dir}/");
            st.files.retain(|k, _| k != dir && !k.starts_with(&prefix));
            st.dirty = true;
        }
        st.wanted.remove(dir);
        st.listed.insert(dir.to_string());
    }

    /// Copy files from there, whole. Each is written beside the copy and
    /// moved in, so the watcher here sees the file arrive and not a part.
    fn fetch(&self, files: &[(String, u64)]) -> Result<(), String> {
        for batch in files.chunks(200) {
            let mut cmd = remote::command(&self.remote, FETCH, false);
            cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
            let mut stdin = child.stdin.take().ok_or("no stdin")?;
            let asks: String = batch.iter().map(|(p, _)| format!("{p}\n")).collect();
            let writer = std::thread::spawn(move || {
                let _ = stdin.write_all(asks.as_bytes());
            });
            let mut out = BufReader::with_capacity(1 << 20, child.stdout.take().ok_or("no stdout")?);
            let parts = root().join(format!(".parts-{}", paths::safe_component(&self.remote.id)));
            let _ = std::fs::create_dir_all(&parts);
            let mut result = Ok(());
            for (path, _) in batch {
                match self.take_file(&mut out, path, &parts) {
                    Ok(()) => {}
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            let _ = writer.join();
            if result.is_err() {
                let _ = child.kill();
            }
            let _ = child.wait();
            result?;
        }
        Ok(())
    }

    fn take_file(&self, out: &mut impl BufRead, path: &str, parts: &Path) -> Result<(), String> {
        let mut head = String::new();
        out.read_line(&mut head).map_err(|e| e.to_string())?;
        let mut it = head.trim_end_matches('\n').splitn(3, ' ');
        let (len, mtime, said) = (it.next().and_then(|s| s.parse::<u64>().ok()), it.next().and_then(|s| s.parse::<i64>().ok()), it.next().unwrap_or(""));
        let (Some(len), Some(mtime)) = (len, mtime) else { return Err("the remote computer's answer was not what was asked".into()) };
        if said != path {
            return Err("the remote computer's answer was not what was asked".into());
        }
        let local = self.local(path);
        let part = parts.join(format!("{}-{}", std::process::id(), name_of(path)));
        {
            let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
            let copied = std::io::copy(&mut out.take(len), &mut f).map_err(|e| e.to_string())?;
            if copied != len {
                return Err("the remote computer's answer was cut short".into());
            }
        }
        let mut trailer = vec![0u8; TRAILER.len()];
        out.read_exact(&mut trailer).map_err(|e| e.to_string())?;
        if trailer != TRAILER {
            return Err("the remote computer's answer was cut short".into());
        }
        // Gone or not a file there by now: nothing.
        if mtime < 0 {
            let _ = std::fs::remove_file(&part);
            return Ok(());
        }
        // Changed here since it was asked for, and not yet sent: the
        // remote's still wins, as it does at a listing.
        let mut st = lock(&self.state);
        if let Some(parent) = local.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        set_time(&part, mtime);
        std::fs::rename(&part, &local).map_err(|e| e.to_string())?;
        let (l, at) = stamp(&local).unwrap_or_default();
        st.files.insert(path.to_string(), Entry { dir: false, size: len, mtime, len: l, at, whole: true });
        st.dirty = true;
        Ok(())
    }

    /// Paths written here: each file that differs from what it was when
    /// last the same, and each folder made, sent there.
    fn send_edits(&self, mut paths: Vec<PathBuf>) -> Result<(), String> {
        let _held = lock(&self.moving);
        paths.sort();
        paths.dedup();
        for p in paths {
            let Some((_, path)) = place_of(&p) else { continue };
            if NEVER_SENT.contains(&name_of(&path)) || path.split('/').any(|c| c == ".git") {
                continue;
            }
            // Only into a folder that is copied, and is there.
            if !lock(&self.state).files.contains_key(&parent_of(&path)) && !lock(&self.state).wanted.contains(&parent_of(&path)) {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            if meta.is_dir() {
                self.send_tree(&p, &path)?;
                continue;
            }
            let known = lock(&self.state).files.get(&path).cloned();
            let now = stamp(&p);
            if known.as_ref().is_some_and(|e| !e.dir && now == Some((e.len, e.at))) {
                continue;
            }
            // A large file still standing empty is never sent over the
            // file there.
            if known.as_ref().is_some_and(|e| !e.dir && !e.whole) && now.is_some_and(|(len, _)| len == 0) {
                continue;
            }
            self.send_file(&p, &path)?;
        }
        Ok(())
    }

    /// A folder made here, and whatever in it was never sent. One that
    /// was there already is left: a file written in it is told of by
    /// itself.
    fn send_tree(&self, local: &Path, path: &str) -> Result<(), String> {
        if lock(&self.state).files.get(path).is_some_and(|e| e.dir) {
            return Ok(());
        }
        remote::run(&self.remote, &format!("mkdir -p {}", sh_quote(path)), Duration::from_secs(30))?;
        {
            let mut st = lock(&self.state);
            st.files.insert(path.to_string(), Entry { dir: true, ..Entry::default() });
            st.dirty = true;
        }
        let Ok(rd) = std::fs::read_dir(local) else { return Ok(()) };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if NEVER_SENT.contains(&name.as_str()) || name == ".git" {
                continue;
            }
            let there = format!("{}/{name}", path.trim_end_matches('/'));
            let p = e.path();
            if p.is_dir() {
                self.send_tree(&p, &there)?;
            } else if !lock(&self.state).files.contains_key(&there) {
                self.send_file(&p, &there)?;
            }
        }
        Ok(())
    }

    /// One file here written there whole, its mode kept when it was
    /// there, and what it is there now kept.
    fn send_file(&self, local: &Path, path: &str) -> Result<(), String> {
        let before = stamp(local);
        let bytes = std::fs::read(local).map_err(|e| e.to_string())?;
        let script = format!("{STAT}\np={}\n{SEND}", sh_quote(path));
        let mut cmd = remote::command(&self.remote, &script, false);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
        let out = remote::wait_output(child, Duration::from_secs(300))?;
        let _ = writer.join();
        if out.0 != Some(0) {
            return Err(remote::ssh_error(&out.2, out.0));
        }
        let mut it = out.1.trim().splitn(2, ' ');
        let (size, mtime) = (it.next().and_then(|s| s.parse().ok()).unwrap_or(0), it.next().and_then(|s| s.parse().ok()).unwrap_or(0));
        let mut st = lock(&self.state);
        // Changed again while it went: sent again at the next look.
        let (len, at) = if stamp(local) == before { before.unwrap_or_default() } else { (u64::MAX, 0) };
        st.files.insert(path.to_string(), Entry { dir: false, size, mtime, len, at, whole: true });
        st.dirty = true;
        Ok(())
    }

    fn save_if_dirty(&self) {
        let files = {
            let mut st = lock(&self.state);
            if !st.dirty {
                return;
            }
            st.dirty = false;
            st.files.clone()
        };
        let _ = paths::write_json(&book(&self.remote.id), &serde_json::to_value(&files).unwrap_or_default());
    }
}

/// Where what each file was is kept.
fn book(id: &str) -> PathBuf {
    root().join(format!(".{}.json", paths::safe_component(id)))
}

fn load(id: &str) -> HashMap<String, Entry> {
    paths::read_json(&book(id)).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// A file's length and time here, the time in nanoseconds.
fn stamp(p: &Path) -> Option<(u64, u128)> {
    let m = std::fs::metadata(p).ok().filter(|m| m.is_file())?;
    let at = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some((m.len(), at))
}

fn set_time(p: &Path, mtime: i64) {
    if let Ok(f) = std::fs::File::options().write(true).open(p) {
        let _ = f.set_modified(UNIX_EPOCH + Duration::from_secs(mtime.max(0) as u64));
    }
}

/// Lines a child prints, on a channel: `None` when it has ended.
fn read_lines(child: &mut Child) -> Receiver<Option<String>> {
    let (tx, rx) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                if tx.send(Some(line)).is_err() {
                    return;
                }
            }
            let _ = tx.send(None);
        });
    }
    rx
}

/// A watcher on a remote's copy here: every path written, made or
/// renamed in it, for `send_edits` to look at.
fn watch_here(dir: &Path, tx: Sender<PathBuf>) -> Option<notify::RecommendedWatcher> {
    use notify::{EventKind, RecursiveMode, Watcher};
    let mut w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
        let Ok(ev) = ev else { return };
        if matches!(ev.kind, EventKind::Access(_) | EventKind::Remove(_)) {
            return;
        }
        for p in ev.paths {
            let _ = tx.send(p);
        }
    })
    .ok()?;
    w.watch(dir, RecursiveMode::Recursive).ok()?;
    Some(w)
}

// ------------------------------------------------------------ moves

/// A move of the tree's (`fsops::move_path`), made there too when
/// either end is a remote's copy: `local` makes it here. Within one
/// remote it is a move there; out of a copy (removed, into the holding
/// folder) the thing goes to the remote's trash; into a copy, from
/// what was removed it comes back from the trash, and from anywhere
/// else it is sent.
pub fn moved(from: &Path, to: &Path, local: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let (a, b) = (place_of(from), place_of(to));
    if a.is_none() && b.is_none() {
        return local();
    }
    match (a, b) {
        (Some((ia, pa)), Some((ib, pb))) if ia == ib => {
            let e = engine(&ia).ok_or("that computer is not in Settings, Connected Devices")?;
            let _held = lock(&e.moving);
            let script = format!("[ -e {b} ] && {{ echo 'already there' >&2; exit 3; }}; mkdir -p {d} && mv {a} {b}", a = sh_quote(&pa), b = sh_quote(&pb), d = sh_quote(&parent_of(&pb)));
            remote::run(&e.remote, &script, Duration::from_secs(60))?;
            local()?;
            let mut st = lock(&e.state);
            let prefix = format!("{pa}/");
            let keys: Vec<String> = st.files.keys().filter(|k| **k == pa || k.starts_with(&prefix)).cloned().collect();
            for k in keys {
                if let Some(v) = st.files.remove(&k) {
                    st.files.insert(format!("{pb}{}", &k[pa.len()..]), v);
                }
            }
            st.dirty = true;
            Ok(())
        }
        (Some(_), Some(_)) => Err("a file cannot be moved from one computer to another".into()),
        (Some((id, path)), None) => {
            let e = engine(&id).ok_or("that computer is not in Settings, Connected Devices")?;
            let _held = lock(&e.moving);
            let out = remote::run(&e.remote, &format!("p={}\n{TRASH}", sh_quote(&path)), Duration::from_secs(60))?;
            let there = out.trim().to_string();
            if there.is_empty() {
                return Err("it could not be moved to the trash there".into());
            }
            if let Err(err) = local() {
                let _ = remote::run(&e.remote, &format!("mv {} {}", sh_quote(&there), sh_quote(&path)), Duration::from_secs(60));
                return Err(err);
            }
            let mut st = lock(&e.state);
            let prefix = format!("{path}/");
            st.files.retain(|k, _| *k != path && !k.starts_with(&prefix));
            st.held.insert(to.to_path_buf(), there);
            st.dirty = true;
            Ok(())
        }
        (None, Some((id, path))) => {
            let e = engine(&id).ok_or("that computer is not in Settings, Connected Devices")?;
            let _held = lock(&e.moving);
            let back = lock(&e.state).held.remove(from);
            local()?;
            if let Some(there) = back {
                let script = format!("[ -e {b} ] && exit 3; mkdir -p {d} && mv {a} {b}", a = sh_quote(&there), b = sh_quote(&path), d = sh_quote(&parent_of(&path)));
                if remote::run(&e.remote, &script, Duration::from_secs(60)).is_ok() {
                    // What it holds is listed again when its folder is.
                    let prefix = format!("{path}/");
                    let mut st = lock(&e.state);
                    st.listed.retain(|k| *k != path && !k.starts_with(&prefix));
                    drop(st);
                    mark_sent(&e, to, &path);
                    e.wake();
                    return Ok(());
                }
            }
            if to.is_dir() {
                e.send_tree(to, &path)
            } else {
                e.send_file(to, &path)
            }
        }
        (None, None) => local(),
    }
}

/// What was brought back from the trash there is as it is here: kept
/// as the same both ways, so the watcher here sends none of it again.
fn mark_sent(e: &Engine, local: &Path, path: &str) {
    let mut st = lock(&e.state);
    let mut stack = vec![(local.to_path_buf(), path.to_string())];
    while let Some((l, p)) = stack.pop() {
        if l.is_dir() {
            st.files.insert(p.clone(), Entry { dir: true, ..Entry::default() });
            if let Ok(rd) = std::fs::read_dir(&l) {
                for k in rd.flatten() {
                    stack.push((k.path(), format!("{p}/{}", k.file_name().to_string_lossy())));
                }
            }
        } else if let Some((len, at)) = stamp(&l) {
            let mtime = (at / 1_000_000_000) as i64;
            let whole = st.files.get(&p).is_none_or(|e| e.whole);
            st.files.insert(p, Entry { dir: false, size: len, mtime, len, at, whole });
        }
    }
    st.dirty = true;
}

// ------------------------------------------------------------ scripts

/// What follows each file's bytes, so one cut short is known.
const TRAILER: &[u8] = b"\n--emaki--\n";

/// `stat` as BSD's or GNU's says it: `emaki_stat <path>` prints `size
/// mtime`, following a link.
const STAT: &str = r#"
emaki_stat() {
  if stat -c %s . >/dev/null 2>&1; then stat -L -c '%s %Y' "$1"; else stat -L -f '%z %m' "$1"; fi
}
"#;

/// `emaki_dir <dir>` prints `D|<dir>`, a line a thing in it (`kind|size|
/// mtime|path`, `stat`'s kind as BSD's or GNU's names it, links
/// followed), and `E|<dir>`; `G|<dir>` for one not there.
const LS: &str = r#"
emaki_dir() {
  if [ ! -d "$1" ]; then printf 'G|%s\n' "$1"; return 0; fi
  printf 'D|%s\n' "$1"
  if stat -c %s . >/dev/null 2>&1; then
    find "$1" -mindepth 1 -maxdepth 1 -exec stat -L -c '%F|%s|%Y|%n' {} + 2>/dev/null
  else
    find "$1" -mindepth 1 -maxdepth 1 -exec stat -L -f '%HT|%z|%m|%N' {} + 2>/dev/null
  fi
  printf 'E|%s\n' "$1"
}
"#;

/// The watch over the folders given as `$@`: once a second, each that
/// changed since the look before (it, or a thing in it, newer than a
/// mark of its own) listed whole, then a `.`, so a link that broke is
/// known by its silence.
const WATCH: &str = r#"
m="${TMPDIR:-/tmp}"; m="${m%/}/emaki-files-$$"
: > "$m"
trap 'rm -f "$m" "$m.n"' EXIT
trap 'exit 0' INT TERM HUP PIPE
while :; do
  : > "$m.n"
  for d in "$@"; do
    if [ ! -d "$d" ]; then
      printf 'G|%s\n' "$d"
    elif [ -n "$(find "$d" -maxdepth 1 -newer "$m" 2>/dev/null | head -n 1)" ]; then
      emaki_dir "$d"
    fi
  done
  mv -f "$m.n" "$m"
  echo .
  sleep 1
done
"#;

/// Each path on stdin sent back: `len mtime path`, the bytes, the
/// trailer; `0 -1 path` for one that is not a file there now.
const FETCH: &str = r#"
emaki_stat() {
  if stat -c %s . >/dev/null 2>&1; then stat -L -c '%s %Y' "$1"; else stat -L -f '%z %m' "$1"; fi
}
t="${TMPDIR:-/tmp}"; t="${t%/}/emaki-get-$$"
trap 'rm -f "$t"' EXIT
while IFS= read -r p; do
  if [ -f "$p" ] && cat "$p" > "$t" 2>/dev/null; then
    m=$(emaki_stat "$p" | cut -d' ' -f2)
    n=$(wc -c < "$t" | tr -d ' ')
    printf '%s %s %s\n' "$n" "$m" "$p"
    cat "$t"
  else
    printf '0 -1 %s\n' "$p"
  fi
  printf '\n--emaki--\n'
done
"#;

/// The bytes on stdin written to `$p`: beside it first, in the
/// temporary folder, then moved in with the mode it had; its size and
/// time there printed.
const SEND: &str = r#"
t="${TMPDIR:-/tmp}"; t="${t%/}/emaki-put-$$"
md=""
if [ -f "$p" ]; then md=$(stat -c %a "$p" 2>/dev/null || stat -f %Lp "$p" 2>/dev/null); fi
mkdir -p "$(dirname "$p")" && cat > "$t" && { [ -z "$md" ] || chmod "$md" "$t"; } && mv -f "$t" "$p" || { rm -f "$t"; exit 1; }
emaki_stat "$p"
"#;

/// `$p` moved to the trash there, the Mac's or the freedesktop one,
/// under a name nothing has there; where it went printed. Where the
/// trash cannot be written (a Mac's, over ssh, without the access it
/// asks for), a folder of Emaki's in the home.
const TRASH: &str = r#"
n=$(basename "$p")
for t in "$HOME/.Trash" "${XDG_DATA_HOME:-$HOME/.local/share}/Trash/files" "$HOME/.emaki-removed"; do
  case "$t" in */Trash/files) [ "$(uname)" = Darwin ] && continue; mkdir -p "$t" 2>/dev/null ;; */.emaki-removed) mkdir -p "$t" ;; esac
  [ -d "$t" ] || continue
  d="$t/$n"; i=2
  while [ -e "$d" ]; do d="$t/$n $i"; i=$((i + 1)); done
  if mv "$p" "$d" 2>/dev/null; then
    case "$t" in */Trash/files)
      i="${t%/files}/info"; mkdir -p "$i" && printf '[Trash Info]\nPath=%s\nDeletionDate=%s\n' "$p" "$(date +%Y-%m-%dT%H:%M:%S)" > "$i/$(basename "$d").trashinfo" ;;
    esac
    printf '%s\n' "$d"; exit 0
  fi
done
exit 1
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_folder_has_one_copy_here_and_back() {
        let local = local_of("ssh://mba/Users/me/my project").unwrap();
        assert!(local.ends_with("remote-files/mba/Users/me/my project"));
        assert_eq!(place_of(&local), Some(("mba".into(), "/Users/me/my project".into())));
        assert_eq!(place_of(&dir_of("mba")), Some(("mba".into(), "/".into())));
        assert_eq!(place_of(Path::new("/Users/me/my project")), None);
        // Nothing climbs out of the copy.
        assert!(local_of("ssh://mba/Users/../../etc").unwrap().starts_with(dir_of("mba")));
    }

    #[test]
    fn a_path_there_is_cut_as_a_path() {
        assert_eq!(norm("/a//b/./c/"), "/a/b/c");
        assert_eq!(parent_of("/a/b"), "/a");
        assert_eq!(parent_of("/a"), "/");
        assert_eq!(name_of("/a/b c"), "b c");
    }
}
