//! OpenAI Codex CLI: `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`
//! (and `archived_sessions/`). One row per event; the ones that matter:
//!
//! ```text
//! session_meta   payload.{id, cwd, originator, cli_version, git.branch}
//! turn_context   payload.{cwd, model, effort, collaboration_mode,
//!                active_permission_profile, sandbox_policy}
//! response_item  payload.type = message | reasoning | function_call
//!                | custom_tool_call | local_shell_call | *_output
//! event_msg      payload.type = task_started | task_complete | turn_aborted
//!                | item_completed | thread_settings_applied | token_count
//! ```
//!
//! A rollout says most things twice. What the model was sent and what it
//! answered are `response_item` rows, and every Codex writes those. A newer
//! one also writes what happened as `item_completed` rows: the prompt, each
//! thing the agent said, each command run and each file changed. **The
//! items are the source where a rollout has them**, since they say what ran
//! and not what the model asked for; a response row that says what an item
//! already said is dropped, and one with no item to match is read as
//! before. That is decided row by row and not once a file, because a
//! rollout can change hands between versions and an imported one has items
//! for some turns only.
//!
//! Codex has no AI-written title: a thread's name is in
//! `session_index.jsonl` when the person gave one, else the title is the
//! first prompt.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use serde_json::{Map, Value};

use super::{turn_state_from_session, Adapter};
use crate::build::{elapsed_ms, media_type_for, one_line, push_interrupted, strip_ansi, tool_subject};
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

    /// Asked of a Codex app server started for the question
    /// (`codex::catalogue`): no model call, no session.
    fn catalogue(&self, cwd: &str) -> crate::options::Catalogue {
        crate::codex::catalogue(cwd).unwrap_or_default()
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
        r.named = thread_name(&s.id);
        r.started = s.started.clone();
        r.updated = s.updated.clone();
        r.git_branch = s.git_branch.clone();
        r.version = s.version.clone();
        r.state = turn_state_from_session(&s);
        r.state.mode = s.mode.clone();
        r
    }

    fn load_path(&self, path: &Path, cwd_hint: &str) -> Session {
        let rows = read_all(path);
        let mut s = build_codex(&rows, cwd_hint);
        s.transcript_path = path.to_string_lossy().to_string();
        if s.id.is_empty() {
            s.id = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        }
        let name = thread_name(&s.id);
        if !name.is_empty() {
            s.title = name;
        }
        s
    }
}

/// The name the person gave a thread, or nothing. Codex keeps names apart
/// from the rollouts, a row a naming in `session_index.jsonl`, the newest
/// last. Read once and again when the file changes: `peek` asks for every
/// session listed.
fn thread_name(id: &str) -> String {
    static NAMES: LazyLock<Mutex<(PathBuf, u64, f64, HashMap<String, String>)>> = LazyLock::new(Default::default);
    if id.is_empty() {
        return String::new();
    }
    let path = paths::codex_home().join("session_index.jsonl");
    let Ok(st) = fs::metadata(&path) else { return String::new() };
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if names.0 != path || names.1 != st.len() || names.2 != mtime_secs(&st) {
        let mut map = HashMap::new();
        for row in read_all(&path) {
            let name = str_of(&row, "thread_name").trim();
            if name.is_empty() {
                map.remove(str_of(&row, "id"));
            } else {
                map.insert(str_of(&row, "id").to_string(), name.to_string());
            }
        }
        *names = (path, st.len(), mtime_secs(&st), map);
    }
    names.3.get(id).cloned().unwrap_or_default()
}

static RE_WRAPPERS: LazyLock<Regex> = LazyLock::new(|| {
    // No backreferences in Rust's regex: one alternative per wrapper tag.
    Regex::new(r"(?s)<environment_context>.*?</environment_context>\s*|<user_instructions>.*?</user_instructions>\s*|<INSTRUCTIONS>.*?</INSTRUCTIONS>\s*|<permissions instructions>.*?</permissions instructions>\s*|<collaboration_mode>.*?</collaboration_mode>\s*|<turn_aborted>.*?</turn_aborted>\s*|<app_context>.*?</app_context>\s*|<system_reminder>.*?</system_reminder>\s*|<developer_instructions>.*?</developer_instructions>\s*|<command-name>.*?</command-name>\s*|<command-message>.*?</command-message>\s*|<command-args>.*?</command-args>\s*|<local-command-stdout>.*?</local-command-stdout>\s*|# AGENTS\.md instructions[^\n]*\s*").unwrap()
});

/// A script's call of the shell tool, up to the end of its command, which
/// the model writes as a JSON string.
static RE_SCRIPT_CMD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"tools\.exec_command\(\s*\{\s*"?cmd"?\s*:\s*("(?:[^"\\]|\\.)*")"#).unwrap());

static RE_HUNK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@@ -(\d+)(?:,\d+)? \+(\d+)").unwrap());

/// What Codex wrote of a stopped turn: the first sentence inside
/// `<turn_aborted>`, the rest being advice to the model.
fn aborted_said(text: &str) -> Option<String> {
    let inner = text.split_once("<turn_aborted>")?.1.split_once("</turn_aborted>")?.0.trim();
    let first = inner.lines().next().unwrap_or("");
    let first = first.split_inclusive(". ").next().unwrap_or(first).trim();
    (!first.is_empty()).then(|| first.to_string())
}

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
                    "input_text" | "output_text" | "text" | "Text" | "inputText" => {
                        let t = str_of(b, "text");
                        if !t.is_empty() {
                            parts.push(t.to_string());
                        }
                    }
                    "input_image" | "image" | "localImage" | "local_image" | "inputImage" => images += 1,
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

// ------------------------------------------------------------------ items

/// A field of an item. The rollout writes them in snake case
/// (`aggregated_output`); the app server's protocol, which names the same
/// things, in camel case. Either is read.
fn fld<'a>(v: &'a Value, snake: &str) -> Option<&'a Value> {
    v.get(snake).filter(|x| !x.is_null()).or_else(|| {
        let mut camel = String::with_capacity(snake.len());
        let mut up = false;
        for c in snake.chars() {
            if c == '_' {
                up = true;
            } else if up {
                camel.extend(c.to_uppercase());
                up = false;
            } else {
                camel.push(c);
            }
        }
        v.get(&camel).filter(|x| !x.is_null())
    })
}

fn fstr<'a>(v: &'a Value, snake: &str) -> &'a str {
    fld(v, snake).and_then(Value::as_str).unwrap_or("")
}

