//! Behavioural tests over the core, on temp directories.

use std::fs;
use std::path::PathBuf;

use emaki_core::adapters::codex::build_codex;
use emaki_core::archive;
use emaki_core::build::{build, strip_wrappers, tool_subject, turn_state, BuildInput, Phase};
use emaki_core::model::{AgentId, CallStatus, Item, Source};
use emaki_core::redact::Redactor;
use emaki_core::search::{fts_query, SearchIndex};
use emaki_core::transcript::{first_prompt_title, peek, read_all, SessionRef, TranscriptTail};
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
    // The reply to copy is the text on both sides of the tool call.
    assert_eq!(r.reply_markdown(), "Looking.\n\nDone.");
    assert_eq!(s.rounds[1].reply_markdown(), "");
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

/// A question the agent asks is a tool call whose result carries the
/// answers in its sidecar; the model keeps them by question, and the
/// markdown draws the options as a checklist with the chosen one ticked.
#[test]
fn a_question_keeps_its_answers() {
    let input = json!({"questions": [{"question": "Tea or coffee?", "header": "Drink", "options": [{"label": "Tea", "description": "Hot"}, {"label": "Coffee", "description": "Hotter"}], "multiSelect": false}]});
    let qs = emaki_core::model::questions_of(input.as_object().unwrap());
    assert_eq!(qs.len(), 1);
    assert_eq!(qs[0].header, "Drink");
    assert_eq!(qs[0].options, vec![("Tea".to_string(), "Hot".to_string()), ("Coffee".to_string(), "Hotter".to_string())]);
    let mut rows = vec![user("ask me", "2026-01-01T10:00:00Z")];
    rows.push(assistant(vec![json!({"type": "tool_use", "id": "t1", "name": "AskUserQuestion", "input": input})], "tool_use", "2026-01-01T10:00:01Z"));
    let mut result = tool_result("t1", "Your questions have been answered: \"Tea or coffee?\"=\"Tea\".", "2026-01-01T10:00:02Z");
    result["toolUseResult"] = json!({"questions": input["questions"], "answers": {"Tea or coffee?": "Tea"}});
    rows.push(result);
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let call = s.rounds[0].tool_calls().next().unwrap();
    assert_eq!(call.answers, vec![("Tea or coffee?".to_string(), "Tea".to_string())]);
    let md = emaki_core::render_md::render(&s, &emaki_core::config::Config::default(), &Redactor::new(false, &[]));
    assert!(md.contains("- [x] Tea — Hot"), "{md}");
    assert!(md.contains("- [ ] Coffee — Hotter"), "{md}");

    // A label with a comma in it is matched whole, not split on the comma.
    let input = json!({"questions": [{"question": "Commit?", "header": "Commit", "options": [{"label": "Yes, commit and push", "description": ""}, {"label": "Not yet", "description": ""}], "multiSelect": false}]});
    let mut rows = vec![user("ask me", "2026-01-01T10:00:00Z")];
    rows.push(assistant(vec![json!({"type": "tool_use", "id": "t2", "name": "AskUserQuestion", "input": input})], "tool_use", "2026-01-01T10:00:01Z"));
    let mut result = tool_result("t2", "answered", "2026-01-01T10:00:02Z");
    result["toolUseResult"] = json!({"questions": input["questions"], "answers": {"Commit?": "Yes, commit and push"}});
    rows.push(result);
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let md = emaki_core::render_md::render(&s, &emaki_core::config::Config::default(), &Redactor::new(false, &[]));
    assert!(md.contains("- [x] Yes, commit and push"), "{md}");
    assert!(!md.contains("> Yes, commit"), "{md}");
}

