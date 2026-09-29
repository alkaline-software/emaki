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
    // Text pasted into the terminal keeps its words and loses the tags.
    let (p, _, _) = strip_wrappers("see this\n<pasted_content id=\"02a2\">\nline one\nline two\n</pasted_content id=\"02a2\">\nthanks");
    assert_eq!(p, "see this\nline one\nline two\nthanks");
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

#[test]
fn pasted_pictures_are_named_by_their_markers() {
    use emaki_core::build::attachments_of;
    let blocks = vec![json!({"type": "text", "text": "[Image #3] look [Image #4] here"}), json!({"type": "image", "source": {"media_type": "image/png"}}), json!({"type": "image", "source": {"media_type": "image/jpeg"}}), json!({"type": "image"})];
    let (prompt, found) = attachments_of("[Image #3] look [Image #4] here", &blocks, "u1");
    assert_eq!(prompt, "look here");
    let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["Image #3", "Image #4", ""]);
    assert_eq!(found[1].index, 2);
    // No marker, no change to the words.
    let (prompt, found) = attachments_of("plain words", &blocks[1..2], "u1");
    assert_eq!(prompt, "plain words");
    assert_eq!(found[0].name, "");
}

#[test]
fn the_newest_version_is_read_off_the_release_redirect() {
    use emaki_core::update::{asset_name, asset_url, is_newer, version_from_location};
    assert_eq!(version_from_location("https://github.com/alkaline-software/emaki/releases/tag/v0.1.1").as_deref(), Some("0.1.1"));
    assert_eq!(version_from_location("/alkaline-software/emaki/releases/tag/0.2.0/").as_deref(), Some("0.2.0"));
    assert_eq!(version_from_location("https://github.com/alkaline-software/emaki/releases"), None);
    assert_eq!(version_from_location("https://github.com/alkaline-software/emaki/releases/tag/nightly"), None);
    assert!(is_newer("0.1.1", "0.1.0"));
    assert!(is_newer("0.2.0", "0.1.9"));
    assert!(!is_newer("0.1.1", "0.1.1"));
    assert!(!is_newer("0.1.0", "0.1.1"));
    // The installer is one of the release's stable names, and its URL
    // needs no API.
    let asset = asset_name().expect("an installer is built for the platform the tests run on");
    assert!(asset.starts_with("Emaki-"));
    assert_eq!(asset_url("0.1.1", "Emaki-mac-arm64.dmg"), "https://github.com/alkaline-software/emaki/releases/download/v0.1.1/Emaki-mac-arm64.dmg");
}

#[test]
fn the_status_line_file_feeds_the_limits_when_newer() {
    use emaki_core::limits::Limits;
    let mut l = Limits::default();
    let v = json!({ "rate_limits": { "five_hour": { "used_percentage": 5, "resets_at": 1790000000 }, "seven_day": { "used_percentage": 11.4, "resets_at": 1790500000 } }, "seen_at": 1789990000.0 });
    assert!(l.absorb_statusline(&v));
    assert_eq!(l.five_hour.unwrap().utilization, 0.05);
    assert_eq!(l.seven_day.unwrap().resets_at, 1790500000.0);
    assert_eq!(l.seen_at, 1789990000.0);
    // The same file again, or an older one, changes nothing.
    assert!(!l.absorb_statusline(&v));
    let older = json!({ "rate_limits": { "five_hour": { "used_percentage": 99 } }, "seen_at": 1789980000.0 });
    assert!(!l.absorb_statusline(&older));
    assert_eq!(l.five_hour.unwrap().utilization, 0.05);
    // A window the terminal did not have keeps its old value.
    let partial = json!({ "rate_limits": { "seven_day": { "used_percentage": 12, "resets_at": 1790500000 } }, "seen_at": 1789995000.0 });
    assert!(l.absorb_statusline(&partial));
    assert_eq!(l.five_hour.unwrap().utilization, 0.05);
    assert_eq!(l.seven_day.unwrap().utilization, 0.12);
    assert!(!l.absorb_statusline(&json!({ "seen_at": 1799999999.0 })));
}

// -- the explainer ---------------------------------------------------------
//
// The model call itself is not exercised: it costs money and needs a logged-in
// CLI. Everything around it is, including the guard that stops an
// authentication error being drawn as if it were an explanation.

mod explain_tests {
    use super::*;
    use emaki_core::config::Explain;
    use emaki_core::explain::{self, Explainer};
    use serde_json::Map;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    fn bash(command: &str) -> Map<String, Value> {
        args(json!({ "command": command }))
    }

