//! Codex driven from the window.
//!
//! ```text
//! codex app-server --listen unix://<socket>     JSON-RPC, a message a WebSocket frame
//! codex resume <id> --remote unix://<socket>    Codex's terminal, joined to that server
//! codex app-server                              the same wire a line a message, on stdin and stdout
//! ```
//!
//! This is Codex's own wire for a program that is not its terminal: its
//! editor extension and its desktop app speak it. One child a driven
//! session, kept between turns and let go when idle, as Claude Code's
//! headless child is (`driver::Driver`); both are a `driver::Drive`, and
//! the window does not ask which it has. The child writes the same
//! rollout file the terminal's Codex does, and everything shown is still
//! read from that file.
//!
//! The child listens on a socket of its own so that Codex's terminal can
//! join it (`Drive::attach`): the terminal is then a second client of the
//! one server, not a second Codex, and the rollout keeps its one writer.
//! Both see every turn, whoever began it, and either can answer what
//! Codex asks. Where there is no Unix socket (Windows), or the socket
//! does not come up, the child speaks on its stdin and stdout and there
//! is no terminal.
//!
//! What goes over the wire (checked against Codex 0.162):
//!
//! - `initialize`, then `initialized`. `model/list`, `permissionProfile/list`
//!   and `collaborationMode/list` say what a session can be set to;
//!   `skills/list` the skills a folder has.
//! - `thread/start` makes a session and names it: the id is Codex's, not
//!   ours. `thread/resume` takes one up by its id.
//! - `turn/start` with the message. A turn ends in `turn/completed`, whose
//!   `status` is `completed`, `interrupted` or `failed`.
//! - `thread/settings/update` sets the model, the effort, the permission
//!   profile with its approval policy, and the collaboration mode, for the
//!   turns that follow. The collaboration mode carries a model and an
//!   effort of its own, which win over the plain ones, so every update
//!   sends it whole.
//! - Codex asks by a request of its own: leave to run a command
//!   (`item/commandExecution/requestApproval`), to change files
//!   (`item/fileChange/requestApproval`), for wider permissions
//!   (`item/permissions/requestApproval`), and a question
//!   (`item/tool/requestUserInput`). Each is answered by its id.
//!   `serverRequest/resolved` says one is settled, whoever settled it.
//! - `turn/interrupt` stops a turn.
//!
//! A mode here is one key for two things Codex keeps apart: the permission
//! profile (`read-only`, `workspace`, `danger-full-access`, or one the
//! person's config adds) and the plan collaboration mode (`plan`). Leaving
//! plan goes back to the profile the session had.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::agents;
use crate::driver::{child_env, effort_color, Caps, CommandInfo, Drive, DriverError, Event, PermissionRequest, State, TurnResult, REQUEST_TIMEOUT};
use crate::json::*;
use crate::model::AgentId;
use crate::options::{humanize, Catalogue, Choice, ModelChoice, Options};

/// The Codex release this wire was last checked against.
pub const TESTED_CODEX_VERSION: &str = "0.162.1";

/// The mode that is Codex's plan collaboration mode, not a permission
/// profile.
pub const PLAN: &str = "plan";

/// What the window sends when the person says go on a plan, the words
/// Codex's own terminal sends.
pub const IMPLEMENT: &str = "Implement the plan.";

/// Where the Codex program is: by name in the folders every agent is
/// looked for in.
pub fn codex_binary() -> Option<PathBuf> {
    let agent = agents::by_id("codex")?;
    agents::find_in(agent, &agents::search_dirs())
}

/// Codex's words for a permission profile it ships, and a line on each.
/// One the person's config adds is named by its key.
pub fn mode_words(key: &str) -> (String, &'static str) {
    match key {
        "read-only" => ("Read Only".into(), "Reads files and answers; changes nothing"),
        "workspace" => ("Default".into(), "Reads and edits in this folder and runs commands in its sandbox; asks for anything beyond"),
        "danger-full-access" => ("Full Access".into(), "Edits anywhere and runs any command with the network, without asking"),
        PLAN => ("Plan".into(), "Looks into the work and writes a plan; changes nothing until you say go"),
        _ => (humanize(key), ""),
    }
}

/// The approval policy a permission profile goes with, as Codex's own
/// presets pair them: full access never asks, the rest ask when the
/// sandbox stops something.
fn approval_for(profile: &str) -> &'static str {
    if profile == "danger-full-access" {
        "never"
    } else {
        "on-request"
    }
}

/// The profile a sandbox is, for a session that names no profile.
fn profile_of_sandbox(kind: &str) -> &'static str {
    match kind {
        "readOnly" | "read-only" => "read-only",
        "dangerFullAccess" | "danger-full-access" => "danger-full-access",
        _ => "workspace",
    }
}

/// What a session can be set to, from the three lists Codex hands out.
pub fn options_from(models: &Value, profiles: &Value, collab: &Value) -> Options {
    let mut out = Options::default();
    for p in arr_of(profiles, "data") {
        if p.get("allowed").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let key = str_of(p, "id").trim_start_matches(':').to_string();
        if key.is_empty() {
            continue;
        }
        let (label, line) = mode_words(&key);
        let detail = match str_of(p, "description") {
            "" => line.to_string(),
            said => said.to_string(),
        };
        out.modes.push(Choice { key, label, detail, ..Default::default() });
    }
    for m in arr_of(collab, "data") {
        if str_of(m, "mode") == PLAN {
            let (label, line) = mode_words(PLAN);
            let label = match str_of(m, "name") {
                "" => label,
                name => name.to_string(),
            };
            out.modes.push(Choice { key: PLAN.into(), label, detail: line.into(), ..Default::default() });
        }
    }
    for m in arr_of(models, "data") {
        if m.get("hidden").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let efforts: Vec<Choice> = arr_of(m, "supportedReasoningEfforts")
            .iter()
            .map(|e| {
                let key = str_of(e, "reasoningEffort").to_string();
                Choice { label: humanize(&key), detail: str_of(e, "description").into(), color: effort_color(&key), key, ..Default::default() }
            })
            .collect();
        let key = str_of(m, "id").to_string();
        if key.is_empty() {
            continue;
        }
        out.models.push(ModelChoice { label: str_of(m, "displayName").into(), detail: str_of(m, "description").into(), resolved: str_of(m, "model").into(), efforts, key });
    }
    out
}

