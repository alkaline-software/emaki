//! The few things the window asks the operating system for: open a file
//! with whatever the system keeps for it, show it in the file manager, and
//! the person's name for the sidebar footer. Everything here has a Windows
//! and a Linux answer, so nothing above it needs to know which it is on.

use std::path::Path;

use crate::i18n::{phrase, t, tf};

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
    Err(t("this session runs in a terminal of yours, which Emaki does not reach into").into())
}

/// What the session's hidden terminal is showing.
pub fn terminal_text(pid: i32) -> Option<String> {
    emaki_core::pty::for_pid(pid).map(|pty| pty.text())
}

/// The same screen with its colors.
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
    Err(tf("this session runs in a terminal of yours: type {text} there", &[("text", &text)]))
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
    Err(tf("this session runs in a terminal of yours: press {key} there", &[("key", &key.name())]))
}

/// Send `text` to the session's hidden terminal as keys, with nothing
/// added: a digit that picks a choice in a dialog, Tab, Return, or words
/// for its text field.
pub fn text_in_terminal(pid: i32, text: &str) -> Result<(), String> {
    if let Some(pty) = emaki_core::pty::for_pid(pid) {
        pty.write(text.as_bytes());
        return Ok(());
    }
    Err(t("this session runs in a terminal of yours: answer it there").into())
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
        let _ = emaki_core::process::command("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = emaki_core::process::command("explorer").arg(format!("/select,{}", path.display())).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(parent) = path.parent() {
            let _ = opener::open(parent);
        }
    }
}

/// What the reveal action is called where we are. These names are drawn
/// through `t` where they are shown.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    phrase("Reveal in Finder")
} else if cfg!(target_os = "windows") {
    phrase("Show in Explorer")
} else {
    phrase("Show in folder")
};

/// The same two things on a session's menu, where each has to say what
/// it shows: the folder the session ran in, and its transcript.
pub const OPEN_SESSION_FOLDER_LABEL: &str = if cfg!(target_os = "macos") {
    phrase("Open Folder in Finder")
} else if cfg!(target_os = "windows") {
    phrase("Open Folder in Explorer")
} else {
    phrase("Open Folder")
};
pub const REVEAL_SESSION_LABEL: &str = if cfg!(target_os = "macos") {
    phrase("Reveal Transcript in Finder")
} else if cfg!(target_os = "windows") {
    phrase("Show Transcript in Explorer")
} else {
    phrase("Show Transcript in Folder")
};

