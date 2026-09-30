//! Emaki's status line for Claude Code: the script built into the binary,
//! where it is installed, and the one setting that points Claude Code at it.
//!
//! The terminal's status line is the only place Claude Code hands out the
//! account's rate limits (see `limits.rs`), so a terminal session can keep
//! the limits row current only through the status-line command in
//! `~/.claude/settings.json`. The first version of the row leaned on a
//! script outside the repository, patched by hand; the patch was lost and
//! the row froze. Now the script is `scripts/statusline.sh`, compiled in
//! as `SCRIPT`, and `install` copies it to `~/.emaki/bin/statusline.sh`
//! (a stable path outside any checkout, which is the lesson of the Python
//! daemon's hooks) and sets `statusLine` to run it.
//!
//! `ensure` runs at every launch of the app (`Hub::start`): it refreshes
//! the script and sets `statusLine` when the setting is not already ours,
//! so the status line is simply there, with nothing to switch on. This is
//! the one write the app makes to Claude Code's settings. It changes no
//! other key, keeps the previous value in `state/statusline.json` for
//! `emaki-core statusline restore`, and leaves a settings file that does
//! not parse alone rather than overwrite it. A copy run with `EMAKI_HOME`
//! set (a second copy beside the installed app) does not touch the
//! settings at all, or it would point Claude Code at its scratch tree.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::paths;

/// `scripts/statusline.sh`, byte for byte.
pub const SCRIPT: &str = include_str!("../../../scripts/statusline.sh");

/// Where the script is installed: `~/.emaki/bin/statusline.sh`.
pub fn script_path() -> PathBuf {
    paths::root().join("bin").join("statusline.sh")
}

/// Claude Code's settings file, `~/.claude/settings.json`
/// (`CLAUDE_CONFIG_DIR` overrides the directory).
pub fn settings_file() -> PathBuf {
    paths::claude_home().join("settings.json")
}

/// The previous `statusLine` value, kept for `restore`.
fn memo_file() -> PathBuf {
    paths::state_dir().join("statusline.json")
}

/// The `statusLine.command` string `install` writes: `bash` plus the
/// script, with the home directory written as `~` so a synced settings
/// file works on another machine.
pub fn command() -> String {
    // A bash line, so the path is written with forward slashes whatever the
    // OS builds it with: on Windows `~\.emaki\bin\statusline.sh` would
    // reach bash as escapes.
    format!("bash {}", paths::tilde(&script_path().to_string_lossy()).replace('\\', "/"))
}

/// What Claude Code's status line is right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The command runs Emaki's script. `current` says the installed file
    /// matches the one built into this binary.
    Emaki { current: bool },
    /// Some other command.
    Other(String),
    /// No `statusLine` in the settings, or no settings file.
    None,
}

fn read_settings() -> io::Result<Map<String, Value>> {
    let path = settings_file();
    if !path.exists() {
        return Ok(Map::new());
    }
    let text = fs::read_to_string(&path)?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} is not a JSON object", path.display()))),
        Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} does not parse: {e}", path.display()))),
    }
}

fn current_command(settings: &Map<String, Value>) -> Option<String> {
    settings.get("statusLine")?.get("command")?.as_str().map(str::to_string)
}

/// Does this `statusLine.command` run our script, at either spelling of
/// the path?
fn is_ours(command: &str) -> bool {
    let full = script_path().to_string_lossy().to_string();
    command.contains(&full) || command.contains(&paths::tilde(&full))
}

pub fn state() -> State {
    let Ok(settings) = read_settings() else { return State::None };
    match current_command(&settings) {
        Some(cmd) if is_ours(&cmd) => State::Emaki { current: fs::read_to_string(script_path()).map(|s| s == SCRIPT).unwrap_or(false) },
        Some(cmd) => State::Other(cmd),
        Option::None => State::None,
    }
}

/// Write the script where `script_path` says, executable. Safe to call
/// any time: it only refreshes the file.
pub fn write_script() -> io::Result<PathBuf> {
    let path = script_path();
    paths::write_atomic(&path, SCRIPT.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}

/// What the app does at launch: install unless the setting already runs
/// the current script. Skipped under `EMAKI_HOME`, see the module note.
/// Returns whether the settings file was written.
pub fn ensure() -> io::Result<bool> {
    if std::env::var_os("EMAKI_HOME").is_some_and(|v| !v.is_empty()) {
        return Ok(false);
    }
    install_if_needed()
}

/// `install`, unless the setting already runs this build's script; then
/// nothing is written, so a launch costs two reads.
pub fn install_if_needed() -> io::Result<bool> {
    if state() == (State::Emaki { current: true }) {
        return Ok(false);
    }
    install().map(|()| true)
}

/// Install the script and point Claude Code's `statusLine` at it. The
/// previous value of the setting, if it was not already ours, is kept for
/// `restore`. Every other key in the settings file is left as it was.
/// Claude Code picks the change up when it next starts.
pub fn install() -> io::Result<()> {
    let mut settings = read_settings()?;
    write_script()?;
    let previous = settings.get("statusLine").cloned();
    let already_ours = current_command(&settings).map(|c| is_ours(&c)).unwrap_or(false);
    if !already_ours {
        paths::ensure_dirs()?;
        paths::write_json(&memo_file(), &json!({ "previous": previous.unwrap_or(Value::Null) }))?;
    }
    settings.insert("statusLine".into(), json!({ "type": "command", "command": command() }));
    paths::write_json(&settings_file(), &Value::Object(settings))
}

/// The `statusLine` value `restore` would put back: the one `install`
/// found, or nothing if there was none or `install` never ran.
pub fn previous() -> Option<Value> {
    let memo = paths::read_json(&memo_file())?;
    match memo.get("previous")? {
        Value::Null => Option::None,
        v => Some(v.clone()),
    }
}

/// Put the setting back as it was before `install`. The installed script
/// stays on disk; nothing runs it. Refuses when the setting is no longer
/// ours, so a change made by hand since is not undone.
pub fn restore() -> io::Result<()> {
    let mut settings = read_settings()?;
    if !current_command(&settings).map(|c| is_ours(&c)).unwrap_or(false) {
        return Err(io::Error::new(io::ErrorKind::Other, "the status line is not Emaki's now; nothing to put back"));
    }
    match previous() {
        Some(v) => {
            settings.insert("statusLine".into(), v);
        }
        Option::None => {
            settings.remove("statusLine");
        }
    }
    paths::write_json(&settings_file(), &Value::Object(settings))?;
    let _ = fs::remove_file(memo_file());
    Ok(())
}