/// How an item ended, in the model's words.
fn item_status(status: &str) -> CallStatus {
    match status.replace('_', "").to_lowercase().as_str() {
        "inprogress" => CallStatus::Pending,
        "failed" | "declined" => CallStatus::Error,
        "interrupted" => CallStatus::Interrupted,
        _ => CallStatus::Ok,
    }
}

fn item_duration(item: &Value) -> u64 {
    match fld(item, "duration") {
        Some(d) => u64_of(d, "secs") * 1000 + u64_of(d, "nanos") / 1_000_000,
        None => fld(item, "duration_ms").and_then(Value::as_u64).unwrap_or(0),
    }
}

/// A command as the person would type it. Codex records the argv it ran,
/// which for nearly every command is a shell, `-lc` and the line.
fn shell_command(command: Option<&Value>) -> String {
    match command {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => {
            let args: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
            match args.as_slice() {
                [shell, flag, line] if shell.ends_with("sh") && matches!(*flag, "-lc" | "-c") => line.to_string(),
                _ => args.join(" "),
            }
        }
        _ => String::new(),
    }
}

/// A command Codex ran, as a `Bash` call.
fn command_call(item: &Value, ts: &str, cwd: &str) -> ToolCall {
    let mut input = Map::new();
    input.insert("command".into(), shell_command(fld(item, "command")).into());
    let out = strip_ansi(fstr(item, "aggregated_output"));
    let status = item_status(fstr(item, "status"));
    let code = fld(item, "exit_code").and_then(Value::as_i64).unwrap_or(0);
    let result_text = if status == CallStatus::Error && fstr(item, "status") == "declined" && out.is_empty() {
        "Declined".to_string()
    } else if status == CallStatus::Error && code > 0 {
        format!("Exit code {code}\n{out}").trim_end().to_string()
    } else {
        out.clone()
    };
    ToolCall {
        ts: ts.into(),
        id: str_of(item, "id").into(),
        name: "Bash".into(),
        tool_kind: ToolKind::Bash,
        subject: tool_subject("Bash", &input, cwd),
        input,
        status,
        stdout: out,
        result_text,
        duration_ms: item_duration(item),
        ..Default::default()
    }
}

/// One file of a patch.
#[derive(Default)]
struct Change {
    path: String,
    /// `add`, `delete` or `update`.
    kind: String,
    move_path: String,
    /// The whole file, for one added or deleted.
    content: String,
    /// The hunks of an update, in the shape of Claude Code's
    /// `structuredPatch`.
    hunks: Vec<Value>,
}

/// A hunk as `structuredPatch` has one: where it starts on each side, how
/// many lines each side has, and the lines with their marks.
fn hunk(old_start: u64, new_start: u64, lines: Vec<String>) -> Value {
    let old = lines.iter().filter(|l| !l.starts_with('+')).count();
    let new = lines.iter().filter(|l| !l.starts_with('-')).count();
    serde_json::json!({"oldStart": old_start, "oldLines": old, "newStart": new_start, "newLines": new, "lines": lines})
}

/// A unified diff as hunks. The file headers go; a diff with no `@@` line
/// is one hunk that does not say where it starts.
fn parse_unified(diff: &str) -> Vec<Value> {
    let mut hunks = Vec::new();
    let mut open: Option<(u64, u64, Vec<String>)> = None;
    for line in diff.lines() {
        if line.starts_with("@@") {
            if let Some((o, n, lines)) = open.take().filter(|h| !h.2.is_empty()) {
                hunks.push(hunk(o, n, lines));
            }
            let at = RE_HUNK.captures(line).map(|c| (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0))).unwrap_or((0, 0));
            open = Some((at.0, at.1, Vec::new()));
            continue;
        }
        if line.starts_with('\\') || (open.is_none() && (line.starts_with("---") || line.starts_with("+++") || line.starts_with("diff ") || line.starts_with("index "))) {
            continue;
        }
        if line.is_empty() || line.starts_with(['+', '-', ' ']) {
            let lines = &mut open.get_or_insert_with(|| (0, 0, Vec::new())).2;
            lines.push(if line.is_empty() { " ".into() } else { line.to_string() });
        }
    }
    if let Some((o, n, lines)) = open.filter(|h| !h.2.is_empty()) {
        hunks.push(hunk(o, n, lines));
    }
    hunks
}

/// The files of an `apply_patch` envelope ("*** Begin Patch", then a
/// section a file). Its hunks carry no line numbers.
fn parse_apply_patch(text: &str) -> Vec<Change> {
    fn close(changes: &mut [Change], lines: &mut Vec<String>) {
        if let (Some(c), false) = (changes.last_mut(), lines.is_empty()) {
            c.hunks.push(hunk(0, 0, std::mem::take(lines)));
        }
        lines.clear();
    }
    let mut changes: Vec<Change> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let section = [("*** Add File: ", "add"), ("*** Delete File: ", "delete"), ("*** Update File: ", "update")].iter().find_map(|(mark, kind)| line.strip_prefix(mark).map(|p| (p, *kind)));
        if let Some((path, kind)) = section {
            close(&mut changes, &mut lines);
            changes.push(Change { path: path.trim().into(), kind: kind.into(), ..Default::default() });
            continue;
        }
        let Some(c) = changes.last_mut() else { continue };
        if let Some(to) = line.strip_prefix("*** Move to: ") {
            c.move_path = to.trim().into();
        } else if line.starts_with("*** ") {
            // "*** End Patch", "*** End of File".
            close(&mut changes, &mut lines);
        } else if c.kind == "add" {
            c.content.push_str(line.strip_prefix('+').unwrap_or(line));
            c.content.push('\n');
        } else if line.starts_with("@@") {
            close(&mut changes, &mut lines);
        } else if c.kind == "update" {
            lines.push(if line.is_empty() { " ".into() } else { line.to_string() });
        }
    }
    close(&mut changes, &mut lines);
    changes
}

