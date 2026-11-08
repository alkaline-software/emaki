//! Files drawn small: which are pictures, what a picture's thumbnail is
//! drawn from, and the tile any other file wears, its kind's glyph on its
//! kind's color (`docs/composer.md`, Attachments).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

use futures::FutureExt as _;
use gpui::{App, AppContext as _, Asset as _, Context, Entity, Hsla, ImageCache, ImageCacheError, ImageCacheItem, ImageSource, IntoElement, ParentElement, RenderImage, Resource, Styled, Task, Window, div, px, rgb};
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

/// The longest side, in pixels, a thumbnail is decoded at: twice the
/// largest box one is drawn in (a picture a tool read, 320 points), so it
/// is sharp on a Retina screen.
pub const THUMB_PX: u32 = 640;

/// How many pictures are decoded at once. A picture is decoded whole
/// before it is brought down, at some twenty megabytes for a screenshot,
/// and a prompt of twenty decoded together left 400 MB the allocator gave
/// back to the system only slowly; two at a time reuse the same room.
static DECODING: async_lock::Semaphore = async_lock::Semaphore::new(2);

/// The pictures one place draws (a conversation, the composer, the
/// lightbox, the file's pane, the comparison of changes), decoded off the
/// main thread. gpui keeps every picture it decodes itself, and its copy
/// on the GPU, until the app quits: a day of pasted screenshots looked at
/// held 2.4 GB. A place's pictures go, both copies, when the place does
/// (its `Entity<Kept>` is dropped), and one made from bytes as soon as
/// nothing else holds those bytes.
pub struct Kept {
    held: HashMap<(u64, u32), Held>,
}

struct Held {
    item: ImageCacheItem,
    /// The bytes a picture was decoded from, for one that came as bytes.
    bytes: Option<Weak<gpui::Image>>,
}

