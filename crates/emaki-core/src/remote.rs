//! Connected Devices (Settings): other computers whose sessions this window lists,
//! reads and runs, over ssh. Each is a `Remote` here. Nothing of Emaki's is installed there; it needs the
//! system's ssh server (Remote Login on a Mac), a key this computer signs
//! in with, and the agents themselves.
//!
//! A remote's folder is written `ssh://<remote>/<path>` wherever a
//! session's folder goes (`folder`, `split`), so a project there is a
//! project of its own beside one at the same path here, and whatever
//! starts an agent knows where.
//!
//! Its sessions are copied here as they are written (`Link`): every file
//! of Claude Code's projects and of Codex's sessions, into
//! `~/.emaki/remote/<id>/`, a file's new bytes at a time. The copy is
//! read as the agents' own folders are, so a session there is listed,
//! rendered and searched as one here is. What Claude Code deletes there
//! after its thirty days stays here, as the archive keeps what it deletes
//! here. Codex deletes nothing by age, and its copy follows its files.
//!
//! An agent there runs as `ssh <remote> -- cd <path> && exec claude ...`
//! (`agent_command`): the same child on the same stdin and stdout as one
//! started here.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::paths;

/// How long a remote has to answer at all before it is taken as not there.
pub const REACH: Duration = Duration::from_secs(5);

/// How a remote's folder begins.
pub const SCHEME: &str = "ssh://";

/// What is copied from a remote, under its home.
const CLAUDE_PROJECTS: &str = ".claude/projects";
const CODEX: &str = ".codex";
const CODEX_ROOTS: [&str; 2] = [".codex/sessions", ".codex/archived_sessions"];

/// One way to reach a computer: what ssh is given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Way {
    /// A name, a number, or a host of `~/.ssh/config`.
    pub host: String,
    /// The account signed in to; empty for ssh's own choice.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub user: String,
    /// 0 for ssh's own choice.
    #[serde(skip_serializing_if = "is_zero")]
    pub port: u16,
}

fn is_zero(n: &u16) -> bool {
    *n == 0
}

impl Way {
    /// Whether it reaches the computer on the network this one is on: a
    /// name the network gives (`.local`, `.lan`, `.home.arpa`) or a
    /// private address. Anything else reaches it from anywhere.
    pub fn local(&self) -> bool {
        let h = self.host.trim_end_matches('.').to_ascii_lowercase();
        if [".local", ".lan", ".home.arpa"].iter().any(|s| h.ends_with(s)) {
            return true;
        }
        match h.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(a)) => a.is_private() || a.is_link_local(),
            Ok(std::net::IpAddr::V6(a)) => (a.segments()[0] & 0xffc0) == 0xfe80 || (a.segments()[0] & 0xfe00) == 0xfc00,
            Err(_) => false,
        }
    }

    /// `user@host`, as ssh is given it and the window says it.
    pub fn address(&self) -> String {
        if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }
}

/// A computer this window reaches over ssh.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Kept")]
pub struct Remote {
    /// The name its folders are written under: letters, digits and
    /// dashes, never changed once given.
    pub id: String,
    /// What the window calls it.
    pub name: String,
    /// The ways to it, in the order they were added: its name on the
    /// network at home, its name on a private network that reaches it from
    /// anywhere, any other.
    pub ways: Vec<Way>,
    /// The way the person connected it by, the one every command takes;
    /// none for the first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub using: Option<Way>,
    /// Disconnected by the person: kept, with its sessions as they were
    /// copied, and nothing reached there until it is connected again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
}

/// A remote as `remotes.json` has it, of now or from before a computer
/// had more than one way, when its one way was its own.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Kept {
    id: String,
    name: String,
    ways: Vec<Way>,
    using: Option<Way>,
    host: String,
    user: String,
    port: u16,
    off: bool,
}

impl From<Kept> for Remote {
    fn from(k: Kept) -> Remote {
        let mut ways = k.ways;
        if ways.is_empty() && !k.host.is_empty() {
            ways.push(Way { host: k.host, user: k.user, port: k.port });
        }
        Remote { id: k.id, name: k.name, ways, using: k.using, off: k.off }
    }
}

impl Remote {
    /// The way it is reached by: the one the person connected it by, else
    /// the one it last answered by, else its first.
    pub fn way(&self) -> Way {
        self.using
            .clone()
            .filter(|w| self.ways.contains(w))
            .or_else(|| probed(&self.id).and_then(|p| p.way).filter(|w| self.ways.contains(w)))
            .or_else(|| self.ways.first().cloned())
            .unwrap_or_default()
    }

    /// The address of the way it is reached by.
    pub fn address(&self) -> String {
        self.way().address()
    }
}

fn remotes_file() -> PathBuf {
    paths::state_dir().join("remotes.json")
}

static LOCK: Mutex<()> = Mutex::new(());

/// How long a file read is trusted before the file is looked at again.
/// The window asks for the remotes many times a frame, a session row
/// at a time; what this process writes is in the cache at once (`save`,
/// `keep_probe`), and only another process's write waits this long.
const LOOK_AGAIN: Duration = Duration::from_millis(500);

type Stamp = (u64, std::time::SystemTime);

fn stamp_of(path: &Path) -> Option<Stamp> {
    std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)))
}

/// The remotes as last read: the file, its stamp, when it was looked at.
struct KeptList {
    path: PathBuf,
    stamp: Option<Stamp>,
    looked: Instant,
    list: Arc<Vec<Remote>>,
}

static KEPT: Mutex<Option<KeptList>> = Mutex::new(None);

/// The remotes, shared: read again only when the file changed, and looked
/// at no more than every `LOOK_AGAIN`.
fn kept() -> Arc<Vec<Remote>> {
    let path = remotes_file();
    let mut cache = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = cache.as_mut().filter(|c| c.path == path) {
        if c.looked.elapsed() < LOOK_AGAIN {
            return Arc::clone(&c.list);
        }
        let stamp = stamp_of(&path);
        if stamp == c.stamp {
            c.looked = Instant::now();
            return Arc::clone(&c.list);
        }
    }
    let stamp = stamp_of(&path);
    let list: Vec<Remote> = stamp.and_then(|_| paths::read_json(&path)).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let list = Arc::new(list);
    *cache = Some(KeptList { path, stamp, looked: Instant::now(), list: Arc::clone(&list) });
    list
}

/// The remotes, in the order they were added.
pub fn remotes() -> Vec<Remote> {
    kept().as_ref().clone()
}

pub fn find(id: &str) -> Option<Remote> {
    kept().iter().find(|d| d.id == id).cloned()
}

fn save(list: &[Remote]) -> std::io::Result<()> {
    let path = remotes_file();
    paths::write_json(&path, &serde_json::to_value(list).unwrap_or_default())?;
    *KEPT.lock().unwrap_or_else(|e| e.into_inner()) = Some(KeptList { stamp: stamp_of(&path), path, looked: Instant::now(), list: Arc::new(list.to_vec()) });
    Ok(())
}

/// The remote an address names, before it is kept: the one kept with
/// that address, or a new one with an id of its own made from its address,
/// the same for the same address again, so a remote removed and added back
/// finds its copied sessions. Either way it is reached by that address
/// alone. Nothing is written: a remote is kept only once it answers
/// (`settle`).
pub fn draft(host: &str, user: &str, port: u16) -> Remote {
    let list = remotes();
    let host = host.trim().to_string();
    let user = user.trim().to_string();
    let way = Way { host: host.clone(), user, port };
    // Typed with no account, it is the way kept with the account that
    // answered then (`settle`).
    let same = |w: &Way| w.host == way.host && w.port == way.port && (way.user.is_empty() || w.user == way.user);
    if let Some((d, w)) = list.iter().find_map(|d| d.ways.iter().find(|w| same(w)).map(|w| (d, w.clone()))) {
        return Remote { ways: vec![w.clone()], using: Some(w), ..d.clone() };
    }
    let base = {
        let from = if host.parse::<std::net::IpAddr>().is_ok() { host.as_str() } else { host.split('.').next().unwrap_or(&host) };
        let s = paths::slugify(from, 32);
        if s.is_empty() { "computer".to_string() } else { s }
    };
    let mut id = base.clone();
    let mut n = 2;
    while list.iter().any(|d| d.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    Remote { id, name: host, ways: vec![way.clone()], using: Some(way), off: false }
}

/// Keep a remote (`keep_in`).
pub fn keep(d: &Remote) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    keep_in(&mut list, d);
    save(&list)
}

/// Keep a remote with its ways. One kept already takes the ways beside
/// its own and keeps its name; disconnected, it is connected by the way
/// it is given, and connected by another way, it stays on that one: a
/// way is changed only by disconnecting first.
fn keep_in(list: &mut Vec<Remote>, d: &Remote) {
    match list.iter_mut().find(|k| k.id == d.id) {
        Some(k) => {
            for w in &d.ways {
                if !k.ways.contains(w) {
                    k.ways.push(w.clone());
                }
            }
            if k.off && !d.off {
                k.off = false;
                k.using = d.using.clone();
            }
        }
        None => list.push(d.clone()),
    }
}

