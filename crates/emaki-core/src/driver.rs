//! A headless Claude Code child the app drives.
//!
//! ```text
//! claude -p --input-format stream-json --output-format stream-json \
//!        --permission-prompt-tool stdio [--resume <id> | --session-id <id>]
//! ```
//!
//! One child per driven session, kept alive between turns and closed after
//! `idle_min` of silence; the next message starts it again with `--resume` on
//! the same id, so the transcript is one file throughout. The driver only
//! writes *into* a session. Everything shown still comes from the transcript.
//!
//! What Claude Code does on that wire (checked against 2.1.272):
//!
//! - `system/init` is emitted at the start of every turn: `model`,
//!   `permissionMode`, `slash_commands`, `skills`, `agents`, `tools`.
//! - stdin stays open across turns. Each user frame is one turn ending in a
//!   `result` frame.
//! - `control_request` from us: `initialize` (answers with the command
//!   catalogue), `set_permission_mode`, `set_model`, `interrupt`.
//! - `control_request` from the child: `can_use_tool` with `tool_name`,
//!   `input`, `tool_use_id`. Only with `--permission-prompt-tool stdio`;
//!   silence is a deny because there is no terminal to fall back to.
//! - A mode change writes no `permission-mode` row; the next user row carries
//!   `permissionMode` instead.
//! - Never `--bare`: it reads auth only from `ANTHROPIC_API_KEY`, never the
//!   keychain, which breaks subscription users.
//!
//! None of this is a public API. `TESTED_CLAUDE_VERSION` is the release the
//! wire was last checked against; a newer Claude Code is noted on stderr at
//! start, not refused, and `frames_from_the_recorded_wire` below keeps the
//! parser honest against frames recorded from that release.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::json::*;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a permission card may sit unanswered before the child is told no.
pub const PERMISSION_TIMEOUT: Duration = Duration::from_secs(600);

/// The Claude Code release this wire was last checked against.
pub const TESTED_CLAUDE_VERSION: &str = "2.1.283";

pub const MODES: &[&str] = &["default", "acceptEdits", "plan", "auto", "bypassPermissions"];
pub const MODELS: &[&str] = &["default", "fable", "opus", "sonnet", "haiku"];
/// `/effort` levels, as `claude --help` lists them.
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// What to call a model on a pill. Claude Code reports the full id in
/// `system/init` (`claude-opus-5-5`, `claude-haiku-4-5-20251001`,
/// `claude-3-5-sonnet-20241022`) and takes the family alias on the wire
/// (`opus`, `opus[1m]`); both read as "Opus 5.5", "Haiku 4.5", "Sonnet
/// 3.5", "Opus" and "Opus 1M". The version is the run of one- or two-digit
/// numbers next to the family name; a date stamp is longer and dropped.
pub fn model_label(model: &str) -> String {
    let m = model.trim();
    if m.is_empty() || m == "default" {
        return "Default model".into();
    }
    let (m, long_context) = match m.strip_suffix("[1m]") {
        Some(base) => (base, true),
        None => (m, false),
    };
    let body = m.strip_prefix("claude-").unwrap_or(m);
    let mut family = String::new();
    let mut version: Vec<&str> = Vec::new();
    for part in body.split('-') {
        let numeric = !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
        if numeric {
            if part.len() <= 2 {
                version.push(part);
            }
        } else if family.is_empty() {
            let mut c = part.chars();
            family = match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            };
        }
    }
    if family.is_empty() {
        family = m.to_string();
    }
    let mut out = family;
    if !version.is_empty() {
        out.push(' ');
        out.push_str(&version.join("."));
    }
    if long_context {
        out.push_str(" 1M");
    }
    out
}

/// What to call an effort level on a pill; empty is the session's default.
pub fn effort_label(effort: &str) -> String {
    match effort {
        "" => "Default effort".into(),
        "xhigh" => "Extra high effort".into(),
        e => {
            let mut c = e.chars();
            let head = c.next().map(|f| f.to_uppercase().collect::<String>()).unwrap_or_default();
            format!("{head}{} effort", c.as_str())
        }
    }
}

/// One line on an effort level, as `/effort` describes them.
pub fn effort_detail(effort: &str) -> &'static str {
    match effort {
        "low" => "Quick answers, little deliberation.",
        "medium" => "Balanced reasoning for everyday work.",
        "high" => "Deeper reasoning on harder problems.",
        "xhigh" => "Very deep reasoning; slower.",
        "max" => "Deepest reasoning; may overthink. For the hardest tasks.",
        _ => "Whatever the session started with.",
    }
}

/// What to call a permission mode on a pill.
pub fn mode_label(mode: &str) -> &'static str {
    match mode {
        "acceptEdits" => "Accept edits",
        "plan" => "Plan mode",
        "auto" => "Auto mode",
        "bypassPermissions" => "Bypass permissions",
        "dontAsk" => "Don't ask",
        _ => "Default permissions",
    }
}

