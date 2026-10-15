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
/// What the system's file manager is called.
pub const FILE_MANAGER: &str = if cfg!(target_os = "macos") {
    "Finder"
} else if cfg!(target_os = "windows") {
    "Explorer"
} else {
    "the file manager"
};

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

/// The keyboard given back to the window's own view, when something
/// else in the window has it or nothing does. A web view set in the
/// window (`html_web.rs`) takes it at a click, and AppKit leaves it on
/// the window itself when that view is hidden or let go: letters then
/// reach nothing, while Return and Delete, which AppKit offers to every
/// view first, still worked. True when it had to be moved.
#[cfg(target_os = "macos")]
pub fn keys_to_window(window: &gpui::Window) -> bool {
    // `EMAKI_KEYS_STAY=1` leaves them where they are, for a probe that
    // looks at where that is.
    if keys_at_window(window) || std::env::var_os("EMAKI_KEYS_STAY").is_some() {
        return false;
    }
    let Some(view) = ns_view(window) else { return false };
    let Some(native) = view.window() else { return false };
    native.makeFirstResponder(Some(view));
    true
}
#[cfg(not(target_os = "macos"))]
pub fn keys_to_window(_: &gpui::Window) -> bool {
    false
}

/// Whether the keyboard is the window's own view's. True where the
/// question does not come up.
#[cfg(target_os = "macos")]
pub fn keys_at_window(window: &gpui::Window) -> bool {
    use objc2_app_kit::{NSResponder, NSView};
    let Some(view) = ns_view(window) else { return true };
    let Some(native) = view.window() else { return true };
    let ours = view as *const NSView as *const ();
    native.firstResponder().is_some_and(|first| std::ptr::eq(&*first as *const NSResponder as *const (), ours))
}
#[cfg(not(target_os = "macos"))]
pub fn keys_at_window(_: &gpui::Window) -> bool {
    true
}

/// The window's own view, which gpui draws in and takes its keys by.
#[cfg(target_os = "macos")]
fn ns_view(window: &gpui::Window) -> Option<&objc2_app_kit::NSView> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    unsafe { (handle.ns_view.as_ptr() as *const objc2_app_kit::NSView).as_ref() }
}

/// Start the system's dictation in the field the keyboard is in, as
/// Edit, Start Dictation does: the words are typed there as they are
/// heard. Whether anything took the action.
#[cfg(target_os = "macos")]
pub fn start_dictation() -> bool {
    use objc2::{sel, MainThreadMarker};
    use objc2_app_kit::NSApplication;
    let Some(mtm) = MainThreadMarker::new() else { return false };
    unsafe { NSApplication::sharedApplication(mtm).sendAction_to_from(sel!(startDictation:), None, None) }
}
#[cfg(not(target_os = "macos"))]
pub fn start_dictation() -> bool {
    false
}

/// What the keyboard and the pointer do in this app, for dictation's key
/// when it is modifiers tapped alone (`dictate::TapWatch`): the Mac's
/// modifier flags when the keys held change, `None` for a key pressed or
/// a click. gpui says which modifiers are held but not on which side, so
/// this is AppKit's own monitor, which sees every event the app is sent
/// and lets it through as it was. Only while the app is the active one,
/// as any other key of the window's.
#[cfg(target_os = "macos")]
pub fn key_events() -> futures::channel::mpsc::UnboundedReceiver<Option<u64>> {
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
    let (tx, rx) = futures::channel::mpsc::unbounded();
    let mask = NSEventMask::FlagsChanged | NSEventMask::KeyDown | NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    let handler = block2::RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
        let e = unsafe { event.as_ref() };
        let said = (e.r#type() == NSEventType::FlagsChanged).then(|| e.modifierFlags().0 as u64);
        let _ = tx.unbounded_send(said);
        event.as_ptr()
    });
    // Kept for the app's life: AppKit holds the handler until the
    // monitor is removed, and it never is.
    std::mem::forget(unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) });
    rx
}

