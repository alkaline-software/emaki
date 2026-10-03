//! Transcript rows -> `Session`.
//!
//! This is the one builder for Claude Code transcripts. Both renderers
//! (markdown for files, the model the app draws) consume its output, which is
//! what keeps them from drifting apart. The function is pure and total: it
//! takes the rows it is given and returns a model, skipping anything it does
//! not recognise rather than failing.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use chrono::{DateTime, FixedOffset};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::json::*;
use crate::model::*;
use crate::transcript::{load_subagents, pick_title, read_all, SubagentRecord};

/// Rows that carry no conversational content.
const IGNORED_TYPES: &[&str] = &[
    "mode",
    "permission-mode",
    "last-prompt",
    "agent-name",
    "file-history-delta",
    "file-history-snapshot",
    "queue-operation",
    "ai-title",
];

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

re!(RE_SYSTEM_REMINDER, r"(?s)<system-reminder>.*?</system-reminder>\s*");
re!(RE_CAVEAT, r"(?s)<local-command-caveat>.*?</local-command-caveat>\s*");
re!(RE_TASK_NOTIFICATION, r"(?s)<task-notification>.*?</task-notification>\s*");
re!(RE_HOOK_OUTPUT, r"(?s)<[a-z-]*hook[a-z-]*>.*?</[a-z-]*hook[a-z-]*>\s*");
re!(RE_COMMAND_NAME, r"(?s)<command-name>(.*?)</command-name>");
re!(RE_COMMAND_ARGS, r"(?s)<command-args>(.*?)</command-args>");
re!(RE_COMMAND_STDOUT, r"(?s)<local-command-stdout>(.*?)</local-command-stdout>");
re!(RE_COMMAND_MESSAGE, r"(?s)<command-message>.*?</command-message>\s*");
re!(
    RE_ANY_TAG_BLOCK,
    r"</?(?:command-name|command-args|command-message|local-command-stdout|local-command-caveat|system-reminder|task-notification)>"
);
re!(RE_PEER_HEADER, r"\A(?:Another Claude session|A peer session) sent a message(?: while you were working)?:\n");
re!(RE_PEER_FOOTER, r"\n\n(?:This came from another Claude session|That \x22other Claude session\x22)[^\n]*\z");
re!(RE_PEER_ENVELOPE, r"(?s)\A<cross-session-message(?: [^>]*)?>\n(.*)\n</cross-session-message>\z");
re!(RE_ATTACHED, r"(?m)^Attached file: (\S.*?)\s*$");
// The terminal writes `[Image #3]` where a picture was pasted; the picture
// itself is a content block after the text. The marker names the block.
re!(RE_IMAGE_MARK, r"\[Image #(\d+)\]\s*");
// Text pasted into the terminal arrives wrapped, with the id on both tags.
re!(RE_PASTED_TAG, r"</?pasted_content[^>]*>\s*");
re!(RE_UPLOAD_ID, r"^[0-9a-f]{12}-");
re!(RE_ANSI, r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|[\x00-\x08\x0b\x0c\x0e-\x1f]");

/// `origin.name` on a message Emaki sent.
pub const PAGE_SENDER: &str = "emaki";
/// What the envelope said before the rename; rows from then are still ours.
pub const PAGE_SENDER_LEGACY: &str = "scribe";

pub fn media_type_for(name: &str) -> &'static str {
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "",
    }
}

/// Split what came with a prompt from the prompt itself. A pasted picture
/// takes its name from the `[Image #n]` marker the terminal left in the
/// text, in order, and the marker goes.
pub fn attachments_of(prompt: &str, blocks: &[Value], row_uuid: &str) -> (String, Vec<Attachment>) {
    let mut found = Vec::new();
    let mut marks = RE_IMAGE_MARK.captures_iter(prompt).map(|c| format!("Image #{}", &c[1]));
    for (i, block) in blocks.iter().enumerate() {
        if block_type(block) == "image" {
            let media = block.get("source").map(|s| str_of(s, "media_type")).unwrap_or("");
            found.push(Attachment {
                kind: "image".into(),
                uuid: row_uuid.into(),
                index: i,
                name: marks.next().unwrap_or_default(),
                media_type: if media.is_empty() { "image/png".into() } else { media.into() },
                ..Default::default()
            });
        }
    }
    let marked = RE_IMAGE_MARK.is_match(prompt);
    let prompt: std::borrow::Cow<str> = if marked { RE_IMAGE_MARK.replace_all(prompt, "") } else { prompt.into() };
    let prompt = prompt.as_ref();
    for cap in RE_ATTACHED.captures_iter(prompt) {
        // A path written before the rename points into ~/.scribe; the file
        // moved with the directory.
        let path = crate::paths::relocate_legacy(cap.get(1).map(|m| m.as_str()).unwrap_or(""));
        let base = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let name = RE_UPLOAD_ID.replace(&base, "").to_string();
        let media = media_type_for(&name);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        found.push(Attachment {
            kind: if media.is_empty() { "file".into() } else { "image".into() },
            path,
            name,
            media_type: media.into(),
            size,
            ..Default::default()
        });
    }
    let prompt = if !found.is_empty() && !prompt.is_empty() {
        RE_ATTACHED.replace_all(prompt, "").trim().to_string()
    } else {
        prompt.to_string()
    };
    (prompt, found)
}