/// A message sent while the agent is working is absorbed into the turn and
/// recorded as a `queued_command` attachment, never as a user row. It is a
/// prompt in its place: a round of its own, from the person or the peer
/// that sent it, with what the agent did next under it. It changes no
/// state: the turn it cut into is still running.
#[test]
fn a_message_sent_mid_turn_is_a_round_in_its_place() {
    let mut rows = vec![user("go", "2026-01-01T10:00:00Z")];
    rows.push(assistant(vec![json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make"}})], "tool_use", "2026-01-01T10:00:01Z"));
    rows.push(tool_result("t1", "ok", "2026-01-01T10:00:02Z"));
    rows.push(row(json!({"type": "attachment", "uuid": "q1", "timestamp": "2026-01-01T10:00:03Z", "sessionId": "s1",
        "attachment": {"type": "queued_command", "commandMode": "prompt", "humanTurn": true, "origin": {"kind": "human"},
            "prompt": [{"type": "text", "text": "[Image #1] also check the tests"}, {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}}]}})));
    rows.push(row(json!({"type": "attachment", "uuid": "q2", "timestamp": "2026-01-01T10:00:04Z", "sessionId": "s1",
        "attachment": {"type": "queued_command", "commandMode": "task-notification", "prompt": "<task-notification>done</task-notification>"}})));
    rows.push(row(json!({"type": "attachment", "uuid": "q3", "timestamp": "2026-01-01T10:00:05Z", "sessionId": "s1",
        "attachment": {"type": "queued_command", "commandMode": "prompt", "isMeta": true,
            "prompt": "<cross-session-message from-name=\"emaki\">\nand this\n</cross-session-message>",
            "origin": {"kind": "peer", "name": "emaki", "body": "and this"}}})));
    rows.push(assistant(vec![json!({"type": "text", "text": "Checked."})], "end_turn", "2026-01-01T10:00:06Z"));
    let st = turn_state(&rows[..4], "");
    assert_eq!(st.phase, Phase::Working, "the turn cut into is still running");
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let prompts: Vec<(&str, Source)> = s.rounds.iter().map(|r| (r.prompt.as_str(), r.source)).collect();
    assert_eq!(prompts, vec![("go", Source::User), ("also check the tests", Source::User), ("and this", Source::Web)], "the pasted-image label is stripped as it is on any prompt");
    assert_eq!(s.rounds[1].uuid, "q1");
    assert_eq!(s.rounds[1].images, 1);
    assert!(s.rounds[2].has_text(), "what the agent did next sits under the message that cut in");
}

/// `/compact` leaves a boundary and then the summary Claude Code hands the
/// model, as a user row flagged `isCompactSummary`. It is not a prompt:
/// nothing is waiting on a reply, and the conversation shows the boundary,
/// not a message the person never sent.
#[test]
fn compact_summary_is_not_a_prompt() {
    let mut rows = vec![user("go", "2026-01-01T10:00:00Z"), assistant(vec![json!({"type": "text", "text": "Done."})], "end_turn", "2026-01-01T10:00:01Z")];
    rows.push(user("/compact", "2026-01-01T10:00:02Z"));
    rows.push(row(json!({"type": "system", "subtype": "compact_boundary", "content": "Conversation compacted", "compactMetadata": {"preTokens": 523717, "postTokens": 11752}, "timestamp": "2026-01-01T10:00:03Z", "sessionId": "s1"})));
    let mut summary = user("This session is being continued from a previous conversation that ran out of context.", "2026-01-01T10:00:04Z");
    summary["isCompactSummary"] = json!(true);
    summary["isVisibleInTranscriptOnly"] = json!(true);
    rows.push(summary);
    rows.push(user("<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>", "2026-01-01T10:00:05Z"));
    rows.push(user("<local-command-stdout>Compacted (ctrl+o to see full summary)</local-command-stdout>", "2026-01-01T10:00:05Z"));
    let st = turn_state(&rows, "");
    assert_eq!(st.phase, Phase::YourTurn, "a local command answers its own prompt; the turn before it still stands");
    assert_eq!(st.reply, "Done.");
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.rounds.len(), 2);
    assert_eq!(s.rounds[1].prompt, "/compact");
    let notices: Vec<&str> = s.rounds[1].items.iter().filter_map(|i| if let Item::Notice { text, .. } = i { Some(text.as_str()) } else { None }).collect();
    assert_eq!(notices, vec!["Context compacted, earlier messages summarised"], "no chip repeating the prompt, no terminal instruction");
    assert!(!s.rounds.iter().any(|r| r.prompt.starts_with("This session is being continued")));
    assert_eq!(first_prompt_title(&rows[2..], 72), "/compact");
    assert_eq!(s.context_tokens, 11752, "the context in use is the summary's size until the next turn");

    // A command run later in the same round keeps its output: the model it set.
    rows.push(user("<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args></command-args>", "2026-01-01T10:01:00Z"));
    rows.push(user("<local-command-stdout>Set model to `Opus 5.5`</local-command-stdout>", "2026-01-01T10:01:00Z"));
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let notices: Vec<&str> = s.rounds[1].items.iter().filter_map(|i| if let Item::Notice { text, .. } = i { Some(text.as_str()) } else { None }).collect();
    assert_eq!(notices, vec!["Context compacted, earlier messages summarised", "Opus 5.5"]);

    let mut rows = vec![user("go", "2026-01-01T10:00:00Z"), assistant(vec![json!({"type": "text", "text": "Done."})], "end_turn", "2026-01-01T10:00:01Z")];
    rows.push(user("/effort high", "2026-01-01T10:00:02Z"));
    rows.push(row(json!({"type": "system", "subtype": "local_command", "commandRun": {"command": "effort", "args": "high"}, "timestamp": "2026-01-01T10:00:03Z", "sessionId": "s1"})));
    assert_eq!(turn_state(&rows, "").phase, Phase::YourTurn);
    rows.push(user("/review this", "2026-01-01T10:00:04Z"));
    assert_eq!(turn_state(&rows, "").phase, Phase::Working, "a skill is a prompt like any other");
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
    let dst = archive::archive_dir().join("claude").join("proj").join("sess.jsonl");
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
    assert_eq!(fs::read_to_string(archive::archive_dir().join("claude").join("proj").join("sess.gen1.jsonl")).unwrap(), "line1\nline2\n");

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
fn a_copy_set_aside_when_earlier_rows_changed() {
    let (_home, _g) = isolated();
    let src_dir = emaki_core::paths::projects_dir().join("-tmp-proj");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join("sess.jsonl");
    fs::write(&src, "line1\n").unwrap();
    let r = SessionRef { agent: AgentId::ClaudeCode, session_id: "sess".into(), path: src.clone(), cwd: "/tmp/proj".into(), ..Default::default() };
    archive::sweep(std::slice::from_ref(&r));
    // Longer, but not the same file grown: what was archived is kept.
    fs::write(&src, "LINE1\nline2\n").unwrap();
    let stats = archive::sweep(std::slice::from_ref(&r));
    assert_eq!(stats.rotated, 1);
    let dir = archive::archive_dir().join("claude").join("proj");
    assert_eq!(fs::read_to_string(dir.join("sess.jsonl")).unwrap(), "LINE1\nline2\n");
    assert_eq!(fs::read_to_string(dir.join("sess.gen1.jsonl")).unwrap(), "line1\n");
}

#[test]
fn a_copy_that_is_not_the_source_is_taken_again() {
    let (_home, _g) = isolated();
    let src_dir = emaki_core::paths::projects_dir().join("-tmp-proj");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join("sess.jsonl");
    fs::write(&src, "a\nb\nc\n").unwrap();
    let r = SessionRef { agent: AgentId::ClaudeCode, session_id: "sess".into(), path: src.clone(), cwd: "/tmp/proj".into(), ..Default::default() };
    archive::sweep(std::slice::from_ref(&r));
    let dir = archive::archive_dir().join("claude").join("proj");
    let dst = dir.join("sess.jsonl");
    // As an earlier Emaki left some: a row missing, a row twice, and its
    // record saying the copy is whole and was never looked at again.
    fs::write(&dst, "a\nc\nc\n").unwrap();
    let state = archive::archive_dir().join("state.json");
    let mut record: serde_json::Value = serde_json::from_str(&fs::read_to_string(&state).unwrap()).unwrap();
    for entry in record.as_object_mut().unwrap().values_mut() {
        entry["shared"] = false.into();
    }
    fs::write(&state, record.to_string()).unwrap();
    let stats = archive::sweep(std::slice::from_ref(&r));
    assert_eq!(fs::read_to_string(&dst).unwrap(), "a\nb\nc\n");
    assert_eq!(stats.rotated, 0, "it held nothing the source lacks");
    assert!(!dir.join("sess.gen1.jsonl").exists());
    // Looked at once: the next sweep has nothing to do.
    assert_eq!(archive::sweep(std::slice::from_ref(&r)).shared, 0);
}

#[test]
fn a_stale_copy_is_packed_and_reads_the_same() {
    let (_home, _g) = isolated();
    let dir = archive::archive_dir().join("claude").join("proj");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.jsonl");
    let text = "{\"type\":\"user\",\"message\":\"the same words over and over\"}\n".repeat(8000);
    fs::write(&path, &text).unwrap();
    let r = SessionRef { agent: AgentId::ClaudeCode, session_id: "old".into(), path: path.clone(), archived: true, mtime: 1.0, ..Default::default() };
    // One the agent still has, and one only just gone, are left alone.
    let live = SessionRef { archived: false, ..r.clone() };
    assert_eq!(archive::pack_stale(std::slice::from_ref(&live)), 0);
    let fresh = SessionRef { mtime: emaki_core::transcript::mtime_secs(&fs::metadata(&path).unwrap()), ..r.clone() };
    assert_eq!(archive::pack_stale(std::slice::from_ref(&fresh)), 0);

    let packed = archive::pack_stale(std::slice::from_ref(&r));
    assert_eq!(fs::read_to_string(&path).unwrap(), text, "the same bytes, packed or not");
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(packed, 1);
        let st = fs::metadata(&path).unwrap();
        assert!(st.blocks() * 512 < st.len() / 2, "it takes less room than it holds");
        // A second look finds nothing left to do.
        assert_eq!(archive::pack_stale(std::slice::from_ref(&r)), 0);
    }
    let _ = packed;
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
    use emaki_core::driver::model_label;
    assert_eq!(model_label(""), "Default model");
    assert_eq!(model_label("default"), "Default model");
    assert_eq!(model_label("opus"), "Opus");
    assert_eq!(model_label("opus[1m]"), "Opus 1M");
    assert_eq!(model_label("claude-opus-5-5"), "Opus 5.5");
    assert_eq!(model_label("claude-fable-5-1"), "Fable 5.1");
    assert_eq!(model_label("claude-haiku-4-5-20251001"), "Haiku 4.5");
    assert_eq!(model_label("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
    assert_eq!(model_label("claude-sonnet-4-20250514"), "Sonnet 4");
}

/// The lists are the agent's: the models and their effort levels from an
/// `initialize` reply, the modes from `--help`. Both as 2.1.289 gives them.
#[test]
fn options_are_what_the_agent_lists() {
    use emaki_core::driver::{help_values, options_from};
    let help = "  --effort <level>                      Effort level for the current session\n                                        (low, medium, high, xhigh, max)\n  --environment <environment_id>        Create a new cloud session\n  --model <model>                       Model for the current session. Provide\n                                        an alias for the latest model (e.g.\n                                        'fable', 'opus', or 'sonnet') or a\n                                        model's full name.\n  --permission-mode <mode>              Permission mode to use for the session\n                                        (choices: \"acceptEdits\", \"auto\",\n                                        \"bypassPermissions\", \"manual\",\n                                        \"dontAsk\", \"plan\", \"sprint\")\n  --plugin-dir <path>                   Load a plugin\n";
    assert_eq!(help_values(help, "--effort"), ["low", "medium", "high", "xhigh", "max"]);
    // A sentence in brackets is not a list, and an option not there has none.
    assert!(help_values(help, "--model").is_empty());
    assert!(help_values(help, "--nothing").is_empty());

    let reply = json!({"models": [
        {"value": "default", "resolvedModel": "claude-opus-5-5", "displayName": "Default (recommended)", "description": "Opus 5.5 · Best for everyday, complex tasks", "supportsEffort": true, "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"]},
        {"value": "opus", "resolvedModel": "claude-opus-5-5", "displayName": "Opus 5.5", "description": "For complex work and everyday tasks", "supportsEffort": true, "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"]},
        {"value": "haiku", "resolvedModel": "claude-haiku-4-5-20251001", "displayName": "Haiku 4.5", "description": "Fastest for quick answers"},
        {"value": "claude-opus-4-6", "resolvedModel": "claude-opus-4-6", "displayName": "Opus 4.6", "description": "Best for everyday, complex tasks", "supportedEffortLevels": ["low", "medium", "high", "max"]},
    ]});
    let o = options_from(&reply, help);
    // The modes in the agent's order, under the names the wire takes:
    // `--help` says "manual" where the wire says "default".
    let modes: Vec<&str> = o.modes.iter().map(|m| m.key.as_str()).collect();
    assert_eq!(modes, ["acceptEdits", "auto", "bypassPermissions", "default", "dontAsk", "plan", "sprint"]);
    assert_eq!(o.mode("default").unwrap().label, "Manual");
    assert_eq!(o.mode("acceptEdits").unwrap().label, "Accept edits");
    // A mode this build has never met is offered under its own name.
    assert_eq!(o.mode("sprint").unwrap().label, "Sprint");
    assert_eq!(o.mode("sprint").unwrap().detail, "");
    // Claude Code's own colour for a mode it has one for, light then dark.
    assert_eq!(o.mode("plan").unwrap().color, Some([0x006666, 0x48968C]));
    assert_eq!(o.mode("sprint").unwrap().color, None);

    assert_eq!(o.models.len(), 4);
    assert_eq!(o.default_model, "default");
    // A session reports the id; the named model answers before the default's stand-in.
    assert_eq!(o.model("claude-opus-5-5").unwrap().label, "Opus 5.5");
    assert_eq!(o.model("default").unwrap().label, "Default (recommended)");
    assert_eq!(o.model("opus").unwrap().detail, "For complex work and everyday tasks");
    assert!(o.model("gpt-9").is_none());
    // Each model has its own levels; one that takes none has none, and a
    // model off the list gets the agent's levels at large.
    let keys = |model: &str| o.efforts_for(model).iter().map(|e| e.key.clone()).collect::<Vec<_>>();
    assert_eq!(keys("claude-opus-4-6"), ["low", "medium", "high", "max"]);
    assert!(keys("haiku").is_empty());
    assert_eq!(keys("gpt-9"), ["low", "medium", "high", "xhigh", "max"]);
    assert_eq!(o.effort_label("opus", "xhigh"), "Extra high");
    // Claude Code's slider colours: one per level, and a rainbow for max.
    let level = |key: &str| o.efforts.iter().find(|e| e.key == key).unwrap();
    assert_eq!(level("low").color, Some([0x966C1E, 0xFFC107]));
    assert_eq!(level("high").color, Some([0x5769F7, 0xB1B9F9]));
    assert_eq!(level("max").color, None);
    assert_eq!(level("max").spectrum.len(), 7);
    assert_eq!(o.effort_label("opus", "turbo"), "Turbo");
    assert_eq!(emaki_core::options::humanize("readOnly-fast_mode"), "Read only fast mode");
}

#[test]
fn options_are_kept_for_the_next_launch() {
    use emaki_core::model::AgentId;
    use emaki_core::options::{Choice, Options};
    let (_home, _guard) = isolated();
    assert!(Options::cached(AgentId::ClaudeCode).is_empty());
    let o = Options { modes: vec![Choice { key: "plan".into(), label: "Plan".into(), ..Default::default() }], ..Default::default() };
    o.remember(AgentId::ClaudeCode);
    // An agent that did not answer does not wipe what is known.
    Options::default().remember(AgentId::ClaudeCode);
    assert_eq!(Options::cached(AgentId::ClaudeCode), o);
    assert!(Options::cached(AgentId::Codex).is_empty());
}

#[test]
fn a_terminal_colour_is_answered_in_both_themes() {
    use emaki_core::driver::theme_pair;
    // The thinking yellow of the dark theme, and the light theme's own.
    assert_eq!(theme_pair(0xFFC107), Some([0x966C1E, 0xFFC107]));
    assert_eq!(theme_pair(0x966C1E), Some([0x966C1E, 0xFFC107]));
    // The mark's colour is one shade in both.
    assert_eq!(theme_pair(0xD77757), Some([0xD77757, 0xD77757]));
    assert_eq!(theme_pair(0x123456), None);
}

#[test]
fn working_on_screen_is_the_line_over_the_prompt() {
    use emaki_core::driver::working_on_screen;
    let foot = "\n\n────\n❯ \n────\n  Context 16% | 5h: 8% (3h33m)\n  ⏵⏵ auto mode on (shift+tab to cycle)\n";
    // As Kaku hands it over with colours, 2.1.289.
    let line = "\x1b[38:2::215:119:87m✽\x1b[39m \x1b[38:2::223:134:102mEmbellishing…\x1b[38:2::215:119:87m \x1b[38:2::153:153:153m(26s · ↓\x1b[39m \x1b[38:2::153:153:153m1.5k tokens)\r";
    let w = working_on_screen(&format!("⏺ Reading… the file\n\n{line}\n  ⎿  Tip: Connect Claude to your IDE · /ide{foot}")).unwrap();
    assert_eq!(w.verb, "Embellishing…");
    assert_eq!(w.color, Some(0xD77757));
    assert_eq!(w.detail, vec![("(26s · ↓ 1.5k tokens)".to_string(), Some(0x999999))]);
    let pulsing = "\x1b[38:2::215:119:87m✶\x1b[39m \x1b[38:2::215:119:87mBillowing… \x1b[38:2::153:153:153m(2m 50s · \x1b[38:2::185:185:185mthinking\x1b[38:2::153:153:153m)\r";
    assert_eq!(working_on_screen(&format!("{pulsing}{foot}")).unwrap().detail, vec![("(2m 50s · thinking)".to_string(), Some(0x999999))]);
    // A turn gone quiet: the mark in bold behind a character-set
    // sequence, the colours turning warm.
    let quiet = "\x1b(B\x1b[0;1m\x1b[38:2::235:156:47m✻\x1b(B\x1b[0m \x1b[38:2::235:156:47mBillowing… \x1b[38:2::153:153:153m(3m 5s)\r";
    let w = working_on_screen(&format!("{quiet}{foot}")).unwrap();
    assert_eq!((w.verb.as_str(), w.color), ("Billowing…", Some(0xEB9C2F)));
    // Plain, as Terminal and iTerm2 give it.
    let w = working_on_screen(&format!("· Embellishing… (3s · thinking with medium effort){foot}")).unwrap();
    assert_eq!((w.verb.as_str(), w.color), ("Embellishing…", None));
    assert_eq!(w.detail, vec![("(3s · thinking with medium effort)".to_string(), None)]);
    assert_eq!(working_on_screen(&format!("✳ Running the tests…{foot}")).unwrap().detail, vec![]);
    // A finished turn, a tool call and a quoted line are not it.
    assert_eq!(working_on_screen(&format!("✻ Brewed for 11s · done 9:44 PM{foot}")), None);
    assert_eq!(working_on_screen(&format!("⏺ Reading… something{foot}")), None);
    assert_eq!(working_on_screen(&format!("  · Embellishing… (3s){foot}")), None);
}

#[test]
fn dialog_on_screen_is_the_question_and_its_choices() {
    use emaki_core::driver::dialog_on_screen;
    let rule = "─".repeat(60);
    // Two questions, the first showing (2.1.289, read off a pty).
    let screen = format!("❯ ask me\n{rule}\n←  ☐ Fruit  ☒ Colors  ✔ Submit  →\nWhich fruit?\n❯ 1. Apple\n     Crisp and sweet,\n     red or green\n  2. Banana\n     Soft\n  3. Type something.\n{rule}\n  4. Chat about this\nEnter to select · Tab/Arrow keys to navigate · Esc to cancel\n");
    let d = dialog_on_screen(&screen).unwrap();
    assert_eq!(d.tabs, vec![("Fruit".to_string(), false), ("Colors".to_string(), true), ("Submit".to_string(), false)]);
    assert_eq!(d.body, vec!["Which fruit?"]);
    assert_eq!(d.current, None);
    // With its colours, the tab showing is the one on a ground of its own.
    let lit = screen.replace("☒ Colors", "\x1b[48:2::177:185:249m\x1b[38:2::0:0:0m ☒ Colors \x1b[49m\x1b[39m");
    assert_eq!(dialog_on_screen(&lit).unwrap().current, Some(1));
    assert_eq!(d.options.iter().map(|o| (o.n, o.label.as_str())).collect::<Vec<_>>(), vec![(1, "Apple"), (2, "Banana"), (3, "Type something."), (4, "Chat about this")]);
    assert_eq!(d.options[0].detail, "Crisp and sweet, red or green");
    assert!(!d.multi);
    // One that takes several.
    let screen = format!("{rule}\n←  ☒ Fruit  ☐ Colors  ✔ Submit  →\nWhich colors?\n❯ 1. [✔] Red\n         Warm\n  2. [ ] Green\n  3. [ ] Type something\n     Submit\n{rule}\n  4. Chat about this\nEnter to select · Tab/Arrow keys to navigate · Esc to cancel\n");
    let d = dialog_on_screen(&screen).unwrap();
    assert!(d.multi);
    assert_eq!(d.options.iter().map(|o| o.checked).collect::<Vec<_>>(), vec![Some(true), Some(false), Some(false), None]);
    assert_eq!((d.options[0].label.as_str(), d.options[0].detail.as_str(), d.options[2].detail.as_str()), ("Red", "Warm", ""));
    // The review, and an approval.
    let screen = format!("{rule}\n←  ☒ Fruit  ☒ Colors  ✔ Submit  →\nReview your answers\n ● Which fruit?\n   → Banana\nReady to submit your answers?\n❯ 1. Submit answers\n  2. Cancel\n");
    let d = dialog_on_screen(&screen).unwrap();
    assert_eq!(d.body, vec!["Review your answers", "Which fruit?", "→ Banana", "Ready to submit your answers?"]);
    assert_eq!(d.options.len(), 2);
    let screen = format!("⏺ earlier\n{rule}\n❯ old\n{rule}\n Bash command\n{}\n │ touch x\n{}\n Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, and always allow access to /tmp/a\n      from this project\n   3. No\n Esc to cancel · Tab to amend\n", "╌".repeat(40), "╌".repeat(40));
    let d = dialog_on_screen(&screen).unwrap();
    assert_eq!(d.body, vec!["Bash command", "touch x", "Do you want to proceed?"]);
    assert_eq!(d.options[1].detail, "from this project");
    // The prompt is not a dialog.
    assert_eq!(dialog_on_screen(&format!("⏺ 1. one\n  2. two\n{rule}\n❯ \n{rule}\n  Context 16%\n  ⏵⏵ auto mode on\n")), None);
    // A named session has a rule under the dialog with its name on it,
    // which is not the dialog's; under the prompt it is still no dialog.
    let named = format!("{rule} v0.1.7 - File system ─");
    let screen = format!("❯ ask me\n{rule}\n←  ☒ Fruit  ☐ Colors  ✔ Submit  →\nWhich colour?\n❯ 1. Red\n  2. Type something.\n{rule}\n  3. Chat about this\n\nEnter to select · Tab/Arrow keys to navigate · Esc to cancel\n{named}\n");
    let d = dialog_on_screen(&screen).unwrap();
    assert_eq!((d.body, d.options.len(), d.tabs.len()), (vec!["Which colour?".to_string()], 3, 3));
    assert_eq!(dialog_on_screen(&format!("{rule}\n⏺ 1. one\n  2. two\n{rule}\n❯ 3. typed\n{named}\n")), None);
}

#[test]
fn suggestion_on_screen_is_the_dim_prompt() {
    use emaki_core::driver::suggestion_on_screen;
    let rule = format!("\x1b[38:2::136:136:136m{}\r", "─".repeat(60));
    let foot = "\x1b[39m  Context 33%\r\n  ⏸ manual mode on\r\n";
    // As Kaku hands it over, 2.1.289.
    let screen = |prompt: &str| format!("✻ Churned for 5s\r\n{rule}\n{prompt}\n{rule}\n{foot}");
    assert_eq!(suggestion_on_screen(&screen("\x1b[39m❯\u{a0}\x1b(B\x1b[0;2mbuild the app\r")).as_deref(), Some("build the app"));
    // A path in it is a link, whose sequences run into the rule below.
    let linked = format!("{rule}\n\x1b[39m❯\u{a0}\x1b(B\x1b[0;2mrun \x1b]8;;file://./x\x1b\\./x\r\n\x1b(B\x1b[0m\x1b[38:2::136:136:136m\x1b]8;;\x1b\\{}\r\n{foot}", "─".repeat(60));
    assert_eq!(suggestion_on_screen(&linked).as_deref(), Some("run ./x"));
    // Typed words, an empty prompt, and a plain screen are not one.
    assert_eq!(suggestion_on_screen(&screen("\x1b[39m❯\u{a0}build the app\r")), None);
    assert_eq!(suggestion_on_screen(&screen("\x1b[39m❯\u{a0}\r")), None);
    assert_eq!(suggestion_on_screen("────────────────────────\n❯ build the app\n────────────────────────\n  Context\n"), None);
}

#[test]
fn mode_on_screen_reads_the_footer_only() {
    use emaki_core::driver::{mode_on_screen, options_from};
    let help = "  --permission-mode <mode>  Permission mode (choices: \"acceptEdits\", \"auto\", \"bypassPermissions\", \"manual\", \"dontAsk\", \"plan\")\n";
    let modes = options_from(&json!({}), help).modes;
    let seen = |text: &str| mode_on_screen(text, &modes);
    // The foot of a terminal session as Kaku hands it over, 2.1.288.
    let foot = |last: &str| format!("⏺ I left plan mode on for you.\n\n\n\n\n\n────\n❯ \n────\n  Context 0% | 5h: 14% (38m) | 7d: 49% (2d3h)\n  {last}\n\n");
    assert_eq!(seen(&foot("⏸ manual mode on · ← 1 agent")).as_deref(), Some("default"));
    assert_eq!(seen(&foot("⏵⏵ accept edits on (shift+tab to cycle)")).as_deref(), Some("acceptEdits"));
    assert_eq!(seen(&foot("⏸ plan mode on (shift+tab to cycle)")).as_deref(), Some("plan"));
    assert_eq!(seen(&foot("⏵⏵ auto mode on (shift+tab to cycle)")).as_deref(), Some("auto"));
    assert_eq!(seen(&foot("⏵⏵ bypass permissions on (shift+tab to cycle)")).as_deref(), Some("bypassPermissions"));
    assert_eq!(seen(&foot("⏵⏵ don't ask on (shift+tab to cycle)")).as_deref(), Some("dontAsk"));
    // A dialog over the prompt: no footer, and the reply's words are not one.
    assert_eq!(seen(&foot("Esc to cancel")), None);
    assert_eq!(seen(""), None);
    // With no list of modes there is nothing to look for.
    assert_eq!(mode_on_screen(&foot("⏸ plan mode on"), &[]), None);
}

/// `/model` and `/effort` answer with a sentence; the conversation keeps
/// what was set, in place of the command's chip. Rows as 2.1.289 writes
/// them for a pick made in the terminal's own picker.
#[test]
fn a_setting_command_says_only_what_it_set() {
    use emaki_core::model::NoticeVariant;
    let said = |name: &str, out: &str| {
        vec![
            row(json!({"type": "user", "timestamp": "2026-01-01T10:01:00Z", "sessionId": "s1", "message": {"role": "user",
                "content": format!("<command-name>/{name}</command-name>\n            <command-message>{name}</command-message>\n            <command-args></command-args>")}})),
            row(json!({"type": "user", "timestamp": "2026-01-01T10:01:00Z", "sessionId": "s1", "message": {"role": "user",
                "content": format!("<local-command-stdout>{out}</local-command-stdout>")}})),
        ]
    };
    let mut rows = vec![
        user("go", "2026-01-01T10:00:00Z"),
        assistant(vec![json!({"type": "text", "text": "Done."})], "end_turn", "2026-01-01T10:00:02Z"),
    ];
    rows.extend(said("effort", "Set effort level to xhigh (saved as your default for new sessions): Comprehensive implementation with extensive testing and documentation"));
    rows.extend(said("model", "Set model to `Fable 5.1` and saved as your default for new sessions"));
    rows.extend(said("model", "Kept model as `Fable 5.1`"));
    rows.extend(said("model", "Set model to `Opus 5 (1M context) (default)` for this session only"));
    rows.extend(said("status", "Version 2.1.289"));
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let notices: Vec<(NoticeVariant, &str)> = s.rounds[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Notice { text, variant, .. } => Some((*variant, text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        notices,
        [
            (NoticeVariant::Effort, "Extra high"),
            // The picker closed with nothing changed leaves nothing, and of
            // two models set one after the other the last is kept.
            (NoticeVariant::Model, "Opus 5 (1M context) (default)"),
            // Any other command keeps its chip and its words.
            (NoticeVariant::Command, "/status"),
            (NoticeVariant::Info, "Version 2.1.289"),
        ]
    );
    assert_eq!(s.effort, "xhigh");
    assert_eq!(NoticeVariant::Effort.said("Extra high"), "Effort Extra high");

    // Toggled again and again, a setting keeps its last word only; one
    // of another kind in between keeps both sides of it.
    let mut rows = vec![user("go", "2026-01-01T10:00:00Z"), assistant(vec![json!({"type": "text", "text": "Done."})], "end_turn", "2026-01-01T10:00:02Z")];
    for (name, out) in [
        ("effort", "Set effort level to high (this session only): words"),
        ("effort", "Set effort level to medium (this session only): words"),
        ("effort", "Set effort level to high (this session only): words"),
        ("model", "Set model to `Fable 5.1`"),
        ("model", "Kept model as `Fable 5.1`"),
        ("model", "Set model to `Opus 5.5`"),
        ("effort", "Set effort level to low (this session only): words"),
    ] {
        rows.extend(said(name, out));
    }
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let notices: Vec<String> = s.rounds[0].items.iter().filter_map(|i| if let Item::Notice { text, variant, .. } = i { Some(variant.said(text)) } else { None }).collect();
    assert_eq!(notices, ["Effort High", "Model Opus 5.5", "Effort Low"]);
    // The ones folded away are set aside in order, for the window.
    let aside: Vec<String> = s.rounds[0].superseded.iter().filter_map(|i| if let Item::Notice { text, variant, .. } = i { Some(variant.said(text)) } else { None }).collect();
    assert_eq!(aside, ["Effort High", "Effort Medium", "Model Fable 5.1"]);
    assert_eq!(NoticeVariant::Info.said("Version 2.1.289"), "Version 2.1.289");
}

/// Claude Code writes nothing when the mode changes; the next prompt row
/// names the new mode, and a `permission-mode` row restates it whenever
/// rows are written. The change is said once, where the file first has it.
#[test]
fn a_change_of_mode_is_said_where_the_transcript_first_has_it() {
    use emaki_core::model::NoticeVariant;
    let prompt = |text: &str, mode: &str, ts: &str| {
        let mut u = user(text, ts);
        u["permissionMode"] = json!(mode);
        u
    };
    let restated = |mode: &str| row(json!({"type": "permission-mode", "permissionMode": mode, "sessionId": "s1"}));
    let rows = vec![
        restated("auto"),
        prompt("one", "auto", "2026-01-01T10:00:00Z"),
        assistant(vec![json!({"type": "text", "text": "Done."})], "end_turn", "2026-01-01T10:00:02Z"),
        restated("auto"),
        prompt("two", "plan", "2026-01-01T10:05:00Z"),
        assistant(vec![json!({"type": "text", "text": "A plan."})], "end_turn", "2026-01-01T10:05:02Z"),
        restated("plan"),
        restated("acceptEdits"),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let modes = |ix: usize| -> Vec<&str> {
        s.rounds[ix]
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Notice { text, variant: NoticeVariant::Mode, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    };
    // Where the session began is not a change; "plan" came with the
    // second prompt, so it is said at the foot of the first round.
    assert_eq!(modes(0), ["Plan"]);
    assert_eq!(modes(1), ["Accept edits"]);
    // Stepped through several before the next row: the last one.
    let mut more = rows.clone();
    more.extend([restated("plan"), restated("default")]);
    let stepped = build(BuildInput { rows: &more, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let last: Vec<String> = stepped.rounds[1].items.iter().filter_map(|i| if let Item::Notice { text, variant: NoticeVariant::Mode, .. } = i { Some(text.clone()) } else { None }).collect();
    assert_eq!(last, ["Manual"]);
    assert_eq!(s.mode, "acceptEdits");
    assert_eq!(s.rounds.len(), 2);
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

/// A message sent mid-turn shows the moment it is queued, and moves to
/// where it was taken up once the row that records it is written.
#[test]
fn a_queued_message_shows_before_it_is_absorbed() {
    let envelope = "<cross-session-message from-name=\"emaki\">\nand also this\n</cross-session-message>";
    let mut rows = vec![
        row(json!({"type": "queue-operation", "operation": "enqueue", "content": "first", "timestamp": "2026-01-01T10:00:00Z", "sessionId": "s1"})),
        row(json!({"type": "queue-operation", "operation": "dequeue", "timestamp": "2026-01-01T10:00:00Z", "sessionId": "s1"})),
        user("first", "2026-01-01T10:00:00Z"),
        assistant(vec![json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "sleep 60"}})], "tool_use", "2026-01-01T10:00:01Z"),
        row(json!({"type": "queue-operation", "operation": "enqueue", "content": "<task-notification>\n<task-id>x</task-id>", "timestamp": "2026-01-01T10:00:02Z", "sessionId": "s1"})),
        row(json!({"type": "queue-operation", "operation": "enqueue", "content": envelope, "timestamp": "2026-01-01T10:00:05Z", "sessionId": "s1"})),
        row(json!({"type": "queue-operation", "operation": "enqueue", "content": "typed in the terminal", "timestamp": "2026-01-01T10:00:09Z", "sessionId": "s1"})),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let seen: Vec<(&str, bool)> = s.rounds.iter().map(|r| (r.prompt.as_str(), r.queued)).collect();
    assert_eq!(seen, vec![("first", false), ("and also this", true), ("typed in the terminal", true)]);
    assert_eq!(s.rounds[1].source, emaki_core::model::Source::Web, "the window's own message is the person's");
    assert_eq!(s.rounds[0].tool_count(), 1, "the running turn keeps its calls");

    // Taken into the turn: the queue empties and the attachment row is the round.
    rows.push(row(json!({"type": "queue-operation", "operation": "remove", "reason": "absorbed_mid_turn", "content": envelope, "timestamp": "2026-01-01T10:00:30Z", "sessionId": "s1"})));
    rows.push(row(json!({"type": "queue-operation", "operation": "remove", "reason": "absorbed_mid_turn", "content": "typed in the terminal", "timestamp": "2026-01-01T10:00:30Z", "sessionId": "s1"})));
    rows.push(row(json!({"type": "attachment", "uuid": "a1", "timestamp": "2026-01-01T10:00:05Z", "sessionId": "s1", "attachment": {"type": "queued_command", "commandMode": "prompt", "prompt": envelope, "origin": {"kind": "peer", "name": "emaki", "body": "and also this"}}})));
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let seen: Vec<(&str, bool)> = s.rounds.iter().map(|r| (r.prompt.as_str(), r.queued)).collect();
    assert_eq!(seen, vec![("first", false), ("and also this", false)]);
}

/// Escape during `/compact` leaves the prompt and nothing after it; the
/// registry's idle, when it is newer than those rows, is what says stopped.
#[test]
fn the_registrys_idle_settles_a_turn_the_transcript_left_working() {
    use emaki_core::build::{turn_state, Phase};
    let rows = vec![user("hello", "2026-01-01T10:00:00Z"), assistant(vec![json!({"type": "text", "text": "hi"})], "end_turn", "2026-01-01T10:00:02Z"), user("/compact", "2026-01-01T10:05:00Z")];
    let at = |ts: &str| emaki_core::build::parse_ts(ts).unwrap().timestamp() as f64;
    let mut st = turn_state(&rows, "/tmp/proj");
    assert_eq!(st.phase, Phase::Working);
    // Idle since the turn before: the registry has not caught up yet.
    assert!(!st.settle_idle(at("2026-01-01T10:00:03Z")));
    assert_eq!(st.phase, Phase::Working);
    assert!(st.settle_idle(at("2026-01-01T10:05:20Z")));
    assert_eq!((st.phase, st.activity_kind.as_str()), (Phase::YourTurn, "stop"));
    // A turn that ended on its own is left as it reads.
    let mut done = turn_state(&rows[..2], "/tmp/proj");
    assert!(!done.settle_idle(at("2026-01-01T10:09:00Z")));
    assert_eq!(done.phase, Phase::YourTurn);
    assert_ne!(done.activity_kind, "stop");
}

/// Escape before the agent did anything withdraws the prompt; after it
/// did, the round stays and only the marker goes.
#[test]
fn a_prompt_stopped_untouched_is_withdrawn() {
    use emaki_core::build::turn_state;
    let mut rows = vec![
        user("hello", "2026-01-01T10:00:00Z"),
        assistant(vec![json!({"type": "text", "text": "hi"})], "end_turn", "2026-01-01T10:00:02Z"),
        user("do the thing", "2026-01-01T10:01:00Z"),
        user("[Request interrupted by user]", "2026-01-01T10:01:03Z"),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.rounds.iter().map(|r| r.prompt.as_str()).collect::<Vec<_>>(), vec!["hello"]);
    assert_eq!(s.withdrawn.as_ref().map(|r| r.prompt.as_str()), Some("do the thing"));
    assert_eq!(turn_state(&rows, "/tmp/proj").activity_kind, "stop");

    // The next prompt is a round as usual, and nothing is withdrawn any more.
    rows.push(user("do the other thing", "2026-01-01T10:02:00Z"));
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.rounds.len(), 2);
    assert!(s.withdrawn.is_none());

    // Stopped after the agent had started: the round stays, and ends on
    // the stop in the marker's own words.
    let rows = vec![
        user("do the thing", "2026-01-01T10:01:00Z"),
        assistant(vec![json!({"type": "text", "text": "starting"})], "", "2026-01-01T10:01:02Z"),
        user("[Request interrupted by user]", "2026-01-01T10:01:03Z"),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.rounds.iter().map(|r| r.prompt.as_str()).collect::<Vec<_>>(), vec!["do the thing"]);
    assert!(s.withdrawn.is_none());
    assert!(matches!(s.rounds[0].items.last(), Some(Item::Notice { text, variant: emaki_core::model::NoticeVariant::Interrupted, .. }) if text == "Request interrupted by user"));
}

/// Escape pressed at once leaves no marker: the prompt's row has nothing
/// under it and the next prompt is written beside it (2.1.293).
#[test]
fn a_prompt_taken_back_at_once_is_no_round() {
    let chained = |mut row: Value, uuid: &str, parent: &str| {
        row["uuid"] = json!(uuid);
        row["parentUuid"] = json!(parent);
        row
    };
    let rows = vec![
        chained(user("hello", "2026-01-01T10:00:00Z"), "u1", ""),
        chained(assistant(vec![json!({"type": "text", "text": "hi"})], "end_turn", "2026-01-01T10:00:02Z"), "a1", "u1"),
        chained(user("try again", "2026-01-01T10:01:00Z"), "u2", "a1"),
        chained(user("try again, and also this", "2026-01-01T10:01:30Z"), "u3", "a1"),
        chained(assistant(vec![json!({"type": "text", "text": "on it"})], "end_turn", "2026-01-01T10:01:32Z"), "a2", "u3"),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.rounds.iter().map(|r| r.prompt.as_str()).collect::<Vec<_>>(), vec!["hello", "try again, and also this"]);
    // Until the next prompt is written there is nothing to tell it by:
    // it is still the last round, for the window to hand back.
    let s = build(BuildInput { rows: &rows[..3], transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.rounds.len(), 2);
    // A prompt the agent answered is kept, whatever is written beside it.
    let mut rows = rows;
    rows.push(chained(user("another way", "2026-01-01T10:03:00Z"), "u4", "a1"));
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.rounds.iter().map(|r| r.prompt.as_str()).collect::<Vec<_>>(), vec!["hello", "try again, and also this", "another way"]);
}

/// Codex says a stop twice, as an event and inside the next user row;
/// the round ends on it once, and reads as stopped, not working.
#[test]
fn a_codex_turn_stopped_says_so() {
    use emaki_core::model::NoticeVariant;
    let user = |text: &str, ts: &str| json!({"timestamp": ts, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]}});
    let mut rows = vec![
        user("list files", "2026-03-02T02:38:02Z"),
        json!({"timestamp": "2026-03-02T02:38:04Z", "type": "event_msg", "payload": {"type": "turn_aborted", "reason": "interrupted"}}),
    ];
    let s = build_codex(&rows, "");
    assert!(matches!(s.rounds[0].items.last(), Some(Item::Notice { text, variant: NoticeVariant::Interrupted, .. }) if text == "Interrupted"));
    assert_eq!(emaki_core::adapters::turn_state_from_session(&s).activity_kind, "stop");

    rows.push(user("<turn_aborted>\nThe user interrupted the previous turn on purpose. Any running unified exec processes may still be running in the background.\n</turn_aborted>", "2026-03-02T02:38:05Z"));
    rows.push(user("try again", "2026-03-02T02:38:09Z"));
    let s = build_codex(&rows, "");
    assert_eq!(s.rounds.len(), 2);
    assert_eq!(s.rounds[0].items.len(), 1);

    // The user row alone, as an older Codex wrote it.
    rows.remove(1);
    let s = build_codex(&rows, "");
    assert!(matches!(s.rounds[0].items.last(), Some(Item::Notice { text, variant: NoticeVariant::Interrupted, .. }) if text == "The user interrupted the previous turn on purpose."));
}

/// Rows of a Codex rollout as 0.162 writes them, for the tests below.
mod cx {
    use serde_json::{json, Value};

    pub fn ev(ts: &str, payload: Value) -> Value {
        json!({"timestamp": format!("2026-10-10T09:00:{ts}Z"), "type": "event_msg", "payload": payload})
    }
    pub fn item(ts: &str, item: Value) -> Value {
        ev(ts, json!({"type": "item_completed", "turn_id": "t", "item": item}))
    }
    pub fn resp(ts: &str, payload: Value) -> Value {
        json!({"timestamp": format!("2026-10-10T09:00:{ts}Z"), "type": "response_item", "payload": payload})
    }
    pub fn started(ts: &str) -> Value {
        ev(ts, json!({"type": "task_started", "turn_id": "t"}))
    }
    pub fn complete(ts: &str) -> Value {
        ev(ts, json!({"type": "task_complete", "turn_id": "t", "last_agent_message": null}))
    }
    /// A prompt, said twice as Codex says it: the row the model is sent,
    /// then the item.
    pub fn prompt(ts: &str, text: &str) -> [Value; 2] {
        [
            resp(ts, json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]})),
            item(ts, json!({"type": "UserMessage", "id": format!("u-{ts}"), "content": [{"type": "text", "text": text, "text_elements": []}]})),
        ]
    }
    /// The agent's words, likewise: the item, then the row under the same id.
    pub fn says(ts: &str, text: &str, phase: &str) -> [Value; 2] {
        [
            item(ts, json!({"type": "AgentMessage", "id": format!("msg-{ts}"), "content": [{"type": "Text", "text": text}], "phase": phase})),
            resp(ts, json!({"type": "message", "id": format!("msg-{ts}"), "role": "assistant", "content": [{"type": "output_text", "text": text}], "phase": phase})),
        ]
    }
    pub fn script(ts: &str, id: &str, js: &str) -> Value {
        resp(ts, json!({"type": "custom_tool_call", "status": "completed", "call_id": id, "name": "exec", "input": js}))
    }
    pub fn script_out(ts: &str, id: &str, printed: &str) -> Value {
        resp(ts, json!({"type": "custom_tool_call_output", "call_id": id, "output": [{"type": "input_text", "text": "Script completed\nWall time 0.1 seconds\nOutput:\n"}, {"type": "input_text", "text": printed}]}))
    }
    pub fn command(ts: &str, id: &str, line: &str, output: &str, status: &str, code: i64) -> Value {
        item(ts, json!({"type": "CommandExecution", "id": id, "command": ["/bin/zsh", "-lc", line], "cwd": "file:///tmp/proj", "status": status, "aggregated_output": output, "exit_code": code, "duration": {"secs": 2, "nanos": 500000000}}))
    }
    pub fn context(ts: &str, payload: Value) -> Value {
        json!({"timestamp": format!("2026-10-10T09:00:{ts}Z"), "type": "turn_context", "payload": payload})
    }
}

