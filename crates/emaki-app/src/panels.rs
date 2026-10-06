//! Beside a conversation: the files of its folder, and its outline.
//!
//! Two panels, one at a time, at the conversation's left: the pane's
//! whole height under the top strip, chosen with a two-segment control at
//! the strip's left end. The files are the session's folder as a tree, read from
//! disk a folder at a time as it is opened; a click on a file shows it
//! over the window, a right click offers what a file manager would. The
//! outline is `emaki_core::outline`: a line on what each round asked and
//! a line on what came of it, the round in view marked, a click going
//! there.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use chrono::Datelike as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use emaki_core::git;
use emaki_core::outline::{self, Kind};

use crate::format::{clock, plural};
use crate::workbench::{float_shadow, pill_button, swallow_click, MenuDo, Notice, Page, Workbench};

/// How wide a panel is; both are, so one takes the other's place without
/// moving the conversation.
pub const PANEL_W: Pixels = px(264.);
/// The panel's edge is dragged between these; a double click on it asks
/// for no more than `PANEL_FIT_MAX`, and dragged narrower than
/// `PANEL_FOLD_AT` the panel is put away.
const PANEL_MIN: Pixels = px(200.);
const PANEL_MAX: Pixels = px(520.);
const PANEL_FIT_MAX: Pixels = px(380.);
const PANEL_FOLD_AT: Pixels = px(130.);
/// The outline's width at a double click: its lines are sentences, cut
/// at any width, so this is a width they read well at.
const OUTLINE_FIT: Pixels = px(300.);
/// The least the conversation keeps beside a dragged panel.
const CONVERSATION_MIN: Pixels = px(360.);
/// How wide the strip at the panel's edge that takes the drag is.
const PANEL_GRIP: Pixels = px(8.);
/// A panel's head is as tall as the folder's band beside it, so the two
/// read as one band across the pane.
const HEAD_H: Pixels = px(31.);
const TREE_ROW_H: Pixels = px(24.);
/// The list of branches shows this many rows of this height and scrolls
/// for the rest.
const BRANCH_ROWS: usize = 10;
const BRANCH_ROW_H: Pixels = px(30.);
/// The comparison: how wide its list of files is, and how tall a row of
/// that list.
const CHANGES_LIST_W: Pixels = px(300.);
const CHANGE_ROW_H: Pixels = px(26.);
/// The most entries one folder lists before "N more".
const DIR_MAX: usize = 400;
/// How often an open tree is read from disk again, in seconds.
const TREE_SECS: f64 = 2.0;
/// The most of a file a preview reads, and the most lines of code it sets.
const PREVIEW_BYTES: usize = 400_000;
const PREVIEW_LINES: usize = 1500;

/// How often a move of the conversation toward an outline entry is
/// stepped, the most it may take, and its shape: how fast it runs while
/// it cannot yet tell how far the round is and for how many ticks, the
/// most of the way it draws once it can (the rest is skipped), how far
/// from a round above it starts when the run did not reach it, and the
/// time over which what is left falls away.
const GLIDE_TICK: Duration = Duration::from_millis(8);
const GLIDE_MOST: Duration = Duration::from_millis(900);
const GLIDE_RUN: f32 = 5200.;
const GLIDE_RUN_TICKS: u32 = 14;
const GLIDE_CAP: f32 = 700.;
const GLIDE_FROM: f32 = 260.;
const GLIDE_EASE: f32 = 0.07;
/// The trackpad travel that moves the outline's mark one entry, and the
/// least time between two moves of it.
const WHEEL_STEP: f32 = 34.;
const WHEEL_PACE: Duration = Duration::from_millis(70);

/// A move of the conversation's list toward a round, a tick at a time.
/// The list knows where a round is only once it has been laid out, so
/// the way there is felt out: with the round's place known, what is left
/// falls away smoothly; with it unknown (a round above, or one below that
/// was never drawn) the list runs that way a few ticks, which lays out
/// what it passes, and if the round is still not found it is gone to
/// directly.
#[derive(Default)]
struct Glide {
    blind: u32,
    jumped: bool,
    still: u32,
    last: Option<f32>,
}

impl Glide {
    /// One tick; whether the move is over.
    fn step(&mut self, list: &ListState, target: usize, dt: f32) -> bool {
        let top = list.logical_scroll_top();
        let left = if top.item_ix == target {
            Some(-f32::from(top.offset_in_item))
        } else if target > top.item_ix {
            list.bounds_for_item(target).map(|b| f32::from(b.top() - list.viewport_bounds().top()))
        } else {
            None
        };
        if std::env::var("EMAKI_GLIDE_DEBUG").is_ok() {
            eprintln!("glide to {target}: at {}+{:.0} left {left:?}", top.item_ix, f32::from(top.offset_in_item));
        }
        let Some(mut left) = left else {
            // Come down onto the list's end, which is where it holds
            // the view while it follows a reply: as near as it gets.
            if top.item_ix >= list.item_count() && self.last.is_some() {
                return true;
            }
            let up = target < top.item_ix;
            // Held at its end the list has no place to run from: a run
            // there moved nothing for its whole length.
            if top.item_ix >= list.item_count() {
                self.blind = GLIDE_RUN_TICKS;
            }
            self.blind += 1;
            if self.blind <= GLIDE_RUN_TICKS {
                list.scroll_by(px(GLIDE_RUN * dt.min(0.05) * if up { -1. } else { 1. }));
                return false;
            }
            if up && !std::mem::replace(&mut self.jumped, true) {
                // Inside the round, a little under its top, to come up
                // onto it.
                list.scroll_to(ListOffset { item_ix: target, offset_in_item: px(GLIDE_FROM) });
                return false;
            }
            return true;
        };
        if left.abs() < 1. {
            return true;
        }
        // The end of the list holds the view short of a last, short
        // round: no nearer after a few ticks is as near as it gets.
        if self.last.is_some_and(|was| (was - left).abs() < 0.5) {
            self.still += 1;
            if self.still >= 4 {
                return true;
            }
        } else {
            self.still = 0;
        }
        self.last = Some(left);
        if left.abs() > GLIDE_CAP {
            let skip = left - GLIDE_CAP * left.signum();
            list.scroll_by(px(skip));
            left -= skip;
        }
        let mut step = left * (1. - (-dt / GLIDE_EASE).exp());
        if step.abs() < 1.5 {
            step = left.abs().min(1.5) * left.signum();
        }
        list.scroll_by(px(step));
        false
    }
}

/// One file or folder in the tree.
#[derive(Clone, PartialEq, Eq)]
pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub dir: bool,
}

/// What a folder holds, as far as it is listed, and how many were left out.
type Listing = Rc<(Vec<Node>, usize)>;

/// A row of the tree as drawn.
enum Row {
    Node(Node, usize, bool),
    More(usize, usize),
}

/// The folders opened in the files panel and what each was last seen to
/// hold. Kept by absolute path, so two sessions of one folder share it.
#[derive(Default)]
pub struct Tree {
    open: HashSet<PathBuf>,
    kids: HashMap<PathBuf, Listing>,
    /// The file last shown, which its row marks.
    picked: Option<PathBuf>,
    read_at: f64,
    /// What git says of the folder it was last asked about, and whether
    /// it is being asked now.
    git: Option<(PathBuf, Rc<git::Status>)>,
    /// Every file with a change, by path, as the status last had them.
    changed: Rc<Vec<(PathBuf, git::State)>>,
    /// The folder's branches, none when it is in no repository.
    branches: Option<(PathBuf, Rc<git::Branches>)>,
    git_reading: bool,
}

/// The comparison showing over the window: which file of the changed
/// ones, and its lines against the last commit.
pub struct Changes {
    root: PathBuf,
    picked: Option<PathBuf>,
    /// The lines of `picked`, once read.
    diff: Option<(PathBuf, Rc<git::Diff>)>,
    pub(crate) list: ListState,
    pub(crate) files: UniformListScrollHandle,
    reading: bool,
}

/// VS Code's colour for a git state, light then dark: the GitHub
/// theme's where it names one, else VS Code's own default.
fn git_rgb(state: git::State) -> (u32, u32) {
    use git::State::*;
    match state {
        Modified | TypeChanged => (0x005cc5, 0x79b8ff),
        Untracked | Added => (0x28a745, 0x34d058),
        Deleted => (0xd73a49, 0xea4a5a),
        Ignored => (0x959da5, 0x6a737d),
        Conflict => (0xe36209, 0xffab70),
        StagedModified => (0x895503, 0xe2c08d),
        StagedDeleted => (0xad0707, 0xc74e39),
        Renamed => (0x007100, 0x73c991),
    }
}

fn git_color(state: git::State, dark: bool) -> Hsla {
    let (light, night) = git_rgb(state);
    rgb(if dark { night } else { light }).into()
}

fn list_dir(dir: &Path) -> (Vec<Node>, usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return (Vec::new(), 0) };
    let mut nodes: Vec<Node> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".git" || name == ".DS_Store" {
                return None;
            }
            let path = e.path();
            let dir = match e.file_type() {
                Ok(t) if t.is_symlink() => path.is_dir(),
                Ok(t) => t.is_dir(),
                Err(_) => false,
            };
            Some(Node { path, name, dir })
        })
        .collect();
    nodes.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let dropped = nodes.len().saturating_sub(DIR_MAX);
    nodes.truncate(DIR_MAX);
    (nodes, dropped)
}

