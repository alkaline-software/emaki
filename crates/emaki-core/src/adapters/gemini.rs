//! Google's Gemini CLI: `~/.gemini/tmp/<project>/chats/session-<ts>-<id>.jsonl`,
//! and the same name with `.json` from before it wrote lines. Read off
//! the CLI's own recorder (`chatRecordingService` in
//! `@google/gemini-cli-core` 0.63). A file is a log, one record a line,
//! and what the session holds is what replaying it leaves:
//!
//! ```text
//! {sessionId, projectHash, startTime, kind, ..}  what the session is; first
//! {id, timestamp, type, content, ..}             a message; one with an id
//!                                                already seen takes its place
//! {"$set": {..}}                                 fields of the first record,
//!                                                and with `messages`, all of them anew
//! {"$rewindTo": id}                              that message and all after it go
//! ```
//!
//! A message's `type` is `user`, `gemini`, or `info`, `warning`, `error`.
//! One of `gemini` carries its `thoughts`, its text in `content`, its
//! `toolCalls` with each one's `result`, its `tokens` and its `model`.
//! The old `.json` file is the first record with every message in
//! `messages`.
//!
//! No record says which folder the session ran in. The project's folder
//! under `tmp` has a file, `.project_root`, that does, and the archive
//! keeps a copy of it beside the sessions it keeps (`archive_ref`).
//!
//! Like Codex's, the phase is derived from the model's tail.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{turn_state_from_session, Adapter};
use crate::build::{elapsed_ms, one_line, rel, strip_ansi};
use crate::json::*;
use crate::model::*;
use crate::paths;
use crate::transcript::{mtime_secs, read_all, SessionRef};

/// The file in a project's folder that names the folder it stands for.
pub const PROJECT_ROOT_FILE: &str = ".project_root";

pub struct GeminiAdapter {
    root: PathBuf,
}

impl GeminiAdapter {
    pub fn new() -> Self {
        Self { root: paths::gemini_home().join("tmp") }
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// A main session's file by its name. A subagent's is named for its id
/// alone, in a folder of its parent's, and is not listed.
fn is_session(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "jsonl" || e == "json") && path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("session-"))
}

/// The folder a session ran in: what `.project_root` says, beside the
/// `chats` folder where Gemini keeps the file and beside the file itself
/// in the archive.
fn project_root(path: &Path) -> String {
    let dir = path.parent();
    [dir, dir.and_then(Path::parent)].into_iter().flatten().find_map(|d| fs::read_to_string(d.join(PROJECT_ROOT_FILE)).ok()).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// The `.project_root` that goes with a session's file, where there is
/// one, for the archive to keep with it.
pub fn project_root_file(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    [Some(dir), dir.parent()].into_iter().flatten().map(|d| d.join(PROJECT_ROOT_FILE)).find(|p| p.is_file())
}

impl Adapter for GeminiAdapter {
    fn id(&self) -> AgentId {
        AgentId::Gemini
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }

    fn owns(&self, path: &Path) -> bool {
        is_session(path) && (path.starts_with(&self.root) || path.starts_with(paths::archive_dir().join(AgentId::Gemini.archive_subdir())))
    }

    fn list(&self, min_size: u64) -> Vec<SessionRef> {
        let mut files: Vec<PathBuf> = Vec::new();
        let Ok(projects) = fs::read_dir(&self.root) else { return Vec::new() };
        for project in projects.filter_map(Result::ok) {
            let Ok(chats) = fs::read_dir(project.path().join("chats")) else { continue };
            files.extend(chats.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file() && is_session(p)));
        }
        // A session carried over from the old format has both files; the
        // one written now is the one read.
        files.sort_by_key(|p| (p.extension().is_some_and(|e| e == "json"), p.clone()));
        let mut seen = std::collections::HashSet::new();
        files
            .into_iter()
            .filter(|p| fs::metadata(p).map(|m| m.len() >= min_size).unwrap_or(false))
            .map(|p| self.peek(&p))
            .filter(|r| !r.session_id.is_empty() && !r.title.is_empty() && seen.insert(r.session_id.clone()))
            .collect()
    }

    fn peek(&self, path: &Path) -> SessionRef {
        // A full parse, as for Codex: these files are small.
        let dir = path.parent();
        let project = if dir.and_then(Path::file_name).is_some_and(|n| n == "chats") { dir.and_then(Path::parent) } else { dir };
        let mut r = SessionRef { agent: AgentId::Gemini, path: path.to_path_buf(), project_dir: project.and_then(Path::file_name).map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), ..Default::default() };
        let Ok(st) = fs::metadata(path) else { return r };
        r.size = st.len();
        r.mtime = mtime_secs(&st);
        let s = self.load_path(path, "");
        r.session_id = s.id.clone();
        r.cwd = s.cwd.clone();
        r.title = s.title.clone();
        r.started = s.started.clone();
        r.updated = s.updated.clone();
        r.state = turn_state_from_session(&s);
        r
    }

    fn load_path(&self, path: &Path, cwd_hint: &str) -> Session {
        // The old file is one record over many lines; the new one a
        // record a line.
        let rows = if path.extension().is_some_and(|e| e == "json") { fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).into_iter().collect() } else { read_all(path) };
        let root = project_root(path);
        let mut s = build_gemini(&rows, if root.is_empty() { cwd_hint } else { &root });
        s.transcript_path = path.to_string_lossy().to_string();
        if s.id.is_empty() {
            s.id = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        }
        s
    }
}