/// Codex 0.162 runs tools from a script the model writes. The rollout
/// also says what the script ran, and that is what is shown: the command
/// as a `Bash` call, the wrapper gone, each thing said once.
#[test]
fn a_codex_script_is_shown_by_what_it_ran() {
    let mut rows = vec![json!({"timestamp": "2026-10-10T09:00:00Z", "type": "session_meta", "payload": {"id": "c2", "cwd": "/tmp/proj", "cli_version": "0.162.0"}}), cx::started("00")];
    rows.extend(cx::prompt("01", "say hello"));
    rows.extend(cx::says("02", "I will run it.", "commentary"));
    rows.push(cx::script("03", "call_1", "const r = await tools.exec_command({cmd:\"echo hello\",\"max_output_tokens\":200});\ntext(r.output);\n"));
    rows.push(cx::command("04", "exec-1", "echo hello", "", "completed", 0));
    rows.push(cx::script_out("04", "call_1", "hello\n"));
    rows.push(cx::ev("05", json!({"type": "token_count", "info": {"total_token_usage": {"input_tokens": 1000, "cached_input_tokens": 600, "output_tokens": 50}, "last_token_usage": {"input_tokens": 700, "cached_input_tokens": 600, "output_tokens": 20}, "model_context_window": 258400}, "rate_limits": {"primary": {"used_percent": 20.0, "window_minutes": 300, "resets_at": 1794215254}, "secondary": {"used_percent": 5.0, "window_minutes": 10080, "resets_at": 1794815254}}})));
    // Mid-turn: the words so far are commentary, and the turn is running.
    let s = build_codex(&rows, "");
    // The row under the composer: the window's size, and the account's
    // windows named by their own lengths.
    assert_eq!((s.context_tokens, s.context_window), (700, 258400));
    assert_eq!(s.usage_windows.iter().map(|w| (w.label(), w.used)).collect::<Vec<_>>(), vec![("5h".to_string(), 0.2), ("7d".to_string(), 0.05)]);
    assert!(s.usage_windows_at > 0.0);
    assert_eq!(emaki_core::adapters::turn_state_from_session(&s).phase, Phase::Working);

    rows.extend(cx::says("06", "It printed hello.", "final_answer"));
    rows.push(cx::complete("07"));
    let s = build_codex(&rows, "");
    assert_eq!(s.rounds.len(), 1);
    let r = &s.rounds[0];
    assert_eq!(r.prompt, "say hello");
    assert_eq!(r.items.len(), 3, "commentary, the command, the answer");
    assert!(matches!(&r.items[0], Item::Text { md, .. } if md == "I will run it."));
    assert!(matches!(&r.items[2], Item::Text { md, .. } if md == "It printed hello."));
    let call = r.tool_calls().next().unwrap();
    assert_eq!((call.name.as_str(), call.tool_kind, call.status), ("Bash", emaki_core::model::ToolKind::Bash, CallStatus::Ok));
    assert_eq!(call.input["command"], "echo hello");
    assert_eq!(call.subject, "echo hello");
    // The command's own output was empty; the one thing the script
    // printed is it.
    assert_eq!(call.stdout, "hello\n");
    assert_eq!(call.duration_ms, 2500);
    // Cached tokens are inside input_tokens.
    assert_eq!((s.usage.input_tokens, s.usage.cache_read, s.usage.output_tokens), (400, 600, 50));
    assert_eq!(r.usage.output_tokens, 50);
    assert_eq!(s.context_tokens, 700);
    let st = emaki_core::adapters::turn_state_from_session(&s);
    assert_eq!((st.phase, st.activity_kind.as_str(), st.reply.as_str()), (Phase::YourTurn, "reply", "It printed hello."));

    // A script that printed the tool's whole return: the output is in it.
    let n = rows.iter().position(|r| r["payload"]["type"] == "custom_tool_call_output").unwrap();
    rows[n] = cx::script_out("04", "call_1", "{\"chunk_id\":\"a\",\"exit_code\":0,\"output\":\"hello\\n\"}");
    assert_eq!(build_codex(&rows, "").rounds[0].tool_calls().next().unwrap().stdout, "hello\n");
}

/// A script whose command left no item (the sandbox refused it) is still
/// a command; one that only computes is shown as the script it is, by its
/// first line. Neither is raw JSON under a tool named "exec".
#[test]
fn a_codex_script_with_no_item_is_one_call() {
    let mut rows = vec![cx::started("00")];
    rows.extend(cx::prompt("01", "make it"));
    rows.push(cx::script("02", "call_1", "const r = await tools.exec_command({cmd:\"cat > hi.sh <<'EOF'\\nhi\\nEOF\",max_output_tokens:1000});\ntext(r.output);\n"));
    rows.push(cx::script_out("03", "call_1", "zsh:1: operation not permitted: hi.sh\n"));
    rows.push(cx::script("04", "call_2", "// add them up\nconst n = [1, 2].reduce((a, b) => a + b);\ntext(String(n));\n"));
    rows.push(cx::script_out("05", "call_2", "3"));
    rows.push(cx::complete("06"));
    let s = build_codex(&rows, "");
    let calls: Vec<_> = s.rounds[0].tool_calls().collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].name, "Bash");
    assert_eq!(calls[0].input["command"], "cat > hi.sh <<'EOF'\nhi\nEOF");
    assert_eq!(calls[0].stdout, "zsh:1: operation not permitted: hi.sh\n");
    assert_eq!(calls[1].name, "exec");
    assert_eq!(calls[1].subject, "const n = [1, 2].reduce((a, b) => a + b);");
    assert_eq!(calls[1].stdout, "3");
    assert_eq!(calls[1].status, CallStatus::Ok);
}

/// A patch is a call a file, with hunks in the shape Claude Code's
/// `structuredPatch` has, from the item of a new rollout and from the
/// `apply_patch` call of an old one.
#[test]
fn a_codex_patch_is_a_call_a_file() {
    use emaki_core::model::ToolKind;
    use emaki_core::render_md::{patch_stat, render_patch};
    let mut rows = vec![json!({"timestamp": "2026-10-10T09:00:00Z", "type": "session_meta", "payload": {"id": "c3", "cwd": "/tmp/proj"}}), cx::started("00")];
    rows.extend(cx::prompt("01", "patch them"));
    rows.push(cx::script("02", "call_1", "const p = await tools.apply_patch(\"*** Begin Patch\\n*** End Patch\");\ntext(p);\n"));
    rows.push(cx::item("03", json!({"type": "FileChange", "id": "exec-9", "status": "completed", "stdout": "Success.", "stderr": "", "changes": {
        "/tmp/proj/a.txt": {"type": "add", "content": "one\ntwo\n"},
        "/tmp/proj/b.txt": {"type": "update", "unified_diff": "--- a/b.txt\n+++ b/b.txt\n@@ -1,3 +1,3 @@\n keep\n-old\n+new\n keep\n", "move_path": null},
        "/tmp/proj/c.txt": {"type": "delete", "content": "gone\n"},
    }})));
    rows.push(cx::script_out("03", "call_1", "{}"));
    rows.push(cx::complete("04"));
    let s = build_codex(&rows, "");
    let calls: Vec<_> = s.rounds[0].tool_calls().collect();
    assert_eq!(calls.iter().map(|c| (c.name.as_str(), c.tool_kind, c.subject.as_str())).collect::<Vec<_>>(), [("Write", ToolKind::Write, "a.txt"), ("Edit", ToolKind::Edit, "b.txt"), ("Delete", ToolKind::Edit, "c.txt")]);
    assert_eq!(calls.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["exec-9", "exec-9:1", "exec-9:2"]);
    assert!(calls.iter().all(|c| c.status == CallStatus::Ok));
    assert_eq!(calls[0].input["content"], "one\ntwo\n");
    assert_eq!(calls[0].file_path, "/tmp/proj/a.txt");
    assert_eq!(calls[1].patch, vec![json!({"oldStart": 1, "oldLines": 3, "newStart": 1, "newLines": 3, "lines": [" keep", "-old", "+new", " keep"]})]);
    assert_eq!(render_patch(&calls[1].patch, 12), "@@ -1,3 +1,3 @@\n keep\n-old\n+new\n keep");
    assert_eq!(patch_stat(&calls[2].patch), "+0 −1");

    // An older Codex: the patch is the call's argument, its result one row.
    let rows = vec![
        json!({"timestamp": "2026-03-02T02:38:02Z", "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "patch them"}]}}),
        json!({"timestamp": "2026-03-02T02:38:03Z", "type": "response_item", "payload": {"type": "custom_tool_call", "call_id": "p1", "name": "apply_patch",
            "input": "*** Begin Patch\n*** Add File: a.txt\n+one\n*** Update File: b.txt\n*** Move to: d.txt\n@@ fn main\n keep\n-old\n+new\n*** End Patch"}}),
        json!({"timestamp": "2026-03-02T02:38:04Z", "type": "response_item", "payload": {"type": "custom_tool_call_output", "call_id": "p1", "output": "{\"output\":\"Success.\",\"metadata\":{\"exit_code\":0}}"}}),
    ];
    let s = build_codex(&rows, "/tmp/proj");
    let calls: Vec<_> = s.rounds[0].tool_calls().collect();
    assert_eq!(calls.iter().map(|c| (c.name.as_str(), c.subject.as_str(), c.status)).collect::<Vec<_>>(), [("Write", "a.txt", CallStatus::Ok), ("Edit", "b.txt → d.txt", CallStatus::Ok)]);
    assert_eq!(calls[0].input["content"], "one\n");
    assert_eq!(patch_stat(&calls[1].patch), "+1 −1");
    assert_eq!(calls[1].result_text, "Success.");
}

/// The mode is one key, the first settings a session names are where it
/// began, and a later change is a line at the foot of the round before.
#[test]
fn a_codex_command_that_outlasts_its_scripts_wait_is_one_call() {
    // The script that starts it gives up waiting; a second script only
    // waits, and the command's item comes under that one.
    let mut rows = vec![cx::started("00")];
    rows.extend(cx::prompt("01", "sleep"));
    rows.push(cx::script("02", "c1", "const r = await tools.exec_command({cmd:\"sleep 12\"});\ntext(JSON.stringify(r));"));
    rows.push(cx::script_out("12", "c1", "{\"chunk_id\":\"a\",\"wall_time_seconds\":10.0}"));
    rows.push(cx::script("13", "c2", "const r = await tools.write_stdin({session_id:1,yield_time_ms:5000});\ntext(JSON.stringify(r));"));
    rows.push(cx::command("14", "exec-1", "sleep 12", "", "completed", 0));
    rows.push(cx::script_out("14", "c2", "{\"exit_code\":0}"));
    rows.extend(cx::says("15", "done", "final_answer"));
    rows.push(cx::complete("16"));
    let s = build_codex(&rows, "");
    let calls: Vec<&emaki_core::model::ToolCall> = s.rounds[0].items.iter().filter_map(|i| if let Item::Tool(c) = i { Some(c) } else { None }).collect();
    assert_eq!(calls.len(), 1, "{:?}", calls.iter().map(|c| (&c.name, &c.subject)).collect::<Vec<_>>());
    assert_eq!((calls[0].name.as_str(), calls[0].input.get("command").and_then(|c| c.as_str())), ("Bash", Some("sleep 12")));
    // It stands where the command began.
    assert_eq!(calls[0].ts, "2026-10-10T09:00:02Z");
}

#[test]
fn codex_settings_are_read_and_their_changes_said() {
    use emaki_core::model::NoticeVariant;
    let ctx = |ts: &str, extra: Value| {
        let mut p = json!({"cwd": "/tmp/proj", "model": "gpt-a", "effort": "low", "collaboration_mode": {"mode": "default", "settings": {"model": "gpt-a", "reasoning_effort": "low"}}});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        cx::context(ts, p)
    };
    let mut rows = vec![cx::started("00"), ctx("00", json!({"sandbox_policy": {"type": "read-only"}}))];
    rows.extend(cx::prompt("01", "one"));
    rows.extend(cx::says("02", "Done.", "final_answer"));
    rows.push(cx::complete("03"));
    let s = build_codex(&rows, "");
    assert_eq!((s.mode.as_str(), s.effort.as_str(), s.models.clone()), ("read-only", "low", vec!["gpt-a".to_string()]));
    assert_eq!(s.rounds[0].items.len(), 1, "where a session began is not a change");

    // Set between turns, twice: the same profile again, then another.
    rows.push(cx::ev("10", json!({"type": "thread_settings_applied", "thread_settings": {"model": "gpt-a", "reasoning_effort": "low", "active_permission_profile": {"id": ":read-only"}, "collaboration_mode": {"mode": "default"}}})));
    rows.push(cx::ev("11", json!({"type": "thread_settings_applied", "thread_settings": {"model": "gpt-a", "reasoning_effort": "low", "active_permission_profile": {"id": ":workspace"}, "collaboration_mode": {"mode": "default"}}})));
    rows.push(cx::started("12"));
    rows.push(ctx("12", json!({"sandbox_policy": {"type": "workspace-write"}, "active_permission_profile": {"id": ":workspace"}, "model": "gpt-b", "effort": "high"})));
    rows.extend(cx::prompt("13", "two"));
    let s = build_codex(&rows, "");
    assert_eq!((s.mode.as_str(), s.effort.as_str()), ("workspace", "high"));
    assert_eq!(s.models, ["gpt-a", "gpt-b"]);
    let said: Vec<(NoticeVariant, String)> = s.rounds[0].items.iter().filter_map(|i| if let Item::Notice { variant, text, .. } = i { Some((*variant, text.clone())) } else { None }).collect();
    assert_eq!(said, [(NoticeVariant::Mode, emaki_core::driver::mode_words("workspace").0), (NoticeVariant::Model, "GPT-B".into()), (NoticeVariant::Effort, emaki_core::driver::effort_words("high").0)]);
    // The change is not when the round before ended.
    assert_eq!(s.rounds[0].end_ts, "2026-10-10T09:00:02Z");

    // Plan mode wins over the profile; an old row has only the sandbox.
    for (extra, key) in [
        (json!({"active_permission_profile": {"id": ":workspace"}, "collaboration_mode": {"mode": "plan"}}), "plan"),
        (json!({"active_permission_profile": {"id": ":danger-full-access"}}), "danger-full-access"),
        (json!({"sandbox_policy": {"type": "workspace-write"}}), "workspace"),
        (json!({"sandbox_policy": {"type": "danger-full-access"}}), "danger-full-access"),
    ] {
        assert_eq!(build_codex(&[ctx("00", extra)], "").mode, key);
    }
}

/// A turn that failed ends on the error, in the notice Claude Code's
/// errors use, and reads as over. A stop is said once, under the command
/// it cut short, which Codex writes after the stop.
#[test]
fn a_codex_turn_that_failed_or_was_stopped_says_so() {
    use emaki_core::model::NoticeVariant;
    let mut rows = vec![cx::started("00")];
    rows.extend(cx::prompt("01", "run it"));
    let s = build_codex(&rows, "");
    assert_eq!(emaki_core::adapters::turn_state_from_session(&s).phase, Phase::Working);

    let mut failed = rows.clone();
    failed.push(cx::ev("02", json!({"type": "task_complete", "turn_id": "t", "last_agent_message": null, "error": {"message": "{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-5' model is not supported.\"}}", "codex_error_info": "other"}})));
    let s = build_codex(&failed, "");
    assert!(matches!(s.rounds[0].items.as_slice(), [Item::Notice { variant: NoticeVariant::Error, text, .. }] if text == "The 'gpt-5' model is not supported."));
    let st = emaki_core::adapters::turn_state_from_session(&s);
    assert_eq!((st.phase, st.activity.as_str()), (Phase::YourTurn, "failed"));

    rows.push(cx::script("02", "call_1", "const r = await tools.exec_command({cmd:\"sleep 30\"});\ntext(JSON.stringify(r));\n"));
    rows.push(cx::resp("03", json!({"type": "custom_tool_call_output", "call_id": "call_1", "output": "aborted by user after 0.2s"})));
    rows.push(cx::resp("03", json!({"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "<turn_aborted>\nThe previous turn was interrupted on purpose.\n</turn_aborted>"}]})));
    rows.push(cx::ev("03", json!({"type": "turn_aborted", "turn_id": "t", "reason": "interrupted"})));
    rows.push(cx::command("03", "exec-1", "sleep 30", "", "failed", -1));
    let s = build_codex(&rows, "");
    let r = &s.rounds[0];
    assert_eq!(r.items.len(), 2);
    let call = r.tool_calls().next().unwrap();
    assert_eq!((call.name.as_str(), call.subject.as_str(), call.status), ("Bash", "sleep 30", CallStatus::Interrupted));
    assert!(matches!(r.items.last(), Some(Item::Notice { variant: NoticeVariant::Interrupted, text, .. }) if text == "Interrupted"));
    let st = emaki_core::adapters::turn_state_from_session(&s);
    assert_eq!((st.phase, st.activity_kind.as_str()), (Phase::YourTurn, "stop"));
}

/// A question and a plan are the calls the window already draws for
/// Claude Code. The plan, said as an item and again inside the reply's
/// row, is one call; it waits until the next prompt, which sent out of
/// plan mode is the go-ahead.
#[test]
fn a_codex_question_and_plan_are_drawn_as_claudes() {
    use emaki_core::model::{questions_of, ToolKind};
    let plan_mode = |ts: &str, mode: &str| cx::context(ts, json!({"model": "gpt-a", "collaboration_mode": {"mode": mode}, "active_permission_profile": {"id": ":workspace"}}));
    let mut rows = vec![cx::started("00"), plan_mode("00", "plan")];
    rows.extend(cx::prompt("01", "a greeting script"));
    rows.push(cx::resp("02", json!({"type": "function_call", "name": "request_user_input", "call_id": "q1",
        "arguments": "{\"questions\":[{\"header\":\"Language\",\"id\":\"lang\",\"question\":\"Which language?\",\"options\":[{\"label\":\"Python\",\"description\":\"No setup.\"},{\"label\":\"Shell\",\"description\":\"Minimal.\"}]}]}"})));
    let s = build_codex(&rows, "");
    let st = emaki_core::adapters::turn_state_from_session(&s);
    assert_eq!((st.phase, st.activity.as_str(), st.activity_kind.as_str()), (Phase::NeedsYou, "Which language?", "ask"));

    rows.push(cx::resp("03", json!({"type": "function_call_output", "call_id": "q1", "output": "{\"answers\":{\"lang\":{\"answers\":[\"Python\"]}}}"})));
    rows.push(cx::item("04", json!({"type": "Plan", "id": "t-plan", "text": "# Greeting\n\n- Create `hi.py`."})));
    rows.push(cx::resp("04", json!({"type": "message", "id": "msg-4", "role": "assistant", "phase": "final_answer", "content": [{"type": "output_text", "text": "<proposed_plan>\n# Greeting\n\n- Create `hi.py`.\n</proposed_plan>"}]})));
    rows.push(cx::complete("05"));
    let s = build_codex(&rows, "");
    assert_eq!(s.mode, "plan");
    let r = &s.rounds[0];
    assert_eq!(r.items.len(), 2, "the question and the plan, once each");
    let ask = r.tool_calls().next().unwrap();
    assert_eq!((ask.name.as_str(), ask.tool_kind, ask.status), ("AskUserQuestion", ToolKind::Ask, CallStatus::Ok));
    let qs = questions_of(&ask.input);
    assert_eq!((qs[0].question.as_str(), qs[0].header.as_str(), qs[0].options.len(), qs[0].multi), ("Which language?", "Language", 2, false));
    assert_eq!(ask.answers, [("Which language?".to_string(), "Python".to_string())]);
    let plan = r.tool_calls().nth(1).unwrap();
    assert_eq!((plan.name.as_str(), plan.tool_kind, plan.status), ("ExitPlanMode", ToolKind::Plan, CallStatus::Pending));
    assert_eq!(plan.input["plan"], "# Greeting\n\n- Create `hi.py`.");
    let st = emaki_core::adapters::turn_state_from_session(&s);
    assert_eq!((st.phase, st.activity_kind.as_str()), (Phase::YourTurn, "plan"));

    rows.push(cx::started("10"));
    rows.push(plan_mode("10", "default"));
    rows.extend(cx::prompt("11", "Implement the plan."));
    let s = build_codex(&rows, "");
    assert_eq!(s.rounds[0].tool_calls().nth(1).unwrap().status, CallStatus::Ok);
    assert_eq!(s.mode, "workspace");
}

/// A rollout moved up from an old Codex: the prompt's row comes before
/// the turn starts and its item after, a thought is written again each
/// time it grows, and rows repeat what the items said. Each is read once.
#[test]
fn a_codex_rollout_said_twice_is_read_once() {
    let (_home, _guard) = isolated();
    let picture = std::env::temp_dir().join("emaki-codex-shot.png");
    let thought = |parts: &[&str]| cx::item("03", json!({"type": "Reasoning", "id": "item-2", "summary_text": parts, "raw_content": []}));
    let rows = vec![
        cx::resp("01", json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "<environment_context>\n<cwd>/tmp</cwd>\n</environment_context>"}]})),
        cx::resp("02", json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "is it true?"}, {"type": "input_image", "image_url": "data:image/png;base64,AAAA"}]})),
        cx::started("02"),
        cx::item("02", json!({"type": "UserMessage", "id": "item-1", "content": [{"type": "text", "text": "is it true?", "text_elements": []}, {"type": "localImage", "path": picture.to_string_lossy()}]})),
        thought(&["**One**"]),
        thought(&["**One**", "**Two**"]),
        cx::item("04", json!({"type": "AgentMessage", "id": "item-3", "content": [{"type": "Text", "text": "Not natively."}]})),
        cx::resp("04", json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": "**One**"}, {"type": "summary_text", "text": "**Two**"}], "content": null})),
        cx::resp("04", json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Not natively."}]})),
        cx::complete("05"),
    ];
    let s = build_codex(&rows, "");
    assert_eq!(s.rounds.len(), 1);
    let r = &s.rounds[0];
    assert_eq!((r.prompt.as_str(), r.images), ("is it true?", 1));
    assert_eq!(r.attachments.len(), 1);
    assert_eq!((r.attachments[0].kind.as_str(), r.attachments[0].name.as_str(), r.attachments[0].media_type.as_str()), ("image", "emaki-codex-shot.png", "image/png"));
    assert_eq!(r.items.len(), 2);
    assert!(matches!(&r.items[0], Item::Thinking { md, .. } if md == "**One**\n\n**Two**"));
    assert!(matches!(&r.items[1], Item::Text { md, .. } if md == "Not natively."));
}