/// The files a `FileChange` item changed. The rollout has a map from path
/// to `{type, content | unified_diff, move_path}`; the app server's
/// protocol a list of `{path, kind: {type, move_path}, diff}`.
fn changes_of(item: &Value) -> Vec<Change> {
    let one = |path: &str, kind: &str, body: &str, move_path: &str| {
        let mut c = Change { path: path.into(), kind: kind.into(), move_path: move_path.into(), ..Default::default() };
        if kind == "update" {
            c.hunks = parse_unified(body);
        } else {
            c.content = body.into();
        }
        c
    };
    match item.get("changes") {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(path, c)| {
                let body = ["content", "unified_diff", "diff"].iter().map(|k| fstr(c, k)).find(|b| !b.is_empty()).unwrap_or("");
                one(path, str_of(c, "type"), body, fstr(c, "move_path"))
            })
            .collect(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|c| {
                let kind = c.get("kind");
                let name = kind.and_then(Value::as_str).or_else(|| kind.map(|k| str_of(k, "type"))).unwrap_or("");
                one(str_of(c, "path"), name, str_of(c, "diff"), kind.map(|k| fstr(k, "move_path")).unwrap_or(""))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A patch as the calls the window draws, one a file: a file added is a
/// `Write` of its content, one updated an `Edit` with its hunks, one
/// deleted an edit that takes every line out. The first keeps the patch's
/// id and the rest add their place to it, so each can be opened by itself.
fn change_calls(id: &str, ts: &str, cwd: &str, changes: Vec<Change>) -> Vec<ToolCall> {
    let rel = |path: &str| {
        let mut m = Map::new();
        m.insert("file_path".into(), path.into());
        tool_subject("Edit", &m, cwd)
    };
    changes
        .into_iter()
        .enumerate()
        .map(|(n, ch)| {
            let mut input = Map::new();
            input.insert("file_path".into(), ch.path.clone().into());
            let mut call = ToolCall {
                ts: ts.into(),
                id: if n == 0 { id.to_string() } else { format!("{id}:{n}") },
                subject: rel(&ch.path),
                file_path: ch.path.clone(),
                tool_kind: ToolKind::Edit,
                ..Default::default()
            };
            match ch.kind.as_str() {
                "add" => {
                    call.name = "Write".into();
                    call.tool_kind = ToolKind::Write;
                    input.insert("content".into(), ch.content.clone().into());
                    call.new_string = ch.content;
                }
                "delete" => {
                    call.name = "Delete".into();
                    if !ch.content.is_empty() {
                        call.patch = vec![hunk(1, 0, ch.content.lines().map(|l| format!("-{l}")).collect())];
                    }
                    call.old_string = ch.content;
                }
                _ => {
                    call.name = "Edit".into();
                    call.patch = ch.hunks;
                    if !ch.move_path.is_empty() {
                        call.subject = format!("{} → {}", call.subject, rel(&ch.move_path));
                        input.insert("move_path".into(), ch.move_path.into());
                    }
                }
            }
            call.input = input;
            call
        })
        .collect()
}

/// The text of whatever a tool handed back: a string, or a list of content
/// blocks.
fn output_text(output: Option<&Value>) -> String {
    match output {
        Some(Value::String(o)) => o.clone(),
        Some(Value::Object(o)) => o.get("content").map(|c| content_text(Some(c)).0).unwrap_or_default(),
        other => content_text(other).0,
    }
}

/// What a code-mode script printed. Its output opens with a header of
/// Codex's ("Script completed", "Wall time 0.1 seconds", "Output:").
fn script_output(text: &str) -> String {
    let head: String = text.chars().take(200).collect();
    match (head.starts_with("Script "), text.split_once("\nOutput:\n")) {
        (true, Some((_, out))) => out.strip_prefix('\n').unwrap_or(out).to_string(),
        _ => text.to_string(),
    }
}

/// A code-mode script as a call. One that does nothing but run a single
/// command is that command, a `Bash` call like any other; anything else is
/// shown as the script it is, under Codex's name for the tool, with its
/// first line as what it is.
fn script_call(id: &str, script: &str, ts: &str, cwd: &str) -> ToolCall {
    let mut input = Map::new();
    let command = RE_SCRIPT_CMD.captures(script).filter(|_| script.matches("tools.").count() == 1).and_then(|c| serde_json::from_str::<String>(&c[1]).ok()).filter(|c| !c.trim().is_empty());
    let (name, subject) = match command {
        Some(command) => {
            input.insert("command".into(), command.into());
            ("Bash", tool_subject("Bash", &input, cwd))
        }
        None => {
            input.insert("command".into(), script.into());
            ("exec", one_line(script.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("//")).unwrap_or(""), 110))
        }
    };
    ToolCall { ts: ts.into(), id: id.into(), name: name.into(), tool_kind: ToolKind::Bash, subject, input, ..Default::default() }
}

/// A shell tool's result where it is JSON around the output
/// (`{"output": …, "metadata": {"exit_code": …}}`, and what a script
/// prints of `exec_command`'s return): the output and the exit code.
fn unwrap_output(text: &str) -> (String, i64) {
    if text.trim_start().starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            if let Some(out) = v.get("output").and_then(Value::as_str) {
                let code = v.get("metadata").and_then(|m| m.get("exit_code")).or(v.get("exit_code")).and_then(Value::as_i64).unwrap_or(0);
                return (out.to_string(), code);
            }
        }
    }
    (text.to_string(), 0)
}

/// `request_user_input` as an `AskUserQuestion` call, its questions in the
/// shape Claude Code gives them. Each keeps Codex's `id`, which the answer
/// names it by.
fn ask_call(id: &str, args: &Map<String, Value>, ts: &str, cwd: &str) -> ToolCall {
    let questions: Vec<Value> = args
        .get("questions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .map(|q| {
            let options: Vec<Value> = arr_of(q, "options").iter().map(|o| serde_json::json!({"label": str_of(o, "label"), "description": str_of(o, "description")})).collect();
            serde_json::json!({"question": str_of(q, "question"), "header": str_of(q, "header"), "id": str_of(q, "id"), "options": options, "multiSelect": fld(q, "multi_select").and_then(Value::as_bool).unwrap_or(false)})
        })
        .collect();
    let mut input = Map::new();
    input.insert("questions".into(), questions.into());
    ToolCall { ts: ts.into(), id: id.into(), name: "AskUserQuestion".into(), tool_kind: ToolKind::Ask, subject: tool_subject("AskUserQuestion", &input, cwd), input, ..Default::default() }
}

/// What the person answered, question by question:
/// `{"answers": {<id>: {"answers": [..]}}}`.
fn ask_answers(input: &Map<String, Value>, output: &str) -> Vec<(String, String)> {
    let Ok(v) = serde_json::from_str::<Value>(output) else { return Vec::new() };
    let Some(answers) = v.get("answers").and_then(Value::as_object) else { return Vec::new() };
    let said = |a: &Value| match a.get("answers").unwrap_or(a) {
        Value::String(t) => t.trim().to_string(),
        Value::Array(items) => items.iter().filter_map(Value::as_str).map(str::trim).collect::<Vec<_>>().join(", "),
        _ => String::new(),
    };
    let questions = input.get("questions").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    questions.iter().filter_map(|q| answers.get(str_of(q, "id")).map(|a| (str_of(q, "question").to_string(), said(a)))).filter(|(_, a)| !a.is_empty()).collect()
}

