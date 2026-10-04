//! The few things the window asks the operating system for: open a file
//! with whatever the system keeps for it, show it in the file manager, and
//! the person's name for the sidebar footer. Everything here has a Windows
//! and a Linux answer, so nothing above it needs to know which it is on.

use std::path::Path;

/// Bring the terminal a session runs in to the front: the application
/// that owns the process `pid`, found by walking up its parents until one
/// is an application the system knows (the terminal app, or an IDE with a
/// terminal inside it), and within it, where the app can be asked, the
/// tab or pane whose tty the process is on. Returns the app's name. There
/// is no general way to put the pane itself in front: Terminal and iTerm2
/// take an AppleScript keyed on the tty, WezTerm (and Kaku, built on it)
/// a `cli activate-pane` keyed on the same, and anything else is brought
/// forward as an app and left as it was.
#[cfg(target_os = "macos")]
pub fn focus_terminal(pid: i32) -> Result<String, String> {
    let host = host_of(pid)?;
    host.focus();
    Ok(host.name)
}

/// What the terminal a session runs in is showing, where the app can be
/// asked: Terminal and iTerm2 by AppleScript for the tab on the tty,
/// WezTerm and Kaku by `cli get-text` for the pane. Nothing for an IDE's
/// terminal, which no one outside the IDE can read.
#[cfg(target_os = "macos")]
pub fn terminal_text(pid: i32) -> Option<String> {
    terminal_read(pid, false)
}

/// The same screen with its colours where the terminal gives them:
/// WezTerm and Kaku write the escape sequences out (`--escapes`);
/// Terminal and iTerm2 hand over plain text.
#[cfg(target_os = "macos")]
pub fn terminal_styled(pid: i32) -> Option<String> {
    terminal_read(pid, true)
}

#[cfg(target_os = "macos")]
fn terminal_read(pid: i32, styled: bool) -> Option<String> {
    let host = host_of(pid).ok()?;
    if host.tty.is_empty() {
        return None;
    }
    let text = match host.bundle.as_str() {
        "com.apple.Terminal" => osascript_out(&format!(
            r#"tell application "Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t is "{}" then return contents of t
    end repeat
  end repeat
end tell"#,
            host.tty
        ))
        .ok()?,
        "com.googlecode.iterm2" => osascript_out(&format!(
            r#"tell application "iTerm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "{}" then return contents of s
      end repeat
    end repeat
  end repeat
end tell"#,
            host.tty
        ))
        .ok()?,
        _ => {
            let (exe, pane) = host.wezterm_pane()?;
            let mut cmd = std::process::Command::new(exe);
            cmd.args(["cli", "get-text", "--pane-id", &pane.to_string()]);
            if styled {
                cmd.arg("--escapes");
            }
            let out = cmd.output().ok()?;
            if !out.status.success() {
                return None;
            }
            String::from_utf8_lossy(&out.stdout).to_string()
        }
    };
    (!text.trim().is_empty()).then_some(text)
}