/// A thread the person named is listed and opened under that name; one
/// they did not, under its first prompt.
#[test]
fn a_codex_thread_named_takes_the_name() {
    use emaki_core::adapters::{codex::CodexAdapter, Adapter};
    let (_home, _guard) = isolated();
    let home = PathBuf::from(std::env::var("CODEX_HOME").unwrap());
    let dir = home.join("sessions/2026/10/10");
    fs::create_dir_all(&dir).unwrap();
    let write = |id: &str| {
        let mut rows = vec![json!({"timestamp": "2026-10-10T09:00:00Z", "type": "session_meta", "payload": {"id": id, "cwd": "/tmp/proj"}}), cx::started("00")];
        rows.extend(cx::prompt("01", "run the trial"));
        rows.extend(cx::says("02", "Done.", "final_answer"));
        rows.push(cx::complete("03"));
        let path = dir.join(format!("rollout-2026-10-10T09-00-00-{id}.jsonl"));
        fs::write(&path, rows.iter().map(|r| r.to_string() + "\n").collect::<String>()).unwrap();
        path
    };
    let (named, plain) = (write("aaaa"), write("bbbb"));
    fs::write(home.join("session_index.jsonl"), "{\"id\":\"aaaa\",\"thread_name\":\"First name\",\"updated_at\":\"2026-10-10T09:01:00Z\"}\n{\"id\":\"aaaa\",\"thread_name\":\"Wire trial\",\"updated_at\":\"2026-10-10T09:02:00Z\"}\n").unwrap();
    let a = CodexAdapter::new();
    let r = a.peek(&named);
    assert_eq!((r.title.as_str(), r.named.as_str(), r.state.phase), ("Wire trial", "Wire trial", Phase::YourTurn));
    assert_eq!(a.load_path(&named, "").title, "Wire trial");
    let r = a.peek(&plain);
    assert_eq!((r.title.as_str(), r.named.as_str()), ("run the trial", ""));
    assert_eq!(a.list(0).len(), 2);
}

/// `/plan` leaves no mode row until the next prompt; its output says it.
#[test]
fn plan_mode_is_read_from_the_commands_own_output() {
    use emaki_core::build::turn_state;
    let mut auto = user("hello", "2026-01-01T10:00:00Z");
    auto["permissionMode"] = json!("auto");
    let rows = vec![
        auto,
        assistant(vec![json!({"type": "text", "text": "hi"})], "end_turn", "2026-01-01T10:00:02Z"),
        row(json!({"type": "user", "timestamp": "2026-01-01T10:01:00Z", "sessionId": "s1", "message": {"role": "user", "content": "<command-name>/plan</command-name>"}})),
        row(json!({"type": "user", "timestamp": "2026-01-01T10:01:00Z", "sessionId": "s1", "message": {"role": "user", "content": "<local-command-stdout>Enabled plan mode</local-command-stdout>"}})),
    ];
    assert_eq!(turn_state(&rows[..2], "/tmp/proj").mode, "auto");
    assert_eq!(turn_state(&rows, "/tmp/proj").mode, "plan");
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
    // A session's own window, as its status line named it, beats the
    // model's; one about another model does not; and a context larger
    // than its window means the window is the large one.
    use emaki_core::limits::SessionContext;
    let own = SessionContext { model: "claude-opus-5-5[1m]".into(), window: 1_000_000, used: 238_000, seen_at: 1.0, ..Default::default() };
    assert_eq!(l.window_for("claude-opus-5-5", 238_000, Some(&own)), 1_000_000);
    assert_eq!(l.window_for("claude-haiku-4-5", 50_000, Some(&own)), 200_000);
    assert_eq!(l.window_for("claude-opus-5-5", 90_000, None), 200_000);
    assert_eq!(l.window_for("claude-opus-5-5", 238_000, None), 1_000_000);
    assert!(l.learn_window(&own));
    assert!(!l.learn_window(&own));
    assert_eq!(l.window_for("claude-opus-5-5", 90_000, None), 1_000_000);
    assert_eq!(until(1790676000.0, 1790660000.0), "4h26m");
    assert_eq!(until(1791205200.0, 1790660000.0), "6d7h");
    assert_eq!(until(1790660100.0, 1790660000.0), "1m");
    assert_eq!(until(1.0, 2.0), "");
    assert_eq!(emaki_core::driver::effort_words("xhigh").0, "Extra high");
    assert_eq!(emaki_core::driver::effort_words("medium").0, "Medium");
    assert_eq!(emaki_core::driver::mode_words("default").0, "Manual");
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
    use emaki_core::limits::{Limits, Window};
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
    // The value kept is the old window's: past its reset nothing of it is
    // spent, and the row says no time. The other window is as it was.
    let five = l.five_hour.unwrap();
    assert_eq!(five.at(1789999999.0), five);
    assert_eq!(five.at(1790000000.0), Window::default());
    assert_eq!(l.seven_day.unwrap().at(1790000000.0).utilization, 0.12);
    // A window with no reset named is never past it.
    let open = Window { utilization: 0.3, resets_at: 0.0 };
    assert_eq!(open.at(1790000000.0), open);
}

#[test]
fn installing_the_status_line_points_claude_code_at_our_script_and_restore_puts_it_back() {
    use emaki_core::statusline::{self, State};
    let (_home, _g) = isolated();
    // Nothing yet: no settings file at all.
    assert_eq!(statusline::state(), State::None);

    // Someone else's status line, and other settings that must survive.
    let settings = statusline::settings_file();
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, r#"{"model": "opus", "statusLine": {"type": "command", "command": "bash ~/.claude/statusline.sh"}}"#).unwrap();
    assert_eq!(statusline::state(), State::Other("bash ~/.claude/statusline.sh".into()));

    statusline::install().unwrap();
    assert_eq!(statusline::state(), State::Emaki { current: true });
    let script = statusline::script_path();
    assert_eq!(std::fs::read_to_string(&script).unwrap(), statusline::SCRIPT);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&script).unwrap().permissions().mode() & 0o111, 0o111);
    }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v["model"], "opus", "other keys are kept");
    assert_eq!(v["statusLine"]["type"], "command");
    assert_eq!(v["statusLine"]["command"], statusline::command());
    assert!(statusline::command().ends_with("bin/statusline.sh"));
    assert_eq!(statusline::previous().unwrap()["command"], "bash ~/.claude/statusline.sh");

    // Installing again keeps the original previous value, not ours, and
    // the launch path writes nothing once the setting is ours and current.
    statusline::install().unwrap();
    assert_eq!(statusline::previous().unwrap()["command"], "bash ~/.claude/statusline.sh");
    assert!(!statusline::install_if_needed().unwrap());
    assert!(!statusline::ensure().unwrap(), "EMAKI_HOME is set here, so a launch leaves the settings alone");

    // An older copy of the script on disk reads as not current, and the
    // launch path refreshes it.
    std::fs::write(&script, "#!/bin/bash\necho old\n").unwrap();
    assert_eq!(statusline::state(), State::Emaki { current: false });
    assert!(statusline::install_if_needed().unwrap());
    assert_eq!(statusline::state(), State::Emaki { current: true });

    statusline::restore().unwrap();
    assert_eq!(statusline::state(), State::Other("bash ~/.claude/statusline.sh".into()));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v["model"], "opus");
    assert!(statusline::previous().is_none());
    assert!(statusline::restore().is_err(), "nothing of ours to put back");

    // With no status line before, restore removes the key.
    std::fs::write(&settings, r#"{"model": "opus"}"#).unwrap();
    statusline::install().unwrap();
    assert!(statusline::previous().is_none());
    statusline::restore().unwrap();
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert!(v.get("statusLine").is_none());
    assert_eq!(v["model"], "opus");

    // A settings file that does not parse is left alone.
    std::fs::write(&settings, "{not json").unwrap();
    assert!(statusline::install().is_err());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{not json");
}

/// The script itself, run as Claude Code runs it: one JSON object on stdin.
/// It leaves the windows for the app and prints the terminal's line.
#[cfg(unix)]
#[test]
fn the_status_line_script_leaves_the_windows_for_the_app_and_never_fails() {
    use emaki_core::limits::Limits;
    use emaki_core::statusline;
    use std::io::Write;
    use std::process::{Command, Stdio};
    let (_home, _g) = isolated();
    if Command::new("jq").arg("--version").output().is_err() {
        eprintln!("jq is not installed; skipping");
        return;
    }
    let script = statusline::write_script().unwrap();
    let run = |stdin: &str| {
        let mut child = Command::new("bash")
            .arg(&script)
            .env("EMAKI_HOME", emaki_core::paths::root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "the script must never fail");
        assert!(out.stderr.is_empty(), "the script must never write to stderr");
        String::from_utf8(out.stdout).unwrap()
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    let input = json!({
        "session_id": "abc-123",
        "model": { "id": "claude-opus-5-5[1m]" },
        "effort": { "level": "high" },
        "context_window": { "current_usage": { "input_tokens": 1000, "cache_read_input_tokens": 8000 }, "context_window_size": 100000 },
        "rate_limits": { "five_hour": { "used_percentage": 7.4, "resets_at": now + 13500.0 }, "seven_day": { "used_percentage": 15, "resets_at": now + 5.0 * 86400.0 + 1800.0 } }
    });

    // Before the app has made its state directory the script writes nothing.
    let line = run(&input.to_string());
    assert!(!Limits::statusline_path().exists());
    assert!(line.contains("Context"), "{line}");

    emaki_core::paths::ensure_dirs().unwrap();
    let line = run(&input.to_string());
    let plain: String = strip_ansi(&line);
    assert!(plain.starts_with("Context 9% | 5h: 7% (3h4"), "{plain}");
    // Half an hour past the day boundary, so the seconds the script takes
    // to run cannot tip the reading over to 4d23h.
    assert!(plain.contains("| 7d: 15% (5d0h)"), "{plain}");
    let mut l = Limits::default();
    assert!(l.refresh_from_statusline());
    assert!((l.five_hour.unwrap().utilization - 0.074).abs() < 1e-9);
    assert_eq!(l.seven_day.unwrap().utilization, 0.15);
    assert!(l.seen_at >= now.floor());
    // And the session's own context window, under its id.
    let c = emaki_core::limits::session_context("abc-123").expect("the script leaves the session's context");
    assert_eq!((c.model.as_str(), c.window, c.used), ("claude-opus-5-5[1m]", 100_000, 9_000));
    assert_eq!(c.effort, "high");
    assert!(emaki_core::limits::session_context("another").is_none());
    assert_eq!(emaki_core::limits::prune_contexts(now, 3600.0), 0);
    assert_eq!(emaki_core::limits::prune_contexts(now + 7200.0, 3600.0), 1);
    assert!(emaki_core::limits::session_context("abc-123").is_none());

    // Empty and broken input: a blank-ish line, the file kept, exit 0.
    let before = std::fs::read_to_string(Limits::statusline_path()).unwrap();
    assert_eq!(strip_ansi(&run("")), "Context 0% | 5h: -- | 7d: --\n");
    assert_eq!(strip_ansi(&run("not json")), "Context 0% | 5h: -- | 7d: --\n");
    assert_eq!(std::fs::read_to_string(Limits::statusline_path()).unwrap(), before);
    assert_eq!(std::fs::read_dir(emaki_core::paths::state_dir()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains(".tmp")).count(), 0);
}

#[cfg(unix)]
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for d in chars.by_ref() {
                if d == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
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

#[test]
fn terminal_script_quotes_and_resumes() {
    use emaki_core::terminal::{resume_argv, script, script_path, shell_quote, write_script};
    let (_home, _guard) = isolated();
    assert_eq!(shell_quote("plain-word_1.txt"), "plain-word_1.txt");
    assert_eq!(shell_quote("has space"), "'has space'");
    assert_eq!(shell_quote("it's"), "'it'\\''s'");
    assert_eq!(shell_quote(""), "''");

    std::env::set_var("EMAKI_CLAUDE", "/opt/x/claude");
    let argv = resume_argv(AgentId::ClaudeCode, "abc-123");
    assert_eq!(argv, vec!["/opt/x/claude", "--resume", "abc-123"]);
    std::env::remove_var("EMAKI_CLAUDE");
    assert_eq!(resume_argv(AgentId::Codex, "abc-123"), vec!["codex", "resume", "abc-123"]);

    let text = script("/Users/me/My Project", &argv);
    assert_eq!(text, format!("#!/bin/bash\n{}\ncd '/Users/me/My Project' && exec /opt/x/claude --resume abc-123\n", emaki_core::terminal::UNSET_LINE));
    // The script really clears an inherited session mark, on this machine's bash.
    #[cfg(unix)]
    {
        let probe = script("/", &["/usr/bin/env".to_string()]);
        let probe_path = emaki_core::paths::run_dir().join("probe.sh");
        fs::create_dir_all(probe_path.parent().unwrap()).unwrap();
        fs::write(&probe_path, probe).unwrap();
        let out = std::process::Command::new("/bin/bash")
            .arg(&probe_path)
            .env("CLAUDE_CODE_CHILD_SESSION", "1")
            .env("CLAUDECODE", "1")
            .env("CLAUDE_CONFIG_DIR", "/keep")
            .env("EMAKI_DISABLE", "1")
            .output()
            .unwrap();
        let env = String::from_utf8_lossy(&out.stdout);
        assert!(env.contains("CLAUDE_CONFIG_DIR=/keep"), "{env}");
        assert!(!env.contains("CLAUDE_CODE_CHILD_SESSION"), "{env}");
        assert!(!env.contains("CLAUDECODE="), "{env}");
        assert!(!env.contains("EMAKI_DISABLE"), "{env}");
    }

    let path = write_script("abc-123", "/Users/me/My Project", &argv).unwrap();
    assert_eq!(path, script_path("abc-123"));
    assert!(path.starts_with(emaki_core::paths::run_dir()));
    assert_eq!(path.extension().unwrap(), "command");
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(fs::metadata(&path).unwrap().permissions().mode() & 0o111, 0);
    }
}

/// The hidden terminal's screen, written out, is what the screen readers
/// take: the mode's footer, an empty prompt, a suggestion in dim, the
/// working line with its colour, and a picker cut out for the card.
#[test]
fn hidden_terminal_screen_feeds_the_readers() {
    use emaki_core::{driver, pty};
    let rule = "─".repeat(40);
    let idle = format!("\x1b[38;2;215;119;87m✻\x1b[0m Worked for 2s\r\n{rule}\r\n❯\u{a0}\x1b[2mrun the tests\x1b[0m\r\n{rule}\r\n  ⏵⏵ accept edits on (shift+tab to cycle)");
    let styled = pty::styled(&pty::rows_after(idle.as_bytes(), 12, 60));
    assert_eq!(driver::suggestion_on_screen(&styled).as_deref(), Some("run the tests"));
    assert_eq!(driver::prompt_on_screen(&styled), Some(true));
    assert_eq!(driver::working_on_screen(&styled), None);
    let modes = vec![emaki_core::options::Choice { key: "acceptEdits".into(), label: "Accept edits".into(), ..Default::default() }];
    assert_eq!(driver::mode_on_screen(&pty::plain(&pty::rows_after(idle.as_bytes(), 12, 60)), &modes).as_deref(), Some("acceptEdits"));

    // A session with a name of its own has it on the rule over the prompt.
    let named = format!("{} My session ─\r\n❯\u{a0}\x1b[2mrun the tests\x1b[0m\r\n{rule}\r\n  ⏵⏵ auto mode on", "─".repeat(30));
    let styled_named = pty::styled(&pty::rows_after(named.as_bytes(), 8, 60));
    assert_eq!(driver::prompt_on_screen(&styled_named), Some(true));
    assert_eq!(driver::suggestion_on_screen(&styled_named).as_deref(), Some("run the tests"));

    let typed = format!("{rule}\r\n❯ half a thought\r\n{rule}");
    assert_eq!(driver::prompt_on_screen(&pty::styled(&pty::rows_after(typed.as_bytes(), 8, 60))), Some(false));

    let busy = format!("\x1b[38;2;215;119;87m✳ Brewing…\x1b[0m \x1b[38;2;153;153;153m(3s)\x1b[0m\r\n{rule}\r\n❯ \r\n{rule}");
    let w = driver::working_on_screen(&pty::styled(&pty::rows_after(busy.as_bytes(), 8, 60))).expect("the working line");
    assert_eq!((w.verb.as_str(), w.color), ("Brewing…", Some(0xd77757)));

    // A picker opens under a line of "▔": the card shows it alone.
    let picker = format!("❯ earlier words\r\n\r\n\r\n{}\r\n   Effort\r\n\r\n\r\n   low  medium  high\r\n", "▔".repeat(40));
    let rows = pty::panel_rows(pty::rows_after(picker.as_bytes(), 12, 60));
    assert_eq!(pty::plain(&rows), "   Effort\n\n   low  medium  high");
    // No picker: a dialog is not the prompt.
    assert_eq!(driver::prompt_on_screen(&pty::styled(&pty::rows_after(picker.as_bytes(), 12, 60))), None);
}

/// A click on the hidden terminal's card becomes the keys that do the
/// same in Claude Code's pickers.
#[test]
fn hidden_terminal_clicks_become_keys() {
    use emaki_core::pty;
    let at = |screen: &str, word: &str| -> Option<Vec<u8>> {
        let rows = pty::rows_after(screen.replace('\n', "\r\n").as_bytes(), 16, 100);
        let lines: Vec<String> = pty::plain(&rows).lines().map(str::to_string).collect();
        let said = |h: &pty::Hit| lines[h.row].chars().skip(h.start).take(h.end - h.start).collect::<String>();
        let hits = pty::hits(&rows);
        // A level is its whole word ("high" is not "xhigh"); a row or a hint is found by a part of it.
        if matches!(word, "low" | "high" | "max") {
            return hits.into_iter().find(|h| said(h) == word).map(|h| h.keys);
        }
        hits.into_iter().find(|h| said(h).contains(word)).map(|h| h.keys)
    };
    let effort = "   Effort\n\n      Faster                Smarter\n      ───────────────▲─────────────      Ultracode  off\n      low  medium  high  xhigh  max      Tab to toggle\n\n   ←/→ to adjust · Enter to confirm · s for this session only · Esc to cancel";
    assert_eq!(at(effort, "low"), Some(b"\x1b[D\x1b[D\r".to_vec()));
    assert_eq!(at(effort, "max"), Some(b"\x1b[C\x1b[C\r".to_vec()));
    assert_eq!(at(effort, "high"), Some(b"\r".to_vec()));
    assert_eq!(at(effort, "Tab to toggle"), Some(b"\t".to_vec()));
    assert_eq!(at(effort, "Enter to confirm"), Some(b"\r".to_vec()));
    assert_eq!(at(effort, "session only"), Some(b"s".to_vec()));
    assert_eq!(at(effort, "Esc to cancel"), Some(b"\x1b".to_vec()));
    assert_eq!(at(effort, "adjust"), None);
    assert_eq!(at(effort, "Ultracode"), None);

    // "Type something" is gone to and not confirmed: it is a field.
    let ask = "   Which fruit?\n\n   ❯ 1. Apple\n     2. Banana\n     3. Type something.\n";
    assert_eq!(at(ask, "Banana"), Some(b"\x1b[B\r".to_vec()));
    assert_eq!(at(ask, "Type something"), Some(b"\x1b[B\x1b[B".to_vec()));

    let model = "   Select model\n\n     1.  Default (recommended)  Opus 5.5 · Best for everyday tasks\n   ❯ 2.  Opus 5.5 ✔             For complex work\n     3.  Fable 5.1              For your toughest challenges\n   ↓ 4.  Sonnet 5.5             Most efficient\n\n   Enter to set as default · s to use this session only · Esc to cancel";
    assert_eq!(at(model, "Default"), Some(b"\x1b[A\r".to_vec()));
    assert_eq!(at(model, "Sonnet"), Some(b"\x1b[B\x1b[B\r".to_vec()));
    assert_eq!(at(model, "Opus 5.5 ✔"), Some(b"\r".to_vec()));
    assert_eq!(at(model, "use this session"), Some(b"s".to_vec()));
}

/// `/rename` writes the name as a `custom-title` row and again as an
/// `agent-name` row; it is the title, over any the agent wrote.
#[test]
fn a_name_the_person_gave_is_the_title() {
    use emaki_core::transcript::pick_title;
    let rows = vec![
        json!({"type": "ai-title", "aiTitle": "Embed terminal session headless"}),
        json!({"type": "custom-title", "customTitle": "v0.1.6 - Headless terminal session"}),
        json!({"type": "agent-name", "agentName": "v0.1.6 - Headless terminal session"}),
        json!({"type": "ai-title", "aiTitle": "Something the agent wrote later"}),
    ];
    assert_eq!(pick_title(&rows), "v0.1.6 - Headless terminal session");
    assert_eq!(pick_title(&rows[..1]), "Embed terminal session headless");
}

/// A name asked for is the session's once its transcript says it, and
/// gives way to a name the transcript took from elsewhere meanwhile.
#[test]
fn a_name_asked_for_lands_or_is_overruled() {
    use emaki_core::transcript::{custom_title, naming, Naming};
    let rows = vec![json!({"type": "ai-title", "aiTitle": "Written by the agent"})];
    assert_eq!(custom_title(&rows), "");
    assert_eq!(naming("", "", "Mine"), Naming::Waiting);
    assert_eq!(naming("Old", "Old", "Mine"), Naming::Waiting);
    assert_eq!(naming("Mine", "Old", "Mine"), Naming::Landed);
    assert_eq!(naming("Typed in a terminal", "Old", "Mine"), Naming::Overruled);
    // The old name's row out of the index's reach is not a new name.
    assert_eq!(naming("", "Old", "Mine"), Naming::Waiting);
    // Back to the name it had: nothing to ask for.
    assert_eq!(naming("Old", "Old", "Old"), Naming::Landed);
}

/// `/clear` starts a new transcript and carries the session's name into
/// it. Until something is said there it is not a conversation.
#[test]
fn a_cleared_session_is_blank_until_something_is_said() {
    let _guard = isolated();
    let dir = emaki_core::paths::projects_dir().join("-tmp-blank");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("blank-1.jsonl");
    let mut rows = vec![
        json!({"type": "custom-title", "customTitle": "Named before", "sessionId": "blank-1"}),
        json!({"type": "agent-name", "agentName": "Named before", "sessionId": "blank-1"}),
        json!({"type": "user", "isMeta": true, "uuid": "u0", "cwd": "/tmp/blank", "message": {"role": "user", "content": "<local-command-caveat>x</local-command-caveat>"}}),
        json!({"type": "user", "uuid": "u1", "cwd": "/tmp/blank", "message": {"role": "user", "content": "<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>"}}),
        json!({"type": "system", "subtype": "local_command", "content": "<local-command-stdout></local-command-stdout>", "commandRun": {"command": "clear", "args": ""}}),
    ];
    let write = |rows: &[serde_json::Value]| {
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        std::fs::write(&path, text).unwrap();
    };
    write(&rows);
    let r = emaki_core::transcript::peek(&path);
    assert!(r.blank);
    assert_eq!(r.title, "Named before");

    rows.push(json!({"type": "user", "uuid": "u2", "cwd": "/tmp/blank", "message": {"role": "user", "content": "hello"}}));
    write(&rows);
    assert!(!emaki_core::transcript::peek(&path).blank);

    // A skill is a prompt, though it is written as a command.
    rows.pop();
    rows.push(json!({"type": "user", "uuid": "u3", "cwd": "/tmp/blank", "message": {"role": "user", "content": "<command-name>/code-review</command-name>\n<command-args></command-args>"}}));
    write(&rows);
    assert!(!emaki_core::transcript::peek(&path).blank);
}

/// A command started in the background runs until Claude Code's
/// notification for its id, which is in the queue the moment it exits.
#[test]
fn background_shells_start_and_end() {
    let start = |id: &str, call: &str| {
        json!({"type": "user", "uuid": format!("r-{id}"), "timestamp": "2026-10-06T03:37:44.000Z", "sessionId": "s1",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": call,
                "content": format!("Command running in background with ID: {id}. Output is being written to: /tmp/tasks/{id}.output. You will be notified when it completes.")}]},
            "toolUseResult": {"stdout": "", "backgroundTaskId": id}})
    };
    let call = |call: &str, cmd: &str, what: &str| {
        json!({"type": "assistant", "uuid": format!("a-{call}"), "timestamp": "2026-10-06T03:37:43.000Z", "sessionId": "s1",
            "message": {"role": "assistant", "content": [{"type": "tool_use", "id": call, "name": "Bash", "input": {"command": cmd, "description": what, "run_in_background": true}}]}})
    };
    let rows = vec![
        json!({"type": "user", "uuid": "u1", "timestamp": "2026-10-06T03:37:40.000Z", "sessionId": "s1", "message": {"role": "user", "content": "build it"}}),
        call("t1", "cargo build --release", "Build the release"),
        start("bg1", "t1"),
        call("t2", "sleep 600", "Wait"),
        start("bg2", "t2"),
        json!({"type": "queue-operation", "operation": "enqueue", "timestamp": "2026-10-06T03:39:15.000Z", "sessionId": "s1",
            "content": "<task-notification>\n<task-id>bg1</task-id>\n<status>completed</status>\n<summary>Background command \"Build the release\" completed (exit code 0)</summary>\n</task-notification>"}),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.shells.len(), 2);
    assert_eq!((s.shells[0].id.as_str(), s.shells[0].status.as_str(), s.shells[0].ended.as_str()), ("bg1", "completed", "2026-10-06T03:39:15.000Z"));
    assert_eq!(s.shells[0].description, "Build the release");
    assert_eq!(s.shells[0].output_path, "/tmp/tasks/bg1.output");
    assert!(s.shells[0].summary.contains("exit code 0"));
    assert_eq!((s.shells[1].command.as_str(), s.shells[1].ended.as_str()), ("sleep 600", ""));
}