/// One line on what a mode does, under its name in the picker.
pub fn mode_detail(mode: &str) -> &'static str {
    match mode {
        "acceptEdits" => "File edits go through; commands still ask.",
        "plan" => "Reads and plans; writes nothing until the plan is approved.",
        "auto" => "Claude Code decides what is safe to run, and asks about the rest.",
        "bypassPermissions" => "Nothing asks; nothing is held.",
        "dontAsk" => "Anything not already allowed is denied without asking.",
        _ => "Asks before each tool that needs permission.",
    }
}
pub const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Starting,
    Idle,
    Running,
    Exited,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    pub argument_hint: String,
    /// Claude Code's own, as opposed to the person's skills and plugins.
    #[serde(default)]
    pub builtin: bool,
}

/// The commands that act by themselves when typed alone: Claude Code's
/// `local` and `local-jsx` commands (`/compact`, `/model`, `/clear`), read
/// out of the 2.1.288 binary, where each command declares its type. The
/// other type, `prompt`, is a skill: text handed to the model, which a
/// person as often names inside a sentence. The catalogue on the wire
/// marks `builtin` and nothing finer, and bundled skills are built in too
/// (`/code-review`, `/loop`), so the type is kept here by name.
const LOCAL_COMMANDS: &[&str] = &[
    "add-dir", "advisor", "agents", "artifacts", "auto-mode-setup", "autocompact", "background", "branch", "brief", "btw", "bug", "cd", "chrome",
    "clear", "color", "compact", "config", "context", "copy", "desktop", "diff", "effort", "exit", "export", "extra-usage", "fast", "feedback",
    "focus", "fork", "goal", "help", "hooks", "ide", "import", "install-github-app", "install-slack-app", "keybindings", "list-agents", "login",
    "logout", "loops", "mcp", "memory", "mobile", "model", "output-style", "permissions", "plan", "plugin", "privacy-settings", "recap",
    "release-notes", "reload-plugins", "reload-skills", "remote-control", "rename", "resume", "rewind", "skill-doctor", "skills", "status",
    "stickers", "tasks", "terminal-setup", "theme", "tui", "ultrareview", "upgrade", "usage", "usage-credits", "version", "voice", "workflows",
];

/// Whether `/name` typed alone does something by itself, rather than being
/// a skill's prompt. A name not on the list is taken for a skill: putting
/// a command in the message is harmless where running one unasked is not.
pub fn acts_alone(name: &str) -> bool {
    LOCAL_COMMANDS.contains(&name)
}

fn name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || c == ':'
}

/// Every `/name` in `text` that stands as a word of its own, as byte
/// ranges with the slash: at the start or after a space or an opening
/// bracket or quote, and not the first part of a path (`/usr/bin`,
/// `/etc.conf`). A colon or dash that ends the name is punctuation.
pub fn slash_tokens(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let opens = prev.map(|p| p.is_whitespace() || "([{\"'".contains(p)).unwrap_or(true);
        prev = Some(c);
        if c != '/' || !opens {
            continue;
        }
        let mut end = i + 1;
        while let Some(&(j, n)) = it.peek() {
            if !name_char(n) {
                break;
            }
            end = j + n.len_utf8();
            prev = Some(n);
            it.next();
        }
        let after = text[end..].chars().next();
        let mut after2 = text[end..].chars().skip(1);
        let path = after == Some('/') || (after == Some('.') && after2.next().is_some_and(|c| c.is_alphanumeric()));
        let name = text[i + 1..end].trim_end_matches([':', '-']);
        if !path && name.chars().next().is_some_and(|c| c.is_alphanumeric()) {
            out.push((i, i + 1 + name.len()));
        }
    }
    out
}

/// The slash token the caret is in or just after, while one is being
/// typed: where it starts, where its name ends, and what of the name lies
/// before the caret.
pub fn slash_token_at(text: &str, caret: usize) -> Option<(usize, usize, String)> {
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let back = text[..caret].chars().rev().take_while(|c| name_char(*c)).map(char::len_utf8).sum::<usize>();
    let name_start = caret - back;
    let start = name_start.checked_sub(1).filter(|s| text.as_bytes()[*s] == b'/')?;
    if text[..start].chars().next_back().is_some_and(|p| !p.is_whitespace() && !"([{\"'".contains(p)) {
        return None;
    }
    let end = caret + text[caret..].chars().take_while(|c| name_char(*c)).map(char::len_utf8).sum::<usize>();
    Some((start, end, text[name_start..caret].to_string()))
}

/// `text` as markdown with each command `known` names set as inline code,
/// which is how the conversation colours it. Code is left alone: fenced
/// blocks, and anything already between backticks on its line.
pub fn mark_commands(text: &str, known: impl Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push_str(line);
            continue;
        }
        if fenced || line.starts_with("    ") || line.starts_with('\t') {
            out.push_str(line);
            continue;
        }
        let mut at = 0;
        for (a, b) in slash_tokens(line) {
            let in_code = line[..a].matches('`').count() % 2 == 1;
            if in_code || !known(&line[a + 1..b]) {
                continue;
            }
            out.push_str(&line[at..a]);
            out.push('`');
            out.push_str(&line[a..b]);
            out.push('`');
            at = b;
        }
        out.push_str(&line[at..]);
    }
    out
}

