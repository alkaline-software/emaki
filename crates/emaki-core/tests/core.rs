//! Behavioural tests over the core, on temp directories.

use std::fs;
use std::path::PathBuf;

use emaki_core::adapters::codex::build_codex;
use emaki_core::archive;
use emaki_core::build::{build, strip_wrappers, tool_subject, turn_state, BuildInput, Phase};
use emaki_core::model::{AgentId, CallStatus, Item};
use emaki_core::redact::Redactor;
use emaki_core::search::{fts_query, SearchIndex};
use emaki_core::transcript::{peek, read_all, SessionRef, TranscriptTail};
use serde_json::{json, Value};

struct Home {
    _dir: tempfile::TempDir,
}

/// Point EMAKI_HOME and CLAUDE_CONFIG_DIR at a temp tree. Tests that touch
/// disk run one at a time (`--test-threads=1` is set in CI; here we serialise
/// with a lock).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn isolated() -> (Home, std::sync::MutexGuard<'static, ()>) {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("EMAKI_HOME", dir.path().join("emaki"));
    std::env::set_var("CLAUDE_CONFIG_DIR", dir.path().join("claude"));
    std::env::set_var("CODEX_HOME", dir.path().join("codex"));
    (Home { _dir: dir }, guard)
}

fn row(v: Value) -> Value {
    v
}

fn user(text: &str, ts: &str) -> Value {
    row(json!({"type": "user", "uuid": format!("u-{ts}"), "timestamp": ts, "sessionId": "s1", "cwd": "/tmp/proj",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]}}))
}