/// A subagent launched without waiting for it is a background task too:
/// it starts at the `Agent` result that says `isAsync`, ends at the
/// notification for its id, and what it is on is the end of its own
/// transcript.
#[test]
fn background_agents_start_and_end() {
    let launch = |id: &str, call: &str| {
        json!({"type": "user", "uuid": format!("r-{id}"), "timestamp": "2026-10-07T02:27:40.000Z", "sessionId": "s1",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": call, "content": [{"type": "text", "text": "Async agent launched successfully."}]}]},
            "toolUseResult": {"isAsync": true, "status": "async_launched", "agentId": id, "description": "From the sidecar"}})
    };
    let call = |call: &str, what: &str| {
        json!({"type": "assistant", "uuid": format!("a-{call}"), "timestamp": "2026-10-07T02:27:39.000Z", "sessionId": "s1",
            "message": {"role": "assistant", "content": [{"type": "tool_use", "id": call, "name": "Agent", "input": {"description": what, "subagent_type": "general-purpose", "prompt": "do it"}}]}})
    };
    let rows = vec![
        json!({"type": "user", "uuid": "u1", "timestamp": "2026-10-07T02:27:30.000Z", "sessionId": "s1", "message": {"role": "user", "content": "split it"}}),
        call("t1", "Trim window doc"),
        launch("ag1", "t1"),
        call("t2", "Trim channels doc"),
        launch("ag2", "t2"),
        // A foreground agent has a result and no `isAsync`: not a task.
        json!({"type": "user", "uuid": "r-fg", "timestamp": "2026-10-07T02:28:00.000Z", "sessionId": "s1",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t9", "content": "done"}]}, "toolUseResult": {"status": "completed", "agentId": "fg"}}),
        json!({"type": "queue-operation", "operation": "enqueue", "timestamp": "2026-10-07T02:29:48.000Z", "sessionId": "s1",
            "content": "<task-notification>\n<task-id>ag1</task-id>\n<status>completed</status>\n<summary>Agent \"Trim window doc\" finished</summary>\n</task-notification>"}),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(s.shells.len(), 2);
    assert!(s.shells.iter().all(|sh| sh.agent && sh.command == "general-purpose"));
    assert_eq!((s.shells[0].description.as_str(), s.shells[0].status.as_str()), ("Trim window doc", "completed"));
    assert_eq!((s.shells[1].description.as_str(), s.shells[1].ended.as_str()), ("Trim channels doc", ""));

    let dir = tempfile::tempdir().unwrap();
    let transcript = dir.path().join("s1.jsonl");
    let path = emaki_core::build::agent_transcript(transcript.to_str().unwrap(), "ag2");
    assert!(path.ends_with("s1/subagents/agent-ag2.jsonl"));
    assert_eq!(emaki_core::build::agent_step(&path), None);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        json!({"type": "assistant", "cwd": "/repo", "message": {"role": "assistant", "content": [{"type": "text", "text": "\nReading the raw file first.\nThen the doc."}]}}),
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "x", "content": "ok"}]}}),
    ];
    std::fs::write(&path, lines.iter().map(|l| l.to_string() + "\n").collect::<String>()).unwrap();
    assert_eq!(emaki_core::build::agent_step(&path).as_deref(), Some("Reading the raw file first."));
    let more = json!({"type": "assistant", "cwd": "/repo", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "x2", "name": "Read", "input": {"file_path": "/repo/docs/window.md"}}]}});
    std::fs::write(&path, lines.iter().chain([&more]).map(|l| l.to_string() + "\n").collect::<String>()).unwrap();
    let step = emaki_core::build::agent_step(&path).unwrap();
    assert!(step.starts_with("read") && step.contains("window.md"), "{step}");
}

/// "@" in the composer: the folder's files, what answers the words typed,
/// and which "@" in a message names something that is there.
#[test]
fn at_names_files_under_the_folder() {
    use emaki_core::files;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
    for f in ["README.md", "src/main.rs", "src/deep/mainframe.rs", "my file.txt", "node_modules/x/main.js"] {
        std::fs::write(root.join(f), "x").unwrap();
    }
    let cwd = root.to_string_lossy().to_string();
    let all = files::list(&cwd);
    let paths: Vec<&str> = all.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"src/") && paths.contains(&"src/deep/") && paths.contains(&"src/main.rs"));
    assert!(!paths.iter().any(|p| p.starts_with("node_modules")));
    let top: Vec<&str> = files::matches(&all, "", 20).iter().map(|e| e.path.as_str()).collect();
    assert_eq!(top, ["src/", "my file.txt", "README.md"]);
    let hit: Vec<&str> = files::matches(&all, "main", 20).iter().map(|e| e.path.as_str()).collect();
    assert_eq!(hit, ["src/main.rs", "src/deep/mainframe.rs"]);
    let inside: Vec<&str> = files::matches(&all, "src/", 20).iter().map(|e| e.path.as_str()).collect();
    assert_eq!(inside, ["src/deep/", "src/main.rs", "src/deep/mainframe.rs"]);
    assert_eq!(files::matches(&all, "smr", 20)[0].path, "src/main.rs");

    let text = "see @src/main.rs. and (@README.md) not a@b.com or @nope, @\"my file.txt\"";
    let toks: Vec<String> = files::at_tokens(text).into_iter().map(|(_, _, p)| p).collect();
    assert_eq!(toks, ["src/main.rs.", "README.md)", "nope,", "my file.txt"]);
    assert_eq!(files::named(&cwd, "src/main.rs."), Some("src/main.rs"));
    assert_eq!(files::named(&cwd, "nope,"), None);
    assert_eq!(files::mark_mentions(text, &cwd), "see `@src/main.rs`. and (`@README.md`) not a@b.com or @nope, `@\"my file.txt\"`");
    assert_eq!(files::at_token_at("look at @src/ma", 15), Some((8, 15, "src/ma".to_string())));
    assert_eq!(files::at_token_at("mail a@b", 8), None);
    assert_eq!(files::at_token_at("(@RE", 4), Some((1, 4, "RE".to_string())));
    assert_eq!(files::written("my file.txt"), "@\"my file.txt\"");
}

#[test]
fn outline_says_what_was_asked_and_what_came_of_it() {
    let rows = vec![
        user("[Image #1] **fix** the `tabs` please\nthey blink", "2026-10-06T03:00:00.000Z"),
        assistant(vec![json!({"type": "text", "text": "Looking."})], "tool_use", "2026-10-06T03:00:01.000Z"),
        assistant(vec![json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}})], "tool_use", "2026-10-06T03:00:02.000Z"),
        tool_result("t1", "ok", "2026-10-06T03:00:03.000Z"),
        assistant(vec![json!({"type": "text", "text": "## Done\n\nThe tabs no longer blink, see [the note](https://x.y).\n\nMore."})], "end_turn", "2026-10-06T03:00:04.000Z"),
        user("and the sidebar?", "2026-10-06T03:01:00.000Z"),
        assistant(vec![json!({"type": "tool_use", "id": "t2", "name": "Bash", "input": {"command": "ls"}})], "tool_use", "2026-10-06T03:01:02.000Z"),
    ];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s1.jsonl", cwd_hint: "", subagents: None, nested: false });
    let o = emaki_core::outline::of(&s);
    assert_eq!(o.len(), 2);
    assert_eq!((o[0].round, o[0].title.as_str(), o[0].gist.as_str(), o[0].tools), (0, "fix the tabs please they blink", "Done", 1));
    assert_eq!((o[1].title.as_str(), o[1].gist.as_str()), ("and the sidebar?", "1 tool call"));
    assert_eq!(emaki_core::outline::first_line("```\ncode\n```\n---\n- see [the note](https://x.y) now"), "see the note now");
    // A short message is its own line and is not put to a model; a long
    // one is, under a name made from its words; a command never is.
    assert!(!o[0].wants_summary() && o[0].key.is_empty());
    let long = "please look at the sidebar again, the two cards scroll together and I would like each to scroll by itself\nAttached file: /x/a.png";
    let rows = vec![user(long, "2026-10-06T03:00:00.000Z"), user("/compact", "2026-10-06T03:01:00.000Z")];
    let s = build(BuildInput { rows: &rows, transcript_path: "/x/s2.jsonl", cwd_hint: "", subagents: None, nested: false });
    let o = emaki_core::outline::of(&s);
    assert!(o[0].wants_summary() && !o[0].prompt.contains("Attached file") && o[0].key == emaki_core::outline::key_for(&o[0].prompt));
    assert!(!o[1].wants_summary());
    // The child's reply: a line a message behind its number.
    let got = emaki_core::outline::parse_summaries("1: Fix sidebar scrolling\n\n2. \"Rename the tabs.\"", 3);
    assert_eq!(got, vec![Some("Fix sidebar scrolling".to_string()), Some("Rename the tabs".to_string()), None]);
    // A reply that answers the messages is no label at all, numbered
    // list in it or not.
    let answered = "I don't see any project files here. You can:\n1. Copy or clone your project files into the working directory, or\n2. Tell me where they are";
    assert_eq!(emaki_core::outline::parse_summaries(answered, 2), vec![None, None]);
    assert_eq!(emaki_core::outline::parse_summaries("1: Fix it\n2: **If you have code to review**, I can help with that", 2), vec![None, None]);
    assert_eq!(emaki_core::outline::parse_summaries("1: Fix it\n9: out of range", 2), vec![None, None]);
    assert_eq!(emaki_core::outline::parse_summaries("Please run /login", 2), vec![None, None]);
    let argv = emaki_core::outline::summary_argv("", &["one", "two"]);
    assert!(argv.contains(&"claude-haiku-4-5".to_string()) && argv.contains(&"--strict-mcp-config".to_string()));
    assert!(argv.last().unwrap().contains("<message n=\"2\">two</message>"));
}

#[test]
fn git_marks_files_and_the_folders_that_hold_them() {
    use emaki_core::git::{self, State};
    let dir = tempfile::tempdir().unwrap();
    // Not canonical on purpose: on macOS the temp folder is behind a symlink.
    let root = dir.path().to_path_buf();
    let run = |args: &[&str]| {
        let ok = std::process::Command::new("git").arg("-C").arg(&root).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
        assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
    };
    assert!(git::status(&root).is_none(), "a folder in no repository has no status");
    // No checkout by its own files: nothing to explain. One whose `.git`
    // git will not take says why, in git's words; a real one says nothing.
    assert_eq!(git::trouble(&root), None);
    std::fs::create_dir(root.join(".git")).unwrap();
    assert!(git::status(&root).is_none());
    assert!(git::trouble(&root).is_some_and(|t| t.words.contains("not a git repository") && t.fix.is_none()), "{:?}", git::trouble(&root));
    // A command is offered for the Xcode licence alone: Apple's own
    // message on a Mac, and nothing that only mentions Xcode or a licence.
    let apple = "You have not agreed to the Xcode license agreements. Please run 'sudo xcodebuild -license' from within a Terminal window to review and agree to the Xcode and Apple SDKs license.";
    assert!(git::xcode_licence(true, apple));
    assert!(!git::xcode_licence(false, apple));
    assert!(!git::xcode_licence(true, "xcrun: error: invalid active developer path, missing xcrun"));
    assert!(!git::xcode_licence(true, "detected dubious ownership in repository at '/x/license'"));
    std::fs::remove_dir(root.join(".git")).unwrap();
    run(&["init", "-q"]);
    assert_eq!(git::trouble(&root), None);
    assert_eq!(git::base_text(&root.join("nothing.txt")), None, "a file git does not have has nothing to compare with");
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("src/deep/a.rs"), "a").unwrap();
    std::fs::write(root.join("src/b.rs"), "b").unwrap();
    std::fs::write(root.join("docs/gone.md"), "x").unwrap();
    std::fs::write(root.join("same.txt"), "same").unwrap();
    std::fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "first"]);

    std::fs::write(root.join("src/deep/a.rs"), "changed").unwrap();
    std::fs::write(root.join("src/new.rs"), "new").unwrap();
    std::fs::create_dir_all(root.join("fresh/inner")).unwrap();
    std::fs::write(root.join("fresh/inner/f.txt"), "f").unwrap();
    std::fs::create_dir_all(root.join("target/debug")).unwrap();
    std::fs::write(root.join("target/debug/bin"), "x").unwrap();
    std::fs::write(root.join("out.log"), "x").unwrap();
    std::fs::remove_file(root.join("docs/gone.md")).unwrap();
    std::fs::write(root.join("staged.txt"), "s").unwrap();
    run(&["add", "staged.txt"]);

    let st = git::status(&root).unwrap();
    let state = |p: &str, dir: bool| st.mark(&root.join(p), dir).map(|m| (m.state, m.folder));
    assert_eq!(state("src/deep/a.rs", false), Some((State::Modified, false)));
    assert_eq!(state("src/new.rs", false), Some((State::Untracked, false)));
    assert_eq!(state("staged.txt", false), Some((State::Added, false)));
    assert_eq!(state("src/b.rs", false), None);
    assert_eq!(state("same.txt", false), None);
    // A folder wears the most pressing thing under it: new before changed.
    assert_eq!(state("src/deep", true), Some((State::Modified, true)));
    assert_eq!(state("src", true), Some((State::Untracked, true)));
    // A file that is gone still marks the folder it was in.
    assert_eq!(state("docs", true), Some((State::Deleted, true)));
    // Git names an untracked or ignored folder once; what is inside takes its state.
    assert_eq!(state("fresh", true), Some((State::Untracked, true)));
    assert_eq!(state("fresh/inner", true), Some((State::Untracked, true)));
    assert_eq!(state("fresh/inner/f.txt", false), Some((State::Untracked, false)));
    assert_eq!(state("target", true), Some((State::Ignored, true)));
    assert_eq!(state("target/debug/bin", false), Some((State::Ignored, false)));
    assert_eq!(state("out.log", false), Some((State::Ignored, false)));
    assert_eq!(State::Ignored.letter(), None);
    assert_eq!(State::Untracked.letter(), Some("U"));

    // What a commit would be asked about: every file by itself, nothing ignored.
    let changed: Vec<String> = st.changed().iter().map(|(p, _)| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")).collect();
    assert_eq!(changed, ["docs/gone.md", "fresh/inner/f.txt", "src/deep/a.rs", "src/new.rs", "staged.txt"]);
    assert_eq!(st.changed_count(), 5);

    // A changed file against the last commit, a new one as all new lines.
    use emaki_core::git::Row;
    let d = git::diff(&root, &root.join("src/deep/a.rs"), State::Modified);
    assert_eq!((d.added, d.removed), (1, 1));
    assert_eq!(d.rows[1], Row::Changed { left: Some((1, "a".into())), right: Some((1, "changed".into())) });
    let d = git::diff(&root, &root.join("src/new.rs"), State::Untracked);
    assert_eq!(d.rows, [Row::Changed { left: None, right: Some((1, "new".into())) }]);
    let d = git::diff(&root, &root.join("docs/gone.md"), State::Deleted);
    assert_eq!(d.rows[1], Row::Changed { left: Some((1, "x".into())), right: None });
    let d = git::diff(&root, &root.join("staged.txt"), State::Added);
    assert_eq!((d.added, d.removed), (1, 0));

    // Lines taken out and the lines put in after them sit side by side.
    let d = git::parse_diff("diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -3,5 +3,6 @@ fn main() {\n keep\n-one\n-two\n+uno\n+dos\n+tres\n same\n-gone\n\\ No newline at end of file\n");
    assert_eq!(
        d.rows,
        [
            Row::Hunk("@@ -3,5 +3,6 @@ fn main() {".into()),
            Row::Same { old: 3, new: 3, text: "keep".into() },
            Row::Changed { left: Some((4, "one".into())), right: Some((4, "uno".into())) },
            Row::Changed { left: Some((5, "two".into())), right: Some((5, "dos".into())) },
            Row::Changed { left: None, right: Some((6, "tres".into())) },
            Row::Same { old: 6, new: 7, text: "same".into() },
            Row::Changed { left: Some((7, "gone".into())), right: None },
        ]
    );
    assert!(git::parse_diff("diff --git a/p.png b/p.png\nBinary files a/p.png and b/p.png differ\n").binary);

    // A rename is followed by the path it came from, which is not an entry.
    let st = git::parse(std::path::Path::new("/r"), "R  new.txt\0old.txt\0 M old.txt\0UU both.txt\0");
    let at = |p: &str| st.mark(std::path::Path::new(p), false).map(|m| m.state);
    assert_eq!(at("/r/new.txt"), Some(State::Renamed));
    assert_eq!(at("/r/old.txt"), Some(State::Modified));
    assert_eq!(at("/r/both.txt"), Some(State::Conflict));
}