/// The slash commands a Codex session takes from the window: the few the
/// driver runs itself, then the folder's skills.
fn commands_from(skills: &Value) -> Vec<CommandInfo> {
    let mut out = vec![CommandInfo { name: "compact".into(), description: "Summarise the conversation so far to free its context".into(), argument_hint: String::new(), builtin: true }];
    for group in arr_of(skills, "data") {
        for s in arr_of(group, "skills") {
            if s.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let name = str_of(s, "name").to_string();
            if name.is_empty() || out.iter().any(|c| c.name == name) {
                continue;
            }
            let description = match str_of(s, "shortDescription") {
                "" => str_of(s, "description"),
                short => short,
            };
            out.push(CommandInfo { name, description: description.into(), argument_hint: String::new(), builtin: false });
        }
    }
    out
}

/// A request Codex is waiting on, and what answering it takes.
/// How the child is spoken to.
enum Wire {
    Stdin(ChildStdin),
    #[cfg(unix)]
    Socket(std::os::unix::net::UnixStream),
}

/// A WebSocket client, as little of one as the app server's socket asks:
/// the upgrade, text frames out (masked, as a client's must be), messages
/// in, and a pong for a ping.
#[cfg(unix)]
mod ws {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU32, Ordering};

    pub enum Message {
        Text(String),
        Ping(Vec<u8>),
    }

    pub fn upgrade(stream: &mut UnixStream) -> std::io::Result<()> {
        // The key is only echoed back hashed; nothing here depends on it.
        stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: ZW1ha2ktY29kZXgtd2lyZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n")?;
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut b)?;
            head.push(b[0]);
            if head.len() > 8192 {
                break;
            }
        }
        if head.starts_with(b"HTTP/1.1 101") {
            Ok(())
        } else {
            Err(std::io::Error::other(String::from_utf8_lossy(&head).lines().next().unwrap_or("no answer").to_string()))
        }
    }

    fn frame(stream: &mut UnixStream, op: u8, payload: &[u8]) -> std::io::Result<()> {
        static SEED: AtomicU32 = AtomicU32::new(0x9e37_79b9);
        let key = SEED.fetch_add(0x6d2b_79f5, Ordering::Relaxed).to_be_bytes();
        let mut out = Vec::with_capacity(payload.len() + 14);
        out.push(0x80 | op);
        match payload.len() {
            n if n < 126 => out.push(0x80 | n as u8),
            n if n <= 0xffff => {
                out.push(0x80 | 126);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                out.push(0x80 | 127);
                out.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        out.extend_from_slice(&key);
        out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        stream.write_all(&out)?;
        stream.flush()
    }

    pub fn text(stream: &mut UnixStream, text: &str) -> std::io::Result<()> {
        frame(stream, 1, text.as_bytes())
    }

    pub fn pong(stream: &mut UnixStream, payload: &[u8]) -> std::io::Result<()> {
        frame(stream, 10, payload)
    }

    /// The next whole message, or none when the server has closed.
    pub fn read(stream: &mut UnixStream) -> Option<Message> {
        let mut whole: Vec<u8> = Vec::new();
        loop {
            let mut h = [0u8; 2];
            stream.read_exact(&mut h).ok()?;
            let (fin, op, masked) = (h[0] & 0x80 != 0, h[0] & 0x0f, h[1] & 0x80 != 0);
            let len = match h[1] & 0x7f {
                126 => {
                    let mut b = [0u8; 2];
                    stream.read_exact(&mut b).ok()?;
                    u16::from_be_bytes(b) as u64
                }
                127 => {
                    let mut b = [0u8; 8];
                    stream.read_exact(&mut b).ok()?;
                    u64::from_be_bytes(b)
                }
                n => n as u64,
            };
            let mut key = [0u8; 4];
            if masked {
                stream.read_exact(&mut key).ok()?;
            }
            let mut payload = vec![0u8; usize::try_from(len).ok()?];
            stream.read_exact(&mut payload).ok()?;
            if masked {
                payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= key[i % 4]);
            }
            match op {
                8 => return None,
                9 => return Some(Message::Ping(payload)),
                10 => {}
                _ => {
                    whole.extend_from_slice(&payload);
                    if fin {
                        return Some(Message::Text(String::from_utf8_lossy(&whole).into_owned()));
                    }
                }
            }
        }
    }
}

struct Pending {
    /// The id the answer goes back under, as Codex sent it.
    rpc_id: Value,
    kind: Ask,
    req: PermissionRequest,
    /// A question's ids by its words, since the card answers by the words.
    question_ids: Vec<(String, String)>,
    /// What was asked for, when the answer is to grant it.
    permissions: Value,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    Command,
    Files,
    Permissions,
    Question,
    /// Not Codex's: the go-ahead on a plan, which its terminal asks itself.
    Plan,
}

struct Inner {
    state: State,
    caps: Caps,
    /// The permission profile, and whether the plan mode is on over it.
    profile: String,
    plan: bool,
    model: String,
    effort: String,
    turn_id: String,
    /// The plan the turn under way has written, when it has.
    plan_text: String,
    idle_since: Instant,
    turn_started: Option<Instant>,
    queue: VecDeque<(String, Value)>,
    waiting: HashMap<u64, Sender<Result<Value, String>>>,
    next_id: u64,
    pending: HashMap<String, Pending>,
    /// Items as they began, by id: a request for leave names its item and
    /// says little else.
    items: HashMap<String, Value>,
    skills: Vec<(String, String)>,
    stderr_tail: VecDeque<String>,
    error: String,
    exit_code: Option<i32>,
    exited: bool,
}

impl Inner {
    fn mode(&self) -> String {
        if self.plan {
            PLAN.into()
        } else {
            self.profile.clone()
        }
    }
}

pub struct CodexDriver {
    session_id: Mutex<String>,
    cwd: String,
    child: Mutex<Option<Child>>,
    wire: Mutex<Option<Wire>>,
    /// The socket the child listens on, when it does: where Codex's
    /// terminal joins it.
    socket: Option<PathBuf>,
    /// The name that socket was asked for under.
    asked: Option<PathBuf>,
    inner: Arc<Mutex<Inner>>,
    events: Sender<Event>,
}

fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Codex wraps a failure from its service as JSON inside a string; the
/// sentence in it is what a person reads.
fn error_words(message: &str) -> String {
    let inner = serde_json::from_str::<Value>(message).ok().and_then(|v| v.get("error").map(|e| str_of(e, "message").to_string())).filter(|m| !m.is_empty());
    inner.unwrap_or_else(|| message.trim().to_string())
}