impl Tree {
    fn listing(&mut self, dir: &Path) -> Listing {
        self.kids.entry(dir.to_path_buf()).or_insert_with(|| Rc::new(list_dir(dir))).clone()
    }

    /// Every row under `root` that is showing, in order.
    fn rows(&mut self, root: &Path) -> Vec<Row> {
        let mut out = Vec::new();
        self.walk(root, 0, &mut out);
        out
    }

    fn walk(&mut self, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        let listing = self.listing(dir);
        for node in &listing.0 {
            let open = node.dir && self.open.contains(&node.path);
            out.push(Row::Node(node.clone(), depth, open));
            if open {
                self.walk(&node.path, depth + 1, out);
            }
        }
        if listing.1 > 0 {
            out.push(Row::More(listing.1, depth));
        }
    }

    /// Read `root` and every folder open under it again; whether anything
    /// differs from what was drawn.
    fn refresh(&mut self, root: &Path) -> bool {
        let mut changed = false;
        self.open.retain(|p| !p.starts_with(root) || p.is_dir());
        let dirs: Vec<PathBuf> = std::iter::once(root.to_path_buf()).chain(self.open.iter().filter(|p| p.starts_with(root)).cloned()).collect();
        for dir in dirs {
            let now = list_dir(&dir);
            if self.kids.get(&dir).is_none_or(|was| **was != now) {
                self.kids.insert(dir, Rc::new(now));
                changed = true;
            }
        }
        changed
    }
}

/// A file shown over the window.
pub struct FileView {
    pub path: PathBuf,
    /// Its path under the session's folder, which is how it is named.
    pub rel: String,
    pub size: u64,
    mtime: Option<std::time::SystemTime>,
    body: FileBody,
}

enum FileBody {
    /// Markdown, drawn as the conversation draws it.
    Markdown(String),
    /// Anything else that is text, as a fenced block in its language,
    /// with how many lines were left out.
    Code(String, usize),
    /// Not text, or not readable: why.
    None(String),
}

/// What a right click in the files panel can ask for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileDo {
    /// What the file holds against the last commit.
    Changes,
    Open,
    Reveal,
    Mention,
    CopyPath,
    CopyRel,
    Rename,
    NewFile,
    NewFolder,
    Trash,
}

/// A name being asked for in the field a session is renamed in: a new
/// name for that path, or a file or folder to make inside it.
#[derive(Clone)]
pub enum FilePrompt {
    Rename(PathBuf),
    NewFile(PathBuf),
    NewFolder(PathBuf),
}

impl FilePrompt {
    pub fn title(&self) -> &'static str {
        match self {
            FilePrompt::Rename(_) => "Rename",
            FilePrompt::NewFile(_) => "New file",
            FilePrompt::NewFolder(_) => "New folder",
        }
    }
}

fn is_picture(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "tiff"))
}

fn human_size(n: u64) -> String {
    match n {
        0..=999 => format!("{n} B"),
        1_000..=999_999 => format!("{:.1} kB", n as f64 / 1e3),
        1_000_000..=999_999_999 => format!("{:.1} MB", n as f64 / 1e6),
        _ => format!("{:.1} GB", n as f64 / 1e9),
    }
}

fn read_file(path: &Path, root: &Path) -> FileView {
    use std::io::Read as _;
    let meta = std::fs::metadata(path).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta.as_ref().and_then(|m| m.modified().ok());
    let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().to_string();
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(PREVIEW_BYTES as u64 + 1).read_to_end(&mut bytes));
    let body = match read {
        Err(e) => FileBody::None(format!("Could not read it: {e}")),
        Ok(_) if bytes.iter().take(8192).any(|b| *b == 0) => FileBody::None("Not a text file.".into()),
        Ok(_) => {
            let cut = bytes.len() > PREVIEW_BYTES;
            bytes.truncate(PREVIEW_BYTES);
            let text = String::from_utf8_lossy(&bytes).to_string();
            let lang = emaki_core::render_md::lang_for_path(&path.to_string_lossy());
            if lang == "markdown" && !cut {
                FileBody::Markdown(text)
            } else {
                let total = text.lines().count();
                let kept: String = text.lines().take(PREVIEW_LINES).collect::<Vec<_>>().join("\n");
                // A file cut by size has more lines than were counted.
                let dropped = total.saturating_sub(PREVIEW_LINES) + usize::from(cut);
                FileBody::Code(emaki_core::render_md::code_block(&kept, lang), dropped)
            }
        }
    };
    FileView { path: path.to_path_buf(), rel, size, mtime, body }
}

/// "Today", "Yesterday", or the date, for the outline's heads.
fn day_label(ts: &str) -> String {
    let Some(t) = emaki_core::build::parse_ts(ts) else { return String::new() };
    let d = t.with_timezone(&chrono::Local).date_naive();
    let today = chrono::Local::now().date_naive();
    if d == today {
        "Today".into()
    } else if Some(d) == today.pred_opt() {
        "Yesterday".into()
    } else if d.year() == today.year() {
        d.format("%b %-d").to_string()
    } else {
        d.format("%b %-d, %Y").to_string()
    }
}

/// A panel's head: its name, something straight after it, and something
/// at the right.
fn panel_head(label: &'static str, after: Option<AnyElement>, right: Option<AnyElement>, theme: &gpui_component::Theme) -> impl IntoElement {
    h_flex()
        .h(HEAD_H)
        .flex_shrink_0()
        .px(px(14.))
        .items_center()
        .bg(theme.muted.opacity(0.3))
        .border_t_1()
        .border_b_1()
        .border_color(theme.border)
        .text_size(px(11.))
        .gap(px(10.))
        .child(div().flex_shrink_0().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(label))
        .children(after)
        .child(div().flex_1())
        .children(right)
}

impl Workbench {
    /// The folder whose files the panel shows: the session's own.
    fn files_root(&self) -> Option<PathBuf> {
        let d = self.detail.as_ref()?;
        let cwd = if d.session.cwd.is_empty() { self.selected_ref()?.cwd.clone() } else { d.session.cwd.clone() };
        (!cwd.is_empty()).then(|| PathBuf::from(cwd))
    }

    /// Which of the two panels is drawn: (files, outline), one at most.
    pub(crate) fn panels_shown(&self) -> (bool, bool) {
        if self.page != Page::Session || self.detail.is_none() {
            return (false, false);
        }
        (self.files_on, self.outline_on && !self.files_on)
    }

    /// The panel chosen, whether or not this page draws it: files is
    /// `true`, the outline `false`.
    fn panel_on(&self) -> Option<bool> {
        if self.files_on {
            Some(true)
        } else if self.outline_on {
            Some(false)
        } else {
            None
        }
    }

    /// A panel's button was pressed: the one showing goes, and any other
    /// takes the place of the one that was.
    pub(crate) fn toggle_panel(&mut self, files: bool, cx: &mut Context<Self>) {
        let from = self.panel_on();
        let to = (from != Some(files)).then_some(files);
        self.files_on = to == Some(true);
        self.outline_on = to == Some(false);
        self.outline_at = None;
        self.panel_anim = Some((from, to, std::time::Instant::now(), self.panel_anim.map(|(_, _, _, n)| n + 1).unwrap_or(0)));
        self.save_ui(true);
        cx.notify();
    }