#[test]
fn git_lists_branches_and_switches_between_them() {
    use emaki_core::git;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let run = |at: &std::path::Path, args: &[&str]| {
        let ok = std::process::Command::new("git").arg("-C").arg(at).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"]).args(args).output().unwrap();
        assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
    };
    assert!(git::branches(&root).is_none(), "a folder in no repository has no branches");
    let origin = root.join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    run(&origin, &["init", "-q"]);
    // Before the first commit the branch is there by name alone.
    let b = git::branches(&origin).unwrap();
    assert_eq!((b.current.as_deref(), b.head.as_str(), b.local.as_slice()), (Some("main"), "", &["main".to_string()][..]));
    std::fs::write(origin.join("a.txt"), "one").unwrap();
    run(&origin, &["add", "."]);
    run(&origin, &["commit", "-q", "-m", "first"]);
    run(&origin, &["branch", "theirs"]);

    let work = root.join("work");
    run(&root, &["clone", "-q", "origin", "work"]);
    run(&work, &["branch", "alpha"]);
    run(&work, &["branch", "zeta"]);
    let b = git::branches(&work).unwrap();
    assert_eq!(b.current.as_deref(), Some("main"));
    assert_eq!(b.default.as_deref(), Some("main"));
    // The default first, then by name; a remote's branch only when it is not here too.
    assert_eq!(b.local, ["main", "alpha", "zeta"]);
    assert_eq!(b.remote, ["theirs"]);
    assert_eq!(b.label(), "main");
    assert!(b.when["main"] > 1_600_000_000 && b.when.contains_key("theirs"));
    // By recency the branch committed to last comes straight after the
    // default one, whatever its name.
    run(&work, &["switch", "-q", "zeta"]);
    std::fs::write(work.join("z.txt"), "z").unwrap();
    run(&work, &["add", "."]);
    let later = std::process::Command::new("git").arg("-C").arg(&work).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "later"]).env("GIT_COMMITTER_DATE", "2030-01-01T00:00:00").output().unwrap();
    assert!(later.status.success());
    run(&work, &["switch", "-q", "main"]);
    let recent = git::branches(&work).unwrap().by_recency();
    assert_eq!((recent[0].0.as_str(), recent[1].0.as_str()), ("main", "zeta"));
    assert_eq!(recent.len(), 4);

    git::switch(&work, "alpha").unwrap();
    assert_eq!(git::branches(&work).unwrap().current.as_deref(), Some("alpha"));
    // A branch only the remote has becomes a local one.
    git::switch(&work, "theirs").unwrap();
    let b = git::branches(&work).unwrap();
    assert_eq!(b.current.as_deref(), Some("theirs"));
    assert!(b.remote.is_empty() && b.local.contains(&"theirs".to_string()));
    git::create(&work, "fresh/idea").unwrap();
    assert_eq!(git::branches(&work).unwrap().current.as_deref(), Some("fresh/idea"));
    assert!(git::create(&work, "fresh/idea").is_err(), "a name already taken is refused");
    assert!(git::switch(&work, "no-such-branch").is_err());

    // A change the switch would write over is refused, in git's words, and kept.
    std::fs::write(work.join("a.txt"), "two").unwrap();
    run(&work, &["commit", "-q", "-am", "second"]);
    std::fs::write(work.join("a.txt"), "mine, not committed").unwrap();
    let why = git::switch(&work, "main").unwrap_err();
    assert!(why.contains("overwritten"), "{why}");
    assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "mine, not committed");
    assert_eq!(git::branches(&work).unwrap().current.as_deref(), Some("fresh/idea"));

    // Left behind: the other branch is as committed, and the changes,
    // a new file among them, wait on the branch they were made on.
    std::fs::write(work.join("new.txt"), "untracked").unwrap();
    assert!(git::dirty(&work));
    assert_eq!(git::switch_with(&work, "main", false, git::Carry::Leave), Ok(()));
    assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "one");
    assert!(!work.join("new.txt").exists() && !git::dirty(&work));
    assert_eq!(git::branches(&work).unwrap().stashed, 0, "nothing was left on main");
    assert!(git::restore(&work).is_err());
    git::switch(&work, "fresh/idea").unwrap();
    assert_eq!(git::branches(&work).unwrap().stashed, 1);
    git::restore(&work).unwrap();
    assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "mine, not committed");
    assert_eq!(std::fs::read_to_string(work.join("new.txt")).unwrap(), "untracked");
    assert_eq!(git::branches(&work).unwrap().stashed, 0);

    // Brought along: a change to a file the branches agree on goes by
    // itself, a new file with it.
    std::fs::write(work.join("a.txt"), "two").unwrap();
    assert!(git::misfits(&work, "alpha").is_empty());
    assert_eq!(git::switch_with(&work, "alpha", false, git::Carry::Bring), Ok(()));
    assert_eq!(git::branches(&work).unwrap().current.as_deref(), Some("alpha"));
    assert_eq!(std::fs::read_to_string(work.join("new.txt")).unwrap(), "untracked");
    // A change that would conflict there is seen beforehand, without a
    // file being touched, and the switch is refused whole: same branch,
    // same files, nothing stashed, no conflict left behind.
    std::fs::write(work.join("a.txt"), "mine on alpha").unwrap();
    assert_eq!(git::misfits(&work, "fresh/idea"), ["a.txt"]);
    assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "mine on alpha");
    let why = git::switch_with(&work, "fresh/idea", false, git::Carry::Bring).unwrap_err();
    assert!(why.contains("do not fit") && why.contains("a.txt"), "{why}");
    assert_eq!(git::branches(&work).unwrap().current.as_deref(), Some("alpha"));
    assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "mine on alpha");
    assert_eq!(std::fs::read_to_string(work.join("new.txt")).unwrap(), "untracked");
    assert!(git::unmerged(&work).is_empty());
    let stashes = std::process::Command::new("git").arg("-C").arg(&work).args(["stash", "list"]).output().unwrap();
    assert!(stashes.stdout.is_empty(), "{}", String::from_utf8_lossy(&stashes.stdout));
    // A new file the other branch has under the same name does not fit either.
    run(&work, &["checkout", "-q", "--", "a.txt"]);
    run(&work, &["switch", "-q", "-c", "has-new"]);
    run(&work, &["add", "new.txt"]);
    run(&work, &["commit", "-q", "-m", "new"]);
    run(&work, &["switch", "-q", "alpha"]);
    std::fs::write(work.join("new.txt"), "untracked again").unwrap();
    assert_eq!(git::misfits(&work, "has-new"), ["new.txt"]);
    // With a conflict already open, no switch is tried at all.
    run(&work, &["add", "new.txt"]);
    run(&work, &["commit", "-q", "-m", "mine"]);
    let merged = std::process::Command::new("git").arg("-C").arg(&work).args(["-c", "user.name=t", "-c", "user.email=t@t", "merge", "-q", "has-new"]).output().unwrap();
    assert!(!merged.status.success());
    assert_eq!(git::unmerged(&work), ["new.txt"]);
    let why = git::switch_with(&work, "main", false, git::Carry::Leave).unwrap_err();
    assert!(why.contains("conflict") && why.contains("new.txt"), "{why}");
    run(&work, &["merge", "--abort"]);
    run(&work, &["switch", "-q", "fresh/idea"]);
    // A switch that cannot be made puts the changes back.
    std::fs::write(work.join("a.txt"), "still mine").unwrap();
    assert!(git::switch_with(&work, "no-such-branch", false, git::Carry::Leave).is_err());
    assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "still mine");
    run(&work, &["checkout", "-q", "--", "a.txt"]);
    std::fs::remove_file(work.join("new.txt")).ok();

    // Detached, the place is the commit.
    run(&work, &["checkout", "-q", "--detach"]);
    let b = git::branches(&work).unwrap();
    assert!(b.current.is_none() && !b.head.is_empty() && b.label() == b.head);

    assert!(git::valid_name("v0.1.8-next") && git::valid_name("feature/x"));
    assert!(!git::valid_name("") && !git::valid_name("two words") && !git::valid_name("-b") && !git::valid_name("a..b"));
}

#[test]
fn mouse_reports_as_a_terminal_sends_them() {
    use emaki_core::pty::{mouse_bytes, MouseForm};
    assert_eq!(mouse_bytes(0, 4, 2, true, MouseForm::Sgr), b"\x1b[<0;5;3M");
    assert_eq!(mouse_bytes(0, 4, 2, false, MouseForm::Sgr), b"\x1b[<0;5;3m");
    assert_eq!(mouse_bytes(64, 0, 0, true, MouseForm::Sgr), b"\x1b[<64;1;1M");
    assert_eq!(mouse_bytes(0, 0, 0, true, MouseForm::Bytes), b"\x1b[M !!");
    // Let go is button 3, with the modifiers kept.
    assert_eq!(mouse_bytes(4, 1, 1, false, MouseForm::Bytes), [0x1b, b'[', b'M', 32 + 7, 34, 34]);
    // Past what a byte holds there is nothing to send.
    assert!(mouse_bytes(0, 300, 0, true, MouseForm::Bytes).is_empty());
    assert!(!mouse_bytes(0, 300, 0, true, MouseForm::Utf8).is_empty());
}

#[test]
fn a_sentence_typed_gets_its_capital() {
    use emaki_core::check::{capital, typed, Typed};
    let cap = |text: &str| capital(text, text.len(), true).map(|(r, s)| (r.start, s));
    assert_eq!(cap("h"), Some((0, "H".into())));
    assert_eq!(cap("Done. n"), Some((6, "N".into())));
    assert_eq!(cap("He said \"stop.\" n"), Some((16, "N".into())));
    assert_eq!(cap("first\nn"), Some((6, "N".into())));
    assert_eq!(cap("first n"), None);
    assert_eq!(cap("Done.n"), None);
    assert_eq!(cap("e.g. n"), None);
    assert_eq!(cap("and so on... n"), None);
    assert_eq!(cap("run `cargo. b"), None);
    assert_eq!(cap("```\nl"), None);
    assert_eq!(cap("é"), Some((0, "É".into())));
    assert_eq!(cap("ß"), None);
    // A lone "i", in English only, and not the start of "i.e.".
    assert_eq!(cap("Then i "), Some((5, "I".into())));
    assert_eq!(cap("i'"), Some((0, "I".into())));
    assert_eq!(cap("Hi "), None);
    assert_eq!(cap("Then i."), None);
    assert_eq!(capital("Luego i ", 8, false), None);

    assert_eq!(typed("", "h", 1), Typed::In('h', 1));
    assert_eq!(typed("helo", "hello", 4), Typed::In('l', 4));
    assert_eq!(typed("hello", "hell", 4), Typed::Out(4));
    assert_eq!(typed("hello", "helo", 3), Typed::Out(3));
    assert_eq!(typed("a long message", "h", 1), Typed::Other);
    assert_eq!(typed("", "pasted", 6), Typed::Other);
    // A correction from the menu that adds one letter inside a word
    // leaves the caret at the word's end, past the letter: not a
    // keystroke, and once a panic.
    assert_eq!(typed("mispelled", "misspelled", 10), Typed::Other);
    assert_eq!(typed("a speling b", "a spelling b", 10), Typed::Other);
}

#[test]
fn only_prose_is_checked() {
    use emaki_core::check::{carry, english, keep, Kind};
    let said = |text: &str, british: bool, learned: &[&str]| -> Vec<(String, Kind)> {
        let learned = learned.iter().map(|w| w.to_string()).collect();
        keep(text, english(text, british), &learned, &Default::default()).into_iter().map(|i| (text[i.range].to_string(), i.kind)).collect()
    };
    let text = "I has went to the store and recieve a apple.";
    let found = said(text, false, &[]);
    assert!(found.contains(&("recieve".into(), Kind::Spelling)), "{found:?}");
    assert!(found.contains(&("a".into(), Kind::Grammar)), "{found:?}");
    assert!(found.iter().any(|(w, k)| w == "has" && *k == Kind::Grammar), "{found:?}");
    // Code, paths, commands, names out of code and taught words are not prose.
    let code = "Run `cargo bild` in crates/emaki-app/src/workbench.rs with --relase, see /code-reviw and @AGENTS.md, then HashMap and snake_case in emaki.";
    assert_eq!(said(code, false, &["emaki"]), vec![]);
    assert_eq!(said("Open emaki.", false, &[]).len(), 1);
    // One dialect's spelling is the other's mistake.
    assert!(said("The colour is fine.", false, &[]).iter().any(|(w, _)| w == "colour"));
    assert!(said("The colour is fine.", true, &[]).is_empty());
    assert!(said("The color is fine.", true, &[]).iter().any(|(w, _)| w == "color"));
    // A fix replaces the range it names.
    let issue = keep(text, english(text, false), &Default::default(), &Default::default()).into_iter().find(|i| &text[i.range.clone()] == "recieve").unwrap();
    assert_eq!(issue.fixes.first().map(String::as_str), Some("receive"));
    // Marks follow an edit until the next check.
    let old = "Teh cat and teh dog.";
    let issues = keep(old, english(old, false), &Default::default(), &Default::default());
    assert_eq!(issues.len(), 2, "{issues:?}");
    let new = "Teh big cat and teh dog.";
    let moved = carry(old, new, &issues);
    assert_eq!(moved.iter().map(|i| &new[i.range.clone()]).collect::<Vec<_>>(), vec!["Teh", "teh"]);
    assert_eq!(carry(old, "Tehx cat and teh dog.", &issues).len(), 1);
}

#[test]
fn a_question_with_previews_is_read_in_two_columns() {
    use emaki_core::driver::dialog_on_screen;
    let rule = "─".repeat(96);
    // Off 2.1.294, the pointer on the second choice.
    let screen = format!(
        "{rule}\n ☐ File view\n\nWhere should a file open?\n\n  1. In place of the              ┌──────────────────────────────────────────┐\n    conversation (Recommended)    │ +--------+---------------------+         │\n❯ 2. As its own tab               │ | side   | [chat] [AS ITS OWN  |         │\n  3. Beside the conversation      │ | bar    |         TAB]        |         │\n                                  │ +--------+---------------------+         │\n                                  └──────────────────────────────────────────┘\n\n                                  Notes: press n to add notes\n\n{rule}\n  Chat about this\n\nEnter to select · ↑/↓ to navigate · n to add notes · Esc to cancel\n"
    );
    let d = dialog_on_screen(&screen).unwrap();
    assert_eq!(d.body, vec!["Where should a file open?"]);
    assert_eq!(d.options.iter().map(|o| (o.n, o.label.as_str(), o.detail.as_str(), o.cursor)).collect::<Vec<_>>(), vec![(1, "In place of the conversation (Recommended)", "", false), (2, "As its own tab", "", true), (3, "Beside the conversation", "", false)]);
    assert_eq!(d.preview.unwrap(), vec!["+--------+---------------------+", "| side   | [chat] [AS ITS OWN  |", "| bar    |         TAB]        |", "+--------+---------------------+"]);
}