/// What the child said it can do, from `initialize` and `system/init`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Caps {
    /// `claude --version`, for the record and for the newer-than-tested note.
    pub version: String,
    pub model: String,
    pub mode: String,
    pub commands: Vec<CommandInfo>,
    pub slash_commands: Vec<String>,
    pub terminal_commands: Vec<String>,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TurnResult {
    pub subtype: String,
    pub is_error: bool,
    pub duration_ms: u64,
    pub cost_usd: f64,
    pub queued: usize,
    /// `modelUsage` from the frame: per model, tokens and `contextWindow`.
    pub model_usage: Value,
}

/// A `can_use_tool` request waiting on the person.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub input: Map<String, Value>,
    pub description: String,
    pub asked_at: f64,
}

impl PermissionRequest {
    /// Whether this is the agent asking a question rather than for leave
    /// to run a tool: `AskUserQuestion` comes down the same wire, and the
    /// answer goes back in the input (`Driver::answer_question`).
    pub fn is_question(&self) -> bool {
        self.tool_name == "AskUserQuestion"
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Init(Caps),
    Turn { text: String, queued: usize },
    Result(TurnResult),
    Mode(String),
    /// The `rate_limit_info` of a `rate_limit_event` frame: the account's
    /// five-hour and seven-day windows.
    RateLimit(Value),
    Permission(PermissionRequest),
    PermissionSettled(String),
    Exit { code: Option<i32>, error: String },
}

#[derive(Debug, Clone)]
pub struct DriverError(pub String);

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DriverError {}

/// What the Claude Code executable is called. The native installer ships
/// `claude.exe` on Windows; npm's global install leaves a `claude.cmd` shim,
/// which `Command` runs through `cmd.exe` on its own.
fn binary_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["claude.exe", "claude.cmd", "claude"]
    } else {
        &["claude"]
    }
}