// ---------------------------------------------------------------- text helpers

/// Remove terminal control sequences, once, at build time.
pub fn strip_ansi(text: &str) -> String {
    RE_ANSI.replace_all(text, "").into_owned()
}

fn text_blocks_joined(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter(|b| block_type(b) == "text")
        .map(|b| str_of(b, "text"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The human-authored part of a user row, wrappers removed.
pub fn user_prompt_text(row: &Value) -> String {
    if let Some((text, _)) = peer_message(row) {
        return text;
    }
    strip_wrappers(&text_blocks_joined(&blocks(row))).0.trim().to_string()
}

/// A user row Claude Code wrote to itself, not one the person sent: a
/// meta row that is not a peer message, or the summary it hands the model
/// after compaction (`isCompactSummary`, which its own view hides too:
/// `isVisibleInTranscriptOnly`). Neither opens a round, and neither is a
/// prompt waiting for a reply.
pub fn machine_authored(row: &Value) -> bool {
    bool_of(row, "isCompactSummary") || (bool_of(row, "isMeta") && peer_message(row).is_none())
}

/// `(text, source)` for a user row delivered through the session inbox.
pub fn peer_message(row: &Value) -> Option<(String, Source)> {
    let origin = row.get("origin")?.as_object()?;
    if origin.get("kind").and_then(Value::as_str) != Some("peer") {
        return None;
    }
    let source = if matches!(origin.get("name").and_then(Value::as_str), Some(PAGE_SENDER) | Some(PAGE_SENDER_LEGACY)) { Source::Web } else { Source::Peer };
    if let Some(body) = origin.get("body").and_then(Value::as_str) {
        return Some((body.trim().to_string(), source));
    }
    let text = text_blocks_joined(&blocks(row));
    let text = RE_PEER_HEADER.replace(&text, "");
    let text = RE_PEER_FOOTER.replace(&text, "");
    let trimmed = text.trim();
    let text = match RE_PEER_ENVELOPE.captures(trimmed) {
        Some(c) => c.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
        None => trimmed.to_string(),
    };
    Some((text.trim().to_string(), source))
}

/// Split raw user text into (prompt, slash-commands, command-output).
pub fn strip_wrappers(text: &str) -> (String, Vec<String>, Vec<String>) {
    if text.is_empty() {
        return (String::new(), Vec::new(), Vec::new());
    }
    let mut commands: Vec<String> =
        RE_COMMAND_NAME.captures_iter(text).map(|c| c[1].trim().to_string()).filter(|s| !s.is_empty()).collect();
    if !commands.is_empty() {
        let args: Vec<String> = RE_COMMAND_ARGS.captures_iter(text).map(|c| c[1].trim().to_string()).collect();
        commands = commands
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let arg = args.get(i).map(String::as_str).unwrap_or("");
                format!("{name} {arg}").trim().to_string()
            })
            .collect();
    }
    let outputs: Vec<String> =
        RE_COMMAND_STDOUT.captures_iter(text).map(|c| c[1].trim().to_string()).filter(|s| !s.is_empty()).collect();

    let mut cleaned = text.to_string();
    for re in [&*RE_SYSTEM_REMINDER, &*RE_CAVEAT, &*RE_TASK_NOTIFICATION, &*RE_HOOK_OUTPUT, &*RE_COMMAND_MESSAGE, &*RE_PASTED_TAG] {
        cleaned = re.replace_all(&cleaned, "").into_owned();
    }
    cleaned = RE_COMMAND_NAME.replace_all(&cleaned, "").into_owned();
    cleaned = RE_COMMAND_ARGS.replace_all(&cleaned, "").into_owned();
    cleaned = RE_COMMAND_STDOUT.replace_all(&cleaned, "").into_owned();
    cleaned = RE_ANY_TAG_BLOCK.replace_all(&cleaned, "").into_owned();
    (cleaned.trim().to_string(), commands, outputs)
}

/// Flatten a `tool_result` body. Returns (text, image_count).
fn result_to_text(content: Option<&Value>) -> (String, u32) {
    match content {
        Some(Value::String(s)) => (s.clone(), 0),
        Some(Value::Array(items)) => {
            let mut images = 0;
            let mut parts = Vec::new();
            for block in items {
                match block_type(block) {
                    "text" => {
                        let t = str_of(block, "text");
                        if !t.is_empty() {
                            parts.push(t.to_string());
                        }
                    }
                    "image" => images += 1,
                    _ => {}
                }
            }
            (parts.join("\n"), images)
        }
        _ => (String::new(), 0),
    }
}

pub fn parse_ts(value: &str) -> Option<DateTime<FixedOffset>> {
    if value.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(value).ok()
}

pub fn elapsed_ms(start: &str, end: &str) -> u64 {
    match (parse_ts(start), parse_ts(end)) {
        (Some(a), Some(b)) => (b - a).num_milliseconds().max(0) as u64,
        _ => 0,
    }
}

// ---------------------------------------------------------------- subjects

/// `path` shortened against `cwd`: the part under it, `.` for the cwd itself,
/// else the path with a `~`. The cwd is matched as a string prefix in either
/// separator style, not through `Path`: Windows does not count a POSIX path
/// as absolute, and a transcript written on one OS is read on another.
fn rel(path: &str, cwd: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    if !cwd.is_empty() {
        let base = cwd.trim_end_matches(['/', '\\']);
        if path == base {
            return ".".into();
        }
        if let Some(rest) = path.strip_prefix(base).and_then(|r| r.strip_prefix(['/', '\\'])) {
            return if rest.is_empty() { ".".into() } else { rest.to_string() };
        }
    }
    crate::paths::tilde(path)
}

pub fn one_line(text: &str, limit: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = line.chars().count();
    if count > limit {
        let mut s: String = line.chars().take(limit).collect();
        s.push('…');
        s
    } else {
        line
    }
}

fn input_str<'a>(data: &'a Map<String, Value>, key: &str) -> &'a str {
    data.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A one-line answer to "what is this call".