impl Kept {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let kept = cx.new(|_| Kept { held: HashMap::new() });
        cx.observe_release(&kept, |kept, cx| {
            for (_, mut held) in kept.held.drain() {
                if let Some(Ok(image)) = held.item.get() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        kept
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// How many pictures are held, and the bytes they take decoded.
    pub fn held(&self) -> (usize, usize) {
        let bytes = self
            .held
            .values()
            .map(|h| match &h.item {
                ImageCacheItem::Loaded(Ok(image)) => (0..image.frame_count()).filter_map(|i| image.as_bytes(i)).map(|b| b.len()).sum(),
                _ => 0,
            })
            .sum();
        (self.held.len(), bytes)
    }

    fn get(&mut self, key: (u64, u32), bytes: Option<Weak<gpui::Image>>, start: impl FnOnce(&mut App) -> Task<Result<Arc<RenderImage>, ImageCacheError>>, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        if let Some(held) = self.held.get_mut(&key) {
            // The same bytes read again (a file reloaded unchanged) are
            // one picture: it lasts as long as the copy drawn last.
            if bytes.is_some() {
                held.bytes = bytes;
            }
            return held.item.get();
        }
        // Something new is drawn: what it takes over from goes now.
        self.sweep(cx);
        let task = start(cx).shared();
        self.held.insert(key, Held { item: ImageCacheItem::Loading(task.clone()), bytes });
        let view = window.current_view();
        window
            .spawn(cx, async move |cx| {
                let _ = task.await;
                cx.on_next_frame(move |_, cx| cx.notify(view));
            })
            .detach();
        None
    }

    fn resource(&mut self, resource: &Resource, most: u32, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let resource = resource.clone();
        self.get(
            (resource_key(&resource), most),
            None,
            move |cx| {
                let read = gpui::ImgResourceLoader::load(resource, cx);
                cx.background_executor().spawn(async move {
                    let _turn = DECODING.acquire().await;
                    read.await.map(|image| shrink(image, most))
                })
            },
            window,
            cx,
        )
    }

    fn bytes(&mut self, image: &Arc<gpui::Image>, most: u32, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let (weak, image) = (Arc::downgrade(image), image.clone());
        self.get(
            (gpui::hash(&(1u8, image.id())), most),
            Some(weak),
            move |cx| {
                let svg = cx.svg_renderer();
                cx.background_executor().spawn(async move {
                    let _turn = DECODING.acquire().await;
                    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| image.to_image_data(svg)));
                    match decoded {
                        Ok(decoded) => decoded.map(|image| shrink(image, most)).map_err(Into::into),
                        Err(_) => Err(ImageCacheError::Asset("the picture could not be decoded".into())),
                    }
                })
            },
            window,
            cx,
        )
    }

    /// Lets go of every picture made from bytes nothing else holds now.
    pub fn sweep(&mut self, cx: &mut App) {
        let gone: Vec<_> = self.held.iter().filter(|(_, h)| h.bytes.as_ref().is_some_and(|b| b.strong_count() == 0)).map(|(k, _)| *k).collect();
        for key in gone {
            if let Some(held) = self.held.remove(&key) {
                let_go(held, cx);
            }
        }
    }

    /// Lets go of the picture read from `resource`, at every size, so the
    /// next draw reads it again: the file changed, or reading it failed.
    pub fn forget(&mut self, resource: &Resource, cx: &mut App) {
        let id = resource_key(resource);
        let gone: Vec<_> = self.held.keys().filter(|k| k.0 == id).copied().collect();
        for key in gone {
            if let Some(held) = self.held.remove(&key) {
                let_go(held, cx);
            }
        }
    }
}

/// A picture read from a file or the web, at its own size: the markdown
/// view's pictures, inside an `image_cache` of a `Kept`. The app's own
/// (a file's icon) stay with gpui, few and small, so a place made anew
/// does not draw them blank for a frame.
impl ImageCache for Kept {
    fn load(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        match resource {
            Resource::Embedded(_) => window.use_asset::<gpui::ImgResourceLoader>(resource, cx),
            _ => self.resource(resource, 0, window, cx),
        }
    }
}

fn resource_key(resource: &Resource) -> u64 {
    gpui::hash(&(0u8, resource))
}

/// A picture let go of, the GPU's copy after the frame being built, when
/// every window is back in the app's hands.
fn let_go(mut held: Held, cx: &mut App) {
    if let Some(Ok(image)) = held.item.get() {
        cx.defer(move |cx| cx.drop_image(image, None));
    }
}

/// `source` drawn through `kept`: a picture read from a file or the web,
/// or made from bytes, decoded there with its longer side brought down
/// to `most` pixels (0 keeps its size). The app's own pictures, few and
/// small, and pixels already made are drawn as they are.
pub fn through(kept: &Entity<Kept>, source: ImageSource, most: u32) -> ImageSource {
    let kept = kept.downgrade();
    match source {
        ImageSource::Resource(Resource::Embedded(_)) | ImageSource::Render(_) | ImageSource::Custom(_) => source,
        ImageSource::Resource(resource) => ImageSource::Custom(Arc::new(move |window, cx| kept.upgrade()?.update(cx, |k, cx| k.resource(&resource, most, window, cx)))),
        ImageSource::Image(image) => ImageSource::Custom(Arc::new(move |window, cx| kept.upgrade()?.update(cx, |k, cx| k.bytes(&image, most, window, cx)))),
    }
}

/// `image` with its longer side brought down to `most` pixels, every
/// frame of it; as it is when it is no larger, or `most` is 0. Area
/// averaging, as a thumbnail wants: the GPU sampling a large picture
/// down to a small box draws it jagged.
fn shrink(image: Arc<RenderImage>, most: u32) -> Arc<RenderImage> {
    let size = image.size(0);
    let (w, h) = (size.width.0.max(0) as u32, size.height.0.max(0) as u32);
    if most == 0 || w.max(h) <= most {
        return image;
    }
    let k = most as f64 / w.max(h) as f64;
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut frames = Vec::with_capacity(image.frame_count());
        for i in 0..image.frame_count() {
            let s = image.size(i);
            let (fw, fh) = (s.width.0.max(0) as u32, s.height.0.max(0) as u32);
            let buffer = image::RgbaImage::from_raw(fw, fh, image.as_bytes(i)?.to_vec())?;
            let (nw, nh) = (((fw as f64 * k).round() as u32).max(1), ((fh as f64 * k).round() as u32).max(1));
            frames.push(image::Frame::from_parts(image::imageops::thumbnail(&buffer, nw, nh), 0, 0, image.delay(i)));
        }
        Some(Arc::new(RenderImage::new(frames)))
    }));
    made.ok().flatten().unwrap_or(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(w: u32, h: u32, frames: usize) -> Arc<RenderImage> {
        let frames: Vec<_> = (0..frames).map(|i| image::Frame::from_parts(image::RgbaImage::from_pixel(w, h, image::Rgba([i as u8, 2, 3, 255])), 0, 0, image::Delay::from_numer_denom_ms(70, 1))).collect();
        Arc::new(RenderImage::new(frames))
    }

    #[test]
    fn a_large_picture_is_brought_down_to_its_box() {
        let small = shrink(render(2000, 1288, 1), THUMB_PX);
        assert_eq!((small.size(0).width.0, small.size(0).height.0), (640, 412));
        assert_eq!(&small.as_bytes(0).unwrap()[..4], &[0, 2, 3, 255]);
        // Every frame of a moving one, at its pace.
        let moving = shrink(render(500, 1500, 3), THUMB_PX);
        assert_eq!(moving.frame_count(), 3);
        assert_eq!((moving.size(2).width.0, moving.size(2).height.0), (213, 640));
        assert_eq!(moving.delay(1), image::Delay::from_numer_denom_ms(70, 1));
        assert_eq!(moving.as_bytes(2).unwrap()[0], 2);
    }

    #[test]
    fn a_small_picture_or_no_box_keeps_its_size() {
        let small = render(300, 200, 1);
        assert!(Arc::ptr_eq(&shrink(small.clone(), THUMB_PX), &small));
        let large = render(2000, 1288, 1);
        assert!(Arc::ptr_eq(&shrink(large.clone(), 0), &large));
    }

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
