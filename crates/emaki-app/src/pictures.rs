//! Files drawn small: which are pictures, what a picture's thumbnail is
//! drawn from, and the tile any other file wears, its kind's glyph on its
//! kind's color (`docs/composer.md`, Attachments).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{Duration, Instant};

use futures::FutureExt as _;
use futures::channel::{mpsc, oneshot};
use gpui::{App, AppContext as _, Asset as _, Context, Entity, EntityId, Hsla, ImageCache, ImageCacheError, ImageCacheItem, ImageSource, IntoElement, ParentElement, RenderImage, Resource, Styled, Task, Window, div, px, rgb};
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
    /// The picture, or the frame of a moving one showing now.
    item: ImageCacheItem,
    /// The bytes a picture was decoded from, for one that came as bytes.
    bytes: Option<Weak<gpui::Image>>,
    /// A picture that may move: its next frames, as they are decoded.
    moving: Option<Moving>,
}

/// A picture that moves (a GIF, an animated WebP), played a frame at a
/// time. gpui decodes every frame before it draws one and keeps on the GPU
/// each frame it has drawn: a GIF of 360 frames of 1600 × 840, 750 KB on
/// disk, took 1.8 GB decoded and as much again on the GPU once it had
/// played through. Here a thread of its own decodes the frames in turn,
/// two ahead of the one showing (`play`); one frame is held and drawn,
/// and the one before goes from the GPU as the next comes in. The window
/// draws the picture again when a frame's time is up, not at the display's
/// rate as gpui's own does.
struct Moving {
    ahead: mpsc::Receiver<Arc<RenderImage>>,
    /// When the frame showing gives way, counted from its first draw.
    due: Option<Instant>,
    /// The draw that brings in the next frame: when, and the views it draws.
    wake: Option<(Instant, Vec<EntityId>, Task<()>)>,
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

    fn get(&mut self, key: (u64, u32), bytes: Option<Weak<gpui::Image>>, start: impl FnOnce(&mut App) -> (Task<Result<Arc<RenderImage>, ImageCacheError>>, Option<mpsc::Receiver<Arc<RenderImage>>>), window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        if let Some(held) = self.held.get_mut(&key) {
            // The same bytes read again (a file reloaded unchanged) are
            // one picture: it lasts as long as the copy drawn last.
            if bytes.is_some() {
                held.bytes = bytes;
            }
            return match held.item.get() {
                Some(Ok(shown)) if held.moving.is_some() => Some(Ok(advance(held, shown, window, cx))),
                got => got,
            };
        }
        // Something new is drawn: what it takes over from goes now.
        self.sweep(cx);
        let (task, ahead) = start(cx);
        let task = task.shared();
        let moving = ahead.map(|ahead| Moving { ahead, due: None, wake: None });
        self.held.insert(key, Held { item: ImageCacheItem::Loading(task.clone()), bytes, moving });
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
                let (to_ahead, ahead) = mpsc::channel(0);
                let read = gpui::ImgResourceLoader::load(resource.clone(), cx);
                let (client, svg) = (cx.http_client(), cx.svg_renderer());
                let task = cx.background_executor().spawn(async move {
                    // A file is looked at for whether it may move, and an
                    // address fetched; a picture that stands still is read
                    // as gpui reads it.
                    let fetched = match &resource {
                        Resource::Path(path) => {
                            if may_move(&head(path)?) {
                                return first_frame(std::fs::read(path.as_ref())?.into(), most, to_ahead).await;
                            }
                            None
                        }
                        Resource::Uri(uri) => {
                            let bytes = fetch(client.as_ref(), uri).await?;
                            if may_move(&bytes) {
                                return first_frame(bytes.into(), most, to_ahead).await;
                            }
                            Some(bytes)
                        }
                        Resource::Embedded(_) => None,
                    };
                    drop(to_ahead);
                    let _turn = DECODING.acquire().await;
                    let decoded = match fetched {
                        Some(bytes) => still(bytes, svg),
                        None => read.await,
                    };
                    decoded.map(|image| shrink(image, most))
                });
                (task, Some(ahead))
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
                if may_move(image.bytes()) {
                    let (to_ahead, ahead) = mpsc::channel(0);
                    return (cx.background_executor().spawn(first_frame(image.bytes().into(), most, to_ahead)), Some(ahead));
                }
                let svg = cx.svg_renderer();
                let task = cx.background_executor().spawn(async move {
                    let _turn = DECODING.acquire().await;
                    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| image.to_image_data(svg)));
                    match decoded {
                        Ok(decoded) => decoded.map(|image| shrink(image, most)).map_err(Into::into),
                        Err(_) => Err(ImageCacheError::Asset("the picture could not be decoded".into())),
                    }
                });
                (task, None)
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