/// What the Mac does with Fn, the globe key, pressed alone: Keyboard
/// settings, "Press 🌐 key to" (`AppleFnUsageType` in HIToolbox's
/// domain). `None` when it does nothing; otherwise what it does, as a
/// clause ("show emoji and symbols"), empty when the value is not one
/// known here. A Mac that never had it set has a job for the key, so no
/// value is a job too. Asks `defaults`, so off the main thread.
#[cfg(target_os = "macos")]
pub fn fn_key_use() -> Option<&'static str> {
    let probe = FN_USE_PROBE.load(std::sync::atomic::Ordering::Relaxed);
    let value = if probe >= 0 {
        probe.to_string()
    } else {
        let out = std::process::Command::new("defaults").args(["read", "com.apple.HIToolbox", "AppleFnUsageType"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    match value.as_str() {
        "0" => None,
        "1" => Some("change the input source"),
        "2" => Some("show emoji and symbols"),
        "3" => Some("start Apple's dictation"),
        _ => Some(""),
    }
}
/// Elsewhere the key has no such setting.
#[cfg(not(target_os = "macos"))]
pub fn fn_key_use() -> Option<&'static str> {
    None
}

/// `EMAKI_GO=fnuse:<n>`: the value `fn_key_use` reads in place of the
/// Mac's, so the guidance can be seen without touching Keyboard settings.
pub static FN_USE_PROBE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Keyboard settings in System Settings, where Fn's use is set.
pub const KEYBOARD_SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.Keyboard-Settings.extension";

/// A sound of the app's own (a WAV file's bytes) played once, quietly:
/// dictation's start and stop. The last one played is held until the
/// next, or the system would let it go before it had sounded.
#[cfg(target_os = "macos")]
pub fn play_sound(wav: &'static [u8]) {
    use objc2::AllocAnyThread as _;
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSData;
    thread_local! {
        static PLAYING: std::cell::RefCell<Option<objc2::rc::Retained<NSSound>>> = const { std::cell::RefCell::new(None) };
    }
    let data = NSData::with_bytes(wav);
    if let Some(sound) = NSSound::initWithData(NSSound::alloc(), &data) {
        sound.setVolume(0.5);
        sound.play();
        PLAYING.with(|p| *p.borrow_mut() = Some(sound));
    }
}
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub fn play_sound(_wav: &'static [u8]) {}

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

/// The native window's own ground, under everything the app draws. gpui
/// leaves it the system's, near white in the light appearance, and the
/// drawing can stop a fraction of a point short of the window's foot: a
/// window as tall as the screen showed a light line under a dark frame.
/// Set to the frame's colour, whatever shows there is the frame.
#[cfg(target_os = "macos")]
pub fn set_window_ground(window: &gpui::Window, ground: gpui::Rgba) {
    use objc2_app_kit::{NSColor, NSView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    let Some(view) = (unsafe { (handle.ns_view.as_ptr() as *const NSView).as_ref() }) else { return };
    if let Some(native) = view.window() {
        let colour = NSColor::colorWithSRGBRed_green_blue_alpha(ground.r as f64, ground.g as f64, ground.b as f64, 1.);
        native.setBackgroundColor(Some(&colour));
    }
}
#[cfg(not(target_os = "macos"))]
pub fn set_window_ground(_window: &gpui::Window, _ground: gpui::Rgba) {}

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
        // `EMAKI_SHOT_FRAME=1` keeps what the system draws around the
        // window, its shadow and its rim.
        let framing = if std::env::var_os("EMAKI_SHOT_FRAME").is_some() { 0 } else { 1 };
        let image = capture(null, 1 << 3, number as u32, framing);
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

// Dictation in other apps. What is pressed while another app is in front
// is seen through an event tap, and the words go into that app by a paste:
// both need Accessibility, which the person grants in System Settings.


/// What was pressed while another app was in front (`outside_keys`).
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
pub enum Outside {
    /// The modifiers held changed: the Mac's flags.
    Flags(u64),
    /// Any other key, or a click.
    Other,
    /// Dictation's key, when it is a key and not a tap. Kept from the app.
    Key,
    /// Escape while dictation listens. Kept from the app.
    Escape,
}

#[cfg(target_os = "macos")]
mod outside {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

    type Ref = *mut c_void;
    type TapCallback = extern "C" fn(proxy: Ref, kind: u32, event: Ref, info: Ref) -> Ref;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: Ref) -> bool;
        static kAXTrustedCheckOptionPrompt: Ref;
        fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapCallback, info: Ref) -> Ref;
        fn CGEventTapEnable(tap: Ref, enable: bool);
        fn CGEventGetFlags(event: Ref) -> u64;
        fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
        fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Ref;
        fn CGEventSetFlags(event: Ref, flags: u64);
        fn CGEventPost(tap: u32, event: Ref);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(alloc: Ref, port: Ref, order: isize) -> Ref;
        fn CFRunLoopGetMain() -> Ref;
        fn CFRunLoopAddSource(rl: Ref, source: Ref, mode: Ref);
        fn CFDictionaryCreate(alloc: Ref, keys: *const Ref, values: *const Ref, n: isize, key_cb: Ref, value_cb: Ref) -> Ref;
        fn CFRelease(r: Ref);
        static kCFRunLoopCommonModes: Ref;
        static kCFBooleanTrue: Ref;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
    }

    const SESSION_TAP: u32 = 1;
    const HID_TAP: u32 = 0;
    const KEY_DOWN: u32 = 10;
    const FLAGS_CHANGED: u32 = 12;
    const DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
    const KEYCODE_FIELD: u32 = 9;
    const AUTOREPEAT_FIELD: u32 = 8;
    const ESCAPE: i64 = 53;
    /// ⌘, ⇧, ⌃ and ⌥, of the Mac's flags.
    pub const MODS: u64 = 0x10_0000 | 0x2_0000 | 0x4_0000 | 0x8_0000;

    static PORT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
    /// Dictation's key as the key's code and the modifiers with it; 0 for
    /// none (a tap, or nothing bound). The code is in the low 16 bits.
    pub static KEY: AtomicU64 = AtomicU64::new(0);
    pub static ESCAPE_TAKEN: AtomicBool = AtomicBool::new(false);

    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    /// Asks the system to list the app in Accessibility, with its own
    /// prompt the first time.
    pub fn ask_trust() -> bool {
        unsafe {
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [kCFBooleanTrue];
            let options = CFDictionaryCreate(std::ptr::null_mut(), keys.as_ptr(), values.as_ptr(), 1, &kCFTypeDictionaryKeyCallBacks as *const c_void as Ref, &kCFTypeDictionaryValueCallBacks as *const c_void as Ref);
            let ok = AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
            ok
        }
    }

    extern "C" fn seen(_proxy: Ref, kind: u32, event: Ref, info: Ref) -> Ref {
        if kind == DISABLED_BY_TIMEOUT || kind == DISABLED_BY_USER {
            let port = PORT.load(Ordering::Relaxed);
            if !port.is_null() {
                unsafe { CGEventTapEnable(port, true) };
            }
            return event;
        }
        // The app's own keys are its local monitor's (`key_events`).
        if super::app_is_active() {
            return event;
        }
        let tx = unsafe { &*(info as *const futures::channel::mpsc::UnboundedSender<super::Outside>) };
        let said = match kind {
            FLAGS_CHANGED => super::Outside::Flags(unsafe { CGEventGetFlags(event) }),
            KEY_DOWN => {
                let code = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) };
                let mods = unsafe { CGEventGetFlags(event) } & MODS;
                let key = KEY.load(Ordering::Relaxed);
                if code == ESCAPE && mods == 0 && ESCAPE_TAKEN.load(Ordering::Relaxed) {
                    let _ = tx.unbounded_send(super::Outside::Escape);
                    return std::ptr::null_mut();
                }
                if key != 0 && code as u64 == key & 0xFFFF && mods == key & MODS {
                    if unsafe { CGEventGetIntegerValueField(event, AUTOREPEAT_FIELD) } == 0 {
                        let _ = tx.unbounded_send(super::Outside::Key);
                    }
                    return std::ptr::null_mut();
                }
                super::Outside::Other
            }
            _ => super::Outside::Other,
        };
        let _ = tx.unbounded_send(said);
        event
    }

    /// The tap, on the main run loop, for the app's life. False when the
    /// system refuses it: Accessibility is not granted.
    pub fn start(tx: futures::channel::mpsc::UnboundedSender<super::Outside>) -> bool {
        let mask = [KEY_DOWN, FLAGS_CHANGED, 1, 3, 25].iter().fold(0u64, |m, t| m | 1 << t);
        let info = Box::into_raw(Box::new(tx)) as Ref;
        let port = unsafe { CGEventTapCreate(SESSION_TAP, 0, 0, mask, seen, info) };
        if port.is_null() {
            drop(unsafe { Box::from_raw(info as *mut futures::channel::mpsc::UnboundedSender<super::Outside>) });
            return false;
        }
        PORT.store(port, Ordering::Relaxed);
        unsafe {
            let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), port, 0);
            CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
            CFRelease(source);
            CGEventTapEnable(port, true);
        }
        true
    }

    /// ⌘V, as if pressed.
    pub fn press_paste() {
        const V: u16 = 9;
        const CMD: u64 = 0x10_0000;
        unsafe {
            for down in [true, false] {
                let e = CGEventCreateKeyboardEvent(std::ptr::null_mut(), V, down);
                if e.is_null() {
                    return;
                }
                CGEventSetFlags(e, CMD);
                CGEventPost(HID_TAP, e);
                CFRelease(e);
            }
        }
    }
}