/// A plan the agent proposed, as the `ExitPlanMode` call the window draws
/// a plan from. Pending is "waiting for your go-ahead", which is where a
/// plan of Codex's stands until the next prompt.
fn plan_call(id: &str, plan: &str, ts: &str) -> ToolCall {
    let mut input = Map::new();
    input.insert("plan".into(), plan.into());
    ToolCall { ts: ts.into(), id: id.into(), name: "ExitPlanMode".into(), tool_kind: ToolKind::Plan, input, ..Default::default() }
}

fn is_plan(item: &Item) -> bool {
    matches!(item, Item::Tool(c) if c.name == "ExitPlanMode")
}

/// A reply with a `<proposed_plan>` block in it: the plan, and the reply
/// without it.
fn split_plan(text: &str) -> (Option<String>, String) {
    let Some((before, rest)) = text.split_once("<proposed_plan>") else { return (None, text.to_string()) };
    let Some((plan, after)) = rest.split_once("</proposed_plan>") else { return (None, text.to_string()) };
    (Some(plan.trim().to_string()), format!("{}\n\n{}", before.trim(), after.trim()).trim().to_string())
}

/// The tool items that are neither a command nor a patch. None of these
/// was in a rollout this was written against: the fields are the app
/// server protocol's.
fn other_call(kind: &str, item: &Value, ts: &str, cwd: &str) -> Option<ToolCall> {
    let object = |v: Option<&Value>| match v {
        Some(Value::Object(o)) => o.clone(),
        Some(Value::String(s)) => parse_arguments(Some(&Value::String(s.clone()))),
        _ => Map::new(),
    };
    let mut call = ToolCall { ts: ts.into(), id: str_of(item, "id").into(), status: item_status(fstr(item, "status")), duration_ms: item_duration(item), ..Default::default() };
    match kind {
        "mcptoolcall" => {
            call.name = format!("mcp__{}__{}", str_of(item, "server"), str_of(item, "tool"));
            call.tool_kind = ToolKind::Mcp;
            call.input = object(fld(item, "arguments"));
            call.subject = call.name.clone();
            call.result_text = match fld(item, "error") {
                Some(e) => {
                    call.status = CallStatus::Error;
                    str_of(e, "message").to_string()
                }
                None => fld(item, "result").map(|r| content_text(r.get("content")).0).unwrap_or_default(),
            };
        }
        "dynamictoolcall" => {
            call.name = str_of(item, "tool").into();
            call.tool_kind = tool_kind(&call.name);
            call.input = object(fld(item, "arguments"));
            call.subject = tool_subject(&call.name, &call.input, cwd);
            call.result_text = content_text(fld(item, "content_items")).0;
            if fld(item, "success").and_then(Value::as_bool) == Some(false) {
                call.status = CallStatus::Error;
            }
        }
        "collabagenttoolcall" => {
            call.name = str_of(item, "tool").into();
            call.tool_kind = ToolKind::Task;
            let prompt = fstr(item, "prompt");
            call.input.insert("description".into(), crate::options::humanize(&call.name).into());
            if !prompt.is_empty() {
                call.input.insert("prompt".into(), prompt.into());
            }
            call.subject = one_line(if prompt.is_empty() { &call.name } else { prompt }, 110);
        }
        "websearch" => {
            let action = fld(item, "action");
            let query = [fstr(item, "query"), action.map(|a| fstr(a, "query")).unwrap_or(""), action.map(|a| fstr(a, "url")).unwrap_or("")].into_iter().find(|q| !q.is_empty()).unwrap_or("");
            call.name = "WebSearch".into();
            call.tool_kind = ToolKind::Web;
            call.input.insert("query".into(), query.into());
            call.subject = one_line(query, 110);
        }
        "imageview" => {
            call.name = "Read".into();
            call.tool_kind = ToolKind::Read;
            call.input.insert("file_path".into(), str_of(item, "path").trim_start_matches("file://").into());
            call.subject = tool_subject("Read", &call.input, cwd);
        }
        "imagegeneration" => {
            call.name = "image_generation".into();
            call.input.insert("prompt".into(), fstr(item, "revised_prompt").into());
            call.subject = one_line(fstr(item, "revised_prompt"), 110);
            call.result_text = fstr(item, "saved_path").into();
        }
        _ => return None,
    }
    Some(call)
}

// ---------------------------------------------------------------- settings

/// The mode a row says the session is in, as one key: `plan` in plan
/// mode, else the permission profile by its id without the colon
/// (`read-only`, `workspace`, `danger-full-access`), else the same three
/// from the sandbox policy an older Codex names.
fn mode_key(p: &Value) -> String {
    if p.get("collaboration_mode").map(|c| str_of(c, "mode")) == Some("plan") {
        return "plan".into();
    }
    let profile = p.get("active_permission_profile").map(|a| str_of(a, "id")).unwrap_or("").trim_start_matches(':');
    if !profile.is_empty() {
        return profile.into();
    }
    let sandbox = p.get("sandbox_policy");
    match sandbox.and_then(Value::as_str).or_else(|| sandbox.map(|s| str_of(s, "type"))).unwrap_or("") {
        "read-only" => "read-only".into(),
        "workspace-write" => "workspace".into(),
        "danger-full-access" => "danger-full-access".into(),
        _ => String::new(),
    }
}

/// An error as Codex records it: often the API's JSON, of which the
/// message is the part to read.
fn error_text(message: &str) -> String {
    let inner = serde_json::from_str::<Value>(message).ok().and_then(|v| v.get("error").map(|e| str_of(e, "message").to_string()).or_else(|| Some(str_of(&v, "message").to_string()))).filter(|m| !m.is_empty());
    one_line(&inner.unwrap_or_else(|| message.to_string()), 300)
}

// ----------------------------------------------------------------- builder

/// Which kind of row opened the round being built, and whether the other
/// kind has said the same prompt since.
#[derive(Clone, Copy, PartialEq)]
enum Opened {
    Row,
    Item,
    Both,
}

struct Builder {
    s: Session,
    rounds: Vec<Round>,
    last_ts: String,
    /// The session's running token total, cached tokens taken out of the
    /// input, and what it was when the open round began.
    total: Usage,
    round_base: Usage,
    opened: Opened,
    /// What items of the open round have said (a reply, a thought), by
    /// item id, until a response row says it again and is dropped for it.
    said: Vec<(String, String)>,
    /// The model's last tool call of the open round: its id and family.
    /// Tool items that follow are what running it did.
    open_call: Option<(String, ToolKind)>,
    /// The calls that are code-mode scripts, by id.
    scripts: HashSet<String>,
    /// A model's tool call to the items that say what it ran.
    inner: HashMap<String, Vec<String>>,
}

