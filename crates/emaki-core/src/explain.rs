//! Plain-English explanations of opaque tool calls.
//!
//! A `python3 - <<'EOF'` heredoc or a piped shell chain is not something a
//! person can evaluate at a glance. This turns it into one or two sentences
//! next to the call, ideally while you are still deciding whether to approve
//! it. Three rules keep it cheap and safe:
//!
//! * **Cheap calls never cost anything.** A `Read` explains itself from its
//!   own arguments; only genuinely opaque calls reach a model.
//! * **Answers are cached by content.** Approving `git status` once explains
//!   it forever, across sessions and projects.
//! * **Nothing ever blocks.** The model runs on a thread and its answer is
//!   handed back through a callback whenever it lands.
//!
//! The model is asked through the `claude` binary itself (`claude -p
//! --model claude-haiku-4-5`), so it runs on the person's own subscription
//! with no key and no HTTP client of ours. The child is a real Claude Code
//! session: it runs with `cwd` under `~/.emaki/run/explain` so the index
//! recognises and drops its transcripts (`paths::is_explainer_cwd`), with no
//! settings, no MCP servers and no tools, and never with `--bare`, which
//! reads auth only from `ANTHROPIC_API_KEY` and breaks subscription users.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Map, Value};

use crate::build::tool_subject;
use crate::config::Explain;
use crate::driver;
use crate::model::{Item, Round, Session};
use crate::paths;

pub const SYSTEM_PROMPT: &str = "You explain one action an AI coding agent wants to take. Your reader is \
deciding whether to allow it and may not be a programmer. Reply with one \
short plain sentence, two at most, under 30 words, and nothing else: no \
preamble, no markdown, no code, no quotes. Use everyday words; name a file \
only when the reader needs it. Say what the action does and, when it \
matters, what it changes: files written or deleted, anything sent over the \
network, anything installed, anything hard to undo. If it only reads or \
looks, say so. Describe only what is shown. Do not mention that the text \
is cut off, do not guess at the wider intent, and do not judge whether to \
allow it.";

/// Shapes that are opaque regardless of length: the argument text alone does
/// not tell you what will happen.
pub const OPAQUE_MARKERS: &[&str] = &[
    "<<", "-c '", "-c \"", "-e '", "-e \"", "eval ", "base64", "| sh", "|sh", "| bash", "|bash", "rm -rf", "sudo ", "curl ", "wget ", "chmod ", "> /", "dd ", "mkfs", ":(){", "xargs ",
];

/// The child must be a single cheap completion and nothing else.
pub const DISALLOWED_TOOLS: &[&str] = &["Bash", "Read", "Edit", "Write", "Glob", "Grep", "WebFetch", "WebSearch", "Task", "Agent", "NotebookEdit", "Skill"];

/// If the CLI answers with one of these, it is reporting its own failure
/// rather than explaining anything. Rendering it as an explanation would be
/// worse than rendering nothing.
const ERROR_HINTS: &[&str] = &["please run /login", "not logged in", "invalid api key", "credit balance is too low", "usage limit reached", "authentication_error", "command not found"];

const CANNED: &[(&str, &str)] = &[
    ("Read", "Reads {subject} without changing it."),
    ("NotebookRead", "Reads the notebook {subject} without changing it."),
    ("Glob", "Lists files matching {subject}. Read-only."),
    ("Grep", "Searches file contents for {subject}. Read-only."),
    ("TodoWrite", "Updates the agent's own task list. Touches nothing on disk."),
    ("TaskCreate", "Adds a task to the agent's task list."),
    ("TaskUpdate", "Updates a task in the agent's task list."),
    ("TaskList", "Reads the agent's task list."),
    ("TaskGet", "Reads one task from the agent's task list."),
    ("WebSearch", "Runs a web search for {subject}."),
    ("WebFetch", "Fetches {subject} and reads the page. Sends a request to that site."),
    ("Write", "Writes {subject}, replacing anything already there."),
    ("Edit", "Edits {subject} in place."),
    ("NotebookEdit", "Edits the notebook {subject} in place."),
    ("Agent", "Hands work to a subagent: {subject}."),
    ("Task", "Hands work to a subagent: {subject}."),
    ("Skill", "Loads the {subject} skill's instructions."),
    ("AskUserQuestion", "Asks you a question and waits for your answer."),
    ("ExitPlanMode", "Presents a plan for your approval and leaves plan mode."),
];

