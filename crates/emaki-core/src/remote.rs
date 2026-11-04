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

/// A computer this window reaches over ssh.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remote {
    /// The name its folders are written under: letters, digits and
    /// dashes, never changed once given.
    pub id: String,
    /// What the window calls it.
    pub name: String,
    /// Its address: a name, a number, or a host of `~/.ssh/config`.
    pub host: String,
    /// The account signed in to; empty for ssh's own choice.
    pub user: String,
    /// 0 for ssh's own choice.
    pub port: u16,
    /// Disconnected by the person: kept, with its sessions as they were
    /// copied, and nothing reached there until it is connected again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
}

impl Remote {
    /// `user@host`, as ssh is given it and the window says it.
    pub fn address(&self) -> String {
        if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }
}

fn remotes_file() -> PathBuf {
    paths::state_dir().join("remotes.json")
}

static LOCK: Mutex<()> = Mutex::new(());

/// The remotes, in the order they were added. Read again only when the
/// file changed: the window asks while it draws.
pub fn remotes() -> Vec<Remote> {
    type Stamp = (PathBuf, u64, std::time::SystemTime);
    static CACHE: Mutex<Option<(Stamp, Vec<Remote>)>> = Mutex::new(None);
    let path = remotes_file();
    let Ok(meta) = std::fs::metadata(&path) else { return Vec::new() };
    let stamp = (path.clone(), meta.len(), meta.modified().unwrap_or(std::time::UNIX_EPOCH));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, list)) = cache.as_ref() {
        if *at == stamp {
            return list.clone();
        }
    }
    let list: Vec<Remote> = paths::read_json(&path).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    *cache = Some((stamp, list.clone()));
    list
}

pub fn find(id: &str) -> Option<Remote> {
    remotes().into_iter().find(|d| d.id == id)
}

fn save(list: &[Remote]) -> std::io::Result<()> {
    paths::write_json(&remotes_file(), &serde_json::to_value(list).unwrap_or_default())
}

/// The remote an address names, before it is kept: the one kept with
/// that address, or a new one with an id of its own made from its address,
/// the same for the same address again, so a remote removed and added back
/// finds its copied sessions. Nothing is written: a remote is kept only once
/// it answers (`keep`).
pub fn draft(host: &str, user: &str, port: u16) -> Remote {
    let list = remotes();
    let host = host.trim().to_string();
    let user = user.trim().to_string();
    if let Some(d) = list.iter().find(|d| d.host == host && d.user == user && d.port == port) {
        return d.clone();
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
    Remote { id, name: host.clone(), host, user, port, off: false }
}

/// Keep a remote, or keep the one with its id as it is now.
pub fn keep(d: &Remote) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    match list.iter_mut().find(|k| k.id == d.id) {
        Some(k) => *k = d.clone(),
        None => list.push(d.clone()),
    }
    save(&list)
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

/// Disconnect a remote, or connect it again (`Remote::off`).
pub fn set_off(id: &str, off: bool) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = remotes();
    if let Some(d) = list.iter_mut().find(|d| d.id == id) {
        d.off = off;
    }
    save(&list)
}

/// Whether a remote is kept and connected: not disconnected by the person.
pub fn is_on(id: &str) -> bool {
    remotes().iter().any(|d| d.id == id && !d.off)
}

/// Forget a remote. Its copied sessions stay on disk, unread, and come
/// back with it.
pub fn remove(id: &str) -> std::io::Result<()> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<Remote> = remotes().into_iter().filter(|d| d.id != id).collect();
    save(&list)
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
    let mut cmd = ssh(d, tty);
    cmd.arg(format!("exec /bin/sh -c {}", sh_quote(script)));
    cmd
}

/// Run a script there and take what it printed, within `timeout`.
pub fn run(d: &Remote, script: &str, timeout: Duration) -> Result<String, String> {
    let mut cmd = command(d, script, false);
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
}