impl Builder {
    fn open_round(&mut self, ts: &str, prompt: String, images: u32, attachments: Vec<Attachment>, opened: Opened) {
        // A plan is not answered in the rollout. The next prompt is the
        // answer: sent out of plan mode it is the go-ahead, sent in plan
        // mode the plan was talked over and not taken.
        let verdict = if self.s.mode == "plan" { CallStatus::NoResult } else { CallStatus::Ok };
        if let Some(rnd) = self.rounds.last_mut() {
            for c in rnd.items.iter_mut().filter(|i| is_plan(i)).filter_map(Item::as_tool_mut).filter(|c| c.status == CallStatus::Pending) {
                c.status = verdict;
            }
        }
        self.rounds.push(Round { index: self.rounds.len() + 1, ts: ts.into(), prompt, source: Source::User, images, attachments, ..Default::default() });
        self.opened = opened;
        self.said.clear();
        self.open_call = None;
        self.round_base = self.total.clone();
    }

    fn ensure_round(&mut self, ts: &str) -> &mut Round {
        if self.rounds.is_empty() {
            self.rounds.push(Round { index: 1, ts: ts.into(), source: Source::System, ..Default::default() });
        }
        self.rounds.last_mut().unwrap()
    }

    /// Something the agent said or did, at the round's foot. A stop stays
    /// the last thing a round says: the command a stop cut short is
    /// written after the stop.
    fn push(&mut self, item: Item) {
        let ts = item.ts().to_string();
        let rnd = self.ensure_round(&ts);
        let stopped = matches!(rnd.items.last(), Some(Item::Notice { variant: NoticeVariant::Interrupted, .. }));
        let at = rnd.items.len() - usize::from(stopped);
        rnd.items.insert(at, item);
    }

    /// A setting's change, at the foot of the round before the turn it
    /// holds for. Changed again straight after itself it is one change,
    /// the last, as `build::RoundBuilder::add_notice` has it.
    fn changed(&mut self, text: String, ts: &str, variant: NoticeVariant) {
        let Some(rnd) = self.rounds.last_mut() else { return };
        if matches!(rnd.items.last(), Some(Item::Notice { variant: v, .. }) if *v == variant) {
            if let Some(old) = rnd.items.pop() {
                rnd.superseded.push(old);
            }
        }
        rnd.items.push(Item::Notice { ts: ts.into(), text, variant });
    }

    /// What a `turn_context` or `thread_settings_applied` row says the
    /// session is set to. The first value of each is where the session
    /// began; a later one that differs is a change, said in the
    /// conversation.
    fn settings(&mut self, p: &Value, ts: &str) {
        let inner = p.get("collaboration_mode").and_then(|c| c.get("settings"));
        let first = |names: &[&str]| names.iter().map(|k| str_of(p, k)).chain(names.iter().map(|k| inner.map(|s| str_of(s, k)).unwrap_or(""))).find(|v| !v.is_empty()).unwrap_or("").to_string();
        let mode = mode_key(p);
        if !mode.is_empty() && mode != self.s.mode {
            if !self.s.mode.is_empty() {
                self.changed(crate::driver::mode_words(&mode).0, ts, NoticeVariant::Mode);
            }
            self.s.mode = mode;
        }
        let model = first(&["model"]);
        if !model.is_empty() && self.s.models.last() != Some(&model) {
            if !self.s.models.is_empty() {
                self.changed(crate::driver::model_label(&model), ts, NoticeVariant::Model);
            }
            // Each model once, the one in use last.
            self.s.models.retain(|m| *m != model);
            self.s.models.push(model);
        }
        let effort = first(&["effort", "reasoning_effort"]);
        if !effort.is_empty() && effort != self.s.effort {
            if !self.s.effort.is_empty() {
                self.changed(crate::driver::effort_words(&effort).0, ts, NoticeVariant::Effort);
            }
            self.s.effort = effort;
        }
    }

    /// Whether `text` is the open round's prompt said again: nothing has
    /// been done under it yet and the words are the same, or one holds
    /// the other (the row a model is sent can carry more than was typed).
    fn same_prompt(&self, text: &str) -> bool {
        let Some(rnd) = self.rounds.last() else { return false };
        let untouched = rnd.items.iter().all(|i| matches!(i, Item::Notice { .. }));
        let p = rnd.prompt.as_str();
        untouched && (p == text || (!p.is_empty() && !text.is_empty() && (p.contains(text) || text.contains(p))))
    }

    fn remember(&mut self, id: &str, text: &str) {
        match self.said.iter_mut().find(|(i, _)| !id.is_empty() && i == id) {
            Some(entry) => entry.1 = text.into(),
            None => self.said.push((id.into(), text.into())),
        }
    }

    /// True when an item of this round already said `text`; that item is
    /// then matched and will not match again.
    fn already_said(&mut self, text: &str) -> bool {
        match self.said.iter().position(|(_, t)| t == text) {
            Some(i) => {
                self.said.remove(i);
                true
            }
            None => false,
        }
    }

    fn plan(&mut self, id: &str, plan: &str, ts: &str) {
        let plan = plan.trim();
        if plan.is_empty() {
            return;
        }
        // Said twice: as a plan item, and inside the reply's row.
        let known = self.rounds.last().is_some_and(|r| r.items.last().is_some_and(is_plan) || r.items.iter().any(|i| matches!(i, Item::Tool(c) if c.name == "ExitPlanMode" && c.input.get("plan").and_then(Value::as_str) == Some(plan))));
        if !known {
            self.push(Item::Tool(plan_call(id, plan, ts)));
        }
    }