/// The text of a message's content: a string, or parts of which the
/// ones with `text` are joined; and how many of the parts are pictures
/// or files.
fn content_text(content: Option<&Value>) -> (String, u32) {
    match content {
        Some(Value::String(s)) => (s.clone(), 0),
        Some(Value::Array(parts)) => {
            let text: Vec<&str> = parts.iter().map(|p| p.as_str().unwrap_or_else(|| str_of(p, "text"))).filter(|t| !t.is_empty()).collect();
            let images = parts.iter().filter(|p| p.get("inlineData").is_some() || p.get("fileData").is_some()).count() as u32;
            (text.join(""), images)
        }
        Some(v @ Value::Object(_)) => (str_of(v, "text").to_string(), 0),
        _ => (String::new(), 0),
    }
}

/// Whether a `user` message is a tool's answer handed back to the model
/// and not something the person said.
fn is_tool_answer(content: Option<&Value>) -> bool {
    content.and_then(Value::as_array).is_some_and(|parts| parts.iter().any(|p| p.get("functionResponse").is_some()))
}

/// What a tool call came back with, as text: the call's own
/// `functionResponse` among the parts of its `result` (some versions
/// hand every call of a turn the whole turn's answers), else what was
/// shown of it.
fn result_text(call: &Value) -> String {
    let (id, name) = (str_of(call, "id"), str_of(call, "name"));
    let mut out: Vec<String> = Vec::new();
    for part in arr_of(call, "result") {
        let Some(fr) = part.get("functionResponse") else {
            let t = str_of(part, "text");
            if !t.is_empty() {
                out.push(t.to_string());
            }
            continue;
        };
        let (fid, fname) = (str_of(fr, "id"), str_of(fr, "name"));
        if !(fid == id || (fid.is_empty() && fname == name)) {
            continue;
        }
        let Some(resp) = fr.get("response") else { continue };
        let said = ["output", "error", "content"].iter().map(|k| str_of(resp, k)).find(|t| !t.is_empty());
        out.push(match said {
            Some(t) => t.to_string(),
            None => serde_json::to_string_pretty(resp).unwrap_or_default(),
        });
    }
    if out.is_empty() {
        if let Some(Value::String(shown)) = call.get("resultDisplay") {
            out.push(shown.clone());
        }
    }
    strip_ansi(out.join("\n").trim())
}

/// Gemini's tools by the family the window draws them as.
fn gemini_tool_kind(name: &str) -> ToolKind {
    match name {
        "run_shell_command" => ToolKind::Bash,
        "replace" | "edit" => ToolKind::Edit,
        "write_file" => ToolKind::Write,
        "read_file" | "read_many_files" | "list_directory" => ToolKind::Read,
        "glob" | "search_file_content" | "grep_search" | "grep" => ToolKind::Search,
        "web_fetch" | "google_web_search" => ToolKind::Web,
        "write_todos" => ToolKind::Todo,
        "ask_user" => ToolKind::Ask,
        "enter_plan_mode" | "exit_plan_mode" => ToolKind::Plan,
        _ => tool_kind(name),
    }
}