impl Probe {
    pub fn has(&self, agent: crate::model::AgentId) -> bool {
        match agent {
            crate::model::AgentId::ClaudeCode => !self.claude.is_empty(),
            crate::model::AgentId::Codex => !self.codex.is_empty(),
        }
    }
}

fn probe_file(remote: &str) -> PathBuf {
    mirror_dir(remote).join("probe.json")
}

/// What the remote said when last reached, kept between launches.
pub fn probed(remote: &str) -> Option<Probe> {
    static CACHE: Mutex<Option<HashMap<String, (std::time::SystemTime, Probe)>>> = Mutex::new(None);
    let path = probe_file(remote);
    let at = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some((when, p)) = map.get(remote) {
        if *when == at {
            return Some(p.clone());
        }
    }
    let p: Probe = paths::read_json(&path).and_then(|v| serde_json::from_value(v).ok())?;
    map.insert(remote.to_string(), (at, p.clone()));
    Some(p)
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
c=$(command -v claude 2>/dev/null); echo "claude=$c"
[ -n "$c" ] && echo "claude_version=$(claude --version 2>/dev/null | head -n 1)"
[ -n "$c" ] && echo "claude_auth=$(claude auth status 2>/dev/null | tr -d ' \t\n\r')"
x=$(command -v codex 2>/dev/null); echo "codex=$x"
[ -n "$x" ] && echo "codex_version=$(codex --version 2>/dev/null | head -n 1)"
echo "end=1"
"#;

/// Ask a remote what it has, and keep the answer.
pub fn probe(d: &Remote) -> Result<Probe, String> {
    // A computer asleep, or one that took the connection and says nothing,
    // fails in a few seconds rather than being waited on: anything at all
    // within `REACH` and it is there. The probe after it, its login shell
    // started, may take longer.
    run(d, "echo emaki", REACH)?;
    let out = run(d, PROBE, Duration::from_secs(30))?;
    let mut p = Probe::default();
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
            "claude" => p.claude = v,
            "claude_version" => p.claude_version = v.split_whitespace().next().unwrap_or("").to_string(),
            "codex" => p.codex = v,
            "codex_version" => p.codex_version = v.split_whitespace().last().unwrap_or("").to_string(),
            "claude_auth" => p.claude_signed_in = signed_in_from(&v),
            "end" => ended = true,
            _ => {}
        }
    }
    if !ended || p.home.is_empty() {
        return Err("it answered, but not as a Mac or Linux does".into());
    }
    let _ = std::fs::create_dir_all(mirror_dir(&d.id));
    let _ = paths::write_json(&probe_file(&d.id), &serde_json::to_value(&p).unwrap_or_default());
    Ok(p)
}

/// The command that starts `program` with `args` in a remote's folder,
/// on the remote: its login `PATH`, `env` set, in the folder or failing
/// with the folder named. `tty` gives it a terminal there.
pub fn agent_command(folder: &str, program: &str, args: &[String], env: &[(&str, &str)], tty: bool) -> Result<Command, String> {
    let (d, script) = agent_script(folder, program, args, env, false)?;
    Ok(command(&d, &script, tty))
}

/// An interactive Claude Code in a remote's folder, on a terminal there,
/// as the argv of the `ssh -tt` that starts it: `args` are Claude Code's
/// own. Before it becomes Claude Code the script names its pid
/// (`pty::ANNOUNCE`), which Claude Code keeps, so its record in the
/// registry there is told from any other.
pub fn terminal_argv(folder: &str, args: &[String]) -> Result<Vec<String>, String> {
    let (d, script) = agent_script(folder, "claude", args, &[("COLORTERM", "truecolor")], true)?;
    let cmd = command(&d, &script, true);
    let mut argv = vec![cmd.get_program().to_string_lossy().to_string()];
    argv.extend(cmd.get_args().map(|a| a.to_string_lossy().to_string()));
    Ok(argv)
}