    /// A tool item: what a call of the model's ran. It belongs to the
    /// call before it when that is the code-mode wrapper, which can run
    /// anything, or a call of its own family.
    fn tool_item(&mut self, call: ToolCall) {
        if let Some((open, kind)) = &self.open_call {
            if self.scripts.contains(open) || *kind == call.tool_kind || (*kind == ToolKind::Edit && call.tool_kind == ToolKind::Write) {
                let inner = self.inner.entry(open.clone()).or_default();
                if !inner.contains(&call.id) {
                    inner.push(call.id.clone());
                }
            }
        }
        // A command that outlasts its script's wait is finished under a
        // later script, one that only waits on it. The first script was
        // kept as the command, having no item of its own: the item takes
        // its place there, where the command began.
        if call.name == "Bash" {
            let (scripts, inner) = (&self.scripts, &self.inner);
            let command = call.input.get("command").and_then(Value::as_str).unwrap_or("").trim().to_string();
            let waited = self.rounds.last_mut().and_then(|r| {
                r.items.iter_mut().filter_map(Item::as_tool_mut).find(|c| {
                    c.name == "Bash" && scripts.contains(&c.id) && !inner.contains_key(&c.id) && !command.is_empty() && c.input.get("command").and_then(Value::as_str).map(str::trim) == Some(command.as_str())
                })
            });
            if let Some(old) = waited {
                let ts = std::mem::take(&mut old.ts);
                let mut call = call;
                if call.duration_ms == 0 {
                    call.duration_ms = elapsed_ms(&ts, &call.ts);
                }
                *old = call;
                old.ts = ts;
                return;
            }
        }
        let rnd = self.ensure_round(&call.ts);
        if let Some(old) = rnd.items.iter_mut().filter_map(Item::as_tool_mut).find(|c| !call.id.is_empty() && c.id == call.id) {
            let ts = std::mem::take(&mut old.ts);
            *old = call;
            old.ts = ts;
            return;
        }
        self.push(Item::Tool(call));
    }

    fn failed(&mut self, message: &str, ts: &str) {
        let text = error_text(message);
        if text.is_empty() {
            return;
        }
        let rnd = self.ensure_round(ts);
        if !matches!(rnd.items.last(), Some(Item::Notice { variant: NoticeVariant::Error, text: t, .. }) if *t == text) {
            rnd.items.push(Item::Notice { ts: ts.into(), text, variant: NoticeVariant::Error });
        }
    }

    fn compacted(&mut self, ts: &str) {
        let rnd = self.ensure_round(ts);
        if !matches!(rnd.items.last(), Some(Item::Notice { variant: NoticeVariant::Compact, .. })) {
            rnd.items.push(Item::Notice { ts: ts.into(), text: "Context compacted, earlier messages summarised".into(), variant: NoticeVariant::Compact });
        }
    }

    fn row(&mut self, row: &Value) {
        let ts = str_of(row, "timestamp");
        if !ts.is_empty() {
            if self.s.started.is_empty() {
                self.s.started = ts.into();
            }
            self.last_ts = ts.into();
        }
        let Some(payload) = row.get("payload") else { return };
        match str_of(row, "type") {
            "session_meta" => {
                if self.s.id.is_empty() {
                    self.s.id = str_of(payload, "id").into();
                }
                if !str_of(payload, "cwd").is_empty() {
                    self.s.cwd = str_of(payload, "cwd").into();
                }
                if !str_of(payload, "cli_version").is_empty() {
                    self.s.version = str_of(payload, "cli_version").into();
                }
                if let Some(b) = payload.get("git").map(|g| str_of(g, "branch")) {
                    if !b.is_empty() {
                        self.s.git_branch = b.into();
                    }
                }
            }
            "turn_context" => {
                if self.s.cwd.is_empty() && !str_of(payload, "cwd").is_empty() {
                    self.s.cwd = str_of(payload, "cwd").into();
                }
                self.settings(payload, ts);
            }
            "response_item" => self.response(payload, ts),
            "event_msg" => self.event(payload, ts),
            "compacted" => self.compacted(ts),
            _ => {}
        }
    }

    fn event(&mut self, payload: &Value, ts: &str) {
        match str_of(payload, "type") {
            "task_started" => self.s.turn_open = Some(true),
            "task_complete" => {
                self.s.turn_open = Some(false);
                // A turn that failed ends like any other, with the error
                // in place of an answer.
                if let Some(message) = payload.get("error").map(|e| str_of(e, "message")).filter(|m| !m.is_empty()) {
                    self.failed(message, ts);
                }
            }
            // The stop as an event of its own, with why: "interrupted".
            "turn_aborted" => {
                self.s.turn_open = Some(false);
                if let Some(rnd) = self.rounds.last_mut() {
                    push_interrupted(&mut rnd.items, &crate::options::humanize(str_of(payload, "reason")), ts);
                }
            }
            "error" => self.failed(str_of(payload, "message"), ts),
            "thread_settings_applied" => {
                if let Some(settings) = payload.get("thread_settings") {
                    self.settings(settings, ts);
                }
            }
            "item_completed" => {
                if let Some(item) = payload.get("item") {
                    self.item(item, ts);
                }
            }
            "token_count" => {
                let Some(info) = payload.get("info").filter(|i| !i.is_null()) else { return };
                // Cumulative totals: the latest is the session's, and what
                // it grew by since a round began is that round's.
                if let Some(u) = info.get("total_token_usage") {
                    let cached = u64_of(u, "cached_input_tokens");
                    // Codex counts cached tokens inside input_tokens.
                    self.total = Usage { input_tokens: u64_of(u, "input_tokens").saturating_sub(cached), output_tokens: u64_of(u, "output_tokens"), cache_read: cached, cache_write: 0 };
                    let (total, base) = (self.total.clone(), self.round_base.clone());
                    if let Some(rnd) = self.rounds.last_mut() {
                        rnd.usage = Usage {
                            input_tokens: total.input_tokens.saturating_sub(base.input_tokens),
                            output_tokens: total.output_tokens.saturating_sub(base.output_tokens),
                            cache_read: total.cache_read.saturating_sub(base.cache_read),
                            cache_write: 0,
                        };
                    }
                }
                // The last request's input, cached part included, is what
                // the context holds.
                let context = info.get("last_token_usage").map(|u| u64_of(u, "input_tokens")).unwrap_or(0);
                if context > 0 {
                    self.s.context_tokens = context;
                }
            }
            _ => {}
        }
    }