/// The one line that says what a call is about.
fn subject(call: &Value, input: &Map<String, Value>, cwd: &str) -> String {
    let arg = |keys: &[&str]| keys.iter().filter_map(|k| input.get(*k).and_then(Value::as_str)).find(|v| !v.is_empty()).unwrap_or("").to_string();
    let said = match str_of(call, "name") {
        "run_shell_command" => arg(&["command"]),
        "read_file" | "write_file" | "replace" | "edit" => rel(&arg(&["file_path", "absolute_path", "path"]), cwd),
        "list_directory" => rel(&arg(&["dir_path", "path"]), cwd),
        "glob" | "search_file_content" | "grep_search" | "grep" => arg(&["pattern", "query"]),
        "web_fetch" => arg(&["url", "prompt"]),
        "google_web_search" => arg(&["query"]),
        _ => String::new(),
    };
    let said = if said.is_empty() { str_of(call, "description").to_string() } else { said };
    one_line(if said.is_empty() { str_of(call, "displayName") } else { &said }, 110)
}

/// The messages a log leaves, in the order they first came: a message
/// written again takes the place of the one before it, as the recorder's
/// own map does.
fn replay(rows: &[Value]) -> (Map<String, Value>, Vec<Value>) {
    let mut meta: Map<String, Value> = Map::new();
    let mut order: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    fn put(order: &mut Vec<String>, by_id: &mut std::collections::HashMap<String, Value>, m: &Value) {
        let id = str_of(m, "id");
        if id.is_empty() || str_of(m, "type").is_empty() {
            return;
        }
        if by_id.insert(id.to_string(), m.clone()).is_none() {
            order.push(id.to_string());
        }
    }
    let take_meta = |meta: &mut Map<String, Value>, from: &Value| {
        for (k, v) in from.as_object().into_iter().flatten().filter(|(k, _)| *k != "messages") {
            meta.insert(k.clone(), v.clone());
        }
    };
    for row in rows {
        if let Some(to) = row.get("$rewindTo").and_then(Value::as_str) {
            match order.iter().position(|id| id == to) {
                Some(at) => {
                    for gone in order.drain(at..) {
                        by_id.remove(&gone);
                    }
                }
                None => {
                    order.clear();
                    by_id.clear();
                }
            }
        } else if let Some(set) = row.get("$set") {
            if let Some(all) = set.get("messages").and_then(Value::as_array) {
                order.clear();
                by_id.clear();
                for m in all {
                    put(&mut order, &mut by_id, m);
                }
            }
            take_meta(&mut meta, set);
        } else if !str_of(row, "id").is_empty() && !str_of(row, "type").is_empty() {
            put(&mut order, &mut by_id, row);
        } else if !str_of(row, "sessionId").is_empty() {
            take_meta(&mut meta, row);
            for m in arr_of(row, "messages") {
                put(&mut order, &mut by_id, m);
            }
        }
    }
    (meta, order.iter().filter_map(|id| by_id.remove(id)).collect())
}