/// The frame of a moving picture to draw now: `shown` until its time is
/// up, then the next, the one before let go of; and the draw that brings
/// in the one after. A moving picture stands still in a window in the
/// background and for a person who asked for less motion, as gpui's own
/// do; the window draws everything again as it comes forward.
fn advance(held: &mut Held, shown: Arc<RenderImage>, window: &mut Window, cx: &mut App) -> Arc<RenderImage> {
    let Some(moving) = held.moving.as_mut() else { return shown };
    if cx.reduce_motion() || !window.is_window_active() {
        (moving.due, moving.wake) = (None, None);
        return shown;
    }
    let now = Instant::now();
    let due = *moving.due.get_or_insert_with(|| now + shows_for(&shown));
    let (mut shown, mut at) = (shown, due);
    if now >= due {
        match moving.ahead.try_recv() {
            Ok(next) => {
                // A frame drawn late shows its whole time; one on time
                // keeps the picture's pace.
                let delay = shows_for(&next);
                at = if now - due < delay { due + delay } else { now + delay };
                moving.due = Some(at);
                if let ImageCacheItem::Loaded(Ok(old)) = std::mem::replace(&mut held.item, ImageCacheItem::Loaded(Ok(next.clone()))) {
                    cx.defer(move |cx| cx.drop_image(old, None));
                }
                shown = next;
            }
            // Played out: a picture of one frame, or one whose frames
            // stopped decoding. The frame showing stays.
            Err(mpsc::TryRecvError::Closed) => {
                held.moving = None;
                return shown;
            }
            // The next is not decoded yet: looked for again in a moment.
            Err(mpsc::TryRecvError::Empty) => at = now + Duration::from_millis(8),
        }
    }
    let view = window.current_view();
    let mut views = match moving.wake.take() {
        Some((when, views, task)) if when == at => {
            if views.contains(&view) {
                moving.wake = Some((when, views, task));
                return shown;
            }
            views
        }
        _ => Vec::new(),
    };
    views.push(view);
    let (draw, wait) = (views.clone(), at.saturating_duration_since(now));
    let task = window.spawn(cx, async move |cx| {
        cx.background_executor().timer(wait).await;
        let _ = cx.update(|_, cx| {
            for view in draw {
                Clock::draw(view, cx);
            }
        });
    });
    moving.wake = Some((at, views, task));
    shown
}

/// The window's clock, through which a moving picture the workbench draws
/// itself (the file's pane, the composer's chips, the lightbox) is drawn
/// again (`ambient::redraw`): a notify of the workbench draws afresh the
/// views the window keeps (the conversation's list, the sidebar), and a
/// GIF did so at every frame of it. One drawn inside a kept view (a
/// markdown file, a thumbnail in the conversation) draws that view again.
pub struct Clock {
    ambient: gpui::WeakEntity<crate::ambient::Ambient>,
    workbench: EntityId,
}

impl gpui::Global for Clock {}

impl Clock {
    /// The clock of the workbench being made.
    pub fn set(ambient: &Entity<crate::ambient::Ambient>, cx: &mut Context<Workbench>) {
        let workbench = cx.entity_id();
        cx.set_global(Clock { ambient: ambient.downgrade(), workbench });
    }

    /// Draws `view` again, the picture drawn in it with it.
    fn draw(view: EntityId, cx: &mut App) {
        let ambient = cx.try_global::<Clock>().filter(|c| c.workbench == view).and_then(|c| c.ambient.upgrade());
        match ambient {
            Some(ambient) => crate::ambient::redraw(&ambient, cx),
            None => cx.notify(view),
        }
    }
}

