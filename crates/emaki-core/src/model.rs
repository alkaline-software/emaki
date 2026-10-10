//! The session model.
//!
//! A session is a list of **rounds**. A round opens at a real user prompt and
//! closes at the next one; everything the agent does in between (text,
//! thinking, tool calls) is an ordered list of items inside it. The model is a
//! pure function of the transcript (see `build`), which is what lets the
//! markdown be regenerated at any time instead of appended to.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Which coding agent wrote a transcript. The archive, the index and the
/// viewer all key on this; adding an agent means adding a variant here and an
/// adapter in `adapters/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentId {
    ClaudeCode,
    Codex,
}

impl AgentId {
    pub const ALL: [AgentId; 2] = [AgentId::ClaudeCode, AgentId::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            AgentId::ClaudeCode => "claude-code",
            AgentId::Codex => "codex",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AgentId::ClaudeCode => "Claude Code",
            AgentId::Codex => "Codex",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude-code" => Some(AgentId::ClaudeCode),
            "codex" => Some(AgentId::Codex),
            _ => None,
        }
    }

    /// Where this agent's sessions live inside `~/.emaki/archive`: a folder
    /// an agent, and in it a folder a project. An archive from before
    /// Claude Code had a folder of its own is moved into this shape
    /// (`archive::settle_layout`).
    pub fn archive_subdir(self) -> &'static str {
        match self {
            AgentId::ClaudeCode => "claude",
            AgentId::Codex => "codex",
        }
    }

    /// What the agent's speaker label is in a transcript.
    pub fn speaker(self) -> &'static str {
        match self {
            AgentId::ClaudeCode => "Claude",
            AgentId::Codex => "Codex",
        }
    }
}

impl Default for AgentId {
    fn default() -> Self {
        AgentId::ClaudeCode
    }
}

/// Tool families. The name drives how a call is rendered, and grouping the long
/// tail into "other" keeps the viewer from needing a branch per tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    Bash,
    Edit,
    Write,
    Read,
    Search,
    Web,
    Task,
    Todo,
    Ask,
    Plan,
    Mcp,
    Other,
}

impl ToolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolKind::Bash => "bash",
            ToolKind::Edit => "edit",
            ToolKind::Write => "write",
            ToolKind::Read => "read",
            ToolKind::Search => "search",
            ToolKind::Web => "web",
            ToolKind::Task => "task",
            ToolKind::Todo => "todo",
            ToolKind::Ask => "ask",
            ToolKind::Plan => "plan",
            ToolKind::Mcp => "mcp",
            ToolKind::Other => "other",
        }
    }
}

pub fn tool_kind(name: &str) -> ToolKind {
    match name {
        "Bash" | "BashOutput" | "KillShell" => ToolKind::Bash,
        "Edit" | "NotebookEdit" | "MultiEdit" => ToolKind::Edit,
        "Write" => ToolKind::Write,
        "Read" | "NotebookRead" => ToolKind::Read,
        "Glob" | "Grep" | "ToolSearch" => ToolKind::Search,
        "WebFetch" | "WebSearch" => ToolKind::Web,
        "Agent" | "Task" | "Skill" => ToolKind::Task,
        "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" => ToolKind::Todo,
        "AskUserQuestion" => ToolKind::Ask,
        "ExitPlanMode" | "EnterPlanMode" => ToolKind::Plan,
        // Codex's built-ins, mapped onto the same families.
        "shell" | "exec_command" | "local_shell" | "container.exec" => ToolKind::Bash,
        "apply_patch" => ToolKind::Edit,
        "web_search" => ToolKind::Web,
        "spawn_agent" => ToolKind::Task,
        "update_plan" => ToolKind::Todo,
        _ if name.starts_with("mcp__") => ToolKind::Mcp,
        _ => ToolKind::Other,
    }
}

/// One of an account's usage windows, as an agent's transcript names it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    /// How long the window is, in minutes: 300 is five hours.
    pub minutes: u64,
    /// How much of it is spent, 0 to 1.
    pub used: f64,
    /// When it resets, Unix seconds.
    pub resets_at: f64,
}