    fn item(&mut self, item: &Value, ts: &str) {
        // "UserMessage" in a rollout, "userMessage" on the app server's wire.
        let kind = str_of(item, "type").to_lowercase();
        let id = str_of(item, "id");
        let cwd = self.s.cwd.clone();
        match kind.as_str() {
            "usermessage" => {
                let (text, images) = content_text(item.get("content"));
                let text = clean_user_text(&text);
                if text.is_empty() && images == 0 {
                    return;
                }
                // A picture given by path can be shown from its file.
                let attachments: Vec<Attachment> = arr_of(item, "content")
                    .iter()
                    .filter(|c| matches!(block_type(c), "localImage" | "local_image"))
                    .map(|c| str_of(c, "path"))
                    .filter(|p| !p.is_empty())
                    .map(|path| {
                        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        Attachment { kind: "image".into(), path: path.into(), media_type: media_type_for(&name).into(), size: fs::metadata(path).map(|m| m.len()).unwrap_or(0), name, ..Default::default() }
                    })
                    .collect();
                if self.opened == Opened::Row && self.same_prompt(&text) {
                    // The row the model was sent opened this round; the
                    // item is the prompt as it was typed.
                    let rnd = self.rounds.last_mut().unwrap();
                    rnd.prompt = text;
                    rnd.images = rnd.images.max(images);
                    if rnd.attachments.is_empty() {
                        rnd.attachments = attachments;
                    }
                    self.opened = Opened::Both;
                    return;
                }
                self.open_round(ts, text, images, attachments, Opened::Item);
            }
            // Commentary on the way and the final answer are both the
            // agent's words, shown in the order they came.
            "agentmessage" => {
                let text = match item.get("content") {
                    Some(content) => content_text(Some(content)).0,
                    None => str_of(item, "text").to_string(),
                };
                let text = text.trim();
                if text.is_empty() {
                    return;
                }
                let rnd = self.ensure_round(ts);
                if let Some(Item::Text { md, .. }) = rnd.items.iter_mut().find(|i| matches!(i, Item::Text { uuid, .. } if !id.is_empty() && uuid == id)) {
                    *md = text.into();
                } else if matches!(rnd.items.last(), Some(Item::Text { uuid, md, .. }) if uuid.is_empty() && md == text) {
                    // A row without an id said it first.
                    return;
                } else {
                    self.push(Item::Text { uuid: id.into(), ts: ts.into(), md: text.into() });
                }
                self.remember(id, text);
            }
            // Written again each time a part is added, under one id.
            "reasoning" => {
                let parts = |key: &str| fld(item, key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n")).unwrap_or_default();
                let md = [parts("summary_text"), parts("summary"), parts("raw_content"), parts("content")].into_iter().find(|m| !m.is_empty()).unwrap_or_default();
                if md.is_empty() {
                    return;
                }
                let rnd = self.ensure_round(ts);
                if let Some(Item::Thinking { md: old, .. }) = rnd.items.iter_mut().find(|i| matches!(i, Item::Thinking { uuid, .. } if !id.is_empty() && uuid == id)) {
                    *old = md.clone();
                } else {
                    let prev = rnd.items.last().map(|i| i.ts().to_string()).unwrap_or_else(|| rnd.ts.clone());
                    let seconds = elapsed_ms(&prev, ts) as f64 / 1000.0;
                    self.push(Item::Thinking { uuid: id.into(), ts: ts.into(), md: md.clone(), seconds });
                }
                self.remember(id, &md);
            }
            "plan" => self.plan(id, str_of(item, "text"), ts),
            "commandexecution" => self.tool_item(command_call(item, ts, &cwd)),
            "filechange" => {
                let status = item_status(fstr(item, "status"));
                for mut call in change_calls(id, ts, &cwd, changes_of(item)) {
                    call.status = status;
                    call.stdout = str_of(item, "stdout").into();
                    call.stderr = str_of(item, "stderr").into();
                    call.result_text = if status == CallStatus::Error && !call.stderr.trim().is_empty() { call.stderr.clone() } else { call.stdout.clone() };
                    self.tool_item(call);
                }
            }
            "contextcompaction" => self.compacted(ts),
            "enteredreviewmode" => {
                let review = one_line(str_of(item, "review"), 200);
                self.push(Item::Notice { ts: ts.into(), text: if review.is_empty() { "Review".into() } else { format!("Review: {review}") }, variant: NoticeVariant::Info });
            }
            "exitedreviewmode" => {
                let review = str_of(item, "review").trim();
                if !review.is_empty() {
                    self.push(Item::Text { uuid: id.into(), ts: ts.into(), md: review.into() });
                }
            }
            other => {
                if let Some(call) = other_call(other, item, ts, &cwd) {
                    self.tool_item(call);
                }
            }
        }
    }

    fn response(&mut self, payload: &Value, ts: &str) {
        let kind = str_of(payload, "type");
        match kind {
            "message" => {
                let (text, images) = content_text(payload.get("content"));
                match str_of(payload, "role") {
                    "user" => {
                        // A stopped turn is told to the model inside the
                        // next user row; its first sentence is what the
                        // conversation says of the stop.
                        if let (Some(said), Some(rnd)) = (aborted_said(&text), self.rounds.last_mut()) {
                            push_interrupted(&mut rnd.items, &said, ts);
                            if self.s.turn_open == Some(true) {
                                self.s.turn_open = Some(false);
                            }
                        }
                        let text = clean_user_text(&text);
                        if text.is_empty() && images == 0 {
                            return;
                        }
                        if self.opened == Opened::Item && self.same_prompt(&text) {
                            self.opened = Opened::Both;
                            return;
                        }
                        self.open_round(ts, text, images, Vec::new(), Opened::Row);
                    }
                    "assistant" => {
                        let (plan, text) = split_plan(text.trim());
                        if let Some(plan) = plan {
                            self.plan(str_of(payload, "id"), &plan, ts);
                        }
                        if text.is_empty() || self.already_said(&text) {
                            return;
                        }
                        self.push(Item::Text { uuid: str_of(payload, "id").into(), ts: ts.into(), md: text });
                    }
                    _ => {}
                }
            }
            "reasoning" => {
                let parts: Vec<&str> = arr_of(payload, "summary").iter().map(|x| str_of(x, "text")).filter(|t| !t.is_empty()).collect();
                let md = parts.join("\n\n");
                if md.is_empty() || self.already_said(&md) {
                    return;
                }
                let rnd = self.ensure_round(ts);
                let prev = rnd.items.last().map(|i| i.ts().to_string()).unwrap_or_else(|| rnd.ts.clone());
                let seconds = elapsed_ms(&prev, ts) as f64 / 1000.0;
                self.push(Item::Thinking { uuid: String::new(), ts: ts.into(), md, seconds });
            }
            "function_call" | "custom_tool_call" | "local_shell_call" => {
                let id = str_of(payload, "call_id");
                let cwd = self.s.cwd.clone();
                let (name, input) = if kind == "local_shell_call" {
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
                // A patch, as its own tool or as the argument of a shell
                // call (`["apply_patch", "*** Begin Patch…"]`).
                let patch = match name.as_str() {
                    "apply_patch" => ["input", "patch", "arguments"].iter().find_map(|k| input.get(*k).and_then(Value::as_str)),
                    _ => input.get("command").and_then(Value::as_array).filter(|c| c.first().and_then(Value::as_str) == Some("apply_patch")).and_then(|c| c.get(1)).and_then(Value::as_str),
                };
                let calls = match (name.as_str(), patch.map(parse_apply_patch).filter(|c| !c.is_empty())) {
                    (_, Some(changes)) => change_calls(id, ts, &cwd, changes),
                    // Code mode: the model writes a script and Codex runs
                    // it. The items that follow say what it ran, and this
                    // call goes once one has (`finish`). A command the
                    // sandbox refused leaves no item, so with none the
                    // script itself is the call and what it printed the
                    // output.
                    ("exec", None) if kind == "custom_tool_call" => {
                        self.scripts.insert(id.into());
                        // A script that only waits on a command begun
                        // before it is no call of its own: the command
                        // has its card.
                        let script = payload.get("input").and_then(Value::as_str).unwrap_or("");
                        if script.matches("tools.").count() == 1 && script.contains("tools.write_stdin(") && !script.contains("chars") {
                            self.open_call = Some((id.to_string(), ToolKind::Bash));
                            return;
                        }
                        vec![script_call(id, payload.get("input").and_then(Value::as_str).unwrap_or("").trim(), ts, &cwd)]
                    }
                    ("request_user_input", None) => vec![ask_call(id, &input, ts, &cwd)],
                    _ => vec![ToolCall { ts: ts.into(), id: id.into(), tool_kind: tool_kind(&name), subject: tool_subject(&name, &input, &cwd), name, input, ..Default::default() }],
                };
                self.ensure_round(ts);
                self.open_call = calls.first().map(|c| (c.id.clone(), c.tool_kind));
                for call in calls {
                    self.push(Item::Tool(call));
                }
            }
            "function_call_output" | "custom_tool_call_output" => {
                let id = str_of(payload, "call_id");
                if id.is_empty() {
                    return;
                }
                let output = strip_ansi(&output_text(payload.get("output")));
                // A patch is a call a file, all under the one id.
                let part = format!("{id}:");
                for call in self.rounds.iter_mut().rev().flat_map(|r| r.items.iter_mut()).filter_map(Item::as_tool_mut).filter(|c| c.id == id || c.id.starts_with(&part)) {
                    call.duration_ms = elapsed_ms(&call.ts, ts);
                    call.status = CallStatus::Ok;
                    if self.scripts.contains(id) {
                        if output.starts_with("aborted by user") {
                            call.status = CallStatus::Interrupted;
                        } else if output.starts_with("Script ") && !output.starts_with("Script completed") {
                            call.status = CallStatus::Error;
                        }
                        // A script may print the tool's whole return.
                        call.result_text = unwrap_output(&script_output(&output)).0;
                    } else {
                        let (text, code) = unwrap_output(&output);
                        call.result_text = text;
                        if code != 0 {
                            call.status = CallStatus::Error;
                        }
                    }
                    if call.tool_kind == ToolKind::Bash {
                        call.stdout = call.result_text.clone();
                    }
                    if call.tool_kind == ToolKind::Ask {
                        call.answers = ask_answers(&call.input, &output);
                    }
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Session {
        // A call of the model's that has items is said by its items. The
        // one thing only the call has is what a script printed: where it
        // ran a single command and that command's own output is empty,
        // the print is the command's output.
        let mut gone: HashSet<String> = HashSet::new();
        let mut lent: HashMap<String, (String, bool)> = HashMap::new();
        for (outer, inner) in self.inner.iter().filter(|(_, inner)| !inner.is_empty()) {
            let Some(call) = self.rounds.iter().flat_map(|r| r.tool_calls()).find(|c| c.id == *outer) else { continue };
            let stopped = call.status == CallStatus::Interrupted;
            let printed = if inner.len() == 1 && call.status == CallStatus::Ok { unwrap_output(&call.result_text).0 } else { String::new() };
            for id in inner {
                lent.insert(id.clone(), (printed.clone(), stopped));
            }
            gone.insert(outer.clone());
        }
        let turn_over = self.s.turn_open == Some(false);
        let n = self.rounds.len();
        for (i, rnd) in self.rounds.iter_mut().enumerate() {
            rnd.items.retain(|item| !matches!(item, Item::Tool(c) if gone.contains(&c.id) || c.id.split_once(':').is_some_and(|(patch, _)| gone.contains(patch))));
            let stopped = rnd.items.iter().any(|i| matches!(i, Item::Notice { variant: NoticeVariant::Interrupted, .. }));
            let mut last = rnd.ts.clone();
            for item in &mut rnd.items {
                // A setting changed after the round was over is said at
                // its foot, and is not when the round ended.
                let setting = matches!(item, Item::Notice { variant, .. } if variant.setting().is_some());
                if !setting && !item.ts().is_empty() && item.ts() > last.as_str() {
                    last = item.ts().into();
                }
                let plan = is_plan(item);
                let Some(c) = item.as_tool_mut() else { continue };
                if let Some((printed, cut)) = lent.get(&c.id) {
                    if c.tool_kind == ToolKind::Bash && c.stdout.trim().is_empty() && !printed.trim().is_empty() {
                        c.stdout = printed.clone();
                        if c.result_text.trim().is_empty() {
                            c.result_text = printed.clone();
                        }
                    }
                    if *cut && c.status != CallStatus::Ok {
                        c.status = CallStatus::Interrupted;
                    }
                }
                // A call with no result in a turn that is over never had
                // one. The last plan is still waiting on the person.
                if c.status == CallStatus::Pending {
                    if i + 1 != n {
                        c.status = CallStatus::NoResult;
                    } else if turn_over && !plan {
                        c.status = if stopped { CallStatus::Interrupted } else { CallStatus::NoResult };
                    }
                }
            }
            rnd.end_ts = last.clone();
            rnd.duration_ms = elapsed_ms(&rnd.ts, &last);
        }
        let mut s = self.s;
        s.usage = self.total;
        if let Some(m) = s.models.last().cloned() {
            let t = s.usage.total();
            if t > 0 {
                s.add_model_usage(&m, t);
            }
        }
        s.updated = self.rounds.last().map(|r| if r.end_ts.is_empty() { r.ts.clone() } else { r.end_ts.clone() }).unwrap_or(self.last_ts);
        s.title = self.rounds.iter().find(|r| !r.prompt.is_empty()).map(|r| one_line(&r.prompt, 72)).unwrap_or_default();
        s.rounds = self.rounds;
        s
    }
}

pub fn build_codex(rows: &[Value], cwd_hint: &str) -> Session {
    let mut b = Builder {
        s: Session { agent: AgentId::Codex, cwd: cwd_hint.into(), ..Default::default() },
        rounds: Vec::new(),
        last_ts: String::new(),
        total: Usage::default(),
        round_base: Usage::default(),
        opened: Opened::Both,
        said: Vec::new(),
        open_call: None,
        scripts: HashSet::new(),
        inner: HashMap::new(),
    };
    for row in rows {
        b.row(row);
    }
    b.finish()
}
