//! The conversation's typefaces. Anthropic Serif and Anthropic Sans are the
//! Claude desktop app's own fonts and not ours to ship, so they are loaded
//! at start from a Claude app installed on this machine, when there is one.
//! Without it the serif falls back to Georgia and the sans to the window's
//! own face, and the settings panel says so.

use std::path::PathBuf;

use gpui::{App, Global, SharedString};

pub const SERIF_FALLBACK: &str = "Georgia";

/// macOS's monospaced system face, SF Mono, by the name CoreText knows it
/// under. "SF Mono" itself is a family only where someone installed it.
const SYSTEM_MONO: &str = ".AppleSystemUIFontMonospaced";

/// Anthropic Mono with a larger em, so it draws at 0.9 of the size asked
/// for: `scripts/anthropic-mono.py` makes it. See `inline_code_family`.
const INLINE_MONO: &str = "Inline Anthropic Mono";

/// The faces the name in the sidebar is tried in, most wanted first: a
/// light, elegant sans (Optima's flared strokes, else Avenir Next; both
/// come with macOS). With none of them, the window's face.
const WORDMARK_FACES: &[&str] = &["Optima", "Avenir Next", "Avenir"];

/// The family names the text system knows the two fonts by, once loaded.
#[derive(Debug, Clone, Default)]
pub struct ChatFonts {
    pub serif: Option<String>,
    pub sans: Option<String>,
    /// The face code is set in, when it is not the toolkit's own.
    pub code: Option<String>,
    /// The face for inline code, when the machine has the smaller copy.
    pub inline_code: Option<String>,
    /// The first of `WORDMARK_FACES` the system has.
    pub wordmark: Option<String>,
    /// Where they came from, for the settings panel.
    pub source: Option<PathBuf>,
}

impl Global for ChatFonts {}

/// Where a Claude desktop app keeps its fonts, per OS.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/Applications/Claude.app/Contents/Resources/fonts"));
        dirs.push(emaki_core::paths::home().join("Applications/Claude.app/Contents/Resources/fonts"));
    } else if cfg!(target_os = "windows") {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            // Squirrel installs: AnthropicClaude/app-<version>/resources/fonts
            if let Ok(rd) = std::fs::read_dir(local.join("AnthropicClaude")) {
                let mut versions: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("app-")).unwrap_or(false)).collect();
                versions.sort();
                for v in versions.into_iter().rev() {
                    dirs.push(v.join("resources").join("fonts"));
                }
            }
            dirs.push(local.join("Programs").join("Claude").join("resources").join("fonts"));
        }
    }
    dirs
}

fn pick(names: &[String], family: &str) -> Option<String> {
    if names.iter().any(|n| n == family) {
        return Some(family.into());
    }
    names.iter().find(|n| n.starts_with(family)).cloned()
}

/// The font files in `dir`, if any.
fn font_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf")).unwrap_or(false))
        .collect();
    files.sort();
    files
}

/// Make the fonts in `files` known to the text system. The Claude app ships
/// each face as one variable font, and gpui matches a weight against the
/// faces it holds, so handed the bytes it would hold one face per file (the
/// default instance, Regular) and draw every bold as regular. On macOS the
/// files are registered with CoreText for this process instead: CoreText
/// lists a variable font's named instances (Light, Medium, Semibold, Bold,
/// their italics) as faces of their own, and gpui's family lookup falls
/// through to the system source when it was not handed a family, so bold
/// finds a bold. Elsewhere the bytes go in as they are.
fn load(files: &[PathBuf], cx: &mut App) -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSString, NSURL};
        #[link(name = "CoreText", kind = "framework")]
        unsafe extern "C" {
            fn CTFontManagerRegisterFontsForURL(url: *const std::ffi::c_void, scope: u32, error: *mut *const std::ffi::c_void) -> u8;
        }
        const SCOPE_PROCESS: u32 = 1;
        let mut any = false;
        for f in files {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&f.to_string_lossy()));
            // A file already registered (the app's own fonts installed by hand,
            // say) answers false and is usable all the same; the family check
            // afterwards is what counts.
            let ok = unsafe { CTFontManagerRegisterFontsForURL(&*url as *const NSURL as *const std::ffi::c_void, SCOPE_PROCESS, std::ptr::null_mut()) };
            any |= ok != 0;
        }
        let _ = cx;
        any || !files.is_empty()
    }
    #[cfg(not(target_os = "macos"))]
    {
        use std::borrow::Cow;
        let data: Vec<Cow<'static, [u8]>> = files.iter().filter_map(|p| std::fs::read(p).ok()).map(Cow::Owned).collect();
        !data.is_empty() && cx.text_system().add_fonts(data).is_ok()
    }
}