/// Type `text` into the terminal a session runs in and send it, as the
/// person would: the slash command Claude Code only takes at its own
/// prompt. Terminal (`do script`), iTerm2 (`write text`) and WezTerm or
/// Kaku (`cli send-text --no-paste`) take the text for the tab or pane on
/// the process's tty. Any other host (an IDE's terminal, say) is typed
/// into with System Events keystrokes, which needs Accessibility access
/// for Emaki; without it the text is put on the clipboard and the error
/// says so. Returns the host's name.
#[cfg(target_os = "macos")]
pub fn type_in_terminal(pid: i32, text: &str) -> Result<String, String> {
    let host = host_of(pid)?;
    host.focus();
    let typed = match host.bundle.as_str() {
        "com.apple.Terminal" => osascript(&format!(
            r#"tell application "Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t is "{}" then do script "{}" in t
    end repeat
  end repeat
end tell"#,
            host.tty,
            as_quoted(text)
        )),
        "com.googlecode.iterm2" => osascript(&format!(
            r#"tell application "iTerm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "{}" then write text "{}"
      end repeat
    end repeat
  end repeat
end tell"#,
            host.tty,
            as_quoted(text)
        )),
        _ => match host.wezterm_pane() {
            Some((exe, pane)) => std::process::Command::new(exe)
                .args(["cli", "send-text", "--no-paste", "--pane-id", &pane.to_string(), &format!("{text}\r")])
                .output()
                .map_err(|e| e.to_string())
                .and_then(|o| if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).to_string()) }),
            None => {
                // The app has to be in front before the keys arrive, and
                // its terminal panel under them.
                host.wait_front()
                    .and_then(|_| host.focus_ide_terminal())
                    .and_then(|_| osascript(&format!(r#"tell application "System Events" to keystroke "{}""#, as_quoted(text))))
                    .and_then(|_| osascript(r#"tell application "System Events" to key code 36"#))
            }
        },
    };
    match typed {
        Ok(()) => Ok(host.name),
        Err(e) => {
            // Second best: the text is one ⌘V away.
            let _ = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn().and_then(|mut c| {
                use std::io::Write;
                if let Some(mut i) = c.stdin.take() {
                    let _ = i.write_all(text.as_bytes());
                }
                c.wait()
            });
            if e.contains("assistive access") || e.contains("not allowed") {
                Err(format!("{text} is on your clipboard: paste it in {}, or give Emaki Accessibility access in System Settings › Privacy & Security so it can type there", host.name))
            } else {
                Err(format!("could not type in {}: {e}. {text} is on your clipboard", host.name))
            }
        }
    }
}

/// A key Claude Code's terminal takes that is not text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKey {
    /// Stops the running turn.
    Escape,
    /// Steps to the next permission mode.
    ShiftTab,
}

/// Press `key` in the terminal a session runs in. WezTerm and Kaku take
/// it for the pane and iTerm2 for the session, without coming to the
/// front; Terminal and any other host have to be in front for System
/// Events to press it (checked, `Host::wait_front`, and an IDE with its
/// terminal panel focused first, `Host::focus_ide_terminal`), which
/// needs Accessibility access for Emaki, and the front is given back to
/// Emaki after, since nothing there needs reading. Returns the host's
/// name.
#[cfg(target_os = "macos")]
pub fn key_in_terminal(pid: i32, key: TerminalKey) -> Result<String, String> {
    let host = host_of(pid)?;
    // What the key is on the wire, as iTerm2 is told it, and as System
    // Events presses it.
    let (bytes, iterm, events, name) = match key {
        TerminalKey::Escape => ("\x1b", "(ASCII character 27)", "key code 53", "Escape"),
        // Back-tab, as a terminal sends it: ESC [ Z.
        TerminalKey::ShiftTab => ("\x1b[Z", r#"((ASCII character 27) & "[Z")"#, "key code 48 using shift down", "⇧Tab"),
    };
    let pressed = if host.bundle == "com.googlecode.iterm2" {
        osascript(&format!(
            r#"tell application "iTerm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "{}" then tell s to write text {} newline NO
      end repeat
    end repeat
  end repeat
end tell"#,
            host.tty, iterm
        ))
    } else if let Some((exe, pane)) = host.wezterm_pane() {
        std::process::Command::new(exe)
            .args(["cli", "send-text", "--no-paste", "--pane-id", &pane.to_string(), bytes])
            .output()
            .map_err(|e| e.to_string())
            .and_then(|o| if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).to_string()) })
    } else {
        host.focus();
        let pressed = host
            .wait_front()
            .and_then(|_| host.focus_ide_terminal())
            .and_then(|_| osascript(&format!(r#"tell application "System Events" to {events}"#)));
        if pressed.is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(150));
            activate_self();
        }
        pressed
    };
    match pressed {
        Ok(()) => Ok(host.name),
        Err(e) if e.contains("assistive access") || e.contains("not allowed") => {
            Err(format!("press {name} in {}, or give Emaki Accessibility access in System Settings › Privacy & Security so it can", host.name))
        }
        Err(e) => Err(format!("could not press {name} from here: {e}. Press it in {}", host.name)),
    }
}