pub fn tool_subject(name: &str, data: &Map<String, Value>, cwd: &str) -> String {
    match name {
        "Bash" | "BashOutput" | "KillShell" => {
            let c = input_str(data, "command");
            one_line(if c.is_empty() { input_str(data, "description") } else { c }, 110)
        }
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "NotebookRead" => {
            let p = input_str(data, "file_path");
            rel(if p.is_empty() { input_str(data, "notebook_path") } else { p }, cwd)
        }
        "Glob" | "Grep" => {
            let pattern = input_str(data, "pattern");
            let where_ = rel(input_str(data, "path"), cwd);
            one_line(&if where_.is_empty() { pattern.to_string() } else { format!("{pattern}  in {where_}") }, 110)
        }
        "Agent" | "Task" => {
            let d = input_str(data, "description");
            let d = if d.is_empty() { input_str(data, "subagent_type") } else { d };
            one_line(if d.is_empty() { "subagent" } else { d }, 110)
        }
        "Skill" => one_line(input_str(data, "skill"), 110),
        "WebFetch" => one_line(input_str(data, "url"), 110),
        "WebSearch" | "ToolSearch" => one_line(input_str(data, "query"), 110),
        "TodoWrite" => match data.get("todos").and_then(Value::as_array) {
            Some(t) => format!("{} items", t.len()),
            None => "todo list".into(),
        },
        "TaskCreate" | "TaskUpdate" => {
            let s = input_str(data, "subject");
            one_line(if s.is_empty() { input_str(data, "taskId") } else { s }, 110)
        }
        "AskUserQuestion" => {
            if let Some(first) = data.get("questions").and_then(Value::as_array).and_then(|q| q.first()) {
                let q = str_of(first, "question");
                let q = if q.is_empty() { str_of(first, "header") } else { q };
                return one_line(q, 110);
            }
            "question".into()
        }
        _ if name.starts_with("mcp__") => name.to_string(),
        _ => {
            for key in ["path", "file", "url", "query", "name", "prompt", "description", "command", "cmd"] {
                let v = input_str(data, key);
                if !v.trim().is_empty() {
                    return one_line(v, 110);
                }
            }
            // Codex writes shell calls as a `command` array.
            if let Some(cmd) = data.get("command").and_then(Value::as_array) {
                let joined = cmd.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
                return one_line(&joined, 110);
            }
            String::new()
        }
    }
}

// ---------------------------------------------------------------- turn state

const ASKS: &[&str] = &["AskUserQuestion", "ExitPlanMode"];
const INTERRUPTED: &str = "[Request interrupted";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    NeedsYou,
    Working,
    YourTurn,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::NeedsYou => "needs_you",
            Phase::Working => "working",
            Phase::YourTurn => "your_turn",
        }
    }
}

/// Where a session stands right now, read off the tail of its transcript.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnState {
    pub phase: Phase,
    pub mode: String,
    pub activity: String,
    pub activity_kind: String,
    pub reply: String,
    pub since: String,
    pub turn_started: String,
    pub tool: String,
}

impl TurnState {
    fn set(&mut self, phase: Phase, activity: impl Into<String>, kind: &str) {
        self.phase = phase;
        self.activity = activity.into();
        self.activity_kind = kind.into();
    }
}