/// The Claude Code binary, or None. `EMAKI_CLAUDE` wins, for tests; then
/// `PATH`, then where the installers put it when `PATH` does not say.
pub fn claude_binary() -> Option<PathBuf> {
    if let Ok(o) = std::env::var("EMAKI_CLAUDE") {
        if !o.is_empty() {
            return Some(PathBuf::from(o));
        }
    }
    let configured = crate::config::Config::load().driver.claude_path;
    if !configured.is_empty() {
        let p = crate::paths::expand_tilde(&configured);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let home = crate::paths::home();
    dirs.push(home.join(".local").join("bin"));
    dirs.push(home.join(".claude").join("local"));
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
    } else {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    for dir in dirs {
        for name in binary_names() {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// `claude --version` as a dotted number, asked once per process. Empty when
/// there is no binary or it did not answer.
pub fn claude_version() -> String {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| {
            claude_binary()
                .and_then(|b| Command::new(b).arg("--version").output().ok())
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().unwrap_or("").to_string())
                .unwrap_or_default()
        })
        .clone()
}

/// Dotted version numbers compared part by part; anything unparsable is 0.
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| -> Vec<u64> { s.split('.').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

/// The environment for a Claude Code child that must be its own session. The
/// app may itself be a grandchild of a session (started from a hook) and
/// would otherwise hand the child its parent's id, inbox and token.
pub fn child_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| !(k.starts_with("CLAUDE") && k != "CLAUDE_CONFIG_DIR"))
        .filter(|(k, _)| k != "EMAKI_DISABLE")
        .collect()
}

pub fn image_block(path: &Path, media_type: &str) -> Option<Value> {
    if !IMAGE_TYPES.contains(&media_type) {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    Some(json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": base64_encode(&data)}}))
}

/// The inverse of `base64_encode`, for image blocks read back out of a
/// transcript. Whitespace is skipped; anything else invalid yields `None`.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in text.as_bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

struct Queued {
    text: String,
    frame: Value,
}

struct Inner {
    session_id: String,
    state: State,
    caps: Caps,
    mode: String,
    model: String,
    idle_since: Instant,
    turn_started: Option<Instant>,
    turns: u64,
    queue: VecDeque<Queued>,
    waiting: HashMap<String, Sender<Value>>,
    pending_permissions: HashMap<String, PermissionRequest>,
    stderr_tail: VecDeque<String>,
    error: String,
    exit_code: Option<i32>,
    exited: bool,
}

impl Inner {
    fn new(session_id: &str, mode: &str, model: &str, version: String) -> Self {
        Inner {
            session_id: session_id.into(),
            state: State::Starting,
            caps: Caps { version, mode: mode.into(), model: model.into(), ..Default::default() },
            mode: mode.into(),
            model: model.into(),
            idle_since: Instant::now(),
            turn_started: None,
            turns: 0,
            queue: VecDeque::new(),
            waiting: HashMap::new(),
            pending_permissions: HashMap::new(),
            stderr_tail: VecDeque::new(),
            error: String::new(),
            exit_code: None,
            exited: false,
        }
    }
}

pub struct Driver {
    pub session_id: String,
    pub cwd: String,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<std::process::ChildStdin>>,
    inner: Arc<Mutex<Inner>>,
    events: Sender<Event>,
}

fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn short_id() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}{:x}", n, std::process::id())
}

impl Driver {
    /// Start a child on `session_id`. `resume` appends to an existing
    /// transcript; otherwise a fresh session with that id is created.
    pub fn start(
        session_id: &str,
        cwd: &str,
        resume: bool,
        mode: &str,
        model: &str,
        events: Sender<Event>,
    ) -> Result<Arc<Driver>, DriverError> {
        if !cwd.is_empty() && !Path::new(cwd).is_dir() {
            return Err(DriverError(format!("the session's directory is gone: {cwd}")));
        }
        let binary = claude_binary().ok_or_else(|| DriverError("claude is not on PATH".into()))?;
        let version = claude_version();
        if !version.is_empty() && version_cmp(&version, TESTED_CLAUDE_VERSION) == std::cmp::Ordering::Greater {
            eprintln!("emaki: Claude Code {version} is newer than {TESTED_CLAUDE_VERSION}, the release this driver was checked against; if turns stop arriving, that is the first suspect");
        }
        let mut cmd = Command::new(binary);
        cmd.args(["-p", "--verbose", "--input-format", "stream-json", "--output-format", "stream-json", "--permission-prompt-tool", "stdio"]);
        if resume {
            cmd.args(["--resume", session_id]);
        } else {
            cmd.args(["--session-id", session_id]);
        }
        if !mode.is_empty() {
            cmd.args(["--permission-mode", if mode == "default" { "manual" } else { mode }]);
        }
        if !model.is_empty() && model != "default" {
            cmd.args(["--model", model]);
        }
        if !cwd.is_empty() {
            cmd.current_dir(cwd);
        }
        cmd.env_clear();
        for (k, v) in child_env() {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| DriverError(format!("could not start claude: {e}")))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let driver = Arc::new(Driver {
            session_id: session_id.into(),
            cwd: cwd.into(),
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(stdin),
            inner: Arc::new(Mutex::new(Inner::new(session_id, mode, model, version))),
            events,
        });

        if let Some(out) = stdout {
            let d = Arc::clone(&driver);
            thread::Builder::new()
                .name(format!("emaki-driver-{}", &session_id[..session_id.len().min(8)]))
                .spawn(move || d.read_loop(out))
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

        match driver.request("initialize", Map::new(), Duration::from_secs(15)) {
            Ok(reply) => driver.absorb_initialize(&reply),
            Err(e) => {
                driver.stop();
                return Err(DriverError(format!("claude did not answer: {e}")));
            }
        }
        {
            let mut g = driver.inner.lock().unwrap();
            if g.state == State::Starting {
                g.state = State::Idle;
                g.idle_since = Instant::now();
            }
        }
        Ok(driver)
    }

    pub fn state(&self) -> State {
        self.inner.lock().unwrap().state
    }

    pub fn alive(&self) -> bool {
        !self.inner.lock().unwrap().exited
    }

    pub fn caps(&self) -> Caps {
        self.inner.lock().unwrap().caps.clone()
    }

    pub fn mode(&self) -> String {
        self.inner.lock().unwrap().mode.clone()
    }

    pub fn model(&self) -> String {
        self.inner.lock().unwrap().model.clone()
    }

    pub fn error(&self) -> String {
        self.inner.lock().unwrap().error.clone()
    }

    pub fn idle_for(&self) -> Duration {
        let g = self.inner.lock().unwrap();
        if g.state != State::Idle {
            return Duration::ZERO;
        }
        g.idle_since.elapsed()
    }

    pub fn turn_elapsed(&self) -> Option<Duration> {
        self.inner.lock().unwrap().turn_started.map(|t| t.elapsed())
    }

    pub fn queued(&self) -> Vec<String> {
        self.inner.lock().unwrap().queue.iter().map(|q| q.text.clone()).collect()
    }

    pub fn drop_queued(&self, index: usize) -> bool {
        let mut g = self.inner.lock().unwrap();
        g.queue.remove(index).is_some()
    }

    pub fn pending_permissions(&self) -> Vec<PermissionRequest> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<PermissionRequest> = g.pending_permissions.values().cloned().collect();
        v.sort_by(|a, b| a.asked_at.partial_cmp(&b.asked_at).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    /// Close stdin and let the child finish; kill it if it will not.
    pub fn stop(&self) {
        {
            let mut s = self.stdin.lock().unwrap();
            *s = None;
        }
        let mut child = self.child.lock().unwrap();
        if let Some(mut c) = child.take() {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match c.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                    _ => {
                        let _ = c.kill();
                        let _ = c.wait();
                        break;
                    }
                }
            }
            let code = c.try_wait().ok().flatten().and_then(|s| s.code());
            self.mark_exited(code);
        }
    }

    // -- sending ---------------------------------------------------------

    /// Queue or write one user turn. Returns whether it was queued.
    pub fn send(&self, text: &str, images: Vec<Value>) -> Result<bool, DriverError> {
        let text = text.trim_end();
        if text.is_empty() && images.is_empty() {
            return Err(DriverError("empty message".into()));
        }
        if !self.alive() {
            let e = self.error();
            return Err(DriverError(if e.is_empty() { "claude has exited".into() } else { e }));
        }
        let mut content = Vec::new();
        if !text.is_empty() {
            content.push(json!({"type": "text", "text": text}));
        }
        content.extend(images);
        let frame = json!({
            "type": "user",
            "message": {"role": "user", "content": content},
            "parent_tool_use_id": null,
            "session_id": self.session_id,
        });
        {
            let mut g = self.inner.lock().unwrap();
            if g.state == State::Running {
                g.queue.push_back(Queued { text: text.into(), frame });
                return Ok(true);
            }
            g.state = State::Running;
            g.turn_started = Some(Instant::now());
            g.turns += 1;
        }
        self.write(&frame)?;
        Ok(false)
    }

    pub fn request(&self, subtype: &str, mut fields: Map<String, Value>, timeout: Duration) -> Result<Value, DriverError> {
        if !self.alive() {
            return Err(DriverError(self.error_or("claude has exited")));
        }
        let rid = short_id();
        let (tx, rx) = mpsc::channel();
        self.inner.lock().unwrap().waiting.insert(rid.clone(), tx);
        fields.insert("subtype".into(), Value::String(subtype.into()));
        self.write(&json!({"type": "control_request", "request_id": rid, "request": fields}))?;
        match rx.recv_timeout(timeout) {
            Ok(reply) => {
                if str_of(&reply, "subtype") == "error" {
                    let e = str_of(&reply, "error");
                    return Err(DriverError(if e.is_empty() { format!("{subtype} failed") } else { e.into() }));
                }
                Ok(reply.get("response").cloned().unwrap_or(Value::Object(Map::new())))
            }
            Err(_) => {
                self.inner.lock().unwrap().waiting.remove(&rid);
                Err(DriverError(format!("{subtype}: no answer in {}s", timeout.as_secs())))
            }
        }
    }

    pub fn interrupt(&self) -> Result<(), DriverError> {
        self.request("interrupt", Map::new(), REQUEST_TIMEOUT).map(|_| ())
    }

    pub fn set_mode(&self, mode: &str) -> Result<String, DriverError> {
        if !MODES.contains(&mode) {
            return Err(DriverError(format!("unknown permission mode: {mode}")));
        }
        let mut f = Map::new();
        f.insert("mode".into(), Value::String(mode.into()));
        let reply = self.request("set_permission_mode", f, REQUEST_TIMEOUT)?;
        let now = str_of(&reply, "mode");
        let now = if now.is_empty() { mode.to_string() } else { now.to_string() };
        let mut g = self.inner.lock().unwrap();
        g.caps.mode = now.clone();
        g.mode = now.clone();
        Ok(now)
    }

    pub fn set_model(&self, model: &str) -> Result<(), DriverError> {
        let mut f = Map::new();
        f.insert("model".into(), if model.is_empty() || model == "default" { Value::Null } else { Value::String(model.into()) });
        self.request("set_model", f, REQUEST_TIMEOUT)?;
        let mut g = self.inner.lock().unwrap();
        g.model = model.into();
        g.caps.model = if model == "default" { String::new() } else { model.into() };
        Ok(())
    }

    /// Set the effort level. There is no control request for it (2.1.284
    /// answers "Unsupported control request subtype: set_effort"), but
    /// `/effort <level>` as a user turn is run as the local command it is,
    /// and the transcript records it, which is where the pill reads it back.
    pub fn set_effort(&self, effort: &str) -> Result<bool, DriverError> {
        if !EFFORTS.contains(&effort) {
            return Err(DriverError(format!("unknown effort level: {effort}")));
        }
        self.send(&format!("/effort {effort}"), Vec::new())
    }

    /// Answer a `can_use_tool` card. Unknown ids are ignored.
    pub fn answer_permission(&self, request_id: &str, allow: bool, message: &str) {
        let req = self.inner.lock().unwrap().pending_permissions.remove(request_id);
        let Some(req) = req else { return };
        let decision = if allow {
            json!({"behavior": "allow", "updatedInput": req.input})
        } else {
            json!({"behavior": "deny", "message": if message.is_empty() { "emaki: denied" } else { message }})
        };
        let _ = self.write(&json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": decision}}));
        let _ = self.events.send(Event::PermissionSettled(request_id.into()));
    }

    /// Answer an `AskUserQuestion` card: the call is allowed with its input
    /// completed by `answers`, question text to the label chosen (or the
    /// words typed), which is how Claude Code's own dialog answers it. An
    /// empty map is "the user did not answer". Unknown ids are ignored.
    pub fn answer_question(&self, request_id: &str, answers: Map<String, Value>) {
        let req = self.inner.lock().unwrap().pending_permissions.remove(request_id);
        let Some(req) = req else { return };
        let decision = question_decision(&req.input, answers);
        let _ = self.write(&json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": decision}}));
        let _ = self.events.send(Event::PermissionSettled(request_id.into()));
    }

    // -- the wire ----------------------------------------------------------

    fn error_or(&self, fallback: &str) -> String {
        let e = self.error();
        if e.is_empty() { fallback.into() } else { e }
    }

    fn write(&self, frame: &Value) -> Result<(), DriverError> {
        let mut line = serde_json::to_string(frame).map_err(|e| DriverError(e.to_string()))?;
        line.push('\n');
        let mut guard = self.stdin.lock().unwrap();
        let Some(stdin) = guard.as_mut() else {
            return Err(DriverError(self.error_or("claude has exited")));
        };
        if let Err(e) = stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()) {
            let msg = format!("claude closed its input: {e}");
            {
                let mut g = self.inner.lock().unwrap();
                if g.error.is_empty() {
                    g.error = msg.clone();
                }
            }
            drop(guard);
            self.mark_exited(None);
            return Err(DriverError(msg));
        }
        Ok(())
    }

    fn read_loop(self: Arc<Self>, out: std::process::ChildStdout) {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(frame) = serde_json::from_str::<Value>(line) else { continue };
            if frame.is_object() {
                self.on_frame(frame);
            }
        }
        let code = {
            let mut child = self.child.lock().unwrap();
            child.as_mut().and_then(|c| c.try_wait().ok().flatten()).and_then(|s| s.code())
        };
        self.mark_exited(code);
    }

    fn on_frame(self: &Arc<Self>, frame: Value) {
        match str_of(&frame, "type") {
            "control_response" => {
                let reply = frame.get("response").cloned().unwrap_or(Value::Null);
                let rid = str_of(&reply, "request_id").to_string();
                let waiter = self.inner.lock().unwrap().waiting.remove(&rid);
                if let Some(tx) = waiter {
                    let _ = tx.send(reply);
                }
            }
            "control_request" => {
                let request = frame.get("request").cloned().unwrap_or(Value::Null);
                let rid = str_of(&frame, "request_id").to_string();
                if str_of(&request, "subtype") == "can_use_tool" {
                    let req = PermissionRequest {
                        request_id: rid.clone(),
                        tool_name: str_of(&request, "tool_name").into(),
                        tool_use_id: str_of(&request, "tool_use_id").into(),
                        input: request.get("input").and_then(Value::as_object).cloned().unwrap_or_default(),
                        description: str_of(&request, "description").into(),
                        asked_at: now_secs(),
                    };
                    self.inner.lock().unwrap().pending_permissions.insert(rid.clone(), req.clone());
                    let _ = self.events.send(Event::Permission(req));
                    let d = Arc::clone(self);
                    thread::spawn(move || {
                        thread::sleep(PERMISSION_TIMEOUT);
                        if d.inner.lock().unwrap().pending_permissions.contains_key(&rid) {
                            d.answer_permission(&rid, false, "emaki: nobody answered");
                        }
                    });
                } else {
                    let _ = self.write(&json!({"type": "control_response", "response": {"subtype": "error", "request_id": rid, "error": format!("emaki does not handle {}", str_of(&request, "subtype"))}}));
                }
            }
            "system" => match str_of(&frame, "subtype") {
                "init" => {
                    self.absorb_init(&frame);
                    let caps = self.caps();
                    let _ = self.events.send(Event::Init(caps));
                }
                "status" if !str_of(&frame, "permissionMode").is_empty() => {
                    let mode = str_of(&frame, "permissionMode").to_string();
                    {
                        let mut g = self.inner.lock().unwrap();
                        g.caps.mode = mode.clone();
                        g.mode = mode.clone();
                    }
                    let _ = self.events.send(Event::Mode(mode));
                }
                _ => {}
            },
            "result" => self.end_turn(&frame),
            "rate_limit_event" => {
                if let Some(info) = frame.get("rate_limit_info") {
                    let _ = self.events.send(Event::RateLimit(info.clone()));
                }
            }
            _ => {}
        }
    }

    fn absorb_initialize(&self, reply: &Value) {
        let mut commands = Vec::new();
        for item in arr_of(reply, "commands") {
            let name = str_of(item, "name");
            if name.is_empty() {
                continue;
            }
            let hint = str_of(item, "argumentHint");
            let hint = if hint.is_empty() { str_of(item, "argument_hint") } else { hint };
            commands.push(CommandInfo { name: name.into(), description: str_of(item, "description").into(), argument_hint: hint.into(), builtin: item.get("builtin").and_then(Value::as_bool).unwrap_or(false) });
        }
        let mut g = self.inner.lock().unwrap();
        if !commands.is_empty() {
            if g.caps.slash_commands.is_empty() {
                g.caps.slash_commands = commands.iter().map(|c| c.name.clone()).collect();
            }
            g.caps.commands = commands;
        }
        if !str_of(reply, "model").is_empty() {
            g.caps.model = str_of(reply, "model").into();
        }
    }

    fn absorb_init(&self, frame: &Value) {
        let mut g = self.inner.lock().unwrap();
        if !str_of(frame, "model").is_empty() {
            g.caps.model = str_of(frame, "model").into();
        }
        if !str_of(frame, "permissionMode").is_empty() {
            g.caps.mode = str_of(frame, "permissionMode").into();
            g.mode = g.caps.mode.clone();
        }
        let strings = |v: &Value, k: &str| -> Option<Vec<String>> {
            v.get(k).and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect())
        };
        if let Some(v) = strings(frame, "slash_commands") {
            g.caps.slash_commands = v;
        }
        if let Some(v) = strings(frame, "terminal_slash_commands") {
            g.caps.terminal_commands = v;
        }
        if let Some(v) = strings(frame, "skills") {
            g.caps.skills = v;
        }
        if let Some(v) = strings(frame, "agents") {
            g.caps.agents = v;
        }
        if let Some(v) = strings(frame, "tools") {
            g.caps.tools = v;
        }
    }

    fn end_turn(&self, result: &Value) {
        let next;
        let queued_len;
        let tr = TurnResult {
            subtype: str_of(result, "subtype").into(),
            is_error: bool_of(result, "is_error"),
            duration_ms: u64_of(result, "duration_ms"),
            cost_usd: result.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
            queued: 0,
            model_usage: result.get("modelUsage").cloned().unwrap_or(Value::Null),
        };
        {
            let mut g = self.inner.lock().unwrap();
            next = g.queue.pop_front();
            if next.is_some() {
                g.state = State::Running;
                g.turn_started = Some(Instant::now());
                g.turns += 1;
            } else {
                g.state = State::Idle;
                g.idle_since = Instant::now();
                g.turn_started = None;
            }
            queued_len = g.queue.len();
        }
        let _ = self.events.send(Event::Result(TurnResult { queued: queued_len, ..tr }));
        if let Some(q) = next {
            let _ = self.write(&q.frame);
            let _ = self.events.send(Event::Turn { text: q.text, queued: queued_len });
        }
    }

    fn mark_exited(&self, code: Option<i32>) {
        let waiting;
        let error;
        {
            let mut g = self.inner.lock().unwrap();
            if g.exited {
                return;
            }
            g.exited = true;
            g.state = State::Exited;
            g.exit_code = code;
            if g.error.is_empty() && code.unwrap_or(0) != 0 {
                if let Some(last) = g.stderr_tail.back() {
                    g.error = last.clone();
                }
            }
            error = g.error.clone();
            waiting = std::mem::take(&mut g.waiting);
            g.pending_permissions.clear();
            let _ = &g.session_id;
        }
        for (_, tx) in waiting {
            let _ = tx.send(json!({"subtype": "error", "error": if error.is_empty() { "claude exited" } else { &error }}));
        }
        let _ = self.events.send(Event::Exit { code, error });
    }
}