/// Send `text` to the terminal a session runs in as keys, with nothing
/// added: a digit that picks a choice in a dialog, Tab, Return, or words
/// for its text field. Only where the terminal takes text for a pane
/// without coming forward (WezTerm and Kaku by `cli send-text
/// --no-paste`, iTerm2 by `write text`); any other host answers with why
/// not, and the dialog is answered in the terminal.
#[cfg(target_os = "macos")]
pub fn text_in_terminal(pid: i32, text: &str) -> Result<(), String> {
    let host = host_of(pid)?;
    if host.bundle == "com.googlecode.iterm2" {
        // AppleScript has no escapes for control characters: the text
        // is its printable stretches joined with `ASCII character`s.
        let mut parts: Vec<String> = Vec::new();
        let mut run = String::new();
        for c in text.chars() {
            if c.is_control() || c == '"' || c == '\\' {
                if !run.is_empty() {
                    parts.push(format!("\"{run}\""));
                    run.clear();
                }
                parts.push(format!("(ASCII character {})", c as u32));
            } else {
                run.push(c);
            }
        }
        if !run.is_empty() {
            parts.push(format!("\"{run}\""));
        }
        return osascript(&format!(
            r#"tell application "iTerm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "{}" then tell s to write text ({}) newline NO
      end repeat
    end repeat
  end repeat
end tell"#,
            host.tty,
            parts.join(" & ")
        ));
    }
    let Some((exe, pane)) = host.wezterm_pane() else {
        return Err(format!("answer it in {}: its keys cannot be sent from here", host.name));
    };
    std::process::Command::new(exe)
        .args(["cli", "send-text", "--no-paste", "--pane-id", &pane.to_string(), text])
        .output()
        .map_err(|e| e.to_string())
        .and_then(|o| if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).to_string()) })
}

#[cfg(not(target_os = "macos"))]
pub fn text_in_terminal(_pid: i32, _text: &str) -> Result<(), String> {
    Err("reaching the terminal's keys from here is not done on this platform yet: answer it in the terminal".into())
}

/// Bring this app back to the front.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn activate_self() {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    NSRunningApplication::currentApplication().activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
}

#[cfg(not(target_os = "macos"))]
pub fn key_in_terminal(_pid: i32, _key: TerminalKey) -> Result<String, String> {
    Err("reaching the terminal's keys from here is not done on this platform yet: press it in the terminal".into())
}

#[cfg(not(target_os = "macos"))]
pub fn focus_terminal(_pid: i32) -> Result<String, String> {
    Err("finding the terminal's window is not done on this platform yet".into())
}

#[cfg(not(target_os = "macos"))]
pub fn terminal_text(_pid: i32) -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn terminal_styled(_pid: i32) -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn type_in_terminal(_pid: i32, _text: &str) -> Result<String, String> {
    Err("typing into the terminal is not done on this platform yet".into())
}

/// The application a session's process runs under, and the tty it is on.
#[cfg(target_os = "macos")]
struct Host {
    app: objc2::rc::Retained<objc2_app_kit::NSRunningApplication>,
    name: String,
    bundle: String,
    path: String,
    tty: String,
}

#[cfg(target_os = "macos")]
impl Host {
    /// The tab or pane in front, where the app can be asked, then the app.
    /// (The ignoring-other-apps flag is said to do nothing from macOS 14 on,
    /// and the activation still lands; it stays for older systems.)
    #[allow(deprecated)]
    fn focus(&self) {
        use objc2_app_kit::NSApplicationActivationOptions;
        if !self.tty.is_empty() {
            select_pane(&self.bundle, &self.path, &self.tty);
        }
        self.app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }

