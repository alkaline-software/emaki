//! Files drawn small: which are pictures, what a picture's thumbnail is
//! drawn from, and the tile any other file wears, its kind's glyph on its
//! kind's color (`docs/composer.md`, Attachments).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use gpui::{Context, Hsla, IntoElement, ParentElement, Styled, div, px, rgb};
use gpui_component::{Icon, Sizable as _};

use crate::workbench::Workbench;

fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Pictures gpui draws itself.
const DRAWN: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "tif", "tiff", "ico", "pbm", "pgm", "ppm", "pnm"];

/// Pictures the system reads and gpui does not: an iPhone's HEIC, AVIF,
/// a camera's raw file, Photoshop's. Drawn from a PNG made from them once
/// (`made`), on a Mac by ImageIO; elsewhere they wear their kind's tile.
const MADE: &[&str] = &["heic", "heif", "avif", "jxl", "jp2", "psd", "tga", "exr", "hdr", "dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2"];

/// Whether a file of this name is a picture, drawn or not.
pub fn is_picture(name: &str) -> bool {
    let ext = ext_of(name);
    DRAWN.contains(&ext.as_str()) || MADE.contains(&ext.as_str())
}

/// Whether a picture of this name can be drawn here: one gpui reads, or
/// on a Mac one the system makes a PNG of.
pub fn can_draw(name: &str) -> bool {
    let ext = ext_of(name);
    DRAWN.contains(&ext.as_str()) || (cfg!(target_os = "macos") && MADE.contains(&ext.as_str()))
}

/// Where the PNG made from `path` is kept: named for the file, its length
/// and its time, so a file changed is made again.
fn made_path(path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified().ok().hash(&mut h);
    Some(emaki_core::paths::cache_dir().join("thumbs").join(format!("{:016x}.png", h.finish())))
}

/// A PNG of the picture at `path`, at most 1600 pixels a side, made by
/// the system's own reader (`sips`, ImageIO): it reads every picture
/// Preview opens.
#[cfg(target_os = "macos")]
fn make(path: &Path, to: &Path) -> bool {
    let Some(dir) = to.parent() else { return false };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let tmp = to.with_extension("tmp.png");
    let made = emaki_core::process::command("/usr/bin/sips")
        .args(["-s", "format", "png", "-Z", "1600"])
        .arg(path)
        .arg("--out")
        .arg(&tmp)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    made && std::fs::rename(&tmp, to).is_ok()
}

/// Off a Mac there is no reader for these: the file wears its tile.
#[cfg(not(target_os = "macos"))]
fn make(_path: &Path, _to: &Path) -> bool {
    false
}

/// Pictures being made now, and those that could not be, so neither is
/// asked for twice.
static MAKING: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);
static FAILED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);

/// Width and height of a picture gpui draws, from its header.
pub fn dims(path: &Path) -> Option<(u32, u32)> {
    crate::workbench::file_image_dims(path).or_else(|| image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_dimensions().ok())
}