/// `phase` is: `NeedsYou` when the last thing the agent did was ask; `Working`
/// when a tool is running or it is mid-reply; `YourTurn` when it ended its
/// turn or was interrupted; `Idle` when there are no content rows at all.
pub fn turn_state(rows: &[Value], cwd: &str) -> TurnState {
    let mut state = TurnState::default();

    for row in rows.iter().rev() {
        let kind = str_of(row, "type");
        if kind == "permission-mode" && !str_of(row, "permissionMode").is_empty() {
            state.mode = str_of(row, "permissionMode").into();
            break;
        }
        if kind == "user" && !str_of(row, "permissionMode").is_empty() && !bool_of(row, "isSidechain") {
            state.mode = str_of(row, "permissionMode").into();
            break;
        }
    }

    let mut decided = false;
    // Local commands that ran, newest first, by name. A slash command the
    // person typed (`/compact`, `/effort high`) is written as a prompt row
    // of its own, and what answers it is not a reply but the command's own
    // rows after it: `<command-name>` in a user row, or a
    // `system/local_command` row. That prompt is not waiting on anything.
    let mut ran: Vec<String> = Vec::new();
    for i in (0..rows.len()).rev() {
        let row = &rows[i];
        let kind = str_of(row, "type");
        if kind == "system" && str_of(row, "subtype") == "local_command" {
            if let Some(c) = row.get("commandRun").map(|c| str_of(c, "command")).filter(|c| !c.is_empty()) {
                ran.push(c.trim_start_matches('/').to_string());
            }
            continue;
        }
        if (kind != "user" && kind != "assistant") || bool_of(row, "isSidechain") {
            continue;
        }
        let bl = blocks(row);
        let ts = str_of(row, "timestamp");

        if kind == "user" {
            if machine_authored(row) {
                continue;
            }
            if peer_message(row).is_none() {
                let (_, commands, _) = strip_wrappers(&text_blocks_joined(&bl));
                ran.extend(commands.iter().filter_map(|c| c.split_whitespace().next()).map(|c| c.trim_start_matches('/').to_string()));
            }
            if bl.iter().any(|b| block_type(b) == "tool_result") {
                if !decided {
                    let interrupted = bl.iter().any(|b| {
                        let c = b.get("content").map(|c| c.to_string()).unwrap_or_default();
                        c.chars().take(80).collect::<String>().contains(INTERRUPTED)
                    });
                    if interrupted {
                        state.set(Phase::YourTurn, "interrupted", "stop");
                    } else {
                        state.set(Phase::Working, "thinking", "wait");
                    }
                    state.since = ts.into();
                    decided = true;
                }
                continue;
            }
            let text = user_prompt_text(row);
            if text.is_empty() && !bl.iter().any(|b| block_type(b) == "image") {
                continue;
            }
            let name = command_name(&text);
            if !name.is_empty() {
                if let Some(at) = ran.iter().position(|c| *c == name) {
                    ran.remove(at);
                    continue;
                }
            }
            if !decided {
                if text.starts_with(INTERRUPTED) {
                    state.set(Phase::YourTurn, "interrupted", "stop");
                } else {
                    state.set(Phase::Working, "reading the prompt", "wait");
                }
                state.since = ts.into();
                decided = true;
            }
            if !text.starts_with(INTERRUPTED) {
                state.turn_started = ts.into();
                break;
            }
            continue;
        }

        if decided {
            continue;
        }
        let message = row.get("message").and_then(Value::as_object);
        let stop = message.and_then(|m| m.get("stop_reason")).and_then(Value::as_str).unwrap_or("");
        let calls: Vec<&Value> = bl.iter().filter(|b| block_type(b) == "tool_use").collect();
        state.since = ts.into();
        if let Some(call) = calls.last() {
            let name = str_of(call, "name");
            let name = if name.is_empty() { "Tool" } else { name };
            let empty = Map::new();
            let input = call.get("input").and_then(Value::as_object).unwrap_or(&empty);
            let subject = tool_subject(name, input, cwd);
            if ASKS.contains(&name) {
                let activity = if subject.is_empty() {
                    if name == "ExitPlanMode" { "plan ready" } else { "question" }.to_string()
                } else {
                    subject
                };
                state.set(Phase::NeedsYou, activity, if name == "ExitPlanMode" { "plan" } else { "ask" });
            } else {
                let activity = if subject.is_empty() { name.to_string() } else { subject };
                state.set(Phase::Working, activity, tool_kind(name).as_str());
                state.tool = name.into();
            }
        } else if (!stop.is_empty() && stop != "tool_use")
            || (stop.is_empty() && bl.iter().any(|b| block_type(b) == "text"))
        {
            state.set(Phase::YourTurn, "replied", "reply");
            state.reply = last_text(rows, i, 160);
        } else {
            state.set(Phase::Working, "thinking", "wait");
        }
        decided = true;
    }
    state
}

fn request_id(row: &Value) -> String {
    let r = str_of(row, "requestId");
    if !r.is_empty() {
        return r.into();
    }
    row.get("message").map(|m| str_of(m, "id")).unwrap_or("").into()
}

/// First line of the final text block of the message ending at `end`.
fn last_text(rows: &[Value], end: usize, limit: usize) -> String {
    let request = request_id(&rows[end]);
    for j in (0..=end).rev() {
        let row = &rows[j];
        if str_of(row, "type") != "assistant" {
            break;
        }
        if !request.is_empty() && request_id(row) != request {
            break;
        }
        for block in blocks(row).iter().rev() {
            if block_type(block) == "text" && !str_of(block, "text").trim().is_empty() {
                let first = str_of(block, "text").lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                let cleaned = first.replace("**", "").replace('`', "");
                let cleaned = cleaned.trim_start_matches(|c| matches!(c, '#' | '>' | '-' | ' ')).trim();
                return one_line(cleaned, limit);
            }
        }
    }
    String::new()
}

// ---------------------------------------------------------------- the builder