    /// The two buttons at the top strip's left end, as one control: a
    /// track with a segment each, and a raised plate under the one whose
    /// panel shows. The plate slides from one to the other, comes in
    /// under the first pressed and fades under the one pressed again.
    pub(crate) fn panel_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let track_bg = if dark { theme.sidebar } else { theme.muted };
        let plate_bg = if dark { theme.secondary_active } else { theme.popover };
        let (seg_w, seg_h, pad, gap) = (30., 24., 3., 2.);
        let x_of = |which: bool| pad + if which { 0. } else { seg_w + gap };
        let on = self.panel_on();
        // Without a press since launch there is nothing to move from.
        let (from, to, serial) = self.panel_anim.map(|(from, to, _, n)| (from, to, n + 1)).unwrap_or((on, on, 0));
        let plate = (from.is_some() || to.is_some()).then(|| {
            let (a, b) = (x_of(from.or(to).unwrap_or(true)), x_of(to.or(from).unwrap_or(true)));
            let (o_a, o_b) = (if from.is_some() { 1. } else { 0. }, if to.is_some() { 1. } else { 0. });
            div().absolute().top(px(pad)).w(px(seg_w)).h(px(seg_h)).rounded(px(7.)).bg(plate_bg).shadow_sm().with_animation(
                ElementId::Name(format!("panel-plate-{serial}").into()),
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                move |d, t| d.left(px(a + (b - a) * t)).opacity(o_a + (o_b - o_a) * t),
            )
        });
        let segment = |id: &'static str, icon: &'static str, tip: &'static str, which: bool, cx: &mut Context<Self>| {
            let active = on == Some(which);
            h_flex()
                .id(id)
                .w(px(seg_w))
                .h(px(seg_h))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .cursor_pointer()
                .text_color(if active { theme.foreground } else { theme.muted_foreground })
                .when(!active, |d| d.hover(|s| s.text_color(theme.foreground)))
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip).build(window, cx))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_panel(which, cx)))
                .child(Icon::default().path(icon).with_size(px(15.)))
        };
        vec![h_flex()
            .relative()
            .flex_shrink_0()
            .p(px(pad))
            .gap(px(gap))
            .rounded(px(9.))
            .bg(track_bg)
            .children(plate)
            .child(segment("panel-files", "icons/tree-view.svg", "Files (⌘⇧E)", true, cx))
            .child(segment("panel-outline", "icons/list-bullets.svg", "Outline (⌘⇧O)", false, cx))
            .into_any_element()]
    }

    /// A panel as it arrives: beside nothing it widens from nothing and
    /// fades in; in the other's place it only fades in, so the
    /// conversation stays where it is.
    fn panel_in(&self, files: bool, el: Div) -> AnyElement {
        match self.panel_anim.filter(|(_, to, at, _)| *to == Some(files) && at.elapsed() < crate::workbench::PANEL_ANIM) {
            Some((from, _, _, serial)) => {
                let fresh = from.is_none();
                let w = self.panel_w;
                div()
                    .h_full()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(el)
                    .with_animation(ElementId::Name(format!("panel-{files}-{serial}").into()), Animation::new(crate::workbench::PANEL_ANIM).with_easing(ease_out_quint()), move |d, t| d.w(if fresh { (w * t).round() } else { w }).opacity(t))
                    .into_any_element()
            }
            None => el.into_any_element(),
        }
    }

    // -- the panel's edge ---------------------------------------------------

    /// Where the panel begins: after the sidebar when that is beside the
    /// content.
    fn panel_left(&self) -> Pixels {
        if self.sidebar_open && !self.narrow { self.sidebar_w } else { px(0.) }
    }

    /// The widest the panel may be dragged: the conversation keeps
    /// `CONVERSATION_MIN` beside it.
    fn panel_max(&self) -> Pixels {
        PANEL_MAX.min(self.pane_w - CONVERSATION_MIN).max(PANEL_MIN)
    }

    /// The panel's edge follows the pointer while it is held, between the
    /// least and the most; well under the least the panel is put away,
    /// and brought back out in the same drag it is there again. As the
    /// sidebar's edge does it.
    pub(crate) fn panel_drag_to(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(files) = self.panel_drag else { return };
        if e.pressed_button != Some(MouseButton::Left) {
            self.panel_drag_end();
            return;
        }
        let w = e.position.x - self.panel_left();
        let open = w >= PANEL_FOLD_AT;
        let was = self.panel_on();
        let now = open.then_some(files);
        if now != was {
            self.files_on = now == Some(true);
            self.outline_on = now == Some(false);
            self.outline_at = None;
            // The control's plate follows; the panel itself is under the
            // pointer and does not play its arrival.
            let long_ago = std::time::Instant::now().checked_sub(crate::workbench::PANEL_ANIM).unwrap_or_else(std::time::Instant::now);
            self.panel_anim = Some((was, now, long_ago, self.panel_anim.map(|(_, _, _, n)| n + 1).unwrap_or(0)));
            cx.notify();
        }
        if open {
            let w = w.clamp(PANEL_MIN, self.panel_max());
            if w != self.panel_w {
                self.panel_w = w;
                cx.notify();
            }
        }
    }

    pub(crate) fn panel_drag_end(&mut self) {
        if self.panel_drag.take().is_some() {
            self.save_ui(true);
        }
    }

    /// The width a double click on the edge goes to: for the files, the
    /// widest row showing, as it would be laid out; for the outline, a
    /// width its lines read well at.
    pub(crate) fn panel_fit(&mut self, window: &Window, cx: &App) -> Pixels {
        if !self.files_on {
            return OUTLINE_FIT.min(self.panel_max());
        }
        let Some(root) = self.files_root() else { return PANEL_W };
        let font = font(cx.theme().font_family.clone());
        let measure = |text: &str| {
            let run = TextRun { len: text.len(), font: font.clone(), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(text.to_string().into(), px(12.5), &[run], None).width)
        };
        // Around a name: the scroller's padding on both sides and the
        // panel's edge (13), the row's indent, chevron, icon, gaps, the
        // git badge and right padding, and a little air.
        let widest = self
            .tree
            .rows(&root)
            .iter()
            .filter_map(|row| match row {
                Row::Node(node, depth, _) => Some(13. + 8. + 12. * *depth as f32 + 10. + 5. + 16. + 5. + measure(&node.name) + 5. + 12. + 8. + 10.),
                Row::More(..) => None,
            })
            .fold(0f32, f32::max);
        px(widest.ceil()).clamp(PANEL_MIN, PANEL_FIT_MAX.min(self.panel_max()))
    }

    /// The strip over the panel's edge that takes the drag and the double
    /// click, lit under the pointer as the sidebar's is.
    pub(crate) fn render_panel_grip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (files, outline) = self.panels_shown();
        if !(files || outline) {
            return None;
        }
        let line = cx.theme().primary.opacity(0.55);
        let held = self.panel_drag.is_some();
        Some(
            div()
                .id("panel-grip")
                .group("panel-grip")
                .absolute()
                .top(crate::workbench::TITLEBAR_H)
                .bottom_0()
                .left(self.panel_left() + self.panel_w - PANEL_GRIP / 2.)
                .w(PANEL_GRIP)
                .occlude()
                .cursor(CursorStyle::ResizeLeftRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        if ev.click_count == 2 {
                            this.panel_drag = None;
                            this.panel_w = this.panel_fit(window, cx);
                            this.save_ui(true);
                        } else {
                            this.panel_drag = Some(files);
                        }
                        swallow_click(window, cx);
                        cx.notify();
                    }),
                )
                .child(div().absolute().top_0().h_full().left(PANEL_GRIP / 2. - px(1.5)).w(px(2.)).when(held, |d| d.bg(line)).group_hover("panel-grip", move |s| s.bg(line))),
        )
    }

    // -- files ----------------------------------------------------------------

    /// Once a second: the tree read again when it is due, and the file
    /// showing read again when it has changed on disk.
    pub(crate) fn tick_files(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = &self.file_view {
            let now = std::fs::metadata(&v.path).ok().and_then(|m| m.modified().ok());
            if now.is_some() && now != v.mtime {
                if let Some(root) = self.files_root() {
                    self.file_view = Some(read_file(&v.path.clone(), &root));
                    cx.notify();
                }
            }
        }
        if (!self.panels_shown().0 && self.changes.is_none()) || self.now - self.tree.read_at < TREE_SECS {
            return;
        }
        self.tree.read_at = self.now;
        self.load_diff(cx);
        if let Some(root) = self.files_root() {
            if self.tree.refresh(&root) {
                cx.notify();
            }
            self.read_git(root, cx);
        }
    }

    /// Ask git about the folder, off the main thread, one asking at a
    /// time; the tree is drawn again when the answer differs.
    fn read_git(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.tree.git_reading {
            return;
        }
        self.tree.git_reading = true;
        cx.spawn(async move |this, cx| {
            let dir = root.clone();
            let (status, branches) = cx.background_executor().spawn(async move { (git::status(&dir).unwrap_or_default(), git::branches(&dir)) }).await;
            this.update(cx, |this, cx| {
                this.tree.git_reading = false;
                if this.tree.git.as_ref().is_none_or(|(was, st)| *was != root || **st != status) {
                    this.tree.changed = Rc::new(status.changed());
                    this.tree.git = Some((root.clone(), Rc::new(status)));
                    cx.notify();
                }
                let branches = branches.map(|b| (root, Rc::new(b)));
                if this.tree.branches != branches {
                    this.tree.branches = branches;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn tree_toggle(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.tree.open.remove(path) {
            self.tree.open.insert(path.to_path_buf());
            // What it holds now, not what it held when last opened.
            self.tree.kids.remove(path);
        }
        cx.notify();
    }

    /// A click on a file: a picture opens in the lightbox, anything else
    /// in the sheet that shows a file.
    pub(crate) fn file_preview(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        self.tree.picked = Some(path.to_path_buf());
        if is_picture(path) {
            let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().to_string();
            self.preview_attachment(rel, Some(path.to_path_buf()), Some(ImageSource::from(path.to_path_buf())), window, cx);
            return;
        }
        self.file_view = Some(read_file(path, &root));
        self.file_view_scroll.set_offset(point(px(0.), px(0.)));
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(crate) fn close_file_view(&mut self, cx: &mut Context<Self>) {
        self.file_view = None;
        cx.notify();
    }

    fn file_menu(&mut self, node: Option<&Node>, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let item = |label: &'static str, what: FileDo, path: &Path| (label, MenuDo::File(what, path.to_path_buf()));
        let items = match node {
            Some(n) if n.dir => vec![
                item(crate::sys::OPEN_FOLDER_LABEL, FileDo::Open, &n.path),
                item("Add to message", FileDo::Mention, &n.path),
                item("New file", FileDo::NewFile, &n.path),
                item("New folder", FileDo::NewFolder, &n.path),
                item("Copy path", FileDo::CopyPath, &n.path),
                item("Copy relative path", FileDo::CopyRel, &n.path),
                item("Rename", FileDo::Rename, &n.path),
                item(crate::sys::TRASH_LABEL, FileDo::Trash, &n.path),
            ],
            Some(n) if self.tree.changed.iter().any(|(p, _)| *p == n.path) => vec![
                item("Show changes", FileDo::Changes, &n.path),
                item("Open", FileDo::Open, &n.path),
                item(crate::sys::REVEAL_LABEL, FileDo::Reveal, &n.path),
                item("Add to message", FileDo::Mention, &n.path),
                item("Copy path", FileDo::CopyPath, &n.path),
                item("Copy relative path", FileDo::CopyRel, &n.path),
                item("Rename", FileDo::Rename, &n.path),
                item(crate::sys::TRASH_LABEL, FileDo::Trash, &n.path),
            ],
            Some(n) => vec![
                item("Open", FileDo::Open, &n.path),
                item(crate::sys::REVEAL_LABEL, FileDo::Reveal, &n.path),
                item("Add to message", FileDo::Mention, &n.path),
                item("Copy path", FileDo::CopyPath, &n.path),
                item("Copy relative path", FileDo::CopyRel, &n.path),
                item("Rename", FileDo::Rename, &n.path),
                item(crate::sys::TRASH_LABEL, FileDo::Trash, &n.path),
            ],
            None => vec![item(crate::sys::OPEN_FOLDER_LABEL, FileDo::Open, &root), item("New file", FileDo::NewFile, &root), item("New folder", FileDo::NewFolder, &root), item("Copy path", FileDo::CopyPath, &root)],
        };
        self.open_menu(at, items, cx);
    }

    /// A choice from the files panel's menu.
    pub(crate) fn file_do(&mut self, what: FileDo, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let rel = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().to_string();
        let copy = |text: String, this: &mut Self, cx: &mut Context<Self>| {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            this.notice = Some(Notice::said("copied"));
        };
        match what {
            FileDo::Changes => self.open_changes(Some(path), cx),
            FileDo::Open => crate::sys::open_path(&path),
            FileDo::Reveal => crate::sys::reveal_path(&path),
            FileDo::CopyPath => copy(path.to_string_lossy().to_string(), self, cx),
            FileDo::CopyRel => copy(rel, self, cx),
            FileDo::Mention => self.file_mention(&path, window, cx),
            FileDo::Rename | FileDo::NewFile | FileDo::NewFolder => {
                let (prompt, value) = match what {
                    FileDo::Rename => (FilePrompt::Rename(path.clone()), path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                    FileDo::NewFile => (FilePrompt::NewFile(path), String::new()),
                    _ => (FilePrompt::NewFolder(path), String::new()),
                };
                self.file_prompt = Some(prompt);
                self.rename_input.update(cx, |s, cx| {
                    s.set_value(value, window, cx);
                    s.focus(window, cx);
                });
            }
            FileDo::Trash => match crate::sys::trash_path(&path) {
                Ok(()) => {
                    if self.file_view.as_ref().is_some_and(|v| v.path.starts_with(&path)) {
                        self.file_view = None;
                    }
                    self.tree.refresh(&root);
                    self.notice = Some(Notice::said(format!("{} is in the {}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), crate::sys::TRASH_NAME)));
                }
                Err(e) => self.notice = Some(Notice::error(format!("not moved: {e}"))),
            },
        }
        cx.notify();
    }

    /// "@path" at the end of what is typed, as the composer's own "@"
    /// writes it, with the caret after it.
    pub(crate) fn file_mention(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let mut rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            rel.push('/');
        }
        let mut text = self.composer.read(cx).value().to_string();
        if !text.is_empty() && !text.ends_with([' ', '\n']) {
            text.push(' ');
        }
        text.push_str(&emaki_core::files::written(&rel));
        text.push(' ');
        let caret = text.len();
        self.set_composer(text, caret, window, cx);
        let handle = self.composer.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// ↩ in the field while it asks for a file's name: the rename, or the
    /// new file or folder, is made; a name that cannot be used says why
    /// and leaves the field up.
    pub(crate) fn commit_file_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.file_prompt.clone() else { return };
        let name = self.rename_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.close_rename(window, cx);
            return;
        }
        let made = (|| -> Result<Option<PathBuf>, String> {
            // A name, or for something new a path under the folder; never
            // a way out of it.
            let nested = !matches!(prompt, FilePrompt::Rename(_));
            let parts: Vec<Component> = Path::new(&name).components().collect();
            if parts.iter().any(|c| !matches!(c, Component::Normal(_))) || (!nested && parts.len() != 1) {
                return Err("that is not a name".into());
            }
            match &prompt {
                FilePrompt::Rename(from) => {
                    let to = from.with_file_name(&name);
                    if to == *from {
                        return Ok(None);
                    }
                    if to.exists() {
                        return Err(format!("{name} is already there"));
                    }
                    std::fs::rename(from, &to).map_err(|e| e.to_string())?;
                    // What was open under the old name is open under the new.
                    let moved: Vec<PathBuf> = self.tree.open.iter().filter(|p| p.starts_with(from)).cloned().collect();
                    for p in moved {
                        self.tree.open.remove(&p);
                        if let Ok(rest) = p.strip_prefix(from) {
                            self.tree.open.insert(if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) });
                        }
                    }
                    if self.file_view.as_ref().is_some_and(|v| v.path.starts_with(from)) {
                        self.file_view = None;
                    }
                    Ok(None)
                }
                FilePrompt::NewFile(dir) | FilePrompt::NewFolder(dir) => {
                    let to = dir.join(&name);
                    if to.exists() {
                        return Err(format!("{name} is already there"));
                    }
                    let file = matches!(prompt, FilePrompt::NewFile(_));
                    let folder = if file { to.parent().unwrap_or(dir).to_path_buf() } else { to.clone() };
                    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                    if file {
                        std::fs::OpenOptions::new().write(true).create_new(true).open(&to).map_err(|e| e.to_string())?;
                    }
                    // Every folder down to it is opened, so it is seen.
                    let mut up = to.parent();
                    while let Some(p) = up.filter(|p| p.starts_with(dir)) {
                        self.tree.open.insert(p.to_path_buf());
                        up = p.parent();
                    }
                    Ok(file.then_some(to))
                }
            }
        })();
        match made {
            Ok(picked) => {
                if let Some(p) = picked {
                    self.tree.picked = Some(p);
                }
                if let Some(root) = self.files_root() {
                    self.tree.refresh(&root);
                }
                self.close_rename(window, cx);
            }
            Err(why) => {
                self.notice = Some(Notice::error(why));
                cx.notify();
            }
        }
    }

    pub(crate) fn render_files_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let root = self.files_root();
        let exists = root.as_ref().is_some_and(|r| r.is_dir());
        let rows = match &root {
            Some(r) if exists => self.tree.rows(r),
            _ => Vec::new(),
        };
        let dark = theme.mode.is_dark();
        // Git's word on the folder; asked for at once when there is none
        // for it yet, so the colours do not wait for the clock.
        let status = self.tree.git.as_ref().filter(|(of, _)| Some(of) == root.as_ref()).map(|(_, st)| st.clone());
        if status.is_none() && exists {
            if let Some(r) = root.clone() {
                self.read_git(r, cx);
            }
        }
        let picked = self.tree.picked.clone();
        let mut list = v_flex().w_full();
        for (ix, row) in rows.into_iter().enumerate() {
            let pad = |depth: usize| px(8. + 12. * depth as f32);
            match row {
                Row::More(n, depth) => {
                    list = list.child(h_flex().h(TREE_ROW_H).flex_shrink_0().pl(pad(depth) + px(15.)).text_size(px(11.5)).text_color(theme.muted_foreground).child(format!("{n} more")));
                }
                Row::Node(node, depth, open) => {
                    let chosen = picked.as_ref() == Some(&node.path);
                    let mark = status.as_ref().and_then(|st| st.mark(&node.path, node.dir));
                    let tint = mark.map(|m| git_color(m.state, dark));
                    let ink = tint.unwrap_or(theme.foreground);
                    // At the row's right, as VS Code has it: a file's
                    // letter, a dot on a folder holding a change, and
                    // nothing on what is ignored.
                    let badge = mark.filter(|m| m.state != git::State::Ignored).zip(tint).map(|(m, tint)| match m.state.letter() {
                        Some(letter) if !m.folder => div().w(px(12.)).flex_shrink_0().flex().justify_center().text_size(px(11.)).text_color(tint).child(letter).into_any_element(),
                        _ => div().w(px(12.)).flex_shrink_0().flex().justify_center().child(div().size(px(5.)).rounded_full().bg(tint.opacity(0.8))).into_any_element(),
                    });
                    let (click, menu) = (node.clone(), node.clone());
                    let icon = img(crate::file_icons::path(&node.name, node.dir, open, dark)).size(px(16.)).flex_shrink_0();
                    list = list.child(
                        h_flex()
                            .id(("tree-row", ix))
                            .h(TREE_ROW_H)
                            .flex_shrink_0()
                            .pl(pad(depth))
                            .pr(px(8.))
                            .gap(px(5.))
                            .items_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .when(chosen, |d| d.bg(theme.primary.opacity(0.12)))
                            .when(!chosen, |d| d.hover(|s| s.bg(theme.muted.opacity(0.7))))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if click.dir {
                                    this.tree_toggle(&click.path, cx)
                                } else {
                                    this.file_preview(&click.path, window, cx)
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.file_menu(Some(&menu), ev.position, cx)
                                }),
                            )
                            .child(div().w(px(10.)).flex_shrink_0().flex().justify_center().when(node.dir, |d| d.child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(10.)).text_color(theme.muted_foreground))))
                            .child(icon)
                            .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(ink).child(node.name.clone()))
                            .children(badge),
                    );
                }
            }
        }
        let body = if exists {
            v_flex()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    v_flex()
                        .id("files-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.files_scroll)
                        .px(px(6.))
                        .py(px(6.))
                        .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, _, cx| this.file_menu(None, ev.position, cx)))
                        .child(list),
                )
                .vertical_scrollbar(&self.files_scroll)
                .into_any_element()
        } else {
            div().p(px(14.)).text_size(px(12.)).text_color(theme.muted_foreground).child("The session's folder is gone.").into_any_element()
        };
        let el = v_flex().w(self.panel_w).h_full().flex_shrink_0().border_r_1().border_color(theme.border).bg(theme.sidebar).child(panel_head("Files", self.branch_pill(cx), self.changes_pill(cx), &theme)).child(body);
        self.panel_in(true, el)
    }

    // -- branches -------------------------------------------------------------

    /// The folder's branches, when it is in a repository.
    fn branches(&self) -> Option<Rc<git::Branches>> {
        let root = self.files_root()?;
        self.tree.branches.as_ref().filter(|(of, _)| *of == root).map(|(_, b)| b.clone())
    }

    /// At the right of the files' head: the branch checked out, on a
    /// button that opens the list of them.
    fn branch_pill(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let b = self.branches()?;
        let open = self.branch_menu.is_some();
        Some(
            h_flex()
                .id("branch-pill")
                .min_w_0()
                .h(px(21.))
                .px(px(7.))
                .gap(px(4.))
                .items_center()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.border)
                .bg(if open { theme.muted } else { theme.background.opacity(0.6) })
                .hover(|s| s.bg(theme.muted))
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(theme.foreground)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, ev: &ClickEvent, window, cx| this.branch_menu_toggle(ev.position(), window, cx)))
                .child(Icon::default().path("icons/git-branch.svg").with_size(px(12.)).text_color(theme.muted_foreground).flex_shrink_0())
                .child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).child(b.label()))
                .child(Icon::new(IconName::ChevronDown).with_size(px(9.)).text_color(theme.muted_foreground).flex_shrink_0())
                .into_any_element(),
        )
    }

    pub(crate) fn branch_menu_toggle(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if self.branch_menu.take().is_none() {
            self.branch_menu = Some(at);
            self.branch_input.update(cx, |s, cx| {
                s.set_value("", window, cx);
                s.focus(window, cx);
            });
            // The list as it is now, not as the clock last read it.
            if let Some(root) = self.files_root() {
                self.read_git(root, cx);
            }
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn close_branch_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.branch_menu = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The branches the field's words leave, local ones first, and
    /// whether the words name none of them and could name a new one.
    fn branch_matches(&self, cx: &App) -> (Vec<(String, bool)>, Option<String>) {
        let Some(b) = self.branches() else { return (Vec::new(), None) };
        let typed = self.branch_input.read(cx).value().trim().to_string();
        let low = typed.to_lowercase();
        let rows: Vec<(String, bool)> = b.local.iter().map(|n| (n.clone(), false)).chain(b.remote.iter().map(|n| (n.clone(), true))).filter(|(n, _)| n.to_lowercase().contains(&low)).collect();
        let fresh = (!typed.is_empty() && !b.has(&typed)).then_some(typed);
        (rows, fresh)
    }

    /// ↩ in the field: the branch the words name, else the first they
    /// leave, else a new one of that name.
    pub(crate) fn branch_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (rows, fresh) = self.branch_matches(cx);
        let typed = self.branch_input.read(cx).value().trim().to_string();
        match (rows.iter().find(|(n, _)| *n == typed).or(rows.first()), fresh) {
            (Some((name, _)), _) => self.branch_go(name.clone(), false, window, cx),
            (None, Some(name)) => self.branch_go(name, true, window, cx),
            _ => {}
        }
    }

    /// Check a branch out, or make one and check it out. Git does it
    /// and git may refuse; nothing is forced. Not under a running turn:
    /// the agent is writing to the files a switch would change.
    fn branch_go(&mut self, name: String, create: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_branch_menu(window, cx);
        let Some(root) = self.files_root() else { return };
        if self.branches().is_some_and(|b| b.current.as_deref() == Some(&name)) || self.branch_busy {
            return;
        }
        if self.selected_ref().is_some_and(|r| self.is_working(r)) {
            self.notice = Some(Notice::error("a turn is running here; switch branches when it is over"));
            return;
        }
        if create && !git::valid_name(&name) {
            self.notice = Some(Notice::error(format!("\u{201c}{name}\u{201d} is not a name git takes for a branch")));
            return;
        }
        self.branch_busy = true;
        cx.spawn(async move |this, cx| {
            let (dir, to) = (root.clone(), name.clone());
            let done = cx.background_executor().spawn(async move { if create { git::create(&dir, &to) } else { git::switch(&dir, &to) } }).await;
            this.update(cx, |this, cx| {
                this.branch_busy = false;
                this.notice = Some(match done {
                    Ok(()) if create => Notice::said(format!("made {name} and switched to it")),
                    Ok(()) => Notice::said(format!("switched to {name}")),
                    Err(why) => Notice::error(format!("not switched: {why}")),
                });
                // The files are another branch's now.
                this.tree.refresh(&root);
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The list of branches: a card under the button, over a clear
    /// sheet any click puts away, as GitHub's is laid out: a head, a
    /// field that narrows the list or names a new branch, the branches
    /// with a tick on the one checked out.
    pub(crate) fn render_branch_menu(&self, at: Point<Pixels>, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(b) = self.branches() else { return div().into_any_element() };
        let (rows, fresh) = self.branch_matches(cx);
        let focus = self.branch_input.read(cx).focus_handle(cx);
        let typed = self.branch_input.read(cx).value().to_string();
        let view = window.viewport_size();
        let w = px(300.).min(view.width - px(16.));
        let x = (at.x - px(40.)).min(view.width - w - px(8.)).max(px(8.));
        let y = (at.y + px(16.)).min(view.height - px(200.)).max(px(8.));
        let chip = |word: &'static str| div().flex_shrink_0().px(px(6.)).rounded_full().border_1().border_color(theme.border).text_size(px(10.5)).text_color(theme.muted_foreground).child(word);
        let mut list = v_flex().w_full();
        for (ix, (name, remote)) in rows.iter().enumerate() {
            let here = b.current.as_deref() == Some(name.as_str());
            let to = name.clone();
            list = list.child(
                h_flex()
                    .id(("branch-row", ix))
                    .h(BRANCH_ROW_H)
                    .flex_shrink_0()
                    .px(px(8.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .hover(|s| s.bg(theme.sidebar_accent))
                    .on_click(cx.listener(move |this, _, window, cx| this.branch_go(to.clone(), false, window, cx)))
                    .child(div().w(px(14.)).flex_shrink_0().when(here, |d| d.child(Icon::new(IconName::Check).with_size(px(13.)))))
                    .child(div().flex_1().min_w_0().truncate().when(here, |d| d.font_weight(FontWeight::MEDIUM)).child(name.clone()))
                    .when(b.default.as_deref() == Some(name.as_str()), |d| d.child(chip("default")))
                    .when(*remote, |d| d.child(chip("remote"))),
            );
        }
        if rows.is_empty() && fresh.is_none() {
            list = list.child(div().px(px(8.)).py(px(8.)).text_size(px(12.5)).text_color(theme.muted_foreground).child("No branch of that name."));
        }
        let make = fresh.map(|name| {
            let to = name.clone();
            h_flex()
                .id("branch-make")
                .min_h(px(34.))
                .px(px(12.))
                .py(px(6.))
                .gap(px(7.))
                .items_center()
                .border_t_1()
                .border_color(theme.border)
                .cursor_pointer()
                .text_size(px(12.5))
                .hover(|s| s.bg(theme.sidebar_accent))
                .on_click(cx.listener(move |this, _, window, cx| this.branch_go(to.clone(), true, window, cx)))
                .child(Icon::default().path("icons/git-branch.svg").with_size(px(13.)).text_color(theme.muted_foreground).flex_shrink_0())
                .child(div().min_w_0().child(StyledText::new(format!("Create branch {name} from {}", b.label())).with_highlights([(14..14 + name.len(), HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() })])))
        });
        let card = v_flex()
            .id("branch-card")
            .key_context(crate::workbench::SEARCH_CONTEXT)
            .on_action(cx.listener(|this, _: &crate::workbench::Escape, window, cx| this.close_branch_menu(window, cx)))
            .absolute()
            .left(x)
            .top(y)
            .w(w)
            .max_h(view.height - y - px(8.))
            .rounded(px(12.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow(float_shadow(&theme))
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, window, cx| swallow_click(window, cx))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .pl(px(14.))
                    .pr(px(8.))
                    .pt(px(10.))
                    .pb(px(8.))
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Switch branches"))
                    .child(Button::new("branch-close").ghost().xsmall().icon(IconName::Close).on_click(cx.listener(|this, _, window, cx| this.close_branch_menu(window, cx)))),
            )
            .child(
                h_flex()
                    .id("branch-field")
                    .track_focus(&focus)
                    .role(Role::TextInput)
                    .aria_label("Find or create a branch")
                    .aria_value(typed)
                    .flex_shrink_0()
                    .mx(px(10.))
                    .mb(px(10.))
                    .px(px(8.))
                    .h(px(32.))
                    .gap(px(4.))
                    .items_center()
                    .rounded(px(8.))
                    .bg(theme.muted)
                    .text_size(px(13.))
                    .child(Icon::new(IconName::Search).with_size(px(13.)).text_color(theme.muted_foreground).flex_shrink_0())
                    .child(div().flex_1().min_w_0().child(gpui_component::input::Input::new(&self.branch_input).appearance(false).bordered(false))),
            )
            .when(!(rows.is_empty() && make.is_some()), |d| {
                d.child(
                    v_flex()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .border_t_1()
                        .border_color(theme.border)
                        .child(v_flex().id("branch-scroll").max_h(BRANCH_ROW_H * BRANCH_ROWS as f32 + px(10.)).overflow_y_scroll().track_scroll(&self.branch_scroll).p(px(5.)).child(list))
                        .vertical_scrollbar(&self.branch_scroll),
                )
            })
            .children(make);
        let shut = |this: &mut Self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>| this.close_branch_menu(window, cx);
        div().id("branch-sheet").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(shut)).on_mouse_down(MouseButton::Right, cx.listener(shut)).child(card).into_any_element()
    }

    // -- the comparison -------------------------------------------------------

    /// At the right of the files' head: how many files have a change,
    /// on a button that opens the comparison.
    fn changes_pill(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let root = self.files_root()?;
        let n = self.tree.git.as_ref().filter(|(of, _)| *of == root).map(|_| self.tree.changed.len()).filter(|n| *n > 0)?;
        Some(
            h_flex()
                .id("changes-pill")
                .flex_shrink_0()
                .h(px(21.))
                .px(px(6.))
                .gap(px(4.))
                .items_center()
                .rounded(px(6.))
                .hover(|s| s.bg(theme.muted))
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(theme.muted_foreground)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| this.open_changes(None, cx)))
                .child(Icon::default().path("icons/git-diff.svg").with_size(px(12.)).flex_shrink_0())
                .child(crate::format::thousands(n))
                .into_any_element(),
        )
    }

    /// Show the comparison, on that file or on the first changed one.
    pub(crate) fn open_changes(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let picked = path.or_else(|| self.tree.changed.first().map(|(p, _)| p.clone()));
        let files = UniformListScrollHandle::new();
        if let Some(at) = picked.as_ref().and_then(|p| self.tree.changed.iter().position(|(c, _)| c == p)) {
            files.scroll_to_item(at, ScrollStrategy::Center);
        }
        self.changes = Some(Changes { root, picked, diff: None, list: ListState::new(0, ListAlignment::Top, px(600.)), files, reading: false });
        self.load_diff(cx);
        cx.notify();
    }

    pub(crate) fn close_changes(&mut self, cx: &mut Context<Self>) {
        self.changes = None;
        cx.notify();
    }

    fn pick_change(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        if c.picked.as_ref() != Some(&path) {
            c.picked = Some(path);
            c.reading = false;
            self.load_diff(cx);
            cx.notify();
        }
    }

    /// Read the picked file's lines against the last commit, off the
    /// main thread. Run again on the clock while the comparison shows;
    /// the lines are replaced only when they differ, so reading on does
    /// not move the reader.
    fn load_diff(&mut self, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        let Some(path) = c.picked.clone() else { return };
        if c.reading {
            return;
        }
        c.reading = true;
        let root = c.root.clone();
        let state = self.tree.changed.iter().find(|(p, _)| *p == path).map(|(_, s)| *s).unwrap_or(git::State::Modified);
        cx.spawn(async move |this, cx| {
            let of = path.clone();
            let diff = cx.background_executor().spawn(async move { git::diff(&root, &of, state) }).await;
            this.update(cx, |this, cx| {
                let Some(c) = &mut this.changes else { return };
                if c.picked.as_ref() != Some(&path) {
                    return;
                }
                c.reading = false;
                if c.diff.as_ref().is_none_or(|(was, d)| *was != path || **d != diff) {
                    c.list.reset(diff.rows.len());
                    c.diff = Some((path, Rc::new(diff)));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The comparison: a sheet over the window, the changed files at
    /// its left and the picked one's lines at its right, what was beside
    /// what is, as GitHub Desktop sets them.
    pub(crate) fn render_changes(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(c) = &self.changes else { return div().into_any_element() };
        let dark = theme.mode.is_dark();
        let changed = self.tree.changed.clone();
        let root = c.root.clone();
        let picked = c.picked.clone();
        let entity = cx.weak_entity();
        let (hover, chosen_bg, ink, quiet, mono) = (theme.muted.opacity(0.7), theme.primary.opacity(if dark { 0.18 } else { 0.12 }), theme.foreground, theme.muted_foreground, theme.mono_font_family.clone());
        let files = uniform_list("changes-files", changed.len(), {
            let (changed, root, picked) = (changed.clone(), root.clone(), picked.clone());
            move |range: std::ops::Range<usize>, _: &mut Window, _: &mut App| {
                range
                    .map(|ix| {
                        let (path, state) = changed[ix].clone();
                        let rel = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                        let (folder, name) = match rel.rfind('/') {
                            Some(cut) => (rel[..=cut].to_string(), rel[cut + 1..].to_string()),
                            None => (String::new(), rel.clone()),
                        };
                        let tint = git_color(state, dark);
                        let chosen = picked.as_ref() == Some(&path);
                        let entity = entity.clone();
                        let row = h_flex()
                            .id(("change-row", ix))
                            .w_full()
                            .h(CHANGE_ROW_H)
                            .px(px(8.))
                            .gap(px(6.))
                            .items_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .text_size(px(12.5))
                            .when(chosen, |d| d.bg(chosen_bg))
                            .when(!chosen, |d| d.hover(move |s| s.bg(hover)))
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| this.pick_change(path.clone(), cx)).ok();
                            })
                            .child(img(crate::file_icons::path(&name, false, false, dark)).size(px(16.)).flex_shrink_0())
                            // The folders give way first, so the name stays whole.
                            .child(h_flex().flex_1().min_w_0().child(div().min_w_0().truncate().text_color(quiet).child(folder)).child(div().flex_shrink_0().text_color(ink).child(name)))
                            .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().text_size(px(11.)).text_color(tint).children(state.letter()));
                        // The list gives a row its whole width; the plate is inset from it.
                        div().w_full().h(CHANGE_ROW_H).px(px(6.)).child(row)
                    })
                    .collect::<Vec<_>>()
            }
        })
        .track_scroll(&c.files)
        .size_full();

        let shown = c.diff.as_ref().filter(|(of, _)| Some(of) == picked.as_ref()).map(|(_, d)| d.clone());
        let rel = picked.as_ref().map(|p| p.strip_prefix(&root).unwrap_or(p).to_string_lossy().replace('\\', "/")).unwrap_or_default();
        // GitHub's own colours for a line put in and a line taken out:
        // the line's ground, and a stronger one behind its number.
        let rgba_of = |hex: u32, a: f32| Hsla::from(rgb(hex)).opacity(a);
        let (add, add_no, del, del_no) = if dark { (rgba_of(0x2ea043, 0.16), rgba_of(0x3fb950, 0.30), rgba_of(0xf85149, 0.14), rgba_of(0xf85149, 0.30)) } else { (Hsla::from(rgb(0xe6ffec)), Hsla::from(rgb(0xccffd8)), Hsla::from(rgb(0xffebe9)), Hsla::from(rgb(0xffd7d5))) };
        let body = match &shown {
            None => div().p(px(20.)).text_size(px(12.5)).text_color(quiet).child(if picked.is_some() { "Reading…" } else { "Nothing has changed since the last commit." }).into_any_element(),
            Some(d) if d.binary => div().p(px(20.)).text_size(px(12.5)).text_color(quiet).child("This file is not text, so there are no lines to compare.").into_any_element(),
            Some(d) if d.rows.is_empty() => div().p(px(20.)).text_size(px(12.5)).text_color(quiet).child("No lines differ (the file is empty, or only its mode changed).").into_any_element(),
            Some(d) => {
                let rows = d.clone();
                // A file that is all new, or all gone, has one side to
                // show, and gets the whole width for it.
                let lone = |pick: fn(&git::Row) -> bool| rows.rows.iter().all(pick);
                let (all_new, all_gone) = (lone(|r| matches!(r, git::Row::Changed { left: None, .. })), lone(|r| matches!(r, git::Row::Changed { right: None, .. })));
                let (border, band, mono) = (theme.border, theme.muted.opacity(0.5), mono.clone());
                let lines = gpui::list(c.list.clone(), move |ix, _, _| {
                    let Some(row) = rows.rows.get(ix) else { return div().into_any_element() };
                    // One side of a row: the number on its ground, a mark, the words.
                    let side = |cell: Option<(&u32, &str)>, mark: &'static str, ground: Option<(Hsla, Hsla)>| {
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .when_some(ground, |el, (bg, _)| el.bg(bg))
                            .when(cell.is_none(), |el| el.bg(band.opacity(0.35)))
                            .child(div().w(px(46.)).h_full().flex_shrink_0().pr(px(8.)).text_right().text_color(quiet).when_some(ground, |el, (_, no)| el.bg(no)).children(cell.map(|(n, _)| n.to_string())))
                            .child(div().w(px(18.)).flex_shrink_0().text_center().text_color(quiet).child(if cell.is_some() { mark } else { "" }))
                            .child(div().flex_1().min_w_0().pr(px(8.)).text_color(ink).children(cell.map(|(_, t)| if t.is_empty() { " ".to_string() } else { t.to_string() })))
                    };
                    let line = h_flex().w_full().items_stretch().font_family(mono.clone()).text_size(px(12.)).line_height(px(19.));
                    match row {
                        git::Row::Hunk(head) => h_flex().w_full().h(px(28.)).px(px(14.)).items_center().bg(band).border_t_1().border_b_1().border_color(border).font_family(mono.clone()).text_size(px(11.5)).text_color(quiet).child(div().truncate().child(head.clone())).into_any_element(),
                        git::Row::Same { old, new, text } => line.child(side(Some((old, text)), "", None)).child(div().w(px(1.)).flex_shrink_0().bg(border)).child(side(Some((new, text)), "", None)).into_any_element(),
                        git::Row::Changed { right: Some((n, t)), .. } if all_new => line.child(side(Some((n, t.as_str())), "+", Some((add, add_no)))).into_any_element(),
                        git::Row::Changed { left: Some((n, t)), .. } if all_gone => line.child(side(Some((n, t.as_str())), "-", Some((del, del_no)))).into_any_element(),
                        git::Row::Changed { left, right } => line
                            .child(side(left.as_ref().map(|(n, t)| (n, t.as_str())), "-", left.as_ref().map(|_| (del, del_no))))
                            .child(div().w(px(1.)).flex_shrink_0().bg(border))
                            .child(side(right.as_ref().map(|(n, t)| (n, t.as_str())), "+", right.as_ref().map(|_| (add, add_no))))
                            .into_any_element(),
                    }
                })
                .size_full();
                v_flex()
                    .size_full()
                    .child(div().relative().flex_1().min_h_0().child(lines).vertical_scrollbar(&c.list))
                    .when(d.cut, |el| el.child(div().flex_shrink_0().px(px(14.)).py(px(8.)).border_t_1().border_color(theme.border).text_size(px(12.)).text_color(quiet).child("Only the beginning of this comparison is shown.")))
                    .into_any_element()
            }
        };
        let (open, reveal) = (picked.clone(), picked.clone());
        let counts = shown.as_ref().filter(|d| !d.binary).map(|d| (d.added, d.removed));
        let branch = self.branches().map(|b| b.label());
        div()
            .id("changes-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| this.close_changes(cx)))
            .child(
                v_flex()
                    .id("changes-sheet")
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(gpui::relative(0.96))
                    .h(gpui::relative(0.92))
                    .rounded(px(16.))
                    .overflow_hidden()
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .child(
                        h_flex()
                            .h(px(48.))
                            .flex_shrink_0()
                            .pl(px(18.))
                            .pr(px(12.))
                            .gap(px(10.))
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(Icon::default().path("icons/git-diff.svg").with_size(px(15.)).text_color(quiet).flex_shrink_0())
                            .child(div().flex_shrink_0().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child("Changes"))
                            .child(div().flex_shrink_0().text_size(px(12.)).text_color(quiet).child(format!("{} {}", crate::format::thousands(changed.len()), if changed.len() == 1 { "file" } else { "files" })))
                            .when_some(branch, |el, b| el.child(h_flex().min_w_0().gap(px(4.)).items_center().text_size(px(12.)).text_color(quiet).child("on").child(Icon::default().path("icons/git-branch.svg").with_size(px(12.)).flex_shrink_0()).child(div().min_w_0().truncate().text_color(ink).child(b))))
                            .child(div().flex_1())
                            .child(Button::new("changes-close").ghost().small().icon(IconName::Close).on_click(cx.listener(|this, _, _, cx| this.close_changes(cx)))),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .items_stretch()
                            .child(div().relative().w(CHANGES_LIST_W).flex_shrink_0().py(px(6.)).border_r_1().border_color(theme.border).bg(theme.sidebar).child(files).vertical_scrollbar(&c.files))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        h_flex()
                                            .h(px(38.))
                                            .flex_shrink_0()
                                            .px(px(14.))
                                            .gap(px(10.))
                                            .items_center()
                                            .border_b_1()
                                            .border_color(theme.border)
                                            .child(div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(12.)).child(rel))
                                            .when_some(counts, |el, (a, r)| {
                                                el.child(div().flex_shrink_0().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(git_color(git::State::Added, dark)).child(format!("+{a}")))
                                                    .child(div().flex_shrink_0().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(git_color(git::State::Deleted, dark)).child(format!("\u{2212}{r}")))
                                            })
                                            .when_some(open, |el, p| el.child(pill_button("changes-open", "Open", &theme, move |_, _, _| crate::sys::open_path(&p))))
                                            .when_some(reveal, |el, p| el.child(pill_button("changes-reveal", crate::sys::REVEAL_LABEL, &theme, move |_, _, _| crate::sys::reveal_path(&p)))),
                                    )
                                    .child(div().flex_1().min_h_0().child(body)),
                            ),
                    ),
            )
            .into_any_element()
    }

    // -- the file shown -------------------------------------------------------

    pub(crate) fn render_file_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(v) = &self.file_view else { return div().into_any_element() };
        let name = v.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let folder = v.rel.strip_suffix(&name).unwrap_or("").trim_end_matches(['/', '\\']).to_string();
        let (open, reveal, mention) = (v.path.clone(), v.path.clone(), v.path.clone());
        let body = match &v.body {
            FileBody::Markdown(text) => div().text_size(px(14.)).line_height(relative(1.6)).child(crate::transcript::md_view(format!("file-{}", v.rel), text.clone(), cx)).into_any_element(),
            FileBody::Code(md, dropped) => v_flex()
                .gap(px(10.))
                .text_size(px(12.5))
                .child(crate::transcript::md_view(format!("file-{}", v.rel), md.clone(), cx))
                .when(*dropped > 0, |d| d.child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(format!("Only the first {PREVIEW_LINES} lines are shown. Open the file for the rest."))))
                .into_any_element(),
            FileBody::None(why) => v_flex().py(px(60.)).gap(px(10.)).items_center().text_color(theme.muted_foreground).child(Icon::default().path(crate::assets::file_icon_path(&name)).with_size(px(36.))).child(div().text_size(px(13.)).child(why.clone())).into_any_element(),
        };
        div()
            .id("file-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| this.close_file_view(cx)))
            .child(
                v_flex()
                    .id("file-sheet")
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(px(880.))
                    .max_w(gpui::relative(0.94))
                    .h(gpui::relative(0.88))
                    .rounded(px(16.))
                    .overflow_hidden()
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .child(
                        h_flex()
                            .h(px(50.))
                            .flex_shrink_0()
                            .pl(px(18.))
                            .pr(px(12.))
                            .gap(px(10.))
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(Icon::default().path(crate::assets::file_icon_path(&name)).with_size(px(16.)).text_color(theme.muted_foreground).flex_shrink_0())
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(8.))
                                    .items_baseline()
                                    .child(div().flex_shrink_0().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                                    .child(div().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(11.)).text_color(theme.muted_foreground).child(folder))
                                    .child(div().flex_shrink_0().text_size(px(11.)).text_color(theme.muted_foreground).child(human_size(v.size))),
                            )
                            .child(pill_button("file-mention", "Add to message", &theme, {
                                let this = cx.entity().downgrade();
                                move |_, window, cx| {
                                    let _ = this.update(cx, |this, cx| {
                                        this.file_view = None;
                                        this.file_mention(&mention, window, cx);
                                        cx.notify();
                                    });
                                }
                            }))
                            .child(pill_button("file-open", "Open", &theme, move |_, _, _| crate::sys::open_path(&open)))
                            .child(pill_button("file-reveal", crate::sys::REVEAL_LABEL, &theme, move |_, _, _| crate::sys::reveal_path(&reveal)))
                            .child(Button::new("file-close").ghost().small().icon(Icon::new(IconName::Close)).tooltip("Close (esc)").on_click(cx.listener(|this, _, _, cx| this.close_file_view(cx)))),
                    )
                    .child(
                        v_flex()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(v_flex().id("file-body").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.file_view_scroll).px(px(22.)).py(px(18.)).child(body))
                            .vertical_scrollbar(&self.file_view_scroll),
                    ),
            )
            .into_any_element()
    }

    // -- the outline ----------------------------------------------------------

    /// An entry was chosen, by a click or by the wheel: it is the one
    /// marked, and the conversation moves to its round, which ends at
    /// the top of the view. The move is drawn (`glide`), a step each
    /// tick until it is there; choosing another entry meanwhile takes
    /// the move over, and a wheel in the conversation ends it.
    pub(crate) fn outline_go(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.outline_pick = Some(ix);
        self.outline_glide += 1;
        let turn = self.outline_glide;
        cx.notify();
        let Some(list) = self.detail.as_ref().map(|d| d.list.clone()) else { return };
        // At the end and following it, the last round is already where
        // it can be.
        if list.is_following_tail() && ix + 1 >= list.item_count() {
            self.outline_gliding = false;
            return;
        }
        self.outline_gliding = true;
        cx.spawn(async move |this, cx| {
            let mut glide = Glide::default();
            let (began, mut last) = (std::time::Instant::now(), std::time::Instant::now());
            loop {
                cx.background_executor().timer(GLIDE_TICK).await;
                let over = this
                    .update(cx, |this, cx| {
                        if this.outline_glide != turn {
                            return true;
                        }
                        let now = std::time::Instant::now();
                        let done = glide.step(&list, ix, (now - last).as_secs_f32()) || began.elapsed() > GLIDE_MOST;
                        last = now;
                        if done {
                            if !list.is_following_tail() {
                                list.scroll_to(ListOffset { item_ix: ix, offset_in_item: px(0.) });
                            }
                            this.outline_gliding = false;
                        }
                        cx.notify();
                        done
                    })
                    .unwrap_or(true);
                if over {
                    break;
                }
            }
        })
        .detach();
    }

    /// The wheel over the outline moves the mark, an entry at a time,
    /// and the conversation follows it: a notch of a mouse wheel is one
    /// entry, a trackpad one for every `WHEEL_STEP` of travel, and never
    /// faster than one each `WHEEL_PACE`, so a flick's momentum walks
    /// the entries and does not fly through them.
    pub(crate) fn outline_wheel(&mut self, e: &ScrollWheelEvent, fresh: bool, cx: &mut Context<Self>) {
        let Some(n) = self.detail.as_ref().map(|d| d.session.rounds.len()).filter(|n| *n > 0) else { return };
        if fresh || e.touch_phase == TouchPhase::Started {
            self.outline_wheel_acc = 0.;
        }
        let step = match e.delta {
            ScrollDelta::Lines(l) => -l.y.signum() as i32 * i32::from(l.y != 0.),
            ScrollDelta::Pixels(p) => {
                self.outline_wheel_acc = (self.outline_wheel_acc - f32::from(p.y)).clamp(-WHEEL_STEP, WHEEL_STEP);
                (self.outline_wheel_acc / WHEEL_STEP) as i32
            }
        };
        if step == 0 || self.outline_stepped.elapsed() < WHEEL_PACE {
            return;
        }
        self.outline_wheel_acc = 0.;
        self.outline_stepped = std::time::Instant::now();
        let at = self.outline_pick.or(self.outline_at.as_ref().map(|(_, at)| *at)).unwrap_or(n - 1);
        let to = (at as i64 + step as i64).clamp(0, n as i64 - 1) as usize;
        if to != at {
            self.outline_go(to, cx);
        }
    }

    pub(crate) fn render_outline_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(d) = self.detail.as_mut() else { return div().into_any_element() };
        let entries = d.outline.get_or_insert_with(|| Rc::new(outline::of(&d.session))).clone();
        let key = d.key.clone();
        // The round in view is the one at the top, or the last once the
        // conversation is at its end, where a short last round never
        // reaches the top. An entry just clicked is the one marked for as
        // long as the view is where the click put it.
        let top = d.list.logical_scroll_top().item_ix;
        let at_end = d.list.is_scrolled_to_end() != Some(false);
        if !self.outline_gliding && self.outline_pick.is_some_and(|ix| !(top == ix || (at_end && top <= ix))) {
            self.outline_pick = None;
        }
        let current = self.outline_pick.unwrap_or(if at_end { entries.len().saturating_sub(1) } else { top }).min(entries.len().saturating_sub(1));

        let rail = theme.muted_foreground.opacity(0.28);
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut current_child = 0;
        let mut day = String::new();
        for (n, e) in entries.iter().enumerate() {
            let this_day = day_label(&e.ts);
            let first = this_day != day;
            if first {
                day = this_day.clone();
                if !this_day.is_empty() {
                    rows.push(div().flex_shrink_0().px(px(10.)).pt(px(if rows.is_empty() { 4. } else { 14. })).pb(px(4.)).text_size(px(10.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground.opacity(0.8)).child(this_day.to_uppercase()).into_any_element());
                }
            }
            let last = entries.get(n + 1).is_none_or(|next| day_label(&next.ts) != day);
            let is_current = e.round == current;
            if is_current {
                current_child = rows.len();
            }
            let quiet = matches!(e.kind, Kind::Command | Kind::Compact);
            let dot = div().size(px(7.)).flex_shrink_0().rounded_full().map(|d| match (is_current, e.kind) {
                (true, _) => d.bg(theme.primary),
                (_, Kind::Compact | Kind::Queued) => d.border_1().border_color(theme.muted_foreground.opacity(0.6)),
                _ => d.bg(theme.muted_foreground.opacity(0.45)),
            });
            let gist = match e.kind {
                Kind::Queued => "Queued".to_string(),
                _ => e.gist.clone(),
            };
            let round = e.round;
            rows.push(
                h_flex()
                    .id(("outline-row", round))
                    .flex_shrink_0()
                    .w_full()
                    .items_stretch()
                    .gap(px(9.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(is_current, |d| d.bg(theme.primary.opacity(if theme.mode.is_dark() { 0.16 } else { 0.10 })))
                    .when(!is_current, |d| d.hover(|s| s.bg(theme.muted.opacity(0.7))))
                    .on_click(cx.listener(move |this, _, _, cx| this.outline_go(round, cx)))
                    // The rail: a line down the entries of one day, a dot
                    // on it for each.
                    .child(
                        div()
                            .relative()
                            .w(px(7.))
                            .flex_shrink_0()
                            .pt(px(11.))
                            .when(!first, |d| d.child(div().absolute().left(px(3.)).top_0().w(px(1.)).h(px(11.)).bg(rail)))
                            .when(!last, |d| d.child(div().absolute().left(px(3.)).top(px(18.)).bottom_0().w(px(1.)).bg(rail)))
                            .child(dot),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .py(px(6.))
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .line_height(relative(1.35))
                                    .line_clamp(2)
                                    .text_ellipsis()
                                    .overflow_hidden()
                                    .when(is_current, |d| d.font_weight(FontWeight::MEDIUM))
                                    .text_color(if quiet { theme.muted_foreground } else { theme.foreground })
                                    .when(quiet, |d| d.font_family(theme.mono_font_family.clone()).text_size(px(11.5)))
                                    .child(e.title.clone()),
                            )
                            .child(
                                h_flex()
                                    .gap(px(5.))
                                    .text_size(px(11.))
                                    .text_color(theme.muted_foreground)
                                    .child(div().flex_shrink_0().opacity(0.8).child(clock(&e.ts)))
                                    .when(!gist.is_empty(), |d| d.child(div().flex_1().min_w_0().truncate().child(gist))),
                            ),
                    )
                    .into_any_element(),
            );
        }
        // The entry in view is kept in sight as the conversation moves.
        let at = Some((key, current));
        if self.outline_at != at && !rows.is_empty() {
            self.outline_at = at;
            self.outline_scroll.scroll_to_item(current_child);
        }
        let body = if rows.is_empty() {
            div().p(px(14.)).text_size(px(12.)).text_color(theme.muted_foreground).child("Nothing said yet.").into_any_element()
        } else {
            v_flex()
                .relative()
                .flex_1()
                .min_h_0()
                .child(v_flex().id("outline-scroll").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.outline_scroll).px(px(6.)).py(px(6.)).children(rows))
                .vertical_scrollbar(&self.outline_scroll)
                .into_any_element()
        };
        let el = v_flex().w(self.panel_w).h_full().flex_shrink_0().border_r_1().border_color(theme.border).bg(theme.sidebar).child(panel_head("Outline", None, Some(div().text_color(theme.muted_foreground.opacity(0.8)).child(plural(entries.len(), "prompt", "prompts")).into_any_element()), &theme)).child(body);
        self.panel_in(false, el)
    }

    /// `EMAKI_GO` steps for the two panels; whether `step` was one.
    pub(crate) fn panel_probe(&mut self, step: &str, cx: &mut Context<Self>) -> bool {
        let root = self.files_root();
        let under = |rel: &str| root.as_ref().map(|r| r.join(rel));
        match step {
            "files" => self.toggle_panel(true, cx),
            // The branch button's click, and words typed in its field.
            "branches" => self.branch_probe = Some(String::new()),
            // The comparison, on the first changed file or on that one.
            "changes" => self.open_changes(None, cx),
            t if t.starts_with("changes:") => self.open_changes(under(&t["changes:".len()..]), cx),
            t if t.starts_with("branchq:") => self.branch_probe = Some(t["branchq:".len()..].to_string()),
            "outline" => self.toggle_panel(false, cx),
            // The panel's edge dragged to that width and let go, and the
            // double click on it.
            t if t.starts_with("panelw:") => {
                if let (Ok(w), Some(which)) = (t["panelw:".len()..].parse::<f32>(), self.panel_on()) {
                    self.panel_drag = Some(which);
                    let at = point(self.panel_left() + px(w), px(300.));
                    self.panel_drag_to(&MouseMoveEvent { position: at, pressed_button: Some(MouseButton::Left), modifiers: Modifiers::default() }, cx);
                    self.panel_drag_end();
                }
            }
            "panelfit" => self.panel_fit_wanted = true,
            t if t.starts_with("tree:") => {
                if let Some(p) = under(&t["tree:".len()..]) {
                    self.tree_toggle(&p, cx);
                }
            }
            t if t.starts_with("file:") => self.file_probe = under(&t["file:".len()..]),
            t if t.starts_with("filemenu:") => {
                if let Some(p) = under(&t["filemenu:".len()..]) {
                    let node = Node { name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), dir: p.is_dir(), path: p };
                    self.file_menu(Some(&node), point(self.pane_w, px(200.)), cx);
                }
            }
            t if t.starts_with("outline:") => {
                if let Ok(n) = t["outline:".len()..].parse::<usize>() {
                    self.outline_go(n.saturating_sub(1), cx);
                }
            }
            _ => return false,
        }
        true
    }
}