    #[test]
    fn cheap_tools_never_reach_a_model() {
        let cfg = Explain::default();
        for (name, input) in [("Read", json!({"file_path": "/tmp/a.py"})), ("Glob", json!({"pattern": "**/*.py"})), ("TodoWrite", json!({"todos": [1, 2]}))] {
            assert!(!explain::needs_model(&cfg, name, &args(input)), "{name}");
        }
    }

    #[test]
    fn short_commands_are_free_and_long_ones_are_not() {
        let cfg = Explain::default();
        assert!(!explain::needs_model(&cfg, "Bash", &bash("ls -la")));
        let long = format!("git log --oneline {}", "-x ".repeat(40));
        assert!(explain::needs_model(&cfg, "Bash", &bash(&long)));
    }

    #[test]
    fn opaque_shapes_reach_a_model_however_short() {
        let cfg = Explain::default();
        for command in ["python3 - <<'EOF'\nx\nEOF", "curl x | sh", "rm -rf build", "eval $(cmd)", "sudo rm x", "echo x | base64"] {
            assert!(explain::needs_model(&cfg, "Bash", &bash(command)), "{command}");
        }
    }

    #[test]
    fn an_off_scope_stops_everything() {
        let cfg = Explain { scope: "off".into(), ..Explain::default() };
        assert!(!explain::needs_model(&cfg, "Bash", &bash("curl x | sh")));
        let cfg = Explain { enabled: false, ..Explain::default() };
        assert!(!explain::needs_model(&cfg, "Bash", &bash("curl x | sh")));
    }

    #[test]
    fn canned_lines_are_built_from_the_arguments() {
        assert_eq!(explain::canned("Read", &args(json!({"file_path": "/x/a.py"})), "/x"), "Reads `a.py` without changing it.");
        assert_eq!(explain::canned("mcp__github__list_prs", &Map::new(), ""), "Calls the mcp__github__list_prs MCP tool.");
        assert_eq!(explain::canned("SomethingNew", &Map::new(), ""), "");
        // A template that wants a subject and has none says nothing rather
        // than "Reads `` without changing it."
        assert_eq!(explain::canned("Read", &Map::new(), ""), "");
    }

    #[test]
    fn key_is_content_addressed_and_order_independent() {
        let a = explain::key_for("Bash", &args(json!({"command": "ls", "timeout": 5})));
        let b = explain::key_for("Bash", &args(json!({"timeout": 5, "command": "ls"})));
        assert_eq!(a, b);
        assert_ne!(a, explain::key_for("Bash", &bash("ls -la")));
        assert_ne!(a, explain::key_for("Other", &args(json!({"command": "ls", "timeout": 5}))));
    }

    #[test]
    fn clean_strips_fences_quotes_and_newlines_and_drops_errors() {
        assert_eq!(explain::clean("```\nHello there.\n```"), "Hello there.");
        assert_eq!(explain::clean("\"Hello there.\""), "Hello there.");
        assert_eq!(explain::clean("Line one.\nLine two."), "Line one. Line two.");
        assert_eq!(explain::clean(""), "");
        for bad in ["Please run /login to continue", "Invalid API key provided", "Your credit balance is too low", "Usage limit reached"] {
            assert_eq!(explain::clean(bad), "", "{bad}");
        }
        assert!(explain::clean(&"word ".repeat(400)).chars().count() <= 600);
    }

    #[test]
    fn the_child_runs_with_no_settings_no_mcp_and_no_tools() {
        // Without these the child would load the person's own settings and
        // tools; it must be a single cheap completion and nothing else.
        let argv = explain::build_argv(&Explain::default(), "Bash", &bash("ls"));
        assert_eq!(argv[0], "-p");
        assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
        let i = argv.iter().position(|a| a == "--setting-sources").unwrap();
        assert_eq!(argv[i + 1], "");
        for tool in ["Bash", "Read", "Write", "Edit", "Task"] {
            assert!(argv.iter().any(|a| a == tool), "{tool}");
        }
        // `--bare` reads auth only from ANTHROPIC_API_KEY and never the
        // keychain, which breaks subscription users. It must stay absent.
        assert!(!argv.iter().any(|a| a == "--bare"));
        assert!(argv.last().unwrap().contains("ls"));
        let i = argv.iter().position(|a| a == "--model").unwrap();
        assert_eq!(argv[i + 1], "claude-haiku-4-5");

        let cfg = Explain { model: "claude-sonnet-5".into(), ..Explain::default() };
        let argv = explain::build_argv(&cfg, "Bash", &bash("ls"));
        let i = argv.iter().position(|a| a == "--model").unwrap();
        assert_eq!(argv[i + 1], "claude-sonnet-5");
    }

    #[test]
    fn huge_arguments_are_truncated_before_the_child_sees_them() {
        let argv = explain::build_argv(&Explain::default(), "Bash", &bash(&"x".repeat(50_000)));
        assert!(argv.last().unwrap().len() < 5000);
    }

