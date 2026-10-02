//! Our own icons, on top of the set gpui-component ships.
//!
//! `Icon::new(IconName::…)` resolves through the app's `AssetSource`; this
//! wrapper answers for the files under `assets/icons/` and hands everything
//! else to the bundled set.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct Assets;

const OWN: &[(&str, &[u8])] = &[
    ("icon/app.png", include_bytes!("../assets/icon/icon-128.png")),
    ("icons/mark.svg", include_bytes!("../assets/icons/mark.svg")),
    ("icons/claude.svg", include_bytes!("../assets/icons/claude.svg")),
    ("icons/file-text.svg", include_bytes!("../assets/icons/file-text.svg")),
    ("icons/file-code.svg", include_bytes!("../assets/icons/file-code.svg")),
    ("icons/file-archive.svg", include_bytes!("../assets/icons/file-archive.svg")),
    ("icons/file-audio.svg", include_bytes!("../assets/icons/file-audio.svg")),
    ("icons/file-video.svg", include_bytes!("../assets/icons/file-video.svg")),
    ("icons/file-table.svg", include_bytes!("../assets/icons/file-table.svg")),
    ("icons/file-image.svg", include_bytes!("../assets/icons/file-image.svg")),
];

/// The icon that stands for a file of this name when there is no thumbnail.
pub fn file_icon_path(name: &str) -> &'static str {
    let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "md" | "txt" | "rtf" | "doc" | "docx" | "pdf" | "pages" | "tex" | "log" => "icons/file-text.svg",
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "json" | "toml" | "yaml" | "yml" | "html" | "css" | "sh" | "zsh" | "go" | "c" | "h" | "cpp" | "java" | "rb" | "swift" | "sql" | "r" | "jl" | "lua" => "icons/file-code.svg",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "dmg" => "icons/file-archive.svg",
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "aiff" => "icons/file-audio.svg",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => "icons/file-video.svg",
        "csv" | "tsv" | "xls" | "xlsx" | "numbers" | "parquet" => "icons/file-table.svg",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" | "bmp" | "tiff" => "icons/file-image.svg",
        _ => "icons/file.svg",
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = OWN.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut v = gpui_component_assets::Assets.list(path)?;
        v.extend(OWN.iter().filter(|(p, _)| p.starts_with(path)).map(|(p, _)| SharedString::from(*p)));
        Ok(v)
    }
}