struct RoundBuilder {
    rounds: Vec<Round>,
    /// Index of the open round in `rounds`.
    current: Option<usize>,
    /// tool_use id -> (round index, item index).
    calls: HashMap<String, (usize, usize)>,
    pending_notices: Vec<Item>,
    last_ts: String,
}

impl RoundBuilder {
    fn new() -> Self {
        Self { rounds: Vec::new(), current: None, calls: HashMap::new(), pending_notices: Vec::new(), last_ts: String::new() }
    }

    fn open_round(&mut self, ts: &str, uuid: &str, prompt: String, source: Source) -> &mut Round {
        let mut rnd = Round {
            index: self.rounds.len() + 1,
            uuid: uuid.into(),
            ts: ts.into(),
            prompt,
            source,
            ..Default::default()
        };
        rnd.items.append(&mut self.pending_notices);
        self.rounds.push(rnd);
        self.current = Some(self.rounds.len() - 1);
        self.rounds.last_mut().unwrap()
    }

    fn ensure_round(&mut self, ts: &str) -> &mut Round {
        if self.current.is_none() {
            self.open_round(ts, "", String::new(), Source::System);
        }
        let i = self.current.unwrap();
        &mut self.rounds[i]
    }

    fn add(&mut self, item: Item) {
        let ts = item.ts().to_string();
        let rnd = self.ensure_round(&ts);
        rnd.items.push(item);
    }

    fn add_call(&mut self, call: ToolCall) {
        let id = call.id.clone();
        let ts = call.ts.clone();
        let rnd = self.ensure_round(&ts);
        rnd.items.push(Item::Tool(call));
        let ri = self.current.unwrap();
        let ii = self.rounds[ri].items.len() - 1;
        self.calls.insert(id, (ri, ii));
    }

    fn call_mut(&mut self, id: &str) -> Option<&mut ToolCall> {
        let (ri, ii) = *self.calls.get(id)?;
        self.rounds.get_mut(ri)?.items.get_mut(ii)?.as_tool_mut()
    }

    fn add_notice(&mut self, text: String, ts: &str, variant: NoticeVariant) {
        let notice = Item::Notice { ts: ts.into(), text, variant };
        match self.current {
            None => self.pending_notices.push(notice),
            Some(i) => self.rounds[i].items.push(notice),
        }
    }

    fn last_seen_ts(&self) -> String {
        if let Some(i) = self.current {
            let rnd = &self.rounds[i];
            if let Some(last) = rnd.items.last() {
                let ts = last.ts();
                return if ts.is_empty() { rnd.ts.clone() } else { ts.to_string() };
            }
            return rnd.ts.clone();
        }
        self.last_ts.clone()
    }
}

pub struct BuildInput<'a> {
    pub rows: &'a [Value],
    pub transcript_path: &'a str,
    pub cwd_hint: &'a str,
    pub subagents: Option<&'a HashMap<String, Vec<SubagentRecord>>>,
    /// Every row in a subagent's own transcript carries `isSidechain: true`. It
    /// is a sidechain *of the parent*, but the whole conversation at this level.
    pub nested: bool,
}