/// The cache is bounded: past this many entries the oldest go.
const CACHE_MAX: usize = 4000;
const CACHE_KEEP: usize = 3000;

/// Where the answers live. Insertion order is kept (serde_json preserves it),
/// which makes "oldest first" the trim order.
pub fn cache_file() -> PathBuf {
    paths::cache_dir().join("explanations.json")
}

/// Where explainer children run: a scratch directory inside our own root,
/// so the indexer recognises the path and drops the child's transcripts,
/// and `prune_transcripts` knows exactly which directory to sweep.
pub fn workdir() -> PathBuf {
    let path = paths::run_dir().join("explain");
    let _ = std::fs::create_dir_all(&path);
    path
}

/// A free one-liner built from the call's own arguments, or "".
pub fn canned(tool_name: &str, input: &Map<String, Value>, cwd: &str) -> String {
    let Some((_, template)) = CANNED.iter().find(|(n, _)| *n == tool_name) else {
        if tool_name.starts_with("mcp__") {
            return format!("Calls the {tool_name} MCP tool.");
        }
        return String::new();
    };
    let subject = tool_subject(tool_name, input, cwd);
    if template.contains("{subject}") && subject.is_empty() {
        return String::new();
    }
    template.replace("{subject}", &if subject.is_empty() { String::new() } else { format!("`{subject}`") })
}

/// Whether this call is opaque enough to be worth a model.
pub fn needs_model(cfg: &Explain, tool_name: &str, input: &Map<String, Value>) -> bool {
    if !cfg.active() {
        return false;
    }
    if cfg.canned_tools.iter().any(|t| t == tool_name) {
        return false;
    }
    let subject = subject_text(tool_name, input);
    if OPAQUE_MARKERS.iter().any(|m| subject.contains(m)) {
        return true;
    }
    subject.chars().count() >= cfg.min_chars
}

fn subject_text(tool_name: &str, input: &Map<String, Value>) -> String {
    if tool_name == "Bash" {
        return input.get("command").and_then(Value::as_str).unwrap_or("").to_string();
    }
    canonical(&Value::Object(input.clone()))
}

/// JSON with object keys sorted, so the same arguments hash the same
/// whatever order the transcript wrote them in.
fn canonical(v: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                let mut out = Map::new();
                for k in keys {
                    out.insert(k.clone(), sort(&m[k]));
                }
                Value::Object(out)
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(v)).unwrap_or_default()
}

/// The content address of a call: its name and its arguments, nothing else.
pub fn key_for(tool_name: &str, input: &Map<String, Value>) -> String {
    let blob = format!("{tool_name}\0{}", canonical(&Value::Object(input.clone())));
    sha1_smol::Sha1::from(blob.as_bytes()).digest().to_string()
}

/// The exact command line the explainer child runs. Separate from the spawn
/// so the isolation flags can be asserted without spawning anything:
/// getting these wrong is how the app ends up recursively logging its own
/// explainer.
pub fn build_argv(cfg: &Explain, tool_name: &str, input: &Map<String, Value>) -> Vec<String> {
    let model = if cfg.model.is_empty() { "claude-haiku-4-5" } else { cfg.model.as_str() };
    let mut argv: Vec<String> = vec![
        "-p".into(),
        "--model".into(),
        model.into(),
        // No user settings, no MCP servers, no tools: this must be a single
        // cheap completion and nothing else.
        "--setting-sources".into(),
        String::new(),
        "--strict-mcp-config".into(),
        "--disallowed-tools".into(),
    ];
    argv.extend(DISALLOWED_TOOLS.iter().map(|s| s.to_string()));
    argv.push("--system-prompt".into());
    argv.push(SYSTEM_PROMPT.into());
    argv.push(user_message(tool_name, input));
    argv
}

fn user_message(tool_name: &str, input: &Map<String, Value>) -> String {
    if tool_name == "Bash" {
        let command = input.get("command").and_then(Value::as_str).unwrap_or("");
        return format!("Tool: Bash\n\nCommand:\n{}", truncate(command, 4000));
    }
    let blob = serde_json::to_string_pretty(&Value::Object(input.clone())).unwrap_or_default();
    format!("Tool: {tool_name}\n\nArguments:\n{}", truncate(&blob, 4000))
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push_str("\n… (truncated)");
    out
}