impl Workbench {
    /// The file a thumbnail of the picture at `path` is drawn from: the
    /// picture itself, or the PNG made from it. The first ask for one not
    /// made yet starts it, off the main thread, and draws the window again
    /// when it is there; until then, and for a file that is no picture,
    /// `None`.
    pub(crate) fn picture_source(&self, path: &Path, cx: &mut Context<Self>) -> Option<PathBuf> {
        let ext = ext_of(&path.to_string_lossy());
        if DRAWN.contains(&ext.as_str()) {
            return Some(path.to_path_buf());
        }
        if !MADE.contains(&ext.as_str()) || !cfg!(target_os = "macos") {
            return None;
        }
        let to = made_path(path)?;
        if to.is_file() {
            return Some(to);
        }
        if FAILED.lock().ok()?.contains(path) || !MAKING.lock().ok()?.insert(path.to_path_buf()) {
            return None;
        }
        let from = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let made = cx.background_executor().spawn({
                let (from, to) = (from.clone(), to.clone());
                async move { make(&from, &to) }
            });
            let made = made.await;
            if let Ok(mut making) = MAKING.lock() {
                making.remove(&from);
            }
            if !made {
                if let Ok(mut failed) = FAILED.lock() {
                    failed.insert(from);
                }
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        None
    }
}

/// The kind a file is, for the tile it wears: a glyph, and a color for
/// each appearance, the dark one lighter as the accents' are.
#[derive(Clone, Copy)]
pub struct Kind {
    pub icon: &'static str,
    light: u32,
    dark: u32,
}

impl Kind {
    pub fn color(&self, dark: bool) -> Hsla {
        rgb(if dark { self.dark } else { self.light }).into()
    }
}

/// The kind of a file of this name: its family by extension, as an
/// editor's file icons have them.
pub fn kind(name: &str) -> Kind {
    let k = |icon: &'static str, light: u32, dark: u32| Kind { icon, light, dark };
    match ext_of(name).as_str() {
        "pdf" => k("icons/kinds/file-type-pdf.svg", 0xC2413B, 0xEC8A84),
        "doc" | "docx" | "docm" | "dotx" | "odt" | "rtf" | "pages" => k("icons/kinds/file-word.svg", 0x2F6BC4, 0x8DB1EE),
        "xls" | "xlsx" | "xlsm" | "ods" | "numbers" | "csv" | "tsv" | "parquet" => k("icons/kinds/file-spreadsheet.svg", 0x2B8050, 0x74C798),
        "ppt" | "pptx" | "pps" | "ppsx" | "odp" | "key" => k("icons/kinds/presentation.svg", 0xBF5E17, 0xEDA36D),
        "md" | "markdown" | "mdx" | "qmd" | "rmd" => k("icons/kinds/markdown.svg", 0x566273, 0xADB8C6),
        "txt" | "log" | "tex" | "rst" | "org" => k("icons/kinds/file-text.svg", 0x566273, 0xADB8C6),
        "rs" | "py" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "go" | "c" | "h" | "cc" | "cpp" | "hpp" | "java" | "kt" | "rb" | "swift" | "sh" | "zsh" | "bash" | "fish" | "lua" | "r" | "jl" | "php" | "cs" | "html" | "htm" | "css" | "scss" | "vue" | "svelte" | "sql" | "ipynb" => {
            k("icons/kinds/file-code.svg", 0x7353BF, 0xB6A2F2)
        }
        "json" | "jsonl" | "yaml" | "yml" | "toml" | "xml" | "ini" | "cfg" | "conf" | "env" | "sqlite" | "db" | "duckdb" => k("icons/kinds/file-database.svg", 0x9A7206, 0xE0BA5C),
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "dmg" => k("icons/kinds/file-zip.svg", 0x86643F, 0xD0AC86),
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "opus" | "aiff" => k("icons/kinds/file-music.svg", 0xB8406F, 0xEC90B5),
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => k("icons/kinds/movie.svg", 0x8541AA, 0xCB93E6),
        _ if is_picture(name) => k("icons/kinds/photo.svg", 0x1F7D7A, 0x6FC8C3),
        _ => k("icons/kinds/file.svg", 0x736F68, 0xAEAAA2),
    }
}

/// The tile a file that is no picture wears: its kind's glyph on a tint
/// of its kind's color.
pub fn kind_tile(name: &str, size: f32, dark: bool) -> impl IntoElement {
    let kind = kind(name);
    let color = kind.color(dark);
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px(size * 0.24))
        .bg(color.opacity(if dark { 0.2 } else { 0.13 }))
        .flex()
        .items_center()
        .justify_center()
        .child(Icon::default().path(kind.icon).with_size(px(size * 0.55)).text_color(color))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_and_kinds() {
        for name in ["a.TIFF", "b.tif", "c.heic", "d.avif", "e.bmp", "f.svg", "g.dng"] {
            assert!(is_picture(name), "{name}");
        }
        for name in ["a.pdf", "b.txt", "c", "d.tiffany"] {
            assert!(!is_picture(name), "{name}");
        }
        assert_eq!(kind("Report.PDF").icon, "icons/kinds/file-type-pdf.svg");
        assert_eq!(kind("data.csv").icon, "icons/kinds/file-spreadsheet.svg");
        assert_eq!(kind("notes.qmd").icon, "icons/kinds/markdown.svg");
        assert_eq!(kind("shot.heic").icon, "icons/kinds/photo.svg");
        assert_eq!(kind("Makefile").icon, "icons/kinds/file.svg");
    }
}
