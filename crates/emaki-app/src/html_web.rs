//! An HTML file shown by the system's own web view (WKWebView on a Mac,
//! WebView2 on Windows), set in the file's pane as a child of the window:
//! the page as a browser has it, scripts and all. A page that lays
//! itself out by script is nothing without one, and no engine of our own
//! runs them.
//!
//! The view is the system's, not gpui's, so it is drawn over whatever
//! gpui draws where it stands. The window hides it while anything of its
//! own is open over the pane (`Workbench::html_web_sync`).
//!
//! A page's files come through a scheme of ours and not `file:`, so the
//! page reads the session's folder and nothing else on the disk. What it
//! asks of the network it gets, as in a browser.
//!
//! Linux has no web view that can be set in a gpui window (WebKitGTK
//! wants a GTK one), so there `open` gives none and the page is drawn by
//! Blitz (`html_shot.rs`).

use std::path::{Path, PathBuf};

use gpui::{Bounds, Pixels, Window};

/// The scheme the page's own files are asked for under.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const SCHEME: &str = "emakifile";

/// Where a file is, as the web view is told it. WebView2 has no schemes
/// of its own making and is given them as hosts.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn url_of(file: &Path) -> String {
    let mut path = String::new();
    for part in file.to_string_lossy().replace('\\', "/").split('/') {
        if !path.is_empty() || part.is_empty() {
            path.push('/');
        }
        for b in part.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => path.push(b as char),
                _ => path.push_str(&format!("%{b:02X}")),
            }
        }
    }
    let path = path.trim_start_matches('/');
    if cfg!(target_os = "windows") { format!("http://{SCHEME}.localhost/{path}") } else { format!("{SCHEME}://localhost/{path}") }
}

/// The file a request's path names: its escapes undone, and on Windows
/// the slash before the drive taken off.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn file_of(path: &str) -> PathBuf {
    let bytes = path.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(hi * 16 + lo);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    let text = String::from_utf8_lossy(&out).into_owned();
    if cfg!(target_os = "windows") { PathBuf::from(text.trim_start_matches('/')) } else { PathBuf::from(text) }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn mime_of(file: &Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" | "xhtml" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "txt" | "md" | "csv" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        _ => "application/octet-stream",
    }
}

/// The web view with a page in it: which file, and as of when.
pub struct Web {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    view: std::rc::Rc<wry::WebView>,
    pub file: PathBuf,
    pub mtime: Option<std::time::SystemTime>,
    placed: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    shown: bool,
    round: Option<(bool, bool, f32)>,
}

impl Web {
    /// A web view in `window` showing `file`, which reads files under
    /// `root` and no others. None where the system has none to give.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn open(window: &Window, file: &Path, mtime: Option<std::time::SystemTime>, root: &Path) -> Option<Web> {
        use std::borrow::Cow;
        use wry::http::Response;
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let answer = |status: u16, kind: &str, body: Vec<u8>| Response::builder().status(status).header("Content-Type", kind).header("Cache-Control", "no-store").body(Cow::Owned(body)).unwrap_or_else(|_| Response::new(Cow::Owned(Vec::new())));
        let ours = if cfg!(target_os = "windows") { format!("http://{SCHEME}.localhost/") } else { format!("{SCHEME}://") };
        let view = wry::WebViewBuilder::new()
            .with_custom_protocol(SCHEME.to_string(), move |_, request| {
                let file = file_of(request.uri().path());
                // Under the session's folder, links followed: a page is
                // not handed the rest of the disk.
                match file.canonicalize().ok().filter(|real| real.starts_with(&root)).and_then(|real| std::fs::read(&real).ok().map(|bytes| (real, bytes))) {
                    Some((real, bytes)) => answer(200, mime_of(&real), bytes),
                    None => answer(404, "text/plain", Vec::new()),
                }
            })
            // A link out of the page's own files is the system
            // browser's; the pane stays on the file.
            .with_navigation_handler({
                let ours = ours.clone();
                move |url| {
                    if url.starts_with(&ours) || url.starts_with("about:") || url.starts_with("data:") || url.starts_with("blob:") {
                        return true;
                    }
                    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
                        let _ = opener::open(&url);
                    }
                    false
                }
            })
            .with_new_window_req_handler(|url, _| {
                if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
                    let _ = opener::open(&url);
                }
                wry::NewWindowResponse::Deny
            })
            .with_focused(false)
            .with_visible(false)
            .with_url(url_of(file))
            .build_as_child(window)
            .ok()?;
        Some(Web { view: std::rc::Rc::new(view), file: file.to_path_buf(), mtime, placed: Default::default(), shown: false, round: None })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn open(_: &Window, _: &Path, _: Option<std::time::SystemTime>, _: &Path) -> Option<Web> {
        None
    }

    /// Another file, or the same one as it is now on the disk.
    pub fn load(&mut self, file: &Path, mtime: Option<std::time::SystemTime>) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let _ = self.view.load_url(&url_of(file));
        self.file = file.to_path_buf();
        self.mtime = mtime;
    }

    /// What puts it where the pane has its room, to be called as the
    /// pane is laid out: in that same frame, so it does not trail the
    /// pane by one.
    pub fn placer(&self) -> impl Fn(Bounds<Pixels>) + 'static {
        let placed = self.placed.clone();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let view = self.view.clone();
        move |at| {
            if placed.replace(Some(at)) == Some(at) {
                return;
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                use wry::dpi::{LogicalPosition, LogicalSize};
                let _ = view.set_bounds(wry::Rect {
                    position: LogicalPosition::new(f32::from(at.origin.x) as f64, f32::from(at.origin.y) as f64).into(),
                    size: LogicalSize::new(f32::from(at.size.width).max(1.) as f64, f32::from(at.size.height).max(1.) as f64).into(),
                });
            }
        }
    }

    /// Whether it has been put anywhere yet.
    pub fn is_placed(&self) -> bool {
        self.placed.get().is_some_and(|at| at.size.width > gpui::px(2.) && at.size.height > gpui::px(2.))
    }

    /// Its lower corners cut round where they are the card's own: the
    /// view is drawn over the window's drawing, the card's round corners
    /// with it.
    pub fn round(&mut self, left: bool, right: bool, r: f32) {
        if self.round == Some((left, right, r)) {
            return;
        }
        self.round = Some((left, right, r));
        #[cfg(target_os = "macos")]
        {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            use wry::WebViewExtMacOS as _;
            let web = self.view.webview();
            unsafe {
                let _: () = msg_send![&*web, setWantsLayer: true];
                let layer: *mut AnyObject = msg_send![&*web, layer];
                if !layer.is_null() {
                    // A layer's corners are named by its own y, and the
                    // web view's runs down: the lower two are "max y"
                    // (4 at the left, 8 at the right). Tried with each
                    // bit alone.
                    let which: usize = if left { 4 } else { 0 } | if right { 8 } else { 0 };
                    let _: () = msg_send![layer, setMasksToBounds: true];
                    let _: () = msg_send![layer, setMaskedCorners: which];
                    let _: () = msg_send![layer, setCornerRadius: if which == 0 { 0.0f64 } else { r as f64 }];
                }
            }
        }
    }

    /// The keyboard to the page, as a click in it gives it. For probes.
    pub fn take_keys(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let _ = self.view.focus();
    }

    /// Shown, or put out of sight while the window draws over the pane.
    pub fn show(&mut self, shown: bool) {
        if self.shown != shown {
            self.shown = shown;
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let _ = self.view.set_visible(shown);
        }
    }
}
