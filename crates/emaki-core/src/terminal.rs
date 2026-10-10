//! Continuing a session in the person's own terminal.
//!
//! The window can talk to a session, but sometimes the terminal is where
//! you want to be. This module says what to run (`resume_argv`) and writes
//! it as a script (`write_script`) that any terminal can be handed: on macOS
//! `open` gives a `.command` file to whatever app the system keeps for
//! shell scripts (Terminal by default, or the one you chose), on Linux the
//! terminal emulator takes the script as its command. The window's `sys`
//! does the handing over, per OS.
//!
//! Only the agent's own resume command is used, never a copy of ours:
//! `claude --resume <id>` appends to the same transcript, `codex resume <id>`
//! reopens the same rollout. Whoever opens this must first make sure no
//! other process is writing the transcript (a terminal session with an
//! inbox, or a driver of ours), or two processes would write one file.

use std::io;
use std::path::PathBuf;

use crate::model::AgentId;
use crate::{driver, paths};

/// The command that continues `session_id` in a terminal, as argv. The
/// first element is the binary: Claude Code's is resolved the way the
/// driver resolves it, so a terminal without `claude` on its `PATH` still
/// works; Codex is left to the shell's `PATH`.
pub fn resume_argv(agent: AgentId, session_id: &str) -> Vec<String> {
    match agent {
        AgentId::ClaudeCode => {
            let bin = driver::claude_binary().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "claude".into());
            vec![bin, "--resume".into(), session_id.into()]
        }
        AgentId::Codex => vec!["codex".into(), "resume".into(), session_id.into()],
        AgentId::Gemini => vec!["gemini".into(), "--resume".into(), session_id.into()],
    }
}

/// `s` as one POSIX shell word: single-quoted, with any single quote inside
/// closed, escaped and reopened.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,~".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The line that clears what `driver::child_env` clears: every `CLAUDE*`
/// variable but `CLAUDE_CONFIG_DIR`, and `EMAKI_DISABLE`. A terminal app
/// that was itself started from inside a Claude Code session carries that
/// session's marker, and Claude Code then treats the resumed session as a
/// child and stops writing its transcript.
pub const UNSET_LINE: &str = "for v in \"${!CLAUDE@}\"; do [ \"$v\" = CLAUDE_CONFIG_DIR ] || unset \"$v\"; done; unset EMAKI_DISABLE";

/// The script text: clear the inherited session marks, change to `cwd`,
/// then become the command. `exec` so the shell leaves no extra process,
/// and so the terminal's window follows the agent's exit.
pub fn script(cwd: &str, argv: &[String]) -> String {
    let words: Vec<String> = argv.iter().map(|a| shell_quote(a)).collect();
    format!("#!/bin/bash\n{UNSET_LINE}\ncd {} && exec {}\n", shell_quote(cwd), words.join(" "))
}

/// Where the script for `session_id` lives: `~/.emaki/run/terminal/`, one
/// file per session, rewritten on every open. The `.command` extension is
/// what macOS recognises as "run me in a terminal".
pub fn script_path(session_id: &str) -> PathBuf {
    paths::run_dir().join("terminal").join(format!("{}.command", paths::safe_component(session_id)))
}

/// Write the script for `session_id`, executable, and say where it is.
pub fn write_script(session_id: &str, cwd: &str, argv: &[String]) -> io::Result<PathBuf> {
    let path = script_path(session_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    paths::write_atomic(&path, script(cwd, argv).as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}