/// Whether this app is the one in front.
#[cfg(target_os = "macos")]
fn app_is_active() -> bool {
    use objc2_app_kit::NSRunningApplication;
    NSRunningApplication::currentApplication().isActive()
}

/// Whether the app may watch keys pressed in other apps and press keys in
/// them: Accessibility, in System Settings.
#[cfg(target_os = "macos")]
pub fn trusted() -> bool {
    outside::trusted()
}

/// A message each time Accessibility's list changes, for any app: the
/// system's own notice, so nothing asks over and over.
#[cfg(target_os = "macos")]
pub fn trust_changes() -> futures::channel::mpsc::UnboundedReceiver<()> {
    use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSOperationQueue, NSString};
    let (tx, rx) = futures::channel::mpsc::unbounded();
    let block = block2::RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
        let _ = tx.unbounded_send(());
    });
    let center = NSDistributedNotificationCenter::defaultCenter();
    let observer = unsafe { center.addObserverForName_object_queue_usingBlock(Some(&NSString::from_str("com.apple.accessibility.api")), None, Some(&NSOperationQueue::mainQueue()), &block) };
    // Kept for the app's life, as the key monitor is.
    std::mem::forget(observer);
    rx
}

/// The system's own question, which puts the app in Accessibility's
/// list and offers to open it there.
#[cfg(target_os = "macos")]
pub fn ask_trust() -> bool {
    outside::ask_trust()
}
#[cfg(not(target_os = "macos"))]
pub fn ask_trust() -> bool {
    false
}