    /// Wait until this app is the one in front, which is where System
    /// Events sends keys. Asking an app to activate is a request the
    /// system may be slow to grant or may refuse, and keys sent before
    /// it lands go to whatever is in front instead: Emaki's own
    /// composer, where ⇧Tab is bound to the mode. Nothing is pressed
    /// unless the app answers as frontmost.
    fn wait_front(&self) -> Result<(), String> {
        let want = self.app.processIdentifier().to_string();
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let out = std::process::Command::new("osascript")
                .args(["-e", r#"tell application "System Events" to get unix id of first process whose frontmost is true"#])
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            if String::from_utf8_lossy(&out.stdout).trim() == want {
                return Ok(());
            }
        }
        Err(format!("{} did not come to the front", self.name))
    }

    /// An IDE built on VS Code (Positron, Cursor, VS Code itself) takes
    /// keys wherever its focus was left: an editor, the file tree. The
    /// app coming to the front does not move that, and keys meant for
    /// Claude Code then land in a file. Its command palette can be asked
    /// for "Terminal: Focus Terminal", which puts the focus in the
    /// terminal panel whatever held it, and is the same from the panel
    /// itself (the toggle on ⌃` would close the panel from there). The
    /// app must already be in front. It focuses the terminal the IDE
    /// has active, which with several open may not be this session's:
    /// nothing outside the IDE can pick one by its tty. Anything that is
    /// not such an IDE is left as it is.
    fn focus_ide_terminal(&self) -> Result<(), String> {
        if !self.is_ide() {
            return Ok(());
        }
        osascript(
            r#"tell application "System Events"
  keystroke "p" using {command down, shift down}
  delay 0.35
  keystroke "Terminal: Focus Terminal"
  delay 0.35
  key code 36
  delay 0.3
end tell"#,
        )
    }

    /// Whether this is an IDE built on VS Code, with a terminal panel
    /// inside it and a focus of its own.
    fn is_ide(&self) -> bool {
        std::path::Path::new(&self.path).join("Contents/Resources/app/product.json").exists()
    }

    /// For WezTerm and Kaku: the CLI beside the gui binary and the pane on
    /// our tty, from `cli list`.
    fn wezterm_pane(&self) -> Option<(std::path::PathBuf, u64)> {
        for cli in ["kaku", "wezterm"] {
            let exe = std::path::Path::new(&self.path).join("Contents/MacOS").join(cli);
            if !exe.exists() {
                continue;
            }
            let out = std::process::Command::new(&exe).args(["cli", "list", "--format", "json"]).output().ok()?;
            let panes: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).ok()?;
            let pane = panes.iter().find(|p| p.get("tty_name").and_then(|v| v.as_str()) == Some(self.tty.as_str()))?;
            return pane.get("pane_id").and_then(|v| v.as_u64()).map(|id| (exe, id));
        }
        None
    }
}

/// Walk up from `pid` to the application the system knows: the terminal
/// app, or the IDE holding a terminal.
#[cfg(target_os = "macos")]
fn host_of(pid: i32) -> Result<Host, String> {
    use objc2_app_kit::NSRunningApplication;
    let tty = ps(pid, "tty").map(|t| format!("/dev/{t}")).unwrap_or_default();
    let mut p = pid;
    for _ in 0..12 {
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(p) {
            if let Some(bundle) = app.bundleIdentifier().map(|b| b.to_string()) {
                let name = app.localizedName().map(|n| n.to_string()).unwrap_or_else(|| "the terminal".into());
                let path = app.bundleURL().and_then(|u| u.path().map(|p| p.to_string())).unwrap_or_default();
                return Ok(Host { app, name, bundle, path, tty });
            }
        }
        match ps(p, "ppid").and_then(|v| v.parse::<i32>().ok()) {
            Some(pp) if pp > 1 => p = pp,
            _ => break,
        }
    }
    Err("could not tell which app the terminal is".into())
}