/// The commands a session in `cwd` would know: built-ins, bundled skills,
/// the person's own skills and command files, plugins. Read from a headless
/// child's `initialize` reply, which costs no model call and leaves no
/// transcript (checked against 2.1.288: nothing is written until a message
/// is sent), about half a second. The child is stopped at once.
pub fn catalogue(cwd: &str) -> Result<Vec<CommandInfo>, DriverError> {
    let (tx, _rx) = mpsc::channel();
    let id = throwaway_session_id();
    let d = Driver::start(&id, cwd, false, "", "", tx)?;
    let commands = d.caps().commands;
    d.stop();
    Ok(commands)
}

/// A fresh v4-shaped session id for a child that will never write a row.
fn throwaway_session_id() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut h = sha1_smol::Sha1::new();
    h.update(nanos.to_string().as_bytes());
    h.update(std::process::id().to_string().as_bytes());
    h.update(b"catalogue");
    let x = h.digest().to_string();
    format!("{}-{}-4{}-a{}-{}", &x[..8], &x[8..12], &x[13..16], &x[17..20], &x[20..32])
}

/// The `can_use_tool` reply that answers a question: allow, with the
/// input the call was made with and the answers beside its questions.
pub fn question_decision(input: &Map<String, Value>, answers: Map<String, Value>) -> Value {
    let mut input = input.clone();
    input.insert("answers".into(), Value::Object(answers));
    json!({"behavior": "allow", "updatedInput": input})
}

