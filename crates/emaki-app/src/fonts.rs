//! The conversation's typefaces. Anthropic Serif and Anthropic Sans are the
//! Claude desktop app's own fonts and not ours to ship, so they are loaded
//! at start from a Claude app installed on this machine, when there is one.
//! Without it the serif falls back to Georgia and the sans to the window's
//! own face, and the settings panel says so.

use std::borrow::Cow;
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

/// Load the Claude app's fonts into the text system, if the app is here.
pub fn install(cx: &mut App) {
    let mut fonts = ChatFonts::default();
    for dir in font_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut data: Vec<Cow<'static, [u8]>> = Vec::new();
        for entry in rd.flatten() {
            let p = entry.path();
            let is_font = p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf")).unwrap_or(false);
            if is_font {
                if let Ok(bytes) = std::fs::read(&p) {
                    data.push(Cow::Owned(bytes));
                }
            }
        }
        if data.is_empty() || cx.text_system().add_fonts(data).is_err() {
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