/// How long a frame of a moving picture shows. One of under 20 ms shows
/// for 100, as browsers draw it: a GIF made with no delay plays at the
/// pace it was made for, not as fast as the window can draw.
fn shows_for(frame: &RenderImage) -> Duration {
    let delay = Duration::from(frame.delay(0));
    if delay < Duration::from_millis(20) { Duration::from_millis(100) } else { delay }
}

/// Whether a picture's first bytes say it may move: a GIF, or a WebP
/// whose header marks it animated. gpui plays no other kind.
fn may_move(head: &[u8]) -> bool {
    let webp = head.len() > 20 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" && &head[12..16] == b"VP8X" && head[20] & 0x02 != 0;
    head.starts_with(b"GIF8") || webp
}

/// A file's first bytes, enough for `may_move`.
fn head(path: &Path) -> Result<Vec<u8>, ImageCacheError> {
    use std::io::Read as _;
    let mut head = Vec::with_capacity(32);
    std::fs::File::open(path)?.take(32).read_to_end(&mut head)?;
    Ok(head)
}

/// A picture's bytes from an address, as gpui's own reader fetches them.
async fn fetch(client: &dyn gpui::http_client::HttpClient, uri: &gpui::SharedUri) -> Result<Vec<u8>, ImageCacheError> {
    use futures::AsyncReadExt as _;
    let mut response = client.get(uri.as_ref(), ().into(), true).await.map_err(|e| ImageCacheError::Other(Arc::new(e)))?;
    let mut body = Vec::new();
    response.body_mut().read_to_end(&mut body).await?;
    if !response.status().is_success() {
        let line = String::from_utf8_lossy(&body).lines().next().unwrap_or("").trim_end().to_string();
        return Err(ImageCacheError::BadStatus { uri: uri.clone(), status: response.status(), body: line });
    }
    Ok(body)
}

/// A picture that stands still, decoded from its bytes as gpui decodes a
/// file's: by what its bytes say it is, and as an SVG when they say
/// nothing.
fn still(bytes: Vec<u8>, svg: gpui::SvgRenderer) -> Result<Arc<RenderImage>, ImageCacheError> {
    use image::ImageFormat as F;
    let format = match image::guess_format(&bytes) {
        Ok(F::Png) => gpui::ImageFormat::Png,
        Ok(F::Jpeg) => gpui::ImageFormat::Jpeg,
        Ok(F::WebP) => gpui::ImageFormat::Webp,
        Ok(F::Gif) => gpui::ImageFormat::Gif,
        Ok(F::Bmp) => gpui::ImageFormat::Bmp,
        Ok(F::Tiff) => gpui::ImageFormat::Tiff,
        Ok(F::Ico) => gpui::ImageFormat::Ico,
        Ok(F::Pnm) => gpui::ImageFormat::Pnm,
        Ok(other) => return Err(ImageCacheError::Asset(format!("{other:?} pictures are not drawn").into())),
        Err(_) => gpui::ImageFormat::Svg,
    };
    let image = gpui::Image::from_bytes(format, bytes);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| image.to_image_data(svg))) {
        Ok(decoded) => decoded.map_err(Into::into),
        Err(_) => Err(ImageCacheError::Asset("the picture could not be decoded".into())),
    }
}

/// A picture that may move: its first frame, from a thread that goes on
/// decoding the rest into `ahead` for as long as the picture is held.
async fn first_frame(bytes: Arc<[u8]>, most: u32, ahead: mpsc::Sender<Arc<RenderImage>>) -> Result<Arc<RenderImage>, ImageCacheError> {
    let (to_first, first) = oneshot::channel();
    std::thread::Builder::new().name("emaki picture".into()).spawn(move || play(&bytes, most, to_first, ahead))?;
    first.await.unwrap_or_else(|_| Err(ImageCacheError::Asset("the picture could not be decoded".into())))
}