impl Drop for Driver {
    fn drop(&mut self) {
        if let Ok(mut c) = self.child.lock() {
            if let Some(mut child) = c.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A driver with no child behind it: frames go in through `on_frame`,
    /// events come out of the channel, writes fail as "exited".
    fn detached() -> (Arc<Driver>, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let d = Arc::new(Driver {
            session_id: "sid-1".into(),
            cwd: String::new(),
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            inner: Arc::new(Mutex::new(Inner::new("sid-1", "", "", String::new()))),
            events: tx,
        });
        (d, rx)
    }

    /// Frames as Claude Code 2.1.283 writes them (`--verbose`, stream-json).
    /// If a release changes a key we read, this is the test that goes red.
    #[test]
    fn slash_commands_in_a_sentence() {
        let text = "please use my /ph-image skill, then /compact. See /usr/bin and a/b, (/model) /x.rs /review: ok";
        let names: Vec<&str> = slash_tokens(text).into_iter().map(|(a, b)| &text[a..b]).collect();
        assert_eq!(names, vec!["/ph-image", "/compact", "/model", "/review"]);
        assert!(acts_alone("compact") && acts_alone("model") && !acts_alone("ph-image") && !acts_alone("code-review"));
        assert_eq!(slash_token_at("use /ph-im now", 10), Some((4, 10, "ph-im".into())));
        assert_eq!(slash_token_at("use /ph-im now", 7), Some((4, 10, "ph".into())));
        assert_eq!(slash_token_at("/mod", 4), Some((0, 4, "mod".into())));
        assert_eq!(slash_token_at("a/mod", 5), None);
        assert_eq!(slash_token_at("use /ph-im now", 11), None);
        let known = |n: &str| n == "ph-image" || n == "compact";
        assert_eq!(mark_commands("use /ph-image and `/compact` or /nope\n```\n/compact\n```\n/compact", known), "use `/ph-image` and `/compact` or /nope\n```\n/compact\n```\n`/compact`");
    }

    #[test]
    fn frames_from_the_recorded_wire() {
        let (d, rx) = detached();
        d.on_frame(json!({
            "type": "system", "subtype": "init", "cwd": "/tmp/p", "session_id": "sid-1",
            "tools": ["Bash", "Read", "Edit"], "mcp_servers": [], "model": "claude-fable-5-1",
            "permissionMode": "acceptEdits", "slash_commands": ["compact", "review"],
            "skills": ["ph-app"], "agents": ["Explore"], "apiKeySource": "none",
            "output_style": "default", "uuid": "u-1"
        }));
        match rx.recv().unwrap() {
            Event::Init(caps) => {
                assert_eq!(caps.model, "claude-fable-5-1");
                assert_eq!(caps.mode, "acceptEdits");
                assert_eq!(caps.slash_commands, vec!["compact", "review"]);
                assert_eq!(caps.tools, vec!["Bash", "Read", "Edit"]);
            }
            other => panic!("expected Init, got {other:?}"),
        }

        d.on_frame(json!({
            "type": "control_request", "request_id": "req-7",
            "request": {"subtype": "can_use_tool", "tool_name": "Bash", "tool_use_id": "toolu_1",
                        "input": {"command": "ls"}, "description": "List files"}
        }));
        match rx.recv().unwrap() {
            Event::Permission(p) => {
                assert_eq!(p.request_id, "req-7");
                assert_eq!(p.tool_name, "Bash");
                assert_eq!(p.input.get("command").and_then(Value::as_str), Some("ls"));
            }
            other => panic!("expected Permission, got {other:?}"),
        }
        assert!(d.inner.lock().unwrap().pending_permissions.contains_key("req-7"));

        // A question comes down the same wire and is answered in its input.
        d.on_frame(json!({
            "type": "control_request", "request_id": "req-8",
            "request": {"subtype": "can_use_tool", "tool_name": "AskUserQuestion", "tool_use_id": "toolu_2",
                        "input": {"questions": [{"question": "Tea or coffee?", "header": "Drink", "options": [{"label": "Tea", "description": ""}, {"label": "Coffee", "description": ""}], "multiSelect": false}]}}
        }));
        match rx.recv().unwrap() {
            Event::Permission(p) => {
                assert!(p.is_question());
                let mut answers = Map::new();
                answers.insert("Tea or coffee?".into(), Value::String("Tea".into()));
                let decision = question_decision(&p.input, answers.clone());
                assert_eq!(decision["behavior"], "allow");
                assert_eq!(decision["updatedInput"]["answers"]["Tea or coffee?"], "Tea");
                assert_eq!(decision["updatedInput"]["questions"][0]["question"], "Tea or coffee?");
                d.answer_question("req-8", answers);
            }
            other => panic!("expected Permission, got {other:?}"),
        }
        assert!(matches!(rx.recv().unwrap(), Event::PermissionSettled(id) if id == "req-8"));
        assert!(!d.inner.lock().unwrap().pending_permissions.contains_key("req-8"));

        d.on_frame(json!({"type": "system", "subtype": "status", "permissionMode": "plan"}));
        assert!(matches!(rx.recv().unwrap(), Event::Mode(m) if m == "plan"));

        d.on_frame(json!({
            "type": "result", "subtype": "success", "is_error": false, "duration_ms": 1234,
            "duration_api_ms": 1000, "num_turns": 1, "result": "done", "session_id": "sid-1",
            "total_cost_usd": 0.0123, "usage": {"input_tokens": 1, "output_tokens": 2}, "uuid": "u-2"
        }));
        let result = loop {
            match rx.recv().unwrap() {
                Event::Result(r) => break r,
                _ => continue,
            }
        };
        assert_eq!(result.subtype, "success");
        assert!(!result.is_error);
        assert_eq!(result.duration_ms, 1234);
        assert!((result.cost_usd - 0.0123).abs() < 1e-9);
    }

    #[test]
    fn versions_compare_by_part() {
        use std::cmp::Ordering::*;
        assert_eq!(version_cmp("2.1.283", "2.1.283"), Equal);
        assert_eq!(version_cmp("2.1.290", "2.1.283"), Greater);
        assert_eq!(version_cmp("2.2", "2.1.283"), Greater);
        assert_eq!(version_cmp("1.9.9", "2.1.283"), Less);
        assert_eq!(version_cmp("", "2.1.283"), Less);
    }
}