/// What is pressed while another app is in front, for dictation started
/// and stopped there. None until Accessibility is granted; asked again
/// once it is.
#[cfg(target_os = "macos")]
pub fn outside_keys() -> Option<futures::channel::mpsc::UnboundedReceiver<Outside>> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    outside::start(tx).then_some(rx)
}

/// Dictation's key as the tap matches it in other apps: a gpui binding
/// ("cmd-shift-d"), or None for a tap of modifiers or a key that has no
/// code here.
#[cfg(target_os = "macos")]
pub fn set_outside_key(binding: Option<&gpui::Keystroke>) {
    let code = binding.and_then(|k| key_code(&k.key).map(|c| (c, k)));
    let packed = code.map_or(0, |(c, k)| {
        let m = &k.modifiers;
        let mut mods = 0u64;
        if m.platform {
            mods |= 0x10_0000;
        }
        if m.shift {
            mods |= 0x2_0000;
        }
        if m.control {
            mods |= 0x4_0000;
        }
        if m.alt {
            mods |= 0x8_0000;
        }
        mods | c as u64
    });
    outside::KEY.store(packed, std::sync::atomic::Ordering::Relaxed);
}
#[cfg(not(target_os = "macos"))]
pub fn set_outside_key(_binding: Option<&gpui::Keystroke>) {}

/// Escape in another app cancels dictation, and is kept from that app,
/// while this is on.
#[cfg(target_os = "macos")]
pub fn take_escape(on: bool) {
    outside::ESCAPE_TAKEN.store(on, std::sync::atomic::Ordering::Relaxed);
}
#[cfg(not(target_os = "macos"))]
pub fn take_escape(_on: bool) {}

/// The Mac's code for a key as gpui names it, on the US layout, as the
/// system's own shortcuts are.
#[cfg(target_os = "macos")]
fn key_code(key: &str) -> Option<u16> {
    const LETTERS: [u16; 26] = [0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6];
    const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    const F: [u16; 12] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Some(LETTERS[(c as u8 - b'a') as usize]);
        }
        if c.is_ascii_digit() {
            return Some(DIGITS[(c as u8 - b'0') as usize]);
        }
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=12).contains(n)) {
        return Some(F[n - 1]);
    }
    Some(match key {
        "space" => 49,
        "enter" => 36,
        "tab" => 48,
        "-" => 27,
        "=" => 24,
        "[" => 33,
        "]" => 30,
        "\\" => 42,
        ";" => 41,
        "'" => 39,
        "," => 43,
        "." => 47,
        "/" => 44,
        "`" => 50,
        _ => return None,
    })
}

/// What the general pasteboard holds, every item in every type, to put
/// back after a paste of our own.
#[cfg(target_os = "macos")]
pub struct Clipboard {
    items: Vec<Vec<(String, Vec<u8>)>>,
    count: isize,
}