/// Decodes a picture's frames in turn: the first into `first`, the rest
/// into `ahead`, which holds one while the next is decoded, and around
/// again from the start at the end. It ends when what is drawn lets go of
/// `ahead`, and after one frame for a picture of one.
fn play(bytes: &Arc<[u8]>, most: u32, first: oneshot::Sender<Result<Arc<RenderImage>, ImageCacheError>>, mut ahead: mpsc::Sender<Arc<RenderImage>>) {
    use futures::SinkExt as _;
    let mut first = Some(first);
    let played = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| loop {
        let mut frames = match frames(bytes) {
            Ok(frames) => frames,
            Err(e) => {
                if let Some(first) = first.take() {
                    let _ = first.send(Err(e));
                }
                return;
            }
        };
        let (w, h) = frames.size();
        let mut small = Small::new(w, h, most);
        let mut n = 0;
        while let Some((delay, changed)) = frames.next_frame() {
            n += 1;
            let frame = match &mut small {
                Some(small) => small.frame(frames.shown(), changed, delay),
                None => frame_of(frames.shown().to_vec(), w, h, delay),
            };
            let Some(frame) = frame else { return };
            let frame = Arc::new(RenderImage::new(vec![frame]));
            let sent = match first.take() {
                Some(first) => first.send(Ok(frame)).is_ok(),
                None => futures::executor::block_on(ahead.send(frame)).is_ok(),
            };
            if !sent {
                return;
            }
        }
        if n <= 1 {
            return;
        }
    }));
    if let Some(first) = first.take() {
        let why = if played.is_err() { "the picture could not be decoded" } else { "the picture has no frames" };
        let _ = first.send(Err(ImageCacheError::Asset(why.into())));
    }
}

/// A frame of `w` × `h` from its pixels.
fn frame_of(pixels: Vec<u8>, w: usize, h: usize, delay: image::Delay) -> Option<image::Frame> {
    Some(image::Frame::from_parts(image::RgbaImage::from_raw(w as u32, h as u32, pixels)?, 0, 0, delay))
}

