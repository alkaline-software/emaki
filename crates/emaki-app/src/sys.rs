//! The few things the window asks the operating system for: open a file
//! with whatever the system keeps for it, show it in the file manager, and
//! the person's name for the sidebar footer. Everything here has a Windows
//! and a Linux answer, so nothing above it needs to know which it is on.

use std::path::Path;

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