/// Keep a remote that just answered with `p` (`keep_in`). When a computer
/// kept already is the one it reached, the way is kept as another way to
/// that one (`join`). The remote it is kept as.
pub fn settle(d: &Remote, p: &Probe) -> std::io::Result<Remote> {
    let mut d = d.clone();
    let mut p = p.clone();
    // A way typed with no account is kept with the account that answered,
    // so every way says whom it signs in as, and goes on doing so from a
    // computer whose own account is named otherwise.
    if !p.user.is_empty() {
        for w in d.ways.iter_mut().chain(d.using.iter_mut()).chain(p.way.iter_mut()) {
            if w.user.is_empty() {
                w.user = p.user.clone();
            }
        }
    }
    let (d, p) = (&d, &p);
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    let new = !list.iter().any(|k| k.id == d.id);
    keep_in(&mut list, d);
    let id = join(&mut list, &d.id, p, false).map(|(id, ..)| id).or_else(|| if new { revive(&mut list, &d.id, p) } else { None }).unwrap_or_else(|| d.id.clone());
    save(&list)?;
    if new && id != d.id {
        // What its probe left under an id never kept.
        let _ = std::fs::remove_file(probe_file(&d.id));
        let _ = std::fs::remove_dir(mirror_dir(&d.id));
        if let Some(m) = PROBES.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            m.remove(&d.id);
        }
    }
    Ok(list.into_iter().find(|k| k.id == id).unwrap_or_else(|| d.clone()))
}

/// A computer saved again after it was removed, by whatever way: the new
/// remote `id` takes the removed one's id, so its copy and its sessions
/// are found again, and the name it had. Known by the answer its copy
/// kept (`same_machine`). The id it takes.
fn revive(list: &mut [Remote], id: &str, p: &Probe) -> Option<String> {
    let kept: Vec<String> = list.iter().map(|k| k.id.clone()).collect();
    let mut old: Vec<String> = std::fs::read_dir(mirror_root()).ok()?.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| !kept.contains(n)).collect();
    old.sort();
    let was = old.into_iter().find(|n| probed(n).is_some_and(|q| same_machine(&q, p)))?;
    let k = list.iter_mut().find(|k| k.id == id)?;
    k.id = was.clone();
    if let Some(name) = std::fs::read_to_string(name_file(&was)).ok().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
        k.name = name;
    }
    keep_probe(&was, p);
    Some(was)
}

/// Make one of a computer kept twice, each by a way of its own, now that
/// `id` answered with `p`: the one kept first takes the other's ways and
/// stays, connected by the way that answered, and the other goes, its copy
/// left on disk unread. The id that stays, when there were two, and the
/// one that went with where it stood.
pub fn join_same(id: &str, p: &Probe) -> Option<(String, usize, Remote)> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    let joined = join(&mut list, id, p, true)?;
    save(&list).ok()?;
    Some(joined)
}

/// `switch`: the one that stays is connected by the way `p` came by even
/// when it was connected by another (`join_same`); otherwise only when it
/// was disconnected (`settle`, as `keep_in`).
fn join(list: &mut Vec<Remote>, id: &str, p: &Probe, switch: bool) -> Option<(String, usize, Remote)> {
    let me = list.iter().position(|k| k.id == id)?;
    let other = list.iter().position(|k| k.id != id && probed(&k.id).is_some_and(|q| same_machine(&q, p)))?;
    let at = me.max(other);
    let gone = list.remove(at);
    let left = gone.clone();
    let kept = &mut list[me.min(other)];
    for w in gone.ways {
        if !kept.ways.contains(&w) {
            kept.ways.push(w);
        }
    }
    if switch || kept.off {
        kept.using = p.way.clone();
    }
    kept.off = kept.off && gone.off;
    keep_probe(&kept.id, p);
    Some((kept.id.clone(), at, left))
}

/// A kept way with no account takes the account that answered by it
/// (`settle`), and is the way in use, having answered. Whether it changed.
pub fn fill_account(id: &str, p: &Probe) -> bool {
    let Some(way) = p.way.clone().filter(|w| w.user.is_empty() && !p.user.is_empty()) else { return false };
    let full = Way { user: p.user.clone(), ..way.clone() };
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    let Some(d) = list.iter_mut().find(|d| d.id == id && d.ways.contains(&way)) else { return false };
    if d.ways.contains(&full) {
        d.ways.retain(|w| *w != way);
    } else {
        for w in d.ways.iter_mut().filter(|w| **w == way) {
            *w = full.clone();
        }
    }
    if d.using.is_none() || d.using.as_ref() == Some(&way) {
        d.using = Some(full.clone());
    }
    let mut p = p.clone();
    p.way = Some(full);
    keep_probe(id, &p);
    save(&list).is_ok()
}

/// Whether two answers came from one computer: its own id, as the system
/// gives it, the same. An answer from before the id was asked is matched
/// by the computer's name, home and system.
pub fn same_machine(a: &Probe, b: &Probe) -> bool {
    match (&a.machine, &b.machine) {
        (Some(x), Some(y)) => !x.is_empty() && x == y,
        _ => !a.hostname.is_empty() && a.hostname == b.hostname && a.home == b.home && a.os == b.os,
    }
}

/// Keep the remote an address names (`draft`, `keep`), named `name` when
/// one is given.
pub fn add(host: &str, user: &str, port: u16, name: &str) -> std::io::Result<Remote> {
    let mut d = draft(host, user, port);
    if !name.trim().is_empty() {
        d.name = name.trim().to_string();
    }
    keep(&d)?;
    Ok(d)
}

/// Disconnect a remote, or connect it again by the way it had
/// (`Remote::off`).
pub fn set_off(id: &str, off: bool) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    if let Some(d) = list.iter_mut().find(|d| d.id == id) {
        d.off = off;
    }
    save(&list)
}

/// Connect a remote by one of its ways.
pub fn connect_by(id: &str, way: &Way) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    if let Some(d) = list.iter_mut().find(|d| d.id == id && d.ways.contains(way)) {
        d.using = Some(way.clone());
        d.off = false;
    }
    save(&list)
}

/// Forget one way to a remote; with none left, the remote (`remove`).
pub fn remove_way(id: &str, way: &Way) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    if let Some(d) = list.iter_mut().find(|d| d.id == id) {
        d.ways.retain(|w| w != way);
        if d.using.as_ref() == Some(way) {
            d.using = None;
        }
    }
    let (gone, list): (Vec<Remote>, Vec<Remote>) = list.into_iter().partition(|d| d.ways.is_empty());
    gone.iter().for_each(remember);
    save(&list)
}

/// Whether a remote is kept and connected: not disconnected by the person.
pub fn is_on(id: &str) -> bool {
    kept().iter().any(|d| d.id == id && !d.off)
}

/// Forget a remote. Its copied sessions stay on disk, unread, and come
/// back with it.
pub fn remove(id: &str) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (gone, list): (Vec<Remote>, Vec<Remote>) = remotes().into_iter().partition(|d| d.id == id);
    gone.iter().for_each(remember);
    save(&list)
}

/// A remote removed is remembered beside its copy, which stays: the name
/// it had, so the same computer saved again by any way comes back as it
/// was (`revive`).
fn remember(d: &Remote) {
    let _ = std::fs::create_dir_all(mirror_dir(&d.id));
    let _ = std::fs::write(name_file(&d.id), &d.name);
}

fn name_file(id: &str) -> PathBuf {
    mirror_dir(id).join("name")
}

/// Rename a remote in the window.
pub fn rename(id: &str, name: &str) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    if let Some(d) = list.iter_mut().find(|d| d.id == id) {
        d.name = name.trim().to_string();
    }
    save(&list)
}

// ------------------------------------------------------------ folders

/// A remote's folder as a session's folder is written.
pub fn folder(remote: &str, path: &str) -> String {
    format!("{SCHEME}{remote}{path}")
}

/// The remote and the path of a remote's folder; none for a folder here.
pub fn split(folder: &str) -> Option<(&str, &str)> {
    let rest = folder.strip_prefix(SCHEME)?;
    match rest.find('/') {
        Some(i) => Some((&rest[..i], &rest[i..])),
        None => Some((rest, "")),
    }
}

pub fn is_remote(folder: &str) -> bool {
    folder.starts_with(SCHEME)
}

/// The path alone, the remote's or this computer's.
pub fn path_of(folder: &str) -> &str {
    split(folder).map(|(_, p)| p).unwrap_or(folder)
}

/// Where a remote's sessions are copied.
pub fn mirror_dir(remote: &str) -> PathBuf {
    paths::root().join("remote").join(paths::safe_component(remote))
}

pub fn mirror_root() -> PathBuf {
    paths::root().join("remote")
}

/// Claude Code's projects as copied from a remote.
pub fn claude_projects(remote: &str) -> PathBuf {
    mirror_dir(remote).join(CLAUDE_PROJECTS)
}

/// Codex's home as copied from a remote.
pub fn codex_home(remote: &str) -> PathBuf {
    mirror_dir(remote).join(CODEX)
}

/// The remote a copied file is of, by where it is.
pub fn remote_of_path(path: &Path) -> Option<String> {
    let rest = path.strip_prefix(mirror_root()).ok()?;
    rest.components().next().map(|c| c.as_os_str().to_string_lossy().to_string())
}

// ------------------------------------------------------------ ssh

/// A string as one word of a POSIX shell.
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:@%+=,".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// The socket a remote's connections share, so each command after the
/// first starts in milliseconds. Short: a socket's path is limited.
#[cfg(unix)]
fn control_path() -> String {
    let dir = paths::run_dir().join("ssh");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("%C").to_string_lossy().to_string();
    if p.len() < 60 {
        p
    } else {
        "/tmp/emaki-ssh-%C".to_string()
    }
}

/// ssh to a remote, with what every connection here asks: no question
/// typed (`BatchMode`; a key signs in), a new remote's key taken the
/// first time and checked after (`accept-new`, shown in Settings), a
/// dead link noticed, and on Unix one connection shared.
pub fn ssh(d: &Remote, tty: bool) -> Command {
    ssh_at(&d.way(), tty)
}