#[test]
fn a_folder_inside_a_repository_is_asked_about_by_itself() {
    use emaki_core::git::{self, State};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let run = |args: &[&str]| {
        let ok = std::process::Command::new("git").arg("-C").arg(&root).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
        assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
    };
    run(&["init", "-q"]);
    for d in ["app/src", "lib", "target/probe"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::write(root.join(".gitignore"), "/target\n").unwrap();
    std::fs::write(root.join("app/src/main.rs"), "one\n").unwrap();
    std::fs::write(root.join("lib/lib.rs"), "one\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "first"]);
    // A change in each of two folders, and a new file in the ignored one.
    std::fs::write(root.join("app/src/main.rs"), "two\n").unwrap();
    std::fs::write(root.join("lib/lib.rs"), "two\n").unwrap();
    std::fs::write(root.join("lib/new.rs"), "new\n").unwrap();
    std::fs::write(root.join("target/probe/out.txt"), "x\n").unwrap();

    let whole = git::status(&root).unwrap();
    assert_eq!(whole.changed_count(), 3);
    // Part way down: the branch is the repository's, the changes are the folder's.
    let app = git::status(&root.join("app")).unwrap();
    assert_eq!(app.changed(), vec![(root.join("app/src/main.rs"), State::Modified)]);
    assert!(app.dirty);
    assert!(git::branches(&root.join("app")).is_some());
    assert_eq!(git::status(&root.join("lib")).unwrap().changed_count(), 2);
    // A clean folder beside changed ones counts none, and a switch still asks.
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/a.md"), "a\n").unwrap();
    run(&["add", "docs"]);
    run(&["commit", "-q", "-m", "docs", "--", "docs"]);
    let docs = git::status(&root.join("docs")).unwrap();
    assert_eq!(docs.changed_count(), 0);
    assert!(docs.dirty, "the repository has changes outside the folder");
    // A folder the repository ignores is in no repository.
    assert!(git::status(&root.join("target/probe")).is_none());
    assert!(git::branches(&root.join("target/probe")).is_none());
    assert!(git::status(&root.join("target")).is_none());
}

#[test]
fn a_table_is_read_with_its_quotes() {
    use emaki_core::files::table;
    let text = "name,note\r\n\"Hu, Pingfan\",\"said \"\"hi\"\"\nthen left\"\n\nlast,\n";
    let (rows, more) = table(text, ',', 10);
    assert_eq!(rows, vec![vec!["name", "note"], vec!["Hu, Pingfan", "said \"hi\"\nthen left"], vec!["last", ""]]);
    assert!(!more);
    let (rows, more) = table("a\tb\n1\t2\n3\t4\n", '\t', 2);
    assert_eq!(rows.len(), 2);
    assert!(more);
}

#[test]
fn a_mark_is_for_what_is_wrong_however_it_is_read() {
    use emaki_core::check::{english, ignore_key, keep};
    let marked = |text: &str, ignored: &[String]| -> Vec<String> {
        let ignored = ignored.iter().cloned().collect();
        keep(text, english(text, false), &Default::default(), &ignored).into_iter().map(|i| text[i.range].to_string()).collect()
    };
    // A guess at a part of speech, and one accepted way of writing over another.
    assert_eq!(marked("It is to the right of the file system; the single-click effect triggers and I opened it.", &[]), Vec::<String>::new());
    assert_eq!(marked("We set up the work flow on the back end of the web site.", &[]), Vec::<String>::new());
    // What is wrong however it is read is still marked.
    let found = marked("I could of done it, and he have a apple.", &[]);
    for wrong in ["could of", "have", "a"] {
        assert!(found.iter().any(|w| w == wrong), "{wrong} in {found:?}");
    }
    // A mark the person ignored is not made again, for that rule and those words.
    let text = "He have a plan.";
    let issue = keep(text, english(text, false), &Default::default(), &Default::default()).into_iter().find(|i| &text[i.range.clone()] == "have").unwrap();
    assert!(!marked(text, &[ignore_key(&issue.rule, "have")]).contains(&"have".to_string()));
}

#[test]
fn a_new_line_starts_where_the_language_says() {
    use emaki_core::indent::{after, unit};
    // Python: a step in after a colon, a step out after what ends a
    // block, and the same place otherwise.
    assert_eq!(after("python", "def f(x):", "", "    "), "    ");
    assert_eq!(after("python", "    if x:  # why", "def f(x):", "    "), "        ");
    assert_eq!(after("python", "        return x", "    if x:", "    "), "    ");
    assert_eq!(after("python", "    y = 1", "def f(x):", "    "), "    ");
    assert_eq!(after("python", "    url = 'http://a'", "", "    "), "    ");
    assert_eq!(after("python", "    d = {", "", "    "), "        ");
    // R: a step in after an opening brace, and after a pipe or a `+`
    // left open, once: the lines of the chain stay level.
    assert_eq!(after("r", "f <- function(x) {", "", "  "), "  ");
    assert_eq!(after("r", "df %>%", "x <- 1", "  "), "  ");
    assert_eq!(after("r", "  filter(a > 1) %>%", "df %>%", "  "), "  ");
    assert_eq!(after("r", "ggplot(df) +", "", "  "), "  ");
    assert_eq!(after("r", "  x <- 1", "f <- function(x) {", "  "), "  ");
    // Anything else: the bracket rule and no more.
    assert_eq!(after("rust", "fn main() {", "", "    "), "    ");
    assert_eq!(after("rust", "    let x = 1;", "", "    "), "    ");
    assert_eq!(after("yaml", "jobs:", "", "  "), "  ");
    assert_eq!(after("text", "a line: ", "", "  "), "");
    // A step is the file's own when it shows one, else the language's.
    assert_eq!(unit("python", "x = 1\n"), (4, false));
    assert_eq!(unit("python", "def f():\n  return 1\n"), (2, false));
    assert_eq!(unit("r", "x <- 1\n"), (2, false));
    assert_eq!(unit("go", "package main\n"), (4, true));
    assert_eq!(unit("c", "int main() {\n\treturn 0;\n}\n"), (4, true));
}

#[test]
fn an_r_file_is_formatted_as_air_formats_it() {
    use emaki_core::format::format;
    let messy = "f<-function(x,y){\nif(x>1){y=x+1}\n      else {y=2}\nreturn(y)}\n";
    let tidy = format("r", messy).unwrap().expect("a messy file changes");
    assert_eq!(tidy, "f <- function(x, y) {\n  if (x > 1) {\n    y <- x + 1\n  } else {\n    y <- 2\n  }\n  return(y)\n}\n");
    // The parser is Air's on a newer grammar than Air pins
    // (vendor/air_r_parser): a file with most of the language in it
    // still parses, formats, and formats to itself.
    let wide = "library(dplyr)\n# a comment\ndf%>%filter(a>1,b%in%c('x',\"y\"))%>%mutate(z=a^2,w=-a)|>summarise(n=n())\nm<-lm(y~x+I(x^2),data=df)\ng<-function(x=1L,...,na.rm=TRUE)x[[1]]$name@slot\nfor(i in 1:10){if(i%%2==0)next else print(i)}\nwhile(TRUE){break}\nh<-\\(x)x+1\nl<-list(a=1,`b c`=NULL,d=1e-3,e=0x1F,f=2i,g=r\"(raw)\")\nrepeat{break}\nx[1,,drop=FALSE]\nif(is.na(x)||!ok&&TRUE)stop('no')else NULL\n";
    let once = format("r", wide).unwrap().expect("it changes");
    assert!(once.contains("df %>%\n  filter(a > 1, b %in% c('x', \"y\")) %>%"), "{once}");
    assert_eq!(format("r", &once), Ok(None), "{once}");
    // What is already in form is left alone, and so is what is not R.
    assert_eq!(format("r", &tidy), Ok(None));
    assert_eq!(format("python", "x=1\n"), Ok(None));
    // A file that does not parse is not touched; the reason comes back.
    assert!(format("r", "f <- function( {\n").is_err());
}

#[test]
fn a_notebook_reads_as_markdown() {
    let nb = json!({"metadata": {"kernelspec": {"language": "python"}}, "cells": [
        {"cell_type": "markdown", "source": ["# Title\n", "Some words."]},
        {"cell_type": "code", "execution_count": 3, "source": "print('hi')\n", "outputs": [
            {"output_type": "stream", "text": ["hi\n"]},
            {"output_type": "execute_result", "data": {"text/plain": ["42"]}},
            {"output_type": "display_data", "data": {"image/png": "AAAA", "text/plain": "<Figure>"}}]},
        {"cell_type": "code", "execution_count": null, "source": [], "outputs": []}]});
    let md = emaki_core::files::notebook_markdown(&nb.to_string()).unwrap();
    assert!(md.starts_with("# Title\nSome words.\n\n**In [3]**\n\n```python\nprint('hi')\n```\n\n```text\nhi\n```\n\n```text\n42\n```\n\n*A picture."), "{md}");
    assert!(emaki_core::files::notebook_markdown("{\"a\": 1}").is_none());
    assert!(emaki_core::files::notebook_markdown("not json").is_none());
}

#[test]
fn changed_lines_are_marked_against_what_git_has() {
    use emaki_core::git::{self, line_marks, LineChange::*, LineMark};
    let base = "one\ntwo\nthree\nfour\nfive\n";
    assert_eq!(line_marks(base, base), vec![]);
    // Two lines put in after the first: lines 1 and 2 are new.
    assert_eq!(line_marks(base, "one\n\nnew\ntwo\nthree\nfour\nfive\n"), vec![LineMark { line: 1, lines: 2, kind: Added }]);
    // A line changed, and one taken out further down: the place it was
    // taken from is the top of the line now there.
    assert_eq!(line_marks(base, "one\nTWO\nthree\nfive\n"), vec![LineMark { line: 1, lines: 1, kind: Modified }, LineMark { line: 3, lines: 0, kind: Deleted }]);
    // Lines that stand where fewer stood: as many are changed as there
    // were, and the rest are new. Where more stood, the rest were taken
    // out, after the changed ones.
    assert_eq!(line_marks("a\n\nb\n", "A\n\nnew\n\nb\n"), vec![LineMark { line: 0, lines: 1, kind: Modified }, LineMark { line: 1, lines: 2, kind: Added }]);
    assert_eq!(line_marks("a\nb\nc\nd\n", "X\nd\n"), vec![LineMark { line: 0, lines: 1, kind: Modified }, LineMark { line: 1, lines: 0, kind: Deleted }]);
    // The last line taken out: the place is past the end.
    assert_eq!(line_marks(base, "one\ntwo\nthree\nfour\n"), vec![LineMark { line: 4, lines: 0, kind: Deleted }]);
    // A place is the lines on both sides, and those lines can be cut
    // out whole: what a revert puts back, and where.
    let now = "one\nTWO\nthree\nfive\n";
    let hunks = git::line_hunks(base, now);
    assert_eq!(hunks, vec![git::Hunk { old: 1..2, new: 1..2 }, git::Hunk { old: 3..4, new: 3..3 }]);
    assert_eq!(git::lines_of(base, &hunks[0].old), (4..8, "two\n".to_string()));
    assert_eq!(git::lines_of(now, &hunks[1].new), (14..14, String::new()));
    assert_eq!(git::lines_of(base, &hunks[1].old).1, "four\n");
    assert_eq!(git::lines_of("a\nb", &(1..2)), (2..3, "b".to_string()));
    assert_eq!(git::lines_of("a\n", &(5..6)), (2..2, String::new()));
    // What git has is the index: the commit's, then what is staged.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let run = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&root).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap().status.success());
    run(&["init", "-q"]);
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/a.txt"), base).unwrap();
    assert_eq!(git::base_text(&root.join("sub/a.txt")), None);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "one"]);
    std::fs::write(root.join("sub/a.txt"), "changed\n").unwrap();
    assert_eq!(git::base_text(&root.join("sub/a.txt")).as_deref(), Some(base));
    run(&["add", "."]);
    assert_eq!(git::base_text(&root.join("sub/a.txt")).as_deref(), Some("changed\n"));
}

#[test]
fn a_sentence_left_small_is_marked() {
    use emaki_core::check::{capitals, keep, Kind};
    let found = |text: &str| capitals(text).into_iter().map(|i| (text[i.range.clone()].to_string(), i.fixes)).collect::<Vec<_>>();
    // The start of the message, of a line, and after a full stop: each
    // word left small is marked, and the capital is what is offered.
    assert_eq!(found("solved. Now let's move on."), vec![("solved".to_string(), vec!["Solved".to_string()])]);
    assert_eq!(found("Done. now this.\nand this"), vec![("now".to_string(), vec!["Now".to_string()]), ("and".to_string(), vec!["And".to_string()])]);
    // Not where a sentence goes on, not in code, and not a word that
    // already has its capital.
    assert_eq!(found("Use e.g. apples, i.e. fruit... and so on"), vec![]);
    assert_eq!(found("Run `cargo build`. Then `x`."), vec![]);
    assert_eq!(found("```\nlet x = 1;\n```\n"), vec![]);
    assert_eq!(found("- one\n- two"), vec![]);
    // It is a grammar mark with a rule of its own, so Ignore keeps it
    // away, and a command or a path at a line's start is not prose.
    let all = capitals("solved. Now");
    assert!(all[0].kind == Kind::Grammar && all[0].rule == "SentenceCapital");
    let ignored: std::collections::HashSet<String> = [emaki_core::check::ignore_key("SentenceCapital", "solved")].into();
    assert!(keep("solved. Now", capitals("solved. Now"), &Default::default(), &ignored).is_empty());
    assert!(keep("src/main.rs is the file", capitals("src/main.rs is the file"), &Default::default(), &Default::default()).is_empty());
}

