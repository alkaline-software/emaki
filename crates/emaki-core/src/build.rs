//! Transcript rows -> `Session`.
//!
//! This is the one builder for Claude Code transcripts. Both renderers
//! (markdown for files, the model the app draws) consume its output, which is
//! what keeps them from drifting apart. The function is pure and total: it
//! takes the rows it is given and returns a model, skipping anything it does
//! not recognise rather than failing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
re!(RE_TASK_ID, r"<task-id>(.*?)</task-id>");
re!(RE_TASK_STATUS, r"<status>(.*?)</status>");
re!(RE_TASK_SUMMARY, r"(?s)<summary>(.*?)</summary>");
re!(RE_TASK_OUTPUT, r"written to: (\S+\.output)");
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
re!(RE_PEER_QUEUED, r#"(?s)\A<cross-session-message from-name="([^"]*)"[^>]*>\n(.*)\n</cross-session-message>\z"#);
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

pub fn text_blocks_joined(blocks: &[Value]) -> String {
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

/// "The turn was stopped", in the agent's own words, as the last thing a
/// round says. Every adapter's builder ends a stopped round with this, so
/// the conversation shows a stop the same way whichever agent wrote it.
/// Said once: an agent may record one stop in two rows.
pub fn push_interrupted(items: &mut Vec<Item>, said: &str, ts: &str) {
    if said.is_empty() || matches!(items.last(), Some(Item::Notice { variant: NoticeVariant::Interrupted, .. })) {
        return;
    }
    items.push(Item::Notice { ts: ts.into(), text: said.into(), variant: NoticeVariant::Interrupted });
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
    /// Claude Code's own word that no turn is running, from the session
    /// registry (`status: idle` since `idle_at`, Unix seconds). The
    /// transcript cannot always say a turn stopped: Escape during
    /// `/compact` leaves the prompt and nothing after it, and the rows
    /// read as working for ever. Idle since after the rows this state was
    /// read from means the turn was stopped; idle from before them is the
    /// registry not having caught up with a turn that just began. Returns
    /// whether the state changed.
    pub fn settle_idle(&mut self, idle_at: f64) -> bool {
        if self.phase != Phase::Working {
            return false;
        }
        let at = |ts: &str| parse_ts(ts).map(|d| d.timestamp_millis() as f64 / 1000.0).unwrap_or(0.0);
        if idle_at <= at(&self.since).max(at(&self.turn_started)) {
            return false;
        }
        self.phase = Phase::YourTurn;
        self.activity = "stopped".into();
        self.activity_kind = "stop".into();
        self.tool.clear();
        true
    }

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
        // `/plan` changes the mode and no mode row follows until the next
        // prompt; its own output is the only word of it.
        if kind == "user" && !bool_of(row, "isSidechain") && row.get("message").and_then(|m| m.get("content")).and_then(Value::as_str).is_some_and(|c| c.contains("<local-command-stdout>Enabled plan mode</local-command-stdout>")) {
            state.mode = "plan".into();
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
    /// The slash command whose output comes next: the open round's prompt
    /// when it is one, then each command row as it arrives.
    command: String,
    /// The last prompt, stopped before anything was done with it.
    withdrawn: Option<Round>,
    /// The permission mode as of the rows read so far.
    mode: String,
    /// The effort level a command last set, when one did.
    effort: Option<String>,
}

impl RoundBuilder {
    fn new() -> Self {
        Self { rounds: Vec::new(), current: None, calls: HashMap::new(), pending_notices: Vec::new(), last_ts: String::new(), command: String::new(), withdrawn: None, mode: String::new(), effort: None }
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
        self.withdrawn = None;
        self.command = command_name(&rnd.prompt);
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

    /// The turn was stopped: one line at the round's foot, once however
    /// many rows say so.
    fn add_interrupted(&mut self, said: &str, ts: &str) {
        let Some(i) = self.current else { return };
        push_interrupted(&mut self.rounds[i].items, said, ts);
    }

    fn add_notice(&mut self, text: String, ts: &str, variant: NoticeVariant) {
        let notice = Item::Notice { ts: ts.into(), text, variant };
        let (items, mut superseded) = match self.current {
            None => (&mut self.pending_notices, None),
            Some(i) => {
                let rnd = &mut self.rounds[i];
                (&mut rnd.items, Some(&mut rnd.superseded))
            }
        };
        // A setting changed again straight after itself is one change, the
        // last: effort to high, to medium, to high again reads "Effort
        // High", once. Anything between two of them (another setting, a
        // reply) keeps both. The one replaced is set aside, not lost.
        if variant.setting().is_some() && matches!(items.last(), Some(Item::Notice { variant: v, .. }) if *v == variant) {
            if let (Some(old), Some(kept)) = (items.pop(), superseded.as_mut()) {
                kept.push(old);
            }
        }
        items.push(notice);
    }

    /// What the command just run printed. `/model` and `/effort` answer
    /// with a sentence ("Set effort level to high (saved as your default
    /// for new sessions): Comprehensive implementation with…"), of which
    /// the conversation keeps what was set and nothing else, in place of
    /// the command's chip: one line, "Effort High". A picker closed with
    /// nothing changed ("Kept model as …") leaves no line at all.
    fn add_output(&mut self, output: &str, ts: &str) {
        let text = strip_ansi(output);
        match setting_said(&self.command, &text) {
            Some(Said::Set(variant, value)) => {
                self.drop_chip();
                let shown = if variant == NoticeVariant::Effort {
                    self.effort = Some(value.clone());
                    crate::driver::effort_words(&value).0
                } else {
                    value
                };
                self.add_notice(shown, ts, variant);
            }
            Some(Said::Kept) => self.drop_chip(),
            None => self.add_notice(one_line(&text, 300), ts, NoticeVariant::Info),
        }
    }

    /// Take back the chip of the command just run, when it is the last
    /// thing said.
    fn drop_chip(&mut self) {
        let items = match self.current {
            Some(i) => &mut self.rounds[i].items,
            None => &mut self.pending_notices,
        };
        let chip = matches!(items.last(), Some(Item::Notice { variant: NoticeVariant::Command, text, .. }) if command_name(text) == self.command);
        if chip {
            items.pop();
        }
    }

    /// The permission mode a row carries. A prompt row names the mode it
    /// was sent in, and Claude Code restates the mode in a
    /// `permission-mode` row whenever it writes; neither is written at the
    /// moment the mode changes, so a change shows where the transcript
    /// first has it. The first mode a session names is where it began, not
    /// a change.
    fn saw_mode(&mut self, mode: &str, ts: &str) {
        if mode.is_empty() || mode == self.mode {
            return;
        }
        if !self.mode.is_empty() {
            let ts = if ts.is_empty() { self.last_ts.clone() } else { ts.to_string() };
            self.add_notice(crate::driver::mode_words(mode).0, &ts, NoticeVariant::Mode);
        }
        self.mode = mode.into();
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

/// What a settings command said it did.
enum Said {
    Set(NoticeVariant, String),
    /// The picker was closed with nothing changed.
    Kept,
}

/// What `/model` or `/effort` printed, read for what was set: the model
/// by the name Claude Code gives it, between backticks since 2.1.28x
/// ("Set model to `Opus 5.5` and saved as your default for new sessions"),
/// the effort by its level ("Set effort level to high (…): …"). None for
/// any other command and for anything else these two say ("Current
/// model: …", a refusal), which is shown as it is.
fn setting_said(command: &str, output: &str) -> Option<Said> {
    let output = output.trim();
    match command {
        "model" => {
            if output.starts_with("Kept model as ") {
                return Some(Said::Kept);
            }
            let rest = output.strip_prefix("Set model to ")?;
            let name = match rest.strip_prefix('`') {
                Some(quoted) => quoted.split('`').next().unwrap_or(""),
                None => rest.split(" and saved").next().unwrap_or("").split(" for this session").next().unwrap_or(""),
            };
            let name = name.trim();
            (!name.is_empty()).then(|| Said::Set(NoticeVariant::Model, name.to_string()))
        }
        "effort" => {
            if output.starts_with("Kept effort level as ") {
                return Some(Said::Kept);
            }
            let rest = output.strip_prefix("Set effort level to ")?;
            let level: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
            (!level.is_empty()).then(|| Said::Set(NoticeVariant::Effort, level.to_lowercase()))
        }
        _ => None,
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

/// The prompts taken back before the agent began on them, and the rows
/// written with each, by `uuid`. Escape pressed at once leaves no
/// "[Request interrupted by user]" (2.1.293): the prompt's row stays,
/// nothing of the agent's is ever written under it, and the next prompt
/// is written beside it, under the same parent. The terminal takes such
/// a prompt off its screen and back into its input, so it is no round
/// here either. What Claude Code writes with a prompt hangs under it all
/// the same (where a picture came from, in an `isMeta` row, and its
/// `attachment` rows), and goes with it. A prompt the agent had started
/// on has a row of another kind under it and is not one of these.
fn taken_back(rows: &[Value]) -> HashSet<&str> {
    let prompt = |r: &Value| str_of(r, "type") == "user" && !bool_of(r, "isSidechain") && !bool_of(r, "isMeta") && !blocks(r).iter().any(|x| block_type(x) == "tool_result");
    let with_prompt = |r: &Value| str_of(r, "type") == "attachment" || str_of(r, "type") == "user" && bool_of(r, "isMeta");
    let mut under: HashMap<&str, Vec<&Value>> = HashMap::new();
    // The last prompt written under each parent.
    let mut last: HashMap<&str, usize> = HashMap::new();
    for (ix, r) in rows.iter().enumerate().filter(|(_, r)| !str_of(r, "parentUuid").is_empty()) {
        under.entry(str_of(r, "parentUuid")).or_default().push(r);
        if prompt(r) {
            last.insert(str_of(r, "parentUuid"), ix);
        }
    }
    let mut taken = HashSet::new();
    for (ix, r) in rows.iter().enumerate().filter(|(_, r)| prompt(r) && !str_of(r, "uuid").is_empty()) {
        if !last.get(str_of(r, "parentUuid")).is_some_and(|l| *l > ix) {
            continue;
        }
        let (mut its, mut todo) = (vec![str_of(r, "uuid")], vec![str_of(r, "uuid")]);
        let mut begun = false;
        while let Some(uuid) = todo.pop() {
            for row in under.get(uuid).into_iter().flatten() {
                begun |= !with_prompt(row);
                if !str_of(row, "uuid").is_empty() {
                    its.push(str_of(row, "uuid"));
                    todo.push(str_of(row, "uuid"));
                }
            }
        }
        if !begun {
            taken.extend(its);
        }
    }
    taken
}

pub fn build(input: BuildInput) -> Session {
    let rows = input.rows;
    let taken_back = taken_back(rows);
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
        // The folder the session was started in: the first a row names.
        // A row's `cwd` is where the shell stood when it was written, and
        // a `cd` in a command moves it for every row after. The session
        // is still the first folder's: that is where Claude Code keeps
        // it, where it is resumed from, and what the index
        // (`transcript::peek`) and the sidebar file it under.
        if session.cwd.is_empty() && !str_of(row, "cwd").is_empty() {
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
    let mut queue = Queue::default();
    if !input.nested {
        session.shells = shells_of(&main_rows);
    }

    for row in &main_rows {
        let rtype = str_of(row, "type");
        let ts = str_of(row, "timestamp");
        if !ts.is_empty() {
            b.last_ts = ts.into();
        }
        if rtype == "queue-operation" {
            queue.apply(row);
        }
        if rtype == "permission-mode" || rtype == "user" {
            b.saw_mode(str_of(row, "permissionMode"), ts);
        }
        if IGNORED_TYPES.contains(&rtype) {
            continue;
        }
        match rtype {
            "system" => handle_system(&mut b, row, ts, &mut session),
            "user" | "attachment" if taken_back.contains(str_of(row, "uuid")) => {}
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
    // What is still in the queue is a message the agent has not reached:
    // a round of its own at the end, until the row that records it lands
    // and the next build puts it where it was taken up.
    for row in queue.waiting() {
        let before = b.rounds.len();
        handle_user(&mut b, &row, str_of(&row, "timestamp"));
        if b.rounds.len() > before {
            b.rounds.last_mut().unwrap().queued = true;
        }
    }
    session.mode = b.mode.clone();
    if let Some(effort) = b.effort.take() {
        session.effort = effort;
    }
    finalize(b, &mut session);
    session
}

/// The commands the agent set running in the background, and how each
/// ended. One starts where a `Bash` result's sidecar carries
/// `backgroundTaskId` (the result's words name the file its output goes
/// to), and is over at the first `<task-notification>` for that id:
/// Claude Code writes it into the queue the moment the command exits,
/// and again as the prompt of the turn it starts. A stop the agent asks
/// for (`KillShell`, `TaskStop`) ends it too. A subagent is the same
/// with another start: an `Agent` result whose sidecar says `isAsync`
/// and names the `agentId`, which is the id its notification carries.
/// A command still running
/// when its Claude Code went away gets no row at all, so whether one
/// without an end is running is for the caller to say, from the process.
fn shells_of(rows: &[&Value]) -> Vec<Shell> {
    let mut calls: HashMap<String, (String, String)> = HashMap::new();
    let mut shells: Vec<Shell> = Vec::new();
    let end = |shells: &mut Vec<Shell>, id: &str, status: &str, summary: &str, ts: &str| {
        if let Some(sh) = shells.iter_mut().find(|sh| sh.id == id && sh.ended.is_empty()) {
            sh.ended = ts.into();
            sh.status = status.into();
            sh.summary = summary.trim().into();
        }
    };
    for row in rows {
        let ts = str_of(row, "timestamp");
        match str_of(row, "type") {
            "assistant" => {
                for block in blocks(row).iter().filter(|x| block_type(x) == "tool_use") {
                    let input = block.get("input");
                    let field = |k: &str| input.and_then(|i| i.get(k)).and_then(Value::as_str).unwrap_or("").to_string();
                    match str_of(block, "name") {
                        "Bash" => {
                            calls.insert(str_of(block, "id").into(), (field("command"), field("description")));
                        }
                        "Agent" | "Task" => {
                            calls.insert(str_of(block, "id").into(), (field("subagent_type"), field("description")));
                        }
                        "KillShell" | "TaskStop" | "KillBash" => {
                            let id = [field("shell_id"), field("task_id"), field("bash_id")].into_iter().find(|v| !v.is_empty()).unwrap_or_default();
                            end(&mut shells, &id, "killed", "", ts);
                        }
                        _ => {}
                    }
                }
            }
            "user" | "queue-operation" | "attachment" => {
                if let Some(id) = row.get("toolUseResult").and_then(|r| r.get("backgroundTaskId")).and_then(Value::as_str) {
                    let result = blocks(row).into_iter().find(|x| block_type(x) == "tool_result");
                    let call = result.as_ref().map(|x| str_of(x, "tool_use_id")).unwrap_or("");
                    let said = result.as_ref().and_then(|x| x.get("content")).map(Value::to_string).unwrap_or_default();
                    let (command, description) = calls.get(call).cloned().unwrap_or_default();
                    if !shells.iter().any(|sh| sh.id == id) {
                        shells.push(Shell { id: id.into(), command, description, output_path: RE_TASK_OUTPUT.captures(&said).map(|c| c[1].to_string()).unwrap_or_default(), started: ts.into(), ..Default::default() });
                    }
                    continue;
                }
                let side = row.get("toolUseResult");
                if let Some(id) = side.filter(|r| r.get("isAsync").and_then(Value::as_bool) == Some(true)).and_then(|r| r.get("agentId")).and_then(Value::as_str) {
                    let call = blocks(row).into_iter().find(|x| block_type(x) == "tool_result").map(|x| str_of(&x, "tool_use_id").to_string()).unwrap_or_default();
                    let (kind, mut description) = calls.get(&call).cloned().unwrap_or_default();
                    if description.is_empty() {
                        description = side.map(|r| str_of(r, "description")).unwrap_or("").into();
                    }
                    if !shells.iter().any(|sh| sh.id == id) {
                        shells.push(Shell { id: id.into(), agent: true, command: kind, description, started: ts.into(), ..Default::default() });
                    }
                    continue;
                }
                let text = row
                    .get("content")
                    .and_then(Value::as_str)
                    .or_else(|| row.get("message").and_then(|m| m.get("content")).and_then(Value::as_str))
                    .or_else(|| row.get("attachment").and_then(|a| a.get("prompt")).and_then(Value::as_str))
                    .unwrap_or("");
                if !text.contains("<task-notification>") {
                    continue;
                }
                for note in RE_TASK_NOTIFICATION.find_iter(text) {
                    let note = note.as_str();
                    let part = |re: &Regex| re.captures(note).map(|c| c[1].to_string()).unwrap_or_default();
                    end(&mut shells, &part(&RE_TASK_ID), &part(&RE_TASK_STATUS), &part(&RE_TASK_SUMMARY), ts);
                }
            }
            _ => {}
        }
    }
    shells
}

/// Where a background subagent of the session at `transcript_path`
/// writes its own transcript.
pub fn agent_transcript(transcript_path: &str, agent_id: &str) -> PathBuf {
    let path = Path::new(transcript_path);
    path.with_extension("").join("subagents").join(format!("agent-{agent_id}.jsonl"))
}

/// What a running subagent is on, from the end of its transcript: the
/// last tool it called with what it called it on, or the first line of
/// the last thing it said. Only the file's end is read, since this is
/// asked on the clock.
pub fn agent_step(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    const TAIL: u64 = 256 * 1024;
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // The first line is cut wherever the tail began, unless the tail is
    // the whole file.
    let skip = usize::from(len > TAIL);
    let lines: Vec<&str> = text.lines().skip(skip).collect();
    for line in lines.iter().rev() {
        let Ok(row) = serde_json::from_str::<Value>(line) else { continue };
        if str_of(&row, "type") != "assistant" {
            continue;
        }
        let cwd = str_of(&row, "cwd");
        for block in blocks(&row).iter().rev() {
            match block_type(block) {
                "tool_use" => {
                    let name = str_of(block, "name");
                    let empty = Map::new();
                    let subject = tool_subject(name, block.get("input").and_then(Value::as_object).unwrap_or(&empty), cwd);
                    let label = name.rsplit("__").next().unwrap_or(name).to_lowercase();
                    return Some(if subject.is_empty() { label } else { format!("{label}  {}", subject.lines().next().unwrap_or("")) });
                }
                "text" => {
                    if let Some(said) = str_of(block, "text").lines().map(str::trim).find(|l| !l.is_empty()) {
                        return Some(said.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Claude Code's queue of messages sent while a turn runs, replayed from
/// its `queue-operation` rows: `enqueue` with the message as `content`
/// the moment it arrives, then `dequeue` (the front one starts a turn of
/// its own) or `remove` with the same `content` (taken into the running
/// turn). The conversation's own row for the message is written only at
/// that second step, which for a long tool call is a minute later, so
/// what is still here at the end of the file is a message sent and not
/// yet shown. An entry without words (a harness notification, or content
/// that was not a string) is kept so the order holds, and never drawn.
#[derive(Default)]
struct Queue {
    entries: Vec<(String, String)>,
}

impl Queue {
    fn apply(&mut self, row: &Value) {
        let content = row.get("content").and_then(Value::as_str).unwrap_or("");
        match str_of(row, "operation") {
            "enqueue" => self.entries.push((content.to_string(), str_of(row, "timestamp").to_string())),
            "dequeue" if !self.entries.is_empty() => {
                self.entries.remove(0);
            }
            "remove" => {
                if let Some(i) = self.entries.iter().position(|(c, _)| c == content) {
                    self.entries.remove(i);
                } else if !self.entries.is_empty() {
                    self.entries.remove(0);
                }
            }
            _ => {}
        }
    }

    /// The waiting messages a person or a peer wrote, each as the user
    /// row it will become.
    fn waiting(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for (content, ts) in &self.entries {
            let text = content.trim();
            let mut row = json!({
                "type": "user",
                "uuid": format!("queued-{ts}"),
                "timestamp": ts,
                "message": {"role": "user", "content": [{"type": "text", "text": text}]},
            });
            if let Some(c) = RE_PEER_QUEUED.captures(text) {
                row["origin"] = json!({"kind": "peer", "name": &c[1], "body": &c[2]});
                row["isMeta"] = Value::Bool(true);
            } else if text.is_empty() || text.starts_with('<') {
                continue;
            }
            out.push(row);
        }
        out
    }
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
            // `/effort <level>` lands as a `system/local_command` row whose
            // `commandRun` names the command and the level asked for; the
            // output below says what it was set to, and is believed over it.
            let run = row.get("commandRun");
            if let Some(run) = run.filter(|r| str_of(r, "command") == "effort" && !str_of(r, "args").trim().is_empty()) {
                b.effort = Some(str_of(run, "args").trim().to_string());
            }
            let content = str_of(row, "content");
            if content.trim().is_empty() || bool_of(row, "isMeta") {
                return;
            }
            let (text, commands, outputs) = strip_wrappers(content);
            for c in commands {
                b.command = command_name(&format!("/{}", c.trim_start_matches('/')));
                b.add_notice(format!("/{}", c.trim_start_matches('/')), ts, NoticeVariant::Command);
            }
            // The row names its command even when its content does not.
            if let Some(name) = run.map(|r| str_of(r, "command").trim_start_matches('/')).filter(|n| !n.is_empty()) {
                b.command = name.to_string();
            }
            if b.command != "compact" {
                for o in outputs {
                    b.add_output(&o, ts);
                }
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

    // "[Request interrupted by user]" is Claude Code's marker for Escape,
    // not something the person said, and is never a round. A prompt it
    // stopped before the agent wrote or ran anything is withdrawn: the
    // terminal takes those words back into its input, and the window does
    // the same with its composer, so the conversation does not keep a
    // message that will be sent again.
    if prompt.starts_with(INTERRUPTED) {
        if let Some(i) = b.current.filter(|i| *i + 1 == b.rounds.len()) {
            let untouched = !b.rounds[i].items.iter().any(|it| !matches!(it, Item::Notice { .. }));
            if untouched && !b.rounds[i].prompt.is_empty() {
                b.withdrawn = b.rounds.pop();
                b.current = b.rounds.len().checked_sub(1);
                return;
            }
        }
        // A turn the agent had started on keeps its round, and the stop
        // is said at its foot in the marker's own words, brackets off.
        if b.current.is_some() {
            let said = prompt.lines().next().unwrap_or("").trim().trim_start_matches('[').trim_end_matches(']').trim();
            b.add_interrupted(said, ts);
        }
        return;
    }

    if !prompt.is_empty() || images > 0 || !attached.is_empty() {
        let rnd = b.open_round(ts, uuid, prompt, Source::User);
        rnd.images = images;
        rnd.attachments = attached;
        for c in commands {
            let name = format!("/{}", c.trim_start_matches('/'));
            b.command = command_name(&name);
            b.add_notice(name, ts, NoticeVariant::Command);
        }
        for o in outputs {
            b.add_output(&o, ts);
        }
        return;
    }

    // A slash command typed in the terminal is written as a prompt row of
    // its own and then as these rows, so the chip would say what the
    // prompt above it already says; it is left out when it would. The
    // output of `/compact` is the terminal's own instruction ("Compacted
    // (ctrl+o to see full summary)"), which means nothing here, and the
    // boundary's notice has already said so. Only that command's output
    // goes: a `/model` run later in the same round keeps its own.
    let said = b.current.map(|i| command_name(&b.rounds[i].prompt)).unwrap_or_default();
    for c in commands {
        let name = command_name(&format!("/{}", c.trim_start_matches('/')));
        if name != said {
            b.add_notice(format!("/{}", c.trim_start_matches('/')), ts, NoticeVariant::Command);
        }
        b.command = name;
    }
    // The output comes in a row of its own, after the command's.
    if b.command == "compact" {
        return;
    }
    for o in outputs {
        b.add_output(&o, ts);
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
    // Each model once, the one in use last: `/model` back to an earlier
    // one moves it to the end. `<synthetic>` is Claude Code's own row (an
    // API error put into words), not a model.
    if !model.is_empty() && model != "<synthetic>" && session.models.last().map(String::as_str) != Some(model) {
        session.models.retain(|m| m != model);
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
    // An AskUserQuestion answered: `answers` maps each question to the
    // label chosen, several joined with ", " when the question allowed
    // more than one, or the words typed instead.
    if let Some(a) = sc.get("answers").and_then(Value::as_object) {
        call.answers = a
            .iter()
            .map(|(q, v)| {
                let text = match v {
                    Value::String(t) => t.trim().to_string(),
                    Value::Array(items) => items.iter().filter_map(Value::as_str).map(str::trim).collect::<Vec<_>>().join(", "),
                    other => other.to_string(),
                };
                (q.clone(), text)
            })
            .filter(|(_, t)| !t.is_empty())
            .collect();
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
    let b_withdrawn = b.withdrawn;
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
    session.withdrawn = b_withdrawn;
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