/// A command as a person would type it: Codex runs each through a login
/// shell (`/bin/zsh -lc '<command>'`), and the shell is not the news.
pub fn plain_command(command: &str) -> String {
    let c = command.trim();
    for shell in ["/bin/zsh -lc ", "/bin/bash -lc ", "zsh -lc ", "bash -lc ", "/bin/sh -c ", "sh -c "] {
        if let Some(rest) = c.strip_prefix(shell) {
            let rest = rest.trim();
            let quoted = (rest.starts_with('\'') && rest.ends_with('\'')) || (rest.starts_with('"') && rest.ends_with('"'));
            if quoted && rest.len() >= 2 {
                let body = &rest[1..rest.len() - 1];
                return if rest.starts_with('\'') { body.replace("'\\''", "'") } else { body.replace("\\\"", "\"") };
            }
            return rest.to_string();
        }
    }
    c.to_string()
}

impl CodexDriver {
    /// Start a child and take up `session_id` on it, or with `resume`
    /// false make a new session, whose id is Codex's to give: ask
    /// `session_id()` afterwards. `mode` and `model` empty leave the
    /// person's own config to say.
    pub fn start(session_id: &str, cwd: &str, resume: bool, mode: &str, model: &str, events: Sender<Event>) -> Result<Arc<CodexDriver>, DriverError> {
        if !cwd.is_empty() && !Path::new(cwd).is_dir() {
            return Err(DriverError(format!("the session's directory is gone: {cwd}")));
        }
        // On a socket, so that Codex's terminal can join; on its stdin
        // and stdout where that cannot be had.
        let driver = match Self::spawn(cwd, events.clone(), cfg!(unix)) {
            Ok(d) => d,
            Err(_) if cfg!(unix) => Self::spawn(cwd, events, false)?,
            Err(e) => return Err(e),
        };
        let ready = (|| -> Result<(), DriverError> {
            driver.handshake()?;
            driver.learn(cwd);
            let mut params = Map::new();
            if !cwd.is_empty() {
                params.insert("cwd".into(), json!(cwd));
            }
            if !model.is_empty() && model != "default" {
                params.insert("model".into(), json!(model));
            }
            let profile = if mode == PLAN { "" } else { mode };
            if !profile.is_empty() {
                params.insert("permissions".into(), json!(format!(":{profile}")));
                params.insert("approvalPolicy".into(), json!(approval_for(profile)));
            }
            let reply = if resume {
                params.insert("threadId".into(), json!(session_id));
                params.insert("excludeTurns".into(), json!(true));
                driver.request("thread/resume", Value::Object(params), REQUEST_TIMEOUT)?
            } else {
                driver.request("thread/start", Value::Object(params), REQUEST_TIMEOUT)?
            };
            let id = reply.get("thread").map(|t| str_of(t, "id").to_string()).unwrap_or_default();
            if id.is_empty() {
                return Err(DriverError("codex named no session".into()));
            }
            *driver.session_id.lock().unwrap() = id;
            driver.absorb_settings(&reply);
            if mode == PLAN && !driver.inner.lock().unwrap().plan {
                driver.set_mode(PLAN)?;
            }
            Ok(())
        })();
        if let Err(e) = ready {
            let tail = driver.inner.lock().unwrap().stderr_tail.back().cloned().unwrap_or_default();
            driver.stop();
            return Err(DriverError(if tail.is_empty() { e.0 } else { format!("{} ({tail})", e.0) }));
        }
        {
            let mut g = driver.inner.lock().unwrap();
            g.state = State::Idle;
            g.idle_since = Instant::now();
        }
        Ok(driver)
    }