/// A `cd` in a command moves the `cwd` of every row after it. The
/// session's folder stays the one it was started in, which is where the
/// index files it and where it is resumed from.
#[test]
fn a_session_keeps_the_folder_it_started_in() {
    let _home = isolated();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sess.jsonl");
    let rows = [
        json!({"type": "user", "sessionId": "sess", "cwd": "/work/papers", "timestamp": "2026-01-01T00:00:00Z", "message": {"role": "user", "content": "hello"}}),
        json!({"type": "assistant", "sessionId": "sess", "cwd": "/work/papers/one/erl", "timestamp": "2026-01-01T00:00:01Z", "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}]}}),
    ];
    std::fs::write(&path, rows.iter().map(|r| r.to_string()).collect::<Vec<_>>().join("\n") + "\n").unwrap();
    assert_eq!(emaki_core::transcript::peek(&path).cwd, "/work/papers");
    let session = build(BuildInput { rows: &rows, transcript_path: "/x/sess.jsonl", cwd_hint: "", subagents: None, nested: false });
    assert_eq!(session.cwd, "/work/papers");
}

/// A program in a terminal copies by asking the terminal to (OSC 52).
/// The sequence may arrive in pieces, ended by BEL or by `ESC \\`.
#[test]
fn a_terminal_copy_is_read_whole() {
    use emaki_core::pty::take_osc52;
    let mut pending = b"before \x1b]52;c;aGVsbG8gd29ybGQ=\x07 after".to_vec();
    assert_eq!(take_osc52(&mut pending).as_deref(), Some("hello world"));
    assert_eq!(take_osc52(&mut pending), None);
    // In two reads, with the other ending.
    let mut pending = b"x\x1b]5".to_vec();
    assert_eq!(take_osc52(&mut pending), None);
    pending.extend_from_slice(b"2;c;5L2g5aW9");
    assert_eq!(take_osc52(&mut pending), None);
    pending.extend_from_slice(b"\x1b\\");
    assert_eq!(take_osc52(&mut pending).as_deref(), Some("\u{4f60}\u{597d}"));
    // A question about the clipboard is not a copy.
    let mut pending = b"\x1b]52;c;?\x07".to_vec();
    assert_eq!(take_osc52(&mut pending), None);
}

/// The same through a terminal of ours: what a program there asks to
/// have copied is kept for the window to take, once.
#[cfg(unix)]
#[test]
fn a_program_in_our_terminal_copies() {
    use emaki_core::pty::Pty;
    let argv: Vec<String> = ["/bin/sh", "-c", "printf '\\033]52;c;aGk=\\007'; sleep 1"].iter().map(|s| s.to_string()).collect();
    let pty = Pty::spawn(&argv, "/", std::sync::Arc::new(|| {})).expect("a pty");
    let mut copied = None;
    for _ in 0..100 {
        copied = pty.take_copied();
        if copied.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(copied.as_deref(), Some("hi"));
    assert_eq!(pty.take_copied(), None);
}

/// An Office file is a zip of XML parts: one made here, part by part.
fn office_zip(path: &std::path::Path, parts: &[(&str, &str)]) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, body) in parts {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

const OFFICE_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="MAIN"/></Relationships>"#;

/// A Word document comes out as markdown: headings by the style's name
/// and not its id, which is in the document's language; a list by what
/// its numbering says it is; a table; bold and italic; and a text box
/// once, though the file holds it twice.
#[test]
fn a_word_file_reads_as_markdown() {
    use emaki_core::office::{read, Office};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.docx");
    let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
    let styles = format!(r#"<w:styles {w}><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/></w:style><w:style w:type="paragraph" w:styleId="Titre2"><w:name w:val="heading 2"/></w:style></w:styles>"#);
    let numbering = format!(
        r#"<w:numbering {w}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
    );
    let item = |list: u8, level: u8, text: &str| format!(r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="{list}"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#);
    let cell = |text: &str| format!("<w:tc><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>");
    let body = [
        r#"<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>Annual report</w:t></w:r></w:p>"#.to_string(),
        r#"<w:p><w:pPr><w:pStyle w:val="Titre2"/></w:pPr><w:r><w:t>Sales</w:t></w:r></w:p>"#.to_string(),
        // The paragraph's own mark is bold, which is not its text's.
        r#"<w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs><w:rPr><w:b/></w:rPr></w:pPr><w:r><w:t xml:space="preserve">Up </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>a lot</w:t></w:r><w:r><w:rPr><w:b w:val="0"/><w:i/></w:rPr><w:t xml:space="preserve"> this</w:t></w:r><w:r><w:t xml:space="preserve"> year, R&amp;D too.</w:t><w:tab/><w:t>end</w:t><w:br/><w:t>next line</w:t></w:r></w:p>"#.to_string(),
        "<w:p/>".to_string(),
        item(1, 0, "apples"),
        item(1, 1, "green ones"),
        item(1, 0, "pears"),
        item(2, 0, "first"),
        item(2, 0, "second"),
        format!("<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>", cell("Region"), cell("Total"), cell("North | South"), cell("12")),
        r#"<w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:txbxContent><w:p><w:r><w:t>In a box</w:t></w:r></w:p></w:txbxContent></mc:Choice><mc:Fallback><w:pict><w:txbxContent><w:p><w:r><w:t>In a box</w:t></w:r></w:p></w:txbxContent></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>"#.to_string(),
    ]
    .concat();
    let document = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {w}><w:body>{body}<w:sectPr/></w:body></w:document>"#);
    office_zip(&path, &[("_rels/.rels", &OFFICE_RELS.replace("MAIN", "word/document.xml")), ("word/document.xml", &document), ("word/styles.xml", &styles), ("word/numbering.xml", &numbering)]);

    let Ok(Office::Markdown(text)) = read(&path, 100) else { panic!("a document is markdown") };
    let want = [
        "# Annual report",
        "",
        "## Sales",
        "",
        "Up **a lot** *this* year, R&D too.\tend  ",
        "next line",
        "",
        "- apples",
        "    - green ones",
        "- pears",
        "1. first",
        "2. second",
        "",
        "| Region | Total |",
        "| --- | --- |",
        "| North \\| South | 12 |",
        "",
        "In a box",
    ]
    .join("\n");
    assert_eq!(text, want);
}

/// A deck comes out slide by slide, in the order the presentation shows
/// them and not the order of the files' numbers, each under its title.
#[test]
fn a_deck_reads_slide_by_slide() {
    use emaki_core::office::{read, Office};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("talk.pptx");
    let ns = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    let presentation = format!(r#"<p:presentation {ns}><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#);
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="x/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="x/slide" Target="slides/slide1.xml"/><Relationship Id="rId3" Type="x/slide" Target="/ppt/slides/slide2.xml"/></Relationships>"#;
    let shape = |ph: &str, paras: &str| format!("<p:sp><p:nvSpPr><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr><a:buNone/></a:lvl1pPr></a:lstStyle>{paras}</p:txBody></p:sp>");
    let para = |props: &str, text: &str| format!("<a:p>{props}<a:r><a:rPr lang=\"en\"/><a:t>{text}</a:t></a:r></a:p>");
    let cell = |text: &str| format!("<a:tc><a:txBody>{}</a:txBody></a:tc>", para("", text));
    let opening = [
        shape(r#"<p:ph type="body" idx="1"/>"#, &[para("", "Why now"), para(r#"<a:pPr lvl="1"/>"#, "Costs fell"), para(r#"<a:pPr><a:buNone/></a:pPr>"#, "A closing line")].concat()),
        shape(r#"<p:ph type="title"/>"#, "<a:p><a:r><a:t>The </a:t></a:r><a:r><a:t>opening</a:t></a:r></a:p>"),
        shape(r#"<p:ph type="sldNum" idx="12"/>"#, "<a:p><a:fld type=\"slidenum\"><a:t>1</a:t></a:fld></a:p>"),
    ]
    .concat();
    let closing = [
        shape("", &[para("", "A note in a box"), para(r#"<a:pPr><a:buAutoNum type="arabicPeriod"/></a:pPr>"#, "one"), para(r#"<a:pPr><a:buAutoNum type="arabicPeriod"/></a:pPr>"#, "two")].concat()),
        format!("<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr>{}{}</a:tr><a:tr>{}{}</a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>", cell("Year"), cell("Users"), cell("2025"), cell("40")),
    ]
    .concat();
    let slide = |shapes: &str| format!(r#"<p:sld {ns}><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>"#);
    office_zip(
        &path,
        &[
            ("_rels/.rels", &OFFICE_RELS.replace("MAIN", "ppt/presentation.xml")),
            ("ppt/presentation.xml", &presentation),
            ("ppt/_rels/presentation.xml.rels", rels),
            ("ppt/slides/slide1.xml", &slide(&closing)),
            ("ppt/slides/slide2.xml", &slide(&opening)),
        ],
    );

    let Ok(Office::Markdown(text)) = read(&path, 100) else { panic!("a deck is markdown") };
    let want = [
        "## 1. The opening",
        "",
        "- Why now",
        "    - Costs fell",
        "",
        "A closing line",
        "",
        "## Slide 2",
        "",
        "A note in a box",
        "",
        "1. one",
        "2. two",
        "",
        "| Year | Users |",
        "| --- | --- |",
        "| 2025 | 40 |",
    ]
    .join("\n");
    assert_eq!(text, want);

    // A deck with no slides yet is an empty one, not a file refused; and
    // with no list of slides to go by, the files' numbers are the order.
    let empty = dir.path().join("template.pptx");
    office_zip(&empty, &[("ppt/presentation.xml", &format!("<p:presentation {ns}/>"))]);
    assert_eq!(read(&empty, 100), Ok(Office::Markdown(String::new())));
    let unlisted = dir.path().join("unlisted.pptx");
    let titled = |title: &str| slide(&shape(r#"<p:ph type="ctrTitle"/>"#, &para("", title)));
    office_zip(&unlisted, &[("ppt/slides/slide10.xml", &titled("Last")), ("ppt/slides/slide2.xml", &titled("First"))]);
    assert_eq!(read(&unlisted, 100), Ok(Office::Markdown("## 1. First\n\n## 2. Last".into())));
}

/// A workbook comes out as its sheets' rows: a cell where the sheet has
/// it, a whole number with no ".0", a date as ISO, and no more rows than
/// were asked for. A hidden sheet is not among them.
#[test]
fn a_workbook_reads_as_rows() {
    use emaki_core::office::{read, Office, Sheet};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.xlsx");
    let main = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    let types = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet3.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;
    let workbook = format!(r#"<workbook {main}><sheets><sheet name="Totals" sheetId="1" r:id="rId1"/><sheet name="Secret" sheetId="2" state="hidden" r:id="rId2"/><sheet name="Long" sheetId="3" r:id="rId3"/></sheets></workbook>"#);
    let sheet_type = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{sheet_type}" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{sheet_type}" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="{sheet_type}" Target="worksheets/sheet3.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
    );
    // The second cell format is a date's (number format 14).
    let styles = format!(r#"<styleSheet {main}><fonts count="1"><font/></fonts><fills count="1"><fill/></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14" applyNumberFormat="1"/></cellXfs></styleSheet>"#);
    let text = |at: &str, s: &str| format!(r#"<c r="{at}" t="inlineStr"><is><t>{s}</t></is></c>"#);
    // It begins at B2, and its last row and last column hold nothing.
    let totals = format!(
        r#"<worksheet {main}><sheetData><row r="2">{}{}{}</row><row r="3">{}<c r="C3"><v>3</v></c><c r="D3"><v>2.5</v></c></row><row r="4"><c r="B4" t="b"><v>1</v></c><c r="C4" t="e"><v>#DIV/0!</v></c><c r="D4" s="1"><v>45943</v></c><c r="E4" s="1"><v>45943.5</v></c></row><row r="5">{}</row></sheetData></worksheet>"#,
        text("B2", "Item"),
        text("C2", "Count"),
        text("D2", "Price"),
        text("B3", "Tea &amp; milk"),
        text("F5", ""),
    );
    let secret = format!(r#"<worksheet {main}><sheetData><row r="1">{}</row></sheetData></worksheet>"#, text("A1", "not shown"));
    let long: String = (1..=5).map(|n| format!(r#"<row r="{n}"><c r="A{n}"><v>{n}</v></c></row>"#)).collect();
    let long = format!(r#"<worksheet {main}><sheetData>{long}</sheetData></worksheet>"#);
    office_zip(
        &path,
        &[
            ("[Content_Types].xml", types),
            ("_rels/.rels", &OFFICE_RELS.replace("MAIN", "xl/workbook.xml")),
            ("xl/workbook.xml", &workbook),
            ("xl/_rels/workbook.xml.rels", &rels),
            ("xl/styles.xml", &styles),
            ("xl/worksheets/sheet1.xml", &totals),
            ("xl/worksheets/sheet2.xml", &secret),
            ("xl/worksheets/sheet3.xml", &long),
        ],
    );

    let Ok(Office::Sheets(sheets)) = read(&path, 3) else { panic!("a workbook is sheets") };
    let row = |cells: &[&str]| cells.iter().map(|c| c.to_string()).collect::<Vec<String>>();
    assert_eq!(sheets.len(), 2);
    assert_eq!(sheets[0].name, "Totals");
    assert!(sheets[0].more, "the sheet has a fourth row");
    assert_eq!(sheets[0].rows, vec![row(&["", "", "", ""]), row(&["", "Item", "Count", "Price"]), row(&["", "Tea & milk", "3", "2.5"])]);
    assert_eq!(sheets[1], Sheet { name: "Long".into(), rows: vec![row(&["1"]), row(&["2"]), row(&["3"])], more: true });

    let Ok(Office::Sheets(sheets)) = read(&path, 100) else { panic!("a workbook is sheets") };
    assert!(!sheets[0].more);
    assert_eq!(sheets[0].rows.len(), 4, "the row that holds nothing is not one");
    assert_eq!(sheets[0].rows[3], row(&["", "TRUE", "#DIV/0!", "2025-10-13", "2025-10-13 12:00:00"]));
    assert_eq!(sheets[0].rows[1], row(&["", "Item", "Count", "Price", ""]), "every row is as wide as the widest");
}

/// What is not an Office file, or cannot be opened, is refused with a
/// reason; the old binary document and deck are not offered at all.
#[test]
fn an_office_file_that_cannot_be_read_says_why() {
    use emaki_core::office::{read, reads};
    let dir = tempfile::tempdir().unwrap();
    for (name, why) in [("a.docx", "it is not a Word file"), ("a.pptx", "it is not a PowerPoint file"), ("a.xlsx", "it is not an Excel file"), ("a.xls", "it is not an Excel file")] {
        let path = dir.path().join(name);
        fs::write(&path, "plain text under an Office name").unwrap();
        assert_eq!(read(&path, 10).err().as_deref(), Some(why));
    }
    // A zip that is no document.
    let path = dir.path().join("other.docx");
    office_zip(&path, &[("readme.txt", "hello")]);
    assert_eq!(read(&path, 10).err().as_deref(), Some("it is not a Word file"));
    // Office keeps a file with a password in its old binary container,
    // the encrypted zip under this name.
    let mut locked = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    locked.extend(vec![0u8; 512]);
    locked.extend("EncryptedPackage".encode_utf16().flat_map(u16::to_le_bytes));
    for name in ["locked.docx", "locked.xlsx", "locked.pptx"] {
        let path = dir.path().join(name);
        fs::write(&path, &locked).unwrap();
        assert_eq!(read(&path, 10).err().as_deref(), Some("it is protected by a password"));
    }
    assert_eq!(read(&dir.path().join("gone.docx"), 10).err().as_deref(), Some("it could not be read"));
    assert!(["docx", "pptx", "xlsx", "xlsm", "xls"].iter().all(|ext| reads(ext)));
    assert!(!reads("doc") && !reads("ppt") && !reads("pdf"));
}

/// A cell of a table is found where it is written, so one cell can be
/// written over and the rest of the file left as it is.
#[test]
fn a_cell_is_found_where_it_is_written() {
    use emaki_core::files::{cell_at, cell_value, cell_written, table, CellAt};
    let text = "\u{feff}\"name\",\"says\",n\r\nann,\"hi, \"\"you\"\"\",1\r\n\r\nbob,\"two\nlines\"\r\nlast,,3";
    let (rows, _) = table(text, ',', 100);
    assert_eq!(rows.len(), 4, "a line with nothing on it is no row");
    // Every cell the table gives is the one found at its place.
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            match cell_at(text, ',', r, c) {
                Some(CellAt::At(span)) => assert_eq!(&cell_value(&text[span]), cell, "row {r} column {c}"),
                other => panic!("row {r} column {c}: {other:?}"),
            }
        }
    }
    // A row that ends before the column says where it ends and how long it is.
    let short = cell_at(text, ',', 2, 2);
    assert!(matches!(short, Some(CellAt::Short { cells: 2, .. })), "{short:?}");
    if let Some(CellAt::Short { end, .. }) = short {
        assert!(text[..end].ends_with("lines\""));
    }
    assert_eq!(cell_at(text, ',', 9, 0), None);
    // Written over, the cell is the only thing that changes, and the line end stays.
    let Some(CellAt::At(span)) = cell_at(text, ',', 1, 2) else { panic!() };
    let mut changed = text.to_string();
    changed.replace_range(span, &cell_written("a, b", ',', false));
    assert_eq!(changed, text.replace(",1\r\n", ",\"a, b\"\r\n"));
    assert_eq!(table(&changed, ',', 100).0[1][2], "a, b");
    // Quotes are kept where they were, and put where they are needed.
    assert_eq!(cell_written("x", ',', true), "\"x\"");
    assert_eq!(cell_written("x", ',', false), "x");
    assert_eq!(cell_written("say \"x\"", ',', false), "\"say \"\"x\"\"\"");
    assert_eq!(cell_written("a\tb", '\t', false), "\"a\tb\"");
}

/// A copy takes a free name beside what is there, a folder goes with
/// all it holds, and nothing is written over.
#[test]
fn a_copy_takes_a_free_name() {
    use emaki_core::files::{copy_into, copy_name};
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    fs::write(dir.join("notes.md"), "one").unwrap();
    fs::create_dir_all(dir.join("src/deep")).unwrap();
    fs::write(dir.join("src/deep/a.rs"), "fn a() {}").unwrap();
    fs::write(dir.join(".env"), "x").unwrap();
    assert_eq!(copy_name(&dir.join("notes.md"), &dir.join("src")), dir.join("src/notes.md"));
    assert_eq!(copy_into(&dir.join("notes.md"), dir).unwrap(), dir.join("notes copy.md"));
    assert_eq!(copy_into(&dir.join("notes.md"), dir).unwrap(), dir.join("notes copy 2.md"));
    // A name that is all "kind" is not cut at its dot.
    assert_eq!(copy_into(&dir.join(".env"), dir).unwrap(), dir.join(".env copy"));
    let copy = copy_into(&dir.join("src"), dir).unwrap();
    assert_eq!(copy, dir.join("src copy"));
    assert_eq!(fs::read_to_string(copy.join("deep/a.rs")).unwrap(), "fn a() {}");
    assert_eq!(fs::read_to_string(dir.join("notes.md")).unwrap(), "one");
    assert!(copy_into(&dir.join("src"), &dir.join("src/deep")).is_err(), "a folder is not copied into itself");
    assert!(copy_into(&dir.join("gone"), dir).is_err());
}

/// Discarding puts a committed file back as the commit has it, staged
/// or not, and only unstages one the commit does not have.
#[test]
fn discarding_goes_back_to_the_last_commit() {
    use emaki_core::git::{discard, Discarded};
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    let run = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&dir).args(args).output().unwrap().status.success(), "git {args:?}");
    run(&["init", "-q"]);
    run(&["config", "user.email", "t@example.com"]);
    run(&["config", "user.name", "t"]);
    run(&["config", "commit.gpgsign", "false"]);
    // Git for Windows turns line ends on checkout unless told not to.
    run(&["config", "core.autocrlf", "false"]);
    fs::write(dir.join("kept.txt"), "as committed\n").unwrap();
    fs::write(dir.join("gone.txt"), "here\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "one"]);
    // Changed and staged, deleted, and two new files, one of them staged.
    fs::write(dir.join("kept.txt"), "changed\n").unwrap();
    run(&["add", "kept.txt"]);
    fs::write(dir.join("kept.txt"), "changed again\n").unwrap();
    fs::remove_file(dir.join("gone.txt")).unwrap();
    fs::write(dir.join("new.txt"), "new\n").unwrap();
    fs::write(dir.join("staged.txt"), "new\n").unwrap();
    run(&["add", "staged.txt"]);
    assert_eq!(discard(&dir, &dir.join("kept.txt")), Ok(Discarded::Restored));
    assert_eq!(fs::read_to_string(dir.join("kept.txt")).unwrap(), "as committed\n");
    assert_eq!(discard(&dir, &dir.join("gone.txt")), Ok(Discarded::Restored));
    assert!(dir.join("gone.txt").exists());
    assert_eq!(discard(&dir, &dir.join("new.txt")), Ok(Discarded::New));
    assert_eq!(discard(&dir, &dir.join("staged.txt")), Ok(Discarded::New));
    assert!(dir.join("staged.txt").exists(), "a new file is left for the trash");
    let status = String::from_utf8(std::process::Command::new("git").arg("-C").arg(&dir).args(["status", "--porcelain"]).output().unwrap().stdout).unwrap();
    assert_eq!(status, "?? new.txt\n?? staged.txt\n");
}

/// A plan's dialog, as 2.1.296 draws it under a plan too long for the
/// screen: the question, the three choices, and the plan's file on the
/// last line, which is where the plan is read from.
#[test]
fn a_plans_dialog_names_its_file() {
    use emaki_core::driver::{dialog_on_screen, plan_file_on_dialog};
    let rule = "─".repeat(92);
    let screen = format!(
        "   - It has a decision with trade-offs, so there is a table.\n{}↓\n  {rule}\n   Claude has written up a plan and is ready to execute. Would you like to proceed?\n\n   ❯ 1. Yes, and use auto mode\n     2. Yes, manually approve edits\n     3. Tell Claude what to change\n        shift+tab to approve with this feedback\n\n   ctrl+g to edit in VS Code · ~/.claude/plans/peaceful-sprouting-moth.md\n",
        " ".repeat(94)
    );
    let d = dialog_on_screen(&screen).unwrap();
    assert_eq!(d.body, vec!["Claude has written up a plan and is ready to execute. Would you like to proceed?"]);
    assert_eq!(d.options.len(), 3);
    assert_eq!(plan_file_on_dialog(&d).as_deref(), Some("~/.claude/plans/peaceful-sprouting-moth.md"));
    // A question is no plan, and names no file.
    let asked = dialog_on_screen(&format!("{rule}\nWhich fruit?\n❯ 1. Apple\n     Crisp and sweet\n  2. Banana\n{rule}\n  3. Chat about this\nEnter to select · Esc to cancel\n")).unwrap();
    assert_eq!(plan_file_on_dialog(&asked), None);
}

#[test]
fn the_catalogue_of_agents_holds_together() {
    use emaki_core::agents::{self, Os};
    let all = agents::all();
    let mut ids = std::collections::HashSet::new();
    for a in all {
        assert!(ids.insert(a.id), "{} is listed twice", a.id);
        assert!(!a.bins.is_empty() && !a.name.is_empty() && !a.about.is_empty(), "{}", a.id);
        assert!(a.site.starts_with("https://") && a.docs.starts_with("https://"), "{}", a.id);
        assert!(!a.install.is_empty() && !a.sign_in.is_empty() && !a.plans.is_empty(), "{}", a.id);
        // Every agent can be installed on a Mac, and a way's command is
        // one line to paste.
        assert!(!a.ways(Os::Mac).is_empty(), "{}", a.id);
        assert!(a.install.iter().all(|w| !w.command.contains('\n') && !w.by.is_empty()), "{}", a.id);
    }
    // Where a way's command gets what it installs, for asking whether it
    // is still published; the test asks nothing of the network.
    let way = |command: &'static str| agents::Way { os: &[Os::Mac], by: "x", command };
    assert_eq!(agents::source_of(&way("curl -fsSL https://claude.ai/install.sh | bash")).as_deref(), Some("https://claude.ai/install.sh"));
    assert_eq!(agents::source_of(&way("powershell -ExecutionPolicy ByPass -c \"irm https://aider.chat/install.ps1 | iex\"")).as_deref(), Some("https://aider.chat/install.ps1"));
    assert_eq!(agents::source_of(&way("brew install --cask codex")).as_deref(), Some("https://formulae.brew.sh/api/cask/codex.json"));
    assert_eq!(agents::source_of(&way("brew install aider")).as_deref(), Some("https://formulae.brew.sh/api/formula/aider.json"));
    assert_eq!(agents::source_of(&way("brew install anomalyco/tap/opencode")), None);
    assert_eq!(agents::source_of(&way("npm install -g @qwen-code/qwen-code@latest")).as_deref(), Some("https://registry.npmjs.org/@qwen-code/qwen-code"));
    assert_eq!(agents::source_of(&way("npm install -g droid")).as_deref(), Some("https://registry.npmjs.org/droid"));
    assert_eq!(agents::source_of(&way("pipx install aider-chat")).as_deref(), Some("https://pypi.org/pypi/aider-chat/json"));
    assert_eq!(agents::source_of(&way("winget install GitHub.Copilot")), None);

    // Every agent whose sessions are read is in the catalogue.
    for id in emaki_core::model::AgentId::ALL {
        assert!(agents::of(id).is_some(), "{}", id.as_str());
    }
}

#[cfg(unix)]
#[test]
fn an_agent_is_found_by_its_program_and_asked_its_version() {
    use emaki_core::agents::{self, Agent};
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("pretend");
    std::fs::write(&program, "#!/bin/sh\necho 'pretend-cli 1.4.2 (build 9)'\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let signed: &'static str = Box::leak(dir.path().join("auth.json").to_string_lossy().to_string().into_boxed_str());
    let home: &'static str = Box::leak(dir.path().to_string_lossy().to_string().into_boxed_str());
    let a = Agent { id: "pretend", bins: &["pretend"], key_env: &[], home, signed: Box::leak(Box::new([signed])), keychain: "", ..*agents::by_id("codex").unwrap() };
    let dirs = vec![std::path::PathBuf::from("/bin"), std::path::PathBuf::from("/usr/bin"), bin.clone()];

    let found = agents::detect_in(&a, &dirs);
    assert_eq!(found.path.as_deref(), Some(program.as_path()));
    assert_eq!(found.version, "1.4.2");
    assert!(found.home);
    assert!(!found.signed);
    std::fs::write(signed, "{}").unwrap();
    assert!(agents::detect_in(&a, &dirs).signed);

    // A settings folder with no program is not an installed agent.
    let none = agents::detect_in(&a, &[dir.path().join("nowhere")]);
    assert!(!none.installed() && none.home && !none.signed && none.version.is_empty());

    assert_eq!(agents::version_from("2.1.296 (Claude Code)"), "2.1.296");
    assert_eq!(agents::version_from("codex-cli 0.162.0"), "0.162.0");
    assert_eq!(agents::version_from("v1.4"), "1.4");
    assert_eq!(agents::version_from("no number here"), "");
}

#[test]
fn git_publishes_fetches_and_branches_from_another_branch() {
    use emaki_core::git;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let run = |at: &std::path::Path, args: &[&str]| {
        let ok = std::process::Command::new("git").arg("-C").arg(at).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main", "-c", "core.autocrlf=false"]).args(args).output().unwrap();
        assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
    };
    let commit = |at: &std::path::Path, file: &str, text: &str| {
        std::fs::write(at.join(file), text).unwrap();
        run(at, &["add", "."]);
        run(at, &["commit", "-q", "-m", file]);
    };
    // A remote with no checkout, a clone that works in it, and a second
    // clone standing in for somebody else.
    run(&root, &["init", "-q", "--bare", "origin.git"]);
    let (work, other) = (root.join("work"), root.join("other"));
    run(&root, &["clone", "-q", "origin.git", "work"]);
    commit(&work, "a.txt", "one\n");
    run(&work, &["push", "-q", "-u", "origin", "main"]);
    run(&root, &["clone", "-q", "origin.git", "other"]);

    // A branch made here is on no remote until it is published.
    run(&work, &["switch", "-q", "-c", "mine"]);
    commit(&work, "b.txt", "two\n");
    let b = git::branches(&work).unwrap();
    assert_eq!((b.origin.as_deref(), b.unpublished.as_slice()), (Some("origin"), &["mine".to_string()][..]));
    assert_eq!(git::publish(&work, "mine"), Ok(()));
    assert!(git::branches(&work).unwrap().unpublished.is_empty());
    assert_eq!(git::ahead_behind(&work, "mine"), Some((0, 0)));
    assert_eq!(git::reach(&work), Ok(()));

    // Somebody else pushes to main: a fetch says how far behind it is
    // here, and it is brought up while another branch is checked out.
    commit(&other, "c.txt", "three\n");
    run(&other, &["push", "-q", "origin", "main"]);
    assert_eq!(git::ahead_behind(&work, "main"), Some((0, 0)), "nothing is known before a fetch");
    assert_eq!(git::fetch(&work), Ok(()));
    assert_eq!(git::ahead_behind(&work, "main"), Some((0, 1)));
    assert_eq!(git::fast_forward(&work, "main"), Ok(()));
    assert_eq!(git::ahead_behind(&work, "main"), Some((0, 0)));
    assert_eq!(git::branches(&work).unwrap().current.as_deref(), Some("mine"), "the checkout did not move");

    // A new branch from main, not from the branch checked out: it has
    // main's file and lacks this branch's, and a change not committed
    // comes along when it fits there.
    std::fs::write(work.join("a.txt"), "one\nand more\n").unwrap();
    assert_eq!(git::create_from(&work, "from-main", "main", git::Carry::Bring), Ok(()));
    assert_eq!(git::branches(&work).unwrap().current.as_deref(), Some("from-main"));
    assert!(work.join("c.txt").exists() && !work.join("b.txt").exists());
    assert_eq!(std::fs::read_to_string(work.join("a.txt")).unwrap(), "one\nand more\n");
    run(&work, &["checkout", "-q", "--", "a.txt"]);

    // Left behind, the changes stay on the branch that was checked out.
    run(&work, &["switch", "-q", "mine"]);
    std::fs::write(work.join("b.txt"), "two, changed\n").unwrap();
    assert_eq!(git::create_from(&work, "from-main-2", "main", git::Carry::Leave), Ok(()));
    assert!(!work.join("b.txt").exists());
    run(&work, &["switch", "-q", "mine"]);
    assert_eq!(git::restore(&work), Ok(()));
    assert_eq!(std::fs::read_to_string(work.join("b.txt")).unwrap(), "two, changed\n");
    run(&work, &["checkout", "-q", "--", "b.txt"]);

    // A main with commits of its own cannot be brought up without a
    // merge, and is left as it is.
    run(&work, &["switch", "-q", "main"]);
    commit(&work, "d.txt", "mine alone\n");
    commit(&other, "e.txt", "theirs alone\n");
    run(&other, &["push", "-q", "origin", "main"]);
    git::fetch(&work).unwrap();
    assert_eq!(git::ahead_behind(&work, "main"), Some((1, 1)));
    assert!(git::fast_forward(&work, "main").is_err());
    assert!(work.join("d.txt").exists() && !work.join("e.txt").exists());

    // A remote that is not there says so, and nothing waits on a prompt.
    run(&work, &["remote", "set-url", "origin", root.join("gone.git").to_str().unwrap()]);
    assert!(git::fetch(&work).is_err());

    // What git's words say went wrong.
    use git::NetTrouble::*;
    assert_eq!(git::why_net("unable to access 'https://github.com/a/b.git/': Could not resolve host: github.com"), Offline);
    assert_eq!(git::why_net("ssh: Could not resolve hostname github.com: nodename nor servname provided"), Offline);
    assert_eq!(git::why_net("could not read Username for 'https://github.com': terminal prompts disabled"), SignIn);
    assert_eq!(git::why_net("Authentication failed for 'https://github.com/a/b.git/'"), SignIn);
    assert_eq!(git::why_net("unable to access 'https://github.com/a/b.git/': The requested URL returned error: 403"), SignIn);
    assert_eq!(git::why_net("git@github.com: Permission denied (publickey)."), SignIn);
    assert_eq!(git::why_net("failed to push some refs to 'origin'"), Other);
    assert_eq!(git::host_of("https://github.com/a/b.git"), ("github.com".to_string(), false));
    assert_eq!(git::host_of("git@github.com:a/b.git"), ("github.com".to_string(), true));
    assert_eq!(git::host_of("ssh://git@example.org:22/a/b.git"), ("example.org".to_string(), true));
    assert_eq!(git::host_of("/a/local/folder.git"), (String::new(), false));
}

/// An archive from before each agent had a folder: Claude Code's projects
/// at its top, one of them named as an agent is, and Codex under an
/// underscore. It is moved into today's shape and nothing in it is lost.
#[test]
fn an_archive_of_the_old_shape_is_moved_into_the_new() {
    let _g = isolated();
    let root = archive::archive_dir();
    let put = |rel: &str, text: &str| {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    };
    put("proj/a.jsonl", "a\n");
    put("proj/a/subagents/agent-1.jsonl", "sub\n");
    put("codex/b.jsonl", "b\n");
    put("_codex/work/rollout-1.jsonl", "c\n");
    put("state.json", "{}");
    assert!(archive::settle_layout());
    let read = |rel: &str| fs::read_to_string(root.join(rel)).unwrap_or_default();
    assert_eq!(read("claude/proj/a.jsonl"), "a\n");
    assert_eq!(read("claude/proj/a/subagents/agent-1.jsonl"), "sub\n");
    // A project of Claude Code's named "codex" is not taken for Codex's folder.
    assert_eq!(read("claude/codex/b.jsonl"), "b\n");
    assert_eq!(read("codex/work/rollout-1.jsonl"), "c\n");
    let top = |root: &std::path::Path| {
        let mut v: Vec<String> = fs::read_dir(root).unwrap().filter_map(Result::ok).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        v.sort();
        v
    };
    assert_eq!(top(&root), ["claude", "codex"]);
    assert_eq!(archive::iter_archived(AgentId::ClaudeCode).len(), 2);
    assert_eq!(archive::iter_archived(AgentId::Codex).len(), 1);
    // Again changes nothing, and a project an older Emaki leaves at the
    // top afterwards is taken in beside what is there.
    put("proj/a.jsonl", "a\n");
    put("proj/later.jsonl", "d\n");
    put("proj/a/subagents/agent-1.jsonl", "sub, rewritten\n");
    assert!(archive::settle_layout());
    assert_eq!(top(&root), ["claude", "codex"]);
    assert_eq!(read("claude/proj/later.jsonl"), "d\n");
    assert_eq!(read("claude/proj/a/subagents/agent-1.jsonl"), "sub\n");
    assert_eq!(read("claude/proj/a/subagents/agent-1.gen1.jsonl"), "sub, rewritten\n");
}