/// The words pasted into the app in front, by the pasteboard and ⌘V. The
/// pasteboard is marked as passing through, which clipboard managers skip;
/// what it held is returned, to be put back once the app has read it.
#[cfg(target_os = "macos")]
pub fn paste_words(words: &str) -> Clipboard {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSData, NSString};
    let board = NSPasteboard::generalPasteboard();
    let mut items = Vec::new();
    if let Some(held) = board.pasteboardItems() {
        for item in held.iter() {
            let mut kept = Vec::new();
            for kind in item.types().iter() {
                if let Some(data) = item.dataForType(&kind) {
                    kept.push((kind.to_string(), data.to_vec()));
                }
            }
            items.push(kept);
        }
    }
    board.clearContents();
    unsafe {
        board.setString_forType(&NSString::from_str(words), NSPasteboardTypeString);
        board.setData_forType(Some(&NSData::new()), &NSString::from_str("org.nspasteboard.TransientType"));
    }
    let count = board.changeCount();
    outside::press_paste();
    Clipboard { items, count }
}

/// The pasteboard as it was before `paste_words`, unless something else
/// has been copied since.
#[cfg(target_os = "macos")]
pub fn restore_clipboard(was: Clipboard) {
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSData, NSString};
    let board = NSPasteboard::generalPasteboard();
    if board.changeCount() != was.count {
        return;
    }
    board.clearContents();
    if was.items.is_empty() {
        return;
    }
    let objects: Vec<objc2::rc::Retained<ProtocolObject<dyn NSPasteboardWriting>>> = was
        .items
        .iter()
        .map(|kinds| {
            let item = NSPasteboardItem::new();
            for (kind, data) in kinds {
                item.setData_forType(&NSData::with_bytes(data), &NSString::from_str(kind));
            }
            ProtocolObject::from_retained(item)
        })
        .collect();
    board.writeObjects(&NSArray::from_retained_slice(&objects));
}

/// Where the pill that shows dictation in other apps goes: the screen
/// under the pointer, by its display's id and width, and its notch when
/// it has one, as the width of the menu bar on either side of it and the
/// notch's height.
#[cfg(target_os = "macos")]
pub struct PillPlace {
    pub display: u32,
    pub width: f32,
    pub notch: Option<(f32, f32, f32)>,
}

#[cfg(target_os = "macos")]
pub fn pill_place() -> Option<PillPlace> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};
    use objc2_foundation::{NSNumber, NSString};
    let mtm = MainThreadMarker::new()?;
    let at = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| {
            let f = s.frame();
            at.x >= f.origin.x && at.x < f.origin.x + f.size.width && at.y >= f.origin.y && at.y < f.origin.y + f.size.height
        })
        .or_else(|| screens.firstObject())?;
    let id = screen.deviceDescription().objectForKey(&NSString::from_str("NSScreenNumber"))?;
    let id = id.downcast_ref::<NSNumber>()?.unsignedIntValue();
    let width = screen.frame().size.width as f32;
    let top = screen.safeAreaInsets().top as f32;
    let notch = (top > 0.).then(|| (screen.auxiliaryTopLeftArea().size.width as f32, screen.auxiliaryTopRightArea().size.width as f32, top));
    Some(PillPlace { display: id, width, notch })
}

/// The pill's window made to stay over every app and every space, never
/// in the way of the pointer, and drawn with no shadow of the system's.
/// It has no title bar, so the system lets it stand over the menu bar,
/// against the screen's top edge.
#[cfg(target_os = "macos")]
pub fn float_pill(window: &gpui::Window) {
    use objc2_app_kit::NSWindowStyleMask;
    let Some(view) = ns_view(window) else { return };
    let Some(native) = view.window() else { return };
    native.setIgnoresMouseEvents(true);
    native.setHidesOnDeactivate(false);
    native.setHasShadow(false);
    native.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
    if let Some(screen) = native.screen() {
        let mut frame = native.frame();
        let top = screen.frame();
        frame.origin.y = top.origin.y + top.size.height - frame.size.height;
        native.setFrame_display(frame, true);
    }
}

/// The window kept over every other app's, for a probe.
#[cfg(target_os = "macos")]
pub fn keep_on_top(window: &gpui::Window) {
    if let Some(native) = ns_view(window).and_then(|v| v.window()) {
        native.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    }
}
#[cfg(not(target_os = "macos"))]
pub fn keep_on_top(_window: &gpui::Window) {}