    /// The child and its two readers, with nothing said to it yet.
    /// `listen` has it on a socket of its own, and fails where the socket
    /// does not come up.
    fn spawn(cwd: &str, events: Sender<Event>, listen: bool) -> Result<Arc<CodexDriver>, DriverError> {
        let binary = codex_binary().ok_or_else(|| DriverError("codex is not installed".into()))?;
        let mut cmd = Command::new(binary);
        cmd.arg("app-server");
        let asked = listen.then(socket_path);
        if let Some(path) = &asked {
            let _ = std::fs::remove_file(path);
            cmd.arg("--listen").arg(format!("unix://{}", path.display()));
        }
        if !cwd.is_empty() {
            cmd.current_dir(cwd);
        }
        cmd.env_clear();
        for (k, v) in child_env() {
            cmd.env(k, v);
        }
        if listen {
            cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        } else {
            cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let mut child = cmd.spawn().map_err(|e| DriverError(format!("could not start codex: {e}")))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let mut socket = None;
        #[cfg(unix)]
        let mut reader = None;
        let wire = match &asked {
            None => child.stdin.take().map(Wire::Stdin),
            #[cfg(unix)]
            Some(path) => match join_socket(path, &mut child) {
                Ok((stream, at)) => {
                    reader = stream.try_clone().ok();
                    socket = Some(at);
                    Some(Wire::Socket(stream))
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(path);
                    return Err(e);
                }
            },
            #[cfg(not(unix))]
            Some(_) => None,
        };
        let inner = Inner {
            state: State::Starting,
            caps: Caps::default(),
            profile: String::new(),
            plan: false,
            model: String::new(),
            effort: String::new(),
            turn_id: String::new(),
            plan_text: String::new(),
            idle_since: Instant::now(),
            turn_started: None,
            queue: VecDeque::new(),
            waiting: HashMap::new(),
            next_id: 0,
            pending: HashMap::new(),
            items: HashMap::new(),
            skills: Vec::new(),
            stderr_tail: VecDeque::new(),
            error: String::new(),
            exit_code: None,
            exited: false,
        };
        let driver = Arc::new(CodexDriver { session_id: Mutex::new(String::new()), cwd: cwd.into(), child: Mutex::new(Some(child)), wire: Mutex::new(wire), socket, asked, inner: Arc::new(Mutex::new(inner)), events });
        if let Some(out) = stdout {
            let d = Arc::clone(&driver);
            let mut lines = BufReader::new(out).lines();
            thread::Builder::new().name("emaki-codex".into()).spawn(move || d.read_loop(move || lines.next().and_then(Result::ok))).ok();
        }
        #[cfg(unix)]
        if let Some(mut stream) = reader {
            let d = Arc::clone(&driver);
            let pinged = Arc::clone(&driver);
            thread::Builder::new()
                .name("emaki-codex".into())
                .spawn(move || {
                    d.read_loop(move || loop {
                        match ws::read(&mut stream)? {
                            ws::Message::Text(t) => return Some(t),
                            ws::Message::Ping(p) => {
                                if let Some(Wire::Socket(w)) = pinged.wire.lock().unwrap().as_mut() {
                                    let _ = ws::pong(w, &p);
                                }
                            }
                        }
                    })
                })
                .ok();
        }
        if let Some(err) = stderr {
            let d = Arc::clone(&driver);
            thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    let line = line.trim_end().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let mut g = d.inner.lock().unwrap();
                    g.stderr_tail.push_back(line.chars().take(400).collect());
                    if g.stderr_tail.len() > 40 {
                        g.stderr_tail.pop_front();
                    }
                }
            });
        }
        Ok(driver)
    }

    fn handshake(&self) -> Result<(), DriverError> {
        let hello = json!({"clientInfo": {"name": "emaki", "title": "Emaki", "version": env!("CARGO_PKG_VERSION")}, "capabilities": {"experimentalApi": true, "requestAttestation": false}});
        let reply = self.request("initialize", hello, Duration::from_secs(15)).map_err(|e| DriverError(format!("codex did not answer: {e}")))?;
        // "emaki/0.162.0 (Mac OS ...)": the release is after the slash.
        let version = str_of(&reply, "userAgent").split('/').nth(1).and_then(|s| s.split_whitespace().next()).unwrap_or("").to_string();
        self.inner.lock().unwrap().caps.version = version;
        self.notify("initialized", json!({}));
        Ok(())
    }

    /// Ask what a session here can be set to and which skills the folder
    /// has. A list that does not come leaves that part empty.
    fn learn(&self, cwd: &str) {
        let ask = |method: &str, params: Value| self.request(method, params, Duration::from_secs(10)).unwrap_or(Value::Null);
        let models = ask("model/list", json!({}));
        let profiles = ask("permissionProfile/list", json!({}));
        let collab = ask("collaborationMode/list", json!({}));
        let skills = if cwd.is_empty() { Value::Null } else { ask("skills/list", json!({"cwds": [cwd]})) };
        let mut g = self.inner.lock().unwrap();
        g.caps.options = options_from(&models, &profiles, &collab);
        g.caps.commands = commands_from(&skills);
        g.caps.slash_commands = g.caps.commands.iter().map(|c| c.name.clone()).collect();
        g.skills = arr_of(&skills, "data").iter().flat_map(|group| arr_of(group, "skills").iter().map(|s| (str_of(s, "name").to_string(), str_of(s, "path").to_string())).collect::<Vec<_>>()).collect();
        g.caps.skills = g.skills.iter().map(|(n, _)| n.clone()).collect();
    }

    /// What a reply or a notice says the session is set to.
    fn absorb_settings(&self, v: &Value) {
        let mut g = self.inner.lock().unwrap();
        let model = str_of(v, "model");
        if !model.is_empty() {
            g.model = model.into();
        }
        let effort = match str_of(v, "reasoningEffort") {
            "" => str_of(v, "effort"),
            e => e,
        };
        if !effort.is_empty() {
            g.effort = effort.into();
        }
        let named = v.get("activePermissionProfile").map(|p| str_of(p, "id").trim_start_matches(':').to_string()).unwrap_or_default();
        let sandbox = v.get("sandbox").or_else(|| v.get("sandboxPolicy")).map(|s| str_of(s, "type")).unwrap_or("");
        if !named.is_empty() {
            g.profile = named;
        } else if !sandbox.is_empty() {
            g.profile = profile_of_sandbox(sandbox).into();
        }
        if let Some(c) = v.get("collaborationMode").filter(|c| c.is_object()) {
            g.plan = str_of(c, "mode") == PLAN;
        }
        g.caps.model = g.model.clone();
        g.caps.effort = g.effort.clone();
        g.caps.mode = g.mode();
    }

    fn write(&self, frame: &Value) -> Result<(), DriverError> {
        let mut guard = self.wire.lock().unwrap();
        let wire = guard.as_mut().ok_or_else(|| DriverError("codex is not running".into()))?;
        let mut line = serde_json::to_string(frame).map_err(|e| DriverError(e.to_string()))?;
        match wire {
            Wire::Stdin(stdin) => {
                line.push('\n');
                stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush())
            }
            #[cfg(unix)]
            Wire::Socket(stream) => ws::text(stream, &line),
        }
        .map_err(|e| DriverError(format!("codex is not listening: {e}")))
    }

    fn notify(&self, method: &str, params: Value) {
        let _ = self.write(&json!({"method": method, "params": params}));
    }

    /// Ask Codex something and wait for its answer.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, DriverError> {
        let (tx, rx) = mpsc::channel();
        let id = {
            let mut g = self.inner.lock().unwrap();
            if g.exited {
                return Err(DriverError("codex is not running".into()));
            }
            g.next_id += 1;
            let id = g.next_id;
            g.waiting.insert(id, tx);
            id
        };
        if let Err(e) = self.write(&json!({"id": id, "method": method, "params": params})) {
            self.inner.lock().unwrap().waiting.remove(&id);
            return Err(e);
        }
        match rx.recv_timeout(timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(DriverError(error_words(&e))),
            Err(_) => {
                self.inner.lock().unwrap().waiting.remove(&id);
                Err(DriverError(format!("codex did not answer {method}")))
            }
        }
    }

    /// The name the socket was asked for under is taken away with it.
    fn forget_socket(&self) {
        if let Some(asked) = &self.asked {
            let _ = std::fs::remove_file(asked);
        }
    }

    fn emit(&self, ev: Event) {
        let _ = self.events.send(ev);
    }

    /// Take what Codex says, a message at a time from `next`, until it
    /// has no more.
    fn read_loop(self: Arc<Self>, mut next: impl FnMut() -> Option<String>) {
        while let Some(line) = next() {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            let method = str_of(&msg, "method").to_string();
            let has_id = msg.get("id").is_some_and(|i| !i.is_null());
            if method.is_empty() {
                // An answer to something we asked.
                let Some(id) = msg.get("id").and_then(Value::as_u64) else { continue };
                let Some(tx) = self.inner.lock().unwrap().waiting.remove(&id) else { continue };
                let _ = tx.send(match msg.get("error") {
                    Some(e) => Err(str_of(e, "message").to_string()),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                });
            } else if has_id {
                self.on_request(&method, msg.get("id").cloned().unwrap_or(Value::Null), msg.get("params").unwrap_or(&Value::Null));
            } else {
                self.on_notice(&method, msg.get("params").unwrap_or(&Value::Null));
            }
        }
        // The child is gone: whoever waits on it is told, once. One that
        // closed its socket and stayed is ended here.
        let code = self.child.lock().unwrap().as_mut().and_then(|c| {
            if self.socket.is_some() {
                end_child(c);
            }
            c.wait().ok()
        }).and_then(|s| s.code());
        self.forget_socket();
        let error = {
            let mut g = self.inner.lock().unwrap();
            let was_stopped = g.exited;
            g.exited = true;
            g.state = State::Exited;
            g.exit_code = code;
            g.waiting.clear();
            g.pending.clear();
            if !was_stopped && g.error.is_empty() && code != Some(0) {
                g.error = g.stderr_tail.back().cloned().unwrap_or_default();
            }
            g.error.clone()
        };
        self.emit(Event::Exit { code, error });
    }

    /// Whether a notice is about this session. Codex says which thread
    /// each is for; one child holds one, but its own helpers (a review, a
    /// sub-agent) are threads too.
    fn ours(&self, params: &Value) -> bool {
        let thread = str_of(params, "threadId");
        thread.is_empty() || thread == self.session_id.lock().unwrap().as_str()
    }

    fn on_notice(&self, method: &str, params: &Value) {
        if !self.ours(params) {
            return;
        }
        match method {
            "turn/started" => {
                // A go-ahead still waiting was answered where the turn
                // began: in Codex's terminal.
                let answered: Vec<String> = {
                    let mut g = self.inner.lock().unwrap();
                    g.state = State::Running;
                    g.turn_id = params.get("turn").map(|t| str_of(t, "id").to_string()).unwrap_or_default();
                    g.turn_started = Some(Instant::now());
                    g.plan_text.clear();
                    g.error.clear();
                    let ids: Vec<String> = g.pending.iter().filter(|(_, p)| p.kind == Ask::Plan).map(|(k, _)| k.clone()).collect();
                    ids.iter().for_each(|k| {
                        g.pending.remove(k);
                    });
                    ids
                };
                for id in answered {
                    self.emit(Event::PermissionSettled(id));
                }
                // Said for a turn begun in Codex's terminal too, which
                // nothing here sent.
                let queued = self.inner.lock().unwrap().queue.len();
                self.emit(Event::Turn { text: String::new(), queued });
            }
            "item/started" => {
                if let Some(item) = params.get("item") {
                    let id = str_of(item, "id").to_string();
                    if !id.is_empty() {
                        self.inner.lock().unwrap().items.insert(id, item.clone());
                    }
                }
            }
            "item/completed" => {
                if let Some(item) = params.get("item") {
                    let mut g = self.inner.lock().unwrap();
                    g.items.remove(str_of(item, "id"));
                    if str_of(item, "type") == "plan" {
                        g.plan_text = str_of(item, "text").to_string();
                    }
                }
            }
            "thread/settings/updated" => {
                if let Some(s) = params.get("threadSettings") {
                    let before = self.mode();
                    self.absorb_settings(s);
                    let now = self.mode();
                    if now != before {
                        self.emit(Event::Mode(now));
                    }
                    self.emit(Event::Init(self.caps()));
                }
            }
            "serverRequest/resolved" => {
                let id = request_key(params.get("requestId").unwrap_or(&Value::Null));
                if self.inner.lock().unwrap().pending.remove(&id).is_some() {
                    self.emit(Event::PermissionSettled(id));
                }
            }
            "error" => {
                let words = params.get("error").map(|e| error_words(str_of(e, "message"))).unwrap_or_default();
                if !words.is_empty() && params.get("willRetry").and_then(Value::as_bool) != Some(true) {
                    self.inner.lock().unwrap().error = words;
                }
            }
            "turn/completed" => self.turn_over(params),
            _ => {}
        }
    }

    /// A turn ended: say how, ask for the go-ahead when it left a plan,
    /// and start what was queued behind it.
    fn turn_over(&self, params: &Value) {
        let turn = params.get("turn").cloned().unwrap_or(Value::Null);
        let status = str_of(&turn, "status").to_string();
        let (result, plan, next) = {
            let mut g = self.inner.lock().unwrap();
            g.turn_id.clear();
            g.turn_started = None;
            g.items.clear();
            g.idle_since = Instant::now();
            let failed = status == "failed";
            let said = turn.get("error").map(|e| error_words(str_of(e, "message"))).filter(|m| !m.is_empty()).unwrap_or_else(|| g.error.clone());
            let plan = std::mem::take(&mut g.plan_text);
            let plan = (g.plan && status == "completed" && !plan.trim().is_empty()).then_some(plan);
            // A plan waits for its answer before anything queued goes.
            let next = if plan.is_some() || status != "completed" { None } else { g.queue.pop_front() };
            g.state = if next.is_some() { State::Running } else { State::Idle };
            let result = TurnResult {
                subtype: if failed && !said.is_empty() { said } else { status.clone() },
                is_error: failed,
                duration_ms: turn.get("durationMs").and_then(Value::as_u64).unwrap_or(0),
                queued: g.queue.len() + usize::from(next.is_some()),
                ..Default::default()
            };
            (result, plan, next)
        };
        self.emit(Event::Result(result));
        if let Some(plan) = plan {
            let id = format!("plan-{}", str_of(&turn, "id"));
            let mut input = Map::new();
            input.insert("plan".into(), json!(plan));
            let req = PermissionRequest { request_id: id.clone(), tool_name: "ExitPlanMode".into(), tool_use_id: id.clone(), input, description: "Implement this plan?".into(), asked_at: now_secs() };
            self.inner.lock().unwrap().pending.insert(id, Pending { rpc_id: Value::Null, kind: Ask::Plan, req: req.clone(), question_ids: Vec::new(), permissions: Value::Null });
            self.emit(Event::Permission(req));
        }
        if let Some((text, input)) = next {
            let queued = self.inner.lock().unwrap().queue.len();
            self.emit(Event::Turn { text, queued });
            if let Err(e) = self.start_turn(input) {
                let mut g = self.inner.lock().unwrap();
                g.state = State::Idle;
                g.error = e.0;
            }
        }
    }

    /// Codex asks something of the person. What the window's cards know
    /// how to draw is a tool call asking leave, so each is put as one:
    /// a command as Claude Code's `Bash`, a question as its
    /// `AskUserQuestion`.
    fn on_request(&self, method: &str, rpc_id: Value, params: &Value) {
        let key = request_key(&rpc_id);
        let item = self.inner.lock().unwrap().items.get(str_of(params, "itemId")).cloned().unwrap_or(Value::Null);
        let mut input = Map::new();
        let mut question_ids = Vec::new();
        let mut permissions = Value::Null;
        let reason = str_of(params, "reason").to_string();
        let (kind, tool) = match method {
            "item/commandExecution/requestApproval" | "execCommandApproval" => {
                let command = match str_of(params, "command") {
                    "" => str_of(&item, "command"),
                    c => c,
                };
                input.insert("command".into(), json!(plain_command(command)));
                if !reason.is_empty() {
                    input.insert("description".into(), json!(reason));
                }
                let cwd = str_of(params, "cwd");
                if !cwd.is_empty() && cwd != self.cwd {
                    input.insert("cwd".into(), json!(cwd));
                }
                (Ask::Command, "Bash")
            }
            "item/fileChange/requestApproval" | "applyPatchApproval" => {
                // The request names its item; the item has the changes.
                let changes = item.get("changes").cloned().unwrap_or(Value::Null);
                let paths: Vec<String> = changes.as_array().map(|a| a.iter().map(|c| str_of(c, "path").to_string()).filter(|p| !p.is_empty()).collect()).unwrap_or_default();
                let diff: Vec<String> = changes.as_array().map(|a| a.iter().map(|c| str_of(c, "diff").to_string()).filter(|d| !d.is_empty()).collect()).unwrap_or_default();
                if let Some(first) = paths.first() {
                    input.insert("file_path".into(), json!(first));
                }
                if paths.len() > 1 {
                    input.insert("files".into(), json!(paths));
                }
                if !diff.is_empty() {
                    input.insert("diff".into(), json!(diff.join("\n")));
                }
                if !reason.is_empty() {
                    input.insert("description".into(), json!(reason));
                }
                let root = str_of(params, "grantRoot");
                if !root.is_empty() {
                    input.insert("grant_root".into(), json!(root));
                }
                (Ask::Files, "apply_patch")
            }
            "item/permissions/requestApproval" => {
                permissions = params.get("permissions").cloned().unwrap_or(Value::Null);
                input.insert("permissions".into(), permissions.clone());
                if !reason.is_empty() {
                    input.insert("description".into(), json!(reason));
                }
                (Ask::Permissions, "request_permissions")
            }
            "item/tool/requestUserInput" => {
                let questions: Vec<Value> = arr_of(params, "questions")
                    .iter()
                    .map(|q| {
                        question_ids.push((str_of(q, "question").to_string(), str_of(q, "id").to_string()));
                        let options: Vec<Value> = arr_of(q, "options").iter().map(|o| json!({"label": str_of(o, "label"), "description": str_of(o, "description")})).collect();
                        json!({"question": str_of(q, "question"), "header": str_of(q, "header"), "options": options, "multiSelect": false})
                    })
                    .collect();
                input.insert("questions".into(), json!(questions));
                (Ask::Question, "AskUserQuestion")
            }
            // Asked of a host that has what we do not (the editor's own
            // tools, its sign-in): said so, and Codex goes on without.
            _ => {
                let _ = self.write(&json!({"id": rpc_id, "error": {"code": -32601, "message": format!("Emaki does not answer {method}")}}));
                return;
            }
        };
        let description = if kind == Ask::Question { String::new() } else { reason };
        let req = PermissionRequest { request_id: key.clone(), tool_name: tool.into(), tool_use_id: str_of(params, "itemId").into(), input, description, asked_at: now_secs() };
        self.inner.lock().unwrap().pending.insert(key, Pending { rpc_id, kind, req: req.clone(), question_ids, permissions });
        self.emit(Event::Permission(req));
    }

    fn start_turn(&self, input: Value) -> Result<(), DriverError> {
        let thread = self.session_id.lock().unwrap().clone();
        {
            let mut g = self.inner.lock().unwrap();
            g.state = State::Running;
            g.turn_started = Some(Instant::now());
            g.error.clear();
        }
        self.request("turn/start", json!({"threadId": thread, "input": input}), REQUEST_TIMEOUT).map(|_| ())
    }

    /// The message as Codex takes one: a skill named at its start as the
    /// skill, the words, then each picture.
    fn input_of(&self, text: &str, images: Vec<Value>) -> Value {
        let mut input: Vec<Value> = Vec::new();
        let words = text.trim();
        if let Some(rest) = words.strip_prefix('/') {
            let name = rest.split_whitespace().next().unwrap_or("");
            let path = self.inner.lock().unwrap().skills.iter().find(|(n, _)| n == name).map(|(_, p)| p.clone());
            if let Some(path) = path {
                input.push(json!({"type": "skill", "name": name, "path": path}));
            }
        }
        if !words.is_empty() {
            input.push(json!({"type": "text", "text": words, "text_elements": []}));
        }
        for img in images {
            // A picture arrives as Claude Code takes one, base64 in a
            // block, or as the path of its file.
            if let Some(path) = img.get("path").and_then(Value::as_str) {
                input.push(json!({"type": "localImage", "path": path}));
            } else if let Some(src) = img.get("source") {
                let (media, data) = (str_of(src, "media_type"), str_of(src, "data"));
                if !data.is_empty() {
                    input.push(json!({"type": "image", "url": format!("data:{media};base64,{data}")}));
                }
            }
        }
        Value::Array(input)
    }

    /// Everything the session is set to, sent whole: the collaboration
    /// mode carries a model and an effort that win over the plain ones.
    fn push_settings(&self, profile: Option<&str>) -> Result<(), DriverError> {
        let thread = self.session_id.lock().unwrap().clone();
        let (plan, model, effort) = {
            let g = self.inner.lock().unwrap();
            (g.plan, g.model.clone(), g.effort.clone())
        };
        let effort_v = if effort.is_empty() { Value::Null } else { json!(effort) };
        let mut params = Map::new();
        params.insert("threadId".into(), json!(thread));
        if !model.is_empty() {
            params.insert("model".into(), json!(model));
            params.insert("collaborationMode".into(), json!({"mode": if plan { PLAN } else { "default" }, "settings": {"model": model, "reasoning_effort": effort_v, "developer_instructions": null}}));
        }
        if !effort.is_empty() {
            params.insert("effort".into(), json!(effort));
        }
        if let Some(profile) = profile {
            params.insert("permissions".into(), json!(format!(":{profile}")));
            params.insert("approvalPolicy".into(), json!(approval_for(profile)));
        }
        self.request("thread/settings/update", Value::Object(params), REQUEST_TIMEOUT).map(|_| ())
    }

    fn answer(&self, rpc_id: &Value, result: Value) {
        let _ = self.write(&json!({"id": rpc_id, "result": result}));
    }
}