/// `ssh` by one way to a computer.
fn ssh_at(d: &Way, tty: bool) -> Command {
    let mut cmd = crate::process::command("ssh");
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "StrictHostKeyChecking=accept-new", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=3", "-o", "Compression=yes", "-o", "LogLevel=ERROR"]);
    // A terminal has a connection of its own: one that goes through the
    // shared connection never tells the remote of a new size, and a
    // program there kept drawing for the size it began at.
    #[cfg(unix)]
    if !tty {
        cmd.args(["-o", "ControlMaster=auto", "-o", &format!("ControlPath={}", control_path()), "-o", "ControlPersist=600"]);
    }
    cmd.arg(if tty { "-tt" } else { "-T" });
    if d.port != 0 {
        cmd.args(["-p", &d.port.to_string()]);
    }
    if !d.user.is_empty() {
        cmd.args(["-l", &d.user]);
    }
    cmd.arg(&d.host).arg("--");
    // Locale and terminal are sent as the system's ssh sends them.
    cmd
}

/// ssh running `script` in a POSIX shell there, whatever the account's
/// own shell is.
pub fn command(d: &Remote, script: &str, tty: bool) -> Command {
    command_at(&d.way(), script, tty)
}

fn command_at(d: &Way, script: &str, tty: bool) -> Command {
    let mut cmd = ssh_at(d, tty);
    cmd.arg(format!("exec /bin/sh -c {}", sh_quote(script)));
    cmd
}

/// Run a script there and take what it printed, within `timeout`.
pub fn run(d: &Remote, script: &str, timeout: Duration) -> Result<String, String> {
    run_at(&d.way(), script, timeout)
}

fn run_at(d: &Way, script: &str, timeout: Duration) -> Result<String, String> {
    let mut cmd = command_at(d, script, false);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
    let out = wait_output(child, timeout)?;
    if out.0 == Some(0) {
        Ok(out.1)
    } else {
        Err(ssh_error(&out.2, out.0))
    }
}

/// A child's exit code, stdout and stderr, killed past `timeout`.
pub(crate) fn wait_output(mut child: Child, timeout: Duration) -> Result<(Option<i32>, String, String), String> {
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = stdout.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let deadline = Instant::now() + timeout;
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("it did not answer in time".into());
            }
        }
    };
    Ok((code, out_t.join().unwrap_or_default(), err_t.join().unwrap_or_default()))
}

/// What went wrong, in a line: ssh's own words where it said any.
pub fn ssh_error(stderr: &str, code: Option<i32>) -> String {
    let line = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("");
    let lower = stderr.to_lowercase();
    if lower.contains("permission denied") {
        return "permission denied".into();
    }
    if lower.contains("host key verification failed") || lower.contains("remote host identification has changed") {
        return "its key is not the one this computer knows".into();
    }
    if lower.contains("could not resolve hostname") {
        return "no computer has that name".into();
    }
    if lower.contains("connection refused") {
        return "it refused the connection: is Remote Login on?".into();
    }
    if lower.contains("timed out") || lower.contains("no route to host") || lower.contains("network is unreachable") {
        return "it could not be reached".into();
    }
    if !line.is_empty() {
        return line.chars().take(200).collect();
    }
    match code {
        Some(c) => format!("ssh ended with {c}"),
        None => "ssh was stopped".into(),
    }
}

// ------------------------------------------------------------ the probe

/// What a remote said of itself when last reached.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Probe {
    /// The login shell's `PATH`: an agent installed for the account is
    /// found where a terminal there finds it.
    pub path: String,
    pub home: String,
    /// `Darwin`, `Linux`.
    pub os: String,
    pub arch: String,
    pub hostname: String,
    pub claude: String,
    pub claude_version: String,
    pub codex: String,
    pub codex_version: String,
    /// Whether Claude Code there is signed in, as it says over ssh (`claude
    /// auth status`); `None` when it did not say. A Mac's keychain is
    /// locked to an ssh session, so one signed in at its own screen can
    /// say no here (`login`).
    #[serde(default)]
    pub claude_signed_in: Option<bool>,
    /// The same of Codex (`codex login status`).
    #[serde(default)]
    pub codex_signed_in: Option<bool>,
    /// The computer's own id, as its system gives it (a Mac's hardware
    /// UUID, Linux's machine id): the same by every way to it, so one
    /// computer is kept once (`same_machine`). `None` from before it was
    /// asked, empty when the system gave none.
    #[serde(default)]
    pub machine: Option<String>,
    /// The way it answered by.
    #[serde(default)]
    pub way: Option<Way>,
    /// What the computer is called by its person: a Mac's Computer Name
    /// (System Settings, General, About); empty elsewhere. `None` from
    /// before it was asked.
    #[serde(default)]
    pub computer: Option<String>,
    /// The account that answered.
    #[serde(default)]
    pub user: String,
}

impl Probe {
    /// The name a computer is kept under when it is first saved: its
    /// person's name for it, else its host's name without the network's
    /// part.
    pub fn name(&self) -> String {
        match self.computer.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(c) => c.to_string(),
            None => host_name(&self.hostname),
        }
    }
}

/// A host's name without the network's part: `pingfan-mba.local` is
/// `pingfan-mba`.
fn host_name(hostname: &str) -> String {
    hostname.trim_end_matches(".local").split('.').next().unwrap_or("").to_string()
}

impl Probe {
    pub fn has(&self, agent: crate::model::AgentId) -> bool {
        match agent {
            crate::model::AgentId::ClaudeCode => !self.claude.is_empty(),
            crate::model::AgentId::Codex => !self.codex.is_empty(),
        }
    }

    /// Whether the agent there is signed in, as it last said; `None` when
    /// it did not say.
    pub fn signed_in(&self, agent: crate::model::AgentId) -> Option<bool> {
        match agent {
            crate::model::AgentId::ClaudeCode => self.claude_signed_in,
            crate::model::AgentId::Codex => self.codex_signed_in,
        }
    }
}

fn probe_file(remote: &str) -> PathBuf {
    mirror_dir(remote).join("probe.json")
}

/// What each remote said when last reached, as `probed` last read it:
/// the file, its stamp, when it was looked at.
type KeptProbe = (PathBuf, Option<Stamp>, Instant, Option<Probe>);
static PROBES: Mutex<Option<HashMap<String, KeptProbe>>> = Mutex::new(None);

/// What the remote said when last reached, kept between launches. Read
/// again only when the file changed, looked at no more than every
/// `LOOK_AGAIN`: the window asks a session row at a time.
pub fn probed(remote: &str) -> Option<Probe> {
    let path = probe_file(remote);
    let mut cache = PROBES.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some((at, stamp, looked, p)) = map.get_mut(remote).filter(|(at, ..)| *at == path) {
        if looked.elapsed() < LOOK_AGAIN {
            return p.clone();
        }
        let now = stamp_of(at);
        if now == *stamp {
            *looked = Instant::now();
            return p.clone();
        }
    }
    let stamp = stamp_of(&path);
    let p: Option<Probe> = stamp.and_then(|_| paths::read_json(&path)).and_then(|v| serde_json::from_value(v).ok());
    map.insert(remote.to_string(), (path, stamp, Instant::now(), p.clone()));
    p
}

/// Keep what a remote said, on disk and in `probed`'s cache.
fn keep_probe(remote: &str, p: &Probe) {
    let path = probe_file(remote);
    let _ = std::fs::create_dir_all(mirror_dir(remote));
    let _ = paths::write_json(&path, &serde_json::to_value(p).unwrap_or_default());
    PROBES.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(remote.to_string(), (path.clone(), stamp_of(&path), Instant::now(), Some(p.clone())));
}

/// The script that asks a remote what it has. The login shell is asked
/// for its `PATH` as a terminal there has it (interactive, so a `.zshrc`
/// that adds `~/.local/bin` counts), and the usual places an installer
/// puts an agent are added behind it.
const PROBE: &str = r#"
P=$("${SHELL:-/bin/sh}" -lic 'echo "__EMAKI__$PATH"' </dev/null 2>/dev/null | sed -n 's/^.*__EMAKI__//p' | tail -n 1)
case "$P" in *:*) ;; *\ *) P=$(printf %s "$P" | tr ' ' ':') ;; esac
[ -n "$P" ] || P="$PATH"
for d in "$HOME/.local/bin" "$HOME/.claude/local" "$HOME/.npm-global/bin" /opt/homebrew/bin /usr/local/bin; do
  case ":$P:" in *":$d:"*) ;; *) P="$P:$d" ;; esac
done
PATH="$P"; export PATH
echo "path=$P"
echo "home=$HOME"
echo "os=$(uname -s)"
echo "arch=$(uname -m)"
echo "hostname=$(hostname 2>/dev/null)"
m=$(ioreg -rd1 -c IOPlatformExpertDevice 2>/dev/null | sed -n 's/.*"IOPlatformUUID" = "\(.*\)".*/\1/p')
[ -n "$m" ] || m=$(cat /etc/machine-id /var/lib/dbus/machine-id 2>/dev/null | head -n 1)
echo "machine=$m"
echo "computer=$(scutil --get ComputerName 2>/dev/null)"
echo "user=$(id -un 2>/dev/null)"
c=$(command -v claude 2>/dev/null); echo "claude=$c"
[ -n "$c" ] && echo "claude_version=$(claude --version 2>/dev/null | head -n 1)"
x=$(command -v codex 2>/dev/null); echo "codex=$x"
[ -n "$x" ] && echo "codex_version=$(codex --version 2>/dev/null | head -n 1)"
"#;