/// Normalise the model's reply, or "" if it is not an explanation.
pub fn clean(text: &str) -> String {
    let mut out = text.trim().to_string();
    if out.is_empty() {
        return String::new();
    }
    let lowered = out.to_lowercase();
    if ERROR_HINTS.iter().any(|h| lowered.contains(h)) {
        return String::new();
    }
    if out.starts_with("```") {
        out = out.lines().filter(|l| !l.trim().starts_with("```")).collect::<Vec<_>>().join("\n").trim().to_string();
    }
    let chars: Vec<char> = out.chars().collect();
    if chars.len() >= 2 && chars[0] == chars[chars.len() - 1] && (chars[0] == '"' || chars[0] == '\'') {
        out = chars[1..chars.len() - 1].iter().collect::<String>().trim().to_string();
    }
    let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().take(600).collect()
}

/// Delete transcripts left behind by explainer children older than
/// `max_age`. Every explanation runs a real `claude -p`, and Claude Code
/// writes a transcript for it; invisible in the window, but they would pile
/// up in `~/.claude/projects` forever. Answers how many went.
pub fn prune_transcripts(max_age: Duration) -> usize {
    let mangled = workdir().to_string_lossy().replace(['/', '\\'], "-");
    let folder = paths::projects_dir().join(mangled);
    if !folder.is_dir() {
        return 0;
    }
    let cutoff = SystemTime::now().checked_sub(max_age).unwrap_or(SystemTime::UNIX_EPOCH);
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let old = entry.metadata().and_then(|m| m.modified()).map(|m| m < cutoff).unwrap_or(false);
            if old && std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    if std::fs::read_dir(&folder).map(|mut d| d.next().is_none()).unwrap_or(false) {
        let _ = std::fs::remove_dir(&folder);
    }
    removed
}