/// A request's id as the key the window keeps it under. Codex numbers
/// them, and may one day name them.
/// Where a child started now listens: a name of its own under the run
/// folder. Codex puts the socket somewhere short and leaves a link here
/// when this is too long for one.
fn socket_path() -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = crate::paths::run_dir();
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("codex-{}-{}.sock", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))
}

/// Join the socket a child was asked to listen on, once it is there: the
/// stream, upgraded, and where the socket is.
#[cfg(unix)]
fn join_socket(asked: &Path, child: &mut Child) -> Result<(std::os::unix::net::UnixStream, PathBuf), DriverError> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(DriverError(format!("codex ended as it started ({status})")));
        }
        if asked.symlink_metadata().is_ok() {
            let at = std::fs::read_link(asked).unwrap_or_else(|_| asked.to_path_buf());
            if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&at) {
                ws::upgrade(&mut stream).map_err(|e| DriverError(format!("codex's socket did not answer: {e}")))?;
                return Ok((stream, at));
            }
        }
        if Instant::now() > deadline {
            return Err(DriverError("codex's socket did not come up".into()));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

/// Ask a child to end, as a terminal's close would.
fn end_child(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    #[cfg(not(unix))]
    let _ = child.kill();
}

fn request_key(id: &Value) -> String {
    match id {
        Value::String(s) => format!("codex-{s}"),
        other => format!("codex-{other}"),
    }
}

impl Drive for CodexDriver {
    fn agent(&self) -> AgentId {
        AgentId::Codex
    }

    fn session_id(&self) -> String {
        self.session_id.lock().unwrap().clone()
    }

    fn cwd(&self) -> String {
        self.cwd.clone()
    }

    fn state(&self) -> State {
        self.inner.lock().unwrap().state
    }

    fn alive(&self) -> bool {
        !self.inner.lock().unwrap().exited
    }

    fn caps(&self) -> Caps {
        self.inner.lock().unwrap().caps.clone()
    }

    fn mode(&self) -> String {
        self.inner.lock().unwrap().mode()
    }

    fn model(&self) -> String {
        self.inner.lock().unwrap().model.clone()
    }

    fn effort(&self) -> String {
        self.inner.lock().unwrap().effort.clone()
    }

    fn error(&self) -> String {
        self.inner.lock().unwrap().error.clone()
    }

    fn idle_for(&self) -> Duration {
        let g = self.inner.lock().unwrap();
        // One waiting on the person is not idle, however long they take.
        if g.state == State::Idle && g.pending.is_empty() {
            g.idle_since.elapsed()
        } else {
            Duration::ZERO
        }
    }

    fn turn_elapsed(&self) -> Option<Duration> {
        self.inner.lock().unwrap().turn_started.map(|t| t.elapsed())
    }

    fn queued(&self) -> Vec<String> {
        self.inner.lock().unwrap().queue.iter().map(|(text, _)| text.clone()).collect()
    }

    fn drop_queued(&self, index: usize) -> bool {
        self.inner.lock().unwrap().queue.remove(index).is_some()
    }

    fn pending_permissions(&self) -> Vec<PermissionRequest> {
        let mut out: Vec<PermissionRequest> = self.inner.lock().unwrap().pending.values().map(|p| p.req.clone()).collect();
        out.sort_by(|a, b| a.asked_at.partial_cmp(&b.asked_at).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    fn attach(&self) -> Option<Vec<String>> {
        let socket = self.socket.as_ref()?;
        let id = self.session_id.lock().unwrap().clone();
        if id.is_empty() || !self.alive() {
            return None;
        }
        Some(vec![codex_binary()?.to_string_lossy().into_owned(), "resume".into(), id, "--remote".into(), format!("unix://{}", socket.display())])
    }

    fn stop(&self) {
        {
            let mut g = self.inner.lock().unwrap();
            if g.exited {
                return;
            }
            g.exited = true;
            g.state = State::Exited;
        }
        // Closing its stdin is how the app server is told to go; it
        // finishes writing the rollout first. One on a socket goes on
        // listening with nobody joined, so it is asked to end.
        self.wire.lock().unwrap().take();
        self.forget_socket();
        if let Some(mut child) = self.child.lock().unwrap().take() {
            if self.socket.is_some() {
                end_child(&mut child);
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
    }

    fn send(&self, text: &str, images: Vec<Value>) -> Result<bool, DriverError> {
        if !self.alive() {
            return Err(DriverError("codex is not running".into()));
        }
        // The one command the driver runs itself.
        if text.trim() == "/compact" && images.is_empty() {
            let thread = self.session_id();
            if self.state() == State::Running {
                return Err(DriverError("wait for the turn to end before compacting".into()));
            }
            self.inner.lock().unwrap().state = State::Running;
            return self.request("thread/compact/start", json!({"threadId": thread}), REQUEST_TIMEOUT).map(|_| false).inspect_err(|_| self.inner.lock().unwrap().state = State::Idle);
        }
        let input = self.input_of(text, images);
        {
            let mut g = self.inner.lock().unwrap();
            if g.state == State::Running || g.pending.values().any(|p| p.kind == Ask::Plan) {
                // A new message while a plan waits is the answer "not
                // this plan": the question goes, the message is sent.
                let plans: Vec<String> = g.pending.iter().filter(|(_, p)| p.kind == Ask::Plan).map(|(k, _)| k.clone()).collect();
                if g.state != State::Running && !plans.is_empty() {
                    for k in &plans {
                        g.pending.remove(k);
                    }
                    drop(g);
                    for k in plans {
                        self.emit(Event::PermissionSettled(k));
                    }
                } else {
                    g.queue.push_back((text.to_string(), input));
                    return Ok(true);
                }
            }
        }
        self.start_turn(input).inspect_err(|_| self.inner.lock().unwrap().state = State::Idle)?;
        Ok(false)
    }

    fn interrupt(&self) -> Result<(), DriverError> {
        let (thread, turn) = (self.session_id(), self.inner.lock().unwrap().turn_id.clone());
        if turn.is_empty() {
            return Ok(());
        }
        self.request("turn/interrupt", json!({"threadId": thread, "turnId": turn}), REQUEST_TIMEOUT).map(|_| ())
    }

    fn set_mode(&self, mode: &str) -> Result<String, DriverError> {
        let before = {
            let g = self.inner.lock().unwrap();
            (g.plan, g.profile.clone())
        };
        let profile = {
            let mut g = self.inner.lock().unwrap();
            if mode == PLAN {
                g.plan = true;
                None
            } else {
                g.plan = false;
                g.profile = mode.to_string();
                Some(mode.to_string())
            }
        };
        if let Err(e) = self.push_settings(profile.as_deref()) {
            let mut g = self.inner.lock().unwrap();
            (g.plan, g.profile) = before;
            return Err(e);
        }
        let now = self.mode();
        self.inner.lock().unwrap().caps.mode = now.clone();
        Ok(now)
    }

    fn set_model(&self, model: &str) -> Result<(), DriverError> {
        let before = {
            let mut g = self.inner.lock().unwrap();
            let before = (g.model.clone(), g.effort.clone());
            g.model = model.to_string();
            // A level the new model does not take becomes the nearest
            // thing to it: the model's own list's last when the old one
            // was beyond it, else left for Codex to choose.
            let efforts: Vec<String> = g.caps.options.efforts_for(model).iter().map(|e| e.key.clone()).collect();
            if !efforts.is_empty() && !efforts.contains(&g.effort) {
                g.effort = if efforts.iter().any(|e| e == "medium") { "medium".into() } else { efforts[0].clone() };
            }
            before
        };
        if let Err(e) = self.push_settings(None) {
            let mut g = self.inner.lock().unwrap();
            (g.model, g.effort) = before;
            return Err(e);
        }
        let mut g = self.inner.lock().unwrap();
        g.caps.model = g.model.clone();
        g.caps.effort = g.effort.clone();
        Ok(())
    }

    fn set_effort(&self, effort: &str) -> Result<bool, DriverError> {
        let before = std::mem::replace(&mut self.inner.lock().unwrap().effort, effort.to_string());
        if let Err(e) = self.push_settings(None) {
            self.inner.lock().unwrap().effort = before;
            return Err(e);
        }
        self.inner.lock().unwrap().caps.effort = effort.to_string();
        Ok(false)
    }

    fn answer_permission(&self, request_id: &str, allow: bool, _message: &str) {
        let Some(p) = self.inner.lock().unwrap().pending.remove(request_id) else { return };
        match p.kind {
            Ask::Command | Ask::Files => self.answer(&p.rpc_id, json!({"decision": if allow { "accept" } else { "decline" }})),
            Ask::Permissions => self.answer(&p.rpc_id, json!({"permissions": if allow { p.permissions.clone() } else { json!({}) }, "scope": "turn"})),
            // No answer but the declined one: a question's answer goes
            // through `answer_question`.
            Ask::Question => self.answer(&p.rpc_id, json!({"answers": {}})),
            Ask::Plan => {
                if allow {
                    // Out of plan, back on the profile the session had,
                    // and told to go, as Codex's terminal does it.
                    let profile = self.inner.lock().unwrap().profile.clone();
                    let left = if profile.is_empty() { self.set_mode("workspace") } else { self.set_mode(&profile) };
                    match left {
                        Ok(now) => {
                            self.emit(Event::Mode(now));
                            let input = self.input_of(IMPLEMENT, Vec::new());
                            self.emit(Event::Turn { text: IMPLEMENT.into(), queued: 0 });
                            if let Err(e) = self.start_turn(input) {
                                let mut g = self.inner.lock().unwrap();
                                g.state = State::Idle;
                                g.error = e.0;
                            }
                        }
                        Err(e) => self.inner.lock().unwrap().error = e.0,
                    }
                }
            }
        }
        self.emit(Event::PermissionSettled(request_id.to_string()));
    }

    fn answer_question(&self, request_id: &str, answers: Map<String, Value>) {
        let Some(p) = self.inner.lock().unwrap().pending.remove(request_id) else { return };
        let mut out = Map::new();
        for (question, id) in &p.question_ids {
            let said: Vec<String> = match answers.get(question) {
                Some(Value::String(s)) => vec![s.clone()],
                Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
                _ => Vec::new(),
            };
            if !said.is_empty() {
                out.insert(id.clone(), json!({"answers": said}));
            }
        }
        self.answer(&p.rpc_id, json!({"answers": out}));
        self.emit(Event::PermissionSettled(request_id.to_string()));
    }
}

impl Drop for CodexDriver {
    fn drop(&mut self) {
        Drive::stop(self);
    }
}

/// What a Codex session in `cwd` can be set to and which commands it
/// takes, asked of a child started for the question and let go after.
/// No model is called and no session is made.
pub fn catalogue(cwd: &str) -> Result<Catalogue, DriverError> {
    let (tx, _rx) = mpsc::channel();
    let dir = if Path::new(cwd).is_dir() { cwd } else { "" };
    let driver = CodexDriver::spawn(dir, tx, false)?;
    let got = driver.handshake().map(|_| {
        driver.learn(dir);
        let caps = driver.caps();
        Catalogue { commands: caps.commands, options: caps.options }
    });
    Drive::stop(&*driver);
    got
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lists_codex_hands_out_become_the_choices() {
        let models = json!({"data": [
            {"id": "gpt-x", "model": "gpt-x", "displayName": "GPT-X", "description": "Fast.", "hidden": false, "isDefault": true,
             "supportedReasoningEfforts": [{"reasoningEffort": "low", "description": "Quick"}, {"reasoningEffort": "high", "description": "Deep"}], "defaultReasoningEffort": "low"},
            {"id": "gpt-hidden", "model": "gpt-hidden", "displayName": "Hidden", "description": "", "hidden": true, "supportedReasoningEfforts": []}
        ]});
        let profiles = json!({"data": [{"id": ":read-only", "description": null, "allowed": true}, {"id": ":workspace", "description": null, "allowed": true}, {"id": ":danger-full-access", "description": null, "allowed": false}, {"id": "mine", "description": "My own", "allowed": true}]});
        let collab = json!({"data": [{"name": "Plan", "mode": "plan"}, {"name": "Default", "mode": "default"}]});
        let o = options_from(&models, &profiles, &collab);
        let keys: Vec<&str> = o.modes.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, ["read-only", "workspace", "mine", "plan"]);
        assert_eq!(o.mode("workspace").unwrap().label, "Default");
        assert_eq!(o.mode("mine").unwrap().detail, "My own");
        assert_eq!(o.models.len(), 1);
        assert_eq!(o.models[0].label, "GPT-X");
        assert_eq!(o.efforts_for("gpt-x").iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), ["low", "high"]);
    }

    #[test]
    fn a_command_is_shown_without_the_shell_it_ran_in() {
        assert_eq!(plain_command("/bin/zsh -lc 'echo hello > a.txt'"), "echo hello > a.txt");
        assert_eq!(plain_command("/bin/zsh -lc \"pwd && rg --files -g 'A.md'\""), "pwd && rg --files -g 'A.md'");
        assert_eq!(plain_command("/bin/zsh -lc 'echo '\\''hi'\\'''"), "echo 'hi'");
        assert_eq!(plain_command("ls -la"), "ls -la");
    }

    #[test]
    fn a_model_the_list_does_not_name_reads_as_the_list_writes_them() {
        assert_eq!(crate::driver::model_label("gpt-5.5"), "GPT-5.5");
        assert_eq!(crate::driver::model_label("gpt-5.6-terra"), "GPT-5.6-Terra");
    }

    #[test]
    fn a_failure_is_said_in_its_own_sentence() {
        let wrapped = r#"{"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-5' model is not supported."}}"#;
        assert_eq!(error_words(wrapped), "The 'gpt-5' model is not supported.");
        assert_eq!(error_words("plain words"), "plain words");
    }
}