/// What the agents there say of their sign-in, with `$c` and `$x` the
/// paths of Claude Code and Codex. Codex says it on stderr.
const AUTH: &str = r#"
[ -n "$c" ] && echo "claude_auth=$(claude auth status 2>/dev/null | tr -d ' \t\n\r')"
[ -n "$x" ] && echo "codex_auth=$(codex login status 2>&1 | head -n 1)"
echo "end=1"
"#;

/// Ask a remote what it has, and keep the answer.
pub fn probe(d: &Remote) -> Result<Probe, String> {
    let way = reach(d)?;
    // The probe, its login shell started, may take longer than `REACH`.
    let out = run_at(&way, &format!("{PROBE}{AUTH}"), Duration::from_secs(30))?;
    let mut p = Probe { way: Some(way), ..Probe::default() };
    let mut ended = false;
    for line in out.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().to_string();
        match k {
            "path" => p.path = v,
            "home" => p.home = v,
            "os" => p.os = v,
            "arch" => p.arch = v,
            "hostname" => p.hostname = v,
            "machine" => p.machine = Some(v),
            "computer" => p.computer = Some(v),
            "user" => p.user = v,
            "claude" => p.claude = v,
            "claude_version" => p.claude_version = v.split_whitespace().next().unwrap_or("").to_string(),
            "codex" => p.codex = v,
            "codex_version" => p.codex_version = v.split_whitespace().last().unwrap_or("").to_string(),
            "claude_auth" => p.claude_signed_in = signed_in_from(&v),
            "codex_auth" => p.codex_signed_in = codex_signed_in_from(&v),
            "end" => ended = true,
            _ => {}
        }
    }
    if !ended || p.home.is_empty() {
        return Err("it answered, but not as a Mac or Linux does".into());
    }
    // Kept before its person's name for it was asked, under its host's
    // name, it takes the person's name now, once.
    if let Some(was) = probed(&d.id).filter(|o| o.computer.is_none()) {
        if find(&d.id).is_some_and(|k| k.name == host_name(&was.hostname)) && p.name() != host_name(&was.hostname) {
            let _ = rename(&d.id, &p.name());
        }
    }
    keep_probe(&d.id, &p);
    Ok(p)
}

/// The way a remote is connected by, when it answers within `REACH`. A
/// computer asleep, or one that took the connection and says nothing,
/// fails in a few seconds rather than being waited on: anything at all
/// and it is there.
pub fn reach(d: &Remote) -> Result<Way, String> {
    let way = d.way();
    if way.host.is_empty() {
        return Err("it has no address".into());
    }
    run_at(&way, "echo emaki", REACH)?;
    Ok(way)
}

/// The command that starts `program` with `args` in a remote's folder,
/// on the remote: its login `PATH`, `env` set, in the folder or failing
/// with the folder named. `tty` gives it a terminal there.
pub fn agent_command(folder: &str, program: &str, args: &[String], env: &[(&str, &str)], tty: bool) -> Result<Command, String> {
    let (d, script) = agent_script(folder, program, args, env, Start::Exec)?;
    Ok(command(&d, &script, tty))
}

/// An interactive Claude Code in a remote's folder, on a terminal there,
/// as the argv of the `ssh -tt` that starts it: `args` are Claude Code's
/// own. Before it becomes Claude Code the script names its pid
/// (`pty::ANNOUNCE`), which Claude Code keeps, so its record in the
/// registry there is told from any other.
pub fn terminal_argv(folder: &str, args: &[String]) -> Result<Vec<String>, String> {
    let (d, script) = agent_script(folder, "claude", args, &[("COLORTERM", "truecolor")], Start::Announced)?;
    let cmd = command(&d, &script, true);
    let mut argv = vec![cmd.get_program().to_string_lossy().to_string()];
    argv.extend(cmd.get_args().map(|a| a.to_string_lossy().to_string()));
    Ok(argv)
}

/// How a script there starts its program.
#[derive(Clone, Copy, PartialEq)]
enum Start {
    /// The script becomes the program.
    Exec,
    /// Its pid named first (`pty::ANNOUNCE`), then the script becomes it.
    Announced,
    /// With no terminal there a program does not hear its ssh end, and
    /// one that waits (a sign-in) would wait on after this computer let
    /// it go, cancelled or quit. So the script stays: it hands the
    /// program each line it is sent, through a pipe of its own, and when
    /// what it is sent ends, the connection gone for whatever reason, it
    /// ends the program. It ends with the program, as the program ended.
    Watched,
}

/// The watcher round a program (`Start::Watched`); `{cmd}` is the
/// program's own command. A job in the background reads nothing by
/// itself (its input is `/dev/null`), so the relay reads what the script
/// is sent from a copy kept first (4). The program is ended as well when
/// the script itself is (a hangup, a pipe gone), and killed outright if
/// it is still there two seconds after it was asked to end: a sign-in
/// left behind once outlived its script.
const WATCHED: &str = r#"exec 4<&0
f="${TMPDIR:-/tmp}/emaki-in-$$"; rm -f "$f"; mkfifo "$f" || exit 98
{cmd} <"$f" & p=$!
exec 3>"$f"; rm -f "$f"
end() { kill "$p" 2>/dev/null; ( sleep 2; kill -9 "$p" 2>/dev/null ) & }
trap 'end; exit 129' HUP TERM PIPE
( while IFS= read -r l; do printf '%s\n' "$l" >&3; done; end ) <&4 &
r=$!
wait "$p"; s=$?
kill "$r" 2>/dev/null
exit "$s""#;

fn agent_script(folder: &str, program: &str, args: &[String], env: &[(&str, &str)], start: Start) -> Result<(Remote, String), String> {
    let (id, path) = split(folder).ok_or_else(|| "not a remote computer's folder".to_string())?;
    let d = find(id).ok_or_else(|| format!("{id} is not one of the devices in Settings, Connected Devices"))?;
    let probe = match probed(id) {
        Some(p) => p,
        None => probe(&d)?,
    };
    let mut script = String::new();
    if !probe.path.is_empty() {
        script.push_str(&format!("PATH={}; export PATH; ", sh_quote(&probe.path)));
    }
    if !path.is_empty() {
        script.push_str(&format!("cd {} || {{ echo {} >&2; exit 97; }}; ", sh_quote(path), sh_quote(&format!("the folder is not there: {path}"))));
    }
    if start == Start::Announced {
        script.push_str(&format!("printf '{}' \"$$\"; ", crate::pty::ANNOUNCE));
    }
    let mut cmd = String::new();
    if !env.is_empty() {
        cmd.push_str("env ");
        for (k, v) in env {
            cmd.push_str(&format!("{k}={} ", sh_quote(v)));
        }
    }
    cmd.push_str(&sh_quote(program));
    for a in args {
        cmd.push(' ');
        cmd.push_str(&sh_quote(a));
    }
    match start {
        Start::Watched => script.push_str(&format!("\n{}", WATCHED.replace("{cmd}", &cmd))),
        _ => script.push_str(&format!("exec {cmd}")),
    }
    Ok((d, script))
}

/// Whether `claude auth status` said it is signed in: its JSON, spaces
/// taken out.
fn signed_in_from(said: &str) -> Option<bool> {
    if said.contains("\"loggedIn\":true") {
        Some(true)
    } else if said.contains("\"loggedIn\":false") {
        Some(false)
    } else {
        None
    }
}

/// Whether `codex login status` said it is signed in: its first line.
fn codex_signed_in_from(said: &str) -> Option<bool> {
    if said.starts_with("Not logged in") {
        Some(false)
    } else if said.starts_with("Logged in") {
        Some(true)
    } else {
        None
    }
}

/// Ask the agents there again whether they are signed in, and keep the
/// answers with the rest of what the remote said. Off the window's
/// thread.
pub fn check_sign_ins(remote: &str) -> Result<Probe, String> {
    let d = find(remote).ok_or_else(|| format!("{remote} is not one of the devices in Settings, Connected Devices"))?;
    let mut p = match probed(remote) {
        Some(p) if !p.path.is_empty() => p,
        _ => return probe(&d),
    };
    let script = format!("PATH={}; export PATH; c=$(command -v claude 2>/dev/null); x=$(command -v codex 2>/dev/null)\n{AUTH}", sh_quote(&p.path));
    let out = run(&d, &script, Duration::from_secs(30))?;
    let (mut claude, mut codex) = (None, None);
    for line in out.lines() {
        match line.split_once('=') {
            Some(("claude_auth", v)) => claude = signed_in_from(v.trim()),
            Some(("codex_auth", v)) => codex = codex_signed_in_from(v.trim()),
            _ => {}
        }
    }
    (p.claude_signed_in, p.codex_signed_in) = (claude, codex);
    keep_probe(remote, &p);
    Ok(p)
}

/// An agent's own sign-in on a remote, done from here, with no terminal
/// there and nothing done at its screen. Claude Code's `claude auth
/// login` prints the page to sign in at and waits for the code that page
/// shows, typed in the window (`finish`); it keeps the sign-in where it
/// can: a Mac's keychain is locked to ssh, and then it writes
/// `~/.claude/.credentials.json`, which every later ssh session reads.
/// Codex's `codex login --device-auth` prints a page and a code to type
/// there, and ends once the page has it (`wait`).
pub struct Login {
    child: Mutex<Child>,
    stdin: Mutex<Option<std::process::ChildStdin>>,
    said: Arc<Mutex<String>>,
    agent: crate::model::AgentId,
}