#[cfg(target_os = "macos")]
fn osascript(script: &str) -> Result<(), String> {
    let out = std::process::Command::new("osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Run a script for what it returns.
#[cfg(target_os = "macos")]
fn osascript_out(script: &str) -> Result<String, String> {
    let out = std::process::Command::new("osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// `text` inside an AppleScript string literal.
#[cfg(target_os = "macos")]
fn as_quoted(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// One column of `ps` for a process, trimmed.
#[cfg(target_os = "macos")]
fn ps(pid: i32, column: &str) -> Option<String> {
    let out = std::process::Command::new("ps").args(["-o", &format!("{column}="), "-p", &pid.to_string()]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty() && s != "??").then_some(s)
}

/// Put the tab or pane on `tty` in front inside the terminal app, where
/// the app can be asked. Best effort: a failure leaves the app to be
/// activated as it is.
#[cfg(target_os = "macos")]
fn select_pane(bundle: &str, bundle_path: &str, tty: &str) {
    let osascript = |script: String| {
        let _ = osascript(&script);
    };
    match bundle {
        "com.apple.Terminal" => osascript(format!(
            r#"tell application "Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t is "{tty}" then
        set selected tab of w to t
        set index of w to 1
      end if
    end repeat
  end repeat
end tell"#
        )),
        "com.googlecode.iterm2" => osascript(format!(
            r#"tell application "iTerm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "{tty}" then
          select s
          select t
          select w
        end if
      end repeat
    end repeat
  end repeat
end tell"#
        )),
        _ => {
            // WezTerm, and Kaku built on it: the CLI beside the gui binary
            // lists every pane with its tty and can focus one.
            for cli in ["kaku", "wezterm"] {
                let exe = std::path::Path::new(bundle_path).join("Contents/MacOS").join(cli);
                if !exe.exists() {
                    continue;
                }
                let Ok(out) = std::process::Command::new(&exe).args(["cli", "list", "--format", "json"]).output() else { continue };
                let Ok(panes) = serde_json::from_slice::<Vec<serde_json::Value>>(&out.stdout) else { continue };
                if let Some(pane) = panes.iter().find(|p| p.get("tty_name").and_then(|v| v.as_str()) == Some(tty)) {
                    if let Some(tab) = pane.get("tab_id").and_then(|v| v.as_u64()) {
                        let _ = std::process::Command::new(&exe).args(["cli", "activate-tab", "--tab-id", &tab.to_string()]).output();
                    }
                    if let Some(id) = pane.get("pane_id").and_then(|v| v.as_u64()) {
                        let _ = std::process::Command::new(&exe).args(["cli", "activate-pane", "--pane-id", &id.to_string()]).output();
                    }
                }
                break;
            }
        }
    }
}

/// Open `path` with the application the system associates with it.
pub fn open_path(path: &Path) {
    let _ = opener::open(path);
}

/// Show `path` selected in the file manager. macOS and Windows can select a
/// file; on Linux the folder opens, which is as close as xdg gets without a
/// D-Bus dependency.
pub fn reveal_path(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(parent) = path.parent() {
            let _ = opener::open(parent);
        }
    }
}

/// Run `argv` in `cwd` in a terminal window of the person's own. The script
/// is `emaki_core::terminal::write_script`; what opens it is per OS. On
/// macOS `open` hands a `.command` file to the app the system keeps for
/// shell scripts, Terminal unless another terminal claimed the type, so the
/// choice is the system's, not ours. On Windows it is a new `cmd` window
/// through `start`, in `cwd`. On Linux `$TERMINAL`, then the usual names.
pub fn open_in_terminal(session_id: &str, cwd: &str, argv: &[String]) -> Result<(), String> {
    let script = emaki_core::terminal::write_script(session_id, cwd, argv).map_err(|e| format!("could not write the script: {e}"))?;
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open").arg(&script).status().map_err(|e| format!("could not run open: {e}"))?;
        if !status.success() {
            return Err("no app on this Mac opens shell scripts".into());
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let _ = script;
        let line = argv.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ");
        std::process::Command::new("cmd")
            .args(["/c", "start", "", "/D", cwd, "cmd", "/k", &line])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open a terminal: {e}"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut candidates: Vec<String> = std::env::var("TERMINAL").ok().filter(|t| !t.is_empty()).into_iter().collect();
        candidates.extend(["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "alacritty", "kitty", "wezterm", "xterm"].map(String::from));
        for term in candidates {
            let mut cmd = std::process::Command::new(&term);
            // gnome-terminal takes its command after `--`; the rest after `-e`.
            if term.ends_with("gnome-terminal") {
                cmd.arg("--");
            } else {
                cmd.arg("-e");
            }
            if cmd.arg(&script).current_dir(cwd).spawn().is_ok() {
                return Ok(());
            }
        }
        Err("no terminal emulator found; set $TERMINAL".into())
    }
}

/// What the reveal action is called where we are.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(target_os = "windows") {
    "Show in Explorer"
} else {
    "Show in folder"
};

/// The same action on the button that shows a session's transcript file.
pub const REVEAL_TRANSCRIPT_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal the transcript in Finder"
} else if cfg!(target_os = "windows") {
    "Show the transcript in Explorer"
} else {
    "Show the transcript in its folder"
};

/// The account's first name, else the login name, capitalised.
pub fn user_first_name() -> String {
    let full = whoami::fallible::realname().unwrap_or_default();
    let name = full
        .split_whitespace()
        .next()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .or_else(|| whoami::fallible::username().ok())
        .unwrap_or_default();
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The Dock icon, for a bare binary run from `target/`. A bundle carries the
/// same picture as `Emaki.icns`; this makes a `cargo run` look the same.
#[cfg(target_os = "macos")]
pub fn install_dock_icon() {
    use objc2::{AllocAnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(include_bytes!("../assets/icon/icon-1024.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}
#[cfg(not(target_os = "macos"))]
pub fn install_dock_icon() {}

/// A picture of the app's own window, written to `path` as a PNG, for
/// looking at a state from a script (`EMAKI_SHOT`). A process may capture
/// its own windows without Screen Recording access, which the terminal
/// that runs a probe usually lacks. `CGWindowListCreateImage` is looked
/// up by name: newer SDKs mark it unavailable in favour of
/// ScreenCaptureKit, which wants that access, while the function itself
/// is still there and still captures a window of the caller's own.
#[cfg(target_os = "macos")]
pub fn shoot_window(window: &gpui::Window, path: &std::path::Path) -> Result<(), String> {
    use std::ffi::{c_char, c_void, CString};
    use std::os::unix::ffi::OsStrExt;

    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[repr(C)]
    struct Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }
    type Capture = unsafe extern "C" fn(Rect, u32, u32, u32) -> *mut c_void;
    #[link(name = "ImageIO", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn CFURLCreateFromFileSystemRepresentation(alloc: *const c_void, path: *const u8, len: isize, is_dir: u8) -> *mut c_void;
        fn CFStringCreateWithCString(alloc: *const c_void, text: *const c_char, encoding: u32) -> *mut c_void;
        fn CGImageDestinationCreateWithURL(url: *mut c_void, kind: *mut c_void, count: usize, options: *const c_void) -> *mut c_void;
        fn CGImageDestinationAddImage(dest: *mut c_void, image: *mut c_void, properties: *const c_void);
        fn CGImageDestinationFinalize(dest: *mut c_void) -> u8;
        fn CFRelease(obj: *mut c_void);
    }

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return Err("not an AppKit window".into()) };
    let view = unsafe { (handle.ns_view.as_ptr() as *const NSView).as_ref() }.ok_or("no view")?;
    let number = view.window().ok_or("the view has no window")?.windowNumber();
    let bytes = path.as_os_str().as_bytes();
    unsafe {
        // RTLD_DEFAULT, then: no rectangle (the window's own bounds),
        // only this window, without its shadow.
        let capture = dlsym(-2isize as *mut c_void, c"CGWindowListCreateImage".as_ptr());
        if capture.is_null() {
            return Err("this system has no CGWindowListCreateImage".into());
        }
        let capture: Capture = std::mem::transmute(capture);
        let null = Rect { x: f64::INFINITY, y: f64::INFINITY, w: 0.0, h: 0.0 };
        let image = capture(null, 1 << 3, number as u32, 1);
        if image.is_null() {
            return Err("the window could not be captured".into());
        }
        let url = CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 0);
        let png = CString::new("public.png").unwrap();
        let kind = CFStringCreateWithCString(std::ptr::null(), png.as_ptr(), 0x0800_0100);
        let dest = CGImageDestinationCreateWithURL(url, kind, 1, std::ptr::null());
        let written = if dest.is_null() {
            false
        } else {
            CGImageDestinationAddImage(dest, image, std::ptr::null());
            let ok = CGImageDestinationFinalize(dest) != 0;
            CFRelease(dest);
            ok
        };
        CFRelease(kind);
        CFRelease(url);
        CFRelease(image);
        if written { Ok(()) } else { Err(format!("could not write {}", path.display())) }
    }
}
#[cfg(not(target_os = "macos"))]
pub fn shoot_window(_window: &gpui::Window, _path: &std::path::Path) -> Result<(), String> {
    Err("a picture of the window is only taken on macOS".into())
}