/// Load the Claude app's fonts into the text system, if the app is here.
pub fn install(cx: &mut App) {
    let mut fonts = ChatFonts::default();
    for dir in font_dirs() {
        let files = font_files(&dir);
        if files.is_empty() || !load(&files, cx) {
            continue;
        }
        fonts.source = Some(dir);
        break;
    }
    // Fonts the person keeps for Emaki alone, in `~/.emaki/fonts`. This is
    // where Anthropic Mono goes: the Claude app fetches it from Anthropic's
    // servers when it runs and keeps no file for it in its bundle, so
    // there is nothing to load it from the way the serif and the sans are.
    let own = font_files(&emaki_core::paths::root().join("fonts"));
    if !own.is_empty() {
        load(&own, cx);
    }
    let names = cx.text_system().all_font_names();
    if fonts.source.is_some() {
        fonts.serif = pick(&names, "Anthropic Serif");
        fonts.sans = pick(&names, "Anthropic Sans");
    }
    fonts.code = pick(&names, "Anthropic Mono").or_else(|| cfg!(target_os = "macos").then(|| SYSTEM_MONO.to_string()));
    fonts.inline_code = pick(&names, INLINE_MONO);
    fonts.wordmark = WORDMARK_FACES.iter().find(|f| names.iter().any(|n| n == *f)).map(|f| f.to_string());
    cx.set_global(fonts);
}

/// The family code is drawn in, inline and in blocks, or None to leave the
/// toolkit's (Menlo, Consolas, DejaVu Sans Mono). The Claude app sets code
/// in Anthropic Mono, a web font its stylesheet names `anthropic-mono` and
/// fetches from Anthropic's servers (the file calls itself "Anthropic Mono
/// Web"), with `ui-monospace, monospace` behind it. So: Anthropic Mono
/// when this machine has it, installed or in `~/.emaki/fonts`, else on
/// macOS the system's monospaced face, which is what `ui-monospace` is
/// there.
pub fn code_family(cx: &App) -> Option<SharedString> {
    cx.global::<ChatFonts>().code.clone().map(SharedString::from)
}

/// The family for inline code, or None for the same face as code blocks.
/// The Claude app draws inline code at 0.9 of the body size, and a gpui
/// text run carries a face but no size, so the smaller size has to be a
/// font of its own.
pub fn inline_code_family(cx: &App) -> Option<SharedString> {
    cx.global::<ChatFonts>().inline_code.clone().map(SharedString::from)
}

/// The family the app's name is set in, in the sidebar, or None to leave
/// the window's own face.
pub fn wordmark_family(cx: &App) -> Option<SharedString> {
    cx.global::<ChatFonts>().wordmark.clone().map(SharedString::from)
}

/// The family a conversation is drawn in for `choice` (`serif` or `sans`),
/// or None to leave the window's own face.
pub fn chat_family(choice: &str, cx: &App) -> Option<SharedString> {
    let fonts = cx.global::<ChatFonts>();
    match choice {
        "sans" => fonts.sans.clone().map(SharedString::from),
        _ => Some(SharedString::from(fonts.serif.clone().unwrap_or_else(|| SERIF_FALLBACK.into()))),
    }
}
