//! The conversation's typefaces. Anthropic Serif and Anthropic Sans are the
//! Claude desktop app's own fonts and not ours to ship, so they are loaded
//! at start from a Claude app installed on this machine, when there is one.
//! Without it the serif falls back to Georgia and the sans to the window's
//! own face, and the settings panel says so.

use std::path::PathBuf;

use gpui::{App, Global, SharedString};

pub const SERIF_FALLBACK: &str = "Georgia";

/// The family names the text system knows the two fonts by, once loaded.
#[derive(Debug, Clone, Default)]
pub struct ChatFonts {
    pub serif: Option<String>,
    pub sans: Option<String>,
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
    if fonts.source.is_some() {
        let names = cx.text_system().all_font_names();
        fonts.serif = pick(&names, "Anthropic Serif");
        fonts.sans = pick(&names, "Anthropic Sans");
    }
    cx.set_global(fonts);
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