/// Where a sign-in begun says to go: the page, and for Codex the code to
/// type on it.
pub struct LoginPage {
    pub url: String,
    pub code: Option<String>,
}

impl Login {
    /// Start it, and wait for the page's address (and Codex's code).
    /// `BROWSER` is a program that does nothing, so no browser opens at
    /// the remote's screen.
    pub fn start(remote: &str, agent: crate::model::AgentId) -> Result<(Login, LoginPage), String> {
        let codex = agent == crate::model::AgentId::Codex;
        let (program, args): (&str, Vec<String>) = if codex { ("codex", vec!["login".into(), "--device-auth".into()]) } else { ("claude", vec!["auth".into(), "login".into()]) };
        let name = agent.display_name();
        // Watched there: a sign-in cancelled here, or left by a window
        // closed or an app quit, ends there with its connection.
        let (d, script) = agent_script(&folder(remote, ""), program, &args, &[("BROWSER", "true")], Start::Watched)?;
        let mut cmd = command(&d, &script, false);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
        let stdin = child.stdin.take();
        let said = Arc::new(Mutex::new(String::new()));
        for pipe in [child.stdout.take().map(|o| Box::new(o) as Box<dyn Read + Send>), child.stderr.take().map(|e| Box::new(e) as Box<dyn Read + Send>)].into_iter().flatten() {
            let said = Arc::clone(&said);
            std::thread::spawn(move || {
                let mut pipe = pipe;
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    said.lock().unwrap_or_else(|e| e.into_inner()).push_str(&String::from_utf8_lossy(&buf[..n]));
                }
            });
        }
        let login = Login { child: Mutex::new(child), stdin: Mutex::new(stdin), said, agent };
        let began = Instant::now();
        loop {
            let said = login.said();
            if let Some(url) = login_url(&said) {
                let code = device_code(&said);
                if !codex || code.is_some() {
                    return Ok((login, LoginPage { url, code }));
                }
            }
            if let Ok(Some(_)) = login.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
                return Err(last_words(&login.said()).unwrap_or_else(|| format!("{name} there did not start its sign-in")));
            }
            if began.elapsed() > Duration::from_secs(40) {
                login.cancel();
                return Err(format!("{name} there did not answer in time"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn said(&self) -> String {
        self.said.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Hand it the code the page showed, and wait for it to end: `Ok`
    /// when it ended well, else its own last words.
    pub fn finish(&self, code: &str) -> Result<(), String> {
        let before = self.said().len();
        {
            let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
            let pipe = stdin.as_mut().ok_or("the sign-in has ended")?;
            pipe.write_all(format!("{}\n", code.trim()).as_bytes()).map_err(|e| e.to_string())?;
            let _ = pipe.flush();
        }
        let began = Instant::now();
        // A code it does not take, Claude Code says so and asks again,
        // still running: a line said after the code, with no end in a few
        // seconds, is why.
        let mut answered: Option<Instant> = None;
        loop {
            if let Ok(Some(st)) = self.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
                let after = self.said();
                let tail = after.get(before..).unwrap_or("");
                return if st.success() { Ok(()) } else { Err(last_words(tail).or_else(|| last_words(&after)).unwrap_or_else(|| "the sign-in did not go through".into())) };
            }
            let tail = self.said().get(before..).unwrap_or("").to_string();
            if answered.is_none() && tail.contains('\n') && last_words(&tail).is_some() {
                answered = Some(Instant::now());
            }
            if answered.is_some_and(|at| at.elapsed() > Duration::from_secs(3)) {
                return Err(last_words(&tail).unwrap_or_else(|| "the sign-in did not go through".into()));
            }
            if began.elapsed() > Duration::from_secs(90) {
                self.cancel();
                return Err(format!("{} there did not answer in time", self.agent.display_name()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Wait for a sign-in that ends by itself, Codex's once its page has
    /// the code: `Ok` when it ended well, else its own last words. Its
    /// code holds for fifteen minutes.
    pub fn wait(&self) -> Result<(), String> {
        let began = Instant::now();
        loop {
            if let Ok(Some(st)) = self.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
                return if st.success() { Ok(()) } else { Err(last_words(&plain(&self.said())).unwrap_or_else(|| "the sign-in did not go through".into())) };
            }
            if began.elapsed() > Duration::from_secs(16 * 60) {
                self.cancel();
                return Err("the code ran out".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// End it: its connection, and with it the agent there, which its
    /// script ends when what it is sent ends (`Start::Watched`).
    pub fn cancel(&self) {
        self.stdin.lock().unwrap_or_else(|e| e.into_inner()).take();
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Login {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The page `claude auth login` says to sign in at.
fn login_url(said: &str) -> Option<String> {
    let at = said.find("https://")?;
    let rest = &said[at..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '\u{1b}' || c == '\u{7}').unwrap_or(rest.len());
    // Whole only once a line ends after it.
    (end < rest.len()).then(|| rest[..end].to_string())
}

/// What a program said, its colors taken out.
fn plain(said: &str) -> String {
    let mut out = String::with_capacity(said.len());
    let mut chars = said.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                // An operating system command (a title, a pid named), to
                // its bell.
                Some(']') => for c in chars.by_ref() {
                    if c == '\u{7}' {
                        break;
                    }
                },
                _ => {}
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The code `codex login --device-auth` says to type on its page: the
/// first word after "one-time code" made of capitals and digits with a
/// hyphen in it.
fn device_code(said: &str) -> Option<String> {
    let text = plain(said);
    let rest = &text[text.find("one-time code")?..];
    rest.split_whitespace().find(|w| w.len() >= 6 && w.contains('-') && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')).map(str::to_string)
}

/// The last line a program said that is not its prompt for the code.
fn last_words(said: &str) -> Option<String> {
    let said = plain(said);
    let line = said.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("Paste code")).last()?;
    let line = line.strip_prefix("Error:").unwrap_or(line).trim();
    Some(line.strip_prefix("Login failed:").unwrap_or(line).trim().to_string())
}

/// A file of this computer's put in the remote's temporary folder, for
/// an agent there to read: a picture pasted at its prompt, which names
/// a path there. Where it went, there. Off the window's thread.
pub fn put_file(remote: &str, file: &Path) -> Result<String, String> {
    let d = find(remote).ok_or_else(|| format!("{remote} is not one of the devices in Settings, Connected Devices"))?;
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let name: String = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().chars().map(|c| if c.is_ascii_alphanumeric() || ".-_".contains(c) { c } else { '-' }).collect();
    let script = format!("d=\"${{TMPDIR:-/tmp}}\"; d=\"${{d%/}}/emaki-pasted\"; mkdir -p \"$d\" && f=\"$d/$$-{name}\" && cat > \"$f\" && printf '%s\\n' \"$f\"");
    let mut cmd = command(&d, &script, false);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&bytes);
    });
    let out = wait_output(child, Duration::from_secs(60))?;
    let _ = writer.join();
    match out {
        (Some(0), said, _) if !said.trim().is_empty() => Ok(said.trim().to_string()),
        (code, _, err) => Err(ssh_error(&err, code)),
    }
}

/// A shell in a remote's folder, on a terminal: the account's own login
/// shell, as a terminal there opens.
pub fn shell_argv(folder: &str) -> Option<Vec<String>> {
    let (id, path) = split(folder)?;
    let d = find(id)?;
    // Its pid named first (`pty::ANNOUNCE`), which tells the terminal
    // here that ssh is up and the size can be said again.
    let script = format!("cd {} 2>/dev/null; printf '{}' \"$$\"; exec \"${{SHELL:-/bin/sh}}\" -l", sh_quote(if path.is_empty() { "." } else { path }), crate::pty::ANNOUNCE);
    let cmd = command(&d, &script, true);
    let mut argv = vec![cmd.get_program().to_string_lossy().to_string()];
    argv.extend(cmd.get_args().map(|a| a.to_string_lossy().to_string()));
    Some(argv)
}

/// Whether a session's folder can be gone to: one here that is there, or
/// one of a remote in Settings, which is taken to be until the remote
/// says otherwise. Cheap, for the window as it draws.
pub fn is_there(folder: &str) -> bool {
    if folder.is_empty() {
        return false;
    }
    match split(folder) {
        Some((id, _)) => kept().iter().any(|d| d.id == id),
        None => Path::new(folder).is_dir(),
    }
}

/// Whether a folder is there on its remote. Asked of the remote, so off
/// the window's thread.
pub fn folder_on_remote(folder: &str) -> Result<bool, String> {
    let (id, path) = split(folder).ok_or_else(|| "not a remote computer's folder".to_string())?;
    let d = find(id).ok_or_else(|| format!("{id} is not one of the devices in Settings, Connected Devices"))?;
    let out = run(&d, &format!("[ -d {} ] && echo yes || echo no", sh_quote(path)), Duration::from_secs(15))?;
    Ok(out.trim() == "yes")
}

/// The folders in a remote's folder, for choosing one to start in: their
/// names, hidden ones left out, sorted. `~` and an empty path are home.
pub fn list_folders(remote: &str, path: &str) -> Result<(String, Vec<String>), String> {
    let d = self::find(remote).ok_or_else(|| format!("{remote} is not one of the devices in Settings, Connected Devices"))?;
    let at = if path.is_empty() || path == "~" { "\"$HOME\"".to_string() } else { sh_quote(path) };
    let script = format!("cd {at} || exit 3; pwd; for f in * ; do [ -d \"$f\" ] && printf '%s\\n' \"$f\"; done; true");
    let out = run(&d, &script, Duration::from_secs(15))?;
    let mut lines = out.lines();
    let here = lines.next().unwrap_or("").to_string();
    let mut names: Vec<String> = lines.filter(|l| !l.is_empty() && *l != "*").map(str::to_string).collect();
    names.sort_by_key(|n| n.to_lowercase());
    Ok((here, names))
}

// ------------------------------------------------------------ keys

/// This computer's public key, made when it has none: what a remote is
/// told to let in.
pub fn public_key() -> Result<String, String> {
    // A key the agent holds (1Password's, Secretive's) is the one ssh
    // signs in with first.
    if let Ok(o) = crate::process::command("ssh-add").arg("-L").stdin(Stdio::null()).output() {
        let listed = String::from_utf8_lossy(&o.stdout);
        if let Some(k) = listed.lines().find(|l| l.starts_with("ssh-") || l.starts_with("ecdsa-")).filter(|_| o.status.success()) {
            return Ok(k.trim().to_string());
        }
    }
    let ssh_dir = paths::home().join(".ssh");
    for name in ["id_ed25519.pub", "id_ecdsa.pub", "id_rsa.pub"] {
        if let Ok(k) = std::fs::read_to_string(ssh_dir.join(name)) {
            if !k.trim().is_empty() {
                return Ok(k.trim().to_string());
            }
        }
    }
    let _ = std::fs::create_dir_all(&ssh_dir);
    let key = ssh_dir.join("id_ed25519");
    let out = crate::process::command("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "emaki", "-f"])
        .arg(&key)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not make a key: {e}"))?;
    if !out.status.success() {
        return Err(format!("could not make a key: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    std::fs::read_to_string(ssh_dir.join("id_ed25519.pub")).map(|k| k.trim().to_string()).map_err(|e| e.to_string())
}

/// Let this computer in to a remote with the account's password, once:
/// its key added to the account's `authorized_keys` there, as
/// `ssh-copy-id` does. The password goes to ssh alone (`SSH_ASKPASS`)
/// and is kept nowhere.
#[cfg(unix)]
pub fn install_key(d: &Remote, password: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let key = public_key()?;
    let ask = paths::run_dir().join("askpass.sh");
    std::fs::write(&ask, "#!/bin/sh\nprintf '%s\\n' \"$EMAKI_SSH_PASSWORD\"\n").map_err(|e| e.to_string())?;
    let _ = std::fs::set_permissions(&ask, std::fs::Permissions::from_mode(0o700));
    let script = format!(
        "umask 077; mkdir -p \"$HOME/.ssh\" && touch \"$HOME/.ssh/authorized_keys\" && {{ grep -qxF {k} \"$HOME/.ssh/authorized_keys\" || printf '%s\\n' {k} >> \"$HOME/.ssh/authorized_keys\"; }}",
        k = sh_quote(&key)
    );
    let mut cmd = crate::process::command("ssh");
    let d = d.way();
    cmd.args(["-o", "StrictHostKeyChecking=accept-new", "-o", "ConnectTimeout=5", "-o", "PubkeyAuthentication=no", "-o", "PreferredAuthentications=keyboard-interactive,password", "-o", "NumberOfPasswordPrompts=1", "-o", "ControlMaster=no", "-o", "LogLevel=ERROR", "-T"]);
    if d.port != 0 {
        cmd.args(["-p", &d.port.to_string()]);
    }
    if !d.user.is_empty() {
        cmd.args(["-l", &d.user]);
    }
    cmd.arg(&d.host).arg("--").arg(format!("exec /bin/sh -c {}", sh_quote(&script)));
    cmd.env("SSH_ASKPASS", &ask).env("SSH_ASKPASS_REQUIRE", "force").env("DISPLAY", ":0").env("EMAKI_SSH_PASSWORD", password);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
    let (code, _, err) = wait_output(child, Duration::from_secs(30))?;
    if code == Some(0) {
        Ok(())
    } else {
        Err(ssh_error(&err, code))
    }
}

#[cfg(not(unix))]
pub fn install_key(_d: &Remote, _password: &str) -> Result<(), String> {
    Err("add this computer's key to the other computer by hand".into())
}

// ------------------------------------------------------------ this computer

/// What another computer needs to reach this one.
#[derive(Debug, Clone, Default)]
pub struct Here {
    /// Whether something answers on port 22: Remote Login is on.
    pub listening: bool,
    pub computer: String,
    pub address: String,
    pub account: String,
    /// Its ssh server's key, as ssh shows it the first time.
    pub host_key: String,
}

/// This computer as another one reaches it. Blocking for a moment.
pub fn here() -> Here {
    let listening = std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], 22)), Duration::from_millis(400)).is_ok();
    let computer = local_name();
    let address = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_default();
    let account = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let host_key = ["/etc/ssh/ssh_host_ed25519_key.pub", "/etc/ssh/ssh_host_ecdsa_key.pub", "/etc/ssh/ssh_host_rsa_key.pub"]
        .iter()
        .find(|p| Path::new(p).exists())
        .and_then(|p| crate::process::command("ssh-keygen").args(["-l", "-f", p]).output().ok())
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().nth(1).map(str::to_string))
        .unwrap_or_default();
    Here { listening, computer, address, account, host_key }
}

fn local_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = crate::process::command("scutil").args(["--get", "LocalHostName"]).output() {
            let n = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !n.is_empty() {
                return format!("{n}.local");
            }
        }
    }
    crate::process::command("hostname").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

// ------------------------------------------------------------ the copy

/// A file there: its path under the remote's home, size and time.
#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
}

/// The `find` over what is copied, each file a line `size mtime path`,
/// as BSD's and GNU's `stat` each say it. `$N` is a `find` test that
/// comes first (`-newer` a mark), or nothing. Of Claude Code's registry
/// (`.claude/sessions`) only the records are copied, never the key
/// files beside them, which hold each inbox's token.
const FIND: &str = r#"
R=""; for r in .claude/projects .codex/sessions .codex/archived_sessions; do [ -d "$r" ] && R="$R $r"; done
[ -f .codex/session_index.jsonl ] && R="$R .codex/session_index.jsonl"
emaki_each() {
  if stat -c %s . >/dev/null 2>&1; then
    find "$@" -exec stat -c '%s %Y %n' {} + 2>/dev/null
  else
    find "$@" -exec stat -f '%z %m %N' {} + 2>/dev/null
  fi
}
emaki_find() {
  [ -n "$R" ] && emaki_each $R -type f $N
  [ -d .claude/sessions ] && emaki_each .claude/sessions -maxdepth 1 -type f -name '*.json' $N
  return 0
}
"#;

/// How much more than its size a record is asked for (`wants`).
const RECORD_ROOM: u64 = 1 << 16;

/// Where Claude Code's registry is copied, under a remote's copy.
const REGISTRY: &str = ".claude/sessions";

fn parse_stats(out: &str) -> Vec<Stat> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, ' ');
            let size = it.next()?.parse().ok()?;
            let mtime = it.next()?.parse().ok()?;
            let path = it.next()?.to_string();
            ok_path(&path).then_some(Stat { path, size, mtime })
        })
        .collect()
}

/// A path that stays inside what is copied.
fn ok_path(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.split('/').any(|c| c == "..") && (p.starts_with(".claude/projects/") || p.starts_with(".codex/") || is_record(p))
}

/// A record of the registry: `.claude/sessions/<pid>.json`.
fn is_record(p: &str) -> bool {
    p.strip_prefix(".claude/sessions/").and_then(|n| n.strip_suffix(".json")).is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
}

/// The remote's registry as copied here (`peer::records_in` reads it).
pub fn registry_dir(remote: &str) -> PathBuf {
    mirror_dir(remote).join(REGISTRY)
}

/// Every file that is copied, as it is there now.
pub fn list(d: &Remote) -> Result<Vec<Stat>, String> {
    let script = format!("cd \"$HOME\" || exit 1\nN=\n{FIND}\nemaki_find\n");
    run(d, &script, Duration::from_secs(120)).map(|o| parse_stats(&o))
}

/// What one look at a file asks for: from where, how much, and how far
/// back from there the copy is checked against the file.
struct Want {
    stat: Stat,
    from: u64,
    check: u64,
}

/// The bytes to ask for each file that changed: what was added to one
/// that grew, all of one that shrank or that is not a transcript.
fn wants(dir: &Path, there: &[Stat]) -> Vec<Want> {
    there
        .iter()
        .filter_map(|s| {
            let local = dir.join(&s.path);
            let meta = std::fs::metadata(&local).ok();
            let (len, mtime) = meta.map(|m| (m.len(), mtime_of(&m))).unwrap_or((0, i64::MIN));
            // A record is rewritten in place, its size and second often
            // the same, and is small: taken whole each time it is told.
            if is_record(&s.path) {
                // Asked for with room: rewritten as it is read, it may
                // have grown.
                return Some(Want { stat: Stat { size: s.size + RECORD_ROOM, ..s.clone() }, from: 0, check: 0 });
            }
            if len == s.size && (mtime == s.mtime || s.path.ends_with(".jsonl")) {
                return None;
            }
            // A transcript is written at its end: what was added is
            // asked for, with a little before it to check the copy
            // still begins as the file does.
            if s.path.ends_with(".jsonl") && len > 0 && s.size > len {
                let check = len.min(256);
                return Some(Want { stat: s.clone(), from: len - check, check });
            }
            Some(Want { stat: s.clone(), from: 0, check: 0 })
        })
        .collect()
}

fn mtime_of(m: &std::fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// What comes after each file's bytes, so a file that was cut short as
/// it was read is told from one that was not.
const TRAILER: &[u8] = b"\n--emaki--\n";

/// The script that sends each file asked for on its stdin (`from len
/// path` a line): a line saying it with the length it has, its bytes,
/// and the trailer. A file that shrank or went since it was looked at
/// (a record of the registry, whose process ended) says how much it
/// has, which may be nothing.
const FETCH: &str = r#"
cd "$HOME" || exit 1
t="${TMPDIR:-/tmp}"; t="${t%/}/emaki-fetch-$$"
trap 'rm -f "$t"' EXIT
while IFS=' ' read -r from len p; do
  tail -c +$((from + 1)) "$p" 2>/dev/null | head -c "$len" > "$t"
  n=$(wc -c < "$t" | tr -d ' ')
  printf '%s %s %s\n' "$from" "$n" "$p"
  cat "$t"
  printf '\n--emaki--\n'
done
"#;

/// Copy what is asked for. Files whose copy no longer begins as they do
/// are named, to be copied whole.
fn fetch(d: &Remote, dir: &Path, wants: &[Want]) -> Result<Vec<Stat>, String> {
    if wants.is_empty() {
        return Ok(Vec::new());
    }
    let mut cmd = command(d, FETCH, false);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let asks: String = wants.iter().map(|w| format!("{} {} {}\n", w.from, w.stat.size - w.from, w.stat.path)).collect();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(asks.as_bytes());
    });
    let mut out = BufReader::with_capacity(1 << 20, child.stdout.take().ok_or("no stdout")?);
    let mut again = Vec::new();
    let mut result = Ok(());
    for w in wants {
        if let Err(e) = take_one(&mut out, dir, w, &mut again) {
            result = Err(e);
            break;
        }
    }
    let _ = writer.join();
    if result.is_err() {
        let _ = child.kill();
    }
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let status = child.wait().ok();
    match result {
        Ok(()) => Ok(again),
        Err(_) if !err.trim().is_empty() => Err(ssh_error(&err, status.and_then(|s| s.code()))),
        Err(e) => Err(e),
    }
}

/// One file's answer, into its copy: written whole beside it and moved
/// in when asked for whole, else checked and added to its end.
fn take_one(out: &mut impl BufRead, dir: &Path, w: &Want, again: &mut Vec<Stat>) -> Result<(), String> {
    let mut head = String::new();
    out.read_line(&mut head).map_err(|e| e.to_string())?;
    let mut it = head.trim_end_matches('\n').splitn(3, ' ');
    let (from, len, path) = (it.next().and_then(|s| s.parse::<u64>().ok()), it.next().and_then(|s| s.parse::<u64>().ok()), it.next().unwrap_or(""));
    let want = w.stat.size - w.from;
    let Some(len) = len.filter(|n| *n <= want) else { return Err("the remote computer's answer was not what was asked".into()) };
    if from != Some(w.from) || path != w.stat.path {
        return Err("the remote computer's answer was not what was asked".into());
    }
    let mut bytes = Vec::with_capacity(len.min(64 << 20) as usize);
    out.take(len).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let mut trailer = vec![0u8; TRAILER.len()];
    if bytes.len() as u64 != len || out.read_exact(&mut trailer).is_err() || trailer != TRAILER {
        return Err("the remote computer's answer was cut short".into());
    }
    // Shorter than it was when looked at: it changed or went as it was
    // read. Left as it is here; the watch tells of it again when it
    // changes.
    // A record is taken when it reads whole, as a rewrite caught part
    // way does not, and a broken record would read as its process gone.
    if is_record(path) {
        if len > 0 && len < want && serde_json::from_slice::<serde_json::Value>(&bytes).is_ok() {
            let local = dir.join(path);
            if let Some(parent) = local.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            paths::write_atomic(&local, &bytes).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if len < want {
        return Ok(());
    }
    let local = dir.join(&w.stat.path);
    if let Some(parent) = local.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if w.from == 0 {
        paths::write_atomic(&local, &bytes).map_err(|e| e.to_string())?;
    } else {
        let (check, rest) = bytes.split_at(w.check as usize);
        let mut f = std::fs::OpenOptions::new().read(true).append(true).open(&local).map_err(|e| e.to_string())?;
        let mut mine = vec![0u8; w.check as usize];
        use std::io::Seek;
        let same = f.seek(std::io::SeekFrom::Start(w.from)).is_ok() && f.read_exact(&mut mine).is_ok() && mine == check && f.metadata().map(|m| m.len()).unwrap_or(0) == w.from + w.check;
        if !same {
            again.push(w.stat.clone());
            return Ok(());
        }
        f.write_all(rest).map_err(|e| e.to_string())?;
    }
    if let Ok(f) = std::fs::File::options().write(true).open(&local) {
        let _ = f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(w.stat.mtime.max(0) as u64));
    }
    Ok(())
}

/// Bring the copy up to what the remote has, `files` of it, a batch at a
/// time so a first copy of a large history moves along. Codex's copy
/// follows its files out too (`prune`).
fn bring(d: &Remote, files: &[Stat], progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let dir = mirror_dir(&d.id);
    let mut todo = wants(&dir, files);
    let total: u64 = todo.iter().map(|w| w.stat.size - w.from).sum();
    let mut done = 0u64;
    let mut rounds = 0;
    while !todo.is_empty() && rounds < 3 {
        rounds += 1;
        let mut again = Vec::new();
        let mut batch: Vec<Want> = Vec::new();
        let mut size = 0u64;
        let flush = |batch: &mut Vec<Want>, again: &mut Vec<Stat>, done: &mut u64| -> Result<(), String> {
            let n: u64 = batch.iter().map(|w| w.stat.size - w.from).sum();
            again.extend(fetch(d, &dir, batch)?);
            *done += n;
            progress(*done, total);
            batch.clear();
            Ok(())
        };
        for w in todo {
            size += w.stat.size - w.from;
            batch.push(w);
            if size > (48 << 20) || batch.len() >= 400 {
                flush(&mut batch, &mut again, &mut done)?;
                size = 0;
            }
        }
        flush(&mut batch, &mut again, &mut done)?;
        todo = again.into_iter().map(|stat| Want { stat, from: 0, check: 0 }).collect();
    }
    Ok(())
}

/// Codex's files gone from the remote go from the copy: Codex deletes
/// nothing by age, so one gone was moved (archived) or deleted there. So
/// do the registry's records of processes that have ended.
fn prune(d: &Remote, files: &[Stat]) {
    let dir = mirror_dir(&d.id);
    let there: std::collections::HashSet<&str> = files.iter().map(|s| s.path.as_str()).collect();
    let mut stack: Vec<PathBuf> = CODEX_ROOTS.iter().chain([&REGISTRY]).map(|r| dir.join(r)).collect();
    while let Some(p) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&p) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(&dir) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if !there.contains(rel.as_str()) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

/// How a remote's link stands, as the window says it.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum LinkState {
    #[default]
    Connecting,
    /// Copying what it has, so much of so much.
    Copying(u64, u64),
    Connected,
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct LinkStatus {
    pub state: LinkState,
    pub probe: Option<Probe>,
}

/// The watch kept running on a remote while it is linked: once a second,
/// each file of the copied ones changed since the look before, as `FIND`
/// says it, then a `.` so a link that broke is known by its silence and a
/// watch whose link broke ends at its next write. The mark it compares
/// with is a file of its own in the remote's temporary folder.
const WATCH: &str = r#"
cd "$HOME" || exit 1
m="${TMPDIR:-/tmp}/emaki-watch-$$"
: > "$m"
trap 'rm -f "$m" "$m.n"' EXIT
trap 'exit 0' INT TERM HUP PIPE
while :; do
  : > "$m.n"
  N="-newer $m"
  emaki_find
  mv -f "$m.n" "$m"
  echo .
  sleep 1
done
"#;

/// A remote's link: its sessions copied and kept up to date for as long
/// as it runs, reconnecting after a break. `changed` is told each time
/// the copy changed; `status` how the link stands.
pub struct Link {
    pub remote: Remote,
    stop: Arc<AtomicBool>,
    poke: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Link {
    pub fn start(remote: Remote, status: Arc<dyn Fn(&str, LinkStatus) + Send + Sync>, changed: Arc<dyn Fn() + Send + Sync>) -> Arc<Link> {
        let link = Arc::new(Link { remote, stop: Arc::new(AtomicBool::new(false)), poke: Arc::new(AtomicBool::new(false)), child: Arc::new(Mutex::new(None)) });
        let me = Arc::clone(&link);
        let _ = std::thread::Builder::new().name(format!("emaki-remote-{}", me.remote.id)).spawn(move || me.run(status, changed));
        link
    }

    /// Stop it for good: the remote was removed or the app quits.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mut c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Try again now, not after the wait a failure set.
    pub fn retry(&self) {
        self.poke.store(true, Ordering::Relaxed);
    }

    fn rest(&self, secs: u64) {
        let until = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < until && !self.stop.load(Ordering::Relaxed) && !self.poke.swap(false, Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn run(self: Arc<Self>, status: Arc<dyn Fn(&str, LinkStatus) + Send + Sync>, changed: Arc<dyn Fn() + Send + Sync>) {
        let id = self.remote.id.clone();
        // A link stopped says nothing more: one started in its place for
        // the same remote speaks for it, and a word from a probe this one
        // was still waiting on came after the new one's.
        let say = |state: LinkState, probe: Option<Probe>| {
            if !self.stop.load(Ordering::Relaxed) {
                status(&id, LinkStatus { state, probe })
            }
        };
        let mut wait = 5;
        while !self.stop.load(Ordering::Relaxed) {
            say(LinkState::Connecting, probed(&id));
            let probe = match probe(&self.remote) {
                Ok(p) => {
                    fill_account(&id, &p);
                    p
                }
                Err(e) => {
                    say(LinkState::Failed(e), probed(&id));
                    self.rest(wait);
                    wait = (wait * 2).min(120);
                    continue;
                }
            };
            let copied = list(&self.remote).and_then(|files| {
                let p = probe.clone();
                bring(&self.remote, &files, &|done, total| {
                    if total > (8 << 20) {
                        say(LinkState::Copying(done, total), Some(p.clone()));
                    }
                })?;
                prune(&self.remote, &files);
                Ok(())
            });
            if let Err(e) = copied {
                say(LinkState::Failed(e), Some(probe));
                self.rest(wait);
                wait = (wait * 2).min(120);
                continue;
            }
            changed();
            say(LinkState::Connected, Some(probe.clone()));
            wait = 5;
            let ended = self.watch(&changed);
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            say(LinkState::Failed(ended), Some(probe));
            self.rest(wait);
        }
    }

    /// Follow the remote's files until the link breaks: each change
    /// copied as it is told. What broke it, in words.
    fn watch(&self, changed: &Arc<dyn Fn() + Send + Sync>) -> String {
        let script = format!("{FIND}\n{WATCH}");
        let mut cmd = command(&self.remote, &script, false);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return format!("could not start ssh: {e}"),
        };
        let out = child.stdout.take();
        let err = child.stderr.take();
        *self.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        let Some(out) = out else { return "no answer".into() };
        // A line comes every second; fifteen without one is a link gone
        // quiet, ended so it is made again.
        let last = Arc::new(Mutex::new(Instant::now()));
        let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
        {
            let tx = tx.clone();
            let last = Arc::clone(&last);
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    *last.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
                    if tx.send(Some(line)).is_err() {
                        break;
                    }
                }
                let _ = tx.send(None);
            });
        }
        let mut pending: HashMap<String, Stat> = HashMap::new();
        let ended = loop {
            if self.stop.load(Ordering::Relaxed) {
                break String::new();
            }
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Some(line)) if line == "." => {
                    if !pending.is_empty() {
                        let files: Vec<Stat> = pending.drain().map(|(_, s)| s).collect();
                        match bring(&self.remote, &files, &|_, _| {}) {
                            Ok(()) => changed(),
                            Err(e) => break e,
                        }
                    }
                }
                Ok(Some(line)) => {
                    for s in parse_stats(&line) {
                        pending.insert(s.path.clone(), s);
                    }
                }
                Ok(None) => break "the connection closed".into(),
                Err(_) if last.lock().unwrap_or_else(|e| e.into_inner()).elapsed() > Duration::from_secs(15) => break "the remote stopped answering".into(),
                Err(_) => {}
            }
        };
        if let Some(mut c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(mut e) = err {
            let mut s = String::new();
            let _ = e.read_to_string(&mut s);
            if !s.trim().is_empty() && !ended.is_empty() {
                return ssh_error(&s, None);
            }
        }
        if ended.is_empty() { "stopped".into() } else { ended }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_folder_is_its_computer_and_its_path() {
        let f = folder("mac-mini", "/Users/me/emaki");
        assert_eq!(f, "ssh://mac-mini/Users/me/emaki");
        assert_eq!(split(&f), Some(("mac-mini", "/Users/me/emaki")));
        assert_eq!(path_of(&f), "/Users/me/emaki");
        assert_eq!(split("/Users/me/emaki"), None);
        assert_eq!(path_of("/Users/me/emaki"), "/Users/me/emaki");
    }

    #[test]
    fn a_computer_is_kept_once_with_every_way_to_it() {
        // Kept before a computer had ways: its one way is its own.
        let old: Remote = serde_json::from_str(r#"{"id":"mba","name":"mba","host":"mba.local","user":"me","port":0}"#).unwrap();
        assert_eq!(old.ways, vec![Way { host: "mba.local".into(), user: "me".into(), port: 0 }]);
        assert_eq!(serde_json::to_value(&old).unwrap(), serde_json::json!({"id":"mba","name":"mba","ways":[{"host":"mba.local","user":"me"}]}));
        // Kept again by another way, it has both.
        let mut list = vec![old.clone()];
        keep_in(&mut list, &Remote { ways: vec![Way { host: "macbook-air".into(), ..Way::default() }], ..old.clone() });
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].ways.iter().map(Way::address).collect::<Vec<_>>(), ["me@mba.local", "macbook-air"]);
        // Connected by one way, it stays on it when another is added;
        // disconnected, it is connected by the one added.
        let air = Way { host: "macbook-air".into(), ..Way::default() };
        list[0].using = Some(list[0].ways[0].clone());
        keep_in(&mut list, &Remote { ways: vec![air.clone()], using: Some(air.clone()), ..old.clone() });
        assert_eq!(list[0].way().host, "mba.local");
        list[0].off = true;
        keep_in(&mut list, &Remote { ways: vec![air.clone()], using: Some(air.clone()), ..old.clone() });
        assert_eq!((list[0].off, list[0].way()), (false, air.clone()));
        // Named as its person names it, else by its host.
        let named = |computer: Option<&str>| Probe { computer: computer.map(str::to_string), hostname: "pingfan-mba.local".into(), ..Probe::default() }.name();
        assert_eq!(named(Some("小茶壶的MacBook Air")), "小茶壶的MacBook Air");
        assert_eq!(named(Some("")), "pingfan-mba");
        assert_eq!(named(None), "pingfan-mba");
        // Which network a way is on.
        assert!(list[0].ways[0].local());
        assert!(!air.local());
        assert!(Way { host: "192.168.1.20".into(), ..Way::default() }.local());
        assert!(!Way { host: "100.113.189.88".into(), ..Way::default() }.local());
        // One computer by its own id, whatever its name.
        let p = |machine: Option<&str>, hostname: &str| Probe { machine: machine.map(str::to_string), hostname: hostname.into(), home: "/Users/me".into(), os: "Darwin".into(), ..Probe::default() };
        assert!(same_machine(&p(Some("A"), "x"), &p(Some("A"), "y")));
        assert!(!same_machine(&p(Some("A"), "x"), &p(Some("B"), "x")));
        assert!(!same_machine(&p(Some(""), "x"), &p(Some(""), "x")));
        // An answer from before the id was asked, by its name, home and system.
        assert!(same_machine(&p(None, "mba.local"), &p(Some("A"), "mba.local")));
        assert!(!same_machine(&p(None, "mba.local"), &p(Some("A"), "mini.local")));
    }

    #[test]
    fn a_word_is_quoted_for_the_shell() {
        assert_eq!(sh_quote("claude"), "claude");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\"'\"'s'");
        assert_eq!(sh_quote(""), "''");
    }

    #[test]
    fn the_sign_in_page_and_its_answer_are_read() {
        let said = "Opening browser to sign in…\nIf the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&state=x\nPaste code here if prompted > ";
        assert_eq!(login_url(said).as_deref(), Some("https://claude.com/cai/oauth/authorize?code=true&state=x"));
        assert_eq!(login_url("visit: https://claude.com/cai/oau"), None);
        assert_eq!(signed_in_from(r#"{"loggedIn":true,"authMethod":"claude.ai"}"#), Some(true));
        assert_eq!(signed_in_from(r#"{"loggedIn":false}"#), Some(false));
        assert_eq!(signed_in_from(""), None);
        assert_eq!(codex_signed_in_from("Logged in using ChatGPT"), Some(true));
        assert_eq!(codex_signed_in_from("Not logged in"), Some(false));
        assert_eq!(codex_signed_in_from("bash: codex: command not found"), None);
        let device = "Follow these steps:\n\n1. Open this link in your browser\n   \u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m\n\n2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m\n   \u{1b}[94mZ5YI-NM22H\u{1b}[0m\n";
        assert_eq!(login_url(device).as_deref(), Some("https://auth.openai.com/codex/device"));
        assert_eq!(device_code(device).as_deref(), Some("Z5YI-NM22H"));
        assert_eq!(device_code("2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)"), None);
        assert_eq!(last_words("\u{1b}]7337;emaki-pid=4242\u{7}Error: could not reach the server\n").as_deref(), Some("could not reach the server"));
        assert_eq!(last_words("Paste code here if prompted > \nError: Invalid code\n").as_deref(), Some("Invalid code"));
    }

    #[test]
    fn of_the_registry_only_records_are_copied() {
        assert!(is_record(".claude/sessions/71343.json"));
        assert!(!is_record(".claude/sessions/71343.abc.key"));
        assert!(!is_record(".claude/sessions/x.json"));
        assert!(!is_record(".claude/sessions/1/2.json"));
        let s = parse_stats("90 1 .claude/sessions/5.json\n9 1 .claude/sessions/5.k.key\n");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn what_find_says_is_read_and_kept_inside_what_is_copied() {
        let out = "120 1700000000 .claude/projects/-a/x y.jsonl\n9 1 /etc/passwd\n9 1 .claude/projects/../../etc\nbad\n";
        let s = parse_stats(out);
        assert_eq!(s, vec![Stat { path: ".claude/projects/-a/x y.jsonl".into(), size: 120, mtime: 1_700_000_000 }]);
    }
}
