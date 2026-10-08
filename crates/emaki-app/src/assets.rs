//! Our own icons, over the set gpui-component ships.
//!
//! `Icon::new(IconName::…)` resolves through the app's `AssetSource`; this
//! wrapper answers for the files under `assets/icons/` and hands everything
//! else to the bundled set. The files there are Tabler's outline icons
//! (MIT, `TABLER-LICENSE` beside them), each under the name the toolkit
//! or the app asks for (`close.svg` is Tabler's `x`), so every icon in
//! the window is drawn from one family; `claude.svg` is Claude's own mark.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct Assets;

const OWN: &[(&str, &[u8])] = &[
    ("icon/app.png", include_bytes!("../assets/icon/icon-128.png")),
    ("icons/briefcase.svg", include_bytes!("../assets/icons/briefcase.svg")),
    ("icons/git-diff.svg", include_bytes!("../assets/icons/git-diff.svg")),
    ("icons/keyboard.svg", include_bytes!("../assets/icons/keyboard.svg")),
    ("icons/pencil-simple.svg", include_bytes!("../assets/icons/pencil-simple.svg")),
    ("icons/trash.svg", include_bytes!("../assets/icons/trash.svg")),
    ("icons/file-plus.svg", include_bytes!("../assets/icons/file-plus.svg")),
    ("icons/folder-plus.svg", include_bytes!("../assets/icons/folder-plus.svg")),
    ("icons/at.svg", include_bytes!("../assets/icons/at.svg")),
    ("icons/hash.svg", include_bytes!("../assets/icons/hash.svg")),
    ("icons/terminal.svg", include_bytes!("../assets/icons/terminal.svg")),
    ("icons/image.svg", include_bytes!("../assets/icons/image.svg")),
    ("icons/git-branch.svg", include_bytes!("../assets/icons/git-branch.svg")),
    ("icons/tree-view.svg", include_bytes!("../assets/icons/tree-view.svg")),
    ("icons/list-bullets.svg", include_bytes!("../assets/icons/list-bullets.svg")),
    ("icons/a-large-small.svg", include_bytes!("../assets/icons/a-large-small.svg")),
    ("icons/arrow-down.svg", include_bytes!("../assets/icons/arrow-down.svg")),
    ("icons/arrow-left.svg", include_bytes!("../assets/icons/arrow-left.svg")),
    ("icons/arrow-right.svg", include_bytes!("../assets/icons/arrow-right.svg")),
    ("icons/arrow-up.svg", include_bytes!("../assets/icons/arrow-up.svg")),
    ("icons/asterisk.svg", include_bytes!("../assets/icons/asterisk.svg")),
    ("icons/battery-charging.svg", include_bytes!("../assets/icons/battery-charging.svg")),
    ("icons/battery-full.svg", include_bytes!("../assets/icons/battery-full.svg")),
    ("icons/battery-low.svg", include_bytes!("../assets/icons/battery-low.svg")),
    ("icons/battery-medium.svg", include_bytes!("../assets/icons/battery-medium.svg")),
    ("icons/battery-warning.svg", include_bytes!("../assets/icons/battery-warning.svg")),
    ("icons/battery.svg", include_bytes!("../assets/icons/battery.svg")),
    ("icons/bell.svg", include_bytes!("../assets/icons/bell.svg")),
    ("icons/book-open.svg", include_bytes!("../assets/icons/book-open.svg")),
    ("icons/bot.svg", include_bytes!("../assets/icons/bot.svg")),
    ("icons/box.svg", include_bytes!("../assets/icons/box.svg")),
    ("icons/building-2.svg", include_bytes!("../assets/icons/building-2.svg")),
    ("icons/calendar.svg", include_bytes!("../assets/icons/calendar.svg")),
    ("icons/case-sensitive.svg", include_bytes!("../assets/icons/case-sensitive.svg")),
    ("icons/chart-pie.svg", include_bytes!("../assets/icons/chart-pie.svg")),
    ("icons/check.svg", include_bytes!("../assets/icons/check.svg")),
    ("icons/chevron-down.svg", include_bytes!("../assets/icons/chevron-down.svg")),
    ("icons/chevron-left.svg", include_bytes!("../assets/icons/chevron-left.svg")),
    ("icons/chevron-right.svg", include_bytes!("../assets/icons/chevron-right.svg")),
    ("icons/chevron-up.svg", include_bytes!("../assets/icons/chevron-up.svg")),
    ("icons/chevrons-up-down.svg", include_bytes!("../assets/icons/chevrons-up-down.svg")),
    ("icons/circle-check.svg", include_bytes!("../assets/icons/circle-check.svg")),
    ("icons/circle-user.svg", include_bytes!("../assets/icons/circle-user.svg")),
    ("icons/circle-x.svg", include_bytes!("../assets/icons/circle-x.svg")),
    ("icons/claude.svg", include_bytes!("../assets/icons/claude.svg")),
    ("icons/close.svg", include_bytes!("../assets/icons/close.svg")),
    ("icons/copy.svg", include_bytes!("../assets/icons/copy.svg")),
    ("icons/cpu.svg", include_bytes!("../assets/icons/cpu.svg")),
    ("icons/dash.svg", include_bytes!("../assets/icons/dash.svg")),
    ("icons/delete.svg", include_bytes!("../assets/icons/delete.svg")),
    ("icons/ellipsis-vertical.svg", include_bytes!("../assets/icons/ellipsis-vertical.svg")),
    ("icons/ellipsis.svg", include_bytes!("../assets/icons/ellipsis.svg")),
    ("icons/external-link.svg", include_bytes!("../assets/icons/external-link.svg")),
    ("icons/eye-off.svg", include_bytes!("../assets/icons/eye-off.svg")),
    ("icons/eye.svg", include_bytes!("../assets/icons/eye.svg")),
    ("icons/file-archive.svg", include_bytes!("../assets/icons/file-archive.svg")),
    ("icons/file-audio.svg", include_bytes!("../assets/icons/file-audio.svg")),
    ("icons/file-code.svg", include_bytes!("../assets/icons/file-code.svg")),
    ("icons/file-image.svg", include_bytes!("../assets/icons/file-image.svg")),
    ("icons/file-table.svg", include_bytes!("../assets/icons/file-table.svg")),
    ("icons/file-text.svg", include_bytes!("../assets/icons/file-text.svg")),
    ("icons/file-video.svg", include_bytes!("../assets/icons/file-video.svg")),
    ("icons/file.svg", include_bytes!("../assets/icons/file.svg")),
    ("icons/folder-closed.svg", include_bytes!("../assets/icons/folder-closed.svg")),
    ("icons/folder-open.svg", include_bytes!("../assets/icons/folder-open.svg")),
    ("icons/folder.svg", include_bytes!("../assets/icons/folder.svg")),
    ("icons/frame.svg", include_bytes!("../assets/icons/frame.svg")),
    ("icons/gallery-vertical-end.svg", include_bytes!("../assets/icons/gallery-vertical-end.svg")),
    ("icons/gauge.svg", include_bytes!("../assets/icons/gauge.svg")),
    ("icons/github.svg", include_bytes!("../assets/icons/github.svg")),
    ("icons/globe.svg", include_bytes!("../assets/icons/globe.svg")),
    ("icons/hard-drive.svg", include_bytes!("../assets/icons/hard-drive.svg")),
    ("icons/heart-off.svg", include_bytes!("../assets/icons/heart-off.svg")),
    ("icons/heart.svg", include_bytes!("../assets/icons/heart.svg")),
    ("icons/inbox.svg", include_bytes!("../assets/icons/inbox.svg")),
    ("icons/info.svg", include_bytes!("../assets/icons/info.svg")),
    ("icons/layout-dashboard.svg", include_bytes!("../assets/icons/layout-dashboard.svg")),
    ("icons/loader-circle.svg", include_bytes!("../assets/icons/loader-circle.svg")),
    ("icons/loader.svg", include_bytes!("../assets/icons/loader.svg")),
    ("icons/map.svg", include_bytes!("../assets/icons/map.svg")),
    ("icons/mark.svg", include_bytes!("../assets/icons/mark.svg")),
    ("icons/maximize.svg", include_bytes!("../assets/icons/maximize.svg")),
    ("icons/memory-stick.svg", include_bytes!("../assets/icons/memory-stick.svg")),
    ("icons/menu.svg", include_bytes!("../assets/icons/menu.svg")),
    ("icons/minimize.svg", include_bytes!("../assets/icons/minimize.svg")),
    ("icons/minus.svg", include_bytes!("../assets/icons/minus.svg")),
    ("icons/moon.svg", include_bytes!("../assets/icons/moon.svg")),
    ("icons/network.svg", include_bytes!("../assets/icons/network.svg")),
    ("icons/palette.svg", include_bytes!("../assets/icons/palette.svg")),
    ("icons/panel-left-close.svg", include_bytes!("../assets/icons/panel-left-close.svg")),
    ("icons/panel-left-open.svg", include_bytes!("../assets/icons/panel-left-open.svg")),
    ("icons/panel-left.svg", include_bytes!("../assets/icons/panel-left.svg")),
    ("icons/pause.svg", include_bytes!("../assets/icons/pause.svg")),
    ("icons/play.svg", include_bytes!("../assets/icons/play.svg")),
    ("icons/plus.svg", include_bytes!("../assets/icons/plus.svg")),
    ("icons/redo-2.svg", include_bytes!("../assets/icons/redo-2.svg")),
    ("icons/redo.svg", include_bytes!("../assets/icons/redo.svg")),
    ("icons/replace.svg", include_bytes!("../assets/icons/replace.svg")),
    ("icons/search.svg", include_bytes!("../assets/icons/search.svg")),
    ("icons/settings-2.svg", include_bytes!("../assets/icons/settings-2.svg")),
    ("icons/settings.svg", include_bytes!("../assets/icons/settings.svg")),
    ("icons/shield.svg", include_bytes!("../assets/icons/shield.svg")),
    ("icons/sort-ascending.svg", include_bytes!("../assets/icons/sort-ascending.svg")),
    ("icons/sort-descending.svg", include_bytes!("../assets/icons/sort-descending.svg")),
    ("icons/square-terminal.svg", include_bytes!("../assets/icons/square-terminal.svg")),
    ("icons/star-fill.svg", include_bytes!("../assets/icons/star-fill.svg")),
    ("icons/star.svg", include_bytes!("../assets/icons/star.svg")),
    ("icons/sun.svg", include_bytes!("../assets/icons/sun.svg")),
    ("icons/thumbs-down.svg", include_bytes!("../assets/icons/thumbs-down.svg")),
    ("icons/thumbs-up.svg", include_bytes!("../assets/icons/thumbs-up.svg")),
    ("icons/triangle-alert.svg", include_bytes!("../assets/icons/triangle-alert.svg")),
    ("icons/undo-2.svg", include_bytes!("../assets/icons/undo-2.svg")),
    ("icons/undo.svg", include_bytes!("../assets/icons/undo.svg")),
    ("icons/user.svg", include_bytes!("../assets/icons/user.svg")),
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
        if path.starts_with(crate::file_icons::PREFIX) {
            return Ok(crate::file_icons::load(path));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut v = gpui_component_assets::Assets.list(path)?;
        v.extend(OWN.iter().filter(|(p, _)| p.starts_with(path)).map(|(p, _)| SharedString::from(*p)));
        Ok(v)
    }
}