fn agent_script(folder: &str, program: &str, args: &[String], env: &[(&str, &str)], announce: bool) -> Result<(Remote, String), String> {
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
    if announce {
        script.push_str(&format!("printf '{}' \"$$\"; ", crate::pty::ANNOUNCE));
    }
    script.push_str("exec ");
    if !env.is_empty() {
        script.push_str("env ");
        for (k, v) in env {
            script.push_str(&format!("{k}={} ", sh_quote(v)));
        }
    }
    script.push_str(&sh_quote(program));
    for a in args {
        script.push(' ');
        script.push_str(&sh_quote(a));
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

/// Ask Claude Code there again whether it is signed in, and keep the
/// answer with the rest of what the remote said. Off the window's thread.
pub fn check_signed_in(remote: &str) -> Result<Option<bool>, String> {
    // Signed out, it says so and ends with 1: what it said is read
    // whatever it ended with.
    let (d, script) = agent_script(&folder(remote, ""), "claude", &["auth".into(), "status".into()], &[], false)?;
    let mut cmd = command(&d, &script, false);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|e| format!("could not start ssh: {e}"))?;
    let (code, out, err) = wait_output(child, Duration::from_secs(30))?;
    if code == Some(255) {
        return Err(ssh_error(&err, code));
    }
    let said: String = out.chars().filter(|c| !c.is_whitespace()).collect();
    let signed = signed_in_from(&said);
    if let Some(mut p) = probed(remote) {
        p.claude_signed_in = signed;
        let _ = paths::write_json(&probe_file(remote), &serde_json::to_value(&p).unwrap_or_default());
    }
    Ok(signed)
}

/// Claude Code's own sign-in on a remote, done from here: `claude auth
/// login` there with no terminal, which prints the page to sign in at and
/// waits for the code that page shows. The page is opened in this
/// computer's browser and the code typed in the window, so nothing is
/// done at the remote's screen; none is needed. Claude Code keeps the
/// sign-in where it can: a Mac's keychain is locked to ssh, and then it
/// writes `~/.claude/.credentials.json`, which every later ssh session
/// reads.
pub struct Login {
    child: Mutex<Child>,
    stdin: Mutex<Option<std::process::ChildStdin>>,
    said: Arc<Mutex<String>>,
}

impl Login {
    /// Start it, and wait for the page's address. `BROWSER` is a program
    /// that does nothing, so no browser opens at the remote's screen.
    pub fn start(remote: &str) -> Result<(Login, String), String> {
        let (d, script) = agent_script(&folder(remote, ""), "claude", &["auth".into(), "login".into()], &[("BROWSER", "true")], false)?;
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
        let login = Login { child: Mutex::new(child), stdin: Mutex::new(stdin), said };
        let began = Instant::now();
        loop {
            if let Some(url) = login_url(&login.said()) {
                return Ok((login, url));
            }
            if let Ok(Some(_)) = login.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
                return Err(last_words(&login.said()).unwrap_or_else(|| "Claude Code there did not start its sign-in".into()));
            }
            if began.elapsed() > Duration::from_secs(40) {
                login.cancel();
                return Err("Claude Code there did not answer in time".into());
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
        loop {
            if let Ok(Some(st)) = self.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
                let after = self.said();
                let tail = after.get(before..).unwrap_or("");
                return if st.success() { Ok(()) } else { Err(last_words(tail).or_else(|| last_words(&after)).unwrap_or_else(|| "the sign-in did not go through".into())) };
            }
            if began.elapsed() > Duration::from_secs(90) {
                self.cancel();
                return Err("Claude Code there did not answer in time".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

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

/// The last line a program said that is not its prompt for the code.
fn last_words(said: &str) -> Option<String> {
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
        Some((id, _)) => remotes().iter().any(|d| d.id == id),
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
        let say = |state: LinkState, probe: Option<Probe>| status(&id, LinkStatus { state, probe });
        let mut wait = 5;
        while !self.stop.load(Ordering::Relaxed) {
            say(LinkState::Connecting, probed(&id));
            let probe = match probe(&self.remote) {
                Ok(p) => p,
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