/// Run one small-model child with that command line and answer what it
/// printed, or nothing when it failed or took longer than `timeout`. The
/// child is isolated as every explainer child is: our scratch folder, no
/// `CLAUDE*` variable of a parent session, `EMAKI_DISABLE` set, and
/// whatever `env` adds.
pub fn run_child(argv: &[String], env: &[(&str, &str)], timeout: Duration) -> Option<String> {
    let claude = driver::claude_binary()?;
    let mut cmd = Command::new(claude);
    cmd.args(argv).current_dir(workdir()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    cmd.env_clear();
    cmd.envs(driver::child_env());
    cmd.env("EMAKI_DISABLE", "1");
    cmd.envs(env.iter().copied());
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // Read on a thread so the timeout can kill a child that never answers.
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = reader.join().unwrap_or_default();
    match status {
        Some(s) if s.success() => Some(String::from_utf8_lossy(&out).to_string()),
        _ => None,
    }
}

/// Whether what a child printed is Claude Code refusing to run, not an answer.
pub fn is_refusal(text: &str) -> bool {
    let lowered = text.to_lowercase();
    ERROR_HINTS.iter().any(|h| lowered.contains(h))
}

/// Locate a tool call anywhere in a session, subagents included.
pub fn find_call<'a>(rounds: &'a [Round], call_id: &str) -> Option<&'a crate::model::ToolCall> {
    for rnd in rounds {
        for item in &rnd.items {
            if let Item::Tool(call) = item {
                if call.id == call_id {
                    return Some(call);
                }
                if let Some(found) = find_call(&call.subagent, call_id) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Called with a call's id and its explanation once one lands.
pub type OnReady = Arc<dyn Fn(&str, &str) + Send + Sync>;

pub struct Explainer {
    cfg: RwLock<Explain>,
    cache: Mutex<Map<String, Value>>,
    inflight: Mutex<HashSet<String>>,
    on_ready: OnReady,
    /// When the child transcripts were last swept.
    pruned: Mutex<Option<Instant>>,
}

impl Explainer {
    pub fn new(cfg: Explain, on_ready: OnReady) -> Arc<Explainer> {
        let cache = paths::read_json(&cache_file())
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .map(|m| m.into_iter().filter(|(_, v)| v.is_string()).collect())
            .unwrap_or_default();
        Arc::new(Explainer { cfg: RwLock::new(cfg), cache: Mutex::new(cache), inflight: Mutex::new(HashSet::new()), on_ready, pruned: Mutex::new(None) })
    }

    pub fn cfg(&self) -> Explain {
        self.cfg.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// A settings change takes effect on the next request.
    pub fn set_cfg(&self, cfg: Explain) {
        *self.cfg.write().unwrap_or_else(|e| e.into_inner()) = cfg;
    }

    pub fn needs_model(&self, tool_name: &str, input: &Map<String, Value>) -> bool {
        needs_model(&self.cfg(), tool_name, input)
    }

    /// Cache-only read. Never spawns anything, never costs anything. Once a
    /// command has been explained it is explained everywhere it appears,
    /// in this session, in older ones and in the markdown.
    pub fn lookup(&self, tool_name: &str, input: &Map<String, Value>) -> String {
        let key = key_for(tool_name, input);
        self.cache.lock().map(|c| c.get(&key).and_then(Value::as_str).unwrap_or("").to_string()).unwrap_or_default()
    }

    /// Whether a model is at work on this call's arguments right now.
    pub fn in_flight(&self, tool_name: &str, input: &Map<String, Value>) -> bool {
        let key = key_for(tool_name, input);
        self.inflight.lock().map(|s| s.contains(&key)).unwrap_or(false)
    }

    /// Ask for an explanation of `call_id`. A cached answer is handed back
    /// at once through `on_ready`; otherwise a thread asks the model, unless
    /// the call is cheap and `force` is off. One request per distinct
    /// arguments is in flight at a time.
    pub fn request(self: &Arc<Self>, call_id: &str, tool_name: &str, input: &Map<String, Value>, force: bool) {
        if !force && !self.needs_model(tool_name, input) {
            return;
        }
        if !self.cfg().enabled {
            return;
        }
        let key = key_for(tool_name, input);
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(text) = cache.get(&key).and_then(Value::as_str) {
                (self.on_ready)(call_id, text);
                return;
            }
        }
        {
            let mut inflight = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            if !inflight.insert(key.clone()) {
                return;
            }
        }
        let me = Arc::clone(self);
        let call_id = call_id.to_string();
        let tool_name = tool_name.to_string();
        let input = input.clone();
        thread::Builder::new()
            .name("emaki-explain".into())
            .spawn(move || {
                let text = me.ask_model(&tool_name, &input);
                me.inflight.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
                if text.is_empty() {
                    (me.on_ready)(&call_id, "");
                    return;
                }
                me.remember(key, &text);
                (me.on_ready)(&call_id, &text);
                me.prune_occasionally();
            })
            .ok();
    }

    fn remember(&self, key: String, text: &str) {
        let snapshot = {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.insert(key, Value::String(text.to_string()));
            if cache.len() > CACHE_MAX {
                let drop = cache.len() - CACHE_KEEP;
                *cache = cache.iter().skip(drop).map(|(k, v)| (k.clone(), v.clone())).collect();
            }
            cache.clone()
        };
        let _ = paths::ensure_dirs();
        let _ = paths::write_json(&cache_file(), &Value::Object(snapshot));
    }

    /// Sweep the child transcripts at most once an hour.
    fn prune_occasionally(&self) {
        let mut last = self.pruned.lock().unwrap_or_else(|e| e.into_inner());
        if last.map(|t| t.elapsed() < Duration::from_secs(3600)).unwrap_or(false) {
            return;
        }
        *last = Some(Instant::now());
        prune_transcripts(Duration::from_secs(3600));
    }

    fn ask_model(&self, tool_name: &str, input: &Map<String, Value>) -> String {
        let cfg = self.cfg();
        let argv = build_argv(&cfg, tool_name, input);
        run_child(&argv, &[], Duration::from_secs(cfg.timeout_s.max(1))).map(|out| clean(&out)).unwrap_or_default()
    }

    /// Fill in every tool call in a session that has a cached explanation.
    /// Cache-only and free, and reaches sessions recorded long before it
    /// was asked for.
    pub fn attach(&self, session: &mut Session) {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        fn walk(rounds: &mut [Round], cache: &Map<String, Value>) {
            for rnd in rounds {
                for item in &mut rnd.items {
                    if let Item::Tool(call) = item {
                        if call.explanation.is_empty() {
                            if let Some(text) = cache.get(&key_for(&call.name, &call.input)).and_then(Value::as_str) {
                                call.explanation = text.to_string();
                            }
                        }
                        if !call.subagent.is_empty() {
                            walk(&mut call.subagent, cache);
                        }
                    }
                }
            }
        }
        walk(&mut session.rounds, &cache);
    }
}