/// What moving a file to the trash is called where we are.
pub const TRASH_LABEL: &str = if cfg!(target_os = "windows") { phrase("Move to Recycle Bin") } else { phrase("Move to Trash") };

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
    phrase("Open in Finder")
} else if cfg!(target_os = "windows") {
    phrase("Open in Explorer")
} else {
    phrase("Open folder")
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
/// Set to the frame's color, whatever shows there is the frame.
#[cfg(target_os = "macos")]
pub fn set_window_ground(window: &gpui::Window, ground: gpui::Rgba) {
    use objc2_app_kit::{NSColor, NSView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    let Some(view) = (unsafe { (handle.ns_view.as_ptr() as *const NSView).as_ref() }) else { return };
    if let Some(native) = view.window() {
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(ground.r as f64, ground.g as f64, ground.b as f64, 1.);
        native.setBackgroundColor(Some(&color));
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

/// Where gpui puts the traffic lights' row on the main window: the close
/// button's top left, from the window's top left (`main.rs`).
pub const LIGHTS_ROW: (f32, f32) = (17., 20.);

/// The main window's traffic lights between gpui's row (`k` 1) and a
/// column at the rail's head about `mid` (`k` 0), as the sidebar folds
/// (`render_rail`). gpui puts them back in its row at a resize, a change
/// of key window or of appearance and more, so each button and their
/// container are watched, and moved again whenever anything moves them.
#[cfg(target_os = "macos")]
pub fn fold_lights(window: &gpui::Window, k: f32, mid: f32) {
    let Some(native) = ns_view(window).and_then(|v| v.window()) else { return };
    lights::FOLD.set((k, mid));
    lights::watch(&native);
    lights::place(&native);
}
#[cfg(not(target_os = "macos"))]
pub fn fold_lights(_: &gpui::Window, _: f32, _: f32) {}

#[cfg(target_os = "macos")]
mod lights {
    use std::cell::{Cell, RefCell};
    use std::ptr::NonNull;

    use objc2::rc::{Retained, Weak};
    use objc2::runtime::ProtocolObject;
    use objc2::Message as _;
    use objc2_app_kit::{NSButton, NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowButton, NSWindowStyleMask};
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize};

    thread_local! {
        pub(super) static FOLD: Cell<(f32, f32)> = const { Cell::new((1., 0.)) };
        /// The distance from one button to the next in the system's row,
        /// read while they stand in it.
        static PITCH: Cell<Option<f64>> = const { Cell::new(None) };
        /// Set while the lights are being moved here, so the moves they
        /// post are not answered.
        static MOVING: Cell<bool> = const { Cell::new(false) };
        static WATCHING: RefCell<(usize, Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>)> = const { RefCell::new((0, Vec::new())) };
    }

    fn buttons(native: &NSWindow) -> Option<[Retained<NSButton>; 3]> {
        Some([
            native.standardWindowButton(NSWindowButton::CloseButton)?,
            native.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
            native.standardWindowButton(NSWindowButton::ZoomButton)?,
        ])
    }

    /// The view the buttons stand in, and the title bar's container
    /// around it, which gpui sizes to the row.
    fn holders(close: &NSButton) -> (Option<Retained<NSView>>, Option<Retained<NSView>>) {
        // SAFETY: the button is the window's own, from
        // `standardWindowButton`, and each view is returned retained.
        let view = unsafe { close.superview() };
        let container = view.as_ref().and_then(|v| unsafe { v.superview() });
        (view, container)
    }

    /// Each button and their container post their moves to `place`.
    /// AppKit can make the buttons anew, and new ones are watched then.
    pub(super) fn watch(native: &NSWindow) {
        let Some(b) = buttons(native) else { return };
        let key = Retained::as_ptr(&b[0]) as usize;
        if WATCHING.with_borrow(|w| w.0 == key) {
            return;
        }
        let center = NSNotificationCenter::defaultCenter();
        WATCHING.with_borrow_mut(|w| {
            for old in w.1.drain(..) {
                unsafe { center.removeObserver(AsRef::<objc2::runtime::AnyObject>::as_ref(&*old)) };
            }
        });
        let weak = Weak::from_retained(&native.retain());
        let block = block2::RcBlock::new(move |_: NonNull<NSNotification>| {
            if let Some(native) = weak.load() {
                place(&native);
            }
        });
        let container = holders(&b[0]).1;
        let views: Vec<Retained<NSView>> = b.iter().map(|b| Retained::into_super(Retained::into_super(b.clone()))).chain(container).collect();
        let tokens = views
            .iter()
            .map(|v| {
                v.setPostsFrameChangedNotifications(true);
                unsafe { center.addObserverForName_object_queue_usingBlock(Some(NSViewFrameDidChangeNotification), Some(v), None, &block) }
            })
            .collect();
        WATCHING.set((key, tokens));
    }

    /// The buttons where the fold puts them. In full screen they are the
    /// system's, as gpui leaves them.
    pub(super) fn place(native: &NSWindow) {
        if MOVING.get() || native.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return;
        }
        let Some(b) = buttons(native) else { return };
        let (Some(view), Some(container)) = holders(&b[0]) else { return };
        let (close, mini) = (b[0].frame(), b[1].frame());
        if (close.origin.y - mini.origin.y).abs() < 0.01 && mini.origin.x > close.origin.x {
            PITCH.set(Some(mini.origin.x - close.origin.x));
        }
        let Some(pitch) = PITCH.get() else { return };
        let (k, mid) = FOLD.get();
        let (k, mid) = (k as f64, mid as f64);
        let (row_x, row_y) = (super::LIGHTS_ROW.0 as f64, super::LIGHTS_ROW.1 as f64);
        let (w, h) = (close.size.width, close.size.height);
        let col_x = mid - w / 2.;
        // Each button's left and its top under the window's top.
        let at = |i: f64| (col_x + (row_x + pitch * i - col_x) * k, row_y + pitch * i * (1. - k));
        // The container holds the buttons, which take a click only
        // inside it: as tall as the column, and while the column is
        // there no wider than it, so it is not over the card's top.
        let window = native.frame().size;
        let height = at(2.).1 + h + row_y;
        let width = if k >= 1. { window.width } else { at(2.).0 + w + row_x };
        MOVING.set(true);
        let mut moved = false;
        let mut frame: NSRect = container.frame();
        if (frame.size.height - height).abs() > 0.01 || (frame.origin.y - (window.height - height)).abs() > 0.01 || (frame.size.width - width).abs() > 0.01 {
            frame.size = NSSize::new(width, height);
            frame.origin.y = window.height - height;
            container.setFrame(frame);
            moved = true;
        }
        let (flipped, inside) = (view.isFlipped(), view.bounds().size.height);
        for (i, button) in b.iter().enumerate() {
            let (x, y) = at(i as f64);
            let origin = NSPoint::new(x, if flipped { y } else { inside - y - h });
            let now = button.frame().origin;
            if (now.x - origin.x).abs() > 0.01 || (now.y - origin.y).abs() > 0.01 {
                button.setFrameOrigin(origin);
                button.updateTrackingAreas();
                moved = true;
            }
        }
        if moved {
            container.updateTrackingAreas();
        }
        MOVING.set(false);
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

// The window's language. On a Mac an app's language is its own
// `AppleLanguages` in its defaults, which System Settings, Language &
// Region, Applications writes. Emaki's setting is the same one, as
// Kotoba's is.

#[cfg(target_os = "macos")]
mod languages {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSBundle, NSString, NSUserDefaults};

    const KEY: &str = "AppleLanguages";

    /// `AppleLanguages` as one domain of the defaults has it, alone: not
    /// the search list's, which falls back from the app's to the system's.
    fn list(domain: &str) -> Vec<String> {
        let defaults = NSUserDefaults::standardUserDefaults();
        let Some(dict) = defaults.persistentDomainForName(&NSString::from_str(domain)) else { return Vec::new() };
        let Some(value) = dict.objectForKey(&NSString::from_str(KEY)) else { return Vec::new() };
        let Ok(all) = value.downcast::<NSArray>() else { return Vec::new() };
        all.iter().filter_map(|o| o.downcast::<NSString>().ok().map(|s| s.to_string())).collect()
    }

    pub fn system() -> Vec<String> {
        list("NSGlobalDomain")
    }

    /// A bare executable (a probe, `cargo run`) has no bundle and so no
    /// domain of its own: it follows the system's.
    pub fn own() -> Option<String> {
        let id = NSBundle::mainBundle().bundleIdentifier()?;
        list(&id.to_string()).into_iter().next()
    }

    pub fn set_own(code: Option<&str>) {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return;
        }
        let defaults = NSUserDefaults::standardUserDefaults();
        let key = NSString::from_str(KEY);
        match code {
            Some(code) => {
                let one = NSArray::from_retained_slice(&[NSString::from_str(code)]);
                let one: &AnyObject = &one;
                unsafe { defaults.setObject_forKey(Some(one), &key) };
            }
            None => defaults.removeObjectForKey(&key),
        }
    }
}

/// The system's preferred languages, in its order, apart from any the
/// app was given of its own.
#[cfg(target_os = "macos")]
pub fn system_languages() -> Vec<String> {
    let all = languages::system();
    if all.is_empty() { sys_locale::get_locales().collect() } else { all }
}
#[cfg(not(target_os = "macos"))]
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// The language the app was given of its own, if any: by Emaki's
/// setting or by System Settings.
#[cfg(target_os = "macos")]
pub fn own_language() -> Option<String> {
    languages::own()
}
#[cfg(not(target_os = "macos"))]
pub fn own_language() -> Option<String> {
    None
}

/// The app given a language of its own (a bundle's code, `zh-Hans`), or
/// none, to follow the system's.
#[cfg(target_os = "macos")]
pub fn set_own_language(code: Option<&str>) {
    languages::set_own(code);
}
#[cfg(not(target_os = "macos"))]
pub fn set_own_language(_code: Option<&str>) {}

/// Keep the display on, or let it sleep again: held while a session
/// works and the setting is on (`AppConfig::keep_display_on`), as a
/// video playing holds it. Asked of the system on the main thread, each
/// change once. On a Mac an IOKit assertion against idle display sleep;
/// on Windows the thread's execution state, which lasts while the main
/// thread does; on Linux `systemd-inhibit` holding off idleness for as
/// long as its child runs, and nothing where there is no systemd.
pub fn hold_display(on: bool) {
    use std::sync::Mutex;
    static HELD: Mutex<Option<display::Held>> = Mutex::new(None);
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    match (on, held.is_some()) {
        (true, false) => *held = display::hold(),
        (false, true) => {
            if let Some(h) = held.take() {
                display::release(h);
            }
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
mod display {
    use std::ffi::{c_char, c_void};

    pub struct Held(u32);

    #[link(name = "IOKit", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(kind: *mut c_void, level: u32, name: *mut c_void, id: *mut u32) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
        fn CFStringCreateWithCString(alloc: *const c_void, text: *const c_char, encoding: u32) -> *mut c_void;
        fn CFRelease(obj: *mut c_void);
    }

    pub fn hold() -> Option<Held> {
        const UTF8: u32 = 0x0800_0100;
        const LEVEL_ON: u32 = 255;
        unsafe {
            let kind = CFStringCreateWithCString(std::ptr::null(), c"PreventUserIdleDisplaySleep".as_ptr(), UTF8);
            let name = CFStringCreateWithCString(std::ptr::null(), c"Emaki: a session is working".as_ptr(), UTF8);
            let mut id = 0u32;
            let done = !kind.is_null() && !name.is_null() && IOPMAssertionCreateWithName(kind, LEVEL_ON, name, &mut id) == 0;
            for s in [kind, name].into_iter().filter(|s| !s.is_null()) {
                CFRelease(s);
            }
            done.then_some(Held(id))
        }
    }

    pub fn release(h: Held) {
        unsafe {
            IOPMAssertionRelease(h.0);
        }
    }
}

#[cfg(windows)]
mod display {
    pub struct Held;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }
    const CONTINUOUS: u32 = 0x8000_0000;
    const SYSTEM_REQUIRED: u32 = 0x0000_0001;
    const DISPLAY_REQUIRED: u32 = 0x0000_0002;

    pub fn hold() -> Option<Held> {
        (unsafe { SetThreadExecutionState(CONTINUOUS | SYSTEM_REQUIRED | DISPLAY_REQUIRED) } != 0).then_some(Held)
    }

    pub fn release(_: Held) {
        unsafe {
            SetThreadExecutionState(CONTINUOUS);
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod display {
    pub struct Held(std::process::Child);

    pub fn hold() -> Option<Held> {
        emaki_core::process::command("systemd-inhibit")
            .args(["--what=idle", "--who=Emaki", "--why=A session is working", "--mode=block", "sleep", "infinity"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
            .map(Held)
    }

    pub fn release(mut h: Held) {
        let _ = h.0.kill();
        let _ = h.0.wait();
    }
}
