//! The few things the window asks the operating system for: open a file
//! with whatever the system keeps for it, show it in the file manager, and
//! the person's name for the sidebar footer. Everything here has a Windows
//! and a Linux answer, so nothing above it needs to know which it is on.

use std::path::Path;

// Every function here that takes a session's pid answers for a hidden
// terminal of our own (`emaki_core::pty`), whose screen is in memory and
// whose keyboard is a write, on every platform. A session in a terminal
// app of the person's is not read, typed into or brought forward: no
// terminal app is asked anything, by its command line or by script, and
// none is ever started. The answer for such a session is `None` or the
// reason, and the thing is done in that terminal by the person.

/// Whether the session's process is on a hidden terminal of ours.
/// Returns the name shown for where it runs.
pub fn focus_terminal(pid: i32) -> Result<String, String> {
    if emaki_core::pty::for_pid(pid).is_some() {
        return Ok("Emaki".into());
    }
    Err("this session runs in a terminal of yours, which Emaki does not reach into".into())
}

/// What the session's hidden terminal is showing.
pub fn terminal_text(pid: i32) -> Option<String> {
    emaki_core::pty::for_pid(pid).map(|pty| pty.text())
}

/// The same screen with its colours.
pub fn terminal_styled(pid: i32) -> Option<String> {
    emaki_core::pty::for_pid(pid).map(|pty| pty.styled())
}

/// Type `text` into the session's hidden terminal and send it: the
/// slash command Claude Code only takes at its own prompt.
pub fn type_in_terminal(pid: i32, text: &str) -> Result<String, String> {
    if let Some(pty) = emaki_core::pty::for_pid(pid) {
        pty.write(format!("{text}\r").as_bytes());
        return Ok("Emaki".into());
    }
    Err(format!("this session runs in a terminal of yours: type {text} there"))
}

/// A key Claude Code's terminal takes that is not text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKey {
    /// Stops the running turn.
    Escape,
    /// Steps to the next permission mode.
    ShiftTab,
}

impl TerminalKey {
    /// The key as a terminal sends it.
    fn bytes(self) -> &'static [u8] {
        match self {
            TerminalKey::Escape => b"\x1b",
            // Back-tab: ESC [ Z.
            TerminalKey::ShiftTab => b"\x1b[Z",
        }
    }

    fn name(self) -> &'static str {
        match self {
            TerminalKey::Escape => "Escape",
            TerminalKey::ShiftTab => "\u{21e7}Tab",
        }
    }
}

/// Press `key` in the session's hidden terminal.
pub fn key_in_terminal(pid: i32, key: TerminalKey) -> Result<String, String> {
    if let Some(pty) = emaki_core::pty::for_pid(pid) {
        pty.write(key.bytes());
        return Ok("Emaki".into());
    }
    Err(format!("this session runs in a terminal of yours: press {} there", key.name()))
}

/// Send `text` to the session's hidden terminal as keys, with nothing
/// added: a digit that picks a choice in a dialog, Tab, Return, or words
/// for its text field.
pub fn text_in_terminal(pid: i32, text: &str) -> Result<(), String> {
    if let Some(pty) = emaki_core::pty::for_pid(pid) {
        pty.write(text.as_bytes());
        return Ok(());
    }
    Err("this session runs in a terminal of yours: answer it there".into())
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

/// What the reveal action is called where we are.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(target_os = "windows") {
    "Show in Explorer"
} else {
    "Show in folder"
};

/// The same two things on a session's menu, where each has to say what
/// it shows: the folder the session ran in, and its transcript.
pub const OPEN_SESSION_FOLDER_LABEL: &str = if cfg!(target_os = "macos") {
    "Open Folder in Finder"
} else if cfg!(target_os = "windows") {
    "Open Folder in Explorer"
} else {
    "Open Folder"
};
pub const REVEAL_SESSION_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal Transcript in Finder"
} else if cfg!(target_os = "windows") {
    "Show Transcript in Explorer"
} else {
    "Show Transcript in Folder"
};

/// What moving a file to the trash is called where we are, and the trash.
pub const TRASH_LABEL: &str = if cfg!(target_os = "windows") { "Move to Recycle Bin" } else { "Move to Trash" };
pub const TRASH_NAME: &str = if cfg!(target_os = "windows") { "Recycle Bin" } else { "Trash" };

/// Put a file on the clipboard as a file, so the file manager and any
/// other app can paste it. False where that is not done (off a Mac, for
/// now), and the caller keeps the path itself.
#[cfg(target_os = "macos")]
pub fn copy_file(path: &Path) -> bool {
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let object: objc2::rc::Retained<ProtocolObject<dyn NSPasteboardWriting>> = ProtocolObject::from_retained(url);
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    board.writeObjects(&NSArray::from_retained_slice(&[object]))
}

#[cfg(not(target_os = "macos"))]
pub fn copy_file(_: &Path) -> bool {
    false
}

/// Move `path` to the system's trash, where it can be put back from. On
/// macOS through the file manager's own call: the crate's default asks
/// Finder by AppleScript, which needs the person's leave to automate it.
pub fn trash_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos as _};
        let mut ctx = trash::TrashContext::default();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        ctx.delete(path).map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(|e| e.to_string())
    }
}

/// What opening a folder in the file manager is called where we are.
pub const OPEN_FOLDER_LABEL: &str = if cfg!(target_os = "macos") {
    "Open in Finder"
} else if cfg!(target_os = "windows") {
    "Open in Explorer"
} else {
    "Open folder"
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

/// Whether the system checks spelling for the window: the languages
/// Harper does not know are the system's, where it has a checker.
pub const SYSTEM_SPELLING: bool = cfg!(target_os = "macos");

/// The words of `text` the system's checker does not know in `language`,
/// each with what it would put there. The system's checker is the Mac's
/// (`NSSpellChecker`); Windows and Linux mark nothing. Call it on the
/// main thread.
#[cfg(target_os = "macos")]
pub fn spelling(text: &str, language: &str) -> Vec<emaki_core::check::Issue> {
    use emaki_core::check::{Issue, Kind};
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{NSRange, NSString};
    const MOST: usize = 60;
    let checker = NSSpellChecker::sharedSpellChecker();
    let (string, language) = (NSString::from_str(text), NSString::from_str(language));
    // The checker counts UTF-16 units; the window counts bytes.
    let mut bytes = Vec::with_capacity(text.len() + 1);
    for (i, c) in text.char_indices() {
        bytes.extend(std::iter::repeat_n(i, c.len_utf16()));
    }
    bytes.push(text.len());
    let mut out = Vec::new();
    let mut from = 0isize;
    while out.len() < MOST {
        let found: NSRange = unsafe { checker.checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(&string, from, Some(&language), false, 0, std::ptr::null_mut()) };
        if found.length == 0 || found.location >= bytes.len() {
            break;
        }
        let (Some(&start), Some(&end)) = (bytes.get(found.location), bytes.get(found.location + found.length)) else { break };
        let fixes = checker.guessesForWordRange_inString_language_inSpellDocumentWithTag(found, &string, Some(&language), 0).map(|guesses| guesses.iter().take(5).map(|g| g.to_string()).collect()).unwrap_or_default();
        out.push(Issue { range: start..end, kind: Kind::Spelling, message: String::new(), fixes, rule: String::new() });
        from = (found.location + found.length) as isize;
    }
    out
}

#[cfg(not(target_os = "macos"))]
pub fn spelling(_text: &str, _language: &str) -> Vec<emaki_core::check::Issue> {
    Vec::new()
}