impl UsageWindow {
    /// The window's name on the row under the composer: "5h", "7d", "30d".
    pub fn label(&self) -> String {
        match self.minutes {
            m if m >= 1440 && m % 1440 == 0 => format!("{}d", m / 1440),
            m if m >= 60 && m % 60 == 0 => format!("{}h", m / 60),
            m => format!("{m}m"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    /// Tokens genuinely produced or consumed fresh. Deliberately excludes
    /// `cache_read`: every assistant message re-reads the whole cached prefix,
    /// so summing that across a long session yields a number like "84M tokens"
    /// for a conversation that generated a few hundred thousand.
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_write
    }

    pub fn add(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }

    pub fn from_raw(raw: Option<&Value>) -> Usage {
        let Some(raw) = raw.and_then(Value::as_object) else {
            return Usage::default();
        };
        let n = |k: &str| raw.get(k).and_then(Value::as_u64).unwrap_or(0);
        Usage {
            input_tokens: n("input_tokens"),
            output_tokens: n("output_tokens"),
            cache_read: n("cache_read_input_tokens"),
            cache_write: n("cache_creation_input_tokens"),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0 && self.cache_read == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeVariant {
    Info,
    Command,
    Compact,
    Error,
    Web,
    /// The session was put in another mode, on another model or at
    /// another effort level; the text is what it was set to, nothing else.
    Mode,
    Model,
    Effort,
    /// The turn was stopped by the person; the text is what the agent
    /// itself wrote of it in its transcript.
    Interrupted,
}

impl NoticeVariant {
    /// What a setting's notice is a change of, or none for the others.
    pub fn setting(self) -> Option<&'static str> {
        match self {
            NoticeVariant::Mode => Some("Mode"),
            NoticeVariant::Model => Some("Model"),
            NoticeVariant::Effort => Some("Effort"),
            _ => None,
        }
    }

    /// The notice as the page reads it: a setting is named before what it
    /// was set to ("Effort High"), anything else is its text.
    pub fn said(self, text: &str) -> String {
        match self.setting() {
            Some(what) => format!("{what} {text}"),
            None => text.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallStatus {
    Pending,
    Ok,
    Error,
    Interrupted,
    NoResult,
}

impl CallStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CallStatus::Pending => "pending",
            CallStatus::Ok => "ok",
            CallStatus::Error => "error",
            CallStatus::Interrupted => "interrupted",
            CallStatus::NoResult => "no-result",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCall {
    pub uuid: String,
    pub ts: String,
    pub id: String,
    pub name: String,
    pub tool_kind: ToolKind,
    pub input: Map<String, Value>,
    /// The one-line "what is this call" summary.
    pub subject: String,
    pub status: CallStatus,
    pub result_text: String,
    pub stdout: String,
    pub stderr: String,
    /// `structuredPatch` hunks, as written by Claude Code.
    pub patch: Vec<Value>,
    pub file_path: String,
    pub old_string: String,
    pub new_string: String,
    pub result_images: u32,
    /// The row and block the result came in, so a picture it returned can
    /// be read back out of the transcript (`transcript::image_block_bytes`).
    #[serde(default)]
    pub result_uuid: String,
    #[serde(default)]
    pub result_index: usize,
    pub duration_ms: u64,
    pub explanation: String,
    pub needs_approval: bool,
    /// Nested rounds for Task calls.
    pub subagent: Vec<Round>,
    pub agent_name: String,
    /// What the person answered an `AskUserQuestion` with, question by
    /// question, from the result's sidecar; empty until they have, or when
    /// they never did.
    #[serde(default)]
    pub answers: Vec<(String, String)>,
}

/// One question of an `AskUserQuestion` call, as the tool's input shapes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    /// The short chip the terminal shows beside the question.
    pub header: String,
    /// Each option's label and the line under it.
    pub options: Vec<(String, String)>,
    pub multi: bool,
}

/// The questions in an `AskUserQuestion` input.
pub fn questions_of(input: &Map<String, Value>) -> Vec<Question> {
    let Some(list) = input.get("questions").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|q| {
            let s = |k: &str| q.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
            let question = s("question");
            if question.is_empty() {
                return None;
            }
            let options = q
                .get("options")
                .and_then(Value::as_array)
                .map(|o| o.iter().map(|opt| (opt.get("label").and_then(Value::as_str).unwrap_or("").trim().to_string(), opt.get("description").and_then(Value::as_str).unwrap_or("").trim().to_string())).filter(|(l, _)| !l.is_empty()).collect())
                .unwrap_or_default();
            Some(Question { question, header: s("header"), options, multi: q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false) })
        })
        .collect()
}

impl Default for ToolKind {
    fn default() -> Self {
        ToolKind::Other
    }
}

impl Default for CallStatus {
    fn default() -> Self {
        CallStatus::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Item {
    Text { uuid: String, ts: String, md: String },
    Thinking { uuid: String, ts: String, md: String, seconds: f64 },
    Notice { ts: String, text: String, variant: NoticeVariant },
    Tool(ToolCall),
}

impl Item {
    pub fn ts(&self) -> &str {
        match self {
            Item::Text { ts, .. } | Item::Thinking { ts, .. } | Item::Notice { ts, .. } => ts,
            Item::Tool(c) => &c.ts,
        }
    }

    pub fn as_tool(&self) -> Option<&ToolCall> {
        match self {
            Item::Tool(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_tool_mut(&mut self) -> Option<&mut ToolCall> {
        match self {
            Item::Tool(c) => Some(c),
            _ => None,
        }
    }
}

/// Who wrote a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    User,
    /// Typed in Emaki (the web page before; this app now).
    Web,
    /// Another agent session, through the inbox.
    Peer,
    Command,
    System,
}

impl Default for Source {
    fn default() -> Self {
        Source::User
    }
}

/// What came with a prompt: a picture carried as a content block, or a file
/// named by path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attachment {
    pub kind: String, // image | file
    pub uuid: String,
    pub index: usize,
    pub path: String,
    pub name: String,
    pub media_type: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Round {
    pub index: usize,
    pub uuid: String,
    pub ts: String,
    pub end_ts: String,
    pub prompt: String,
    pub source: Source,
    pub items: Vec<Item>,
    pub usage: Usage,
    pub usage_by_model: Vec<(String, u64)>,
    pub duration_ms: u64,
    pub images: u32,
    pub attachments: Vec<Attachment>,
    /// Sent while the agent was working and not reached yet: the message
    /// is in Claude Code's queue, and this round is where it waits.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
    /// Setting notices a later one of the same kind took the place of
    /// (effort to high, then to medium: the first), in order. Not part of
    /// the round as read; kept for the window, which shows one again when
    /// something it knows of and the transcript does not yet, a change of
    /// mode, came between the two.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub superseded: Vec<Item>,
}

impl Round {
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.items.iter().filter_map(Item::as_tool)
    }

    pub fn tool_count(&self) -> usize {
        self.tool_calls().count()
    }

    pub fn has_text(&self) -> bool {
        self.items.iter().any(|i| matches!(i, Item::Text { .. }))
    }

    /// What the agent said this round, as the markdown it wrote: every text
    /// item in order with a blank line between, tool calls and thoughts
    /// left out. It is what the window's copy button puts on the clipboard.
    pub fn reply_markdown(&self) -> String {
        let texts: Vec<&str> = self.items.iter().filter_map(|i| if let Item::Text { md, .. } = i { Some(md.trim()) } else { None }).filter(|m| !m.is_empty()).collect();
        texts.join("\n\n")
    }

    pub(crate) fn add_model_usage(&mut self, model: &str, total: u64) {
        add_model_usage(&mut self.usage_by_model, model, total)
    }
}

pub(crate) fn add_model_usage(list: &mut Vec<(String, u64)>, model: &str, total: u64) {
    if let Some(entry) = list.iter_mut().find(|(m, _)| m == model) {
        entry.1 += total;
    } else {
        list.push((model.to_string(), total));
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub agent: AgentId,
    pub id: String,
    pub cwd: String,
    pub project: String,
    pub title: String,
    pub slug: String,
    pub started: String,
    pub updated: String,
    pub version: String,
    pub git_branch: String,
    pub models: Vec<String>,
    pub rounds: Vec<Round>,
    pub usage: Usage,
    pub usage_by_model: Vec<(String, u64)>,
    /// What the last request carried (input, cache read, cache creation):
    /// the context in use, against the model's window.
    pub context_tokens: u64,
    /// The size of the model's window, where the transcript says it
    /// (Codex); 0 where it does not.
    #[serde(default)]
    pub context_window: u64,
    /// The account's usage windows as the transcript last recorded them
    /// (Codex), the shorter first, and when that was, Unix seconds.
    #[serde(default)]
    pub usage_windows: Vec<UsageWindow>,
    #[serde(default)]
    pub usage_windows_at: f64,
    /// The effort level the session was last set to with `/effort`, or
    /// empty when it never was.
    pub effort: String,
    /// The permission mode the transcript last recorded; a change since
    /// then is not in the file until the next turn.
    #[serde(default)]
    pub mode: String,
    /// Whether a turn is running, for an agent whose transcript says
    /// when one starts and ends (Codex: `task_started`, then
    /// `task_complete` or `turn_aborted`). None where the rows do not
    /// say, and the state is read off the model's tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_open: Option<bool>,
    /// The last prompt, when it was stopped before the agent did anything
    /// with it: taken out of `rounds`, as Claude Code's own terminal takes
    /// it back into its input, and kept here so the window can hand it
    /// back to the composer. Cleared by the next prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawn: Option<Round>,
    /// The commands the agent set running in the background, in the
    /// order it started them, each with how it ended once it has.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shells: Vec<Shell>,
    pub transcript_path: String,
    pub log_path: String,
}

/// Something the agent started in the background: a command (`Bash`
/// with `run_in_background`) or a subagent (`Agent` launched without
/// waiting for it). The turn goes on, or ends, while it runs, and Claude
/// Code tells the agent when it is over.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Shell {
    /// Claude Code's id for the task.
    pub id: String,
    /// A subagent and not a command. `command` is then the kind of agent
    /// it is ("general-purpose") and there is no output file: what it
    /// does is in its own transcript.
    #[serde(default)]
    pub agent: bool,
    pub command: String,
    /// The agent's own line on what the command is for.
    pub description: String,
    /// The file Claude Code writes the command's output to as it runs.
    pub output_path: String,
    pub started: String,
    /// When it was over; empty while it runs.
    pub ended: String,
    /// `completed`, `failed`, `killed`, as Claude Code says it; empty
    /// while it runs.
    pub status: String,
    /// Claude Code's sentence on how it ended.
    pub summary: String,
}

impl Session {
    pub fn tool_count(&self) -> usize {
        self.rounds.iter().map(Round::tool_count).sum()
    }

    pub(crate) fn add_model_usage(&mut self, model: &str, total: u64) {
        add_model_usage(&mut self.usage_by_model, model, total)
    }
}
