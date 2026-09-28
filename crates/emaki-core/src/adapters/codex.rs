//! OpenAI Codex CLI: `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`
//! (and `archived_sessions/`). One row per event; the ones that matter:
//!
//! ```text
//! session_meta   payload.{id, cwd, originator, cli_version, git.branch}
//! turn_context   payload.{cwd, model}
//! response_item  payload.type = message | reasoning | function_call
//!                | local_shell_call | function_call_output
//! event_msg      payload.type = token_count (usage), agent_message, ...
//! ```
//!
//! Codex has no AI-written title and no stop reason, so the title is the first
//! prompt and the phase is derived from the model's tail.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::{turn_state_from_session, Adapter};
use crate::build::{elapsed_ms, one_line, tool_subject};
use crate::json::*;
use crate::model::*;
use crate::paths;
use crate::transcript::{mtime_secs, read_all, SessionRef};

pub struct CodexAdapter {
    roots: Vec<PathBuf>,
}

impl CodexAdapter {
    pub fn new() -> Self {
        let home = paths::codex_home();
        Self { roots: vec![home.join("sessions"), home.join("archived_sessions")] }
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn is_rollout(path: &Path) -> bool {
    path.extension().map(|e| e == "jsonl").unwrap_or(false)
        && path.file_name().map(|n| n.to_string_lossy().starts_with("rollout-")).unwrap_or(false)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else if is_rollout(&p) {
            out.push(p);
        }
    }
}

impl Adapter for CodexAdapter {
    fn id(&self) -> AgentId {
        AgentId::Codex
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }

    fn owns(&self, path: &Path) -> bool {
        is_rollout(path)
    }

    fn list(&self, min_size: u64) -> Vec<SessionRef> {
        let mut files = Vec::new();
        for root in &self.roots {
            walk(root, &mut files);
        }
        files
            .into_iter()
            .filter(|p| fs::metadata(p).map(|m| m.len() >= min_size).unwrap_or(false))
            .map(|p| self.peek(&p))
            .filter(|r| !r.session_id.is_empty() && !r.title.is_empty())
            .collect()
    }

    fn peek(&self, path: &Path) -> SessionRef {
        // Rollouts are small next to Claude transcripts; a full parse is the
        // cheap path, and it is cached on (size, mtime) like Claude's peek.
        let mut r = SessionRef {
            agent: AgentId::Codex,
            path: path.to_path_buf(),
            project_dir: path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            ..Default::default()
        };
        let Ok(st) = fs::metadata(path) else { return r };
        r.size = st.len();
        r.mtime = mtime_secs(&st);
        let s = self.load_path(path, "");
        r.session_id = s.id.clone();
        r.cwd = s.cwd.clone();
        r.title = s.title.clone();
        r.started = s.started.clone();
        r.updated = s.updated.clone();
        r.git_branch = s.git_branch.clone();
        r.version = s.version.clone();
        r.state = turn_state_from_session(&s);
        r
    }

    fn load_path(&self, path: &Path, cwd_hint: &str) -> Session {
        let rows = read_all(path);
        let mut s = build_codex(&rows, cwd_hint);
        s.transcript_path = path.to_string_lossy().to_string();
        if s.id.is_empty() {
            s.id = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        }
        s
    }
}

static RE_WRAPPERS: LazyLock<Regex> = LazyLock::new(|| {
    // No backreferences in Rust's regex: one alternative per wrapper tag.
    Regex::new(r"(?s)<environment_context>.*?</environment_context>\s*|<user_instructions>.*?</user_instructions>\s*|<INSTRUCTIONS>.*?</INSTRUCTIONS>\s*|<permissions instructions>.*?</permissions instructions>\s*|<collaboration_mode>.*?</collaboration_mode>\s*|<turn_aborted>.*?</turn_aborted>\s*|<app_context>.*?</app_context>\s*|<system_reminder>.*?</system_reminder>\s*|<developer_instructions>.*?</developer_instructions>\s*|<command-name>.*?</command-name>\s*|<command-message>.*?</command-message>\s*|<command-args>.*?</command-args>\s*|<local-command-stdout>.*?</local-command-stdout>\s*|# AGENTS\.md instructions[^\n]*\s*").unwrap()
});

fn clean_user_text(text: &str) -> String {
    RE_WRAPPERS.replace_all(text, "").trim().to_string()
}

fn content_text(content: Option<&Value>) -> (String, u32) {
    match content {
        Some(Value::String(s)) => (s.clone(), 0),
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            let mut images = 0;
            for b in items {
                match block_type(b) {
                    "input_text" | "output_text" | "text" => {
                        let t = str_of(b, "text");
                        if !t.is_empty() {
                            parts.push(t.to_string());
                        }
                    }
                    "input_image" | "image" => images += 1,
                    _ => {}
                }
            }
            (parts.join("\n"), images)
        }
        _ => (String::new(), 0),
    }
}