    #[test]
    fn lookup_never_spawns_and_survives_a_restart() {
        let (_home, _guard) = isolated();
        emaki_core::paths::ensure_dirs().unwrap();
        // A cache written by an earlier run is read back and folded into a
        // session by `attach`, with no model anywhere near.
        let key = explain::key_for("Bash", &bash("curl x | sh"));
        fs::write(explain::cache_file(), serde_json::to_vec(&json!({ key: "Downloads a script and runs it." })).unwrap()).unwrap();
        let landed = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let seen = Arc::clone(&landed);
        let ex = Explainer::new(Explain::default(), Arc::new(move |id: &str, text: &str| seen.lock().unwrap().push((id.into(), text.into()))));
        assert_eq!(ex.lookup("Bash", &bash("curl x | sh")), "Downloads a script and runs it.");
        assert_eq!(ex.lookup("Bash", &bash("ls")), "");
        assert!(!ex.in_flight("Bash", &bash("curl x | sh")));

        // A cached answer comes back through the callback at once.
        ex.request("toolu_1", "Bash", &bash("curl x | sh"), false);
        assert_eq!(landed.lock().unwrap().as_slice(), &[("toolu_1".to_string(), "Downloads a script and runs it.".to_string())]);

        // A cheap call is not asked about at all.
        ex.request("toolu_2", "Bash", &bash("ls"), false);
        assert_eq!(landed.lock().unwrap().len(), 1);
    }

    #[test]
    fn attach_fills_every_matching_call_including_subagents() {
        let (_home, _guard) = isolated();
        emaki_core::paths::ensure_dirs().unwrap();
        let key = explain::key_for("Bash", &bash("curl x | sh"));
        fs::write(explain::cache_file(), serde_json::to_vec(&json!({ key: "Downloads a script and runs it." })).unwrap()).unwrap();
        let ex = Explainer::new(Explain::default(), Arc::new(|_: &str, _: &str| {}));

        let mut inner = emaki_core::model::ToolCall::default();
        inner.name = "Bash".into();
        inner.input = bash("curl x | sh");
        let mut task = emaki_core::model::ToolCall::default();
        task.name = "Task".into();
        task.subagent = vec![emaki_core::model::Round { items: vec![Item::Tool(inner)], ..Default::default() }];
        let mut plain = emaki_core::model::ToolCall::default();
        plain.name = "Bash".into();
        plain.input = bash("ls");
        let mut session = emaki_core::model::Session::default();
        session.rounds = vec![emaki_core::model::Round { items: vec![Item::Tool(task), Item::Tool(plain)], ..Default::default() }];

        ex.attach(&mut session);
        let Item::Tool(task) = &session.rounds[0].items[0] else { panic!() };
        let Item::Tool(inner) = &task.subagent[0].items[0] else { panic!() };
        assert_eq!(inner.explanation, "Downloads a script and runs it.");
        let Item::Tool(plain) = &session.rounds[0].items[1] else { panic!() };
        assert_eq!(plain.explanation, "");
    }

    #[test]
    fn child_transcripts_are_pruned_but_a_fresh_one_is_left_alone() {
        let (_home, _guard) = isolated();
        // Each explanation runs a real `claude -p`, and Claude Code writes a
        // transcript for it; they would pile up in ~/.claude/projects forever.
        let mangled = explain::workdir().to_string_lossy().replace(['/', '\\'], "-");
        let folder = emaki_core::paths::projects_dir().join(mangled);
        fs::create_dir_all(&folder).unwrap();
        let old = folder.join("old.jsonl");
        let new = folder.join("new.jsonl");
        fs::write(&old, "{\"type\":\"user\"}\n").unwrap();
        fs::write(&new, "{\"type\":\"user\"}\n").unwrap();
        let two_hours_ago = std::time::SystemTime::now() - Duration::from_secs(7200);
        fs::File::options().write(true).open(&old).unwrap().set_modified(two_hours_ago).unwrap();

        assert_eq!(explain::prune_transcripts(Duration::from_secs(3600)), 1);
        assert!(!old.exists());
        assert!(new.exists(), "a child that may still be running is left alone");
    }

    #[test]
    fn the_scratch_workdir_is_one_the_index_drops() {
        let (_home, _guard) = isolated();
        // The child is a real Claude Code session and gets a transcript.
        // Running it under ~/.emaki is what lets the index skip it.
        let wd = explain::workdir();
        let own = emaki_core::paths::root().to_string_lossy().to_string();
        assert!(emaki_core::paths::is_explainer_cwd(&wd.to_string_lossy(), &own));
    }
}