pub fn build(input: BuildInput) -> Session {
    let rows = input.rows;
    let mut session = Session { agent: AgentId::ClaudeCode, transcript_path: input.transcript_path.into(), ..Default::default() };

    let (main_rows, side_rows): (Vec<&Value>, Vec<&Value>) = if input.nested {
        (rows.iter().collect(), Vec::new())
    } else {
        rows.iter().partition(|r| !bool_of(r, "isSidechain"))
    };

    for row in rows {
        if !str_of(row, "sessionId").is_empty() {
            session.id = str_of(row, "sessionId").into();
        } else if !str_of(row, "session_id").is_empty() {
            session.id = str_of(row, "session_id").into();
        }
        if !str_of(row, "cwd").is_empty() {
            session.cwd = str_of(row, "cwd").into();
        }
        if !str_of(row, "gitBranch").is_empty() {
            session.git_branch = str_of(row, "gitBranch").into();
        }
        if !str_of(row, "version").is_empty() {
            session.version = str_of(row, "version").into();
        }
        if !str_of(row, "slug").is_empty() {
            session.slug = str_of(row, "slug").into();
        }
        // `/effort <level>` lands as a `system/local_command` row whose
        // `commandRun` names the command; the last one is the level in force.
        if let Some(run) = row.get("commandRun") {
            if str_of(run, "command") == "effort" && !str_of(run, "args").trim().is_empty() {
                session.effort = str_of(run, "args").trim().to_string();
            }
        }
    }
    session.title = pick_title(rows);
    if session.cwd.is_empty() {
        session.cwd = input.cwd_hint.into();
    }
    if session.id.is_empty() && !input.transcript_path.is_empty() {
        session.id = Path::new(input.transcript_path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    }

    let cwd = session.cwd.clone();
    let mut b = RoundBuilder::new();

    for row in &main_rows {
        let rtype = str_of(row, "type");
        let ts = str_of(row, "timestamp");
        if !ts.is_empty() {
            b.last_ts = ts.into();
        }
        if IGNORED_TYPES.contains(&rtype) {
            continue;
        }
        match rtype {
            "system" => handle_system(&mut b, row, ts, &mut session),
            "user" => handle_user(&mut b, row, ts),
            "assistant" => handle_assistant(&mut b, row, ts, &mut session),
            "attachment" => {
                if let Some(row) = queued_prompt(row) {
                    handle_user(&mut b, &row, ts);
                }
            }
            _ => {}
        }
    }

    if let Some(subs) = input.subagents {
        attach_subagent_files(&mut b, subs, &cwd);
    }
    attach_sidechains(&mut b, &side_rows, &cwd);
    finalize(b, &mut session);
    session
}

/// A message sent while the agent was working, as the user row it would
/// have been. Claude Code does not write one: the message is absorbed into
/// the running turn (`queue-operation` `remove`, reason `absorbed_mid_turn`)
/// and recorded as an `attachment` row of type `queued_command` whose
/// `prompt` is the text, or the content blocks when a picture was pasted,
/// with the same `origin` a user row carries (`human`, or `peer` with the
/// sender's name). A `task-notification` in the same shape is the
/// harness's, not the person's, and is left out. The row keeps the
/// attachment row's uuid, so a pasted picture is read back from it.
fn queued_prompt(row: &Value) -> Option<Value> {
    let a = row.get("attachment")?;
    if str_of(a, "type") != "queued_command" || str_of(a, "commandMode") != "prompt" {
        return None;
    }
    let content = match a.get("prompt")? {
        Value::String(text) => json!([{"type": "text", "text": text}]),
        blocks @ Value::Array(_) => blocks.clone(),
        _ => return None,
    };
    let mut user = json!({
        "type": "user",
        "uuid": str_of(row, "uuid"),
        "timestamp": str_of(row, "timestamp"),
        "message": {"role": "user", "content": content},
    });
    if let Some(origin) = a.get("origin") {
        user["origin"] = origin.clone();
    }
    if bool_of(a, "isMeta") {
        user["isMeta"] = Value::Bool(true);
    }
    Some(user)
}

fn handle_system(b: &mut RoundBuilder, row: &Value, ts: &str, session: &mut Session) {
    match str_of(row, "subtype") {
        "compact_boundary" => {
            b.add_notice("Context compacted, earlier messages summarised".into(), ts, NoticeVariant::Compact);
            // The context in use drops to the summary's size, and no
            // assistant row says so until the next turn: the boundary's
            // own count holds the row under the composer until then.
            let post = row.get("compactMetadata").map(|m| u64_of(m, "postTokens")).unwrap_or(0);
            if post > 0 {
                session.context_tokens = post;
            }
        }
        "turn_duration" => {
            if let Some(i) = b.current {
                let d = u64_of(row, "durationMs");
                b.rounds[i].duration_ms = b.rounds[i].duration_ms.max(d);
            }
        }
        "away_summary" => {}
        _ => {
            let content = str_of(row, "content");
            if content.trim().is_empty() || bool_of(row, "isMeta") {
                return;
            }
            let (text, commands, outputs) = strip_wrappers(content);
            for c in commands {
                b.add_notice(format!("/{}", c.trim_start_matches('/')), ts, NoticeVariant::Command);
            }
            for o in outputs {
                b.add_notice(one_line(&strip_ansi(&o), 300), ts, NoticeVariant::Info);
            }
            if !text.is_empty() {
                b.add_notice(one_line(&strip_ansi(&text), 200), ts, NoticeVariant::Info);
            }
        }
    }
}

fn handle_user(b: &mut RoundBuilder, row: &Value, ts: &str) {
    let bl = blocks(row);
    let uuid = str_of(row, "uuid");

    let results: Vec<(usize, &Value)> = bl.iter().enumerate().filter(|(_, x)| block_type(x) == "tool_result").collect();
    if !results.is_empty() {
        let sidecar = row.get("toolUseResult");
        for (index, block) in results {
            let id = str_of(block, "tool_use_id");
            if let Some(call) = b.call_mut(id) {
                apply_result(call, block, sidecar, ts);
                call.result_uuid = uuid.into();
                call.result_index = index;
            }
        }
        return;
    }

    if let Some((text, source)) = peer_message(row) {
        let (text, attached) = attachments_of(&text, &[], uuid);
        if !text.is_empty() || !attached.is_empty() {
            let rnd = b.open_round(ts, uuid, text, source);
            rnd.attachments = attached;
        }
        return;
    }

    if machine_authored(row) {
        return;
    }

    let images = bl.iter().filter(|x| block_type(x) == "image").count() as u32;
    let raw = text_blocks_joined(&bl);
    let (prompt, commands, outputs) = strip_wrappers(&raw);
    let (prompt, attached) = attachments_of(&prompt, &bl, uuid);

    if prompt.is_empty() && commands.is_empty() && outputs.is_empty() && images == 0 && attached.is_empty() {
        return;
    }

    if !prompt.is_empty() || images > 0 || !attached.is_empty() {
        let rnd = b.open_round(ts, uuid, prompt, Source::User);
        rnd.images = images;
        rnd.attachments = attached;
        for c in commands {
            rnd.items.push(Item::Notice { ts: ts.into(), text: format!("/{}", c.trim_start_matches('/')), variant: NoticeVariant::Command });
        }
        for o in outputs {
            rnd.items.push(Item::Notice { ts: ts.into(), text: one_line(&strip_ansi(&o), 300), variant: NoticeVariant::Info });
        }
        return;
    }

    // A slash command typed in the terminal is written as a prompt row of
    // its own and then as these rows, so the chip would say what the
    // prompt above it already says; it is left out when it would. The
    // output of `/compact` is the terminal's own instruction ("Compacted
    // (ctrl+o to see full summary)"), which means nothing here, and the
    // boundary's notice has already said so.
    let said = b.current.map(|i| command_name(&b.rounds[i].prompt)).unwrap_or_default();
    let mut compacted = said == "compact";
    for c in commands {
        let name = command_name(&format!("/{}", c.trim_start_matches('/')));
        compacted |= name == "compact";
        if name != said {
            b.add_notice(format!("/{}", c.trim_start_matches('/')), ts, NoticeVariant::Command);
        }
    }
    // The output comes in a row of its own, after the command's.
    if compacted {
        return;
    }
    for o in outputs {
        b.add_notice(one_line(&strip_ansi(&o), 300), ts, NoticeVariant::Info);
    }
}

/// The name of the slash command `text` is, without the slash or its
/// arguments, or empty when it is not one.
fn command_name(text: &str) -> String {
    text.strip_prefix('/').and_then(|t| t.split_whitespace().next()).unwrap_or("").to_string()
}

fn handle_assistant(b: &mut RoundBuilder, row: &Value, ts: &str, session: &mut Session) {
    let message = row.get("message").and_then(Value::as_object);
    let model = message.and_then(|m| m.get("model")).and_then(Value::as_str).unwrap_or("");
    if !model.is_empty() && !session.models.iter().any(|m| m == model) {
        session.models.push(model.into());
    }
    let usage = Usage::from_raw(message.and_then(|m| m.get("usage")));
    session.usage.add(&usage);
    let context = usage.input_tokens + usage.cache_read + usage.cache_write;
    if context > 0 {
        session.context_tokens = context;
    }
    let total = usage.total();
    {
        let rnd = b.ensure_round(ts);
        rnd.usage.add(&usage);
        if total > 0 {
            rnd.add_model_usage(if model.is_empty() { "unknown" } else { model }, total);
        }
    }
    if total > 0 {
        session.add_model_usage(if model.is_empty() { "unknown" } else { model }, total);
    }

    let uuid = str_of(row, "uuid");
    for block in blocks(row) {
        match block_type(&block) {
            "text" => {
                let text = str_of(&block, "text").trim();
                if !text.is_empty() {
                    b.add(Item::Text { uuid: uuid.into(), ts: ts.into(), md: text.into() });
                }
            }
            "thinking" => {
                let text = str_of(&block, "thinking").trim();
                if text.is_empty() {
                    continue;
                }
                let seconds = if b.last_ts.is_empty() { 0.0 } else { elapsed_ms(&b.last_seen_ts(), ts) as f64 / 1000.0 };
                b.add(Item::Thinking { uuid: uuid.into(), ts: ts.into(), md: text.into(), seconds });
            }
            "tool_use" => {
                let name = str_of(&block, "name");
                let name = if name.is_empty() { "Tool" } else { name };
                let input = block.get("input").and_then(Value::as_object).cloned().unwrap_or_default();
                let call = ToolCall {
                    uuid: uuid.into(),
                    ts: ts.into(),
                    id: str_of(&block, "id").into(),
                    name: name.into(),
                    tool_kind: tool_kind(name),
                    subject: tool_subject(name, &input, &session.cwd),
                    input,
                    ..Default::default()
                };
                b.add_call(call);
            }
            _ => {}
        }
    }
}

fn apply_result(call: &mut ToolCall, block: &Value, sidecar: Option<&Value>, ts: &str) {
    let (text, images) = result_to_text(block.get("content"));
    call.result_text = strip_ansi(&text);
    call.result_images = images;
    call.duration_ms = elapsed_ms(&call.ts, ts);
    call.status = if bool_of(block, "is_error") { CallStatus::Error } else { CallStatus::Ok };

    let Some(sc) = sidecar.and_then(Value::as_object) else {
        return;
    };
    if sc.contains_key("stdout") || sc.contains_key("stderr") {
        call.stdout = strip_ansi(sc.get("stdout").and_then(Value::as_str).unwrap_or(""));
        call.stderr = strip_ansi(sc.get("stderr").and_then(Value::as_str).unwrap_or(""));
        if sc.get("interrupted").and_then(Value::as_bool).unwrap_or(false) {
            call.status = CallStatus::Interrupted;
        }
    }
    if let Some(patch) = sc.get("structuredPatch").and_then(Value::as_array) {
        if !patch.is_empty() {
            call.patch = patch.clone();
        }
    }
    for key in ["filePath", "file_path"] {
        if let Some(p) = sc.get(key).and_then(Value::as_str) {
            if !p.is_empty() {
                call.file_path = p.into();
                break;
            }
        }
    }
    if let Some(p) = sc.get("file").and_then(Value::as_object).and_then(|f| f.get("filePath")).and_then(Value::as_str) {
        if !p.is_empty() {
            call.file_path = p.into();
        }
    }
    if let Some(old) = sc.get("oldString") {
        if !old.is_null() {
            call.old_string = old.as_str().unwrap_or("").into();
            call.new_string = sc.get("newString").and_then(Value::as_str).unwrap_or("").into();
        }
    }
}

/// Nest subagent conversations recorded in their own files, keyed by the
/// exact `toolUseId` that spawned each.
fn attach_subagent_files(b: &mut RoundBuilder, subs: &HashMap<String, Vec<SubagentRecord>>, cwd: &str) {
    for (tool_use_id, records) in subs {
        if tool_use_id.is_empty() {
            continue;
        }
        let Some(record) = records.first() else { continue };
        if b.calls.get(tool_use_id).is_none() {
            continue;
        }
        let conversation = build(BuildInput { rows: &record.rows, transcript_path: "", cwd_hint: cwd, subagents: None, nested: true });
        let name = if !record.agent_type.is_empty() { record.agent_type.clone() } else { record.description.clone() };
        if let Some(call) = b.call_mut(tool_use_id) {
            call.subagent = conversation.rounds;
            if !name.is_empty() {
                call.agent_name = name;
            }
        }
    }
}

/// Older Claude Code versions interleaved sidechain rows into the main file
/// with no link back; each contiguous run goes to the next unfilled task call.
fn attach_sidechains(b: &mut RoundBuilder, side_rows: &[&Value], cwd: &str) {
    if side_rows.is_empty() {
        return;
    }
    let mut task_ids: Vec<(String, String)> = b
        .calls
        .iter()
        .filter_map(|(id, &(ri, ii))| {
            let c = b.rounds[ri].items[ii].as_tool()?;
            (c.tool_kind == ToolKind::Task && c.subagent.is_empty()).then(|| (c.ts.clone(), id.clone()))
        })
        .collect();
    if task_ids.is_empty() {
        return;
    }
    task_ids.sort();

    let mut runs: Vec<Vec<Value>> = Vec::new();
    let mut current: Vec<Value> = Vec::new();
    let mut prev_uuid: Option<String> = None;
    for row in side_rows {
        let parent = row.get("parentUuid").and_then(Value::as_str);
        if !current.is_empty() && parent.is_some() && parent.map(str::to_string) != prev_uuid {
            runs.push(std::mem::take(&mut current));
        }
        current.push((*row).clone());
        prev_uuid = Some(str_of(row, "uuid").to_string());
    }
    if !current.is_empty() {
        runs.push(current);
    }

    for ((_, id), run) in task_ids.iter().zip(runs.iter()) {
        let conversation = build(BuildInput { rows: run, transcript_path: "", cwd_hint: cwd, subagents: None, nested: true });
        let agent_name = run.iter().map(|r| str_of(r, "agentName")).find(|s| !s.is_empty()).map(str::to_string);
        if let Some(call) = b.call_mut(id) {
            call.subagent = conversation.rounds;
            if let Some(n) = agent_name {
                call.agent_name = n;
            }
        }
    }
}

fn finalize(b: RoundBuilder, session: &mut Session) {
    let mut rounds = b.rounds;
    let n = rounds.len();
    for (i, rnd) in rounds.iter_mut().enumerate() {
        let mut last = rnd.ts.clone();
        for item in &rnd.items {
            let ts = item.ts();
            if !ts.is_empty() && ts > last.as_str() {
                last = ts.to_string();
            }
        }
        rnd.end_ts = last.clone();
        if rnd.duration_ms == 0 {
            rnd.duration_ms = elapsed_ms(&rnd.ts, &last);
        }
        let is_last = i == n - 1;
        for item in &mut rnd.items {
            if let Some(call) = item.as_tool_mut() {
                if call.status == CallStatus::Pending && !is_last {
                    call.status = CallStatus::NoResult;
                }
                for sub in &mut call.subagent {
                    for nested in &mut sub.items {
                        if let Some(nc) = nested.as_tool_mut() {
                            if nc.status == CallStatus::Pending {
                                nc.status = CallStatus::NoResult;
                            }
                        }
                    }
                }
            }
        }
    }
    if let (Some(first), Some(last)) = (rounds.first(), rounds.last()) {
        if session.started.is_empty() {
            session.started = first.ts.clone();
        }
        session.updated = if last.end_ts.is_empty() { last.ts.clone() } else { last.end_ts.clone() };
    }
    if session.title.is_empty() {
        session.title = fallback_title(&rounds, 72);
    }
    session.rounds = rounds;
}

pub fn fallback_title(rounds: &[Round], limit: usize) -> String {
    for rnd in rounds {
        if !rnd.prompt.is_empty() {
            return one_line(&rnd.prompt, limit);
        }
    }
    "Untitled session".into()
}

pub fn build_from_path(path: &Path, cwd_hint: &str) -> Session {
    let rows = read_all(path);
    let subs = load_subagents(path);
    build(BuildInput {
        rows: &rows,
        transcript_path: &path.to_string_lossy(),
        cwd_hint,
        subagents: Some(&subs),
        nested: false,
    })
}