fn parse_arguments(raw: Option<&Value>) -> Map<String, Value> {
    match raw {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s).ok().and_then(|v| v.as_object().cloned()).unwrap_or_else(|| {
            let mut m = Map::new();
            m.insert("arguments".into(), Value::String(s.clone()));
            m
        }),
        Some(Value::Object(o)) => o.clone(),
        _ => Map::new(),
    }
}

fn find_call<'a>(rounds: &'a mut [Round], id: &str) -> Option<&'a mut ToolCall> {
    for rnd in rounds.iter_mut().rev() {
        for item in rnd.items.iter_mut().rev() {
            if let Item::Tool(c) = item {
                if c.id == id {
                    return Some(c);
                }
            }
        }
    }
    None
}

pub fn build_codex(rows: &[Value], cwd_hint: &str) -> Session {
    let mut s = Session { agent: AgentId::Codex, cwd: cwd_hint.into(), ..Default::default() };
    let mut rounds: Vec<Round> = Vec::new();
    let mut last_ts = String::new();
    let mut last_usage = Usage::default();

    let ensure_round = |rounds: &mut Vec<Round>, ts: &str| {
        if rounds.is_empty() {
            rounds.push(Round { index: 1, ts: ts.into(), source: Source::System, ..Default::default() });
        }
    };

    for row in rows {
        let ts = str_of(row, "timestamp");
        if !ts.is_empty() {
            if s.started.is_empty() {
                s.started = ts.into();
            }
            last_ts = ts.into();
        }
        let Some(payload) = row.get("payload") else { continue };
        match str_of(row, "type") {
            "session_meta" => {
                if s.id.is_empty() {
                    s.id = str_of(payload, "id").into();
                }
                if !str_of(payload, "cwd").is_empty() {
                    s.cwd = str_of(payload, "cwd").into();
                }
                if !str_of(payload, "cli_version").is_empty() {
                    s.version = str_of(payload, "cli_version").into();
                }
                if let Some(b) = payload.get("git").map(|g| str_of(g, "branch")) {
                    if !b.is_empty() {
                        s.git_branch = b.into();
                    }
                }
            }
            "turn_context" => {
                if s.cwd.is_empty() && !str_of(payload, "cwd").is_empty() {
                    s.cwd = str_of(payload, "cwd").into();
                }
                let m = str_of(payload, "model");
                if !m.is_empty() && !s.models.iter().any(|x| x == m) {
                    s.models.push(m.into());
                }
            }
            "response_item" => match str_of(payload, "type") {
                "message" => {
                    let role = str_of(payload, "role");
                    let (text, images) = content_text(payload.get("content"));
                    match role {
                        "user" => {
                            let text = clean_user_text(&text);
                            if text.is_empty() && images == 0 {
                                continue;
                            }
                            rounds.push(Round {
                                index: rounds.len() + 1,
                                ts: ts.into(),
                                prompt: text,
                                source: Source::User,
                                images,
                                ..Default::default()
                            });
                        }
                        "assistant" => {
                            let text = text.trim();
                            if text.is_empty() {
                                continue;
                            }
                            ensure_round(&mut rounds, ts);
                            rounds.last_mut().unwrap().items.push(Item::Text { uuid: String::new(), ts: ts.into(), md: text.into() });
                        }
                        _ => {}
                    }
                }
                "reasoning" => {
                    let parts: Vec<&str> = arr_of(payload, "summary").iter().map(|x| str_of(x, "text")).filter(|t| !t.is_empty()).collect();
                    if parts.is_empty() {
                        continue;
                    }
                    ensure_round(&mut rounds, ts);
                    let rnd = rounds.last_mut().unwrap();
                    let prev = rnd.items.last().map(|i| i.ts().to_string()).unwrap_or_else(|| rnd.ts.clone());
                    let seconds = elapsed_ms(&prev, ts) as f64 / 1000.0;
                    rnd.items.push(Item::Thinking { uuid: String::new(), ts: ts.into(), md: parts.join("\n\n"), seconds });
                }
                "function_call" | "custom_tool_call" | "local_shell_call" => {
                    ensure_round(&mut rounds, ts);
                    let (name, input) = if str_of(payload, "type") == "local_shell_call" {
                        let mut m = Map::new();
                        if let Some(cmd) = payload.get("action").and_then(|a| a.get("command")) {
                            m.insert("command".into(), cmd.clone());
                        }
                        ("shell".to_string(), m)
                    } else {
                        let name = str_of(payload, "name");
                        let name = if name.is_empty() { "tool" } else { name };
                        let args = payload.get("arguments").or(payload.get("input"));
                        (name.to_string(), parse_arguments(args))
                    };
                    let call = ToolCall {
                        ts: ts.into(),
                        id: str_of(payload, "call_id").into(),
                        tool_kind: tool_kind(&name),
                        subject: tool_subject(&name, &input, &s.cwd),
                        name,
                        input,
                        ..Default::default()
                    };
                    rounds.last_mut().unwrap().items.push(Item::Tool(call));
                }
                "function_call_output" | "custom_tool_call_output" => {
                    let id = str_of(payload, "call_id");
                    let (output, _) = match payload.get("output") {
                        Some(Value::String(o)) => (o.clone(), 0),
                        other => content_text(other),
                    };
                    if let Some(call) = find_call(&mut rounds, id) {
                        call.result_text = crate::build::strip_ansi(&output);
                        call.duration_ms = elapsed_ms(&call.ts, ts);
                        call.status = CallStatus::Ok;
                        if call.tool_kind == ToolKind::Bash {
                            call.stdout = call.result_text.clone();
                        }
                    }
                }
                _ => {}
            },
            "event_msg" => {
                if str_of(payload, "type") == "token_count" {
                    // Cumulative totals; keep the latest and diff into the session at the end.
                    if let Some(u) = payload.get("info").and_then(|i| i.get("total_token_usage")) {
                        last_usage = Usage {
                            input_tokens: u64_of(u, "input_tokens"),
                            output_tokens: u64_of(u, "output_tokens"),
                            cache_read: u64_of(u, "cached_input_tokens"),
                            cache_write: 0,
                        };
                        // Codex counts cached tokens inside input_tokens.
                        last_usage.input_tokens = last_usage.input_tokens.saturating_sub(last_usage.cache_read);
                    }
                }
            }
            "compacted" => {
                ensure_round(&mut rounds, ts);
                rounds.last_mut().unwrap().items.push(Item::Notice {
                    ts: ts.into(),
                    text: "Context compacted, earlier messages summarised".into(),
                    variant: NoticeVariant::Compact,
                });
            }
            _ => {}
        }
    }

    let n = rounds.len();
    for (i, rnd) in rounds.iter_mut().enumerate() {
        let mut last = rnd.ts.clone();
        for item in &rnd.items {
            if !item.ts().is_empty() && item.ts() > last.as_str() {
                last = item.ts().into();
            }
        }
        rnd.end_ts = last.clone();
        rnd.duration_ms = elapsed_ms(&rnd.ts, &last);
        if i + 1 != n {
            for item in &mut rnd.items {
                if let Some(c) = item.as_tool_mut() {
                    if c.status == CallStatus::Pending {
                        c.status = CallStatus::NoResult;
                    }
                }
            }
        }
    }
    s.usage = last_usage;
    if let Some(m) = s.models.last().cloned() {
        let t = s.usage.total();
        if t > 0 {
            s.add_model_usage(&m, t);
        }
    }
    s.updated = rounds.last().map(|r| if r.end_ts.is_empty() { r.ts.clone() } else { r.end_ts.clone() }).unwrap_or(last_ts);
    s.title = rounds.iter().find(|r| !r.prompt.is_empty()).map(|r| one_line(&r.prompt, 72)).unwrap_or_default();
    s.rounds = rounds;
    s
}