/// A part of a picture, in pixels: columns `x0..x1`, rows `y0..y1`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Part {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Part {
    fn whole(w: usize, h: usize) -> Part {
        Part { x0: 0, y0: 0, x1: w, y1: h }
    }

    fn with(self, o: Part) -> Part {
        Part { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
}

/// A moving picture's frames in turn, each the whole picture as it shows
/// then, in gpui's order of channels (BGRA), kept in one place and
/// changed there: `next_frame` says how long a frame shows and what part
/// of the picture it changed, and `shown` is the picture as it is now.
trait Frames {
    fn size(&self) -> (usize, usize);
    fn next_frame(&mut self) -> Option<(image::Delay, Part)>;
    fn shown(&self) -> &[u8];
}

/// The frames of a GIF, or of an animated WebP.
fn frames(bytes: &Arc<[u8]>) -> Result<Box<dyn Frames>, ImageCacheError> {
    let cursor = std::io::Cursor::new(bytes.clone());
    if bytes.starts_with(b"GIF8") {
        return Ok(Box::new(GifFrames::new(cursor)?));
    }
    Ok(Box::new(WebpFrames::new(cursor)?))
}

/// An animated WebP's frames, each whole as the image crate makes them
/// (what gpui reads one with). A frame that fails to decode is passed
/// over, as gpui does.
struct WebpFrames {
    frames: image::Frames<'static>,
    width: usize,
    height: usize,
    shown: Vec<u8>,
}

impl WebpFrames {
    fn new(bytes: std::io::Cursor<Arc<[u8]>>) -> Result<Self, ImageCacheError> {
        use image::{AnimationDecoder as _, ImageDecoder as _};
        let mut decoder = image::codecs::webp::WebPDecoder::new(bytes)?;
        let _ = decoder.set_background_color(image::Rgba([0, 0, 0, 0]));
        let (width, height) = decoder.dimensions();
        Ok(WebpFrames { frames: decoder.into_frames(), width: width as usize, height: height as usize, shown: Vec::new() })
    }
}

impl Frames for WebpFrames {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn next_frame(&mut self) -> Option<(image::Delay, Part)> {
        let frame = self.frames.by_ref().flatten().next()?;
        let delay = frame.delay();
        let buffer = frame.into_buffer();
        if buffer.dimensions() != (self.width as u32, self.height as u32) {
            return None;
        }
        self.shown = buffer.into_raw();
        for pixel in self.shown.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Some((delay, Part::whole(self.width, self.height)))
    }

    fn shown(&self) -> &[u8] {
        &self.shown
    }
}

/// A GIF's frames laid on its canvas, as the image crate lays them (what
/// gpui reads a GIF with): a frame's clear pixels show what is under them,
/// and what it leaves for the next is as its disposal says. That reader
/// builds the whole canvas again, a pixel at a time, for every frame, and
/// a new copy of it: a third of a core for a GIF of 1600 × 840 at thirty
/// frames a second, played over and over. Here only the part a frame
/// covers is drawn, on the picture as it shows, in gpui's order of
/// channels from the start, and that part is what the frame changed.
struct GifFrames {
    decoder: gif::Decoder<std::io::Cursor<Arc<[u8]>>>,
    width: usize,
    height: usize,
    /// The picture as it shows now.
    shown: Vec<u8>,
    /// The canvas as the next frame finds it.
    canvas: Vec<u8>,
    /// Where the last frame left the canvas other than it showed (its
    /// disposal cleared it or took it back), to put back before the next.
    undo: Option<Part>,
    first: bool,
}

impl GifFrames {
    fn new(bytes: std::io::Cursor<Arc<[u8]>>) -> Result<Self, ImageCacheError> {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        // A frame as large as the image crate allows one, not the gif
        // crate's 50 MB, which a picture of 3600 pixels a side passes.
        options.set_memory_limit(gif::MemoryLimit::Bytes(std::num::NonZeroU64::new(512 << 20).expect("not zero")));
        let decoder = options.read_info(bytes).map_err(|e| ImageCacheError::Asset(format!("the GIF could not be read: {e}").into()))?;
        let (width, height) = (usize::from(decoder.width()), usize::from(decoder.height()));
        if width == 0 || height == 0 {
            return Err(ImageCacheError::Asset("the GIF has no size".into()));
        }
        Ok(GifFrames { decoder, width, height, shown: vec![0; width * height * 4], canvas: vec![0; width * height * 4], undo: None, first: true })
    }
}

impl Frames for GifFrames {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn next_frame(&mut self) -> Option<(image::Delay, Part)> {
        let frame = self.decoder.read_next_frame().ok()??;
        let (left, top, fw, fh) = (usize::from(frame.left), usize::from(frame.top), usize::from(frame.width), usize::from(frame.height));
        if frame.buffer.len() < fw * fh * 4 {
            return None;
        }
        // The part of the frame on the canvas: a frame may reach past it.
        let (x1, y1) = ((left + fw).min(self.width).max(left.min(self.width)), (top + fh).min(self.height).max(top.min(self.height)));
        let part = Part { x0: left.min(self.width), y0: top.min(self.height), x1, y1 };
        let mut changed = part;
        // What the last frame's disposal left other than it showed.
        if let Some(undo) = self.undo.take() {
            for y in undo.y0..undo.y1 {
                let at = (y * self.width + undo.x0) * 4..(y * self.width + undo.x1) * 4;
                self.shown[at.clone()].copy_from_slice(&self.canvas[at]);
            }
            changed = changed.with(undo);
        }
        let cols = part.x1 - part.x0;
        for y in 0..part.y1 - part.y0 {
            let from = &frame.buffer[y * fw * 4..][..cols * 4];
            let at = ((part.y0 + y) * self.width + part.x0) * 4..((part.y0 + y) * self.width + part.x0 + cols) * 4;
            let row = &mut self.shown[at.clone()];
            for (to, from) in row.chunks_exact_mut(4).zip(from.chunks_exact(4)) {
                if from[3] != 0 {
                    to.copy_from_slice(&[from[2], from[1], from[0], from[3]]);
                }
            }
            match frame.dispose {
                gif::DisposalMethod::Any | gif::DisposalMethod::Keep => self.canvas[at].copy_from_slice(row),
                gif::DisposalMethod::Background => self.canvas[at].fill(0),
                gif::DisposalMethod::Previous => {}
            }
        }
        if matches!(frame.dispose, gif::DisposalMethod::Background | gif::DisposalMethod::Previous) {
            self.undo = Some(part);
        }
        if std::mem::take(&mut self.first) {
            changed = Part::whole(self.width, self.height);
        }
        Some((image::Delay::from_numer_denom_ms(u32::from(frame.delay) * 10, 1), changed))
    }

    fn shown(&self) -> &[u8] {
        &self.shown
    }
}

/// A moving picture's frames brought down to a thumbnail, its longer side
/// `most` pixels, as `shrink` brings a still picture down (the image
/// crate's `thumbnail`): each pixel the mean of the picture's pixels under
/// it. Brought down whole at every frame, a GIF of 1600 × 840 in a
/// composer's chip took more of a core than at its own size; here the
/// thumbnail is kept, and only its pixels over what a frame changed are
/// worked out again.
struct Small {
    width: usize,
    height: usize,
    tw: usize,
    th: usize,
    /// For each column of the thumbnail, the picture's columns it is the
    /// mean of; for each row, its rows.
    cols: Vec<std::ops::Range<usize>>,
    rows: Vec<std::ops::Range<usize>>,
    pixels: Vec<u8>,
}

impl Small {
    /// None when the picture is no larger than `most`, or `most` is 0.
    fn new(width: usize, height: usize, most: u32) -> Option<Small> {
        let most = most as usize;
        if most == 0 || width.max(height) <= most {
            return None;
        }
        let k = most as f64 / width.max(height) as f64;
        let (tw, th) = (((width as f64 * k).round() as usize).max(1), ((height as f64 * k).round() as usize).max(1));
        // The image crate's sums, to the float, so a frame comes out as
        // the still picture of it would.
        let spans = |n: usize, t: usize| -> Vec<std::ops::Range<usize>> {
            let ratio = n as f32 / t as f32;
            (0..t)
                .map(|i| {
                    let from = i as f32 * ratio;
                    let start = (from.ceil() as usize).min(n - 1);
                    start..((from + ratio).ceil() as usize).clamp(start, n)
                })
                .collect()
        };
        Some(Small { width, height, tw, th, cols: spans(width, tw), rows: spans(height, th), pixels: vec![0; tw * th * 4] })
    }

    /// The thumbnail of `shown`, `changed` being all that differs from the
    /// picture it was last worked out from.
    fn frame(&mut self, shown: &[u8], changed: Part, delay: image::Delay) -> Option<image::Frame> {
        if shown.len() < self.width * self.height * 4 {
            return None;
        }
        // The thumbnail's pixels with any of `changed` under them.
        let over = |spans: &[std::ops::Range<usize>], from: usize, to: usize| spans.partition_point(|s| s.end <= from)..spans.partition_point(|s| s.start < to);
        for ty in over(&self.rows, changed.y0, changed.y1) {
            let rows = self.rows[ty].clone();
            for tx in over(&self.cols, changed.x0, changed.x1) {
                let cols = self.cols[tx].clone();
                let mut sum = [0u32; 4];
                for y in rows.clone() {
                    for pixel in shown[(y * self.width + cols.start) * 4..(y * self.width + cols.end) * 4].chunks_exact(4) {
                        for c in 0..4 {
                            sum[c] += u32::from(pixel[c]);
                        }
                    }
                }
                let n = (cols.len() * rows.len()).max(1) as u32;
                let to = &mut self.pixels[(ty * self.tw + tx) * 4..][..4];
                for c in 0..4 {
                    to[c] = ((sum[c] + n / 2) / n).min(255) as u8;
                }
            }
        }
        frame_of(self.pixels.clone(), self.tw, self.th, delay)
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

    /// A GIF of `w` × `h` in four colors, the last clear, of these frames:
    /// where each is, its pixels, and what it leaves for the next.
    fn gif_of(w: u16, h: u16, frames: &[(u16, u16, u16, u16, &[u8], gif::DisposalMethod)]) -> Arc<[u8]> {
        let palette = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let mut out = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut out, w, h, &palette).unwrap();
            for (i, &(left, top, width, height, pixels, dispose)) in frames.iter().enumerate() {
                let frame = gif::Frame { left, top, width, height, buffer: pixels.into(), transparent: Some(3), dispose, delay: 3 + i as u16, ..Default::default() };
                encoder.write_frame(&frame).unwrap();
            }
        }
        out.into()
    }

    fn gif(frames: &[(u16, u16, u16, u16, &[u8], gif::DisposalMethod)]) -> Arc<[u8]> {
        gif_of(4, 3, frames)
    }

    /// Every frame of a picture played once: the picture as it showed,
    /// how long, and what the frame said it changed.
    fn played(bytes: &Arc<[u8]>) -> Vec<(Vec<u8>, image::Delay, Part)> {
        let mut frames = frames(bytes).unwrap();
        let mut out = Vec::new();
        while let Some((delay, changed)) = frames.next_frame() {
            out.push((frames.shown().to_vec(), delay, changed));
        }
        out
    }

    /// Frames of a GIF of 40 × 30 that each change a part of it, with
    /// every kind of disposal and clear pixels.
    fn moving_gif() -> Arc<[u8]> {
        use gif::DisposalMethod::{Background, Keep, Previous};
        let fill = |w: usize, h: usize, seed: usize| -> Vec<u8> { (0..w * h).map(|i| ((i * 7 + seed * 3 + i / w) % 4) as u8).collect() };
        let (a, b, c, d, e) = (vec![0u8; 1200], fill(13, 9, 1), fill(20, 17, 2), fill(7, 25, 3), fill(31, 4, 4));
        gif_of(40, 30, &[(0, 0, 40, 30, &a, Keep), (3, 5, 13, 9, &b, Background), (11, 2, 20, 17, &c, Previous), (30, 4, 7, 25, &d, Keep), (8, 25, 31, 4, &e, Keep), (3, 5, 13, 9, &b, Keep)])
    }

    #[test]
    fn a_gif_is_laid_on_its_canvas_as_the_image_crate_lays_it() {
        use gif::DisposalMethod::{Background, Keep, Previous};
        use image::AnimationDecoder as _;
        let bytes = gif(&[
            (0, 0, 4, 3, &[0; 12], Keep),
            (1, 1, 2, 2, &[1, 3, 1, 1], Background),
            (0, 0, 2, 2, &[2, 2, 3, 2], Previous),
            (2, 1, 2, 2, &[3, 1, 3, 3], Keep),
            (1, 0, 3, 1, &[2, 3, 1], Keep),
        ]);
        let theirs: Vec<_> = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes.clone())).unwrap().into_frames().map(Result::unwrap).collect();
        let ours = played(&bytes);
        assert_eq!(ours.len(), 5);
        assert_eq!(theirs.len(), 5);
        for (n, ((ours, delay, _), theirs)) in ours.iter().zip(&theirs).enumerate() {
            let mut want = theirs.buffer().as_raw().clone();
            for pixel in want.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            assert_eq!(ours, &want, "frame {n}");
            assert_eq!(*delay, theirs.delay(), "frame {n}");
        }
    }

    #[test]
    fn what_a_frame_says_it_changed_is_all_it_changed() {
        let frames = played(&moving_gif());
        assert_eq!(frames[0].2, Part::whole(40, 30));
        for n in 1..frames.len() {
            let (before, (now, _, changed)) = (&frames[n - 1].0, &frames[n]);
            for y in 0..30 {
                for x in 0..40 {
                    let inside = (changed.x0..changed.x1).contains(&x) && (changed.y0..changed.y1).contains(&y);
                    let at = (y * 40 + x) * 4..(y * 40 + x + 1) * 4;
                    assert!(inside || before[at.clone()] == now[at], "frame {n} changed ({x}, {y}) outside {changed:?}");
                }
            }
        }
    }

    #[test]
    fn a_frame_reaching_past_the_canvas_is_cut_at_its_edge() {
        let bytes = gif(&[(0, 0, 4, 3, &[0; 12], gif::DisposalMethod::Keep), (3, 2, 2, 2, &[1, 1, 1, 1], gif::DisposalMethod::Keep)]);
        let (last, _, changed) = played(&bytes).pop().unwrap();
        assert_eq!(last.len(), 4 * 3 * 4);
        assert_eq!(changed, Part { x0: 3, y0: 2, x1: 4, y1: 3 });
        // Green, in gpui's order, at the one pixel of it on the canvas.
        assert_eq!(&last[(2 * 4 + 3) * 4..][..4], &[0, 255, 0, 255]);
    }

    #[test]
    fn a_thumbnail_worked_out_a_part_at_a_time_is_the_one_worked_out_whole() {
        let frames = played(&moving_gif());
        let mut kept = Small::new(40, 30, 17).unwrap();
        for (n, (shown, delay, changed)) in frames.iter().enumerate() {
            let ours = kept.frame(shown, *changed, *delay).unwrap();
            let whole = Small::new(40, 30, 17).unwrap().frame(shown, Part::whole(40, 30), *delay).unwrap();
            assert_eq!(ours.buffer().dimensions(), (17, 13));
            assert_eq!(ours.buffer().as_raw(), whole.buffer().as_raw(), "frame {n}");
        }
    }

    #[test]
    fn a_moving_thumbnail_is_brought_down_as_a_still_one_is() {
        let (w, h) = (203usize, 131usize);
        let pixels: Vec<u8> = (0..w * h * 4).map(|i| ((i * 37 + i / 811 * 11) % 256) as u8).collect();
        let delay = image::Delay::from_numer_denom_ms(50, 1);
        let ours = Small::new(w, h, 64).unwrap().frame(&pixels, Part::whole(w, h), delay).unwrap();
        let theirs = image::imageops::thumbnail(&image::RgbaImage::from_raw(w as u32, h as u32, pixels).unwrap(), 64, 41);
        assert_eq!(ours.buffer().dimensions(), theirs.dimensions());
        assert_eq!(ours.buffer().as_raw(), theirs.as_raw());
        // A picture no larger than the box is drawn as it is.
        assert!(Small::new(64, 40, 64).is_none());
        assert!(Small::new(640, 400, 0).is_none());
    }

    #[test]
    fn what_may_move_is_known_by_its_first_bytes() {
        assert!(may_move(&gif(&[(0, 0, 4, 3, &[0; 12], gif::DisposalMethod::Keep)])));
        let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\x02\0\0\0".to_vec();
        assert!(may_move(&webp));
        webp[20] = 0x10;
        assert!(!may_move(&webp));
        assert!(!may_move(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn a_moving_picture_plays_round_and_a_still_one_stops() {
        use futures::StreamExt as _;
        let take = |bytes: Arc<[u8]>, n: usize| {
            let (to_first, first) = oneshot::channel();
            let (to_ahead, mut ahead) = mpsc::channel(0);
            std::thread::spawn(move || play(&bytes, 0, to_first, to_ahead));
            futures::executor::block_on(async {
                let mut delays = vec![first.await.unwrap().unwrap().delay(0)];
                while delays.len() < n {
                    match ahead.next().await {
                        Some(frame) => delays.push(frame.delay(0)),
                        None => break,
                    }
                }
                delays.into_iter().map(|d| Duration::from(d).as_millis()).collect::<Vec<_>>()
            })
        };
        let moving = gif(&[(0, 0, 4, 3, &[0; 12], gif::DisposalMethod::Keep), (0, 0, 4, 3, &[1; 12], gif::DisposalMethod::Keep), (0, 0, 4, 3, &[2; 12], gif::DisposalMethod::Keep)]);
        assert_eq!(take(moving, 7), [30, 40, 50, 30, 40, 50, 30]);
        let still = gif(&[(0, 0, 4, 3, &[0; 12], gif::DisposalMethod::Keep)]);
        assert_eq!(take(still, 7), [30]);
    }

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