fn assistant(blocks: Vec<Value>, stop: &str, ts: &str) -> Value {
    row(json!({"type": "assistant", "uuid": format!("a-{ts}"), "timestamp": ts, "sessionId": "s1",
        "message": {"role": "assistant", "model": "claude-x", "stop_reason": stop, "content": blocks,
                    "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 1000}}}))
}

fn tool_result(id: &str, text: &str, ts: &str) -> Value {
    row(json!({"type": "user", "timestamp": ts, "sessionId": "s1",
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": text}]},
        "toolUseResult": {"stdout": text, "stderr": ""}}))
}

#[test]
fn strip_wrappers_splits_commands_and_output() {
    let raw = "<command-name>/effort</command-name><command-args>high</command-args><local-command-stdout>ok</local-command-stdout>\nreal prompt";
    let (prompt, commands, outputs) = strip_wrappers(raw);
    assert_eq!(prompt, "real prompt");
    assert_eq!(commands, vec!["/effort high"]);
    assert_eq!(outputs, vec!["ok"]);
    let (p, _, _) = strip_wrappers("<system-reminder>ignore</system-reminder>hello");
    assert_eq!(p, "hello");
}

#[test]
fn builder_makes_rounds_and_closes_tool_calls() {
    let rows = vec![
        user("fix the bug", "2026-01-01T10:00:00Z"),
        assistant(vec![json!({"type": "text", "text": "Looking."}), json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls -la"}})], "tool_use", "2026-01-01T10:00:02Z"),
        tool_result("t1", "a\nb\n", "2026-01-01T10:00:04Z"),
        assistant(vec![json!({"type": "text", "text": "Done."})], "end_turn", "2026-01-01T10:00:06Z"),
        user("thanks", "2026-01-01T10:01:00Z"),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.id, "s1");
    assert_eq!(s.cwd, "/tmp/proj");
    assert_eq!(s.rounds.len(), 2);
    let r = &s.rounds[0];
    assert_eq!(r.prompt, "fix the bug");
    assert_eq!(r.tool_count(), 1);
    let call = r.tool_calls().next().unwrap();
    assert_eq!(call.status, CallStatus::Ok);
    assert_eq!(call.stdout, "a\nb\n");
    assert_eq!(call.subject, "ls -la");
    assert_eq!(call.duration_ms, 2000);
    assert_eq!(s.usage.total(), 30);
    assert_eq!(s.usage.cache_read, 2000);
    assert_eq!(s.models, vec!["claude-x"]);
    assert_eq!(s.title, "fix the bug");
}

#[test]
fn turn_state_reads_the_tail() {
    let mut rows = vec![user("go", "2026-01-01T10:00:00Z")];
    assert_eq!(turn_state(&rows, "").phase, Phase::Working);
    rows.push(assistant(vec![json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make"}})], "tool_use", "2026-01-01T10:00:01Z"));
    let st = turn_state(&rows, "");
    assert_eq!(st.phase, Phase::Working);
    assert_eq!(st.tool, "Bash");
    assert_eq!(st.activity, "make");
    rows.push(tool_result("t1", "ok", "2026-01-01T10:00:02Z"));
    assert_eq!(turn_state(&rows, "").phase, Phase::Working);
    rows.push(assistant(vec![json!({"type": "text", "text": "**All built.** Next?"})], "end_turn", "2026-01-01T10:00:03Z"));
    let st = turn_state(&rows, "");
    assert_eq!(st.phase, Phase::YourTurn);
    assert_eq!(st.reply, "All built. Next?");
    rows.push(assistant(vec![json!({"type": "tool_use", "id": "t2", "name": "AskUserQuestion", "input": {"questions": [{"question": "Which one?"}]}})], "tool_use", "2026-01-01T10:00:04Z"));
    let st = turn_state(&rows, "");
    assert_eq!(st.phase, Phase::NeedsYou);
    assert_eq!(st.activity, "Which one?");
    rows.push(row(json!({"type": "permission-mode", "permissionMode": "plan", "timestamp": "2026-01-01T10:00:05Z"})));
    assert_eq!(turn_state(&rows, "").mode, "plan");
    rows.push(user("[Request interrupted by user]", "2026-01-01T10:00:06Z"));
    let st = turn_state(&rows, "");
    assert_eq!(st.phase, Phase::YourTurn);
    assert_eq!(st.activity, "interrupted");
}

#[test]
fn tool_subjects_are_the_identifying_fragment() {
    let m = |v: Value| v.as_object().cloned().unwrap();
    assert_eq!(tool_subject("Read", &m(json!({"file_path": "/tmp/proj/src/a.rs"})), "/tmp/proj"), "src/a.rs");
    // Either separator style, on any OS: a transcript is read where it was
    // not written, and Windows does not count a POSIX path as absolute.
    assert_eq!(tool_subject("Read", &m(json!({"file_path": "C:\\Users\\jp\\proj\\src\\a.rs"})), "C:\\Users\\jp\\proj"), "src\\a.rs");
    assert_eq!(tool_subject("Read", &m(json!({"file_path": "/tmp/proj"})), "/tmp/proj/"), ".");
    assert_eq!(tool_subject("Grep", &m(json!({"pattern": "fn main", "path": "/tmp/proj"})), "/tmp/proj"), "fn main in .");
    assert_eq!(tool_subject("shell", &m(json!({"command": ["ls", "-la"]})), ""), "ls -la");
    assert_eq!(tool_subject("TodoWrite", &m(json!({"todos": [1, 2, 3]})), ""), "3 items");
}

#[test]
fn transcript_tail_restarts_on_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("s.jsonl");
    fs::write(&p, "{\"type\":\"user\"}\n{\"type\":\"assis").unwrap();
    let mut tail = TranscriptTail::new(p.clone());
    assert_eq!(tail.read_new().len(), 1);
    fs::write(&p, "{\"type\":\"user\"}\n{\"type\":\"assistant\"}\n").unwrap();
    let rows = tail.read_new();
    assert!(!tail.restarted);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["type"], "assistant");
    fs::write(&p, "{\"type\":\"user\"}\n").unwrap();
    let rows = tail.read_new();
    assert!(tail.restarted);
    assert_eq!(rows.len(), 1);
}

#[test]
fn peek_finds_title_cwd_and_state() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("abc.jsonl");
    let mut text = String::new();
    for r in [user("first prompt here", "2026-01-01T10:00:00Z"), row(json!({"type": "ai-title", "aiTitle": "Fixing the bug"})), assistant(vec![json!({"type": "text", "text": "ok"})], "end_turn", "2026-01-01T10:00:03Z")] {
        text.push_str(&serde_json::to_string(&r).unwrap());
        text.push('\n');
    }
    fs::write(&p, text).unwrap();
    let r = peek(&p);
    assert_eq!(r.session_id, "s1");
    assert_eq!(r.cwd, "/tmp/proj");
    assert_eq!(r.title, "Fixing the bug");
    assert_eq!(r.state.phase, Phase::YourTurn);
    assert_eq!(r.started, "2026-01-01T10:00:00Z");
}

#[test]
fn archive_appends_and_rotates() {
    let (_home, _g) = isolated();
    let src_dir = emaki_core::paths::projects_dir().join("-tmp-proj");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join("sess.jsonl");
    fs::write(&src, "line1\n").unwrap();
    let r = SessionRef { agent: AgentId::ClaudeCode, session_id: "sess".into(), path: src.clone(), cwd: "/tmp/proj".into(), ..Default::default() };
    let stats = archive::sweep(std::slice::from_ref(&r));
    assert_eq!(stats.files, 1);
    let dst = archive::archive_dir().join("proj").join("sess.jsonl");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "line1\n");

    // Append: only the tail is copied.
    fs::write(&src, "line1\nline2\n").unwrap();
    let stats = archive::sweep(std::slice::from_ref(&r));
    assert_eq!(stats.files, 1);
    assert_eq!(stats.bytes, 6);
    assert_eq!(fs::read_to_string(&dst).unwrap(), "line1\nline2\n");

    // Unchanged: nothing copied.
    let stats = archive::sweep(std::slice::from_ref(&r));
    assert_eq!(stats.files, 0);

    // Rewritten shorter: the old copy is rotated, never overwritten.
    fs::write(&src, "new\n").unwrap();
    let stats = archive::sweep(std::slice::from_ref(&r));
    assert_eq!(stats.rotated, 1);
    assert_eq!(fs::read_to_string(&dst).unwrap(), "new\n");
    assert_eq!(fs::read_to_string(archive::archive_dir().join("proj").join("sess.gen1.jsonl")).unwrap(), "line1\nline2\n");

    // The archived copy is listed as a peer source once the original is gone.
    fs::remove_file(&src).unwrap();
    let refs = emaki_core::adapters::index_all(0, &[]);
    let kept: Vec<&SessionRef> = refs.iter().filter(|x| x.archived).collect();
    assert_eq!(kept.len(), 2, "session and its generation file");
    // Feeding the archive its own file is a no-op.
    let again = archive::sweep(&refs);
    assert_eq!(again.files, 0);
    assert_eq!(again.rotated, 0);
}

#[test]
fn search_indexes_and_groups_hits() {
    let (_home, _g) = isolated();
    let src_dir = emaki_core::paths::projects_dir().join("-tmp-proj");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join("s1.jsonl");
    let rows = vec![
        user("please fix the reveal.js fragment bug", "2026-01-01T10:00:00Z"),
        assistant(vec![json!({"type": "text", "text": "The fragment bug is in slides.js"})], "end_turn", "2026-01-01T10:00:03Z"),
    ];
    let text: String = rows.iter().map(|r| serde_json::to_string(r).unwrap() + "\n").collect();
    fs::write(&src, text).unwrap();
    let refs = emaki_core::adapters::index_all(0, &[]);
    assert_eq!(refs.len(), 1);
    let mut idx = SearchIndex::open().unwrap();
    let report = idx.sync(&refs, false);
    assert_eq!(report.indexed, 1);
    assert_eq!(report.documents, 2);
    let res = idx.search("fragment bug", 100, 4);
    assert_eq!(res.sessions.len(), 1);
    assert_eq!(res.sessions[0].hits, 2);
    assert!(res.sessions[0].matches[0].snippet.contains('\x02'));
    let again = idx.sync(&refs, false);
    assert_eq!(again.skipped, 1);
    assert_eq!(fts_query("rm -rf a:b \"exact phrase\" rev*"), "\"rm\" \"rf\" \"a b\" \"exact phrase\" \"rev\"*");
}

#[test]
fn redactor_masks_secrets_and_skips_innocuous() {
    let r = Redactor::new(true, &[]);
    assert_eq!(r.scrub("key sk-ant-abcdefghijklmnopqrstuvwxyz1234 here"), "key [redacted] here");
    assert_eq!(r.scrub("DATABASE_PASSWORD=hunter22"), "DATABASE_PASSWORD=[redacted]");
    assert_eq!(r.scrub("password: ${SECRET}"), "password: ${SECRET}");
    assert_eq!(r.scrub("Authorization: Bearer abcdefghij"), "Authorization: Bearer [redacted]");
    assert_eq!(r.scrub("nothing to see"), "nothing to see");
}

#[test]
fn codex_rollout_becomes_rounds() {
    let rows: Vec<Value> = vec![
        json!({"timestamp": "2026-03-02T02:38:00Z", "type": "session_meta", "payload": {"id": "c1", "cwd": "/tmp/proj", "cli_version": "0.34.0", "git": {"branch": "main"}}}),
        json!({"timestamp": "2026-03-02T02:38:01Z", "type": "turn_context", "payload": {"model": "gpt-5-codex"}}),
        json!({"timestamp": "2026-03-02T02:38:02Z", "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "<environment_context>\n<cwd>/tmp</cwd>\n</environment_context>\nlist files"}]}}),
        json!({"timestamp": "2026-03-02T02:38:03Z", "type": "response_item", "payload": {"type": "reasoning", "summary": [{"type": "summary_text", "text": "I should list."}]}}),
        json!({"timestamp": "2026-03-02T02:38:04Z", "type": "response_item", "payload": {"type": "function_call", "name": "shell", "call_id": "f1", "arguments": "{\"command\":[\"ls\",\"-la\"]}"}}),
        json!({"timestamp": "2026-03-02T02:38:05Z", "type": "response_item", "payload": {"type": "function_call_output", "call_id": "f1", "output": "a\nb"}}),
        json!({"timestamp": "2026-03-02T02:38:06Z", "type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Two files."}]}}),
    ];
    let s = build_codex(&rows, "");
    assert_eq!(s.agent, AgentId::Codex);
    assert_eq!(s.id, "c1");
    assert_eq!(s.cwd, "/tmp/proj");
    assert_eq!(s.git_branch, "main");
    assert_eq!(s.models, vec!["gpt-5-codex"]);
    assert_eq!(s.rounds.len(), 1);
    let r = &s.rounds[0];
    assert_eq!(r.prompt, "list files");
    assert_eq!(r.items.len(), 3);
    assert!(matches!(r.items[0], Item::Thinking { .. }));
    let call = r.tool_calls().next().unwrap();
    assert_eq!(call.subject, "ls -la");
    assert_eq!(call.stdout, "a\nb");
    assert_eq!(call.status, CallStatus::Ok);
    assert_eq!(s.title, "list files");
    let st = emaki_core::adapters::turn_state_from_session(&s);
    assert_eq!(st.phase, Phase::YourTurn);
    assert_eq!(st.reply, "Two files.");
}

#[test]
fn read_all_skips_bad_lines() {
    let dir = tempfile::tempdir().unwrap();
    let p: PathBuf = dir.path().join("x.jsonl");
    fs::write(&p, "{\"a\":1}\nnot json\n\n[1,2]\n{\"b\":2}\n").unwrap();
    assert_eq!(read_all(&p).len(), 2);
}

#[test]
fn model_label_reads_ids_and_aliases() {
    use emaki_core::driver::{mode_label, model_label};
    assert_eq!(model_label(""), "Default model");
    assert_eq!(model_label("default"), "Default model");
    assert_eq!(model_label("opus"), "Opus");
    assert_eq!(model_label("opus[1m]"), "Opus 1M");
    assert_eq!(model_label("claude-opus-5-5"), "Opus 5.5");
    assert_eq!(model_label("claude-fable-5-1"), "Fable 5.1");
    assert_eq!(model_label("claude-haiku-4-5-20251001"), "Haiku 4.5");
    assert_eq!(model_label("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
    assert_eq!(model_label("claude-sonnet-4-20250514"), "Sonnet 4");
    assert_eq!(mode_label("auto"), "Auto mode");
    assert_eq!(mode_label("default"), "Default permissions");
}

#[test]
fn find_index_locates_prompts_items_and_subagents() {
    use emaki_core::find::{FindIndex, Hit};
    let rows = vec![
        user("Please refactor the Widget", "2026-01-01T00:00:00Z"),
        assistant(vec![json!({"type": "thinking", "thinking": "the widget needs a Gadget"})], "tool_use", "2026-01-01T00:00:01Z"),
        assistant(vec![json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "grep -r Sprocket src"}})], "tool_use", "2026-01-01T00:00:02Z"),
        tool_result("t1", "src/a.rs: Sprocket::new()", "2026-01-01T00:00:03Z"),
        assistant(vec![json!({"type": "text", "text": "Done with the widget."})], "end_turn", "2026-01-01T00:00:04Z"),
        user("thanks", "2026-01-01T00:00:05Z"),
        assistant(vec![json!({"type": "text", "text": "Any time."})], "end_turn", "2026-01-01T00:00:06Z"),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let idx = FindIndex::build(&s);
    // Case-insensitive, prompt and items alike, in reading order.
    let hits = idx.find("WIDGET");
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0], Hit { round: 0, item: None });
    assert!(hits[1..].iter().all(|h| h.round == 0 && h.item.is_some()));
    // A tool call is found by its command and by what came back.
    assert_eq!(idx.find("sprocket").len(), 1);
    assert_eq!(idx.find("src/a.rs").len(), 1);
    // The second round.
    assert_eq!(idx.find("any time"), vec![Hit { round: 1, item: Some(0) }]);
    assert!(idx.find("   ").is_empty());
    assert!(idx.find("nothing here").is_empty());
}

#[test]
fn builder_records_context_and_effort() {
    let mut rows = vec![
        user("go", "2026-01-01T10:00:00Z"),
        assistant(vec![json!({"type": "text", "text": "Done."})], "end_turn", "2026-01-01T10:00:02Z"),
    ];
    rows.push(json!({"type": "user", "uuid": "u-cmd", "timestamp": "2026-01-01T10:01:00Z", "sessionId": "s1",
        "message": {"role": "user", "content": "<command-name>/effort</command-name><command-args>max</command-args>"}}));
    rows.push(json!({"type": "system", "subtype": "local_command", "uuid": "s-cmd", "timestamp": "2026-01-01T10:01:00Z", "sessionId": "s1",
        "content": "<local-command-stdout>Set effort level to max</local-command-stdout>", "commandRun": {"command": "effort", "args": "max"}}));
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    // input 10 + cache read 1000 (the test assistant rows carry no cache creation).
    assert_eq!(s.context_tokens, 1010);
    assert_eq!(s.effort, "max");
}

#[test]
fn limits_absorb_the_wire_and_size_a_context() {
    use emaki_core::limits::{until, Limits};
    let mut l = Limits::default();
    let info = json!({"unifiedWindows": {"five_hour": {"utilization": 0.04, "resetsAt": 1790676000}, "seven_day": {"utilization": 0.07, "resetsAt": 1791205200}}});
    assert!(l.absorb_rate_limit(&info, 1790660000.0));
    assert_eq!(l.five_hour.unwrap().utilization, 0.04);
    assert_eq!(l.seven_day.unwrap().resets_at, 1791205200.0);
    assert_eq!(l.seen_at, 1790660000.0);
    assert!(!l.absorb_rate_limit(&json!({}), 1.0));
    assert!(l.absorb_model_usage(&json!({"claude-fable-5-1": {"contextWindow": 1000000}})));
    assert!(!l.absorb_model_usage(&json!({"claude-fable-5-1": {"contextWindow": 1000000}})));
    assert_eq!(l.context_window("claude-fable-5-1"), 1_000_000);
    assert_eq!(l.context_window("claude-opus-5-5"), 200_000);
    assert_eq!(l.context_window("opus[1m]"), 1_000_000);
    assert_eq!(until(1790676000.0, 1790660000.0), "4h26m");
    assert_eq!(until(1791205200.0, 1790660000.0), "6d7h");
    assert_eq!(until(1790660100.0, 1790660000.0), "1m");
    assert_eq!(until(1.0, 2.0), "");
    assert_eq!(emaki_core::driver::effort_label("xhigh"), "Extra high effort");
    assert_eq!(emaki_core::driver::effort_label(""), "Default effort");
}