pub fn build_gemini(rows: &[Value], cwd: &str) -> Session {
    let (meta, messages) = replay(rows);
    let meta = Value::Object(meta);
    let mut s = Session { agent: AgentId::Gemini, id: str_of(&meta, "sessionId").into(), cwd: cwd.into(), started: str_of(&meta, "startTime").into(), ..Default::default() };
    // With no `.project_root` to go by, the first folder the session was
    // given.
    if s.cwd.is_empty() {
        s.cwd = arr_of(&meta, "directories").first().and_then(Value::as_str).unwrap_or("").into();
    }
    let mut rounds: Vec<Round> = Vec::new();
    let mut last_ts = String::new();
    let ensure_round = |rounds: &mut Vec<Round>, ts: &str| {
        if rounds.is_empty() {
            rounds.push(Round { index: 1, ts: ts.into(), source: Source::System, ..Default::default() });
        }
    };

    for m in &messages {
        let ts = str_of(m, "timestamp");
        if !ts.is_empty() {
            if s.started.is_empty() {
                s.started = ts.into();
            }
            last_ts = ts.into();
        }
        match str_of(m, "type") {
            "user" => {
                if is_tool_answer(m.get("content")) {
                    continue;
                }
                // What the person typed, before a file named with "@" was
                // put in its place, where the recorder kept that.
                let (text, images) = content_text(m.get("displayContent").or(m.get("content")));
                let text = text.trim().to_string();
                if text.is_empty() && images == 0 {
                    continue;
                }
                rounds.push(Round { index: rounds.len() + 1, ts: ts.into(), prompt: text, source: Source::User, images, ..Default::default() });
            }
            "gemini" => {
                ensure_round(&mut rounds, ts);
                let rnd = rounds.last_mut().unwrap();
                for t in arr_of(m, "thoughts") {
                    let (about, said) = (str_of(t, "subject").trim(), str_of(t, "description").trim());
                    let md = match (about.is_empty(), said.is_empty()) {
                        (false, false) => format!("**{about}**\n\n{said}"),
                        (false, true) => format!("**{about}**"),
                        (true, false) => said.to_string(),
                        (true, true) => continue,
                    };
                    let at = if str_of(t, "timestamp").is_empty() { ts } else { str_of(t, "timestamp") };
                    let prev = rnd.items.last().map(|i| i.ts().to_string()).unwrap_or_else(|| rnd.ts.clone());
                    rnd.items.push(Item::Thinking { uuid: String::new(), ts: at.into(), md, seconds: elapsed_ms(&prev, at) as f64 / 1000.0 });
                }
                let (text, _) = content_text(m.get("content"));
                if !text.trim().is_empty() {
                    rnd.items.push(Item::Text { uuid: str_of(m, "id").into(), ts: ts.into(), md: text.trim().into() });
                }
                for call in arr_of(m, "toolCalls") {
                    let name = str_of(call, "name");
                    let name = if name.is_empty() { "tool" } else { name };
                    let input = obj_of(call, "args").cloned().unwrap_or_default();
                    let ended = str_of(call, "timestamp");
                    let kind = gemini_tool_kind(name);
                    let result = result_text(call);
                    rnd.items.push(Item::Tool(ToolCall {
                        ts: ts.into(),
                        id: str_of(call, "id").into(),
                        tool_kind: kind,
                        subject: subject(call, &input, &s.cwd),
                        name: name.into(),
                        status: match str_of(call, "status") {
                            "success" => CallStatus::Ok,
                            "error" => CallStatus::Error,
                            "cancelled" => CallStatus::Interrupted,
                            _ => CallStatus::Pending,
                        },
                        duration_ms: if ended.is_empty() { 0 } else { elapsed_ms(ts, ended) },
                        stdout: if kind == ToolKind::Bash { result.clone() } else { String::new() },
                        result_text: result,
                        input,
                        ..Default::default()
                    }));
                }
                // What the turn was billed: Gemini counts cached tokens
                // inside the input, and thoughts beside the output.
                if let Some(t) = m.get("tokens").filter(|t| t.is_object()) {
                    let cached = u64_of(t, "cached");
                    let u = Usage { input_tokens: u64_of(t, "input").saturating_sub(cached), output_tokens: u64_of(t, "output") + u64_of(t, "thoughts"), cache_read: cached, cache_write: 0 };
                    let model = str_of(m, "model");
                    if !model.is_empty() {
                        if !s.models.iter().any(|x| x == model) {
                            s.models.push(model.into());
                        }
                        s.add_model_usage(model, u.total());
                    }
                    s.usage.add(&u);
                }
            }
            kind @ ("error" | "warning" | "info") => {
                let (text, _) = content_text(m.get("content"));
                if text.trim().is_empty() {
                    continue;
                }
                ensure_round(&mut rounds, ts);
                rounds.last_mut().unwrap().items.push(Item::Notice { ts: ts.into(), text: text.trim().into(), variant: if kind == "error" { NoticeVariant::Error } else { NoticeVariant::Info } });
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
    s.updated = rounds.last().map(|r| if r.end_ts.is_empty() { r.ts.clone() } else { r.end_ts.clone() }).unwrap_or(last_ts);
    // The name Gemini gave the session, else its first prompt.
    let named = str_of(&meta, "summary").trim();
    s.title = if named.is_empty() { rounds.iter().find(|r| !r.prompt.is_empty()).map(|r| one_line(&r.prompt, 72)).unwrap_or_default() } else { one_line(named, 72) };
    s.rounds = rounds;
    s
}
