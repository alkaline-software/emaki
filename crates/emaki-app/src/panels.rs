//! Beside a conversation: the files of its folder, and its outline.
//!
//! Two panels, one at a time, at the conversation's left: the pane's
//! whole height under the top strip, chosen with a two-segment control at
//! the strip's left end. The files are the session's folder as a tree, read from
//! disk a folder at a time as it is opened; a click on a file shows it
//! at the conversation's right, a right click offers what a file manager
//! would. The
//! outline is `emaki_core::outline`: a line on what each round asked and
//! a line on what came of it, the round in view marked, a click going
//! there.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use chrono::Datelike as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use emaki_core::git;
use emaki_core::outline::{self, Kind};

use crate::format::{clock, plural};
use crate::workbench::{choice_pill, float_shadow, Press, swallow_click, MenuDo, Notice, Page, Workbench};

/// How wide a panel is; both are, so one takes the other's place without
/// moving the conversation.
pub const PANEL_W: Pixels = px(264.);
/// The panel's edge is dragged between these; a double click on it asks
/// for no more than `PANEL_FIT_MAX`, and dragged narrower than
/// `PANEL_FOLD_AT` the panel is put away.
pub(crate) const PANEL_MIN: Pixels = px(200.);
const PANEL_MAX: Pixels = px(520.);
const PANEL_FIT_MAX: Pixels = px(380.);
const PANEL_FOLD_AT: Pixels = px(130.);
/// The outline's width at a double click: its lines are sentences, cut
/// at any width, so this is a width they read well at.
const OUTLINE_FIT: Pixels = px(300.);
/// How far past the outline's view, above and below, labels are asked
/// for, and how long its scroller rests before they are.
const LABEL_AHEAD: Pixels = px(320.);
const LABEL_REST: Duration = Duration::from_millis(300);
/// How long a label that has just landed takes to fade in.
const LABEL_FADE: Duration = Duration::from_millis(320);
/// The room over the pinned day's words.
const PIN_LEAD: Pixels = px(10.);
/// The least the conversation keeps beside the panels at its sides: the
/// width at which the composer's row of pills and its send button still
/// fit inside the card.
pub(crate) const CONVERSATION_MIN: Pixels = px(480.);
/// How wide the strip at the panel's edge that takes the drag is.
const PANEL_GRIP: Pixels = px(8.);
/// A panel's head is as tall as the folder's band beside it, so the two
/// read as one band across the pane.
const HEAD_H: Pixels = px(31.);
const TREE_ROW_H: Pixels = px(24.);
/// How long a row of the tree takes to come or go.
const TREE_ANIM: Duration = Duration::from_millis(200);
/// What the row a new name is typed in goes by among the rows' paths.
const TREE_EDIT_KEY: &str = "\0name";
/// The list of branches shows this many rows of this height and scrolls
/// for the rest.
const BRANCH_ROWS: usize = 10;
const BRANCH_ROW_H: Pixels = px(30.);
/// The column a branch's tag stands in, and the one its time does.
const BRANCH_TAG_W: Pixels = px(72.);
const BRANCH_WHEN_W: Pixels = px(100.);
/// The comparison: how wide its list of files is, and how tall a row of
/// that list.
pub(crate) const CHANGES_LIST_W: Pixels = px(300.);
const CHANGE_ROW_H: Pixels = px(26.);
/// How long the comparison takes to go.
const CHANGES_OUT: Duration = Duration::from_millis(140);
/// The most entries one folder lists before "N more".
const DIR_MAX: usize = 400;
/// How often an open tree is read from disk again, in seconds.
const TREE_SECS: f64 = 2.0;
/// The most of a file a preview reads, and the most lines of code it sets.
const PREVIEW_BYTES: usize = 1_500_000;
const PREVIEW_LINES: usize = 20_000;

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
    /// Where a new file or folder is being named: its depth, and
    /// whether it is a folder.
    Edit(usize, bool),
}

/// A row of the tree as it was last drawn: its path, what it showed
/// (none for the row a name is typed in), its depth, and whether it
/// was an open folder.
type Seen = (PathBuf, Option<Node>, usize, bool);

/// A row that has just gone, drawn a moment more where it stood: under
/// the row `after`, or at the head.
struct TreeGhost {
    after: Option<PathBuf>,
    row: Seen,
    at: std::time::Instant,
}

/// The folders opened in the files panel and what each was last seen to
/// hold. Kept by absolute path, so two sessions of one folder share it.
#[derive(Default)]
pub struct Tree {
    open: HashSet<PathBuf>,
    /// The sessions' folders whose head row was closed. One is open
    /// until it is, so the tree shows at first.
    shut: HashSet<PathBuf>,
    /// The rows as last drawn and the folder they were of, the rows
    /// that have just come, and the ones that have just gone
    /// (`Tree::seen_now`).
    seen: Option<(PathBuf, Vec<Seen>)>,
    born: HashMap<PathBuf, std::time::Instant>,
    gone: Vec<TreeGhost>,
    kids: HashMap<PathBuf, Listing>,
    /// The file last shown, which its row marks.
    picked: Option<PathBuf>,
    /// The rows chosen together, by ⌘A or a rectangle dragged over them.
    marked: Vec<PathBuf>,
    read_at: f64,
    /// What git says of the folder it was last asked about, and whether
    /// it is being asked now.
    git: Option<(PathBuf, Rc<git::Status>)>,
    /// Every file with a change, by path, as the status last had them.
    changed: Rc<Vec<(PathBuf, git::State)>>,
    /// The folder's branches, none when it is in no repository.
    branches: Option<(PathBuf, Rc<git::Branches>)>,
    /// Why git says nothing of a folder that is a checkout
    /// (`git::trouble`), and the folder.
    git_trouble: Option<(PathBuf, git::Trouble)>,
    git_reading: bool,
}

/// The comparison showing over the window: which file of the changed
/// ones, and its lines against the last commit.
pub struct Changes {
    pub(crate) root: PathBuf,
    pub(crate) picked: Option<PathBuf>,
    /// The lines of `picked`, once read; in the history, of the commit's
    /// file, under the name `history_key` gives the two.
    pub(crate) diff: Option<(PathBuf, Rc<git::Diff>)>,
    /// A compared picture's two sides as gpui draws them, made when
    /// `diff` is: made at each draw they were hashed and copied each time.
    pub(crate) pictures: [Option<Arc<gpui::Image>>; 2],
    pub(crate) list: ListState,
    pub(crate) files: UniformListScrollHandle,
    pub(crate) reading: bool,
    /// The files chosen together, `picked` among them: what a menu or
    /// Discard is asked of. Chosen as the tree's rows are.
    pub(crate) marked: Vec<PathBuf>,
    /// Where ⇧ and a click reaches from.
    anchor: Option<PathBuf>,
    /// A rectangle being dragged over the files: where the press was
    /// and where the pointer is, and whether it has moved yet.
    band: Option<(Point<Pixels>, Point<Pixels>)>,
    band_live: bool,
    band_at: Option<std::time::Instant>,
    /// The files a commit leaves out: every other one changed goes in.
    pub(crate) left_out: HashSet<PathBuf>,
    /// The history shows, not the changes: the branch's commits, the
    /// one picked by its hash, the files it changed (kept with the hash
    /// they are of), and the one of those whose lines show.
    pub(crate) history: bool,
    pub(crate) commits: Option<Rc<Vec<git::Commit>>>,
    pub(crate) commit: Option<String>,
    pub(crate) commit_files: Option<(String, Rc<Vec<(String, git::State)>>)>,
    pub(crate) commit_file: Option<String>,
    /// The repository's top folder, which a commit's paths are from.
    pub(crate) top: Option<PathBuf>,
    pub(crate) commits_scroll: ScrollHandle,
    pub(crate) cfiles_scroll: ScrollHandle,
    /// Since when the sheet has been going: it is drawn that moment
    /// more, fading, and takes no click.
    pub(crate) closing: Option<std::time::Instant>,
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

pub(crate) fn git_color(state: git::State, dark: bool) -> Hsla {
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
        if !self.shut.contains(root) {
            self.walk(root, 0, &mut out);
        }
        out
    }

    /// The rows about to be drawn against how they were last: one that
    /// was not there has just come and grows in, one that is no longer
    /// has just gone and is drawn a moment more, shrinking. That is a
    /// folder opened or closed, a file made or removed, on disk or
    /// here. A folder drawn for the first time plays nothing. Whether
    /// anything came or went.
    fn seen_now(&mut self, root: &Path, rows: &[Row]) -> bool {
        let edit = PathBuf::from(TREE_EDIT_KEY);
        let now: Vec<Seen> = rows
            .iter()
            .filter_map(|row| match row {
                Row::Node(node, depth, open) => Some((node.path.clone(), Some(node.clone()), *depth, *open)),
                Row::Edit(depth, dir) => Some((edit.clone(), None, *depth, *dir)),
                Row::More(..) => None,
            })
            .collect();
        let at = std::time::Instant::now();
        let mut changed = false;
        match self.seen.take() {
            Some((of, was)) if of == root => {
                let here: HashSet<&PathBuf> = now.iter().map(|row| &row.0).collect();
                let there: HashSet<&PathBuf> = was.iter().map(|row| &row.0).collect();
                let born: Vec<PathBuf> = now.iter().filter(|row| !there.contains(&row.0)).map(|row| row.0.clone()).collect();
                // The name's row became the thing named: one takes the
                // other's place and neither moves.
                let named = there.contains(&edit) && !here.contains(&edit) && !born.is_empty();
                let mut after = None;
                for row in &was {
                    if here.contains(&row.0) {
                        after = Some(row.0.clone());
                    } else if !(named && row.0 == edit) {
                        self.born.remove(&row.0);
                        self.gone.push(TreeGhost { after: after.clone(), row: row.clone(), at });
                        changed = true;
                    }
                }
                for path in born.into_iter().filter(|_| !named) {
                    self.gone.retain(|ghost| ghost.row.0 != path);
                    self.born.insert(path, at);
                    changed = true;
                }
            }
            _ => {
                self.born.clear();
                self.gone.clear();
            }
        }
        self.seen = Some((root.to_path_buf(), now));
        self.born.retain(|_, at| at.elapsed() < TREE_ANIM);
        self.gone.retain(|ghost| ghost.at.elapsed() < TREE_ANIM);
        changed
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

/// How many of a PDF's pages are drawn, how wide in pixels, and how
/// large a PDF is read at all.
const PDF_PAGES: usize = 40;
/// A page is drawn as wide in pixels as the pane shows it, between these.
/// How long the pages drawn before stay under the new ones.
const PDF_UNDER: Duration = Duration::from_millis(1500);
const PDF_W_LEAST: f32 = 600.;
const PDF_W_MOST: f32 = 2400.;
const PDF_BYTES: u64 = 120_000_000;
/// How much of a table is shown.
const TABLE_ROWS: usize = 500;
const TABLE_COLS: usize = 40;
/// The largest Office file the pane reads: it is read whole, at once.
const OFFICE_BYTES: usize = 40_000_000;
const TABLE_CELL: usize = 48;
/// How long the file's pane takes to come and to go.
pub(crate) const FILE_ANIM: Duration = Duration::from_millis(200);
/// How long files dragged in are held over a closed folder before it
/// opens.
const TREE_SPRING: Duration = Duration::from_millis(650);
/// How wide the pane is to begin with, and the least it is dragged to.
pub(crate) const FILE_W: Pixels = px(460.);
pub(crate) const FILE_MIN: Pixels = px(280.);
/// What the file pane's head keeps for the file's name before it folds
/// Open and Reveal into one More button, and Save into its icon.
const HEAD_NAME_ROOM: f32 = 110.;

/// A panel beside the conversation that gives its room up when the
/// window has too little: the file shown, the files or the outline at
/// the left, and the terminal at the right. The sidebar is not one: it
/// leaves the row first, for an overlay. Which goes is by their rank
/// (`Workbench::side_rank`), the one opened or used last at its top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    File,
    Left,
    Term,
}

impl Side {
    /// The order before any is opened or used: the file, which was asked
    /// for to be read; the panel the file is found in; the terminal,
    /// which goes on running away and is one key from coming back.
    pub(crate) const RANK: [Side; 3] = [Side::File, Side::Left, Side::Term];

    /// The least it takes of the row.
    pub(crate) fn least(self) -> Pixels {
        match self {
            Side::File => FILE_MIN,
            Side::Left => PANEL_MIN,
            Side::Term => crate::term_panel::TERM_MIN,
        }
    }
}

/// One page of a PDF: its picture and that picture's size in pixels,
/// the page's own size in its units, and which of the document's glyphs
/// are on it.
#[derive(Clone)]
pub(crate) struct PdfPage {
    image: std::sync::Arc<gpui::Image>,
    /// The same page drawn small, for the list of pages: a large
    /// picture drawn small is jagged.
    thumb: std::sync::Arc<gpui::Image>,
    px: (u32, u32),
    unit: (f32, f32),
    glyphs: std::ops::Range<usize>,
}

/// A glyph of a PDF's text, where the page draws it, in the page's
/// units: the line it stands on and how far along it reaches. This is
/// what a selection is made of, since the page shown is a picture.
#[derive(Clone)]
pub(crate) struct PdfGlyph {
    x0: f32,
    x1: f32,
    base: f32,
    em: f32,
    text: String,
    /// What stands between it and the glyph before: nothing, a space,
    /// or a line break.
    gap: u8,
}

/// A PDF as the pane shows it: the pages drawn, how many the file has,
/// and the text in the order the file draws it.
pub(crate) struct PdfDoc {
    /// How wide in pixels its pages were asked for (`pdf_fit`).
    drawn_w: f32,
    pages: Vec<PdfPage>,
    total: usize,
    glyphs: Vec<PdfGlyph>,
    /// The file's own table of contents: each heading, the page it
    /// leads to when that could be told, and how deep it is.
    contents: Vec<(String, Option<usize>, usize)>,
}

impl PdfDoc {
    /// The words from one glyph to another, both taken in.
    fn text(&self, from: usize, to: usize) -> String {
        let mut out = String::new();
        for (ix, g) in self.glyphs.iter().enumerate().take(to + 1).skip(from) {
            if ix > from {
                match g.gap {
                    1 => out.push(' '),
                    2 => out.push('\n'),
                    _ => {}
                }
            }
            out.push_str(&g.text);
        }
        out
    }

    /// The glyph at a point of a page, or the nearest on that page: the
    /// nearest line first, then the nearest along it.
    fn glyph_at(&self, page: usize, x: f32, y: f32) -> Option<usize> {
        let range = self.pages.get(page)?.glyphs.clone();
        let mut best: Option<(f32, usize)> = None;
        for ix in range {
            let g = &self.glyphs[ix];
            let (top, bottom) = (g.base - g.em * 0.85, g.base + g.em * 0.3);
            let dy = if y < top { top - y } else if y > bottom { y - bottom } else { 0. };
            let dx = if x < g.x0 { g.x0 - x } else if x > g.x1 { x - g.x1 } else { 0. };
            let far = dy * 8. + dx;
            if best.is_none_or(|(d, _)| far < d) {
                best = Some((far, ix));
            }
        }
        best.map(|(_, ix)| ix)
    }

    /// The word a glyph is in, and the line it is on.
    fn word(&self, at: usize) -> (usize, usize) {
        let solid = |g: &PdfGlyph| !g.text.trim().is_empty();
        let (mut a, mut b) = (at, at);
        while a > 0 && self.glyphs[a].gap == 0 && solid(&self.glyphs[a - 1]) && solid(&self.glyphs[a]) {
            a -= 1;
        }
        while b + 1 < self.glyphs.len() && self.glyphs[b + 1].gap == 0 && solid(&self.glyphs[b + 1]) && solid(&self.glyphs[b]) {
            b += 1;
        }
        (a, b)
    }

    fn line(&self, at: usize) -> (usize, usize) {
        let (mut a, mut b) = (at, at);
        while a > 0 && self.glyphs[a].gap != 2 {
            a -= 1;
        }
        while b + 1 < self.glyphs.len() && self.glyphs[b + 1].gap != 2 {
            b += 1;
        }
        (a, b)
    }

    /// Every place the words are found, as glyphs from and to, whatever
    /// the case.
    fn find(&self, words: &str) -> Vec<(usize, usize)> {
        let needle = words.to_lowercase();
        if needle.trim().is_empty() {
            return Vec::new();
        }
        let mut text = String::new();
        // Where each glyph begins in the text searched.
        let mut starts = Vec::with_capacity(self.glyphs.len());
        for g in &self.glyphs {
            if g.gap != 0 {
                text.push(' ');
            }
            starts.push(text.len());
            text.push_str(&g.text.to_lowercase());
        }
        let glyph_of = |byte: usize| starts.partition_point(|s| *s <= byte).saturating_sub(1);
        text.match_indices(&needle).map(|(at, hit)| (glyph_of(at), glyph_of(at + hit.len() - 1))).collect()
    }
}

/// How wide the list beside a PDF's pages is, and a page in it.
const PDF_SIDE_W: f32 = 132.;
const PDF_THUMB_W: f32 = 240.;

/// A PDF's table of contents, read out of the file: the tree under
/// `/Outlines`, each heading with the page its destination names. A
/// destination is an array beginning with the page, or a name for one
/// kept in the catalog's `/Dests` or in the name tree under `/Names`,
/// which is how LaTeX writes them.
mod pdf_contents {
    use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Object};
    use hayro::hayro_syntax::Pdf;
    use std::collections::HashMap;

    type Pages = HashMap<(i32, i32), usize>;

    fn title(bytes: &[u8]) -> String {
        let text = if bytes.starts_with(&[0xfe, 0xff]) {
            String::from_utf16_lossy(&bytes[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<u16>>())
        } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            String::from_utf8_lossy(&bytes[3..]).to_string()
        } else {
            bytes.iter().map(|b| *b as char).collect()
        };
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn named<'a>(node: &Dict<'a>, key: &[u8], depth: usize) -> Option<Object<'a>> {
        if depth > 12 {
            return None;
        }
        if let Some(names) = node.get::<Array<'_>>("Names") {
            let mut pairs = names.iter::<Object<'_>>();
            while let (Some(name), Some(value)) = (pairs.next(), pairs.next()) {
                if matches!(&name, Object::String(s) if s.as_bytes() == key) {
                    return Some(value);
                }
            }
        }
        node.get::<Array<'_>>("Kids")?.iter::<Dict<'_>>().find_map(|kid| named(&kid, key, depth + 1))
    }

    fn page_of<'a>(dest: Object<'a>, root: &Dict<'a>, pages: &Pages, depth: usize) -> Option<usize> {
        if depth > 4 {
            return None;
        }
        match dest {
            Object::Array(a) => match a.raw_iter().next()? {
                MaybeRef::Ref(r) => pages.get(&(r.obj_number, r.gen_number)).copied(),
                MaybeRef::NotRef(Object::Number(n)) => Some(n.as_f64() as usize),
                _ => None,
            },
            Object::Dict(d) => page_of(d.get::<Object<'_>>("D")?, root, pages, depth + 1),
            Object::String(s) => page_of(named(&root.get::<Dict<'_>>("Names")?.get::<Dict<'_>>("Dests")?, s.as_bytes(), 0)?, root, pages, depth + 1),
            Object::Name(n) => page_of(root.get::<Dict<'_>>("Dests")?.get::<Object<'_>>(&*n)?, root, pages, depth + 1),
            _ => None,
        }
    }

    fn walk<'a>(mut item: Option<Dict<'a>>, depth: usize, root: &Dict<'a>, pages: &Pages, out: &mut Vec<(String, Option<usize>, usize)>) {
        while let Some(it) = item {
            if out.len() >= 2000 || depth > 8 {
                return;
            }
            let name = it.get::<hayro::hayro_syntax::object::String<'_>>("Title").map(|s| title(s.as_bytes())).unwrap_or_default();
            let dest = it.get::<Object<'_>>("Dest").or_else(|| it.get::<Dict<'_>>("A").and_then(|a| a.get::<Object<'_>>("D")));
            if !name.is_empty() {
                out.push((name, dest.and_then(|d| page_of(d, root, pages, 0)), depth));
            }
            walk(it.get::<Dict<'_>>("First"), depth + 1, root, pages, out);
            item = it.get::<Dict<'_>>("Next");
        }
    }

    pub fn read(pdf: &Pdf) -> Vec<(String, Option<usize>, usize)> {
        let xref = pdf.xref();
        let Some(root) = xref.get::<Dict<'_>>(xref.root_id()) else { return Vec::new() };
        let pages: Pages = pdf.pages().iter().enumerate().filter_map(|(ix, page)| page.raw().obj_id().map(|id| ((id.obj_number, id.gen_number), ix))).collect();
        let mut out = Vec::new();
        walk(root.get::<Dict<'_>>("Outlines").and_then(|o| o.get::<Dict<'_>>("First")), 0, &root, &pages, &mut out);
        out
    }
}

/// What a right click on a PDF's page asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PdfDo {
    Copy,
    SelectAll,
}

/// A file shown at the conversation's right.
#[derive(Clone)]
pub struct FileView {
    pub path: PathBuf,
    /// Its path under the session's folder, which is how it is named.
    pub rel: String,
    pub size: u64,
    mtime: Option<std::time::SystemTime>,
    body: FileBody,
    /// The file's text exactly as it is on disk, when the pane holds all
    /// of it and it is UTF-8: what the editor may change and write back.
    /// A file cut for the preview, or one with bytes that are no text,
    /// has none and is read only.
    source: Option<Rc<str>>,
    /// What each column of a table holds, where the file says: a
    /// Parquet file's. Shown over the columns in place of their letters.
    kinds: Vec<String>,
    /// How many rows the file has, where it says without being read
    /// through.
    rows_all: Option<usize>,
}

/// What was typed into a file's editor and not saved, and the time on
/// the file it was typed over, so a save still knows a file that has
/// changed since.
#[derive(Clone, PartialEq)]
pub(crate) struct Draft {
    text: String,
    base: Option<std::time::SystemTime>,
}

fn drafts_path() -> PathBuf {
    emaki_core::paths::state_dir().join("file_drafts.json")
}

/// Whether a row of `state/file_drafts.json` is another Emaki's that is
/// running now: its drafts are its own, still being typed in, and
/// neither taken up here nor written over. A row with no writer is from
/// an Emaki from before rows had one: another Emaki's while one runs
/// (`others_running`), which may be that one, else no one's.
fn row_is_live_other(row: &serde_json::Value, own: u32, alive: &dyn Fn(u32) -> bool, others_running: bool) -> bool {
    match row.get("pid").and_then(|p| p.as_u64()) {
        Some(pid) => u32::try_from(pid).is_ok_and(|pid| pid != own && alive(pid)),
        None => others_running,
    }
}

/// Whether another Emaki is running on this machine now. `pgrep` leaves
/// out its own ancestors, this Emaki among them. Off Unix none is known.
fn other_emaki_running() -> bool {
    if cfg!(unix) {
        std::process::Command::new("pgrep").args(["-x", "Emaki"]).output().is_ok_and(|o| o.status.success())
    } else {
        false
    }
}

/// The drafts this launch takes up: those of an Emaki that is no longer
/// running, which ended with no moment to ask (`keep_drafts`). Another
/// Emaki running now beside this one (the installed app beside a build)
/// writes to the same file, and its drafts are not left over: taken up,
/// a file saved there later showed here as not saved, with the older
/// words.
fn drafts_left(rows: &[serde_json::Value], own: u32, alive: &dyn Fn(u32) -> bool, others_running: bool) -> HashMap<PathBuf, Draft> {
    rows.iter()
        .filter(|row| !row_is_live_other(row, own, alive, others_running))
        .filter_map(|row| {
            let path = PathBuf::from(row.get("path")?.as_str()?);
            let text = row.get("text")?.as_str()?.to_string();
            let base = row.get("secs").and_then(|s| s.as_u64()).map(|secs| std::time::UNIX_EPOCH + Duration::new(secs, row.get("nanos").and_then(|n| n.as_u64()).unwrap_or(0) as u32));
            // One the file already holds is nothing to save: the app was
            // quit between the save and the next writing of this list.
            if std::fs::read_to_string(&path).is_ok_and(|on_disk| on_disk == text) {
                return None;
            }
            Some((path, Draft { text, base }))
        })
        .collect()
}

/// A row of the drafts file as who wrote it and the file it is for.
fn row_key(row: &serde_json::Value) -> (Option<u64>, Option<String>) {
    (row.get("pid").and_then(|p| p.as_u64()), row.get("path").and_then(|p| p.as_str()).map(str::to_string))
}

/// The rows this launch took up (`load_drafts`): written again as this
/// Emaki's, so they leave the file under their old writer.
static DRAFTS_TAKEN: std::sync::OnceLock<Vec<(Option<u64>, Option<String>)>> = std::sync::OnceLock::new();

/// The drafts kept from the last run (`keep_drafts`).
pub(crate) fn load_drafts() -> HashMap<PathBuf, Draft> {
    let Some(serde_json::Value::Array(rows)) = emaki_core::paths::read_json(&drafts_path()) else { return HashMap::new() };
    let (own, alive, others) = (std::process::id(), &emaki_core::peer::process_alive, other_emaki_running());
    let _ = DRAFTS_TAKEN.set(rows.iter().filter(|row| !row_is_live_other(row, own, alive, others)).map(row_key).collect());
    drafts_left(&rows, own, alive, others)
}

/// What `state/file_drafts.json` is written as: this Emaki's drafts, each
/// with its writer, and every other row but those this launch took up.
/// Another Emaki's are its own to change, and one that ended left its
/// own for its next launch: written over here, a build relaunched
/// beside the installed app lost what it had kept.
fn drafts_rows(all: &HashMap<PathBuf, Draft>, on_disk: &[serde_json::Value], own: u32, taken: &[(Option<u64>, Option<String>)]) -> Vec<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = on_disk.iter().filter(|row| row_key(row).0 != Some(own as u64) && !taken.contains(&row_key(row))).cloned().collect();
    rows.extend(all.iter().map(|(path, d)| {
        let since = d.base.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        serde_json::json!({ "path": path.to_string_lossy(), "text": d.text, "secs": since.map(|s| s.as_secs()), "nanos": since.map(|s| s.subsec_nanos()), "pid": own })
    }));
    rows
}

/// What was being done when a file with changes not saved stood in the
/// way, done once the question is answered.
#[derive(Clone, PartialEq)]
pub(crate) enum FileThen {
    /// The file's pane was being closed.
    Close,
    /// Another file was being shown in its place.
    Open(PathBuf),
    CloseWindow,
    Quit,
}

/// The question asked of a file with changes not saved: save them,
/// drop them, or do not do what was asked after all.
#[derive(Clone)]
pub(crate) struct FileAsk {
    path: PathBuf,
    then: FileThen,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum FileAnswer {
    Save,
    Discard,
    Cancel,
}

/// The dot on a file with changes not saved: orange, in both
/// appearances.
fn unsaved_dot() -> Hsla {
    gpui::rgb(0xF2802B).into()
}

/// How many editors of files not showing are kept with nothing to save
/// in them, for their undo histories.
const PARKED_CLEAN: usize = 8;

/// What `sync_file_editor` makes an editor from.
struct EditorWant {
    key: String,
    path: PathBuf,
    text: Rc<str>,
    lang: &'static str,
    saved: Option<Rc<str>>,
    mtime: Option<std::time::SystemTime>,
}

/// A table's cell: whose table, which row and column of what the pane
/// shows, and whether it is being written in. With `to`, the cells
/// chosen are the block from this one to that: this is where a drag or
/// a ⇧ click began, and stays the one that is written in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TableAt {
    path: PathBuf,
    row: usize,
    col: usize,
    to: Option<(usize, usize)>,
    editing: bool,
}

/// A table's grid: a row's height, the head's, and the width of the
/// column of row numbers.
const GRID_ROW: f32 = 26.;
const GRID_HEAD: f32 = 24.;
const GRID_NUM: f32 = 46.;

/// A column's width in the grid: as wide as its longest cell, up to a
/// point (`TABLE_CELL`).
fn table_col_w(chars: usize) -> f32 {
    chars as f32 * 7.8 + 22.
}

/// A column's name as a spreadsheet has it: A to Z, then AA.
fn column_name(mut n: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The editor in the file's pane, and what it needs to save.
pub(crate) struct FileEditor {
    /// A name for the file and what it held when the editor was made.
    key: String,
    path: PathBuf,
    /// The language, as `render_md::lang_for_path` names it.
    lang: &'static str,
    pub(crate) state: Entity<gpui_component::input::EditorState>,
    /// What the file held when it was read or last saved here, and when
    /// it was written; None when the file is read only.
    saved: Option<Rc<str>>,
    mtime: Option<std::time::SystemTime>,
    /// What git has of the file (`git::base_text`), which the marks in
    /// the margin compare the editor's text with; none for a file git
    /// does not track. Whether the marks are to be worked out again,
    /// and whether they are being.
    base: Option<std::sync::Arc<str>>,
    marks_due: bool,
    marks_busy: bool,
    /// The places the text differs from git's copy, as last worked out:
    /// a mark's `id` is its place here.
    hunks: Vec<git::Hunk>,
}

/// One change in the file's editor, opened from its mark in the margin:
/// the lines git has there and the lines there now, where the click
/// was, and where the lines now are in the text, to put git's back.
#[derive(Clone)]
pub(crate) struct FilePeek {
    old: String,
    new: String,
    bytes: std::ops::Range<usize>,
    at: Point<Pixels>,
    can_revert: bool,
}

impl FileEditor {
    /// Whether what is in the editor is not what the file holds.
    fn dirty(&self, cx: &App) -> bool {
        self.saved.as_ref().is_some_and(|saved| self.state.read(cx).value().as_ref() != saved.as_ref())
    }
}

#[derive(Clone)]
enum FileBody {
    /// Markdown, drawn as the conversation draws it.
    Markdown(String),
    /// Anything else that is text: its lines, the language they are
    /// in, and how many lines were left out.
    Code(String, &'static str, usize),
    /// A picture, with its size in pixels when its header says. An SVG
    /// the pane holds whole is drawn from its text and not its path: a
    /// picture read by path is kept by path, and one saved here would
    /// go on showing as it was.
    Picture(Option<(u32, u32)>, Option<std::sync::Arc<gpui::Image>>),
    /// A Jupyter notebook, as markdown made of its cells
    /// (`files::notebook_markdown`); the file itself is JSON.
    Notebook(String),
    /// A page of HTML, drawn as far as the toolkit's text view reads
    /// it: its words, headings, lists, tables, links and pictures. No
    /// style sheet and no script is run.
    Html(String),
    /// A PDF: its first pages and their text. None while they are being
    /// drawn.
    Pages(Option<std::sync::Arc<PdfDoc>>),
    /// A comma- or tab-separated file: rows of cells, the first the
    /// head, each column's width in characters, and whether the file
    /// has more rows than are shown.
    Table(Vec<Vec<String>>, Vec<usize>, bool),
    /// A Word document or a PowerPoint deck, read as markdown
    /// (`emaki_core::office`). It is shown and nothing more: the file
    /// is not text, so there is no way it is written to show.
    Doc(String),
    /// A Word document, written in by its own pane (`word.rs`).
    Word,
    /// An Excel workbook: each sheet's name and what a table has.
    Sheets(Vec<(String, Vec<Vec<String>>, Vec<usize>, bool)>),
    /// Not something the window shows, or not readable: why.
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
    /// The file or folder itself to the clipboard, and what the
    /// clipboard holds into this folder (or beside this file).
    Copy,
    /// To the clipboard, and moved by the paste that follows.
    Cut,
    Paste,
    /// A copy beside it, under a free name.
    Duplicate,
    Rename,
    NewFile,
    NewFolder,
    Trash,
    /// Every folder of the tree closed.
    Collapse,
}

/// A name being asked for in a field in the tree, where the row is or
/// will be: a new name for that path, or a file or folder to make
/// inside it.
#[derive(Clone)]
pub enum FilePrompt {
    Rename(PathBuf),
    NewFile(PathBuf),
    NewFolder(PathBuf),
}

fn is_picture(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "tiff"))
}

/// A compared picture's sides as gpui draws them: before, then after.
pub(crate) fn picture_sides(path: &Path, d: &git::Diff) -> [Option<Arc<gpui::Image>>; 2] {
    use gpui::ImageFormat as F;
    let kind = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => F::Png,
        Some("jpg" | "jpeg") => F::Jpeg,
        Some("webp") => F::Webp,
        Some("gif") => F::Gif,
        Some("svg") => F::Svg,
        Some("bmp") => F::Bmp,
        Some("tif" | "tiff") => F::Tiff,
        Some("ico") => F::Ico,
        Some("pbm" | "pgm" | "ppm") => F::Pnm,
        _ => return [None, None],
    };
    let Some(p) = &d.pictures else { return [None, None] };
    let made = |b: &Option<Arc<[u8]>>| b.as_ref().map(|b| Arc::new(gpui::Image::from_bytes(kind, b.to_vec())));
    [made(&p.before), made(&p.after)]
}

/// A changed picture, as GitHub shows one: what it was beside what it is,
/// each on its own plate under a word for its side, its size in pixels
/// and in bytes. A picture added or deleted has the one side, the
/// width to itself. A large picture is brought down to its plate; a small
/// one keeps its size.
fn picture_compare(p: &git::Pictures, drawn: &[Option<Arc<gpui::Image>>; 2], theme: &gpui_component::Theme) -> AnyElement {
    let dark = theme.mode.is_dark();
    // GitHub's red and green, as the lines' numbers wear them.
    let (gone, put) = if dark { (rgb(0xf85149), rgb(0x3fb950)) } else { (rgb(0xcf222e), rgb(0x1a7f37)) };
    let both = p.before.is_some() && p.after.is_some();
    let side = |bytes: &Option<Arc<[u8]>>, image: &Option<Arc<gpui::Image>>, word: &'static str, mark: Rgba| {
        let (bytes, image) = (bytes.clone()?, image.clone()?);
        let size = match crate::workbench::image_dims(&bytes) {
            Some((w, h)) => format!("{w} × {h} · {}", human_size(bytes.len() as u64)),
            None => human_size(bytes.len() as u64),
        };
        Some(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(10.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .text_size(px(12.))
                        .child(div().size(px(7.)).rounded_full().bg(mark))
                        .child(div().font_weight(FontWeight::MEDIUM).child(word))
                        .child(div().min_w_0().truncate().text_color(theme.muted_foreground).child(size)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .p(px(16.))
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(if dark { theme.sidebar } else { theme.muted.opacity(0.4) })
                        .child(img(ImageSource::Image(image)).size_full().object_fit(ObjectFit::ScaleDown)),
                ),
        )
    };
    let (was, now) = if both { ("Before", "After") } else { ("Deleted", "Added") };
    h_flex()
        .size_full()
        .p(px(20.))
        .gap(px(16.))
        .items_stretch()
        .children(side(&p.before, &drawn[0], was, gone))
        .children(side(&p.after, &drawn[1], now, put))
        .into_any_element()
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
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if is_picture(path) {
        // An SVG is text as well as a picture: it has a source to show
        // and to edit.
        let text = (ext == "svg" && size as usize <= PREVIEW_BYTES).then(|| std::fs::read_to_string(path).ok()).flatten();
        let picture = text.as_ref().map(|t| std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Svg, t.as_bytes().to_vec())));
        return FileView { path: path.to_path_buf(), rel, size, mtime, body: FileBody::Picture(crate::workbench::file_image_dims(path), picture), source: text.map(Into::into), kinds: Vec::new(), rows_all: None };
    }
    if crate::word::opens(&ext).is_some() {
        return FileView { path: path.to_path_buf(), rel, size, mtime, body: FileBody::Word, source: None, kinds: Vec::new(), rows_all: None };
    }
    if ext == "pdf" || ext == "pptx" {
        return FileView { path: path.to_path_buf(), rel, size, mtime, body: FileBody::Pages(None), source: None, kinds: Vec::new(), rows_all: None };
    }
    // Word, PowerPoint and Excel files are read by `emaki_core::office`,
    // to look at: a document or a deck as markdown, a workbook as its
    // sheets.
    if emaki_core::office::reads(&ext) {
        let body = if size as usize > OFFICE_BYTES {
            FileBody::None("This file is too large to show here.".into())
        } else {
            match emaki_core::office::read(path, TABLE_ROWS) {
                Ok(emaki_core::office::Office::Markdown(text)) if text.trim().is_empty() => FileBody::None("There is no text in this file to show.".into()),
                Ok(emaki_core::office::Office::Markdown(text)) => FileBody::Doc(text),
                Ok(emaki_core::office::Office::Sheets(sheets)) => FileBody::Sheets(
                    sheets
                        .into_iter()
                        .map(|sheet| {
                            let (rows, widths) = table_shape(sheet.rows);
                            (sheet.name, rows, widths, sheet.more)
                        })
                        .collect(),
                ),
                Err(why) => FileBody::None(format!("Emaki cannot show this file: {why}.")),
            }
        };
        return FileView { path: path.to_path_buf(), rel, size, mtime, body, source: None, kinds: Vec::new(), rows_all: None };
    }
    // A Parquet file is a table with its columns' names as the first
    // row, as a comma-separated file has them.
    if ext == "parquet" {
        return match emaki_core::parquet_file::read(path, TABLE_ROWS) {
            Ok(table) => {
                let (names, kinds): (Vec<String>, Vec<String>) = table.columns.into_iter().unzip();
                let more = table.total > table.rows.len();
                let (rows, mut widths) = table_shape(std::iter::once(names).chain(table.rows).collect());
                for (w, kind) in widths.iter_mut().zip(&kinds) {
                    *w = (*w).max(kind.chars().count());
                }
                FileView { path: path.to_path_buf(), rel, size, mtime, body: FileBody::Sheets(vec![(String::new(), rows, widths, more)]), source: None, kinds, rows_all: Some(table.total) }
            }
            Err(why) => FileView { path: path.to_path_buf(), rel, size, mtime, body: FileBody::None(format!("Emaki cannot show this file: {why}.")), source: None, kinds: Vec::new(), rows_all: None },
        };
    }
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(PREVIEW_BYTES as u64 + 1).read_to_end(&mut bytes));
    let (body, source) = match read {
        Err(e) => (FileBody::None(format!("Could not read it: {e}")), None),
        Ok(_) => body_of(path, bytes),
    };
    FileView { path: path.to_path_buf(), rel, size, mtime, body, source, kinds: Vec::new(), rows_all: None }
}

/// Rows as the grid shows them: every row as long as the longest, up to
/// `TABLE_COLS`, a cell one line, and each column's width in characters.
fn table_shape(mut rows: Vec<Vec<String>>) -> (Vec<Vec<String>>, Vec<usize>) {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0).min(TABLE_COLS);
    let mut widths = vec![3usize; cols];
    for row in rows.iter_mut() {
        row.resize(cols, String::new());
        for (cell, w) in row.iter_mut().zip(widths.iter_mut()) {
            // A cell is one line here; what it holds past that is the file's.
            if let Some(at) = cell.find('\n') {
                cell.truncate(at);
                cell.push('…');
            }
            *w = (*w).max(cell.chars().count()).min(TABLE_CELL);
        }
    }
    (rows, widths)
}

/// What the pane shows for a file that holds `bytes`, and its source
/// when it can be edited. The bytes are the file's, or what its editor
/// holds when that is to be read before it is saved (`edited_body`).
fn body_of(path: &Path, mut bytes: Vec<u8>) -> (FileBody, Option<Rc<str>>) {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    // All of the file, and all of it text: it can be edited.
    let whole = bytes.len() <= PREVIEW_BYTES && std::str::from_utf8(&bytes).is_ok();
    let mut source: Option<Rc<str>> = None;
    let body = match () {
        () if bytes.iter().take(8192).any(|b| *b == 0) => FileBody::None("Emaki cannot show this kind of file.".into()),
        () if ext == "csv" || ext == "tsv" => {
            let cut = bytes.len() > PREVIEW_BYTES;
            bytes.truncate(PREVIEW_BYTES);
            let (rows, more) = emaki_core::files::table(&String::from_utf8_lossy(&bytes), if ext == "tsv" { '\t' } else { ',' }, TABLE_ROWS);
            let (rows, widths) = table_shape(rows);
            if whole && !more {
                source = Some(String::from_utf8_lossy(&bytes).into());
            }
            FileBody::Table(rows, widths, more || cut)
        }
        () => {
            let cut = bytes.len() > PREVIEW_BYTES;
            bytes.truncate(PREVIEW_BYTES);
            let text = String::from_utf8_lossy(&bytes).to_string();
            let lang = emaki_core::render_md::lang_for_path(&path.to_string_lossy());
            let notebook = (ext == "ipynb" && whole).then(|| emaki_core::files::notebook_markdown(&text)).flatten();
            if let Some(cells) = notebook {
                source = Some(text.as_str().into());
                FileBody::Notebook(cells)
            } else if lang == "markdown" && !cut {
                if whole {
                    source = Some(text.as_str().into());
                }
                FileBody::Markdown(text)
            } else if lang == "html" && !cut {
                if whole {
                    source = Some(text.as_str().into());
                }
                FileBody::Html(text)
            } else {
                let total = text.lines().count();
                if whole && total <= PREVIEW_LINES {
                    source = Some(text.as_str().into());
                }
                let kept: String = text.lines().take(PREVIEW_LINES).collect::<Vec<_>>().join("\n");
                // A file cut by size has more lines than were counted.
                let dropped = total.saturating_sub(PREVIEW_LINES) + usize::from(cut);
                FileBody::Code(kept, lang, dropped)
            }
        }
    };
    (body, source)
}

/// The file as it would read with `text` in it: what the pane shows as
/// it reads while its editor holds changes not saved.
fn edited_body(path: &Path, text: &str) -> FileBody {
    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        return FileBody::Picture(crate::workbench::file_image_dims(path), Some(std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Svg, text.as_bytes().to_vec()))));
    }
    body_of(path, text.as_bytes().to_vec()).0
}


/// Collects a page's text as the interpreter draws it: each glyph
/// with its place on the page.
struct PdfText {
    out: Vec<PdfGlyph>,
}

impl<'a> hayro::hayro_interpret::Device<'a> for PdfText {
    fn draw_path(&mut self, _: &hayro::kurbo::BezPath, _: hayro::hayro_interpret::DrawProps<'a>, _: &hayro::hayro_interpret::DrawMode) {}
    fn push_clip_path(&mut self, _: &hayro::hayro_interpret::ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<hayro::hayro_interpret::SoftMask<'a>>, _: hayro::hayro_interpret::BlendMode) {}
    fn draw_glyph_run(&mut self, run: &hayro::hayro_interpret::font::GlyphRun<'_, 'a>, props: hayro::hayro_interpret::DrawProps<'a>, _: &hayro::hayro_interpret::DrawMode) {
        use hayro::hayro_interpret::font::Glyph;
        use hayro::hayro_interpret::hayro_cmap::BfString;
        use hayro::kurbo::Point;
        for glyph in run.glyphs() {
            let Some(unicode) = glyph.as_unicode() else { continue };
            let text = match unicode {
                BfString::Char(c) => c.to_string(),
                BfString::String(s) => s,
            };
            // A glyph's own room is a thousand to the em.
            let place = props.transform * glyph.transform();
            let advance = match &**glyph {
                Glyph::Outline(outline) => outline.advance_width().unwrap_or(500.) as f64,
                _ => 500.,
            };
            let (from, to, up) = (place * Point::new(0., 0.), place * Point::new(advance, 0.), place * Point::new(0., 1000.));
            let em = (up.y - from.y).abs().max((up.x - from.x).abs()) as f32;
            if em < 0.5 || !from.x.is_finite() || !from.y.is_finite() {
                continue;
            }
            let (x0, x1) = (from.x.min(to.x) as f32, from.x.max(to.x) as f32);
            self.out.push(PdfGlyph { x0, x1: x1.max(x0 + em * 0.2), base: from.y as f32, em, text, gap: 0 });
        }
    }
    fn draw_image(&mut self, _: hayro::hayro_interpret::Image<'a, '_>, _: hayro::hayro_interpret::ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// Rows of the tree, dragged: their paths, none under another.
#[derive(Clone)]
pub(crate) struct TreeDrag(Vec<PathBuf>);

/// What follows the pointer while rows are dragged: a name, or how many.
struct TreeDragged(String);

impl Render for TreeDragged {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .h(px(24.))
            .px(px(9.))
            .gap(px(6.))
            .items_center()
            .rounded(px(7.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.primary.opacity(0.55))
            .shadow_md()
            .text_size(px(12.))
            .text_color(theme.foreground)
            .child(Icon::default().path("icons/file.svg").with_size(px(13.)).text_color(theme.muted_foreground))
            .child(div().max_w(px(200.)).truncate().child(self.0.clone()))
    }
}

/// Where what is out of the tree is held until it can no longer be
/// brought back (`emaki_core::fsops`).
fn fs_hold() -> PathBuf {
    emaki_core::paths::state_dir().join("held")
}

/// What is held and can no longer be reached goes to the system's
/// trash, and the places it stood in are removed.
fn fs_flush(held: Vec<PathBuf>) {
    for path in &held {
        let _ = crate::sys::trash_path(path);
    }
    if !held.is_empty() {
        emaki_core::fsops::Journal::tidy(&fs_hold());
    }
}

/// The journal a window starts with. What the window before it left
/// held, a quit or a crash ago, goes to the trash first: nothing knows
/// any more where it came from.
pub(crate) fn open_journal() -> emaki_core::fsops::Journal {
    let hold = fs_hold();
    fs_flush(emaki_core::fsops::Journal::leftovers(&hold));
    emaki_core::fsops::Journal::tidy(&hold);
    emaki_core::fsops::Journal::new(hold)
}

/// The paths that are under none of the others: a folder chosen takes
/// what is in it along.
fn tops_of(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|p| !paths.iter().any(|other| other != *p && p.starts_with(other))).cloned().collect()
}

fn path_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn kind_word(kind: emaki_core::fsops::Kind) -> &'static str {
    use emaki_core::fsops::Kind;
    match kind {
        Kind::Made => "making",
        Kind::Renamed => "renaming",
        Kind::Moved => "moving",
        Kind::Copied => "copying",
        Kind::Removed => "removing",
    }
}

/// What an HTML file's pictures were drawn for: the file, its words (as
/// a hash), the width in points, and the screen's pixels to a point,
/// in hundredths.
pub(crate) type HtmlKey = (PathBuf, u64, u32, u32);

fn html_hash(text: &str) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// A file shown as pages: a PDF's, or a Word document's.
fn drawn_pages(path: &Path, want_w: f32) -> Result<PdfDoc, String> {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("pptx") => pptx_pages(path, want_w),
        _ => pdf_pages(path, want_w),
    }
}

/// A PowerPoint deck's first slides as pictures, drawn by DeckCraft,
/// which is all Rust: a slide a page. Their words are not kept beside
/// them yet, so nothing on a slide is selected or found.
fn pptx_pages(path: &Path, want_w: f32) -> Result<PdfDoc, String> {
    use deckcraft_render::{render_slide, RenderOpts};
    let want_w = want_w.clamp(PDF_W_LEAST, PDF_W_MOST).round();
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > PDF_BYTES {
        return Err("This deck is too large to show here.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read it: {e}"))?;
    let drawn = std::panic::catch_unwind(move || {
        let deck = deckcraft_pptx::import(&bytes).ok()?;
        let (w, h) = (deck.slide_size.width as f32, deck.slide_size.height as f32);
        let png = |shot: &deckcraft_render::Image| std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, shot.to_png()));
        let mut pages = Vec::new();
        for ix in 0..deck.slides.len().min(PDF_PAGES) {
            let shot = render_slide(&deck, ix, &RenderOpts { scale: (want_w / w.max(1.)) as f64, ..Default::default() });
            let thumb = render_slide(&deck, ix, &RenderOpts { scale: (PDF_THUMB_W / w.max(1.)) as f64, ..Default::default() });
            pages.push(PdfPage { image: png(&shot), thumb: png(&thumb), px: (shot.width, shot.height), unit: (w, h), glyphs: 0..0 });
        }
        Some(PdfDoc { drawn_w: want_w, pages, total: deck.slides.len(), glyphs: Vec::new(), contents: Vec::new() })
    });
    match drawn {
        Ok(Some(doc)) if !doc.pages.is_empty() => Ok(doc),
        _ => Err("This deck could not be drawn.".into()),
    }
}

/// A PDF's first pages as pictures with their text, and how many pages
/// it has. Slow enough to be kept off the main thread. The renderer is
/// hayro, which is all Rust; a file it cannot read, or stops on, is
/// said so.
fn pdf_pages(path: &Path, want_w: f32) -> Result<PdfDoc, String> {
    let want_w = want_w.clamp(PDF_W_LEAST, PDF_W_MOST).round();
    use hayro::hayro_interpret::{interpret_page, Context, InterpreterCache, InterpreterSettings, TransformExt as _};
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > PDF_BYTES {
        return Err("This PDF is too large to show here.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read it: {e}"))?;
    let drawn = std::panic::catch_unwind(move || {
        let pdf = Pdf::new(bytes).ok()?;
        let cache = hayro::RenderCache::new();
        let words = InterpreterCache::new();
        let all = pdf.pages();
        let (mut pages, mut glyphs) = (Vec::new(), Vec::<PdfGlyph>::new());
        for page in all.iter().take(PDF_PAGES) {
            let (w, h) = page.render_dimensions();
            let scale = (want_w / w.max(1.)).min(8.);
            let pixmap = hayro::render(page, &cache, &InterpreterSettings::default(), &hayro::RenderSettings::default(), &hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE });
            let px = (pixmap.width() as u32, pixmap.height() as u32);
            let png = pixmap.into_png().ok()?;
            let small = PDF_THUMB_W / w.max(1.);
            let thumb = hayro::render(page, &cache, &InterpreterSettings::default(), &hayro::RenderSettings::default(), &hayro::PixmapSettings { x_scale: small, y_scale: small, bg_color: WHITE }).into_png().ok()?;
            // The text, by the same reading of the page that drew it.
            let mut text = PdfText { out: Vec::new() };
            let mut context = Context::new(page.initial_transform(true).to_kurbo(), hayro::kurbo::Rect::new(0., 0., w as f64, h as f64), &words, page.xref(), InterpreterSettings::default());
            interpret_page(page, &mut context, &mut text);
            let from = glyphs.len();
            for (ix, mut g) in text.out.into_iter().enumerate() {
                // A new line where the baseline moves or the text goes
                // back; a space where there is room for one.
                g.gap = match glyphs.last().filter(|_| ix > 0) {
                    None => 2,
                    Some(was) if (g.base - was.base).abs() > was.em.max(g.em) * 0.5 || g.x0 < was.x0 - was.em => 2,
                    Some(was) if g.x0 - was.x1 > g.em * 0.18 => 1,
                    Some(_) => 0,
                };
                glyphs.push(g);
            }
            pages.push(PdfPage { image: std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png)), thumb: std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, thumb)), px, unit: (w, h), glyphs: from..glyphs.len() });
        }
        Some(PdfDoc { drawn_w: want_w, pages, total: all.len(), glyphs, contents: pdf_contents::read(&pdf) })
    });
    match drawn {
        Ok(Some(doc)) if !doc.pages.is_empty() => Ok(doc),
        _ => Err("This PDF could not be drawn.".into()),
    }
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
/// A change of branch that is waiting to be told where the changes not
/// yet committed go.
#[derive(Clone)]
pub(crate) struct BranchAsk {
    pub(crate) name: String,
    pub(crate) create: bool,
    /// The choice so far: leave them on the branch being left.
    pub(crate) leave: bool,
    /// The files whose changes would conflict on the branch gone to,
    /// once git has been asked; bringing them is offered only when
    /// there are none.
    pub(crate) fit: Option<Vec<String>>,
    /// Not a question but a refusal to read: its title and its words.
    pub(crate) stop: Option<(String, String)>,
    /// A new branch is made from this branch, not from the one checked
    /// out.
    pub(crate) base: Option<String>,
    /// That branch is brought up to the remote's first.
    pub(crate) update: bool,
}

impl BranchAsk {
    pub(crate) fn stop(title: &str, words: String) -> Self {
        BranchAsk { name: String::new(), create: false, leave: false, fit: None, stop: Some((title.to_string(), words)), base: None, update: false }
    }
}

/// The ground of a panel's row under the pointer. A panel is on the
/// sidebar's ground, and in a light window the theme's muted grey is all
/// but that same colour, so the row takes a wash of the ink there.
pub(crate) fn row_hover(theme: &gpui_component::Theme, dark: bool) -> Hsla {
    if dark {
        theme.foreground.opacity(0.08)
    } else {
        theme.foreground.opacity(0.07)
    }
}

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
    pub(crate) fn files_root(&self) -> Option<PathBuf> {
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
        let to = (self.panel_on() != Some(files)).then_some(files);
        self.panel_go(to, cx);
    }

    /// Which of the side panels asked for do not fit beside the
    /// conversation in `room`, the content's width with the sidebar
    /// away: each from the top of the rank down takes its least while
    /// the conversation keeps its own (`conv_need`), and those left over
    /// do not fit. The one at the top always fits: a panel asked for is
    /// shown, whatever else gives way, and in the narrowest window the
    /// conversation is what is left (`main::MIN_W` keeps that at least
    /// `CONVERSATION_MIN`). Beside a conversation only.
    ///
    /// Also the least the ones that fit take of the row together.
    pub(crate) fn sides_fit(&self, room: Pixels) -> (Vec<Side>, Pixels) {
        let (mut unfit, mut fits) = (Vec::new(), px(0.));
        if self.page != Page::Session || self.detail.is_none() {
            return (unfit, fits);
        }
        let need = self.conv_need.get();
        for side in self.side_rank.into_iter().filter(|s| self.side_wanted(*s)) {
            if fits == px(0.) || need + fits + side.least() <= room {
                fits += side.least();
            } else {
                unfit.push(side);
            }
        }
        (unfit, fits)
    }

    /// Whether a side panel is asked for, whether or not it fits.
    pub(crate) fn side_wanted(&self, side: Side) -> bool {
        match side {
            Side::File => self.file_view.is_some(),
            Side::Left => self.files_on || self.outline_on,
            Side::Term => self.side_term,
        }
    }

    /// A side panel was asked for: it is the last to give its room up,
    /// and the others that no longer fit beside it go.
    pub(crate) fn side_asked(&mut self, side: Side, cx: &mut Context<Self>) {
        self.side_used(side);
        self.fit_sides(false, cx);
    }

    /// A side panel was opened or used: it is the last to give its room
    /// up.
    pub(crate) fn side_used(&mut self, side: Side) {
        if self.side_rank[0] != side {
            let mut rank = vec![side];
            rank.extend(self.side_rank.iter().copied().filter(|s| *s != side));
            self.side_rank.copy_from_slice(&rank);
        }
    }

    /// A press in a side panel: it is used, and keeps its room longest.
    /// What the press does is the panel's own; this only ranks it.
    pub(crate) fn side_pressed(&mut self, at: Point<Pixels>) {
        let (files, outline) = self.panels_shown();
        let side = if self.term_panel_bounds().is_some_and(|b| b.contains(&at)) {
            Side::Term
        } else if self.file_pane_bounds().is_some_and(|b| b.contains(&at)) {
            Side::File
        } else if (files && self.files_scroll.bounds().contains(&at)) || (outline && self.outline_scroll.bounds().contains(&at)) {
            Side::Left
        } else {
            return;
        };
        self.side_used(side);
    }

    /// The side panels that do not fit go, by their rank: the panel at
    /// the left and the terminal are put away as their buttons put them
    /// away, and the file shown is only not drawn (`fold_file`), so what
    /// is not saved in it stays. `held`: put away for the window's width,
    /// so their room still counts against the sidebar (`room_held`).
    pub(crate) fn fit_sides(&mut self, held: bool, cx: &mut Context<Self>) {
        // Putting a panel away clears what is held, as a press of its
        // button does; what this fit puts away is counted after.
        let was_held = self.room_held;
        let mut freed = px(0.);
        for side in self.sides_fit(self.view_w - crate::workbench::FRAME).0 {
            match side {
                Side::Left => self.panel_go(None, cx),
                Side::Term => self.term_go(None, cx),
                Side::File => continue,
            }
            freed += side.least();
        }
        if held {
            self.room_held = was_held + freed;
        }
    }

    /// The panel goes to the files, to the outline, or away, and the
    /// change is drawn.
    pub(crate) fn panel_go(&mut self, to: Option<bool>, cx: &mut Context<Self>) {
        let from = self.panel_on();
        if from == to {
            return;
        }
        self.room_held = px(0.);
        self.files_on = to == Some(true);
        self.outline_on = to == Some(false);
        if to.is_some() {
            self.side_asked(Side::Left, cx);
        }
        self.outline_at = None;
        self.panel_anim = Some((from, to, std::time::Instant::now(), self.panel_anim.map(|(_, _, _, n)| n + 1).unwrap_or(0)));
        self.save_ui(true);
        cx.notify();
        if from.is_some() {
            // The panel left is drawn while it goes: once more when it has.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(crate::workbench::PANEL_ANIM + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    /// The two buttons at the top strip's left end, as one control: a
    /// track with a segment each, and a raised plate under the one whose
    /// panel shows. The plate slides from one to the other, comes in
    /// under the first pressed and fades under the one pressed again.
    pub(crate) fn panel_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let track_bg = if dark { theme.tab_bar_segmented } else { theme.muted };
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
                // On the active one too, where it changes nothing: gpui
                // keeps "the pointer is over this" for the text's colour
                // and only hears the pointer leave while a hover style is
                // set. Without one the mark stayed lit when its segment
                // went back to rest with the pointer elsewhere.
                .hover(|s| s.text_color(theme.foreground))
                .managed_tooltip(move |window, cx| crate::workbench::tip(tip, window, cx))
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

    /// The panel that was showing until a moment ago, put away or
    /// replaced, drawn for the moment it takes to go.
    pub(crate) fn panel_leaving(&self) -> Option<bool> {
        if self.page != Page::Session || self.detail.is_none() {
            return None;
        }
        match self.panel_anim {
            Some((Some(from), to, at, _)) if to != Some(from) && at.elapsed() < crate::workbench::PANEL_ANIM => Some(from),
            _ => None,
        }
    }

    /// A panel as it arrives and as it goes: beside nothing it widens
    /// from nothing and fades in, and put away it narrows and fades out;
    /// in the other's place it fades in while the other fades out over
    /// it, so the conversation stays where it is. The wrapper is there at rest too, under the same
    /// name: an animation inside the panel is kept by the names above
    /// it, and began again when the wrapper went.
    fn panel_in(&self, files: bool, el: Div) -> AnyElement {
        let live = self.panel_anim.filter(|(_, _, at, _)| at.elapsed() < crate::workbench::PANEL_ANIM);
        // (widens or narrows, comes or goes)
        let play = match live {
            Some((from, to, _, _)) if to == Some(files) => Some((from.is_none(), true)),
            Some((from, to, _, _)) if from == Some(files) => Some((to.is_none(), false)),
            _ => None,
        };
        // Going while the other comes: over the other's place, out of the
        // row's layout.
        let over = matches!(live, Some((from, Some(_), _, _)) if from == Some(files));
        let serial = self.panel_anim.map(|(_, _, _, n)| n + 1).unwrap_or(0);
        let w = self.panel_w_now();
        div()
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .when(over, |d| d.absolute().top_0().left_0())
            .child(el)
            .with_animation(ElementId::Name(format!("panel-{files}-{serial}").into()), Animation::new(crate::workbench::PANEL_ANIM).with_easing(ease_out_quint()), move |d, t| match play {
                Some((wide, comes)) => {
                    let t = if comes { t } else { 1. - t };
                    d.w(if wide { (w * t).round() } else { w }).opacity(t)
                }
                None => d,
            })
            .into_any_element()
    }

    // -- the panel's edge ---------------------------------------------------

    /// Where the panel begins: after the sidebar when that is beside the
    /// content.
    fn panel_left(&self) -> Pixels {
        self.content_left()
    }

    /// The widest the panel may be dragged: the conversation keeps
    /// `CONVERSATION_MIN` beside it.
    fn panel_max(&self) -> Pixels {
        let term = if self.term_panel_shown() { crate::term_panel::TERM_MIN } else { px(0.) };
        PANEL_MAX.min(self.pane_w - self.conv_min.get() - term - self.file_pane_least()).max(PANEL_MIN)
    }

    /// Whether the file's pane is drawn.
    pub(crate) fn file_pane_shown(&self) -> bool {
        self.file_view.is_some() && self.page == Page::Session && self.detail.is_some() && !self.fold_file
    }

    /// The least the file's pane takes of the row, none when it is away.
    pub(crate) fn file_pane_least(&self) -> Pixels {
        if self.file_pane_shown() { FILE_MIN } else { px(0.) }
    }

    /// Where the file's pane begins: after the files or the outline.
    fn file_pane_left(&self) -> Pixels {
        let (files, outline) = self.panels_shown();
        self.panel_left() + if files || outline { self.panel_w_now() } else { px(0.) }
    }

    /// The pane's width as drawn: what it was dragged to, less when the
    /// conversation would be left with too little. It gives way before
    /// the terminal and the panel at its left do. Enlarged, it is the
    /// conversation's room as well, and on the way there or back it is
    /// between the two.
    pub(crate) fn file_pane_w(&self) -> Pixels {
        let term = if self.term_panel_shown() { self.term_panel_w() } else { px(0.) };
        let all = self.view_w - self.file_pane_left() - term;
        let own = self.file_w.min(all - self.conv_min.get()).max(FILE_MIN);
        let t = self.file_big_t();
        (own + (all.max(own) - own) * t).round()
    }

    /// How far the pane is toward the conversation's room: none as it
    /// usually stands, all of it enlarged.
    fn file_big_t(&self) -> f32 {
        let t = match self.file_big_at.map(|at| at.elapsed().as_secs_f32() / FILE_ANIM.as_secs_f32()) {
            Some(t) if t < 1. => ease_out_quint()(t),
            _ => 1.,
        };
        if self.file_big { t } else { 1. - t }
    }

    /// Whether the file has the conversation's room, all of it: the
    /// conversation is not drawn then.
    pub(crate) fn file_over(&self) -> bool {
        self.file_big && self.file_pane_shown() && self.file_big_t() >= 1.
    }

    /// The pane's enlarge button: the file takes the conversation's room,
    /// and gives it back at the next press or when it is closed.
    pub(crate) fn file_enlarge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file_big = !self.file_big;
        self.file_big_at = Some(std::time::Instant::now());
        // The caret goes back to the composer with its room.
        if !self.file_big && self.file_focus.is_focused(window) {
            let composer = self.composer.read(cx).focus_handle(cx);
            window.focus(&composer, cx);
        }
        cx.notify();
    }

    /// The conversation beside the file's pane: as it is, or, while the
    /// pane is on its way over it or back, at the width it has beside the
    /// pane, cut by the pane's edge and fading, so nothing in it is laid
    /// out again a frame at a time. None under a pane that has its room.
    pub(crate) fn beside_file(&self, conversation: Div) -> Option<AnyElement> {
        let t = if self.file_pane_shown() { self.file_big_t() } else { 0. };
        if t <= 0. {
            return Some(conversation.into_any_element());
        }
        if t >= 1. {
            return None;
        }
        let term = if self.term_panel_shown() { self.term_panel_w() } else { px(0.) };
        let all = self.view_w - self.file_pane_left() - term;
        let own = self.file_w.min(all - self.conv_min.get()).max(FILE_MIN);
        Some(div().flex_1().min_w_0().h_full().overflow_hidden().opacity(1. - t).child(conversation.flex_none().w((all - own).max(px(0.)))).into_any_element())
    }

    /// The pane's edge follows the pointer while it is held.
    pub(crate) fn file_drag_to(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            if std::mem::take(&mut self.file_drag) {
                self.save_ui(true);
            }
            return;
        }
        let term = if self.term_panel_shown() { crate::term_panel::TERM_MIN } else { px(0.) };
        let left = self.file_pane_left();
        let w = (e.position.x - left).clamp(FILE_MIN, (self.view_w - left - term - self.conv_min.get()).max(FILE_MIN));
        if w != self.file_w {
            self.file_w = w;
            cx.notify();
        }
    }

    /// The strip over the pane's right edge, lit under the pointer; a
    /// double click puts the width back.
    pub(crate) fn render_file_grip(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.file_pane_shown() || self.file_big {
            return None;
        }
        let line = cx.theme().primary.opacity(0.55);
        let held = self.file_drag;
        Some(
            div()
                .id("file-grip")
                .group("file-grip")
                .absolute()
                .top(crate::workbench::TITLEBAR_H)
                .bottom(crate::workbench::FRAME)
                .left(self.file_pane_left() + self.file_pane_w() - PANEL_GRIP / 2.)
                .w(PANEL_GRIP)
                .occlude()
                .cursor(CursorStyle::ResizeLeftRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                        if ev.click_count == 2 {
                            this.file_drag = false;
                            this.file_w = FILE_W;
                            this.save_ui(true);
                        } else {
                            this.file_drag = true;
                        }
                        swallow_click(window, cx);
                        cx.notify();
                    }),
                )
                .child(div().absolute().top_0().h_full().left(PANEL_GRIP / 2. - px(1.5)).w(px(2.)).when(held, |d| d.bg(line)).group_hover("file-grip", move |s| s.bg(line))),
        )
    }

    /// The panel's width as drawn: what it was dragged to, less when the
    /// conversation would be left with too little (the window narrowed,
    /// the terminal opened). The terminal gives way first, down to its
    /// least, then this panel.
    pub(crate) fn panel_w_now(&self) -> Pixels {
        self.panel_w.min(self.panel_max())
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
                Row::Node(node, depth, _) => Some(13. + 8. + 12. * (*depth + 1) as f32 + 10. + 5. + 16. + 5. + measure(&node.name) + 5. + 12. + 8. + 10.),
                Row::More(..) | Row::Edit(..) => None,
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
                .bottom(crate::workbench::FRAME)
                .left(self.panel_left() + self.panel_w_now() - PANEL_GRIP / 2.)
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
        self.keep_drafts(cx);
        if let Some(v) = &self.file_view {
            let now = std::fs::metadata(&v.path).ok().and_then(|m| m.modified().ok());
            if now.is_some() && now != v.mtime {
                let path = v.path.clone();
                self.show_file(&path, cx);
                cx.notify();
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
        if let Some(path) = self.file_editor.as_ref().map(|ed| ed.path.clone()) {
            self.read_file_base(path, cx);
        }
    }

    /// Ask git about the folder, off the main thread, one asking at a
    /// time; the tree is drawn again when the answer differs.
    pub(crate) fn read_git(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.tree.git_reading {
            return;
        }
        self.tree.git_reading = true;
        cx.spawn(async move |this, cx| {
            let dir = root.clone();
            let (status, branches, trouble) = cx
                .background_executor()
                .spawn(async move {
                    let (status, branches) = (git::status(&dir), git::branches(&dir));
                    // Asked only of a folder git gave nothing for.
                    let trouble = if status.is_none() && branches.is_none() { git::trouble(&dir) } else { None };
                    (status.unwrap_or_default(), branches, trouble)
                })
                .await;
            // `EMAKI_GO=gitlicence`: the strip as a Mac whose Xcode
            // licence has not been agreed to has it.
            let trouble = if std::env::var("EMAKI_GO").is_ok_and(|go| go == "gitlicence") {
                Some(git::Trouble::licence())
            } else {
                trouble
            };
            this.update(cx, |this, cx| {
                this.tree.git_reading = false;
                if this.tree.git.as_ref().is_none_or(|(was, st)| *was != root || **st != status) {
                    this.tree.changed = Rc::new(status.changed());
                    this.tree.git = Some((root.clone(), Rc::new(status)));
                    cx.notify();
                    // The comparison opened before git had answered
                    // (⌘⇧G with the files panel never shown) shows its
                    // first file now.
                    // And one whose file is no longer changed (it was
                    // committed, or discarded) lets it go: its lines
                    // stood there after a commit, beside an empty list.
                    let first = this.tree.changed.first().map(|(p, _)| p.clone());
                    let still: HashSet<PathBuf> = this.tree.changed.iter().map(|(p, _)| p.clone()).collect();
                    if let Some(c) = this.changes.as_mut().filter(|c| c.root == root && c.picked.as_ref().is_none_or(|p| !still.contains(p))) {
                        c.marked.retain(|p| still.contains(p));
                        c.left_out.retain(|p| still.contains(p));
                        c.picked = first.clone();
                        c.anchor = first.clone();
                        if c.marked.is_empty() {
                            c.marked = first.into_iter().collect();
                        }
                        if !c.history {
                            c.diff = None;
                            c.reading = false;
                            c.list.reset(0);
                            this.load_diff(cx);
                        }
                    }
                }
                let trouble = trouble.map(|t| (root.clone(), t));
                if this.tree.git_trouble != trouble {
                    this.tree.git_trouble = trouble;
                    cx.notify();
                }
                let branches = branches.map(|b| (root, Rc::new(b)));
                if branches.is_none() && this.branch_new_wait.take().is_some() {
                    this.notice = Some(Notice::error("this folder is not in a git repository"));
                    cx.notify();
                }
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

    /// A click on a file in the tree shows it, or puts it away when it
    /// is the one showing.
    fn file_clicked(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_view.as_ref().is_some_and(|v| v.path == path) {
            self.tree.picked = Some(path.to_path_buf());
            if self.file_pane_shown() {
                self.close_file_view(cx);
            } else {
                // Folded for want of room: the click asks for it, and
                // what gives way is by the rank.
                self.side_asked(Side::File, cx);
                cx.notify();
            }
        } else {
            self.file_preview(path, window, cx);
        }
    }

    /// Shows a file between the tree and the conversation. The pane
    /// comes in motion when there was none; another file takes the place
    /// of the one showing.
    pub(crate) fn file_preview(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.files_root().is_none() {
            return;
        }
        // One file shows at a time, so another takes this one's place:
        // not while this one has changes not saved.
        if self.file_view.as_ref().is_some_and(|v| v.path != path) && !self.file_guard(FileThen::Open(path.to_path_buf()), cx) {
            return;
        }
        self.tree.picked = Some(path.to_path_buf());
        let was = self.file_view.is_some();
        self.show_file(path, cx);
        self.file_view_scroll.set_offset(point(px(0.), px(0.)));
        self.file_serial += 1;
        self.pdf_reset();
        if !was {
            self.file_gone = None;
            self.file_anim = Some((true, std::time::Instant::now(), self.file_serial));
        }
        // The file is the last to give its room up, and the files it is
        // found in, when they are what shows at the left, are next: the
        // panels that no longer fit beside them go.
        if self.files_on {
            self.side_used(Side::Left);
        }
        self.side_asked(Side::File, cx);
        // A file opened from the tree leaves the keyboard in the tree: the
        // row is still what ⌘C and ⌘V are about, until a click in the
        // file's pane takes it there. Opened any other way, the keyboard
        // is the window's.
        if !self.tree_focus.is_focused(window) {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    /// A Word document is read off the main thread and given a pane of
    /// its own to be written in. One showing already stays, with what
    /// was written in it; when the file has changed on the disk and
    /// nothing here is unsaved, it is read again.
    fn show_word(&mut self, path: &Path, cx: &mut Context<Self>) {
        let on_disk = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
        let have = self.word.as_ref().map(|w| w.read(cx)).filter(|w| w.path == path).map(|w| (w.dirty(), w.mtime));
        match have {
            Some((dirty, mtime)) if dirty || mtime == on_disk => return,
            Some(_) => {}
            None => self.word = None,
        }
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let from = path.clone();
            let got = cx.background_executor().spawn(async move { crate::word::read(&from) }).await;
            let _ = this.update(cx, |this, cx| {
                if !this.file_view.as_ref().is_some_and(|v| v.path == path) {
                    return;
                }
                match (got, this.word.clone().filter(|w| w.read(cx).path == path)) {
                    (Ok(doc), Some(pane)) => pane.update(cx, |pane, cx| pane.reload(doc, cx)),
                    (Ok(doc), None) => {
                        let pane = cx.new(|cx| crate::word::WordPane::new(&path, doc, cx));
                        cx.subscribe(&pane, |this, _, said: &crate::word::WordEvent, cx| {
                            if let crate::word::WordEvent::Said(words) = said {
                                this.notice = Some(crate::workbench::Notice::error(words.clone()));
                            }
                            cx.notify();
                        })
                        .detach();
                        this.word = Some(pane);
                    }
                    (Err(why), _) => {
                        this.word = None;
                        if let Some(v) = this.file_view.as_mut() {
                            v.body = FileBody::None(format!("Emaki cannot show this file: {why}."));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the file for the pane. A PDF's pages are drawn off the main
    /// thread and put in when they are done, if the file is still the
    /// one showing.
    fn show_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let view = read_file(path, &root);
        let pdf = matches!(view.body, FileBody::Pages(None));
        // The pages drawn so far stay while a changed file is drawn again.
        let kept = self.file_view.take().filter(|was| pdf && was.path == path).map(|was| was.body);
        let word = matches!(view.body, FileBody::Word);
        self.file_view = Some(FileView { body: kept.unwrap_or(view.body.clone()), ..view });
        if word {
            self.show_word(path, cx);
        } else {
            self.word = None;
        }
        if !pdf {
            return;
        }
        let path = path.to_path_buf();
        let want = self.pdf_want_now();
        cx.spawn(async move |this, cx| {
            let from = path.clone();
            let got = cx.background_executor().spawn(async move { drawn_pages(&from, want) }).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(v) = this.file_view.as_mut().filter(|v| v.path == path) {
                    v.body = match got {
                        Ok(doc) => FileBody::Pages(Some(std::sync::Arc::new(doc))),
                        Err(why) => FileBody::None(why),
                    };
                    this.pdf_sel = None;
                    this.pdf_find(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// An HTML file as it reads is the system's web view, set over the
    /// pane's room: made when one shows, told of another file or of this
    /// one changed on the disk, moved with the pane, put out of sight
    /// while the window draws over its pages, and let go with the file.
    /// It shows the file as saved, not as its editor has it. Called at
    /// every draw. Where the system has no web view to give, the page
    /// is `html_fit`'s to draw.
    pub(crate) fn html_web_sync(&mut self, window: &mut Window, _: &mut Context<Self>) {
        let shows = self.file_pane_shown() && !self.file_raw;
        let Some((path, mtime)) = self.file_view.as_ref().filter(|v| shows && matches!(v.body, FileBody::Html(_))).map(|v| (v.path.clone(), v.mtime)) else {
            // Let go, and the keyboard with it back to the window: a
            // click in the page had given it to the page.
            if self.html_web.take().is_some() {
                crate::sys::keys_to_window(window);
            }
            return;
        };
        if self.html_web.is_none() && !self.html_web_none {
            let Some(root) = self.files_root() else { return };
            self.html_web = crate::html_web::Web::open(window, &path, mtime, &root);
            self.html_web_none = self.html_web.is_none();
        }
        let over = self.over_page();
        // Its lower corners are the card's where nothing stands between.
        let (files, outline) = self.panels_shown();
        let (left, right) = (!files && !outline, self.file_over() && !self.term_panel_shown());
        let Some(web) = self.html_web.as_mut() else { return };
        if web.file != path || web.mtime != mtime {
            web.load(&path, mtime);
        }
        web.round(left, right, f32::from(crate::workbench::CARD_R));
        // Where it stands is the pane's to say as it is laid out
        // (`Web::placer`, in `file_pane`). Out of sight, the keyboard is
        // not left with it.
        let shown = !over && web.is_placed();
        web.show(shown);
        if !shown {
            crate::sys::keys_to_window(window);
        }
    }

    /// An HTML file is drawn as a page off the main thread
    /// (`html_shot::shoot`) and shown as pictures, so it is drawn again
    /// when what it was drawn for is no longer so: the words (the file
    /// changed, or its editor's text came back), the pane's width, the
    /// screen's pixels. New words are drawn at once; a width waits until
    /// it has stood still a moment. The last pictures show until the new
    /// are in, and before the first the page reads as plain words.
    /// Called at every draw.
    pub(crate) fn html_fit(&mut self, cx: &mut Context<Self>) {
        let Some((path, text)) = self.file_view.as_ref().filter(|_| !self.file_raw).and_then(|v| match &v.body {
            FileBody::Html(text) => Some((v.path.clone(), text.clone())),
            _ => None,
        }) else {
            self.html_fit_at = None;
            return;
        };
        // The web view shows it, where there is one.
        if self.html_web.is_some() || !self.html_web_none {
            return;
        }
        let want = self.html_want.get().round();
        if want <= 0. || self.html_busy {
            return;
        }
        let key: HtmlKey = (path.clone(), html_hash(&text), want as u32, (self.ui_scale * 100.) as u32);
        let have = self.html_shot.as_ref().map(|(k, _)| k);
        if have == Some(&key) {
            self.html_fit_at = None;
            return;
        }
        const STILL: Duration = Duration::from_millis(280);
        // Other words are drawn at once; a width, once it stands still.
        let fresh = have.is_none_or(|k| (&k.0, k.1) != (&key.0, key.1));
        let waited = matches!(&self.html_fit_at, Some((k, at)) if *k == key && at.elapsed() >= STILL);
        if fresh || waited {
            self.html_fit_at = None;
            self.html_busy = true;
            let scale = self.ui_scale;
            cx.spawn(async move |this, cx| {
                let from = path.clone();
                let got = cx.background_executor().spawn(async move { crate::html_shot::shoot(&text, &from, want, scale) }).await;
                let _ = this.update(cx, |this, cx| {
                    this.html_busy = false;
                    if let Ok(shot) = got {
                        this.html_shot = Some((key, std::sync::Arc::new(shot)));
                    }
                    cx.notify();
                });
            })
            .detach();
        } else if !matches!(&self.html_fit_at, Some((k, _)) if *k == key) {
            self.html_fit_at = Some((key, std::time::Instant::now()));
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(STILL + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    /// The pages drawn before the ones showing are let go, and the
    /// toolkit's copies of their pictures with them: it keeps every
    /// picture it has read until told.
    fn pdf_under_drop(&mut self, cx: &mut Context<Self>) {
        if let Some((old, _)) = self.pdf_under.take() {
            for page in &old.pages {
                page.image.clone().remove_asset(cx);
            }
        }
    }

    /// How wide in pixels a PDF's page is on the screen: what the pane
    /// last drew it at, or before it has, what its width leaves.
    fn pdf_want_now(&self) -> f32 {
        let want = self.pdf_want.get();
        if want > 0. { want } else { (f32::from(self.file_pane_w()) - 37.) * self.ui_scale }
    }

    /// A PDF's pages are pictures, sharp at one size: the pixels the
    /// pane shows them in. Drawn at a size of its own, a page was scaled
    /// on the way to the screen and its letters were soft. So when the
    /// page's width on the screen is no longer what the pictures were
    /// drawn for, and has stood still a moment, they are drawn again,
    /// the old ones showing until the new are in. Called at every draw.
    pub(crate) fn pdf_fit(&mut self, cx: &mut Context<Self>) {
        if self.pdf_under.as_ref().is_some_and(|(_, at)| at.elapsed() >= PDF_UNDER) {
            self.pdf_under_drop(cx);
        }
        let Some((path, drawn)) = self.file_view.as_ref().and_then(|v| match &v.body {
            FileBody::Pages(Some(doc)) => Some((v.path.clone(), doc.drawn_w)),
            _ => None,
        }) else {
            self.pdf_fit_at = None;
            return;
        };
        let want = self.pdf_want.get().clamp(PDF_W_LEAST, PDF_W_MOST).round();
        if self.pdf_want.get() <= 0. || (want - drawn).abs() <= 2. || self.pdf_fitting {
            self.pdf_fit_at = None;
            return;
        }
        const STILL: Duration = Duration::from_millis(280);
        match self.pdf_fit_at {
            Some((w, at)) if w == want && at.elapsed() >= STILL => {
                self.pdf_fit_at = None;
                self.pdf_fitting = true;
                cx.spawn(async move |this, cx| {
                    let from = path.clone();
                    let got = cx.background_executor().spawn(async move { drawn_pages(&from, want) }).await;
                    let _ = this.update(cx, |this, cx| {
                        this.pdf_fitting = false;
                        if let (Ok(doc), Some(v)) = (got, this.file_view.as_mut().filter(|v| v.path == path && matches!(v.body, FileBody::Pages(Some(_))))) {
                            // The pictures just made are read by the
                            // toolkit over the next frames, and a page
                            // is blank until its own is. The ones they
                            // take over from stay under them for that
                            // while, so the page does not blink.
                            if let FileBody::Pages(Some(old)) = std::mem::replace(&mut v.body, FileBody::Pages(Some(std::sync::Arc::new(doc)))) {
                                this.pdf_under_drop(cx);
                                this.pdf_under = Some((old, std::time::Instant::now()));
                                cx.spawn(async move |this, cx| {
                                    cx.background_executor().timer(PDF_UNDER + Duration::from_millis(20)).await;
                                    let _ = this.update(cx, |_, cx| cx.notify());
                                })
                                .detach();
                            }
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            Some((w, _)) if w == want => {}
            _ => {
                self.pdf_fit_at = Some((want, std::time::Instant::now()));
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(STILL + Duration::from_millis(20)).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        }
    }

    /// The pane goes, in motion; drawn as it was for that moment.
    pub(crate) fn close_file_view(&mut self, cx: &mut Context<Self>) {
        if !self.file_guard(FileThen::Close, cx) {
            return;
        }
        if let Some(v) = self.file_view.take() {
            // The conversation has its room back at once, and the pane
            // goes from the width it has beside it.
            self.file_big = false;
            self.file_big_at = None;
            self.tree_focus_back = true;
            self.file_gone = Some(v);
            self.file_serial += 1;
            self.file_anim = Some((false, std::time::Instant::now(), self.file_serial));
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FILE_ANIM + Duration::from_millis(20)).await;
                let _ = this.update(cx, |this, cx| {
                    if this.file_view.is_none() && this.file_anim.is_some_and(|(_, at, _)| at.elapsed() >= FILE_ANIM) {
                        this.file_gone = None;
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// The folder showing changed: the file shown is that folder's own,
    /// kept while another folder's session shows and back when one of
    /// this folder's does. Nothing moves; the pane is another folder's.
    pub(crate) fn sync_file_root(&mut self, cx: &mut Context<Self>) {
        let root = self.files_root();
        if root == self.file_root {
            return;
        }
        match (self.file_root.take(), self.file_view.take()) {
            (Some(old), Some(v)) => {
                self.file_for.insert(old, v.path);
            }
            (Some(old), None) => {
                self.file_for.remove(&old);
            }
            _ => {}
        }
        (self.file_gone, self.file_anim) = (None, None);
        self.pdf_reset();
        self.file_root = root.clone();
        if let Some(path) = root.and_then(|r| self.file_for.get(&r).cloned()).filter(|p| p.is_file()) {
            self.tree.picked = Some(path.clone());
            self.show_file(&path, cx);
        }
    }

    // -- a PDF's text ---------------------------------------------------------

    fn pdf_doc(&self) -> Option<std::sync::Arc<PdfDoc>> {
        match &self.file_view.as_ref()?.body {
            FileBody::Pages(Some(doc)) => Some(doc.clone()),
            _ => None,
        }
    }

    /// Another file, or none: what was selected and found was the last one's.
    fn pdf_reset(&mut self) {
        (self.pdf_sel, self.pdf_drag, self.pdf_find_open, self.pdf_hit) = (None, None, false, 0);
        self.pdf_hits.clear();
        self.pdf_bounds.borrow_mut().clear();
    }

    /// The glyph under a point of the window, on whichever page it is.
    fn pdf_glyph_at(&self, at: Point<Pixels>) -> Option<usize> {
        let doc = self.pdf_doc()?;
        let bounds = self.pdf_bounds.borrow();
        // The page under the point, or the nearest above or below it.
        let (page, b) = bounds.iter().enumerate().min_by_key(|(_, b)| if at.y < b.top() { f32::from(b.top() - at.y) as i64 } else if at.y > b.bottom() { f32::from(at.y - b.bottom()) as i64 } else { 0 })?;
        let unit = doc.pages.get(page)?.unit;
        let scale = unit.0 / f32::from(b.size.width).max(1.);
        doc.glyph_at(page, f32::from(at.x - b.left()) * scale, f32::from(at.y - b.top()) * scale)
    }

    /// The left button went down on a page: a selection begins there,
    /// by the letter, or is the word or the line on a second or third
    /// click. With ⇧ the one there reaches to the press.
    fn pdf_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.file_focus, cx);
        let (Some(doc), Some(at)) = (self.pdf_doc(), self.pdf_glyph_at(e.position)) else { return };
        match (e.click_count, self.pdf_sel) {
            (2, _) => (self.pdf_sel, self.pdf_drag) = (Some(doc.word(at)), None),
            (n, _) if n >= 3 => (self.pdf_sel, self.pdf_drag) = (Some(doc.line(at)), None),
            (_, Some((from, _))) if e.modifiers.shift => (self.pdf_sel, self.pdf_drag) = (Some((from.min(at), from.max(at))), Some(from)),
            // A press alone selects nothing until the pointer moves.
            _ => (self.pdf_sel, self.pdf_drag) = (None, Some(at)),
        }
        cx.notify();
    }

    /// The pointer moved with the button held: the selection reaches it.
    fn pdf_mouse_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(anchor) = self.pdf_drag else { return };
        if e.pressed_button != Some(MouseButton::Left) {
            self.pdf_drag = None;
            return;
        }
        if let Some(at) = self.pdf_glyph_at(e.position) {
            let sel = Some((anchor.min(at), anchor.max(at)));
            if sel != self.pdf_sel {
                self.pdf_sel = sel;
                cx.notify();
            }
        }
    }

    /// A right click on a page: Copy for what is selected, Select All,
    /// and what can be done with the file.
    fn pdf_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.file_focus, cx);
        let mut items: Vec<(&'static str, MenuDo)> = Vec::new();
        if self.pdf_sel.is_some() {
            items.push(("Copy", MenuDo::Pdf(PdfDo::Copy)));
        }
        items.push(("Select All", MenuDo::Pdf(PdfDo::SelectAll)));
        self.file_pane_menu(items, at, cx);
    }

    /// Escape with the keyboard in the file's pane lets go of what is
    /// selected there, as an editor does: the editor's selection, words
    /// selected in a file as it reads, a block of a table's cells.
    /// Whether there was anything to let go.
    pub(crate) fn file_unselect(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.file_view.is_none() || !self.file_focus.contains_focused(window, cx) {
            return false;
        }
        if let Some(state) = self.file_editor.as_ref().map(|ed| ed.state.clone()).filter(|state| !state.read(cx).selected_range().is_empty()) {
            state.update(cx, |s, cx| {
                let caret = s.cursor();
                s.set_selected_range(caret..caret, cx);
            });
            return true;
        }
        if gpui_base::TextSelection::has_selection(window, cx) {
            gpui_base::TextSelection::clear(window, cx);
            cx.notify();
            return true;
        }
        if let Some(at) = self.table_at.as_mut().filter(|at| at.to.is_some()) {
            at.to = None;
            cx.notify();
            return true;
        }
        false
    }

    /// The pane's menu: what the place pressed offers, and nothing of
    /// the file's own. What is asked of the file (Add to Message, Open,
    /// Reveal, its paths) is on its row in the tree and in the pane's
    /// head; here it stood under a Copy that was about the words.
    fn file_pane_menu(&mut self, items: Vec<(&'static str, MenuDo)>, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.file_view.is_none() || items.is_empty() {
            return;
        }
        self.open_menu(at, items, cx);
    }

    /// A right click on a file that is not a PDF: Copy for the words
    /// selected, or the picture. The text
    /// under the pointer selects its word at the same press, so what is
    /// selected is asked once the press has been handed round.
    fn file_body_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let picture = self.file_view.as_ref().filter(|v| matches!(v.body, FileBody::Picture(..))).map(|v| v.path.clone());
        let this = cx.entity();
        window.defer(cx, move |window, cx| {
            let text = gpui_base::TextSelection::selected_text(window, cx);
            this.update(cx, |this, cx| {
                let mut items: Vec<(&'static str, MenuDo)> = Vec::new();
                if !text.trim().is_empty() {
                    items.push(("Copy", MenuDo::Copy(text)));
                }
                if let Some(path) = picture {
                    items.push(("Copy Image", MenuDo::CopyImage(crate::workbench::Pic::File(path))));
                }
                this.file_pane_menu(items, at, cx);
            });
        });
    }

    /// One of the two buttons over a PDF: its list of pages, or its
    /// table of contents. The one showing goes at a second press.
    fn pdf_side_toggle(&mut self, pages: bool, cx: &mut Context<Self>) {
        let was = self.pdf_side;
        self.pdf_side = (was != Some(pages)).then_some(pages);
        self.pdf_side_anim = Some((was, std::time::Instant::now(), self.pdf_side_anim.map(|(_, _, n)| n + 1).unwrap_or(1)));
        self.save_ui(true);
        if was.is_some() {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(FILE_ANIM + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        cx.notify();
    }

    /// The page at the top of the pane's view.
    fn pdf_page_now(&self) -> usize {
        let line = self.file_view_scroll.bounds().top() + px(90.);
        let bounds = self.pdf_bounds.borrow();
        bounds.iter().rposition(|b| b.size.height > px(0.) && b.top() <= line).unwrap_or(0)
    }

    /// Brings a page to the top of the pane.
    fn pdf_go_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let Some(b) = self.pdf_bounds.borrow().get(page).copied() else { return };
        let (view, at) = (self.file_view_scroll.bounds(), self.file_view_scroll.offset());
        let to = at.y - (b.top() - view.top() - px(10.));
        self.file_view_scroll.set_offset(point(at.x, to.clamp(-self.file_view_scroll.max_offset().y, px(0.))));
        cx.notify();
    }

    pub(crate) fn pdf_do(&mut self, what: PdfDo, cx: &mut Context<Self>) {
        let Some(doc) = self.pdf_doc() else { return };
        match what {
            PdfDo::Copy => {
                if let Some((from, to)) = self.pdf_sel {
                    cx.write_to_clipboard(ClipboardItem::new_string(doc.text(from, to)));
                }
            }
            PdfDo::SelectAll => self.pdf_sel = (!doc.glyphs.is_empty()).then(|| (0, doc.glyphs.len() - 1)),
        }
        cx.notify();
    }

    /// A key in the pane with a PDF showing: ⌘C copies what is selected
    /// and ⌘A selects it all. Anything else is the window's.
    fn pdf_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.pdf_doc().is_none() || !e.keystroke.modifiers.secondary() {
            return;
        }
        match e.keystroke.key.as_str() {
            "c" if self.pdf_sel.is_some() => self.pdf_do(PdfDo::Copy, cx),
            "a" => self.pdf_do(PdfDo::SelectAll, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// ⌘F with the pane's PDF in hand: its own find row, with the caret
    /// in it. False when there is no PDF to find in or the keyboard is
    /// elsewhere, and the find is the conversation's.
    pub(crate) fn pdf_find_open(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let here = self.file_focus.is_focused(window) || self.pdf_find_input.read(cx).focus_handle(cx).is_focused(window);
        if self.pdf_doc().is_none() || !self.file_pane_shown() || !here {
            return false;
        }
        self.pdf_find_open = true;
        self.pdf_find_input.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.pdf_find(cx);
        true
    }

    pub(crate) fn pdf_find_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pdf_find_open = false;
        self.pdf_hits.clear();
        window.focus(&self.file_focus, cx);
        cx.notify();
    }

    /// Finds the row's words in the PDF and goes to the first place.
    pub(crate) fn pdf_find(&mut self, cx: &mut Context<Self>) {
        if !self.pdf_find_open {
            return;
        }
        let words = self.pdf_find_input.read(cx).value().to_string();
        self.pdf_hits = self.pdf_doc().map(|doc| doc.find(&words)).unwrap_or_default();
        self.pdf_hit = 0;
        self.pdf_find_show(cx);
    }

    /// To the next place the words are, or the one before, round the ends.
    pub(crate) fn pdf_find_step(&mut self, by: i64, cx: &mut Context<Self>) {
        let n = self.pdf_hits.len() as i64;
        if n > 0 {
            self.pdf_hit = ((self.pdf_hit as i64 + by).rem_euclid(n)) as usize;
            self.pdf_find_show(cx);
        }
    }

    /// Brings the place the find is on into view, a little under the
    /// pane's top.
    fn pdf_find_show(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        let (Some(doc), Some((from, _))) = (self.pdf_doc(), self.pdf_hits.get(self.pdf_hit).copied()) else { return };
        let Some(page) = doc.pages.iter().position(|p| p.glyphs.contains(&from)) else { return };
        let Some(b) = self.pdf_bounds.borrow().get(page).copied() else { return };
        let y = b.top() + b.size.height * (doc.glyphs[from].base / doc.pages[page].unit.1.max(1.));
        let view = self.file_view_scroll.bounds();
        if y < view.top() + px(40.) || y > view.bottom() - px(40.) {
            let at = self.file_view_scroll.offset();
            let to = at.y - (y - view.top() - px(110.));
            self.file_view_scroll.set_offset(point(at.x, to.clamp(-self.file_view_scroll.max_offset().y, px(0.))));
        }
    }

    /// The toolkit's name for a language, or plain text where it has no
    /// grammar for it.
    fn editor_language(&self, lang: &str) -> &'static str {
        match lang {
            "jsx" => "javascript",
            "scss" => "css",
            "python" => "python",
            "r" => "r",
            "javascript" => "javascript",
            "typescript" => "typescript",
            "tsx" => "tsx",
            "json" => "json",
            "bash" => "bash",
            "yaml" => "yaml",
            "toml" => "toml",
            "markdown" => "markdown",
            "html" => "html",
            "css" => "css",
            "sql" => "sql",
            "go" => "go",
            "rust" => "rust",
            "c" => "c",
            "cpp" => "cpp",
            "java" => "java",
            "ruby" => "ruby",
            "swift" => "swift",
            "lua" => "lua",
            "csv" => "csv",
            "tsv" => "tsv",
            "csharp" => "csharp",
            "kotlin" => "kotlin",
            "php" => "php",
            "zig" => "zig",
            "scala" => "scala",
            "elixir" => "elixir",
            "proto" => "proto",
            "graphql" => "graphql",
            "cmake" => "cmake",
            "make" => "make",
            "diff" => "diff",
            "svelte" => "svelte",
            "astro" => "astro",
            _ => "text",
        }
    }

    /// The editor for the file showing, made when the file, or what it
    /// holds, is another: code, or markdown as it is written.
    pub(crate) fn sync_file_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_file_editor_only(window, cx);
        self.sync_file_marks(cx);
    }

    fn sync_file_editor_only(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.file_editor_want();
        // The same file, changed in the editor and not saved: the editor
        // stays as it is whatever the disk now holds. Saving asks.
        if let (Some(want), Some(ed)) = (&want, &self.file_editor) {
            if ed.key == want.key || (ed.path == want.path && ed.dirty(cx)) {
                return;
            }
        }
        // The editor goes, or is another file's. It is kept, whole, for
        // when the file shows again: what was typed and not saved, and
        // the history ⌘Z walks back through. One with nothing to save is
        // kept for its history alone, a few of them.
        if let Some(ed) = self.file_editor.take() {
            self.file_parked.retain(|p| p.path != ed.path);
            self.file_parked.push(ed);
            let clean: Vec<PathBuf> = self.file_parked.iter().filter(|p| !p.dirty(cx)).map(|p| p.path.clone()).collect();
            if clean.len() > PARKED_CLEAN {
                let gone = &clean[..clean.len() - PARKED_CLEAN];
                self.file_parked.retain(|p| !gone.contains(&p.path));
            }
        }
        self.file_conflict = None;
        let Some(want) = want else { return };
        // The one kept for this file, if what it was made from is what
        // the file still holds, or it has changes of its own.
        if let Some(at) = self.file_parked.iter().position(|p| p.path == want.path) {
            let ed = self.file_parked.remove(at);
            if ed.key == want.key || ed.dirty(cx) {
                self.file_editor = Some(ed);
                return;
            }
        }
        let language = self.editor_language(want.lang);
        // What was typed and not saved is back, over the file it was
        // typed over.
        let draft = self.file_drafts.remove(&want.path).filter(|_| want.saved.is_some());
        let mtime = draft.as_ref().map(|d| d.base).unwrap_or(want.mtime);
        let text = draft.map(|d| d.text).unwrap_or_else(|| want.text.to_string());
        // One step of indent is the file's own, and a new line starts
        // where the language says (`emaki_core::indent`).
        let lang = want.lang;
        let (columns, tabs) = emaki_core::indent::unit(lang, &text);
        let step = if tabs { "\t".to_string() } else { " ".repeat(columns) };
        let state = cx.new(|cx| {
            let mut state = gpui_component::input::EditorState::new(window, cx).language(language).line_number(true).soft_wrap(true).tab_size(gpui_component::input::TabSize { tab_size: columns, hard_tabs: tabs });
            state.set_next_line_indent(move |before, above| Some(emaki_core::indent::after(lang, before, above, &step)));
            state.set_value(text, window, cx);
            state
        });
        // A click on a mark in the margin opens that change.
        let opener = cx.entity().downgrade();
        state.update(cx, |s, _| {
            s.set_gutter_click(move |id, at, _, cx| {
                let _ = opener.update(cx, |this, cx| this.open_file_peek(id, at, cx));
            })
        });
        // The head says "not saved" from the first letter, and the
        // margin's marks follow what is typed.
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&state, |this, state, ev: &gpui_component::input::InputEvent, _| {
            if let gpui_component::input::InputEvent::Change = ev {
                for ed in this.file_editor.iter_mut().chain(this.file_parked.iter_mut()).filter(|ed| ed.state == state) {
                    ed.marks_due = true;
                }
            }
        })
        .detach();
        let path = want.path.clone();
        self.file_editor = Some(FileEditor { key: want.key, path: want.path, lang, state, saved: want.saved, mtime, base: None, marks_due: false, marks_busy: false, hunks: Vec::new() });
        self.read_file_base(path, cx);
    }

    /// Ask git for its copy of a file an editor holds, off the main
    /// thread: at the editor's making, after a save, and on the tree's
    /// clock, since a commit or a staging elsewhere changes it.
    fn read_file_base(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let asked = path.clone();
            let base = cx.background_executor().spawn(async move { git::base_text(&asked) }).await.map(std::sync::Arc::<str>::from);
            let _ = this.update(cx, |this, cx| {
                for ed in this.file_editor.iter_mut().chain(this.file_parked.iter_mut()).filter(|ed| ed.path == path) {
                    if ed.base != base {
                        ed.base = base.clone();
                        ed.marks_due = true;
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// A click on a mark in the margin: the change it stands for, the
    /// lines git has beside the lines there now, on a card at the click
    /// (`render_file_peek`). Not while the marks are behind the text:
    /// the place would be another's.
    fn open_file_peek(&mut self, id: usize, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(ed) = self.file_editor.as_ref().filter(|ed| !ed.marks_due && !ed.marks_busy) else { return };
        let (Some(hunk), Some(base)) = (ed.hunks.get(id), ed.base.as_ref()) else { return };
        let text = ed.state.read(cx).value().to_string();
        let (bytes, new) = git::lines_of(&text, &hunk.new);
        let old = git::lines_of(base, &hunk.old).1;
        self.file_peek = Some(FilePeek { old, new, bytes, at, can_revert: ed.saved.is_some() });
        self.file_peek_serial += 1;
        cx.notify();
    }

    /// Put git's lines back where the change showing is, as one step of
    /// undo, and put the card away.
    pub(crate) fn revert_file_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(peek) = self.file_peek.take() else { return };
        if let Some(ed) = self.file_editor.as_ref().filter(|ed| ed.saved.is_some()) {
            // Still the text the card was opened on.
            if ed.state.read(cx).value().get(peek.bytes.clone()) == Some(peek.new.as_str()) {
                ed.state.update(cx, |s, cx| {
                    s.replace_bytes(peek.bytes.clone(), &peek.old, peek.bytes.start, window, cx);
                    s.focus(window, cx);
                });
            }
        }
        cx.notify();
    }

    /// The card a mark in the margin opens, as VS Code's does in its
    /// own way: what git has there in red, what is there now in green,
    /// and Revert, which puts git's back. Over the file's pane, under
    /// the click or over it; a click off it or Escape puts it away.
    pub(crate) fn render_file_peek(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(peek) = self.file_peek.clone() else { return div().into_any_element() };
        let view = window.viewport_size();
        let pane = self.file_pane_bounds().unwrap_or(Bounds::new(point(px(0.), px(0.)), view));
        let card_w = (pane.size.width - px(24.)).min(px(620.)).max(px(220.));
        let left = (pane.left() + px(12.)).min(view.width - card_w - px(8.)).max(px(8.));
        let up = peek.at.y > view.height * 0.6;
        let (scroll, _) = self.kept_scroll(format!("file-peek-{}", self.file_peek_serial).into(), crate::workbench::Inner::Over);
        let (red, green) = (gpui::rgb(0xF85149), gpui::rgb(0x2EA043));
        let side = |text: &str, sign: &'static str, ink: gpui::Rgba| {
            let wash: Hsla = ink.into();
            v_flex().w_full().children(text.split_inclusive('\n').map(|line| {
                let line = line.trim_end_matches(['\n', '\r']);
                h_flex().w_full().items_start().bg(wash.opacity(0.13)).child(div().w(px(18.)).flex_shrink_0().text_center().text_color(wash).child(sign)).child(div().flex_1().min_w_0().child(if line.is_empty() { " ".to_string() } else { line.to_string() }))
            }))
        };
        let title = match (peek.old.is_empty(), peek.new.is_empty()) {
            (true, _) => "Added",
            (_, true) => "Removed",
            _ => "Changed",
        };
        let card = v_flex()
            .id(("file-peek", self.file_peek_serial as usize))
            .absolute()
            .left(left)
            .w(card_w)
            .map(|d| if up { d.bottom(view.height - peek.at.y + px(10.)) } else { d.top(peek.at.y + px(12.)) })
            .rounded(px(10.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow(float_shadow(&theme))
            .overflow_hidden()
            .on_click(|_, window, cx| swallow_click(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .w_full()
                    .px(px(10.))
                    .py(px(6.))
                    .gap(px(8.))
                    .items_center()
                    .border_b_1()
                    .border_color(theme.border)
                    .bg(theme.muted.opacity(0.4))
                    // A clip is a rectangle: the head's ground is rounded
                    // as the card is, the card's less its border.
                    .rounded_t(px(9.))
                    .child(div().flex_1().min_w_0().text_size(px(12.)).font_weight(FontWeight::MEDIUM).child(format!("{title}, against git's copy")))
                    .when(peek.can_revert, |d| {
                        d.child(crate::branches::small_pill("file-peek-revert", "icons/undo-2.svg", "Revert", Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.revert_file_peek(window, cx)
                        })))
                    })
                    .child(Button::new("file-peek-close").ghost().xsmall().icon(Icon::new(IconName::Close)).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.file_peek = None;
                        cx.notify();
                    }))),
            )
            // Room under the lines, as deep as the card's corner, so a
            // line's ground never stands square past its curve.
            .child(
                v_flex().relative().w_full().pb(px(9.)).child(
                    v_flex()
                        .id(("file-peek-lines", self.file_peek_serial as usize))
                        .w_full()
                        .max_h(px(280.))
                        .overflow_y_scroll()
                        .track_scroll(&scroll)
                        .font_family(theme.mono_font_family.clone())
                        .text_size(px(11.5))
                        .line_height(px(17.))
                        .when(!peek.old.is_empty(), |d| d.child(side(&peek.old, "−", red)))
                        .when(!peek.new.is_empty(), |d| d.child(side(&peek.new, "+", green))),
                )
                .vertical_scrollbar(&scroll),
            )
            .with_animation(ElementId::Name(format!("file-peek-in-{}", self.file_peek_serial).into()), Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()), move |d, t| d.opacity(t).map(|d| if up { d.mb(px(-5. * (1. - t))) } else { d.mt(px(-5. * (1. - t))) }));
        div()
            .id("file-peek-sheet")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.file_peek = None;
                    cx.notify();
                }),
            )
            .child(card)
            .into_any_element()
    }

    /// The marks in the editor's margin, as VS Code has them: a green
    /// bar beside lines git does not have, a blue one beside lines that
    /// differ, a red wedge where lines were taken out. They compare
    /// what the editor holds, saved or not, with git's copy of the file
    /// (`git::line_marks`), worked out off the main thread, one working
    /// at a time, again when the text or git's copy changes.
    fn sync_file_marks(&mut self, cx: &mut Context<Self>) {
        let Some(ed) = self.file_editor.as_mut().filter(|ed| ed.marks_due && !ed.marks_busy) else { return };
        ed.marks_due = false;
        let (state, base) = (ed.state.clone(), ed.base.clone());
        let Some(base) = base else {
            state.update(cx, |s, cx| s.set_gutter_marks(Vec::new(), cx));
            return;
        };
        ed.marks_busy = true;
        let text = state.read(cx).value().to_string();
        cx.spawn(async move |this, cx| {
            let hunks = cx.background_executor().spawn(async move { git::line_hunks(&base, &text) }).await;
            let _ = this.update(cx, |this, cx| {
                let marks = git::hunk_marks(&hunks)
                    .iter()
                    .map(|(id, m)| {
                        let color: Hsla = match m.kind {
                            git::LineChange::Added => gpui::rgb(0x2EA043),
                            git::LineChange::Modified => gpui::rgb(0x0078D4),
                            git::LineChange::Deleted => gpui::rgb(0xF85149),
                        }
                        .into();
                        gpui_component::input::GutterMark { id: *id, lines: m.line..m.line + m.lines, color, gone: m.kind == git::LineChange::Deleted }
                    })
                    .collect();
                state.update(cx, |s, cx| s.set_gutter_marks(marks, cx));
                for ed in this.file_editor.iter_mut().chain(this.file_parked.iter_mut()).filter(|ed| ed.state == state) {
                    ed.marks_busy = false;
                    ed.hunks = hunks.clone();
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Everything typed into a file and not saved, written to
    /// `state/file_drafts.json` on the clock when it differs from what
    /// was last written: the app is quit, or replaced by a new build,
    /// with no moment to ask, and what was typed is back at the next
    /// launch.
    pub(crate) fn keep_drafts(&mut self, cx: &mut Context<Self>) {
        let mut all = self.file_drafts.clone();
        for ed in self.file_editor.iter().chain(self.file_parked.iter()).filter(|ed| ed.dirty(cx)) {
            all.insert(ed.path.clone(), Draft { text: ed.state.read(cx).value().to_string(), base: ed.mtime });
        }
        if all == self.drafts_kept && all.is_empty() {
            return;
        }
        let on_disk = match emaki_core::paths::read_json(&drafts_path()) {
            Some(serde_json::Value::Array(rows)) => rows,
            _ => Vec::new(),
        };
        // Nothing new, and the file still holds them: another Emaki
        // writing the file (one from before rows had a writer writes
        // its own and nothing else) can leave them out.
        let own = Some(std::process::id() as u64);
        if all == self.drafts_kept && all.keys().all(|p| on_disk.iter().any(|row| row_key(row) == (own, Some(p.to_string_lossy().to_string())))) {
            return;
        }
        let rows = drafts_rows(&all, &on_disk, std::process::id(), DRAFTS_TAKEN.get().map(Vec::as_slice).unwrap_or_default());
        if emaki_core::paths::write_json(&drafts_path(), &serde_json::Value::Array(rows)).is_ok() {
            self.drafts_kept = all;
        }
    }

    /// What the file's pane wants an editor for, if anything: code, or
    /// markdown or a table as it is written.
    fn file_editor_want(&self) -> Option<EditorWant> {
        let v = self.file_view.as_ref()?;
        let (text, lang): (Rc<str>, &'static str) = match &v.body {
            FileBody::Code(text, lang, _) => (v.source.clone().unwrap_or_else(|| text.as_str().into()), *lang),
            FileBody::Markdown(text) if self.file_raw => (v.source.clone().unwrap_or_else(|| text.as_str().into()), "markdown"),
            FileBody::Html(text) if self.file_raw => (v.source.clone().unwrap_or_else(|| text.as_str().into()), "html"),
            FileBody::Notebook(_) if self.file_raw => (v.source.clone()?, "json"),
            // A table has its editor whichever way it shows: a cell
            // written in the grid is written in the editor's text, so
            // undo, the dot and saving are the editor's either way.
            FileBody::Table(..) => (v.source.clone()?, if v.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsv")) { "tsv" } else { "csv" }),
            FileBody::Picture(..) if self.file_raw => (v.source.clone()?, "html"),
            _ => return None,
        };
        Some(EditorWant { key: format!("{}|{:?}|{}", v.path.display(), v.mtime, text.len()), path: v.path.clone(), text, lang, saved: v.source.clone(), mtime: v.mtime })
    }

    /// Whether the file showing has changes that are not saved: in its
    /// editor, or kept from when it last showed.
    pub(crate) fn file_dirty(&self, cx: &App) -> bool {
        let Some(v) = &self.file_view else { return false };
        self.unsaved(cx).contains(&v.path)
    }

    /// Every file with changes not saved: the one in the editor, those
    /// whose editors are kept, and those typed in a run before this one.
    fn unsaved(&self, cx: &App) -> Vec<PathBuf> {
        let mut all: Vec<PathBuf> = self.file_editor.iter().chain(self.file_parked.iter()).filter(|ed| ed.dirty(cx)).map(|ed| ed.path.clone()).collect();
        all.extend(self.file_drafts.keys().filter(|p| !all.contains(p)).cloned().collect::<Vec<_>>());
        all.extend(self.word.as_ref().map(|w| w.read(cx)).filter(|w| w.dirty()).map(|w| w.path.clone()));
        all
    }

    /// Whether `then` may go ahead. With changes not saved in the way it
    /// may not, and the question is asked instead (`render_file_ask`):
    /// for closing a file or showing another, of the file showing; for
    /// closing the window or quitting, of every file there is, one at a
    /// time. This is what an editor does, and the reason is the same:
    /// nothing typed goes without the person saying so.
    pub(crate) fn file_guard(&mut self, then: FileThen, cx: &mut Context<Self>) -> bool {
        let unsaved = self.unsaved(cx);
        let path = match &then {
            FileThen::Close | FileThen::Open(_) => self.file_view.as_ref().map(|v| v.path.clone()).filter(|p| unsaved.contains(p)),
            FileThen::CloseWindow | FileThen::Quit => unsaved.into_iter().next(),
        };
        let Some(path) = path else { return true };
        self.file_ask = Some(FileAsk { path, then });
        self.file_ask_serial += 1;
        cx.notify();
        false
    }

    /// The question's answer, and then what was being done.
    pub(crate) fn file_answer(&mut self, answer: FileAnswer, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ask) = self.file_ask.take() else { return };
        window.focus(&self.focus_handle, cx);
        cx.notify();
        let showing = self.file_editor.as_ref().is_some_and(|ed| ed.path == ask.path);
        match answer {
            FileAnswer::Cancel => return,
            FileAnswer::Save if self.word.as_ref().is_some_and(|w| w.read(cx).path == ask.path) => {
                if !self.save_word(cx) {
                    return;
                }
            }
            FileAnswer::Save if showing => {
                self.save_file(window, cx);
                // Not saved after all (the file changed on disk, or could
                // not be written): the row says why, and nothing goes on.
                if self.file_editor.as_ref().is_some_and(|ed| ed.dirty(cx)) {
                    return;
                }
            }
            FileAnswer::Save => {
                // A file not showing: its kept editor's text, or what was
                // typed in an earlier run.
                let kept = self.file_parked.iter().find(|p| p.path == ask.path).map(|p| (p.state.read(cx).value().to_string(), p.lang));
                let lang = emaki_core::render_md::lang_for_path(&ask.path.to_string_lossy());
                let Some((text, lang)) = kept.or_else(|| self.file_drafts.get(&ask.path).map(|d| (d.text.clone(), lang))) else { return };
                let text = if self.cfg.app.format_on_save { emaki_core::format::format(lang, &ask.path, &text, &self.cfg.app.format_langs).ok().flatten().unwrap_or(text) } else { text };
                if let Err(e) = std::fs::write(&ask.path, &text) {
                    let name = ask.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    self.notice = Some(Notice::error(format!("could not save {name}: {e}")));
                    return;
                }
                self.file_parked.retain(|p| p.path != ask.path);
                self.file_drafts.remove(&ask.path);
            }
            FileAnswer::Discard if self.word.as_ref().is_some_and(|w| w.read(cx).path == ask.path) => self.word = None,
            FileAnswer::Discard => {
                // The editor goes back to what the file holds, as one
                // step, so even this can be undone while it shows.
                if let Some(ed) = self.file_editor.as_ref().filter(|_| showing) {
                    if let Some(saved) = ed.saved.clone() {
                        ed.state.update(cx, |s, cx| {
                            let len = s.value().len();
                            s.replace_bytes(0..len, &saved, 0, window, cx);
                        });
                    }
                }
                self.file_parked.retain(|p| p.path != ask.path);
                self.file_drafts.remove(&ask.path);
            }
        }
        match ask.then {
            FileThen::Close => self.close_file_view(cx),
            FileThen::Open(path) => self.file_preview(&path, window, cx),
            FileThen::CloseWindow => self.close_window(window, cx),
            FileThen::Quit => self.quit(cx),
        }
    }

    /// Quit, once no file has changes not saved.
    pub(crate) fn quit(&mut self, cx: &mut Context<Self>) {
        if self.file_guard(FileThen::Quit, cx) {
            self.keep_drafts(cx);
            // What the tree removed and still held is the trash's now.
            fs_flush(self.fs_journal.close());
            cx.quit();
        }
    }

    /// Close the window, once no file has changes not saved.
    pub(crate) fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_guard(FileThen::CloseWindow, cx) {
            self.keep_composers(cx);
            self.close_settings(cx);
            window.remove_window();
        }
    }

    /// The question over the window, as an editor asks it, in the
    /// window's own card: ↩ saves, Escape cancels.
    pub(crate) fn render_file_ask(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(ask) = self.file_ask.clone() else { return div().into_any_element() };
        // The keyboard is the card's while it is up, not the editor's
        // under it.
        if !self.file_ask_focus.is_focused(window) {
            window.focus(&self.file_ask_focus, cx);
        }
        let name = ask.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // Over the file's pane when the file is the one showing: the
        // question is that pane's. Else in the middle of the window.
        let over = self.file_pane_bounds().filter(|b| self.file_view.as_ref().is_some_and(|v| v.path == ask.path) && b.size.width > px(120.));
        let card_w = over.map(|b| (b.size.width - px(24.)).min(px(420.))).unwrap_or(px(420.));
        let answer = |answer: FileAnswer| {
            cx.listener(move |this, _: &ClickEvent, window, cx| {
                swallow_click(window, cx);
                this.file_answer(answer, window, cx);
            })
        };
        div()
            .id("file-ask-overlay")
            .track_focus(&self.file_ask_focus)
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .when(over.is_none(), |d| d.flex().flex_col().items_center().pt(px(120.)))
            .on_click(|_, window, cx| swallow_click(window, cx))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "enter" {
                    cx.stop_propagation();
                    this.file_answer(FileAnswer::Save, window, cx);
                }
            }))
            .child(
                v_flex()
                    .id("file-ask")
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(card_w)
                    .max_w(gpui::relative(0.94))
                    .map(|d| match over {
                        Some(b) => d.absolute().left(b.left() + (b.size.width - card_w) / 2.).top(b.top() + px(48.)),
                        None => d,
                    })
                    .p(px(16.))
                    .gap(px(10.))
                    .rounded(px(16.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .child(div().w_full().text_size(px(14.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).child(format!("Do you want to save the changes you made to {name}?")))
                    .child(div().w_full().text_size(px(12.5)).line_height(px(19.)).text_color(theme.muted_foreground).child("Your changes will be lost if you don't save them."))
                    .child(
                        h_flex()
                            .w_full()
                            .pt(px(2.))
                            .gap(px(8.))
                            .flex_wrap()
                            .child(choice_pill("file-ask-discard", "icons/trash.svg", "Don't Save", Press::Danger, false, &theme).on_click(answer(FileAnswer::Discard)))
                            .child(div().flex_1())
                            .child(choice_pill("file-ask-cancel", "icons/close.svg", "Cancel", Press::Quiet, false, &theme).on_click(answer(FileAnswer::Cancel)))
                            .child(choice_pill("file-ask-save", "icons/check.svg", "Save", Press::Primary, false, &theme).on_click(answer(FileAnswer::Save))),
                    )
                    .with_animation(ElementId::Name(format!("file-ask-in-{}", self.file_ask_serial).into()), Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t).mt(px(-6. * (1. - t)))),
            )
            .into_any_element()
    }

    /// ⌘S: what is in the file's editor goes to the file. A file that
    /// was written by someone else since it was read here (the agent,
    /// most often) is not written over at the first asking: the row
    /// under the composer says so, and a second ⌘S does it.
    ///
    /// A language with a formatter is put in its form first
    /// (`emaki_core::format`: R, by Air), in the editor as one step of
    /// undo and then on disk. A file its formatter cannot read is saved
    /// as it is, and the row says why it was not formatted.
    /// The Word document showing is written to its file. Whether it was.
    fn save_word(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(pane) = self.word.clone() else { return false };
        let (path, mtime) = (pane.read(cx).path.clone(), pane.read(cx).mtime);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let on_disk = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        if on_disk != mtime && self.file_conflict != Some(on_disk) {
            self.file_conflict = Some(on_disk);
            self.notice = Some(Notice::error(format!("{name} changed on disk since it was opened. Save again to write over it.")));
            cx.notify();
            return false;
        }
        self.file_conflict = None;
        let saved = pane.update(cx, |pane, cx| pane.save(cx));
        if let Err(why) = &saved {
            self.notice = Some(Notice::error(format!("could not save {name}: {why}")));
        }
        self.tree.read_at = 0.;
        cx.notify();
        saved.is_ok()
    }

    pub(crate) fn save_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.word.as_ref().is_some_and(|w| w.read(cx).dirty()) && self.file_view.as_ref().is_some_and(|v| matches!(v.body, FileBody::Word)) {
            self.save_word(cx);
            return;
        }
        let Some(ed) = self.file_editor.as_ref().filter(|ed| ed.dirty(cx)) else { return };
        let (path, mut text) = (ed.path.clone(), ed.state.read(cx).value().to_string());
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut unformatted = None;
        // Off in Settings, a file is saved as typed.
        match if self.cfg.app.format_on_save { emaki_core::format::format(ed.lang, &path, &text, &self.cfg.app.format_langs) } else { Ok(None) } {
            Ok(Some(formed)) => {
                ed.state.update(cx, |s, cx| {
                    let caret = s.cursor().min(formed.len());
                    s.replace_bytes(0..text.len(), &formed, caret, window, cx);
                });
                text = formed;
            }
            Ok(None) => {}
            Err(why) => unformatted = Some(why),
        }
        let on_disk = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
        if on_disk != ed.mtime && self.file_conflict != Some(on_disk) {
            self.file_conflict = Some(on_disk);
            self.notice = Some(Notice::error(format!("{name} changed on disk since it was opened. Save again to write over it.")));
            cx.notify();
            return;
        }
        // Written in place, so the file keeps its permissions.
        if let Err(e) = std::fs::write(&path, &text) {
            self.notice = Some(Notice::error(format!("could not save {name}: {e}")));
            cx.notify();
            return;
        }
        self.file_conflict = None;
        self.show_file(&path, cx);
        // The editor is the one for what the file now holds: it is not
        // made again, and the caret and the undo history stay.
        let want = self.file_editor_want();
        if let Some(ed) = self.file_editor.as_mut() {
            ed.saved = Some(text.as_str().into());
            ed.mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
            if let Some(want) = want.filter(|w| w.path == ed.path) {
                ed.key = want.key;
            }
        }
        self.tree.read_at = 0.;
        self.read_file_base(path.clone(), cx);
        if let Some(why) = unformatted {
            self.notice = Some(Notice::error(format!("{name} is saved but not formatted: {why}.")));
        }
        cx.notify();
    }

    /// The editor of the table showing, when it can be written in.
    fn table_editor(&self) -> Option<&FileEditor> {
        let v = self.file_view.as_ref().filter(|v| matches!(v.body, FileBody::Table(..)))?;
        self.file_editor.as_ref().filter(|ed| ed.path == v.path && ed.saved.is_some())
    }

    fn table_sep(path: &Path) -> char {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsv")) { '\t' } else { ',' }
    }

    /// The table reads as its editor has it.
    fn table_refresh(&mut self, cx: &mut Context<Self>) {
        let Some((path, text)) = self.table_editor().map(|ed| (ed.path.clone(), ed.state.read(cx).value().to_string())) else { return };
        if let Some(v) = self.file_view.as_mut() {
            v.body = edited_body(&path, &text);
        }
        for ed in self.file_editor.iter_mut() {
            ed.marks_due = true;
        }
        cx.notify();
    }

    /// The grid showing: a table's rows and widths, or those of the
    /// workbook's sheet that is chosen.
    fn grid_rows(&self) -> Option<(&Vec<Vec<String>>, &Vec<usize>)> {
        let v = self.file_view.as_ref()?;
        match &v.body {
            FileBody::Table(rows, widths, _) => Some((rows, widths)),
            FileBody::Sheets(sheets) => sheets.get(self.sheet_of(&v.path, sheets.len())).map(|(_, rows, widths, _)| (rows, widths)),
            _ => None,
        }
    }

    /// Which sheet of a workbook shows: the one chosen for that file,
    /// else the first.
    fn sheet_of(&self, path: &Path, sheets: usize) -> usize {
        self.sheet_at.as_ref().filter(|(p, _)| p == path).map(|(_, ix)| *ix).unwrap_or(0).min(sheets.saturating_sub(1))
    }

    /// The cell chosen in the table showing, kept inside what it has.
    fn table_cell(&self) -> Option<(usize, usize)> {
        let v = self.file_view.as_ref()?;
        let (rows, widths) = self.grid_rows()?;
        let at = self.table_at.as_ref().filter(|at| at.path == v.path)?;
        (at.row < rows.len() && at.col < widths.len()).then_some((at.row, at.col))
    }

    /// The block of cells chosen, from its top left to its bottom right,
    /// both inside what the table has: one cell, or the cells between
    /// the chosen one and where the choice was taken to.
    fn table_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let (row, col) = self.table_cell()?;
        let (rows, widths) = self.grid_rows()?;
        let (to_row, to_col) = self.table_at.as_ref().and_then(|at| at.to).map(|(r, c)| (r.min(rows.len() - 1), c.min(widths.len() - 1))).unwrap_or((row, col));
        Some(((row.min(to_row), col.min(to_col)), (row.max(to_row), col.max(to_col))))
    }

    /// The chosen cells as text: a tab between cells and a line a row,
    /// as a spreadsheet copies them.
    fn table_range_text(&self) -> Option<String> {
        let ((r0, c0), (r1, c1)) = self.table_range()?;
        let (rows, _) = self.grid_rows()?;
        Some(rows[r0..=r1].iter().map(|row| row[c0..=c1.min(row.len().saturating_sub(1))].join("\t")).collect::<Vec<_>>().join("\n"))
    }

    /// A cell is chosen: what was being written in another is kept first.
    pub(crate) fn table_select(&mut self, row: usize, col: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.table_commit(None, window, cx);
        let Some(path) = self.file_view.as_ref().map(|v| v.path.clone()) else { return };
        self.table_at = Some(TableAt { path, row, col, to: None, editing: false });
        window.focus(&self.file_focus, cx);
        self.table_reveal();
        cx.notify();
    }

    /// The choice is taken from the chosen cell to this one: a drag over
    /// the cells, or a ⇧ click.
    fn table_extend(&mut self, row: usize, col: usize, cx: &mut Context<Self>) {
        let Some(at) = self.table_at.as_mut().filter(|at| !at.editing) else { return };
        let to = ((row, col) != (at.row, at.col)).then_some((row, col));
        if at.to != to {
            at.to = to;
            cx.notify();
        }
    }

    /// Where in a cell's words a point across the pane falls, as a count
    /// of bytes: the words are measured as the cell's field sets them.
    fn table_caret(&self, col: usize, words: &str, x: Pixels, window: &Window, cx: &App) -> usize {
        let Some((_, widths)) = self.grid_rows() else { return words.len() };
        let before: f32 = widths[..col.min(widths.len())].iter().map(|w| table_col_w(*w)).sum();
        let left = self.file_view_scroll.bounds().left() + self.file_view_scroll.offset().x + px(before + 8.);
        let run = TextRun { len: words.len(), font: font(cx.theme().font_family.clone()), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
        window.text_system().shape_line(words.to_string().into(), px(12.), &[run], None).closest_index_for_x(x - left)
    }

    /// The cell the choice was last taken to, else the chosen one, is
    /// brought into view.
    fn table_reveal(&self) {
        let Some((row, col)) = self.table_cell() else { return };
        let Some((rows, widths)) = self.grid_rows() else { return };
        let (row, col) = self.table_at.as_ref().and_then(|at| at.to).map(|(r, c)| (r.min(rows.len() - 1), c.min(widths.len() - 1))).unwrap_or((row, col));
        let view = self.file_view_scroll.bounds().size;
        let mut off = self.file_view_scroll.offset();
        let x: f32 = widths[..col].iter().map(|w| table_col_w(*w)).sum();
        let (w, y) = (table_col_w(widths[col]), row as f32 * GRID_ROW);
        let (left, top) = (-f32::from(off.x), -f32::from(off.y));
        // The two heads stand outside the scroller, so its room is the
        // cells' own.
        let (room_w, room_h) = (f32::from(view.width), f32::from(view.height));
        if x < left {
            off.x = px(-x);
        } else if x + w > left + room_w {
            off.x = px(-(x + w - room_w).max(0.).min(x));
        }
        if y < top {
            off.y = px(-y);
        } else if y + GRID_ROW > top + room_h {
            off.y = px(-(y + GRID_ROW - room_h).max(0.));
        }
        self.file_view_scroll.set_offset(off);
    }

    /// The chosen cell is opened for writing, with what it holds, or
    /// with `typed` in its place when a letter opened it.
    pub(crate) fn table_edit(&mut self, typed: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((row, col)) = self.table_cell() else { return };
        let Some(ed) = self.table_editor() else {
            self.notice = Some(Notice::error("this table cannot be changed here: it is a workbook, or a file shown in part".to_string()));
            cx.notify();
            return;
        };
        let text = ed.state.read(cx).value().to_string();
        let held = match emaki_core::files::cell_at(&text, Self::table_sep(&ed.path), row, col) {
            Some(emaki_core::files::CellAt::At(span)) => emaki_core::files::cell_value(&text[span]),
            Some(emaki_core::files::CellAt::Short { .. }) => String::new(),
            None => return,
        };
        let value = typed.map(str::to_string).unwrap_or(held);
        self.table_input.update(cx, |s, cx| {
            s.set_value(value, window, cx);
            s.focus(window, cx);
        });
        if let Some(at) = self.table_at.as_mut() {
            at.editing = true;
            at.to = None;
        }
        cx.notify();
    }

    /// What was written in the cell goes into the file's text, in that
    /// cell's place and nowhere else: the rest of the file keeps its
    /// quotes and its line ends. Then, with `step`, the cell that many
    /// rows and columns on is chosen.
    pub(crate) fn table_commit(&mut self, step: Option<(isize, isize)>, window: &mut Window, cx: &mut Context<Self>) {
        let editing = self.table_at.as_ref().is_some_and(|at| at.editing);
        if editing {
            if let Some(at) = self.table_at.as_mut() {
                at.editing = false;
            }
            let value = self.table_input.read(cx).value().to_string();
            self.table_write(&value, window, cx);
            window.focus(&self.file_focus, cx);
        }
        if let Some((rows, cols)) = step {
            self.table_step(rows, cols, false, cx);
        }
        cx.notify();
    }

    /// Leave the cell as it was.
    pub(crate) fn table_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(at) = self.table_at.as_mut() {
            at.editing = false;
        }
        window.focus(&self.file_focus, cx);
        cx.notify();
    }

    /// Put `value` in the chosen cell, as one step of the editor's undo.
    fn table_write(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((row, col)) = self.table_cell() else { return };
        self.table_put(row, col, value, window, cx);
        self.table_refresh(cx);
    }

    /// Every chosen cell is emptied, the last first, so a cell's place
    /// in the text is still where it was read. A step of undo a cell.
    fn table_clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(((r0, c0), (r1, c1))) = self.table_range() else { return };
        for row in (r0..=r1).rev() {
            for col in (c0..=c1).rev() {
                self.table_put(row, col, "", window, cx);
            }
        }
        self.table_refresh(cx);
    }

    /// `value` into the cell at `row` and `col` of the file's text. The
    /// grid is not read again here (`table_refresh`).
    fn table_put(&mut self, row: usize, col: usize, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.table_editor() else { return };
        let (state, sep) = (ed.state.clone(), Self::table_sep(&ed.path));
        let text = state.read(cx).value().to_string();
        let (span, written) = match emaki_core::files::cell_at(&text, sep, row, col) {
            Some(emaki_core::files::CellAt::At(span)) => {
                let was = &text[span.clone()];
                if emaki_core::files::cell_value(was) == value {
                    return;
                }
                let written = emaki_core::files::cell_written(value, sep, was.starts_with('"'));
                (span, written)
            }
            // A row that ends before this column is made long enough.
            Some(emaki_core::files::CellAt::Short { end, cells }) => {
                if value.is_empty() {
                    return;
                }
                (end..end, format!("{}{}", sep.to_string().repeat(col + 1 - cells), emaki_core::files::cell_written(value, sep, false)))
            }
            None => return,
        };
        let caret = span.start + written.len();
        state.update(cx, |s, cx| s.replace_bytes(span, &written, caret, window, cx));
    }

    /// The choice moves by rows and columns, and stops at the edges.
    /// With `extend` (⇧ and an arrow) the chosen cell stays and the far
    /// corner of the block moves.
    fn table_step(&mut self, rows: isize, cols: isize, extend: bool, cx: &mut Context<Self>) {
        let Some((all, widths)) = self.grid_rows() else { return };
        let (n_rows, n_cols) = (all.len() as isize, widths.len() as isize);
        let step = |(row, col): (usize, usize)| ((row as isize + rows).clamp(0, (n_rows - 1).max(0)) as usize, (col as isize + cols).clamp(0, (n_cols - 1).max(0)) as usize);
        if let Some(at) = self.table_at.as_mut() {
            if extend {
                let to = step(at.to.unwrap_or((at.row, at.col)));
                at.to = (to != (at.row, at.col)).then_some(to);
            } else {
                (at.row, at.col) = step((at.row, at.col));
                at.to = None;
            }
        }
        self.table_reveal();
        cx.notify();
    }

    /// `EMAKI_GO=cell:<row>,<col>` chooses a cell (from 0),
    /// `cell:to:<row>,<col>` takes the choice to another,
    /// `cell:type:<words>` writes in it and goes down a row,
    /// `cell:open` opens it and leaves the field up, `cell:undo` is ⌘Z.
    pub(crate) fn table_probe(&mut self, step: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(words) = step.strip_prefix("type:") {
            self.table_edit(Some(words), window, cx);
            self.table_commit(Some((1, 0)), window, cx);
        } else if let Some((row, col)) = step.strip_prefix("to:").and_then(|to| to.split_once(',')).and_then(|(r, c)| Some((r.parse().ok()?, c.parse().ok()?))) {
            self.table_extend(row, col, cx);
            self.table_reveal();
        } else if step == "open" {
            self.table_edit(None, window, cx);
        } else if step == "undo" {
            if let Some(state) = self.table_editor().map(|ed| ed.state.clone()) {
                state.update(cx, |s, cx| s.undo_step(window, cx));
                self.table_refresh(cx);
            }
        } else if let Some((row, col)) = step.split_once(',').and_then(|(r, c)| Some((r.parse().ok()?, c.parse().ok()?))) {
            self.table_select(row, col, window, cx);
        }
    }

    /// The table's keys, with a cell chosen and none being written in:
    /// the arrows and Tab move, ⇧ and an arrow takes the choice further,
    /// ↩ opens the cell, Delete empties what is chosen, ⌘C copies it, a
    /// letter opens the cell with that letter in place of what it held,
    /// and undo and redo are the editor's.
    fn table_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_raw || !self.file_focus.is_focused(window) || self.table_cell().is_none() || self.table_at.as_ref().is_some_and(|at| at.editing) {
            return;
        }
        let m = e.keystroke.modifiers;
        match e.keystroke.key.as_str() {
            "up" => self.table_step(-1, 0, m.shift, cx),
            "down" => self.table_step(1, 0, m.shift, cx),
            "left" => self.table_step(0, -1, m.shift, cx),
            "right" => self.table_step(0, 1, m.shift, cx),
            "tab" => self.table_step(0, if m.shift { -1 } else { 1 }, false, cx),
            "enter" => self.table_edit(None, window, cx),
            "backspace" | "delete" if self.table_editor().is_some() => self.table_clear(window, cx),
            "z" if m.secondary() => {
                let Some(state) = self.table_editor().map(|ed| ed.state.clone()) else { return };
                state.update(cx, |s, cx| if m.shift { s.redo_step(window, cx) } else { s.undo_step(window, cx) });
                self.table_refresh(cx);
            }
            "c" if m.secondary() => {
                let Some(text) = self.table_range_text() else { return };
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            _ => match e.keystroke.key_char.as_deref().filter(|c| !m.secondary() && !m.control && !c.chars().any(char::is_control)) {
                Some(typed) => {
                    let typed = typed.to_string();
                    self.table_edit(Some(&typed), window, cx)
                }
                None => return,
            },
        }
        cx.stop_propagation();
    }

    /// A table's cells: lines between them and every other row a shade
    /// darker; the chosen cell outlined, and the block chosen with it
    /// washed. A press chooses a cell, a drag or a ⇧ press takes the
    /// choice to another, and a second press opens the cell, in a field
    /// that stands in the cell's place, with the caret where the press
    /// was. The heads are `render_table_heads`.
    fn render_table(&self, v: &FileView, rows: &[Vec<String>], widths: &[usize], more: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let at = self.table_at.as_ref().filter(|at| at.path == v.path);
        let chosen = at.map(|at| (at.row, at.col));
        let editing = at.is_some_and(|at| at.editing);
        let block = at.and_then(|_| self.table_range());
        let total: f32 = widths.iter().map(|w| table_col_w(*w)).sum();
        let (line, stripe) = (theme.border, theme.foreground.opacity(if theme.mode.is_dark() { 0.035 } else { 0.03 }));
        let field_focus = self.table_input.read(cx).focus_handle(cx);
        let body = v_flex().text_size(px(12.)).children(rows.iter().enumerate().map(|(ix, row)| {
            h_flex()
                .h(px(GRID_ROW))
                .flex_shrink_0()
                .border_b_1()
                .border_color(line)
                .when(ix % 2 == 1, |d| d.bg(stripe))
                .when(ix == 0, |d| d.font_weight(FontWeight::SEMIBOLD))
                .children(row.iter().zip(widths).enumerate().map(|(col, (cell, chars))| {
                    let here = chosen == Some((ix, col));
                    let within = block.is_some_and(|((r0, c0), (r1, c1))| (r0..=r1).contains(&ix) && (c0..=c1).contains(&col));
                    let base = div().relative().w(px(table_col_w(*chars))).h_full().flex_shrink_0().border_r_1().border_color(line).flex().items_center();
                    if here && editing {
                        return base
                            .child(
                                div()
                                    .id("table-field")
                                    .key_context(crate::workbench::FIND_CONTEXT)
                                    .on_action(cx.listener(|this, _: &crate::workbench::Escape, window, cx| this.table_cancel(window, cx)))
                                    .track_focus(&field_focus)
                                    .role(Role::TextInput)
                                    .aria_label("Cell")
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .bg(theme.background)
                                    .font_weight(FontWeight::NORMAL)
                                    .text_size(px(12.))
                                    .child(
                                        // A size of its own: the
                                        // toolkit's field then pads its
                                        // words as far as a cell pads
                                        // its, and sets them as large.
                                        gpui_component::input::Input::new(&self.table_input).with_size(gpui_component::Size::Size(px(12. / 0.875))).appearance(false).bordered(false).on_secondary_click(self.input_menu(&self.table_input, false, cx)),
                                    ),
                            )
                            // The outline is laid over the field, not
                            // set around it: as a border of the field
                            // it moved the words in from where the
                            // cell had them.
                            .child(div().absolute().inset_0().border_2().border_color(theme.primary))
                            .into_any_element();
                    }
                    base.px(px(8.))
                        .cursor_default()
                        .child(div().min_w_0().truncate().child(cell.clone()))
                        .when(within, |d| d.child(div().absolute().inset_0().bg(theme.primary.opacity(0.08))))
                        .when(here, |d| d.child(div().absolute().inset_0().border_2().border_color(theme.primary)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                if e.modifiers.shift && this.table_cell().is_some() {
                                    this.table_commit(None, window, cx);
                                    this.table_extend(ix, col, cx);
                                    return;
                                }
                                this.table_select(ix, col, window, cx);
                                this.table_drag = e.click_count < 2;
                                if e.click_count >= 2 {
                                    this.table_edit(None, window, cx);
                                    if this.table_at.as_ref().is_some_and(|at| at.editing) {
                                        // The caret goes where the press was.
                                        let words = this.table_input.read(cx).value().to_string();
                                        let caret = this.table_caret(col, &words, e.position.x, window, cx);
                                        this.table_input.update(cx, |s, cx| s.set_selected_range(caret..caret, cx));
                                        // The keyboard is the field's: the
                                        // pane takes it at every press in
                                        // it that is not said to be
                                        // someone's, and took it from the
                                        // field as it opened.
                                        window.prevent_default();
                                    }
                                }
                            }),
                        )
                        .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                            if this.table_drag && e.pressed_button == Some(MouseButton::Left) {
                                this.table_extend(ix, col, cx);
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                // A press on one of several chosen is
                                // about them all; elsewhere it chooses.
                                let several = this.table_at.as_ref().is_some_and(|at| at.to.is_some());
                                let inside = this.table_range().is_some_and(|((r0, c0), (r1, c1))| (r0..=r1).contains(&ix) && (c0..=c1).contains(&col));
                                if !(several && inside) {
                                    this.table_select(ix, col, window, cx);
                                }
                                let Some((rows, _)) = this.grid_rows() else { return };
                                let items = match this.table_range_text().filter(|_| several && inside) {
                                    Some(cells) => vec![("Copy Cells", MenuDo::Copy(cells))],
                                    None => vec![("Copy Cell", MenuDo::Copy(rows[ix][col].clone())), ("Copy Row", MenuDo::Copy(rows[ix].join("\t")))],
                                };
                                this.file_pane_menu(items, e.position, cx);
                            }),
                        )
                        .into_any_element()
                }))
        }));
        // The width is said outright: stretched across the scroller, the
        // grid was as wide as the pane and nothing scrolled sideways.
        v_flex()
            .w(px(total))
            .flex_shrink_0()
            .child(body)
            .when(more && v.rows_all.is_none(), |d| d.child(div().p(px(12.)).text_size(px(12.)).text_color(theme.muted_foreground).child(format!("Only the first {TABLE_ROWS} rows are shown. Open the file for the rest."))))
            // A file that says how many rows it has: that, whether or
            // not all of them are here. The first row is the names.
            .children(v.rows_all.map(|all| {
                let (rows, cols) = (plural(all, "row", "rows"), plural(widths.len(), "column", "columns"));
                div().p(px(12.)).text_size(px(12.)).text_color(theme.muted_foreground).child(if more { format!("{rows}, {cols}. The first {} are shown.", crate::format::thousands(TABLE_ROWS)) } else { format!("{rows}, {cols}.") })
            }))
            .into_any_element()
    }

    /// A grid's two heads: the columns' letters in a row over the cells
    /// and the rows' numbers in a column at their left, the corner
    /// between them in the first. Neither is in the scroller. Each is a
    /// strip of its own that cuts what it holds, and what it holds is
    /// moved along the strip by the scroll the cells have that way, so
    /// the letters cannot go up or down and the numbers cannot go
    /// sideways. Laid over the cells in the scroller and moved against
    /// both scrolls, they shook, and stood off their place by whatever
    /// the scroller put around its content.
    fn render_table_heads(&self, v: &FileView, rows: usize, widths: &[usize], cx: &App) -> (AnyElement, AnyElement) {
        let theme = cx.theme().clone();
        let off = self.file_view_scroll.offset();
        let block = self.table_at.as_ref().filter(|at| at.path == v.path).and_then(|_| self.table_range());
        let (line, head_bg, lit) = (theme.border, theme.sidebar, theme.primary.opacity(0.16));
        let head = |here: bool, d: Div| d.flex_shrink_0().flex().items_center().justify_center().border_color(line).text_color(if here { theme.foreground } else { theme.muted_foreground }).when(here, |d| d.bg(lit));
        let letters = h_flex().relative().left(off.x).h_full().children(widths.iter().enumerate().map(|(col, chars)| {
            let here = block.is_some_and(|((_, c0), (_, c1))| (c0..=c1).contains(&col));
            head(here, div().w(px(table_col_w(*chars))).h_full().border_r_1()).child(v.kinds.get(col).cloned().unwrap_or_else(|| column_name(col)))
        }));
        let numbers = v_flex().relative().top(off.y).children((0..rows).map(|ix| {
            let here = block.is_some_and(|((r0, _), (r1, _))| (r0..=r1).contains(&ix));
            head(here, div().w_full().h(px(GRID_ROW)).border_b_1()).child((ix + 1).to_string())
        }));
        let top = h_flex()
            .h(px(GRID_HEAD))
            .flex_shrink_0()
            .bg(head_bg)
            .border_b_1()
            .border_color(line)
            .text_size(px(11.))
            .child(div().w(px(GRID_NUM)).h_full().flex_shrink_0().border_r_1().border_color(line))
            .child(div().flex_1().min_w_0().h_full().overflow_hidden().child(letters))
            .into_any_element();
        let side = div().w(px(GRID_NUM)).h_full().flex_shrink_0().overflow_hidden().bg(head_bg).border_r_1().border_color(line).text_size(px(11.)).child(numbers).into_any_element();
        (top, side)
    }

    /// Markdown as it reads, or as it is written.
    fn set_file_raw(&mut self, raw: bool, cx: &mut Context<Self>) {
        if self.file_raw != raw {
            // As it reads, with changes not saved in its editor: it reads
            // as the editor has it, not as the disk does.
            if !raw {
                let edited = self.file_editor.as_ref().filter(|ed| ed.saved.is_some()).map(|ed| (ed.path.clone(), ed.state.read(cx).value().to_string()));
                if let (Some((path, text)), Some(v)) = (edited, self.file_view.as_mut()) {
                    if v.path == path {
                        v.body = edited_body(&path, &text);
                    }
                }
            }
            self.file_raw = raw;
            self.file_mode_serial += 1;
            self.file_serial += 1;
            self.file_view_scroll.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
    }

    /// Where the wheel is over the file's pane.
    pub(crate) fn file_pane_bounds(&self) -> Option<Bounds<Pixels>> {
        self.file_pane_shown().then(|| self.file_view_scroll.bounds())
    }

    fn file_menu(&mut self, node: Option<&Node>, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let item = |label: &'static str, what: FileDo, path: &Path| (label, MenuDo::File(what, path.to_path_buf()));
        let rule = || ("", MenuDo::Rule);
        // Paste is offered only with a file on the clipboard to paste.
        let paste = !self.clipboard_files(cx).is_empty();
        // In groups, a line between them: what opens it, what it gives
        // the message or the clipboard, and what changes it on disk.
        let mut items = match node {
            Some(n) if n.dir => vec![
                item(crate::sys::OPEN_FOLDER_LABEL, FileDo::Open, &n.path),
                rule(),
                item("New File", FileDo::NewFile, &n.path),
                item("New Folder", FileDo::NewFolder, &n.path),
                rule(),
                item("Add to Message", FileDo::Mention, &n.path),
                item("Copy Path", FileDo::CopyPath, &n.path),
                item("Copy Relative Path", FileDo::CopyRel, &n.path),
                rule(),
                item("Cut", FileDo::Cut, &n.path),
                item("Copy", FileDo::Copy, &n.path),
                item("Duplicate", FileDo::Duplicate, &n.path),
                item("Rename", FileDo::Rename, &n.path),
                item(crate::sys::TRASH_LABEL, FileDo::Trash, &n.path),
            ],
            Some(n) => {
                let mut items = Vec::new();
                if self.tree.changed.iter().any(|(p, _)| *p == n.path) {
                    items.push(item("Show Changes", FileDo::Changes, &n.path));
                }
                items.extend([
                    item("Open", FileDo::Open, &n.path),
                    item(crate::sys::REVEAL_LABEL, FileDo::Reveal, &n.path),
                    rule(),
                    item("Add to Message", FileDo::Mention, &n.path),
                    item("Copy Path", FileDo::CopyPath, &n.path),
                    item("Copy Relative Path", FileDo::CopyRel, &n.path),
                    rule(),
                    item("Cut", FileDo::Cut, &n.path),
                    item("Copy", FileDo::Copy, &n.path),
                    item("Duplicate", FileDo::Duplicate, &n.path),
                    item("Rename", FileDo::Rename, &n.path),
                    item(crate::sys::TRASH_LABEL, FileDo::Trash, &n.path),
                ]);
                items
            }
            None => vec![
                item(crate::sys::OPEN_FOLDER_LABEL, FileDo::Open, &root),
                rule(),
                item("New File", FileDo::NewFile, &root),
                item("New Folder", FileDo::NewFolder, &root),
                rule(),
                item("Collapse All Folders", FileDo::Collapse, &root),
                rule(),
                item("Copy Path", FileDo::CopyPath, &root),
            ],
        };
        if paste {
            let target = node.map(|n| n.path.clone()).unwrap_or(root);
            let after = items.iter().position(|(label, _)| *label == "Copy").map(|at| at + 1).unwrap_or(items.len());
            items.insert(after, item("Paste", FileDo::Paste, &target));
        }
        self.open_menu(at, items, cx);
    }

    /// A choice from the files panel's menu.
    /// The rectangle follows the pointer, and the rows it lies over are
    /// the ones chosen. A press that has not moved a few points is still
    /// a click. Every row is one height and the list starts at the
    /// scroller's top, so a row's place is its number.
    fn tree_band_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some((from, _)) = self.tree_band else { return };
        if !self.tree_band_live && (at.x - from.x).abs() < px(4.) && (at.y - from.y).abs() < px(4.) {
            return;
        }
        self.tree_band = Some((from, at));
        self.tree_band_live = true;
        let Some(root) = self.files_root() else { return };
        let top = self.files_scroll.bounds().top() + self.files_scroll.offset().y;
        let (first, last) = (from.y.min(at.y), from.y.max(at.y));
        self.tree.picked = None;
        self.tree.marked = self
            .tree
            .rows(&root)
            .into_iter()
            .enumerate()
            .filter_map(|(ix, row)| {
                let y = top + TREE_ROW_H * ix as f32;
                match row {
                    Row::Node(node, _, _) if y < last && y + TREE_ROW_H > first => Some(node.path),
                    _ => None,
                }
            })
            .collect();
        cx.notify();
    }

    fn tree_band_end(&mut self, cx: &mut Context<Self>) {
        if self.tree_band.take().is_some() {
            cx.notify();
        }
    }

    /// A right click on one of several rows chosen together: what can be
    /// asked of them all at once.
    fn files_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let paths = self.tree.marked.clone();
        let item = |label: &'static str, what: FileDo| (label, MenuDo::Files(what, paths.clone()));
        let items = vec![item("Add to Message", FileDo::Mention), item("Copy Paths", FileDo::CopyPath), item("Copy Relative Paths", FileDo::CopyRel), ("", MenuDo::Rule), item("Cut", FileDo::Cut), item("Duplicate", FileDo::Duplicate), item(crate::sys::TRASH_LABEL, FileDo::Trash)];
        self.open_menu(at, items, cx);
    }

    /// What `files_menu` offers, done to each: the paths go to the
    /// clipboard a line each, and the rest is `file_do` in turn.
    pub(crate) fn files_do(&mut self, what: FileDo, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        match what {
            FileDo::CopyPath | FileDo::CopyRel => {
                let lines: Vec<String> = paths.iter().map(|p| if what == FileDo::CopyRel { p.strip_prefix(&root).unwrap_or(p).to_string_lossy().to_string() } else { p.to_string_lossy().to_string() }).collect();
                cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));
                self.notice = Some(Notice::said("copied"));
            }
            FileDo::Mention => {
                for path in tops_of(&paths) {
                    self.file_do(what, path, window, cx);
                }
            }
            FileDo::Trash => self.tree_trash(paths, cx),
            FileDo::Cut => self.tree_cut(paths, cx),
            FileDo::Duplicate => self.tree_duplicate(paths, cx),
            _ => {}
        }
        cx.notify();
    }

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
            FileDo::Copy => self.tree_copy(&path, cx),
            FileDo::Cut => self.tree_cut(vec![path], cx),
            FileDo::Duplicate => self.tree_duplicate(vec![path], cx),
            FileDo::Paste => self.tree_paste(&path, cx),
            FileDo::Rename | FileDo::NewFile | FileDo::NewFolder => {
                let (prompt, value) = match what {
                    FileDo::Rename => (FilePrompt::Rename(path.clone()), path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                    FileDo::NewFile => (FilePrompt::NewFile(path), String::new()),
                    _ => (FilePrompt::NewFolder(path), String::new()),
                };
                // The folder a new one goes in is open, so its row is
                // where the name is typed.
                if let FilePrompt::NewFile(dir) | FilePrompt::NewFolder(dir) = &prompt {
                    self.tree.shut.remove(&root);
                    if *dir != root && self.tree.open.insert(dir.clone()) {
                        self.tree.kids.remove(dir);
                    }
                }
                // A rename begins with the name selected up to its
                // extension, as a file manager's does.
                let stem = match &prompt {
                    FilePrompt::Rename(path) if !path.is_dir() => Path::new(&value).file_stem().map(|s| s.len()).filter(|n| *n > 0).unwrap_or(value.len()),
                    _ => value.len(),
                };
                self.file_prompt = Some(prompt);
                self.tree_input.update(cx, |s, cx| {
                    s.set_value(value, window, cx);
                    s.focus(window, cx);
                    s.set_selected_range(0..stem, cx);
                });
            }
            FileDo::Collapse => self.tree_collapse(cx),
            FileDo::Trash => self.tree_trash(vec![path], cx),
        }
        cx.notify();
    }

    /// The files the clipboard holds: what the file manager or another
    /// app copied, else the one copied here when the clipboard still
    /// says so (where a file cannot be put on it as a file, its path is,
    /// and a copy made since of anything else is not pasted as that
    /// file).
    fn clipboard_files(&self, cx: &App) -> Vec<PathBuf> {
        let Some(item) = cx.read_from_clipboard() else { return Vec::new() };
        let files: Vec<PathBuf> = item
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                ClipboardEntry::ExternalPaths(paths) => Some(paths.paths().to_vec()),
                _ => None,
            })
            .flatten()
            .collect();
        if !files.is_empty() {
            return files;
        }
        let text = item.text().unwrap_or_default();
        self.file_clip.iter().filter(|clip| clip.to_string_lossy() == text.trim()).cloned().collect()
    }

    /// ⌘C on a row, or Copy on its menu: the file itself goes to the
    /// clipboard, to be pasted here or in the file manager.
    pub(crate) fn tree_copy(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !crate::sys::copy_file(path) {
            cx.write_to_clipboard(ClipboardItem::new_string(path.to_string_lossy().to_string()));
        }
        self.file_clip = Some(path.to_path_buf());
        self.notice = Some(Notice::said(format!("{} copied", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())));
        cx.notify();
    }

    /// ⌘V, or Paste: every file on the clipboard is copied into the
    /// folder `at` is, or the folder the file `at` is in, under a name
    /// that is free there. Nothing is written over.
    pub(crate) fn tree_paste(&mut self, at: &Path, cx: &mut Context<Self>) {
        // What was cut here is moved, while it is all still there.
        let cut = std::mem::take(&mut self.file_cut);
        if !cut.is_empty() && cut.iter().all(|p| p.exists()) {
            let Some(root) = self.files_root() else { return };
            let dir = if at.is_dir() { at.to_path_buf() } else { at.parent().map(Path::to_path_buf).unwrap_or(root) };
            return self.tree_move(cut, &dir, cx);
        }
        let files = self.clipboard_files(cx);
        if files.is_empty() {
            self.notice = Some(Notice::error("there is no file on the clipboard to paste".to_string()));
            cx.notify();
            return;
        }
        self.tree_put(&files, at, "pasted", cx);
    }

    /// Files copied into the folder `at` is, or the folder the file `at`
    /// is in, each under a name that is free there: what a paste and a
    /// drop from the file manager both do. `said` is the word for it.
    fn tree_put(&mut self, files: &[PathBuf], at: &Path, said: &str, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let dir = if at.is_dir() { at.to_path_buf() } else { at.parent().map(Path::to_path_buf).unwrap_or(root.clone()) };
        if !dir.starts_with(&root) {
            return;
        }
        let mut last = None;
        let mut made = Vec::new();
        for file in files {
            match emaki_core::files::copy_into(file, &dir) {
                Ok(to) => {
                    made.push(to.clone());
                    last = Some(to)
                }
                Err(why) => {
                    self.notice = Some(Notice::error(format!("{} was not {said}: {why}", file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())));
                    last = None;
                    break;
                }
            }
        }
        // Taken back, the copies go; done again, they come back.
        if !made.is_empty() {
            let moves = made.iter().map(|to| (self.fs_journal.slot(to), to.clone())).collect();
            self.fs_done(emaki_core::fsops::Kind::Copied, moves);
        }
        // The folder put into is opened, and the copy is the row chosen.
        self.tree.shut.remove(&root);
        if dir != root {
            self.tree.open.insert(dir.clone());
        }
        self.tree.refresh(&root);
        self.tree.read_at = 0.;
        if let Some(to) = last {
            self.notice = Some(Notice::said(format!("{said} as {}", to.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())));
            self.tree.picked = Some(to);
        }
        cx.notify();
    }

    /// Files dragged in from the file manager are over the tree at `at`:
    /// over `row` (its path, and whether it is a folder), or over the
    /// room around the rows. Where they would go is the folder under
    /// them, the folder of the file under them, or the session's folder,
    /// and is lit as it changes. A folder held over a moment opens, so
    /// what is in it can be reached, as a spring-loaded folder does.
    ///
    /// Every row and the panel hear every move, in no promised order, so
    /// the panel yields a move a row has already taken.
    fn tree_drop_over(&mut self, at: Point<Pixels>, row: Option<(&Path, bool)>, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        if row.is_none() && self.tree_drop_probe == Some((at, true)) {
            return;
        }
        self.tree_drop_probe = Some((at, row.is_some()));
        let dir = match row {
            Some((path, true)) => path.to_path_buf(),
            Some((path, false)) => path.parent().map(Path::to_path_buf).unwrap_or(root.clone()),
            None => root.clone(),
        };
        if self.tree_drop.as_ref().map(|(d, _)| d) == Some(&dir) {
            return;
        }
        self.tree_drop = Some((dir.clone(), std::time::Instant::now()));
        self.tree_drop_gone = None;
        let closed = row.is_some_and(|(_, folder)| folder) && !self.tree.open.contains(&dir);
        if closed {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TREE_SPRING).await;
                let _ = this.update(cx, |this, cx| {
                    // Still held over that folder, and still dragging.
                    if (cx.has_active_drag() || this.tree_drop_hold) && this.tree_drop.as_ref().is_some_and(|(d, at)| *d == dir && at.elapsed() >= TREE_SPRING) && this.tree.open.insert(dir.clone()) {
                        this.tree.kids.remove(&dir);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// The files are over the tree no longer.
    fn tree_drop_off(&mut self, cx: &mut Context<Self>) {
        self.tree_drop_probe = None;
        if let Some((dir, _)) = self.tree_drop.take() {
            self.tree_drop_gone = Some((self.tree_drop_words(&dir), std::time::Instant::now()));
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TREE_ANIM + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
            cx.notify();
        }
    }

    /// They are let go over the tree: copied into the folder lit, or,
    /// rows of the tree's own with no ⌥ held, moved there.
    fn tree_drop_done(&mut self, files: &[PathBuf], copy: bool, cx: &mut Context<Self>) {
        let Some(dir) = self.tree_drop.as_ref().map(|(dir, _)| dir.clone()).or_else(|| self.files_root()) else { return };
        self.tree_drop_off(cx);
        if copy {
            self.tree_put(files, &dir, "copied in", cx);
        } else {
            self.tree_move(files.to_vec(), &dir, cx);
        }
    }

    /// Whether what is dragged would be copied where it is let go: said
    /// again when ⌥ goes down or comes up on the way.
    fn tree_drop_kind(&mut self, copy: bool, cx: &mut Context<Self>) {
        if self.tree_drop_copy != copy {
            self.tree_drop_copy = copy;
            cx.notify();
        }
    }

    /// ⌘ and a click on a row, or ⇧ and one: the rows chosen together
    /// grow or shrink by it, as a file manager's list does.
    fn tree_click_more(&mut self, path: &Path, range: bool, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        // The one row chosen so far is the first of the several.
        if self.tree.marked.is_empty() {
            self.tree.marked.extend(self.tree.picked.clone());
        }
        if range {
            let rows: Vec<PathBuf> = self.tree.rows(&root).into_iter().filter_map(|row| match row {
                Row::Node(node, _, _) => Some(node.path),
                Row::More(..) | Row::Edit(..) => None,
            }).collect();
            let from = self.tree_anchor.clone().or(self.tree.marked.first().cloned()).and_then(|a| rows.iter().position(|r| *r == a)).unwrap_or(0);
            if let Some(to) = rows.iter().position(|r| r == path) {
                self.tree.marked = rows[from.min(to)..=from.max(to)].to_vec();
            }
        } else {
            if let Some(ix) = self.tree.marked.iter().position(|p| p == path) {
                self.tree.marked.remove(ix);
            } else {
                self.tree.marked.push(path.to_path_buf());
            }
            self.tree_anchor = Some(path.to_path_buf());
        }
        self.tree.picked = None;
        cx.notify();
    }

    /// Where a drop would go, in words: the folder as it is named under
    /// the session's.
    fn tree_drop_words(&self, dir: &Path) -> String {
        let root = self.files_root();
        let named = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let verb = if self.tree_drop_copy { "Copy" } else { "Move" };
        match root.as_ref().and_then(|r| dir.strip_prefix(r).ok()).filter(|rel| !rel.as_os_str().is_empty()) {
            Some(rel) => format!("{verb} into {}", rel.to_string_lossy().replace('\\', "/")),
            None => format!("{verb} into {}", root.as_deref().map(named).unwrap_or_default()),
        }
    }

    /// The key that renames the row chosen, as the system's file manager
    /// has it: ↩ on a Mac, F2 elsewhere. Whether it was that key.
    fn tree_rename_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let key = if cfg!(target_os = "macos") { "enter" } else { "f2" };
        // With the keyboard on the tree itself: the ↩ that keeps a name
        // comes by here too, from the field.
        if !self.tree_focus.is_focused(window) || self.file_prompt.is_some() || e.keystroke.key != key || e.keystroke.modifiers.modified() || self.tree.marked.len() > 1 {
            return false;
        }
        let Some(path) = self.tree.picked.clone().filter(|p| p.exists()) else { return false };
        cx.stop_propagation();
        self.file_do(FileDo::Rename, path, window, cx);
        true
    }

    /// New File and New Folder at the tree's head: in the folder chosen,
    /// the folder the chosen file is in, or the session's folder.
    fn tree_new(&mut self, folder: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let dir = match self.tree.picked.clone() {
            Some(p) if p.is_dir() => p,
            Some(p) => p.parent().map(Path::to_path_buf).unwrap_or(root.clone()),
            None => root.clone(),
        };
        let dir = if dir.starts_with(&root) { dir } else { root };
        self.file_do(if folder { FileDo::NewFolder } else { FileDo::NewFile }, dir, window, cx);
    }

    /// Every folder of the tree is closed.
    fn tree_collapse(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        self.tree.open.retain(|p| !p.starts_with(&root));
        cx.notify();
    }

    /// The name's field is put away and nothing is made.
    pub(crate) fn close_file_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_prompt.take().is_some() {
            window.focus(&self.tree_focus, cx);
        }
        cx.notify();
    }

    /// The keyboard has left the name's field: a name typed is kept, as
    /// an editor's explorer keeps it, and one that cannot be used is
    /// dropped with the reason said.
    pub(crate) fn file_prompt_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_prompt.is_none() {
            return;
        }
        self.commit_file_prompt(false, window, cx);
        if self.file_prompt.take().is_some() {
            cx.notify();
        }
    }

    /// The files panel's keys, as a file manager and VS Code's explorer
    /// have them. With ⌘: A every row, C copy, X cut, V paste (a cut is
    /// moved), D a copy beside it, Z take the last thing back, ⇧Z (and Y
    /// off a Mac) do it again, ⌫ remove. Alone: the arrows go from row to
    /// row and open and close folders, and Delete removes off a Mac.
    fn tree_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        // While a name is typed the keys are the field's.
        if self.file_prompt.is_some() {
            return;
        }
        let Some(root) = self.files_root() else { return };
        let (key, m) = (e.keystroke.key.as_str(), e.keystroke.modifiers);
        if !m.secondary() {
            if m.modified() {
                return;
            }
            match key {
                "up" => self.tree_step(-1, cx),
                "down" => self.tree_step(1, cx),
                "left" => self.tree_side(false, cx),
                "right" => self.tree_side(true, cx),
                "delete" if !cfg!(target_os = "macos") => {
                    let chosen = self.tree_chosen();
                    self.tree_trash(chosen, cx)
                }
                _ => return,
            }
            return cx.stop_propagation();
        }
        match key {
            // Every row showing, as a file manager's ⌘A has it.
            "a" => {
                self.tree.marked = self.tree.rows(&root).into_iter().filter_map(|row| match row {
                    Row::Node(node, _, _) => Some(node.path),
                    Row::More(..) | Row::Edit(..) => None,
                }).collect();
                self.tree.picked = None;
                cx.notify();
            }
            "c" => {
                let Some(path) = self.tree.picked.clone() else { return };
                self.tree_copy(&path, cx);
            }
            "x" => {
                let chosen = self.tree_chosen();
                self.tree_cut(chosen, cx)
            }
            "v" => {
                let at = self.tree.picked.clone().unwrap_or(root);
                self.tree_paste(&at, cx);
            }
            "d" => {
                let chosen = self.tree_chosen();
                self.tree_duplicate(chosen, cx)
            }
            "backspace" | "delete" => {
                let chosen = self.tree_chosen();
                self.tree_trash(chosen, cx)
            }
            "z" => self.tree_undo(m.shift, cx),
            "y" if !cfg!(target_os = "macos") => self.tree_undo(true, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The rows a key is about: the several chosen, or the one.
    fn tree_chosen(&self) -> Vec<PathBuf> {
        if self.tree.marked.is_empty() { self.tree.picked.iter().cloned().collect() } else { self.tree.marked.clone() }
    }

    /// ↑ and ↓: the row before or after the one chosen is, brought into
    /// view. With none chosen, the first or the last.
    fn tree_step(&mut self, by: i32, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let rows: Vec<PathBuf> = self.tree.rows(&root).into_iter().map(|row| match row {
            Row::Node(node, _, _) => node.path,
            Row::More(..) | Row::Edit(..) => PathBuf::new(),
        }).collect();
        let nodes: Vec<usize> = (0..rows.len()).filter(|ix| !rows[*ix].as_os_str().is_empty()).collect();
        let Some(&last) = nodes.last() else { return };
        let at = self.tree.picked.as_ref().or(self.tree.marked.last()).and_then(|p| rows.iter().position(|r| r == p));
        let to = match at {
            Some(at) if by < 0 => nodes.iter().rev().copied().find(|ix| *ix < at).unwrap_or(nodes[0]),
            Some(at) => nodes.iter().copied().find(|ix| *ix > at).unwrap_or(last),
            None if by < 0 => last,
            None => nodes[0],
        };
        self.tree.marked.clear();
        self.tree.picked = Some(rows[to].clone());
        self.tree_anchor = self.tree.picked.clone();
        // Into view, by its number: every row is one height.
        let (room, top) = (self.files_scroll.bounds().size.height, TREE_ROW_H * to as f32);
        let off = -self.files_scroll.offset().y;
        if top < off {
            self.files_scroll.set_offset(point(px(0.), -top));
        } else if room > px(0.) && top + TREE_ROW_H > off + room {
            self.files_scroll.set_offset(point(px(0.), -(top + TREE_ROW_H - room + px(6.))));
        }
        cx.notify();
    }

    /// → opens the folder chosen, or goes into one already open; ←
    /// closes it, or goes up to the folder the row is in.
    fn tree_side(&mut self, open: bool, cx: &mut Context<Self>) {
        let (Some(root), Some(path)) = (self.files_root(), self.tree.picked.clone()) else { return };
        let is_open = self.tree.open.contains(&path);
        match (open, path.is_dir(), is_open) {
            (true, true, false) | (false, true, true) => self.tree_toggle(&path, cx),
            (true, true, true) => self.tree_step(1, cx),
            (false, _, _) => {
                if let Some(up) = path.parent().filter(|up| *up != root && up.starts_with(&root)) {
                    self.tree.picked = Some(up.to_path_buf());
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------ done, and taken back

    /// A thing done to the disk is written down, and what can no longer
    /// be taken back or done again goes to the system's trash.
    fn fs_done(&mut self, kind: emaki_core::fsops::Kind, moves: Vec<(PathBuf, PathBuf)>) {
        let out = self.fs_journal.record(emaki_core::fsops::Op { kind, moves });
        fs_flush(out);
    }

    /// What was open under a path that is gone goes with it.
    fn tree_forget(&mut self, path: &Path) {
        if self.file_view.as_ref().is_some_and(|v| v.path.starts_with(path)) {
            self.file_view = None;
        }
        // What was typed in it and not saved goes with it.
        self.file_editor.take_if(|ed| ed.path.starts_with(path));
        self.file_parked.retain(|ed| !ed.path.starts_with(path));
        self.file_drafts.retain(|p, _| !p.starts_with(path));
        if self.tree.picked.as_ref().is_some_and(|p| p.starts_with(path)) {
            self.tree.picked = None;
        }
        self.tree.marked.retain(|p| !p.starts_with(path));
        self.file_cut.retain(|p| !p.starts_with(path));
    }

    /// What was at `from` is at `to`: the folders open under it, the
    /// file showing, and what was typed in any file under it and not
    /// saved, are under the new path.
    fn tree_follow(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        let now = |p: &Path| p.strip_prefix(from).ok().map(|rest| if rest.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rest) });
        let open: Vec<PathBuf> = self.tree.open.iter().filter(|p| p.starts_with(from)).cloned().collect();
        for p in open {
            self.tree.open.remove(&p);
            if let Some(new) = now(&p) {
                self.tree.open.insert(new);
            }
        }
        let shown = self.file_view.as_ref().and_then(|v| now(&v.path));
        for ed in self.file_editor.iter_mut().chain(self.file_parked.iter_mut()) {
            if let Some(new) = now(&ed.path) {
                ed.path = new;
            }
        }
        let drafts: Vec<PathBuf> = self.file_drafts.keys().filter(|p| p.starts_with(from)).cloned().collect();
        for old in drafts {
            if let (Some(draft), Some(new)) = (self.file_drafts.remove(&old), now(&old)) {
                self.file_drafts.insert(new, draft);
            }
        }
        for p in self.tree.picked.iter_mut().chain(self.tree.marked.iter_mut()).chain(self.file_cut.iter_mut()) {
            if let Some(new) = now(p) {
                *p = new;
            }
        }
        if let Some(new) = shown {
            // Shown again under its name; its editor is kept and found
            // by the path it now has.
            if let Some(ed) = self.file_editor.take() {
                self.file_parked.push(ed);
            }
            self.show_file(&new, cx);
        }
    }

    /// Remove: the rows go out of the tree and are held, so ⌘Z puts
    /// them back exactly. They reach the system's trash once that key
    /// can no longer bring them back (`fsops`). One on another volume
    /// than the holding folder cannot be held, and goes to the trash at
    /// once, which is said.
    pub(crate) fn tree_trash(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let tops: Vec<PathBuf> = tops_of(&paths).into_iter().filter(|p| *p != root && p.starts_with(&root)).collect();
        if tops.is_empty() {
            return;
        }
        let (mut moves, mut straight, mut failed) = (Vec::new(), 0, None);
        for path in &tops {
            let slot = self.fs_journal.slot(path);
            match emaki_core::fsops::move_path(path, &slot) {
                Ok(()) => moves.push((path.clone(), slot)),
                Err(_) => match crate::sys::trash_path(path) {
                    Ok(()) => straight += 1,
                    Err(e) => {
                        failed = Some(e);
                        continue;
                    }
                },
            }
            self.tree_forget(path);
        }
        let held = moves.len();
        if !moves.is_empty() {
            self.fs_done(emaki_core::fsops::Kind::Removed, moves);
        }
        self.tree.marked.clear();
        self.tree.refresh(&root);
        self.tree.read_at = 0.;
        let what = match tops.as_slice() {
            [one] => path_name(one),
            many => format!("{} items", many.len()),
        };
        self.notice = Some(match failed {
            Some(e) => Notice::error(format!("not removed: {e}")),
            None if held == 0 && straight > 0 => Notice::said(format!("{what} moved to the {}", crate::sys::TRASH_NAME)),
            None => Notice::said(format!("{what} removed. {} puts it back", if cfg!(target_os = "macos") { "⌘Z" } else { "Ctrl+Z" })),
        });
        cx.notify();
    }

    /// Move rows into a folder, as a drag or a cut and a paste does. One
    /// already in that folder stays; a folder is not moved into itself;
    /// a name taken there is not written over. What did move is one
    /// thing to take back.
    pub(crate) fn tree_move(&mut self, paths: Vec<PathBuf>, dir: &Path, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        if !dir.starts_with(&root) || !dir.is_dir() {
            return;
        }
        let (mut moves, mut refused) = (Vec::new(), None);
        for from in tops_of(&paths) {
            let to = dir.join(from.file_name().unwrap_or_default());
            if from.parent() == Some(dir) {
                continue;
            }
            if dir.starts_with(&from) {
                refused = Some(format!("{} cannot be moved into itself", path_name(&from)));
                continue;
            }
            match emaki_core::fsops::move_path(&from, &to) {
                Ok(()) => {
                    self.tree_follow(&from, &to, cx);
                    moves.push((from, to));
                }
                Err(why) => refused = Some(why),
            }
        }
        let moved: Vec<PathBuf> = moves.iter().map(|(_, to)| to.clone()).collect();
        if !moves.is_empty() {
            self.fs_done(emaki_core::fsops::Kind::Moved, moves);
            // The folder moved into is opened, and what moved is chosen.
            self.tree.shut.remove(&root);
            if dir != root {
                self.tree.open.insert(dir.to_path_buf());
                self.tree.kids.remove(dir);
            }
            self.tree.picked = moved.last().cloned();
            self.tree.marked = if moved.len() > 1 { moved.clone() } else { Vec::new() };
        }
        self.tree.refresh(&root);
        self.tree.read_at = 0.;
        self.notice = match (refused, moved.as_slice()) {
            (Some(why), _) => Some(Notice::error(why)),
            (None, []) => None,
            (None, [one]) => Some(Notice::said(format!("{} moved", path_name(one)))),
            (None, many) => Some(Notice::said(format!("{} items moved", many.len()))),
        };
        cx.notify();
    }

    /// ⌘X: the rows are the ones the next paste moves, drawn faint
    /// until then. Nothing on the disk changes yet.
    pub(crate) fn tree_cut(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        self.file_cut = tops_of(&paths).into_iter().filter(|p| *p != root).collect();
        if !self.file_cut.is_empty() {
            self.notice = Some(Notice::said("cut. Paste moves it".to_string()));
        }
        cx.notify();
    }

    /// ⌘D: a copy of each beside it, under a free name.
    pub(crate) fn tree_duplicate(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in tops_of(&paths) {
            if let Some(dir) = path.parent().map(Path::to_path_buf) {
                self.tree_put(&[path], &dir, "copied", cx);
            }
        }
    }

    /// ⌘Z and ⌘⇧Z: the last thing done in the tree is taken back, or the
    /// last taken back is done again, and the window follows the files.
    pub(crate) fn tree_undo(&mut self, redo: bool, cx: &mut Context<Self>) {
        use emaki_core::fsops::Kind;
        let Some(root) = self.files_root() else { return };
        let hold = fs_hold();
        let ran = if redo { self.fs_journal.redo() } else { self.fs_journal.undo() };
        let op = match ran {
            None => {
                self.notice = Some(Notice::said(if redo { "nothing to do again" } else { "nothing to take back" }.to_string()));
                return cx.notify();
            }
            Some(Err((why, held))) => {
                fs_flush(held);
                self.notice = Some(Notice::error(format!("not {}: {why}", if redo { "done again" } else { "taken back" })));
                return cx.notify();
            }
            Some(Ok(op)) => op,
        };
        // Each move as it just ran: from where the thing was to where it is.
        let mut here = None;
        for (before, after) in &op.moves {
            let (from, to) = if redo { (before, after) } else { (after, before) };
            match (from.starts_with(&hold), to.starts_with(&hold)) {
                (false, true) => self.tree_forget(from),
                (true, false) => here = Some(to.clone()),
                _ => {
                    self.tree_follow(from, to, cx);
                    here = Some(to.clone());
                }
            }
        }
        // What came back, or moved, is seen and chosen.
        if let Some(at) = &here {
            self.tree.shut.remove(&root);
            let mut up = at.parent();
            while let Some(dir) = up.filter(|dir| *dir != root && dir.starts_with(&root)) {
                if self.tree.open.insert(dir.to_path_buf()) {
                    self.tree.kids.remove(dir);
                }
                up = dir.parent();
            }
            self.tree.marked.clear();
            self.tree.picked = Some(at.clone());
        }
        self.tree.refresh(&root);
        self.tree.read_at = 0.;
        let n = op.moves.len();
        let named = |ix: usize| op.moves.first().map(|(a, b)| path_name(if ix == 0 { a } else { b })).unwrap_or_default();
        let what = match (op.kind, n) {
            (Kind::Renamed, _) => format!("renaming {} to {}", named(0), named(1)),
            (kind, 1) => format!("{} {}", kind_word(kind), named(1 - (kind == Kind::Removed) as usize)),
            (kind, n) => format!("{} {n} items", kind_word(kind)),
        };
        self.notice = Some(Notice::said(format!("{}: {what}", if redo { "done again" } else { "taken back" })));
        cx.notify();
    }

    /// A question on the window's own card over everything: a line, a
    /// line under it, and its two buttons.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ask_card(&self, id: &'static str, serial: u32, title: String, detail: String, no: impl IntoElement, yes: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        div()
            .id(SharedString::from(format!("{id}-overlay")))
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .flex_col()
            .items_center()
            .pt(px(120.))
            .on_click(|_, window, cx| swallow_click(window, cx))
            .child(
                v_flex()
                    .id(id)
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(px(420.))
                    .max_w(gpui::relative(0.94))
                    .p(px(16.))
                    .gap(px(10.))
                    .rounded(px(16.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .child(div().w_full().text_size(px(14.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().w_full().text_size(px(12.5)).line_height(px(19.)).text_color(theme.muted_foreground).child(detail))
                    .child(h_flex().w_full().pt(px(2.)).gap(px(8.)).child(div().flex_1()).child(no).child(yes))
                    .with_animation(ElementId::Name(format!("{id}-in-{serial}").into()), Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t).mt(px(-6. * (1. - t)))),
            )
            .into_any_element()
    }

    /// Discard, pressed in the comparison: the question first.
    pub(crate) fn ask_discard(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let serial = self.discard_ask.as_ref().map(|(_, n)| n + 1).unwrap_or(0);
        self.discard_ask = Some((paths, serial));
        cx.notify();
    }

    /// The question answered yes: the file is as the last commit has it
    /// (`git::discard`), or, when that commit has no such file, in the
    /// trash. What was typed in it here and not saved goes too. The
    /// comparison moves to the next changed file, or closes with none.
    fn discard_now(&mut self, cx: &mut Context<Self>) {
        let Some((paths, _)) = self.discard_ask.take() else { return };
        let Some(root) = self.changes.as_ref().map(|c| c.root.clone()) else { return };
        let named = |path: &Path| path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (mut said, mut failed, mut done) = (Vec::new(), Vec::new(), 0);
        for path in &paths {
            let name = named(path);
            let did = git::discard(&root, path).and_then(|did| match did {
                git::Discarded::New if path.exists() => crate::sys::trash_path(path).map(|_| format!("{name} is in the {}", crate::sys::TRASH_NAME)),
                _ => Ok(format!("the changes to {name} are discarded")),
            });
            match did {
                Ok(words) => {
                    done += 1;
                    said.push(words);
                    self.file_editor.take_if(|ed| ed.path == *path);
                    self.file_parked.retain(|ed| ed.path != *path);
                    self.file_drafts.remove(path);
                    if self.file_view.as_ref().is_some_and(|v| v.path == *path) {
                        if path.exists() { self.show_file(path, cx) } else { self.file_view = None }
                    }
                }
                Err(why) => failed.push(format!("{name} was not discarded: {why}")),
            }
        }
        if done > 0 {
            // Git is asked again here and now, so the list is right
            // at the next draw.
            let status = git::status(&root).unwrap_or_default();
            self.tree.changed = Rc::new(status.changed());
            self.tree.git = Some((root.clone(), Rc::new(status)));
            self.tree.refresh(&root);
            self.tree.read_at = 0.;
            if self.tree.changed.is_empty() {
                self.changes = None;
            } else {
                self.open_changes(None, cx);
            }
        }
        self.notice = Some(match (failed.first(), done) {
            (Some(why), _) if failed.len() == 1 => Notice::error(why.clone()),
            (Some(_), _) => Notice::error(format!("{} were not discarded", plural(failed.len(), "file", "files"))),
            (None, 1) => Notice::said(said.remove(0)),
            (None, n) => Notice::said(format!("the changes to {} are discarded", plural(n, "file", "files"))),
        });
        cx.notify();
    }

    /// The question Discard asks, on the window's own card over the
    /// comparison. Nothing answers it but its two buttons and Escape.
    pub(crate) fn render_discard_ask(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some((paths, serial)) = self.discard_ask.clone() else { return div().into_any_element() };
        let name = paths.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let new = paths.iter().filter(|path| self.tree.changed.iter().any(|(p, s)| p == *path && matches!(s, git::State::Untracked | git::State::Added))).count();
        let (title, detail, yes) = if paths.len() > 1 {
            let detail = match new {
                0 => "Each file goes back to what the last commit has. This cannot be undone from here.".to_string(),
                n if n == paths.len() => format!("The last commit has none of them, so discarding them moves them to the {}. They can be put back from there.", crate::sys::TRASH_NAME),
                n => format!("Each goes back to what the last commit has, which cannot be undone from here. {} the last commit does not have {} to the {}.", plural(n, "file", "files"), if n == 1 { "goes" } else { "go" }, crate::sys::TRASH_NAME),
            };
            (format!("Discard the changes to {} files?", paths.len()), detail, "Discard Changes")
        } else if new == 1 {
            (format!("Move {name} to the {}?", crate::sys::TRASH_NAME), "The last commit has no such file, so discarding it removes it. It can be put back from there.".to_string(), crate::sys::TRASH_LABEL)
        } else {
            (format!("Discard the changes to {name}?"), "The file goes back to what the last commit has. This cannot be undone from here.".to_string(), "Discard Changes")
        };
        let cancel = cx.listener(|this, _: &ClickEvent, window, cx| {
            swallow_click(window, cx);
            this.discard_ask = None;
            cx.notify();
        });
        let go = cx.listener(|this, _: &ClickEvent, window, cx| {
            swallow_click(window, cx);
            this.discard_now(cx);
        });
        let no = choice_pill("discard-ask-cancel", "icons/close.svg", "Cancel", Press::Quiet, false, cx.theme()).on_click(cancel);
        let yes = choice_pill("discard-ask-yes", if new > 0 && new == paths.len() { "icons/trash.svg" } else { "icons/undo-2.svg" }, yes, Press::Danger, false, cx.theme()).on_click(go);
        self.ask_card("discard-ask", serial, title, detail, no, yes, cx)
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

    /// ↩ in the tree's field: the rename, or the new file or folder, is
    /// made and its row chosen; a name that cannot be used says why and
    /// leaves the field up.
    ///
    /// `refocus` is whether the keyboard goes back to the tree: it does
    /// after ↩, and not when the field was left for somewhere else.
    pub(crate) fn commit_file_prompt(&mut self, refocus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.file_prompt.clone() else { return };
        let name = self.tree_input.read(cx).value().trim().to_string();
        let done = |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            if refocus {
                this.close_file_prompt(window, cx);
            } else {
                this.file_prompt = None;
                cx.notify();
            }
        };
        if name.is_empty() {
            return done(self, window, cx);
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
                        return Ok(Some(to));
                    }
                    if to.exists() {
                        return Err(format!("{name} is already there"));
                    }
                    emaki_core::fsops::move_path(from, &to)?;
                    // What was open under the old name is open under the
                    // new, and the rename is one thing to take back.
                    self.tree_follow(from, &to, cx);
                    self.fs_done(emaki_core::fsops::Kind::Renamed, vec![(from.clone(), to.clone())]);
                    Ok(Some(to))
                }
                FilePrompt::NewFile(dir) | FilePrompt::NewFolder(dir) => {
                    let to = dir.join(&name);
                    if to.exists() {
                        return Err(format!("{name} is already there"));
                    }
                    let file = matches!(prompt, FilePrompt::NewFile(_));
                    let folder = if file { to.parent().unwrap_or(dir).to_path_buf() } else { to.clone() };
                    // What is made is everything from the first folder
                    // that was not there: taken back, all of it goes.
                    let top = to.ancestors().take_while(|p| p.starts_with(dir) && *p != dir.as_path()).filter(|p| !p.exists()).last().map(Path::to_path_buf).unwrap_or(to.clone());
                    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                    if file {
                        std::fs::OpenOptions::new().write(true).create_new(true).open(&to).map_err(|e| e.to_string())?;
                    }
                    let slot = self.fs_journal.slot(&top);
                    self.fs_done(emaki_core::fsops::Kind::Made, vec![(slot, top)]);
                    // Every folder down to it is opened, so it is seen.
                    let mut up = to.parent();
                    while let Some(p) = up.filter(|p| p.starts_with(dir)) {
                        self.tree.open.insert(p.to_path_buf());
                        up = p.parent();
                    }
                    Ok(Some(to))
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
                done(self, window, cx);
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
        let mut rows = match &root {
            Some(r) if exists => self.tree.rows(r),
            _ => Vec::new(),
        };
        let dark = theme.mode.is_dark();
        // A name being typed has a row of its own where the new file or
        // folder will stand: a folder first in its folder, a file after
        // the folders there, as the list is sorted.
        let prompt = self.file_prompt.clone();
        if let (Some(r), Some(FilePrompt::NewFile(dir) | FilePrompt::NewFolder(dir))) = (&root, &prompt) {
            let folder = matches!(prompt, Some(FilePrompt::NewFolder(_)));
            let at = if dir == r {
                Some((0, 0))
            } else {
                rows.iter().enumerate().find_map(|(ix, row)| match row {
                    Row::Node(n, depth, _) if n.path == *dir => Some((ix + 1, depth + 1)),
                    _ => None,
                })
            };
            if let Some((mut ix, depth)) = at {
                while !folder {
                    match rows.get(ix) {
                        Some(Row::Node(n, d, _)) if *d > depth || (*d == depth && n.dir) => ix += 1,
                        Some(Row::More(_, d)) if *d > depth => ix += 1,
                        _ => break,
                    }
                }
                rows.insert(ix, Row::Edit(depth, folder));
            }
        }
        // The field itself, made once: one row has it. Its icon is the
        // one the name typed so far would get.
        let typed = self.tree_input.read(cx).value().to_string();
        let mut field = prompt.is_some().then(|| self.name_field("tree-field", &self.tree_input, 12.5, "Name", |this, window, cx| this.close_file_prompt(window, cx), cx).into_any_element());
        let pad = |depth: usize| px(8. + 12. * (depth + 1) as f32);
        let chevron = |open: bool| Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(10.)).text_color(theme.muted_foreground);
        // The row a name is typed in: the row's own shape, with the
        // field where the name stands. Its clicks are its own.
        let edit_row = |depth: usize, dir: bool, open: bool, field: AnyElement| {
            h_flex()
                .id("tree-edit")
                .h(TREE_ROW_H)
                .flex_shrink_0()
                .pl(pad(depth))
                .pr(px(8.))
                .gap(px(5.))
                .items_center()
                .on_click(|_, window, cx| swallow_click(window, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .child(div().w(px(10.)).flex_shrink_0().flex().justify_center().when(dir, |d| d.child(chevron(open))))
                .child(img(crate::file_icons::path(typed.trim(), dir, open, dark)).size(px(16.)).flex_shrink_0())
                .child(field)
        };
        // Git's word on the folder; asked for at once when there is none
        // for it yet, so the colours do not wait for the clock.
        let status = self.tree.git.as_ref().filter(|(of, _)| Some(of) == root.as_ref()).map(|(_, st)| st.clone());
        if status.is_none() && exists {
            if let Some(r) = root.clone() {
                self.read_git(r, cx);
            }
        }
        let picked = self.tree.picked.clone();
        let marked = self.tree.marked.clone();
        // Where files dragged in from the file manager would go. A drag
        // that left the window says nothing as it goes, so what it lit
        // is put out here.
        if self.tree_drop.is_some() && !cx.has_active_drag() && !self.tree_drop_hold {
            self.tree_drop_off(cx);
        }
        let drop_dir = self.tree_drop.as_ref().map(|(dir, _)| dir.clone());
        let drop_root = drop_dir.is_some() && drop_dir == root;
        // What came and what went since the last draw, each in motion.
        if let Some(r) = root.as_ref().filter(|_| exists) {
            if self.tree.seen_now(r, &rows) {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(TREE_ANIM + Duration::from_millis(20)).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        }
        let born: HashSet<PathBuf> = self.tree.born.keys().cloned().collect();
        let mut ghosts: HashMap<Option<PathBuf>, Vec<Seen>> = HashMap::new();
        for ghost in &self.tree.gone {
            ghosts.entry(ghost.after.clone()).or_default().push(ghost.row.clone());
        }
        // The rows gone from under `key`, each as it was, shrinking.
        let gone_under = |ghosts: &mut HashMap<Option<PathBuf>, Vec<Seen>>, key: Option<&PathBuf>| -> Vec<AnyElement> {
            ghosts
                .remove(&key.cloned())
                .unwrap_or_default()
                .into_iter()
                .map(|(path, node, depth, open)| {
                    let row = h_flex().h(TREE_ROW_H).flex_shrink_0().pl(pad(depth)).pr(px(8.)).gap(px(5.)).items_center().children(node.map(|node| {
                        let tint = status.as_ref().and_then(|st| st.mark(&node.path, node.dir)).map(|m| git_color(m.state, dark));
                        [
                            div().w(px(10.)).flex_shrink_0().flex().justify_center().when(node.dir, |d| d.child(chevron(open))).into_any_element(),
                            img(crate::file_icons::path(&node.name, node.dir, open, dark)).size(px(16.)).flex_shrink_0().into_any_element(),
                            div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(tint.unwrap_or(theme.foreground)).child(node.name.clone()).into_any_element(),
                        ]
                    }).into_iter().flatten());
                    div()
                        .overflow_hidden()
                        .flex_shrink_0()
                        .child(row)
                        .with_animation(ElementId::Name(format!("tree-out-{}", path.display()).into()), Animation::new(TREE_ANIM).with_easing(ease_out_quint()), |d, t| d.h(TREE_ROW_H * (1. - t)).opacity(1. - t))
                        .into_any_element()
                })
                .collect()
        };
        // A row goes in the list: growing when it has just come, and
        // with what has just gone from under it after it.
        let put = |list: Div, ghosts: &mut HashMap<Option<PathBuf>, Vec<Seen>>, key: Option<PathBuf>, row: AnyElement| -> Div {
            let row = match &key {
                Some(path) if born.contains(path) => div()
                    .overflow_hidden()
                    .flex_shrink_0()
                    .child(row)
                    .with_animation(ElementId::Name(format!("tree-in-{}", path.display()).into()), Animation::new(TREE_ANIM).with_easing(ease_out_quint()), |d, t| d.h(TREE_ROW_H * t).opacity(t))
                    .into_any_element(),
                _ => row,
            };
            let list = list.child(row);
            match &key {
                Some(path) => list.children(gone_under(ghosts, Some(path))),
                None => list,
            }
        };
        let mut list = v_flex().w_full().children(gone_under(&mut ghosts, None));
        // Which rows are lit, known before any is drawn: rows lit one
        // under another are one block, round at its two ends alone.
        let lit: Vec<bool> = rows.iter().map(|row| matches!(row, Row::Node(node, _, _) if picked.as_ref() == Some(&node.path) || marked.contains(&node.path))).collect();
        for (ix, row) in rows.into_iter().enumerate() {
            match row {
                Row::Edit(depth, dir) => {
                    if let Some(field) = field.take() {
                        list = put(list, &mut ghosts, Some(PathBuf::from(TREE_EDIT_KEY)), edit_row(depth, dir, false, field).into_any_element());
                    }
                }
                Row::Node(node, depth, open) if matches!(&prompt, Some(FilePrompt::Rename(p)) if *p == node.path) => {
                    if let Some(field) = field.take() {
                        list = put(list, &mut ghosts, Some(node.path.clone()), edit_row(depth, node.dir, open, field).into_any_element());
                    }
                }
                Row::More(n, depth) => {
                    list = list.child(h_flex().h(TREE_ROW_H).flex_shrink_0().pl(pad(depth) + px(15.)).text_size(px(11.5)).text_color(theme.muted_foreground).child(format!("{n} more")));
                }
                Row::Node(node, depth, open) => {
                    let chosen = lit[ix];
                    let (above, below) = (ix > 0 && lit[ix - 1], lit.get(ix + 1).copied().unwrap_or(false));
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
                    let (click, menu, over) = (node.clone(), node.clone(), node.clone());
                    let icon = img(crate::file_icons::path(&node.name, node.dir, open, dark)).size(px(16.)).flex_shrink_0();
                    // Files dragged in: the folder they would go into is
                    // lit, and what it holds under it more faintly, so
                    // the whole of where they land reads as one place.
                    // What a drag of this row takes: the rows chosen with
                    // it, or itself.
                    let dragged = TreeDrag(if marked.len() > 1 && marked.contains(&node.path) { tops_of(&marked) } else { vec![node.path.clone()] });
                    let cut = self.file_cut.iter().any(|c| node.path.starts_with(c));
                    let drop_here = !drop_root && node.dir && drop_dir.as_ref() == Some(&node.path);
                    let drop_in = !drop_root && !drop_here && drop_dir.as_ref().is_some_and(|d| node.path.starts_with(d));
                    list = put(
                        list,
                        &mut ghosts,
                        Some(node.path.clone()),
                        h_flex()
                            .id(("tree-row", ix))
                            .h(TREE_ROW_H)
                            .flex_shrink_0()
                            .pl(pad(depth))
                            .pr(px(8.))
                            .gap(px(5.))
                            .items_center()
                            .rounded(px(6.))
                            .when(chosen && above, |d| d.rounded_t(px(0.)))
                            .when(chosen && below, |d| d.rounded_b(px(0.)))
                            .cursor_pointer()
                            .when(chosen, |d| d.bg(theme.primary.opacity(0.12)))
                            .when(!chosen, |d| d.hover(|s| s.bg(row_hover(&theme, dark))))
                            .when(drop_here, |d| d.bg(theme.primary.opacity(0.22)).rounded_b(px(0.)))
                            .when(drop_in, |d| d.bg(theme.primary.opacity(0.08)).rounded(px(0.)))
                            .when(cut, |d| d.opacity(0.45))
                            // A press on a row is the row's: a click, or
                            // the start of its drag, never a rectangle.
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.tree_press_row = true))
                            // Dragged, the row (with the others chosen)
                            // is moved where it is let go, or copied
                            // there with ⌥ held.
                            .on_drag(dragged, |drag, _, _, cx| {
                                let label = match drag.0.as_slice() {
                                    [one] => path_name(one),
                                    many => format!("{} items", many.len()),
                                };
                                cx.new(|_| TreeDragged(label))
                            })
                            .on_drag_move::<ExternalPaths>(cx.listener({
                                let over = over.clone();
                                move |this, e: &DragMoveEvent<ExternalPaths>, _, cx| {
                                    if e.bounds.contains(&e.event.position) {
                                        this.tree_drop_copy = true;
                                        this.tree_drop_over(e.event.position, Some((&over.path, over.dir)), cx);
                                    }
                                }
                            }))
                            .on_drag_move::<TreeDrag>(cx.listener(move |this, e: &DragMoveEvent<TreeDrag>, _, cx| {
                                if e.bounds.contains(&e.event.position) {
                                    this.tree_drop_kind(e.event.modifiers.alt, cx);
                                    this.tree_drop_over(e.event.position, Some((&over.path, over.dir)), cx);
                                }
                            }))
                            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                // The click is the row's: the room around
                                // the rows takes one as "choose nothing".
                                swallow_click(window, cx);
                                // The keyboard is the panel's from here:
                                // ⌘C and ⌘V are this row's.
                                window.focus(&this.tree_focus, cx);
                                // The end of a rectangle's drag is not
                                // a click on the row it ended over.
                                if std::mem::take(&mut this.tree_band_live) {
                                    return;
                                }
                                // ⌘ and a click adds the row to those
                                // chosen or takes it out; ⇧ and a click
                                // chooses from the row last clicked to
                                // this one. Neither opens anything.
                                let keys = ev.modifiers();
                                if keys.secondary() || keys.shift {
                                    return this.tree_click_more(&click.path, keys.shift, cx);
                                }
                                this.tree_anchor = Some(click.path.clone());
                                this.tree.marked.clear();
                                if click.dir {
                                    this.tree.picked = Some(click.path.clone());
                                    this.tree_toggle(&click.path, cx)
                                } else {
                                    this.file_clicked(&click.path, window, cx);
                                    this.tree.picked = Some(click.path.clone());
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    // The row asked about is the one lit:
                                    // itself, or with the others chosen
                                    // when it is one of them.
                                    if this.tree.marked.len() > 1 && this.tree.marked.contains(&menu.path) {
                                        return this.files_menu(ev.position, cx);
                                    }
                                    this.tree.marked.clear();
                                    this.tree.picked = Some(menu.path.clone());
                                    this.file_menu(Some(&menu), ev.position, cx)
                                }),
                            )
                            .child(div().w(px(10.)).flex_shrink_0().flex().justify_center().when(node.dir, |d| d.child(chevron(open))))
                            .child(icon)
                            .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(ink).child(node.name.clone()))
                            .children(badge)
                            .into_any_element(),
                    );
                }
            }
        }
        // Files dragged in, over the session's folder itself: a line
        // round the whole panel. And at its foot, where they would go,
        // in words, coming up as they arrive and fading as they leave.
        let ring = drop_root.then(|| {
            div()
                .absolute()
                .inset(px(3.))
                .rounded(px(8.))
                .border_1()
                .border_color(theme.primary.opacity(0.75))
                .bg(theme.primary.opacity(0.06))
                .with_animation("tree-drop-ring", Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
        });
        let words = |text: String| {
            h_flex()
                .h(px(26.))
                .px(px(10.))
                .gap(px(6.))
                .items_center()
                .rounded_full()
                .bg(theme.popover)
                .border_1()
                .border_color(theme.primary.opacity(0.6))
                .shadow_md()
                .text_size(px(12.))
                .text_color(theme.foreground)
                .child(Icon::default().path(if self.tree_drop_copy { "icons/copy.svg" } else { "icons/arrow-right.svg" }).with_size(px(13.)).text_color(theme.primary))
                .child(div().max_w(px(220.)).truncate().child(text))
        };
        let drop_words = match (&drop_dir, &self.tree_drop_gone) {
            (Some(dir), _) => Some(h_flex().absolute().left_0().right_0().bottom(px(12.)).justify_center().child(words(self.tree_drop_words(dir))).with_animation("tree-drop-words-in", Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t).bottom(px(4. + 8. * t))).into_any_element()),
            (None, Some((text, at))) if at.elapsed() < TREE_ANIM => {
                Some(h_flex().absolute().left_0().right_0().bottom(px(12.)).justify_center().child(words(text.clone())).with_animation("tree-drop-words-out", Animation::new(TREE_ANIM), |d, t| d.opacity(1. - t)).into_any_element())
            }
            _ => None,
        };
        let body = if exists {
            v_flex()
                .relative()
                .flex_1()
                .min_h_0()
                // Files from the file manager, anywhere over the panel:
                // the rows say which folder, the room around them is
                // the session's own.
                .on_drag_move::<ExternalPaths>(cx.listener(|this, e: &DragMoveEvent<ExternalPaths>, _, cx| {
                    if e.bounds.contains(&e.event.position) {
                        this.tree_drop_copy = true;
                        this.tree_drop_over(e.event.position, None, cx);
                    } else {
                        this.tree_drop_off(cx);
                    }
                }))
                .on_drop(cx.listener(|this, files: &ExternalPaths, _, cx| this.tree_drop_done(files.paths(), true, cx)))
                // Rows of the tree itself, dragged: moved, or copied
                // with ⌥ held as they are let go.
                .on_drag_move::<TreeDrag>(cx.listener(|this, e: &DragMoveEvent<TreeDrag>, _, cx| {
                    if e.bounds.contains(&e.event.position) {
                        this.tree_drop_kind(e.event.modifiers.alt, cx);
                        this.tree_drop_over(e.event.position, None, cx);
                    } else {
                        this.tree_drop_off(cx);
                    }
                }))
                .on_drop(cx.listener(|this, rows: &TreeDrag, window, cx| this.tree_drop_done(&rows.0, window.modifiers().alt, cx)))
                .children(ring)
                .child(
                    v_flex()
                        .id("files-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.files_scroll)
                        .px(px(6.))
                        .pb(px(6.))
                        .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, _, cx| this.file_menu(None, ev.position, cx)))
                        // A press anywhere in the panel may become a
                        // rectangle dragged over the rows (`tree_band_to`).
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, ev: &MouseDownEvent, _, _| {
                                // A row took the press: it is its click
                                // or its drag.
                                if std::mem::take(&mut this.tree_press_row) {
                                    return;
                                }
                                this.tree_band = Some((ev.position, ev.position));
                                this.tree_band_live = false;
                            }),
                        )
                        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, cx| {
                            if ev.pressed_button == Some(MouseButton::Left) {
                                this.tree_band_to(ev.position, cx);
                                if this.tree_band_live {
                                    window.focus(&this.tree_focus, cx);
                                }
                            }
                        }))
                        .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.tree_band_end(cx)))
                        .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.tree_band_end(cx)))
                        // A click on the panel's empty room lets what
                        // is chosen go.
                        .on_click(cx.listener(|this, _, window, cx| {
                            window.focus(&this.tree_focus, cx);
                            if std::mem::take(&mut this.tree_band_live) {
                                return;
                            }
                            this.tree.marked.clear();
                            this.tree.picked = None;
                            cx.notify();
                        }))
                        .child(list),
                )
                .children(drop_words)
                // The rectangle, over the rows, cut to the panel.
                .children(self.tree_band.filter(|_| self.tree_band_live).map(|(a, b)| {
                    let room = self.files_scroll.bounds();
                    let (left, top) = (a.x.min(b.x).max(room.left()), a.y.min(b.y).max(room.top()));
                    let (right, bottom) = (a.x.max(b.x).min(room.right()), a.y.max(b.y).min(room.bottom()));
                    div().absolute().left(left - room.left()).top(top - room.top()).w((right - left).max(px(0.))).h((bottom - top).max(px(0.))).rounded(px(3.)).border_1().border_color(theme.primary.opacity(0.7)).bg(theme.primary.opacity(0.10))
                }))
                .vertical_scrollbar(&self.files_scroll)
                .into_any_element()
        } else {
            div().p(px(14.)).text_size(px(12.)).text_color(theme.muted_foreground).child("The session's folder is gone.").into_any_element()
        };
        // The session's folder heads the tree, open to begin with: a row
        // that does not scroll away, with what is asked of the whole
        // tree at its right. A click opens or closes it and chooses
        // nothing, so New File goes in the folder itself.
        let root_row = root.as_ref().filter(|_| exists).map(|r| {
            let (open, toggled) = (!self.tree.shut.contains(r), r.clone());
            let name = r.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| r.to_string_lossy().to_string());
            let tool = |id: &'static str, icon: &'static str, tip: &'static str| Button::new(id).ghost().small().icon(Icon::default().path(icon)).tooltip(tip);
            h_flex()
                .id("tree-root")
                .h(TREE_ROW_H)
                .flex_shrink_0()
                .mx(px(6.))
                .mt(px(6.))
                .pl(px(8.))
                .pr(px(2.))
                .gap(px(5.))
                .items_center()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    window.focus(&this.tree_focus, cx);
                    this.tree.marked.clear();
                    this.tree.picked = None;
                    if !this.tree.shut.remove(&toggled) {
                        this.tree.shut.insert(toggled.clone());
                    }
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.file_menu(None, ev.position, cx)
                    }),
                )
                .child(div().w(px(10.)).flex_shrink_0().flex().justify_center().child(chevron(open)))
                .child(img(crate::file_icons::path(&name, true, open, dark)).size(px(16.)).flex_shrink_0())
                .child(div().flex_1().min_w_0().truncate().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(name))
                .child(tool("tree-new-file", "icons/file-plus.svg", "New File").on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.tree_new(false, window, cx);
                })))
                .child(tool("tree-new-folder", "icons/folder-plus.svg", "New Folder").on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.tree_new(true, window, cx);
                })))
                .child(tool("tree-collapse", "icons/copy-minus.svg", "Collapse All Folders").on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.tree_collapse(cx);
                })))
        });
        let el = v_flex().track_focus(&self.tree_focus).on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
            if !this.tree_rename_key(e, window, cx) {
                this.tree_key(e, cx);
            }
        })).w(self.panel_w_now()).h_full().flex_shrink_0().border_r_1().border_color(theme.border).bg(theme.sidebar).child(panel_head("Files", self.branch_pill(cx), self.changes_pill(cx), &theme)).children(self.git_trouble_strip(cx)).children(self.stash_strip(cx)).children(self.publish_strip(cx)).children(self.sync_strip(cx)).children(root_row).child(body);
        self.panel_in(true, el)
    }

    // -- branches -------------------------------------------------------------

    /// The folder's branches, when it is in a repository.
    /// How many files git says have a change in the folder showing; 0
    /// until it has been asked.
    pub(crate) fn panel_changes(&self) -> usize {
        self.files_root().filter(|root| self.tree.git.as_ref().is_some_and(|(of, _)| of == root)).map_or(0, |_| self.tree.changed.len())
    }

    /// Every file with a change, as git last had them.
    pub(crate) fn changed_files(&self) -> Rc<Vec<(PathBuf, git::State)>> {
        self.tree.changed.clone()
    }

    pub(crate) fn branches(&self) -> Option<Rc<git::Branches>> {
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
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener({
                        let name = b.label();
                        move |this, ev: &MouseDownEvent, _, cx| this.open_menu(ev.position, vec![("Copy Branch Name", MenuDo::Copy(name.clone()))], cx)
                    }),
                )
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
            // The list as it is now, not as the clock last read it, and
            // as the remote has it now, not as it was last fetched.
            if let Some(root) = self.files_root() {
                self.read_git(root, cx);
            }
            self.branch_fetch(false, cx);
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
        // By when each was last committed to, the newest first, or as git
        // names them: the default, the rest by name, then a remote's own.
        let all: Vec<(String, bool)> = if self.branch_by_name { b.local.iter().map(|n| (n.clone(), false)).chain(b.remote.iter().map(|n| (n.clone(), true))).collect() } else { b.by_recency() };
        let rows: Vec<(String, bool)> = all.into_iter().filter(|(n, _)| n.to_lowercase().contains(&low)).collect();
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
            (None, Some(name)) => self.branch_new_open(Some(name), window, cx),
            _ => {}
        }
    }

    /// Check a branch out, or make one and check it out. Git does it
    /// and git may refuse; nothing is forced. An agent at work in the
    /// folder is no reason to wait: git is the person's to run.
    pub(crate) fn branch_go(&mut self, name: String, create: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.branch_begin(name, create, None, false, window, cx);
    }

    /// `branch_go`, for a new branch that may start from `base`, a
    /// branch other than the one checked out, brought up to the
    /// remote's first when `update`.
    pub(crate) fn branch_begin(&mut self, name: String, create: bool, base: Option<String>, update: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_branch_menu(window, cx);
        let Some(root) = self.files_root() else { return };
        if self.branches().is_some_and(|b| b.current.as_deref() == Some(&name)) || self.branch_busy {
            return;
        }
        if create && !git::valid_name(&name) {
            self.notice = Some(Notice::error(format!("\u{201c}{name}\u{201d} is not a name git takes for a branch")));
            return;
        }
        // With changes not yet committed the person says where they go,
        // as GitHub Desktop asks it. A detached head has no branch to
        // leave them on.
        // The changes are the repository's, not only this folder's: a
        // switch moves all of them.
        if self.tree.git.as_ref().is_some_and(|(of, st)| *of == root && st.dirty) {
            // A conflict still open is resolved before anything else: git
            // switches nowhere with one, and a stash cannot hold it.
            let open: Vec<String> = self.tree.changed.iter().filter(|(_, st)| *st == git::State::Conflict).map(|(p, _)| p.strip_prefix(&root).unwrap_or(p).to_string_lossy().to_string()).collect();
            if !open.is_empty() {
                self.branch_ask = Some(BranchAsk::stop("Resolve the conflicts first", format!("{} on this branch {} a conflict git is waiting to have resolved: {}. Resolve and commit, or undo the merge, then switch.", plural(open.len(), "file", "files"), if open.len() == 1 { "has" } else { "have" }, git::name_some(&open))));
                cx.notify();
                return;
            }
            // Whether the changes would fit on the other branch is asked
            // of git while the question is up; a new branch starts from
            // here, so they always do.
            self.branch_ask = Some(BranchAsk { name: name.clone(), create, leave: true, fit: (create && base.is_none()).then(Vec::new), stop: None, base: base.clone(), update });
            if !create || base.is_some() {
                cx.spawn(async move |this, cx| {
                    // Where the changes have to fit: the branch gone to,
                    // or the one a new branch starts from.
                    let (dir, to) = (root.clone(), base.clone().unwrap_or_else(|| name.clone()));
                    let bad = cx.background_executor().spawn(async move { git::misfits(&dir, &to) }).await;
                    this.update(cx, |this, cx| {
                        if let Some(a) = this.branch_ask.as_mut().filter(|a| a.name == name && a.stop.is_none()) {
                            a.fit = Some(bad);
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
            }
            cx.notify();
            return;
        }
        self.branch_run_from(name, create, base, update, git::Carry::Bring, cx);
    }

    /// The switch itself, off the main thread, with the changes going
    /// where `carry` says.
    pub(crate) fn branch_run(&mut self, name: String, create: bool, carry: git::Carry, cx: &mut Context<Self>) {
        let (base, update) = self.branch_ask.as_ref().map(|a| (a.base.clone(), a.update)).unwrap_or_default();
        self.branch_run_from(name, create, base, update, carry, cx);
    }

    fn branch_run_from(&mut self, name: String, create: bool, base: Option<String>, update: bool, carry: git::Carry, cx: &mut Context<Self>) {
        self.branch_ask = None;
        let Some(root) = self.files_root() else { return };
        if self.branch_busy {
            return;
        }
        let from = self.branches().map(|b| b.label()).unwrap_or_default();
        let origin = self.branches().and_then(|b| b.origin.clone()).unwrap_or_else(|| "origin".into());
        // A branch only the remote has is asked for again first: what
        // is checked out is then what the remote has now, not what it
        // had at the last fetch.
        let theirs = !create && self.branches().is_some_and(|b| b.remote.contains(&name));
        self.branch_busy = true;
        cx.spawn(async move |this, cx| {
            let (dir, to, start) = (root.clone(), name.clone(), base.clone());
            let done = cx
                .background_executor()
                .spawn(async move {
                    let was_dirty = git::dirty(&dir);
                    let stale = if theirs { git::fetch_branch(&dir, &to).err() } else { None };
                    // The branch a new one starts from, brought up to the
                    // remote's: refused, nothing is made.
                    if let (true, Some(b)) = (update, &start) {
                        if let Err(why) = git::fetch(&dir).and_then(|_| git::fast_forward(&dir, b)) {
                            return (was_dirty, Err(why), stale, true);
                        }
                    }
                    let went = match (&start, create) {
                        (Some(b), true) => git::create_from(&dir, &to, b, carry),
                        _ => git::switch_with(&dir, &to, create, carry),
                    };
                    (was_dirty, went, stale, false)
                })
                .await;
            this.update(cx, |this, cx| {
                this.branch_busy = false;
                let (was_dirty, went, stale, at_update) = done;
                let made = match (create, &base) {
                    (true, Some(b)) => format!("made {name} from {b} and switched to it"),
                    (true, None) => format!("made {name} and switched to it"),
                    _ => format!("switched to {name}"),
                };
                this.notice = Some(match (&went, was_dirty) {
                    (Ok(()), false) => Notice::said(made),
                    (Ok(()), true) if carry == git::Carry::Leave => Notice::said(format!("{made}; your changes stay on {from}")),
                    (Ok(()), true) => Notice::said(format!("{made}, with your changes")),
                    (Err(why), _) => Notice::error(format!("not switched: {why}")),
                });
                match went {
                    // The branch was checked out as last fetched, and the
                    // remote was not reached: said, since what is here
                    // may be behind.
                    Ok(()) => {
                        if let Some(why) = stale {
                            this.notice = Some(Notice::error(format!("switched to {name} as it was last fetched; {origin} could not be asked: {why}")));
                            if git::why_net(&why) == git::NetTrouble::SignIn {
                                this.open_gate(format!("fetch {name}"), why, cx);
                            }
                        }
                    }
                    Err(why) if at_update => {
                        let b = base.clone().unwrap_or_default();
                        match git::why_net(&why) {
                            git::NetTrouble::Other => this.branch_ask = Some(BranchAsk::stop("Not created", format!("{b} could not be brought up to {origin}, so no branch was made and nothing was changed. Git said: {why}."))),
                            _ => this.net_failed("Not created", format!("update {b}"), why, cx),
                        }
                    }
                    // A refusal is also said where it cannot be missed: the
                    // row under the composer may be under a sheet.
                    Err(why) => this.branch_ask = Some(BranchAsk::stop(if create { "Not created" } else { "Not switched" }, format!("Nothing was changed. {}{}.", why[..1].to_uppercase(), &why[1..]))),
                }
                // The files are another branch's now.
                this.tree.refresh(&root);
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Put back the changes left on this branch at an earlier switch.
    /// Git refuses, and keeps them, when they would write over a change
    /// made here since.
    fn branch_restore(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        if self.branch_busy {
            return;
        }
        self.branch_busy = true;
        cx.spawn(async move |this, cx| {
            let dir = root.clone();
            let done = cx.background_executor().spawn(async move { git::restore(&dir) }).await;
            this.update(cx, |this, cx| {
                this.branch_busy = false;
                this.notice = Some(match done {
                    Ok(()) => Notice::said("your changes are back"),
                    Err(why) => Notice::error(format!("not put back: {why}")),
                });
                this.tree.refresh(&root);
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Under the files' head, in a checkout git says nothing of: why, in
    /// git's own words, wrapped and not cut, since the words are what to
    /// do about it. Without it such a folder only lacked its branch and
    /// its marks, and nothing said so.
    fn git_trouble_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let root = self.files_root()?;
        let trouble = self.tree.git_trouble.as_ref().filter(|(of, _)| *of == root).map(|(_, trouble)| trouble.clone())?;
        // The command that puts it right, on a line of its own with a
        // copy button: only for a trouble known for certain
        // (`git::xcode_licence`).
        let fix = trouble.fix.map(|command| {
            let key = SharedString::from("git-trouble-fix");
            let done = self.copied.as_ref() == Some(&key);
            let hover_bg = theme.muted;
            h_flex()
                .mt(px(5.))
                .pl(px(8.))
                .pr(px(3.))
                .py(px(2.))
                .gap(px(6.))
                .items_center()
                .rounded(px(6.))
                .bg(theme.background)
                .border_1()
                .border_color(theme.border.opacity(0.6))
                .child(div().flex_1().min_w_0().font_family(theme.mono_font_family.clone()).text_size(px(11.)).text_color(theme.foreground).child(command))
                .child(
                    div()
                        .id(key.clone())
                        .size(px(20.))
                        .flex_shrink_0()
                        .rounded(px(5.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .managed_tooltip(move |window, cx| crate::workbench::tip(if done { "Copied" } else { "Copy" }, window, cx))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.copy_text(key.clone(), command.to_string(), cx);
                        }))
                        .child(Icon::new(if done { IconName::Check } else { IconName::Copy }).with_size(px(12.)).text_color(theme.muted_foreground)),
                )
        });
        let words = trouble.words;
        Some(
            h_flex()
                .w_full()
                .flex_shrink_0()
                .px(px(12.))
                .py(px(7.))
                .gap(px(8.))
                .items_start()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.warning.opacity(0.10))
                .text_size(px(11.5))
                .line_height(px(16.))
                .child(div().h(px(16.)).flex().items_center().flex_shrink_0().child(Icon::default().path("icons/git-branch.svg").with_size(px(12.)).text_color(theme.warning)))
                .child(v_flex().flex_1().min_w_0().child(div().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child("Git did not answer for this folder")).child(div().text_color(theme.muted_foreground).child(words)).children(fix))
                .with_animation("git-trouble-in", Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
                .into_any_element(),
        )
    }

    /// Under the files' head, on a branch that changes were left on:
    /// one line saying so, and the button that puts them back.
    fn stash_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let b = self.branches()?;
        if b.stashed == 0 {
            return None;
        }
        Some(
            h_flex()
                .w_full()
                .flex_shrink_0()
                .px(px(12.))
                .py(px(7.))
                .gap(px(8.))
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.primary.opacity(0.07))
                .text_size(px(11.5))
                .child(Icon::default().path("icons/git-branch.svg").with_size(px(12.)).text_color(theme.link).flex_shrink_0())
                .child(div().flex_1().min_w_0().truncate().text_color(theme.foreground).child("Changes you left on this branch"))
                .child(crate::branches::small_pill("stash-restore", "icons/undo-2.svg", "Restore", Press::Quiet, self.branch_busy, &theme).when(!self.branch_busy, |d| {
                    d.on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.branch_restore(cx)
                    }))
                }))
                .into_any_element(),
        )
    }

    /// The question a switch asks when there are changes not yet
    /// committed, as GitHub Desktop asks it: leave them on the branch
    /// being left, or bring them along.
    pub(crate) fn render_branch_ask(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(ask) = self.branch_ask.clone() else { return div().into_any_element() };
        let from = self.branches().map(|b| b.label()).unwrap_or_default();
        let can_leave = self.branches().is_some_and(|b| b.current.is_some());
        // A refusal: its words and one button.
        if let Some((title, words)) = ask.stop.clone() {
            return div()
                .id("branch-ask-overlay")
                .absolute()
                .inset_0()
                .occlude()
                .bg(theme.overlay)
                .flex()
                .flex_col()
                .items_center()
                .pt(px(120.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.branch_ask = None;
                    cx.notify();
                }))
                .child(
                    v_flex()
                        .id("branch-ask")
                        .occlude()
                        .on_click(|_, window, cx| swallow_click(window, cx))
                        .w(px(440.))
                        .max_w(gpui::relative(0.94))
                        .p(px(16.))
                        .gap(px(12.))
                        .rounded(px(16.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .shadow(float_shadow(&theme))
                        .child(h_flex().gap(px(8.)).items_center().child(Icon::new(IconName::TriangleAlert).with_size(px(15.)).text_color(theme.danger)).child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(title)))
                        .child(div().text_size(px(12.5)).line_height(px(19.)).text_color(theme.foreground).child(words))
                        .child(h_flex().w_full().justify_end().child(choice_pill("ask-close", "icons/close.svg", "Close", Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.branch_ask = None;
                            cx.notify();
                        })))),
                )
                .into_any_element();
        }
        // Bringing is offered once git has said the changes fit there.
        let fits = ask.fit.as_ref().is_some_and(Vec::is_empty);
        let bring_line = match &ask.fit {
            None => "Checking whether your changes fit there…".to_string(),
            Some(bad) if bad.is_empty() => "Your work in progress follows you to the new branch.".to_string(),
            Some(bad) => format!("Not possible: {} would conflict there ({}). Leave them here, or commit them first.", plural(bad.len(), "file", "files"), git::name_some(bad)),
        };
        let option = |id: &'static str, on: bool, enabled: bool, title: String, line: String| {
            h_flex()
                .id(id)
                .w_full()
                .px(px(12.))
                .py(px(10.))
                .gap(px(10.))
                .items_start()
                .when(on, |d| d.bg(theme.primary.opacity(0.07)))
                .when(enabled && !on, |d| d.cursor_pointer().hover(|s| s.bg(theme.muted.opacity(0.6))))
                // Rounded where the box is, the first at its top and the
                // second at its foot: a clip is a rectangle.
                .map(|d| if id == "ask-leave" { d.rounded_t(px(9.)) } else { d.rounded_b(px(9.)) })
                .when(!enabled, |d| d.opacity(0.45))
                .child(
                    div().mt(px(2.)).size(px(14.)).flex_shrink_0().rounded_full().border_1().border_color(if on { theme.primary } else { theme.muted_foreground.opacity(0.6) }).flex().items_center().justify_center().when(on, |d| d.child(div().size(px(8.)).rounded_full().bg(theme.primary))),
                )
                .child(v_flex().flex_1().min_w_0().gap(px(2.)).child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title)).child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(line)))
        };
        let pick = |leave: bool| {
            cx.listener(move |this, _: &ClickEvent, window, cx| {
                swallow_click(window, cx);
                if let Some(a) = this.branch_ask.as_mut() {
                    let can_leave = this.tree.branches.as_ref().is_some_and(|(_, b)| b.current.is_some());
                    let fits = a.fit.as_ref().is_some_and(Vec::is_empty);
                    // A choice that is not on offer is not taken.
                    if (leave && can_leave) || (!leave && fits) {
                        a.leave = leave;
                    }
                }
                cx.notify();
            })
        };
        // Leaving is the choice to begin with, as in GitHub Desktop, where
        // there is a branch to leave them on.
        let (name, create, leave) = (ask.name.clone(), ask.create, ask.leave && can_leave);
        // With neither on offer (a detached head whose changes do not
        // fit) there is nothing to go ahead with.
        let ready = leave || fits;
        div()
            .id("branch-ask-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .flex_col()
            .items_center()
            .pt(px(120.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.branch_ask = None;
                cx.notify();
            }))
            .child(
                v_flex()
                    .id("branch-ask")
                    .occlude()
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(px(440.))
                    .max_w(gpui::relative(0.94))
                    .p(px(16.))
                    .gap(px(12.))
                    .rounded(px(16.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(if create { "Create branch" } else { "Switch branch" }))
                    .child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child("You have changes on this branch. What would you like to do with them?"))
                    .child(
                        v_flex()
                            .w_full()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(theme.border)
                            .overflow_hidden()
                            .child(option("ask-leave", leave, can_leave, format!("Leave my changes on {from}"), "Your work in progress is stashed on this branch for you to return to later.".to_string()).on_click(pick(true)))
                            .child(div().h(px(1.)).w_full().bg(theme.border))
                            .child(option("ask-bring", !leave && fits, fits, format!("Bring my changes to {name}"), bring_line).on_click(pick(false))),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .gap(px(8.))
                            .child(choice_pill("ask-cancel", "icons/close.svg", "Cancel", Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                                swallow_click(window, cx);
                                this.branch_ask = None;
                                cx.notify();
                            })))
                            .child(choice_pill("ask-go", "icons/git-branch.svg", if create { "Create branch" } else { "Switch branch" }, Press::Primary, !ready, &theme).when(ready, |d| {
                                d.on_click(cx.listener(move |this, _, window, cx| {
                                    swallow_click(window, cx);
                                    this.branch_run(name.clone(), create, if leave { git::Carry::Leave } else { git::Carry::Bring }, cx);
                                }))
                            })),
                    ),
            )
            .into_any_element()
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
        let w = px(420.).min(view.width - px(16.));
        let x = (at.x - px(40.)).min(view.width - w - px(8.)).max(px(8.));
        let y = (at.y + px(16.)).min(view.height - px(200.)).max(px(8.));
        // The outline is a thinned ink, not the border colour: that is the
        // row's own ground under the pointer, and the pill went with it.
        let chip = |word: &'static str| div().flex_shrink_0().px(px(6.)).rounded_full().border_1().border_color(theme.muted_foreground.opacity(0.4)).text_size(px(10.5)).text_color(theme.muted_foreground).child(word);
        let origin = b.origin.clone().unwrap_or_else(|| "origin".into());
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
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener({
                            let name = name.clone();
                            move |this, ev: &MouseDownEvent, _, cx| {
                                // Not the sheet's own right click, which puts the list away.
                                cx.stop_propagation();
                                this.open_menu(ev.position, vec![("Copy Branch Name", MenuDo::Copy(name.clone()))], cx);
                            }
                        }),
                    )
                    .child(div().w(px(14.)).flex_shrink_0().when(here, |d| d.child(Icon::new(IconName::Check).with_size(px(13.)))))
                    // The name, then a column of its own for the tag, each
                    // starting where the others do, and the time at the
                    // row's far end.
                    .child(div().flex_1().min_w_0().truncate().when(here, |d| d.font_weight(FontWeight::MEDIUM)).child(name.clone()))
                    .child(h_flex().w(BRANCH_TAG_W).flex_shrink_0().map(|d| {
                        if b.default.as_deref() == Some(name.as_str()) {
                            d.child(chip("default"))
                        } else if *remote {
                            d.child(chip("remote"))
                        } else if b.unpublished.contains(name) {
                            // Made here and on no remote yet: a tag that
                            // says so, as the others say what a branch is,
                            // and beside it the arrow that sends it there.
                            // Both in the accent, since it is the one row
                            // with something left to do.
                            let busy = self.branch_publishing.as_deref() == Some(name.as_str());
                            let (to, tip) = (name.clone(), if busy { format!("Publishing to {origin}…") } else { format!("Publish this branch to {origin}") });
                            d.gap(px(4.)).child(div().flex_shrink_0().px(px(6.)).rounded_full().border_1().border_color(theme.primary.opacity(0.6)).text_size(px(10.5)).text_color(theme.primary).child("local")).child(
                                div()
                                    .id(("branch-publish", ix))
                                    .size(px(18.))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(theme.primary.opacity(0.12))
                                    .when(!busy, |d| d.cursor_pointer().hover(|s| s.bg(theme.primary.opacity(0.28))))
                                    .managed_tooltip(move |window, cx| crate::workbench::tip(tip.clone(), window, cx))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        // Not the row's own click, which switches.
                                        cx.stop_propagation();
                                        swallow_click(window, cx);
                                        this.branch_publish(to.clone(), cx);
                                    }))
                                    .child(if busy {
                                        Icon::new(IconName::LoaderCircle).with_size(px(11.)).text_color(theme.primary).with_animation(("branch-publishing", ix), Animation::new(Duration::from_millis(900)).repeat(), |icon, t| icon.rotate(gpui::Radians(t * std::f32::consts::TAU))).into_any_element()
                                    } else {
                                        Icon::new(IconName::ArrowUp).with_size(px(11.)).text_color(theme.primary).into_any_element()
                                    }),
                            )
                        } else {
                            d
                        }
                    }))
                    .child(div().w(BRANCH_WHEN_W).flex_shrink_0().whitespace_nowrap().text_right().text_size(px(11.5)).text_color(theme.muted_foreground).children(b.when.get(name).filter(|at| **at > 0).map(|at| crate::format::ago(*at as f64, self.now)))),
            );
        }
        // Words that leave no branch: said, with the way to make one of
        // that name, as GitHub Desktop has it.
        if rows.is_empty() {
            let typed = self.branch_input.read(cx).value().trim().to_string();
            list = list.child(
                v_flex()
                    .w_full()
                    .px(px(16.))
                    .py(px(18.))
                    .gap(px(6.))
                    .items_center()
                    .child(div().size(px(36.)).rounded_full().bg(theme.primary.opacity(0.10)).flex().items_center().justify_center().child(Icon::default().path("icons/git-branch.svg").with_size(px(17.)).text_color(theme.primary)))
                    .child(div().pt(px(4.)).text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child("Sorry, I can't find that branch"))
                    .child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child("Do you want to create a new branch instead?"))
                    .child(div().pt(px(8.)).w_full().child(choice_pill("branch-make-new", "icons/git-branch.svg", "Create New Branch", Press::Primary, false, &theme).w_full().justify_center().on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.branch_new_open(Some(typed.clone()).filter(|t| !t.is_empty()), window, cx);
                    }))))
                    .child(div().pt(px(6.)).text_size(px(11.5)).text_color(theme.muted_foreground).child(crate::workbench::key_text("Press ⌘⇧N to create a branch from anywhere in the app"))),
            );
        }
        let fresh = fresh.filter(|_| !rows.is_empty());
        let fetch_line = self.branch_fetch_line(cx);
        // What is last in the card is rounded at its foot as the card is.
        let make_last = fetch_line.is_none();
        let list_last = fresh.is_none() && fetch_line.is_none();
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
                .when(make_last, |d| d.rounded_b(px(11.)))
                .hover(|s| s.bg(theme.sidebar_accent))
                .on_click(cx.listener(move |this, _, window, cx| this.branch_new_open(Some(to.clone()), window, cx)))
                .child(Icon::default().path("icons/git-branch.svg").with_size(px(13.)).text_color(theme.muted_foreground).flex_shrink_0())
                .child(div().min_w_0().child(StyledText::new(format!("Create branch {name}…")).with_highlights([(14..14 + name.len(), HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() })])))
        });
        // How the list is ordered: the settings panel's segmented
        // control in a small size, its plate sliding to the choice.
        let sort = div().flex_shrink_0().mr(px(4.)).child(self.segmented_sized(
            "branch-sort",
            vec![("recent", "Recent".to_string(), None), ("name", "Name".to_string(), None)],
            if self.branch_by_name { "name" } else { "recent" },
            Rc::new(|this: &mut Self, key, window, cx| {
                swallow_click(window, cx);
                this.branch_by_name = key == "name";
                this.save_ui(true);
                cx.notify();
            }),
            true,
            cx,
        ));
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
                    .child(div().flex_1().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Switch branches"))
                    .child(sort)
                    .child(Button::new("branch-close").ghost().xsmall().icon(IconName::Close).on_click(cx.listener(|this, _, window, cx| this.close_branch_menu(window, cx)))),
            )
            .child(h_flex().flex_shrink_0().mx(px(10.)).mb(px(10.)).gap(px(8.)).items_center().child(
                h_flex()
                    .id("branch-field")
                    .track_focus(&focus)
                    .role(Role::TextInput)
                    .aria_label("Find or create a branch")
                    .aria_value(typed)
                    .flex_1()
                    .min_w_0()
                    .px(px(8.))
                    .h(px(32.))
                    .gap(px(4.))
                    .items_center()
                    .rounded(px(8.))
                    .bg(theme.muted)
                    .text_size(px(13.))
                    .child(Icon::new(IconName::Search).with_size(px(13.)).text_color(theme.muted_foreground).flex_shrink_0())
                    .child(div().flex_1().min_w_0().child(gpui_component::input::Input::new(&self.branch_input).appearance(false).bordered(false).on_secondary_click(self.input_menu(&self.branch_input, false, cx)))),
            ).child(choice_pill("branch-new-button", "icons/plus.svg", "New Branch", Press::Quiet, false, &theme)
                .role(Role::Button)
                .aria_label("New Branch")
                .managed_tooltip(|window, cx| crate::workbench::tip("New Branch (⌘⇧N)", window, cx))
                // As tall as the field it stands beside.
                .h(px(32.))
                .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                let typed = this.branch_input.read(cx).value().trim().to_string();
                let fresh = this.branches().is_some_and(|b| !typed.is_empty() && !b.has(&typed));
                this.branch_new_open(fresh.then_some(typed), window, cx);
            }))))
            .when(true, |d| {
                d.child(
                    v_flex()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .border_t_1()
                        .border_color(theme.border)
                        .when(list_last, |d| d.pb(px(2.)))
                        .child(v_flex().id("branch-scroll").max_h(BRANCH_ROW_H * BRANCH_ROWS as f32 + px(10.)).overflow_y_scroll().track_scroll(&self.branch_scroll).p(px(5.)).child(list))
                        .vertical_scrollbar(&self.branch_scroll),
                )
            })
            .children(make)
            .children(fetch_line);
        let shut = |this: &mut Self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>| this.close_branch_menu(window, cx);
        div().id("branch-sheet").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(shut)).on_mouse_down(MouseButton::Right, cx.listener(shut)).child(card).into_any_element()
    }

    // -- the comparison -------------------------------------------------------

    /// At the right of the files' head: how many files have a change,
    /// on a button that opens the comparison.
    fn changes_pill(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let root = self.files_root()?;
        // There in any repository, with nothing changed too: the sheet
        // it opens has the history as well, and Push.
        self.branches()?;
        let n = self.tree.git.as_ref().filter(|(of, _)| *of == root).map_or(0, |_| self.tree.changed.len());
        // A button as the branch's is, lit while its sheet is up.
        let open = self.changes.is_some();
        let tip = if n > 0 { format!("{} \u{b7} Changes and History (⌘⇧G)", plural(n, "changed file", "changed files")) } else { "Changes and History (⌘⇧G)".to_string() };
        Some(
            h_flex()
                .id("changes-pill")
                .flex_shrink_0()
                .h(px(21.))
                .px(px(7.))
                .gap(px(5.))
                .items_center()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.border)
                .bg(if open { theme.muted } else { theme.background.opacity(0.6) })
                .hover(|s| s.bg(theme.muted))
                .cursor_pointer()
                .text_size(px(11.5))
                .text_color(theme.foreground)
                .managed_tooltip(move |window, cx| crate::workbench::tip(tip.clone(), window, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| this.open_changes(None, cx)))
                .child(Icon::default().path("icons/git-diff.svg").with_size(px(12.)).text_color(theme.muted_foreground).flex_shrink_0())
                .when(n > 0, |d| d.child(div().font_weight(FontWeight::MEDIUM).child(crate::format::thousands(n))))
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
        let marked: Vec<PathBuf> = picked.iter().cloned().collect();
        self.changes = Some(Changes { root, anchor: picked.clone(), marked, picked, diff: None, pictures: [None, None], list: ListState::new(0, ListAlignment::Top, px(600.)), files, reading: false, band: None, band_live: false, band_at: None, left_out: HashSet::new(), history: false, commits: None, commit: None, commit_files: None, commit_file: None, top: None, commits_scroll: ScrollHandle::new(), cfiles_scroll: ScrollHandle::new(), closing: None });
        self.changes_focus_due = true;
        // How the branch stands against the remote's, for the buttons
        // that push and pull.
        self.branch_fetch(false, cx);
        self.load_diff(cx);
        cx.notify();
    }

    /// ⌘⇧G: the comparison, shown or put away. Git is asked first when
    /// the files panel never was, so the sheet does not open empty on a
    /// folder with changes.
    pub(crate) fn toggle_changes(&mut self, cx: &mut Context<Self>) {
        if self.changes.as_ref().is_some_and(|c| c.closing.is_none()) {
            return self.close_changes(cx);
        }
        let Some(root) = self.files_root().filter(|r| git::inside(r)) else {
            self.notice = Some(Notice::error("open a session in a git repository to see its changes"));
            cx.notify();
            return;
        };
        self.read_git(root, cx);
        self.open_changes(None, cx);
    }

    /// Put the comparison away: it fades where it is for a moment, then
    /// is gone.
    pub(crate) fn close_changes(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.changes.as_mut().filter(|c| c.closing.is_none()) else { return };
        c.closing = Some(std::time::Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CHANGES_OUT).await;
            this.update(cx, |this, cx| {
                if this.changes.as_ref().is_some_and(|c| c.closing.is_some()) {
                    this.changes = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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

    /// A click on a changed file. Alone it is the one chosen and shown;
    /// with ⌘ it joins or leaves the ones chosen; with ⇧ every file from
    /// the last one clicked to it is chosen. The file shown is one of
    /// them while any is chosen.
    fn change_click(&mut self, path: PathBuf, m: Modifiers, cx: &mut Context<Self>) {
        let order: Vec<PathBuf> = self.tree.changed.iter().map(|(p, _)| p.clone()).collect();
        let Some(c) = &mut self.changes else { return };
        // The click that comes with letting a rectangle go is no click.
        if c.band_live || c.band_at.is_some_and(|at| at.elapsed() < Duration::from_millis(300)) {
            return;
        }
        let mut show = path.clone();
        if m.shift {
            let from = c.anchor.as_ref().or(c.picked.as_ref()).and_then(|a| order.iter().position(|p| p == a)).unwrap_or(0);
            if let Some(to) = order.iter().position(|p| *p == path) {
                c.marked = order[from.min(to)..=from.max(to)].to_vec();
            }
        } else if m.secondary() {
            match c.marked.iter().position(|p| *p == path) {
                Some(ix) => {
                    c.marked.remove(ix);
                    if c.picked.as_ref() == Some(&path) {
                        show = c.marked.last().cloned().unwrap_or(path.clone());
                    } else {
                        show = c.picked.clone().unwrap_or(path.clone());
                    }
                }
                None => c.marked.push(path.clone()),
            }
            c.anchor = Some(path);
        } else {
            c.marked = vec![path.clone()];
            c.anchor = Some(path);
        }
        self.pick_change(show, cx);
        cx.notify();
    }

    /// The rectangle follows the pointer, and the files its rows lie
    /// under are the ones chosen. A press that has not moved a few
    /// points is still a click. Every row is one height, so a row's
    /// place is its number.
    fn changes_band_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let changed = self.tree.changed.clone();
        let Some(c) = &mut self.changes else { return };
        let Some((from, _)) = c.band else { return };
        if !c.band_live && (at.x - from.x).abs() < px(4.) && (at.y - from.y).abs() < px(4.) {
            return;
        }
        c.band = Some((from, at));
        c.band_live = true;
        let top = {
            let handle = &c.files.0.borrow().base_handle;
            handle.bounds().top() + handle.offset().y
        };
        let (first, last) = (from.y.min(at.y), from.y.max(at.y));
        c.marked = changed
            .iter()
            .enumerate()
            .filter(|(ix, _)| {
                let y = top + CHANGE_ROW_H * *ix as f32;
                y < last && y + CHANGE_ROW_H > first
            })
            .map(|(_, (p, _))| p.clone())
            .collect();
        cx.notify();
    }

    /// A click on the list's empty room lets what is chosen go. The
    /// file shown stays shown.
    fn changes_let_go(&mut self, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        if c.band_live || c.band_at.is_some_and(|at| at.elapsed() < Duration::from_millis(300)) || c.marked.is_empty() {
            return;
        }
        c.marked.clear();
        c.anchor = None;
        cx.notify();
    }

    /// The press is over. The file shown is one of those chosen, when
    /// any is.
    fn changes_band_end(&mut self, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        let was = c.band.take().is_some();
        if std::mem::take(&mut c.band_live) {
            c.band_at = Some(std::time::Instant::now());
            match c.marked.first().cloned() {
                Some(first) if c.picked.as_ref().is_none_or(|p| !c.marked.contains(p)) => {
                    c.anchor = Some(first.clone());
                    self.pick_change(first, cx);
                }
                _ => {}
            }
        }
        if was {
            cx.notify();
        }
    }

    /// A right click on a changed file: the menu of the several chosen
    /// when it is one of them, and else its own, which chooses it alone.
    fn changes_menu(&mut self, path: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        let several = c.marked.len() > 1 && c.marked.contains(&path);
        let items = if several {
            let paths = c.marked.clone();
            let item = |label: &'static str, what: FileDo| (label, MenuDo::Files(what, paths.clone()));
            vec![item("Add to Message", FileDo::Mention), item("Copy Paths", FileDo::CopyPath), item("Copy Relative Paths", FileDo::CopyRel), ("", MenuDo::Rule), ("Discard Changes", MenuDo::Discard(paths.clone()))]
        } else {
            c.marked = vec![path.clone()];
            c.anchor = Some(path.clone());
            self.pick_change(path.clone(), cx);
            let item = |label: &'static str, what: FileDo| (label, MenuDo::File(what, path.clone()));
            let mut items = Vec::new();
            // A file taken out has nothing to open or to show.
            if path.exists() {
                items.extend([item("Open", FileDo::Open), item(crate::sys::REVEAL_LABEL, FileDo::Reveal), ("", MenuDo::Rule), item("Add to Message", FileDo::Mention)]);
            }
            items.extend([item("Copy Path", FileDo::CopyPath), item("Copy Relative Path", FileDo::CopyRel), ("", MenuDo::Rule), ("Discard Changes", MenuDo::Discard(vec![path.clone()]))]);
            items
        };
        self.open_menu(at, items, cx);
    }

    /// Read the picked file's lines against the last commit, off the
    /// main thread. Run again on the clock while the comparison shows;
    /// the lines are replaced only when they differ, so reading on does
    /// not move the reader.
    pub(crate) fn load_diff(&mut self, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        let Some(path) = c.picked.clone() else { return };
        // The history's lines are a commit's, read once: the clock's
        // reading of the picked file would take their place.
        if c.reading || c.history {
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
                // Asked for before the history was turned to: the lines
                // showing are a commit's now, and these took their place,
                // so the history said "Reading…" until a file was clicked.
                if c.history {
                    return;
                }
                if c.diff.as_ref().is_none_or(|(was, d)| *was != path || **d != diff) {
                    c.list.reset(diff.rows.len());
                    c.pictures = picture_sides(&path, &diff);
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
        let (hover, chosen_bg, ink, quiet, mono) = (row_hover(&theme, dark), theme.primary.opacity(if dark { 0.18 } else { 0.12 }), theme.foreground, theme.muted_foreground, theme.mono_font_family.clone());
        let marked: Rc<HashSet<PathBuf>> = Rc::new(c.marked.iter().cloned().collect());
        let left_out: Rc<HashSet<PathBuf>> = Rc::new(c.left_out.clone());
        let (accent, on_accent) = (theme.primary, theme.primary_foreground);
        let files = uniform_list("changes-files", changed.len(), {
            let (changed, root, marked) = (changed.clone(), root.clone(), marked.clone());
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
                        // Chosen rows one under another are one lit block.
                        let is = |at: Option<usize>| at.and_then(|at| changed.get(at)).is_some_and(|(p, _)| marked.contains(p));
                        let (chosen, above, below) = (marked.contains(&path), is(ix.checked_sub(1)), is(Some(ix + 1)));
                        let (entity, menu_entity, menu_path) = (entity.clone(), entity.clone(), path.clone());
                        // The box that says the file goes into the commit.
                        let taken = !left_out.contains(&path);
                        let (tick_entity, tick_path) = (entity.clone(), path.clone());
                        let tick = div()
                            .id(("change-tick", ix))
                            .flex_shrink_0()
                            .size(px(15.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if taken { accent } else { quiet.opacity(0.7) })
                            .when(taken, |d| d.bg(accent))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .when(taken, |d| d.child(Icon::new(IconName::Check).with_size(px(10.)).text_color(on_accent)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                swallow_click(window, cx);
                                tick_entity.update(cx, |this, cx| this.commit_tick(tick_path.clone(), cx)).ok();
                            });
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
                            .when(chosen && above, |d| d.rounded_t(px(0.)))
                            .when(chosen && below, |d| d.rounded_b(px(0.)))
                            .when(!chosen, |d| d.hover(move |s| s.bg(hover)))
                            .on_click(move |e: &ClickEvent, window, cx| {
                                // Not the list's too, which lets go.
                                swallow_click(window, cx);
                                entity.update(cx, |this, cx| this.change_click(path.clone(), e.modifiers(), cx)).ok();
                            })
                            .on_mouse_down(MouseButton::Right, move |e: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                menu_entity.update(cx, |this, cx| this.changes_menu(menu_path.clone(), e.position, cx)).ok();
                            })
                            .child(tick)
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

        // Whose lines show: the picked file's, or in the history the
        // picked commit's file's.
        let showing = if c.history { crate::branches::history_key(c) } else { picked.clone() };
        let shown = c.diff.as_ref().filter(|(of, _)| Some(of) == showing.as_ref()).map(|(_, d)| d.clone());
        let rel = picked.as_ref().map(|p| p.strip_prefix(&root).unwrap_or(p).to_string_lossy().replace('\\', "/")).unwrap_or_default();
        // GitHub's own colours for a line put in and a line taken out:
        // the line's ground, and a stronger one behind its number.
        let rgba_of = |hex: u32, a: f32| Hsla::from(rgb(hex)).opacity(a);
        let (add, add_no, del, del_no) = if dark { (rgba_of(0x2ea043, 0.16), rgba_of(0x3fb950, 0.30), rgba_of(0xf85149, 0.14), rgba_of(0xf85149, 0.30)) } else { (Hsla::from(rgb(0xe6ffec)), Hsla::from(rgb(0xccffd8)), Hsla::from(rgb(0xffebe9)), Hsla::from(rgb(0xffd7d5))) };
        let body = match &shown {
            None => div().p(px(20.)).text_size(px(12.5)).text_color(quiet).child(if showing.is_some() { "Reading…" } else if c.history { "" } else { "Nothing has changed since the last commit." }).into_any_element(),
            Some(d) if d.pictures.is_some() => picture_compare(d.pictures.as_ref().unwrap(), &c.pictures, &theme),
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
                // Room under the lines as deep as the sheet's corner, so
                // a line's ground never stands square past its curve. The
                // note of a cut stands there instead when there is one.
                v_flex()
                    .size_full()
                    .when(!d.cut, |el| el.pb(px(15.)))
                    .child(div().relative().flex_1().min_h_0().child(lines).vertical_scrollbar(&c.list))
                    .when(d.cut, |el| el.child(div().flex_shrink_0().px(px(14.)).py(px(8.)).border_t_1().border_color(theme.border).text_size(px(12.)).text_color(quiet).child("Only the beginning of this comparison is shown.")))
                    .into_any_element()
            }
        };
        // Discard is asked of every file chosen; Open and Reveal of the one shown.
        // With none chosen, all three are asked of the file shown.
        let (open, reveal, discard) = (picked.clone().filter(|_| c.marked.len() <= 1), picked.clone().filter(|_| c.marked.len() <= 1), if c.marked.is_empty() { picked.clone().map(|p| vec![p]) } else { Some(c.marked.clone()) });
        let discard_label: SharedString = if c.marked.len() > 1 { format!("Discard {}", c.marked.len()).into() } else { "Discard".into() };
        // The rectangle, over the rows, cut to the list's room.
        let band = c.band.filter(|_| c.band_live).map(|(a, b)| {
            let room = c.files.0.borrow().base_handle.bounds();
            let (left, top) = (a.x.min(b.x).max(room.left()), a.y.min(b.y).max(room.top()));
            let (right, bottom) = (a.x.max(b.x).min(room.right()), a.y.max(b.y).min(room.bottom()));
            div().absolute().left(left - room.left()).top(top - room.top()).w((right - left).max(px(0.))).h((bottom - top).max(px(0.))).rounded(px(3.)).border_1().border_color(theme.primary.opacity(0.7)).bg(theme.primary.opacity(0.12))
        });
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
            // The strip's height over and under the sheet, so it stays in
            // the window's middle: as a share of the height, a low
            // window put the sheet's head under its three buttons.
            .py(crate::workbench::TITLEBAR_H)
            .on_click(cx.listener(|this, _, _, cx| this.close_changes(cx)))
            .child(
                v_flex()
                    .id("changes-sheet")
                    .occlude()
                    // ⌘A chooses every file, while the sheet itself has
                    // the keyboard: in a field of the commit's it is
                    // the field's.
                    .track_focus(&self.changes_focus)
                    // ⌘↩ commits and ⌘P pushes, as in GitHub Desktop,
                    // wherever in the sheet the keyboard is. ⌘↩ is the
                    // window's Send, taken here before the composer's.
                    .on_action(cx.listener(|this, _: &crate::workbench::Send, window, cx| {
                        if this.changes.as_ref().is_some_and(|c| !c.history) {
                            this.commit_go(window, cx);
                        }
                    }))
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                        let keys = &e.keystroke;
                        if keys.key == "p" && keys.modifiers.secondary() && !keys.modifiers.shift && !keys.modifiers.alt {
                            if this.sync_words().is_some_and(|(_, what)| what == Some(false)) {
                                this.sync_go(false, cx);
                            }
                            cx.stop_propagation();
                            return;
                        }
                        if keys.key == "a" && keys.modifiers.secondary() && !keys.modifiers.shift && !keys.modifiers.alt && this.changes_focus.is_focused(window) {
                            let all: Vec<PathBuf> = this.tree.changed.iter().map(|(p, _)| p.clone()).collect();
                            if let Some(c) = &mut this.changes {
                                c.anchor = all.first().cloned();
                                c.marked = all;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        } else if matches!(keys.key.as_str(), "up" | "down") && !keys.modifiers.modified() && this.changes_focus.is_focused(window) && this.changes.as_ref().is_some_and(|c| c.history) {
                            this.history_step(keys.key == "down", cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(gpui::relative(0.96))
                    .h_full()
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
                            // The two things the sheet shows: what is not
                            // committed yet, and what has been.
                            .child(div().flex_shrink_0().child(self.segmented_sized(
                                "changes-tab",
                                vec![("changes", "Changes".to_string(), None), ("history", "History".to_string(), None)],
                                if c.history { "history" } else { "changes" },
                                Rc::new(|this: &mut Self, key, window, cx| {
                                    swallow_click(window, cx);
                                    this.history_show(key == "history", cx);
                                }),
                                true,
                                cx,
                            )))
                            .child(div().flex_shrink_0().text_size(px(12.)).text_color(quiet).child(if c.history { c.commits.as_ref().map(|l| plural(l.len(), "commit", "commits")).unwrap_or_default() } else { plural(changed.len(), "file", "files") }))
                            .when_some(branch, |el, b| el.child(h_flex().min_w_0().gap(px(4.)).items_center().text_size(px(12.)).text_color(quiet).child("on").child(Icon::default().path("icons/git-branch.svg").with_size(px(12.)).flex_shrink_0()).child(div().min_w_0().truncate().text_color(ink).child(b))))
                            .child(div().flex_1())
                            .child(Button::new("changes-close").ghost().small().icon(IconName::Close).on_click(cx.listener(|this, _, _, cx| this.close_changes(cx)))),
                    )
                    .child(if c.history {
                        self.render_history(body, counts, cx)
                    } else {
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .items_stretch()
                            // A clip is a rectangle, so the column's ground is
                            // rounded where the sheet's corner is: square, it
                            // showed past the sheet's own curve.
                            .child(v_flex().w(CHANGES_LIST_W).flex_shrink_0().border_r_1().border_color(theme.border).bg(theme.sidebar).rounded_bl(px(15.)).child(
                                div()
                                    .id("changes-list")
                                    .relative()
                                    .w_full()
                                    .flex_1()
                                    .min_h_0()
                                    .py(px(6.))
                                    // A press anywhere in the list may become a
                                    // rectangle dragged over the files.
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, e: &MouseDownEvent, _, _| {
                                            if let Some(c) = &mut this.changes {
                                                c.band = Some((e.position, e.position));
                                                c.band_live = false;
                                            }
                                        }),
                                    )
                                    // A press on the files gives the sheet the
                                    // keyboard back from a field of the commit's.
                                    .capture_any_mouse_down(cx.listener(|this, _, window, cx| window.focus(&this.changes_focus, cx)))
                                    .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                                        if e.pressed_button == Some(MouseButton::Left) {
                                            this.changes_band_to(e.position, cx);
                                        }
                                    }))
                                    .on_click(cx.listener(|this, _, _, cx| this.changes_let_go(cx)))
                                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.changes_band_end(cx)))
                                    .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.changes_band_end(cx)))
                                    .child(div().relative().size_full().child(files).children(band))
                                    .vertical_scrollbar(&c.files),
                            ).child(self.render_commit_box(cx)))
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
                                            .when_some(discard, |el, p| {
                                                let this = cx.entity().downgrade();
                                                el.child(crate::branches::small_pill("changes-discard", "icons/undo-2.svg", discard_label.clone(), Press::Danger, false, &theme).on_click(move |_, _, cx| {
                                                    let _ = this.update(cx, |this, cx| this.ask_discard(p.clone(), cx));
                                                }))
                                            })
                                            .when_some(open, |el, p| el.child(crate::branches::small_pill("changes-open", "icons/external-link.svg", "Open", Press::Quiet, false, &theme).on_click(move |_, _, _| crate::sys::open_path(&p))))
                                            .when_some(reveal, |el, p| el.child(crate::branches::small_pill("changes-reveal", "icons/folder-open.svg", crate::sys::REVEAL_LABEL, Press::Quiet, false, &theme).on_click(move |_, _, _| crate::sys::reveal_path(&p)))),
                                    )
                                    .child(div().flex_1().min_h_0().child(body)),
                            )
                            .into_any_element()
                    }),
            )
            // Going, it fades where it is.
            .map(|d| match c.closing {
                Some(_) => d.with_animation("changes-out", Animation::new(CHANGES_OUT), |d, t| d.opacity(1. - t)).into_any_element(),
                None => d.into_any_element(),
            })
    }

    // -- the file shown -------------------------------------------------------

    /// The file's pane in the row beside the conversation, or none:
    /// between the tree and the conversation, widening in and narrowing
    /// out as the panels at its sides do.
    pub(crate) fn render_file_pane(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.page != Page::Session || self.detail.is_none() || self.fold_file {
            return None;
        }
        let live = self.file_anim.filter(|(_, at, _)| at.elapsed() < FILE_ANIM);
        let (v, leaving) = match (&self.file_view, &self.file_gone, live) {
            (Some(v), _, _) => (v, false),
            (None, Some(v), Some((false, _, _))) => (v, true),
            _ => return None,
        };
        let w = self.file_pane_w();
        let serial = self.file_anim.map(|(_, _, n)| n).unwrap_or(0);
        let play = live.map(|(opening, _, _)| opening);
        Some(
            div()
                .h_full()
                .flex_shrink_0()
                .overflow_hidden()
                .child(self.file_pane(v, w, leaving, cx))
                .with_animation(ElementId::Name(format!("file-pane-{serial}").into()), Animation::new(FILE_ANIM).with_easing(ease_out_quint()), move |d, t| match play {
                    Some(opening) => {
                        let t = if opening { t } else { 1. - t };
                        d.w((w * t).round()).opacity(t)
                    }
                    None => d,
                })
                .into_any_element(),
        )
    }

    /// The pane: a head naming the file, with what can be done with it,
    /// over the file as the window can show it.
    fn file_pane(&self, v: &FileView, w: Pixels, leaving: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let name = v.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let folder = v.rel.strip_suffix(&name).unwrap_or("").trim_end_matches(['/', '\\']).to_string();
        let (open, open_too, reveal, more) = (v.path.clone(), v.path.clone(), v.path.clone(), v.path.clone());
        let big = self.file_big;
        // A table's grid runs to the pane's edges.
        let grid = (matches!(v.body, FileBody::Table(..)) && !self.file_raw) || matches!(v.body, FileBody::Sheets(_));
        let (pad_x, pad_y) = if grid { (0., 0.) } else { (18., 16.) };
        // A PDF's pages give the list beside them its room.
        let beside = if matches!(v.body, FileBody::Pages(Some(_))) && self.pdf_side.is_some() { PDF_SIDE_W } else { 0. };
        let room = (f32::from(w) - beside - 2. * pad_x - 1.).max(120.);
        if matches!(v.body, FileBody::Pages(_)) && !leaving {
            self.pdf_want.set(room * self.ui_scale);
        }
        let quiet = |words: String| div().text_size(px(12.)).text_color(theme.muted_foreground).child(words);
        let mut wide = false;
        let mut heads: Option<(AnyElement, AnyElement)> = None;
        let markdown = matches!(v.body, FileBody::Markdown(_));
        let pdf = matches!(v.body, FileBody::Pages(Some(_)));
        let body: AnyElement = match &v.body {
            // Code, and markdown as it is written, are the editor's.
            FileBody::Markdown(_) if self.file_raw => div().into_any_element(),
            FileBody::Code(..) => div().into_any_element(),
            FileBody::Markdown(text) => {
                // A YAML header is the file's fields, not its first
                // words: markdown reads it as a rule and a heading. It
                // is drawn as a table in the same text, so a selection
                // runs through it as through the rest.
                let words = match emaki_core::files::front_matter(text) {
                    Some((fields, rest)) => format!("{}\n{rest}", emaki_core::files::front_matter_table(&fields)),
                    None => text.clone(),
                };
                div().text_size(px(14.)).line_height(relative(1.6)).child(crate::transcript::md_view(format!("file-{}", v.rel), words, cx)).into_any_element()
            }
            FileBody::Notebook(text) => div().text_size(px(14.)).line_height(relative(1.6)).child(crate::transcript::md_view(format!("file-{}", v.rel), text.clone(), cx)).into_any_element(),
            // The web view stands over the pane's room; nothing of the
            // page is drawn under it.
            FileBody::Html(_) if self.html_web.is_some() => div().into_any_element(),
            FileBody::Html(text) => {
                if !leaving {
                    self.html_want.set(room);
                }
                match self.html_shot.as_ref().filter(|(key, _)| key.0 == v.path) {
                    // The page as drawn: its strips one under another
                    // with nothing between, and over them a place a
                    // link, which goes where a link out of the app goes.
                    Some((_, shot)) => {
                        let by = room / shot.css_w.max(1.);
                        let strips = shot.strips.iter().map(|(image, rows)| img(ImageSource::Image(image.clone())).w(px(room)).h(px(room * *rows as f32 / shot.px_w.max(1) as f32)).flex_shrink_0());
                        let links = shot.links.iter().enumerate().filter(|(_, (href, _))| ["http://", "https://", "mailto:"].iter().any(|s| href.starts_with(s))).map(|(ix, (href, at))| {
                            let href = href.clone();
                            div().id(("html-link", ix)).absolute().left(px(at[0] * by)).top(px(at[1] * by)).w(px(at[2] * by)).h(px(at[3] * by)).cursor_pointer().on_click(move |_, window, cx| {
                                swallow_click(window, cx);
                                cx.open_url(&href)
                            })
                        });
                        v_flex()
                            .gap(px(10.))
                            .child(v_flex().relative().w(px(room)).bg(gpui::white()).border_1().border_color(theme.border).overflow_hidden().children(strips).children(links))
                            .when(shot.more, |d| d.child(quiet("The page goes on past here.".into())))
                            .into_any_element()
                    }
                    // Until it is drawn, its words as the toolkit reads
                    // them.
                    None => div().text_size(px(14.)).line_height(relative(1.6)).child(gpui_component::text::TextView::html(SharedString::from(format!("file-html-{}-{}", v.rel, self.file_serial)), text.clone()).selectable(true)).into_any_element(),
                }
            }
            FileBody::Picture(size, drawn) => {
                // At its own size when that fits, never larger.
                let (pw, ph) = match size {
                    Some((iw, ih)) if *iw > 0 && *ih > 0 => {
                        let pw = room.min(*iw as f32);
                        (pw, pw * *ih as f32 / *iw as f32)
                    }
                    _ => (room, room * 0.75),
                };
                v_flex().items_center().gap(px(8.)).child(match drawn { Some(picture) => img(picture.clone()), None => img(v.path.clone()) }.w(px(pw)).h(px(ph)).object_fit(ObjectFit::Contain).rounded(px(6.))).children(size.map(|(iw, ih)| quiet(format!("{iw} × {ih}")))).into_any_element()
            }
            // The document's own pane stands in the body's place.
            FileBody::Word if self.word.is_some() => div().into_any_element(),
            FileBody::Word => v_flex().py(px(60.)).items_center().child(quiet("Opening the document…".into())).into_any_element(),
            FileBody::Pages(None) => v_flex().py(px(60.)).items_center().child(quiet("Drawing the pages…".into())).into_any_element(),
            FileBody::Pages(Some(doc)) => {
                let (sel, hits, on) = (self.pdf_sel, std::rc::Rc::new(self.pdf_hits.clone()), self.pdf_hits.get(self.pdf_hit).copied());
                let showing = self.pdf_find_open;
                let under = self.pdf_under.as_ref().map(|(old, _)| old.clone());
                let (ink_sel, ink_hit, ink_on) = (gpui::rgba(0x2f7cf655), gpui::rgba(0xffd43b66), gpui::rgba(0xff922bb0));
                v_flex()
                    .gap(px(12.))
                    .items_center()
                    .children(doc.pages.iter().enumerate().map(|(ix, page)| {
                        let (doc, places, hits) = (doc.clone(), self.pdf_bounds.clone(), hits.clone());
                        let tall = room * page.px.1 as f32 / page.px.0.max(1) as f32;
                        div()
                            .id(("pdf-page", ix))
                            .relative()
                            .w(px(room))
                            .h(px(tall))
                            .flex_shrink_0()
                            .border_1()
                            .border_color(theme.border)
                            .bg(gpui::white())
                            .cursor(CursorStyle::IBeam)
                            .children(under.as_ref().and_then(|old| old.pages.get(ix)).map(|old| img(ImageSource::Image(old.image.clone())).absolute().top_0().left_0().size_full()))
                            .child(img(ImageSource::Image(page.image.clone())).absolute().top_0().left_0().size_full())
                            // Over the picture: where the page is, kept for
                            // the mouse, and the marks of what is selected
                            // and found, a line at a time.
                            .child(
                                canvas(
                                    move |bounds, _, _| {
                                        let mut places = places.borrow_mut();
                                        if places.len() <= ix {
                                            places.resize(ix + 1, Bounds::default());
                                        }
                                        places[ix] = bounds;
                                    },
                                    move |bounds, _, window, _| {
                                        let page = &doc.pages[ix];
                                        let scale = f32::from(bounds.size.width) / page.unit.0.max(1.);
                                        let mut mark = |from: usize, to: usize, ink: gpui::Rgba| {
                                            let (from, to) = (from.max(page.glyphs.start), to.min(page.glyphs.end.saturating_sub(1)));
                                            if page.glyphs.is_empty() || from > to {
                                                return;
                                            }
                                            let mut run: Option<(f32, f32, f32, f32)> = None;
                                            let mut flush = |run: &mut Option<(f32, f32, f32, f32)>| {
                                                if let Some((x0, x1, base, em)) = run.take() {
                                                    let at = bounds.origin + point(px(x0 * scale), px((base - em * 0.85) * scale));
                                                    window.paint_quad(fill(Bounds::new(at, size(px((x1 - x0) * scale), px(em * 1.15 * scale))), ink));
                                                }
                                            };
                                            for g in &doc.glyphs[from..=to] {
                                                match &mut run {
                                                    Some((_, x1, base, em)) if g.gap != 2 && (g.base - *base).abs() < *em * 0.5 => {
                                                        *x1 = x1.max(g.x1);
                                                        *em = em.max(g.em);
                                                    }
                                                    _ => {
                                                        flush(&mut run);
                                                        run = Some((g.x0, g.x1, g.base, g.em));
                                                    }
                                                }
                                            }
                                            flush(&mut run);
                                        };
                                        if showing {
                                            for (from, to) in hits.iter() {
                                                mark(*from, *to, if on == Some((*from, *to)) { ink_on } else { ink_hit });
                                            }
                                        }
                                        if let Some((from, to)) = sel {
                                            mark(from, to, ink_sel);
                                        }
                                    },
                                )
                                .absolute()
                                .inset_0(),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                                    this.pdf_mouse_down(e, window, cx);
                                    swallow_click(window, cx);
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.pdf_menu(e.position, window, cx);
                                }),
                            )
                    }))
                    .child(quiet(if doc.total > doc.pages.len() { format!("The first {} of {} pages. Open the file for the rest.", doc.pages.len(), doc.total) } else { plural(doc.total, "page", "pages") }))
                    .into_any_element()
            }
            FileBody::Table(rows, widths, more) => {
                wide = true;
                heads = Some(self.render_table_heads(v, rows.len(), widths, cx));
                self.render_table(v, rows, widths, *more, cx)
            }
            FileBody::Doc(text) => div().text_size(px(14.)).line_height(relative(1.6)).child(crate::transcript::md_view(format!("file-{}", v.rel), text.clone(), cx)).into_any_element(),
            FileBody::Sheets(sheets) => match sheets.get(self.sheet_of(&v.path, sheets.len())) {
                Some((_, rows, widths, more)) if !rows.is_empty() => {
                    wide = true;
                    heads = Some(self.render_table_heads(v, rows.len(), widths, cx));
                    self.render_table(v, rows, widths, *more, cx)
                }
                _ => div().p(px(18.)).child(quiet("This sheet is empty.".into())).into_any_element(),
            },
            FileBody::None(why) => v_flex()
                .py(px(60.))
                .gap(px(12.))
                .items_center()
                .text_color(theme.muted_foreground)
                .child(Icon::default().path(crate::assets::file_icon_path(&name)).with_size(px(36.)))
                .child(div().text_size(px(13.)).text_center().child(why.clone()))
                .child(choice_pill("file-open-default", "icons/external-link.svg", "Open with default app", Press::Quiet, false, &theme).on_click(move |_, _, _| crate::sys::open_path(&open_too)))
                .into_any_element(),
        };
        // Code, and markdown or a table as it is written: the toolkit's
        // editor, with line numbers and the language's colours. It
        // finds (⌘F), selects and copies by itself, and a file the pane
        // holds whole can be changed in it and saved (⌘S, `save_file`).
        // What shows two ways, as it reads and as it is written: markdown
        // and HTML, and a table or an SVG the pane holds whole.
        let table = matches!(v.body, FileBody::Table(..)) && v.source.is_some();
        let two_ways = markdown || table || matches!(v.body, FileBody::Html(_) | FileBody::Notebook(_)) || (matches!(v.body, FileBody::Picture(..)) && v.source.is_some());
        let code = matches!(v.body, FileBody::Code(..)) || (two_ways && self.file_raw);
        let dirty = !leaving && self.file_dirty(cx);
        let editor = self.file_editor.as_ref().filter(|_| code && !leaving).map(|ed| {
            let (state, readonly) = (&ed.state, ed.saved.is_none());
            let menu = {
                let (this, state) = (cx.entity().downgrade(), state.clone());
                move |at: Point<Pixels>, window: &mut Window, cx: &mut App| {
                    let (this, state) = (this.clone(), state.clone());
                    // Asked from inside the editor's own update.
                    window.defer(cx, move |window, cx| {
                        // With nothing selected, the word under the
                        // pointer is, as a conversation's is, so the
                        // menu has Copy for it.
                        let selection = state.update(cx, |s, cx| s.select_word_at(at, window, cx));
                        let focus = gpui::Focusable::focus_handle(state.read(cx), cx);
                        let _ = this.update(cx, |this, cx| {
                            let mut items: Vec<(&'static str, MenuDo)> = Vec::new();
                            if selection {
                                items.push(("Copy", MenuDo::Edit(crate::workbench::EditDo::Copy, focus.clone())));
                            }
                            items.push(("Select All", MenuDo::Edit(crate::workbench::EditDo::SelectAll, focus)));
                            // The text's own two and no more: what is
                            // asked of the file is on its row's menu.
                            this.open_menu(at, items, cx);
                        });
                    });
                }
            };
            gpui_component::input::Editor::new(state).readonly(readonly).appearance(false).bordered(false).h_full().font_family(theme.mono_font_family.clone()).text_size(px(crate::workbench::editor_px(&self.cfg.app.chat_size))).on_secondary_click(menu).into_any_element()
        });
        let word = self.word.as_ref().filter(|w| matches!(v.body, FileBody::Word) && !leaving && w.read(cx).path == v.path).map(|w| w.clone().into_any_element());
        let cut = match &v.body {
            FileBody::Code(_, _, dropped) if *dropped > 0 => Some(div().flex_shrink_0().px(px(pad_x)).py(px(8.)).border_t_1().border_color(theme.border).child(quiet(format!("Only the first {PREVIEW_LINES} lines are shown. Open the file for the rest.")))),
            _ => None,
        };
        // What the file is drawn again as, so a new one fades in.
        let body = div().child(body).with_animation(ElementId::Name(format!("file-body-{}", self.file_serial).into()), Animation::new(FILE_ANIM), |d, t| d.opacity(t));
        let tool = |id: &'static str, icon: Icon, tip: &'static str| Button::new(id).ghost().small().icon(icon).tooltip(tip);
        // Two segments in the head, as the files and the outline have
        // in the strip, smaller: for markdown how it shows, for a PDF
        // what stands beside its pages. `on` is the first segment, the
        // second, or neither; `was` what it was before the last press.
        let pair = |key: &'static str, on: Option<bool>, was: Option<bool>, serial: u64, first: (&'static str, &'static str), second: (&'static str, &'static str), press: fn(&mut Self, bool, &mut Context<Self>), cx: &mut Context<Self>| {
            let dark = theme.mode.is_dark();
            let (seg_w, seg_h, pad, gap) = (26., 20., 2., 2.);
            let x_of = |first: bool| pad + if first { 0. } else { seg_w + gap };
            let (from, to) = if serial == 0 { (on, on) } else { (was, on) };
            let plate = (from.is_some() || to.is_some()).then(|| {
                let (a, b) = (x_of(from.or(to).unwrap_or(true)), x_of(to.or(from).unwrap_or(true)));
                let (o_a, o_b) = (if from.is_some() { 1. } else { 0. }, if to.is_some() { 1. } else { 0. });
                div().absolute().top(px(pad)).w(px(seg_w)).h(px(seg_h)).rounded(px(6.)).bg(if dark { theme.secondary_active } else { theme.popover }).shadow_sm().with_animation(
                    ElementId::Name(format!("{key}-plate-{serial}").into()),
                    Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                    move |d, t| d.left(px(a + (b - a) * t)).opacity(o_a + (o_b - o_a) * t),
                )
            });
            let mut row = h_flex().relative().flex_shrink_0().p(px(pad)).gap(px(gap)).rounded(px(8.)).bg(if dark { theme.tab_bar_segmented } else { theme.muted }).children(plate);
            for (which, (icon, tip)) in [(true, first), (false, second)] {
                row = row.child(
                    h_flex()
                        .id(SharedString::from(format!("{key}-{which}")))
                        .w(px(seg_w))
                        .h(px(seg_h))
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_color(if on == Some(which) { theme.foreground } else { theme.muted_foreground })
                        .hover(|s| s.text_color(theme.foreground))
                        .managed_tooltip(move |window, cx| crate::workbench::tip(tip, window, cx))
                        .on_click(cx.listener(move |this, _, _, cx| press(this, which, cx)))
                        .child(Icon::default().path(icon).with_size(px(13.))),
                );
            }
            row
        };
        let modes = if leaving {
            None
        } else if two_ways {
            Some(pair("file-mode", Some(!self.file_raw), Some(self.file_raw), self.file_mode_serial, (if table { "icons/file-table.svg" } else { "icons/eye.svg" }, if table { "Table" } else { "Rendered" }), ("icons/file-code.svg", "Raw"), |this, first, cx| this.set_file_raw(!first, cx), cx))
        } else if pdf {
            let (was, serial) = self.pdf_side_anim.map(|(was, _, n)| (was, n)).unwrap_or((self.pdf_side, 0));
            Some(pair("pdf-side", self.pdf_side, was, serial, ("icons/gallery-vertical-end.svg", "Pages"), ("icons/list-bullets.svg", "Contents"), |this, first, cx| this.pdf_side_toggle(first, cx), cx))
        } else {
            None
        };
        // Whether the head's row is the narrow one: what its parts take
        // without the name (its padding, the file's icon, Save, the two
        // segments, four buttons of 24, the gaps of 7 between) leaves the
        // name less than `HEAD_NAME_ROOM`.
        let gap = 7.;
        let parts = |save: f32, buttons: f32| 12. + 6. + 15. + gap + if dirty { save + gap } else { 0. } + if modes.is_some() { 58. + gap } else { 0. } + buttons * (24. + gap);
        let compact = !leaving && f32::from(w) < parts(68., 4.) + HEAD_NAME_ROOM;
        // The size (a PDF's page) is said where it and the whole name fit
        // in the room the words share, and left out, never cut, where
        // they do not: the name comes first. Their widths are their
        // letters' at the head's sizes, a little over.
        let words_room = f32::from(w) - if compact { parts(24., 3.) } else { parts(68., 4.) };
        let size_text = match &v.body {
            FileBody::Pages(Some(doc)) => format!("page {} of {}", self.pdf_page_now() + 1, doc.total),
            _ => human_size(v.size),
        };
        let size_said = words_room >= name.chars().count() as f32 * 7.4 + gap + size_text.chars().count() as f32 * 6.4;
        // Beside a PDF's pages: the pages small, or the file's table of
        // contents. A click goes to the page; the one in view is marked.
        let side_live = self.pdf_side_anim.filter(|(_, at, _)| at.elapsed() < FILE_ANIM);
        let side_what = self.pdf_side.or(side_live.and_then(|(was, _, _)| was));
        let side = match (&v.body, side_what) {
            (FileBody::Pages(Some(doc)), Some(pages)) if !leaving => {
                let now = self.pdf_page_now();
                let list: AnyElement = if pages {
                    v_flex()
                        .gap(px(10.))
                        .items_center()
                        .children(doc.pages.iter().enumerate().map(|(ix, page)| {
                            let w = PDF_SIDE_W - 36.;
                            v_flex()
                                .id(("pdf-thumb", ix))
                                .p(px(5.))
                                .gap(px(3.))
                                .items_center()
                                .rounded(px(8.))
                                .cursor_pointer()
                                .when(ix == now, |d| d.bg(theme.primary.opacity(0.16)))
                                .when(ix != now, |d| d.hover(|s| s.bg(theme.muted)))
                                .on_click(cx.listener(move |this, _, _, cx| this.pdf_go_page(ix, cx)))
                                .child(img(ImageSource::Image(page.thumb.clone())).w(px(w)).h(px(w * page.px.1 as f32 / page.px.0.max(1) as f32)).rounded(px(3.)).border_1().border_color(theme.border).bg(gpui::white()))
                                .child(div().text_size(px(10.5)).text_color(if ix == now { theme.foreground } else { theme.muted_foreground }).child((ix + 1).to_string()))
                        }))
                        .into_any_element()
                } else if doc.contents.is_empty() {
                    div().px(px(10.)).py(px(14.)).text_size(px(11.5)).text_color(theme.muted_foreground).child("This file has no table of contents.").into_any_element()
                } else {
                    // The heading the page in view is under: the last one at or before it.
                    let here = doc.contents.iter().rposition(|(_, page, _)| page.is_some_and(|p| p <= now));
                    v_flex()
                        .gap(px(1.))
                        .children(doc.contents.iter().enumerate().map(|(ix, (title, page, depth))| {
                            let page = *page;
                            div()
                                .id(("pdf-heading", ix))
                                .w_full()
                                .pl(px(8. + 9. * *depth as f32))
                                .pr(px(6.))
                                .py(px(4.))
                                .rounded(px(6.))
                                .text_size(px(11.5))
                                .line_height(px(15.))
                                .text_color(if Some(ix) == here { theme.foreground } else { theme.muted_foreground })
                                .when(Some(ix) == here, |d| d.bg(theme.primary.opacity(0.16)).font_weight(FontWeight::MEDIUM))
                                .when(page.is_some(), |d| d.cursor_pointer().hover(|s| s.bg(theme.muted).text_color(theme.foreground)))
                                .when_some(page, |d, page| d.on_click(cx.listener(move |this, _, _, cx| this.pdf_go_page(page, cx))))
                                .child(title.clone())
                        }))
                        .into_any_element()
                };
                let serial = self.pdf_side_anim.map(|(_, _, n)| n).unwrap_or(0);
                // (widens or narrows, comes or goes)
                let play = side_live.map(|(was, _, _)| (was.is_none(), self.pdf_side.is_some()));
                self.inner_scroller(&self.pdf_side_scroll, crate::workbench::Inner::Held);
                Some(
                    div()
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(div().relative().h_full().child(div().id("pdf-side").w(px(PDF_SIDE_W)).h_full().border_r_1().border_color(theme.border).bg(theme.muted.opacity(0.25)).overflow_y_scroll().track_scroll(&self.pdf_side_scroll).px(px(6.)).py(px(8.)).child(list)).vertical_scrollbar(&self.pdf_side_scroll))
                        .with_animation(ElementId::Name(format!("pdf-side-{serial}").into()), Animation::new(FILE_ANIM).with_easing(ease_out_quint()), move |d, t| match play {
                            Some((wide, comes)) if wide || !comes => {
                                let t = if comes { t } else { 1. - t };
                                d.w(px((PDF_SIDE_W * t).round())).opacity(t)
                            }
                            _ => d,
                        }),
                )
            }
            _ => None,
        };
        // A PDF's own find, under the head while it is asked for.
        let find = (pdf && self.pdf_find_open && !leaving).then(|| {
            let focus = self.pdf_find_input.read(cx).focus_handle(cx);
            let value = self.pdf_find_input.read(cx).value().to_string();
            let n = self.pdf_hits.len();
            let count = if value.trim().is_empty() { String::new() } else if n == 0 { "no matches".into() } else { format!("{} of {n}", self.pdf_hit + 1) };
            h_flex()
                .id("file-find")
                .key_context(crate::workbench::FIND_CONTEXT)
                .h(px(34.))
                .flex_shrink_0()
                .pl(px(12.))
                .pr(px(6.))
                .gap(px(6.))
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(Icon::new(IconName::Search).with_size(px(13.)).text_color(theme.muted_foreground))
                .child(div().id("file-find-field").track_focus(&focus).flex_1().min_w_0().text_size(px(12.5)).child(gpui_component::input::Input::new(&self.pdf_find_input).appearance(false).bordered(false).on_secondary_click(self.input_menu(&self.pdf_find_input, false, cx))))
                .child(div().flex_shrink_0().text_size(px(11.)).text_color(if n == 0 && !value.trim().is_empty() { theme.danger } else { theme.muted_foreground }).child(count))
                .child(tool("file-find-prev", Icon::new(IconName::ChevronUp), "Previous (⇧↩)").on_click(cx.listener(|this, _, _, cx| this.pdf_find_step(-1, cx))))
                .child(tool("file-find-next", Icon::new(IconName::ChevronDown), "Next (↩)").on_click(cx.listener(|this, _, _, cx| this.pdf_find_step(1, cx))))
                .child(tool("file-find-close", Icon::new(IconName::Close), "Close (esc)").on_click(cx.listener(|this, _, window, cx| this.pdf_find_close(window, cx))))
        });
        // A workbook's sheets, by name, in a row over the grid: the one
        // showing on a plate, as a segment is.
        let sheets = match &v.body {
            FileBody::Sheets(all) if all.len() > 1 && !leaving => {
                let now = self.sheet_of(&v.path, all.len());
                Some(
                    h_flex()
                        .id("file-sheets")
                        .h(px(32.))
                        .flex_shrink_0()
                        .px(px(8.))
                        .gap(px(2.))
                        .items_center()
                        .border_b_1()
                        .border_color(theme.border)
                        .overflow_x_scroll()
                        .children(all.iter().enumerate().map(|(ix, (name, ..))| {
                            let path = v.path.clone();
                            div()
                                .id(("file-sheet", ix))
                                .h(px(22.))
                                .px(px(9.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .rounded(px(6.))
                                .cursor_pointer()
                                .text_size(px(12.))
                                .when(ix == now, |d| d.bg(theme.muted).font_weight(FontWeight::MEDIUM).text_color(theme.foreground))
                                .when(ix != now, |d| d.text_color(theme.muted_foreground).hover(|s| s.text_color(theme.foreground)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    swallow_click(window, cx);
                                    this.sheet_at = Some((path.clone(), ix));
                                    this.table_at = None;
                                    this.file_serial += 1;
                                    this.file_view_scroll.set_offset(point(px(0.), px(0.)));
                                    cx.notify();
                                }))
                                .child(name.clone())
                        }))
                        .with_animation(ElementId::Name(format!("file-sheets-{}", v.path.display()).into()), Animation::new(FILE_ANIM), |d, t| d.opacity(t)),
                )
            }
            _ => None,
        };
        let scroller = v_flex().id("file-body").flex_1().min_h_0().px(px(pad_x)).py(px(pad_y)).track_scroll(&self.file_view_scroll).when(!pdf && !leaving, |d| {
            // A PDF's pages and a table's cells have menus of their own.
            d.on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, window, cx| this.file_body_menu(e.position, window, cx)))
        });
        v_flex()
            .id("file-pane")
            .w(w)
            .h_full()
            .flex_shrink_0()
            .bg(theme.background)
            .border_r_1()
            .border_color(theme.border)
            .when(!leaving, |d| d.track_focus(&self.file_focus))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                this.pdf_key(e, cx);
                this.table_key(e, window, cx);
            }))
            // A table's heads stay put: they are drawn where the scroll is.
            .when(grid, |d| d.on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify())))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.pdf_mouse_move(e, cx)))
            // A drag over a table's cells ends where the button is let go.
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.table_drag = false))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.table_drag = false))
            // The page in view is said in the head and marked in the list.
            .when(pdf, |d| d.on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify())))
            .on_click(|_, window, cx| swallow_click(window, cx))
            .child(
                h_flex()
                    .h(HEAD_H)
                    .flex_shrink_0()
                    .pl(px(12.))
                    .pr(px(6.))
                    .gap(px(7.))
                    .items_center()
                    .bg(theme.muted.opacity(0.3))
                    .border_t_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(img(crate::file_icons::path(&name, false, false, theme.mode.is_dark())).size(px(15.)).flex_shrink_0())
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(7.))
                            .items_baseline()
                            // The name before the folder, which takes what
                            // is left; with no folder to say, all of it.
                            .child(div().flex_shrink_0().when(!folder.is_empty(), |d| d.max_w(relative(0.6))).max_w_full().truncate().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(name.clone()))

                            .child(div().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(10.5)).text_color(theme.muted_foreground).child(folder))
                            // The size is the file's on disk: not said of one
                            // with changes not saved, where Save stands instead.
                            .when(!dirty && size_said, |d| d.child(div().flex_shrink_0().text_size(px(10.5)).text_color(theme.muted_foreground).child(size_text))),
                    )
                    .when(dirty, |d| {
                        let save = |this: &mut Self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>| {
                            swallow_click(window, cx);
                            this.save_file(window, cx)
                        };
                        let button = if compact {
                            Button::new("file-save").ghost().small().icon(Icon::default().path("icons/device-floppy.svg")).on_click(cx.listener(save)).into_any_element()
                        } else {
                            crate::branches::small_pill("file-save", "icons/device-floppy.svg", "Save", Press::Quiet, false, &theme).on_click(cx.listener(save)).into_any_element()
                        };
                        d.child(
                            div().id("file-save-tip").flex_shrink_0().managed_tooltip(|window, cx| crate::workbench::tip("Save (⌘S)", window, cx)).child(button)
                            .with_animation(if compact { "file-save-in-compact" } else { "file-save-in" }, Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t)),
                        )
                    })
                    .children(modes)
                    .when(!leaving, |d| {
                        // In a narrow pane Open and Reveal are one More
                        // button's list, the two the file's row in the
                        // tree has; they fade in as they change places.
                        let ways = if compact {
                            let path = more.clone();
                            h_flex().child(tool("file-more", Icon::new(IconName::Ellipsis), "More").on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                                swallow_click(window, cx);
                                this.open_menu(e.position(), vec![("Open", MenuDo::File(FileDo::Open, path.clone())), (crate::sys::REVEAL_LABEL, MenuDo::File(FileDo::Reveal, path.clone()))], cx);
                            })))
                        } else {
                            h_flex()
                                .gap(px(7.))
                                .child(tool("file-open", Icon::default().path("icons/external-link.svg"), "Open with default app").on_click(move |_, _, _| crate::sys::open_path(&open)))
                                .child(tool("file-reveal", Icon::default().path("icons/folder-open.svg"), crate::sys::REVEAL_LABEL).on_click(move |_, _, _| crate::sys::reveal_path(&reveal)))
                        };
                        d.child(ways.flex_shrink_0().with_animation(if compact { "file-ways-compact" } else { "file-ways" }, Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t)))
                        .child(tool("file-enlarge", Icon::default().path(if big { "icons/minimize.svg" } else { "icons/maximize.svg" }), if big { "Restore" } else { "Enlarge" }).on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.file_enlarge(window, cx)
                        })))
                        // The close button, as an editor's tab has it: with
                        // changes not saved it is an orange dot, and the
                        // cross again under the pointer.
                        .child({
                            let hover_bg = theme.secondary_hover;
                            div()
                                .id("file-close")
                                .group("file-close")
                                .relative()
                                .size(px(24.))
                                .flex_shrink_0()
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .managed_tooltip(move |window, cx| crate::workbench::tip(if dirty { "Close (⌘W), not saved" } else { "Close (⌘W)" }, window, cx))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    swallow_click(window, cx);
                                    this.close_file_view(cx)
                                }))
                                .child(div().flex().items_center().justify_center().when(dirty, |d| d.opacity(0.).group_hover("file-close", |s| s.opacity(1.))).child(Icon::new(IconName::Close).with_size(px(14.))))
                                .when(dirty, |d| {
                                    d.child(div().absolute().inset_0().flex().items_center().justify_center().group_hover("file-close", |s| s.opacity(0.)).child(
                                        div().size(px(8.)).rounded_full().bg(unsaved_dot()).with_animation("file-dirty-in", Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t)),
                                    ))
                                })
                        })
                    }),
            )
            .children(find)
            .children(sheets)
            .child(
                h_flex().flex_1().min_h_0().items_stretch().children(side).child(
                    v_flex()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        // The web view is put over this room as the room
                        // is laid out, so it moves in the frame the pane
                        // does.
                        .when_some(self.html_web.as_ref().filter(|_| !leaving && matches!(v.body, FileBody::Html(_)) && !self.file_raw).map(|web| web.placer()), |d, place| {
                            d.child(canvas(move |bounds, _, _| place(bounds), |_, _, _, _| {}).absolute().inset_0())
                        })
                        .map(|d| match (word, editor) {
                            // The document's pane scrolls itself too.
                            (Some(word), _) => d.child(div().flex_1().min_h_0().child(word)),
                            // The editor scrolls itself; the wrapper only
                            // says where the pane's body is.
                            (None, Some(editor)) => d.child(div().id("file-body").flex_1().min_h_0().track_scroll(&self.file_view_scroll).pt(px(6.)).font_family(theme.mono_font_family.clone()).text_size(px(12.5)).child(editor)).children(cut),
                            // What is wider than the pane keeps its own
                            // width: stretched across the scroller, as a
                            // column's child is, it was as wide as the
                            // pane to the scroller and never scrolled
                            // sideways.
                            (None, None) => match heads {
                                // A grid: the letters over the cells'
                                // scroller and the numbers at its left,
                                // outside it (`render_table_heads`).
                                Some((top, side)) => d.child(top).child(h_flex().flex_1().min_h_0().items_stretch().child(side).child(v_flex().relative().flex_1().min_w_0().min_h_0().child(scroller.items_start().overflow_scroll().child(body)).vertical_scrollbar(&self.file_view_scroll))),
                                None => d.child(if wide { scroller.items_start().overflow_scroll().child(body) } else { scroller.overflow_y_scroll().child(body) }).vertical_scrollbar(&self.file_view_scroll),
                            },
                        }),
                ),
            )
            .into_any_element()
    }

    // -- the outline ----------------------------------------------------------

    /// An entry was chosen, by a click: it is the one
    /// marked, and the conversation moves to its round, which ends at
    /// the top of the view. The move is drawn (`glide`), a step each
    /// tick until it is there; choosing another entry meanwhile takes
    /// the move over, and a wheel in the conversation ends it.
    pub(crate) fn outline_go(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.outline_pick = Some(ix);
        self.outline_pick_end = None;
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
                            // Ended on the list's end, the round is as near
                            // the top as it gets, and the list says its top
                            // is past the last round: the entry is kept on
                            // that word, and the list is left to follow.
                            if list.logical_scroll_top().item_ix >= list.item_count() {
                                this.outline_pick_end = Some(list.item_count());
                            } else if !list.is_following_tail() {
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

    /// Ask a small model for a label on each of those entries, off the
    /// main thread. What comes back is
    /// kept for good; what does not is left as the message's own words
    /// until the next launch.
    fn ask_labels(&mut self, asked: Vec<(String, String)>, cx: &mut Context<Self>) {
        for (key, _) in &asked {
            self.outline_asking.insert(key.clone());
        }
        let cfg = self.hub.explainer.cfg();
        // The labels are the session's own agent's to write.
        let Some(agent) = self.detail.as_ref().map(|d| d.session.agent) else { return };
        cx.spawn(async move |this, cx| {
            let prompts: Vec<String> = asked.iter().map(|(_, p)| p.clone()).collect();
            let lines = cx.background_executor().spawn(async move { outline::summarize(&cfg, agent, &prompts) }).await;
            let saved = this
                .update(cx, |this, cx| {
                    for ((key, _), line) in asked.into_iter().zip(lines) {
                        this.outline_asking.remove(&key);
                        match line {
                            Some(line) => {
                                this.outline_fresh.insert(key.clone(), std::time::Instant::now());
                                this.outline_labels.insert(key, line.into());
                            }
                            None => {
                                this.outline_failed.insert(key);
                            }
                        }
                    }
                    cx.notify();
                    this.outline_labels.clone()
                })
                .ok();
            if let Some(mut labels) = saved {
                cx.background_executor().spawn(async move { outline::save_labels(&mut labels) }).await;
            }
        })
        .detach();
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
        let count = d.list.item_count();
        let held = top >= count && self.outline_pick_end == Some(count);
        if !self.outline_gliding && !held && self.outline_pick.is_some_and(|ix| !(top == ix || (at_end && top <= ix))) {
            self.outline_pick = None;
            self.outline_pick_end = None;
        }
        let current = self.outline_pick.unwrap_or(if at_end { entries.len().saturating_sub(1) } else { top }).min(entries.len().saturating_sub(1));

        let waiting = self.outline_asking.len();
        // The entries with no label yet, and which child of the scroller
        // each is, for asking about the ones in sight.
        let mut unlabelled: Vec<(usize, usize, String, String)> = Vec::new();
        // The day heads, and which child of the scroller each is.
        // with the room over its words.
        let mut heads: Vec<(usize, f32, String)> = Vec::new();

        let dark = theme.mode.is_dark();
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut current_child = 0;
        let mut day = String::new();
        for e in entries.iter() {
            let this_day = day_label(&e.ts);
            if this_day != day {
                day = this_day.clone();
                if !this_day.is_empty() {
                    let lead = if rows.is_empty() { 4. } else { 14. };
                    heads.push((rows.len(), lead, this_day.to_uppercase()));
                    rows.push(div().flex_shrink_0().px(px(10.)).pt(px(lead)).pb(px(4.)).text_size(px(10.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground.opacity(0.8)).child(this_day.to_uppercase()).into_any_element());
                }
            }
            let is_current = e.round == current;
            if is_current {
                current_child = rows.len();
            }
            let quiet = matches!(e.kind, Kind::Command | Kind::Compact);
            // What the entry says: a command as it is, a short message
            // as it is, a long one by its label, or while that is being
            // made, two bars that breathe.
            let label = e.wants_summary().then(|| self.outline_labels.get(&e.key).and_then(|v| v.as_str()).map(str::to_string)).flatten();
            let pending = e.wants_summary() && label.is_none() && !self.outline_failed.contains(&e.key);
            if pending && !self.outline_asking.contains(&e.key) {
                unlabelled.push((rows.len(), e.round, e.key.clone(), e.prompt.clone()));
            }
            let fresh = label.is_some() && self.outline_fresh.get(&e.key).is_some_and(|at| at.elapsed() < LABEL_FADE);
            let words = label.unwrap_or_else(|| e.title.clone());
            let said: AnyElement = if pending {
                let bar = |w: f32| div().h(px(8.)).w(relative(w)).rounded_full().bg(theme.muted_foreground.opacity(0.22));
                v_flex()
                    .py(px(4.))
                    .gap(px(7.))
                    .child(bar(0.92))
                    .child(bar(0.58))
                    .with_animation(("outline-wait", e.round), Animation::new(Duration::from_millis(1100)).repeat().with_easing(pulsating_between(0.35, 1.0)), |d, t| d.opacity(t))
                    .into_any_element()
            } else {
                let text = div()
                    .text_size(px(12.5))
                    .line_height(relative(1.35))
                    .line_clamp(2)
                    .text_ellipsis()
                    .overflow_hidden()
                    .when(is_current, |d| d.font_weight(FontWeight::MEDIUM))
                    .text_color(if quiet { theme.muted_foreground } else { theme.foreground })
                    .when(quiet, |d| d.font_family(theme.mono_font_family.clone()).text_size(px(11.5)))
                    .child(words);
                if fresh {
                    text.with_animation(("outline-label", e.round), Animation::new(LABEL_FADE).with_easing(ease_out_quint()), |d, t| d.opacity(t)).into_any_element()
                } else {
                    text.into_any_element()
                }
            };
            // When it was said, on a small plate in front of the words.
            let when = clock(&e.ts);
            let stamp = div()
                .flex_shrink_0()
                .mt(px(1.))
                .h(px(16.))
                .px(px(5.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(10.))
                .map(|d| match (is_current, e.kind) {
                    (true, _) => d.bg(theme.primary.opacity(if dark { 0.26 } else { 0.16 })).text_color(theme.link),
                    (_, Kind::Queued) => d.border_1().border_color(theme.muted_foreground.opacity(0.4)).text_color(theme.muted_foreground),
                    _ => d.bg(theme.muted_foreground.opacity(if dark { 0.16 } else { 0.11 })).text_color(theme.muted_foreground),
                })
                .child(if e.kind == Kind::Queued && when.is_empty() { "queued".to_string() } else { when });
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
                    .when(is_current, |d| d.bg(theme.primary.opacity(if dark { 0.16 } else { 0.10 })))
                    .when(!is_current, |d| d.hover(|s| s.bg(row_hover(&theme, dark))))
                    .on_click(cx.listener(move |this, _, _, cx| this.outline_go(round, cx)))
                    .child(h_flex().flex_1().min_w_0().py(px(7.)).gap(px(8.)).items_start().child(stamp).child(div().flex_1().min_w_0().child(said)))
                    .into_any_element(),
            );
        }
        // The entry in view is kept in sight as the conversation moves.
        let at = Some((key.clone(), current));
        // An outline at its foot stays there as it grows: a new entry, or
        // a label that takes a second line, would leave the last row cut
        // by the panel's edge, since the row is not laid out when the
        // scroller is asked to show it. The last entry is gone to the
        // same way, by the foot and not by its row.
        let (off, most) = (self.outline_scroll.offset().y, self.outline_scroll.max_offset().y);
        let at_foot = most > px(0.) && off <= px(1.) - most;
        let leaving = self.panel_leaving() == Some(false);
        let moved = !leaving && self.outline_at != at && !rows.is_empty();
        if moved {
            self.outline_at = at;
        }
        if at_foot || (moved && current + 1 >= entries.len()) {
            self.outline_scroll.scroll_to_bottom();
        } else if moved {
            self.outline_scroll.scroll_to_item(current_child);
        }
        // Labels are asked for where the person is looking and no
        // further: the entries in the scroller's view and `LABEL_AHEAD`
        // above and below it, once the scroller has rested `LABEL_REST`
        // and nothing is being asked already, so a scroll through a long
        // conversation asks about where it stops and not what it passes.
        if !unlabelled.is_empty() && !leaving {
            let y = f32::from(self.outline_scroll.offset().y);
            let rested = match &self.outline_rest {
                Some((k, at, since)) if *k == key && (*at - y).abs() < 0.5 => since.elapsed() >= LABEL_REST,
                _ => {
                    self.outline_rest = Some((key.clone(), y, std::time::Instant::now()));
                    false
                }
            };
            if !rested {
                // Look again once it has had the time to rest.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(LABEL_REST + Duration::from_millis(30)).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            } else if self.outline_asking.is_empty() {
                // A child's bounds are where it was laid out, before the
                // scroller's offset is taken: the offset is added here.
                let (view, off) = (self.outline_scroll.bounds(), self.outline_scroll.offset().y);
                let (top, bottom) = (view.top() - LABEL_AHEAD, view.bottom() + LABEL_AHEAD);
                let wanted: Vec<(String, String)> = unlabelled
                    .into_iter()
                    .filter(|(child, round, _, _)| match self.outline_scroll.bounds_for_item(*child) {
                        Some(b) => b.bottom() + off >= top && b.top() + off <= bottom,
                        // Not laid out yet: the ones about the marked entry.
                        None => round.abs_diff(current) <= 8,
                    })
                    .map(|(_, _, key, prompt)| (key, prompt))
                    .collect();
                if !wanted.is_empty() {
                    self.ask_labels(wanted, cx);
                }
            }
        }
        // The day the entries at the top belong to stays at the top once
        // its own head has scrolled up to there: the last head whose
        // words have reached the place the pinned ones are drawn at
        // (`PIN_LEAD` under the view's top), so the two are in one place
        // at the change and nothing jumps.
        let pinned = {
            let (view, off) = (self.outline_scroll.bounds(), self.outline_scroll.offset().y);
            heads.iter().rev().find(|(child, lead, _)| self.outline_scroll.bounds_for_item(*child).is_some_and(|b| b.top() + off + px(*lead) < view.top() + PIN_LEAD - px(0.5))).map(|(_, _, day)| day.clone())
        };
        let body = if rows.is_empty() {
            div().p(px(14.)).text_size(px(12.)).text_color(theme.muted_foreground).child("Nothing said yet.").into_any_element()
        } else {
            v_flex()
                .relative()
                .flex_1()
                .min_h_0()
                .child(v_flex().id("outline-scroll").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.outline_scroll).px(px(6.)).py(px(6.)).children(rows))
                .vertical_scrollbar(&self.outline_scroll)
                .when_some(pinned, |d, day| {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .px(px(16.))
                            .pt(PIN_LEAD)
                            .pb(px(6.))
                            .bg(theme.sidebar)
                            .border_b_1()
                            .border_color(theme.border.opacity(0.6))
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground.opacity(0.8))
                            .child(day),
                    )
                })
                .into_any_element()
        };
        let el = v_flex().w(self.panel_w_now()).h_full().flex_shrink_0().border_r_1().border_color(theme.border).bg(theme.sidebar).child(panel_head("Outline", None, Some(if waiting > 0 {
            // The labels are on their way.
            h_flex()
                .gap(px(6.))
                .items_center()
                .text_color(theme.muted_foreground)
                .child(crate::workbench::agent_glyph(emaki_core::model::AgentId::ClaudeCode, px(11.), crate::workbench::agent_color(emaki_core::model::AgentId::ClaudeCode, &theme), true, "outline-summing"))
                .child("Summarizing…")
                .into_any_element()
        } else {
            div().text_color(theme.muted_foreground.opacity(0.8)).child(plural(entries.len(), "prompt", "prompts")).into_any_element()
        }), &theme)).child(body);
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
            // The comparison's other side, and ↓ in it.
            "history" => self.history_show(true, cx),
            "history:down" => self.history_step(true, cx),
            // In the comparison: changeclick:<path> is a click on a file,
            // changeclick:cmd:<path> and changeclick:shift:<path> with
            // that key held, changeband:<y>:<y2> a rectangle dragged
            // down the list, changemenu:<path> a right click, and
            // changesel prints the files chosen and the one shown.
            t if t.starts_with("changeclick:") => {
                let rest = &t["changeclick:".len()..];
                let (m, path) = match rest.split_once(':') {
                    Some(("cmd", path)) => (Modifiers::secondary_key(), path),
                    Some(("shift", path)) => (Modifiers::shift(), path),
                    _ => (Modifiers::default(), rest),
                };
                if let Some(path) = under(path) {
                    self.change_click(path, m, cx);
                }
            }
            t if t.starts_with("changeband:") => {
                let mut ys = t["changeband:".len()..].split(':').filter_map(|y| y.parse::<f32>().ok());
                if let (Some(a), Some(b), Some(room)) = (ys.next(), ys.next(), self.changes.as_ref().map(|c| c.files.0.borrow().base_handle.bounds())) {
                    if let Some(c) = &mut self.changes {
                        c.band = Some((point(room.left() + px(30.), room.top() + px(a)), point(room.left() + px(30.), room.top() + px(a))));
                    }
                    self.changes_band_to(point(room.left() + px(170.), room.top() + px(b)), cx);
                }
            }
            "changeband" => self.changes_band_end(cx),
            // A click on the list's empty room.
            "changeoff" => self.changes_let_go(cx),
            t if t.starts_with("changemenu:") => {
                if let (Some(path), Some(room)) = (under(&t["changemenu:".len()..]), self.changes.as_ref().map(|c| c.files.0.borrow().base_handle.bounds())) {
                    self.changes_menu(path, room.origin + point(px(120.), px(60.)), cx);
                }
            }
            "changesel" => {
                let named = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                match &self.changes {
                    Some(c) => eprintln!("emaki: changes chosen {:?} shown {:?}", c.marked.iter().map(named).collect::<Vec<_>>(), c.picked.as_ref().map(named)),
                    None => eprintln!("emaki: changes closed"),
                }
            }
            t if t.starts_with("changes:") => self.open_changes(under(&t["changes:".len()..]), cx),
            // The question a switch asks with changes not committed.
            t if t.starts_with("branchask:") => {
                // Through the menu's own way in, so the check for a
                // conflict runs as it does at a click.
                let name = t["branchask:".len()..].to_string();
                if let Some(root) = self.files_root() {
                    self.read_git(root, cx);
                }
                self.branch_ask_probe = Some(name);
            }
            // Its answer: `branchgo:<name>:leave` or `:bring`; and the
            // strip's button.
            t if t.starts_with("branchgo:") => {
                let (name, how) = t["branchgo:".len()..].rsplit_once(':').unwrap_or((&t["branchgo:".len()..], "bring"));
                self.branch_run(name.to_string(), false, if how == "leave" { git::Carry::Leave } else { git::Carry::Bring }, cx)
            }
            "branchrestore" => self.branch_restore(cx),
            // The sheet a branch is made on, with a name in its field;
            // and the sheet that says how to sign in, as after a remote
            // that refused.
            "branchnew" => self.branch_new_probe = Some(String::new()),
            t if t.starts_with("branchnew:") => self.branch_new_probe = Some(t["branchnew:".len()..].to_string()),
            "gitgate" => self.open_gate("publish this branch".into(), "could not read Username for 'https://github.com': terminal prompts disabled".into(), cx),
            "branchfetch" => self.branch_fetch(true, cx),
            // Create Branch on that sheet, and Publish on the branch
            // checked out.
            "branchnewgo" => self.branch_new_go_probe = true,
            "branchpublish" => {
                if let Some(cur) = self.branches().and_then(|b| b.current.clone()) {
                    self.branch_publish(cur, cx);
                }
            }
            t if t.starts_with("branchq:") => self.branch_probe = Some(t["branchq:".len()..].to_string()),
            t if t.starts_with("commitsay:") => {
                self.commit_probe = Some(t["commitsay:".len()..].to_string());
                cx.notify();
            }
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
            "file:off" => self.close_file_view(cx),
            // Files dragged in from the file manager: held over a row or
            // over the panel's room (`treedrag`, `treedrag:<path>`), and
            // let go (`treedrop:<file>`).
            t if t.starts_with("treedrag") => {
                let row = t.strip_prefix("treedrag:").and_then(under);
                self.tree_drop_probe = None;
                self.tree_drop_hold = true;
                self.tree_drop_copy = true;
                self.tree_drop_over(point(px(0.), px(0.)), row.as_ref().map(|p| (p.as_path(), p.is_dir())), cx);
            }
            t if t.starts_with("treedrop:") => {
                self.tree_drop_hold = false;
                self.tree_drop_done(&[PathBuf::from(&t["treedrop:".len()..])], true, cx);
            }
            // A click in the page: the keyboard is the page's.
            "web:keys" => {
                if let Some(web) = &self.html_web {
                    web.take_keys();
                }
            }
            // The pane's enlarge button.
            // In a Word document: word:type:<words> types, word:run:<id>
            // runs one of the engine's commands, word:end and word:all
            // move and select.
            step if step.starts_with("word:") => {
                if let Some(pane) = self.word.clone() {
                    pane.update(cx, |pane, cx| pane.probe(&step[5..], cx));
                }
            }
            "file:big" => {
                self.file_big = !self.file_big;
                self.file_big_at = Some(std::time::Instant::now());
                cx.notify();
            }
            // In the tree: copy a file or folder, paste at one (or at
            // the folder itself with no path). In the comparison: ask
            // to discard a file's changes, and say yes.
            t if t.starts_with("treecopy:") => {
                if let Some(p) = under(&t["treecopy:".len()..]) {
                    self.tree_copy(&p, cx);
                }
            }
            t if t.starts_with("treepaste") => {
                let at = t.strip_prefix("treepaste:").and_then(under).or_else(|| self.files_root());
                if let Some(at) = at {
                    self.tree_paste(&at, cx);
                }
            }
            "discard:yes" => self.discard_now(cx),
            "treefocus" => self.pending_tree_focus = true,
            // A key pressed as the keyboard presses it, through every
            // listener and binding on the way: press:shift-up.
            t if t.starts_with("press:") => self.pending_press.push(t["press:".len()..].to_string()),
            // The pointer moved there, in points from the window's top
            // left, as the mouse moves it: pointer:832:166; clickat: is
            // a press and a release there.
            // wheel:<x>:<y>:<dy> is a scroll of that many points there.
            t if t.starts_with("pointer:") || t.starts_with("clickat:") || t.starts_with("wheel:") => {
                let mut at = t.splitn(2, ':').nth(1).unwrap_or("").split(':').filter_map(|v| v.parse::<f32>().ok());
                if let (Some(x), Some(y)) = (at.next(), at.next()) {
                    let how = if t.starts_with("clickat:") { Some(0.) } else if t.starts_with("wheel:") { Some(at.next().unwrap_or(-200.)) } else { None };
                    self.pending_pointer.push((point(px(x), px(y)), how));
                    cx.notify();
                }
            }
            "treecaret" => {
                let s = self.tree_input.read(cx);
                eprintln!("emaki: tree field {:?} of {}", s.selected_range(), s.value().len());
            }
            "treecollapse" => self.tree_collapse(cx),
            // The name's field in the tree: treenew:file|folder|rename:<path>
            // opens it, and :<words> after the path types them.
            t if t.starts_with("treenew:") => {
                let mut parts = t["treenew:".len()..].splitn(3, ':');
                let what = match parts.next() {
                    Some("folder") => FileDo::NewFolder,
                    Some("rename") => FileDo::Rename,
                    _ => FileDo::NewFile,
                };
                let path = parts.next().filter(|p| !p.is_empty() && *p != ".").and_then(under).or_else(|| self.files_root());
                if let Some(path) = path {
                    self.pending_tree_do = Some((what, path, parts.next().map(str::to_string)));
                }
            }
            "treenew" => self.pending_tree_do = None,
            "treeundo" => self.tree_undo(false, cx),
            "treeredo" => self.tree_undo(true, cx),
            t if t.starts_with("treetrash:") => {
                if let Some(p) = under(&t["treetrash:".len()..]) {
                    self.tree_trash(vec![p], cx);
                }
            }
            t if t.starts_with("treemove:") => {
                if let Some((what, dir)) = t["treemove:".len()..].split_once(':') {
                    if let (Some(what), Some(dir)) = (under(what), if dir.is_empty() { self.files_root() } else { under(dir) }) {
                        self.tree_move(vec![what], &dir, cx);
                    }
                }
            }
            t if t.starts_with("discard:") => {
                if let Some(p) = under(&t["discard:".len()..]) {
                    self.open_changes(Some(p.clone()), cx);
                    self.ask_discard(vec![p], cx);
                }
            }
            "pdf:pages" => self.pdf_side_toggle(true, cx),
            "pdf:contents" => self.pdf_side_toggle(false, cx),
            t if t.starts_with("pdf:page:") => {
                if let Ok(page) = t["pdf:page:".len()..].parse::<usize>() {
                    self.pdf_go_page(page, cx);
                }
            }
            // A right click in the pane: on the first line's word when the
            // editor shows, which takes it as the mouse's (the window is
            // needed, so it is made at the next draw).
            "file:menu" if self.file_editor.is_some() => self.file_menu_probe = true,
            // ⌘A in the files tree, and a rectangle dragged down it from
            // one height to another, in points from the tree's top.
            "treeall" => self.tree_key(&KeyDownEvent { keystroke: Keystroke::parse("secondary-a").unwrap(), is_held: false, prefer_character_input: false }, cx),
            t if t.starts_with("treeband:") => {
                let mut ys = t["treeband:".len()..].split(':').filter_map(|y| y.parse::<f32>().ok());
                if let (Some(a), Some(b)) = (ys.next(), ys.next()) {
                    let room = self.files_scroll.bounds();
                    self.tree_band = Some((point(room.left() + px(30.), room.top() + px(a)), point(room.left() + px(30.), room.top() + px(a))));
                    self.tree_band_to(point(room.left() + px(170.), room.top() + px(b)), cx);
                }
            }
            "file:menu" => {
                let at = self.file_view_scroll.bounds().origin + point(px(40.), px(40.));
                self.file_pane_menu(vec![("Copy", MenuDo::Copy(String::new()))], at, cx);
            }
            // Fold the section that line (from 1) of the file's editor heads.
            t if t.starts_with("file:fold:") => {
                if let (Some(ed), Ok(line)) = (&self.file_editor, t["file:fold:".len()..].parse::<usize>()) {
                    ed.state.update(cx, |s, cx| s.toggle_fold_at(line.saturating_sub(1), cx));
                }
            }
            // Put words at the start of the file's editor, as typing
            // does, and save what it holds.
            t if t.starts_with("file:type:") => {
                if let Some(ed) = self.file_editor.as_ref().filter(|ed| ed.saved.is_some()) {
                    let words = t["file:type:".len()..].to_string();
                    let at = words.len();
                    self.pending_edit = Some((ed.state.clone(), words, at));
                }
            }
            "file:save" => self.pending_save = true,
            // In a table: choose a cell, write in the chosen one, take
            // the last step back.
            t if t.starts_with("cell:") => self.pending_cell = Some(t["cell:".len()..].to_string()),
            // Answer the question a file with changes not saved asks,
            // and press Quit.
            // Open the change at a mark (from 0) on a card, and revert it.
            t if t.starts_with("file:peek:") => {
                if let Ok(id) = t["file:peek:".len()..].parse::<usize>() {
                    let at = self.file_view_scroll.bounds().origin + point(px(40.), px(90.));
                    self.open_file_peek(id, at, cx);
                }
            }
            "file:revert" => self.pending_revert = true,
            "file:ask:save" => self.pending_answer = Some(FileAnswer::Save),
            "file:ask:discard" => self.pending_answer = Some(FileAnswer::Discard),
            "file:ask:cancel" => self.pending_answer = Some(FileAnswer::Cancel),
            "quit" => self.quit(cx),
            "file:raw" => self.set_file_raw(true, cx),
            "file:read" => self.set_file_raw(false, cx),
            // In the PDF showing: select everything, or glyphs from and
            // to; print what is selected; find words, and step on.
            "pdf:all" => self.pdf_do(PdfDo::SelectAll, cx),
            "pdf:text" => {
                if let (Some(doc), Some((from, to))) = (self.pdf_doc(), self.pdf_sel) {
                    eprintln!("emaki: pdf {:?}", doc.text(from, to));
                }
            }
            "pdf:next" => self.pdf_find_step(1, cx),
            t if t.starts_with("pdf:sel:") => {
                let n: Vec<usize> = t["pdf:sel:".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
                if let [from, to] = n[..] {
                    self.pdf_sel = Some((from, to));
                    cx.notify();
                }
            }
            t if t.starts_with("pdf:find:") => {
                self.pdf_find_open = true;
                self.pdf_hits = self.pdf_doc().map(|doc| doc.find(&t["pdf:find:".len()..])).unwrap_or_default();
                self.pdf_hit = 0;
                eprintln!("emaki: pdf found {}", self.pdf_hits.len());
                self.pdf_find_show(cx);
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

#[cfg(test)]
mod draft_tests {
    use super::{drafts_left, drafts_rows, row_key, Draft};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn row(path: &str, text: &str, pid: Option<u64>) -> serde_json::Value {
        match pid {
            Some(pid) => serde_json::json!({ "path": path, "text": text, "pid": pid }),
            None => serde_json::json!({ "path": path, "text": text }),
        }
    }

    const OWN: u32 = 100;
    const LIVE: u32 = 200;
    const GONE: u32 = 300;
    fn alive(pid: u32) -> bool {
        pid == LIVE
    }

    #[test]
    fn a_running_emakis_drafts_are_not_taken_up() {
        let rows = [row("/no/such/live.qmd", "typed there", Some(LIVE as u64))];
        assert!(drafts_left(&rows, OWN, &alive, false).is_empty());
    }

    #[test]
    fn an_ended_emakis_drafts_are_taken_up() {
        let rows = [row("/no/such/gone.qmd", "typed", Some(GONE as u64)), row("/no/such/old.qmd", "typed", None)];
        let left = drafts_left(&rows, OWN, &alive, false);
        assert!(left.contains_key(&PathBuf::from("/no/such/gone.qmd")));
        assert!(left.contains_key(&PathBuf::from("/no/such/old.qmd")), "a row from before rows had a writer is no one's with no other Emaki running");
    }

    #[test]
    fn a_row_with_no_writer_waits_while_another_emaki_runs() {
        // The installed app from before rows had a writer, typing now.
        let rows = [row("/no/such/old.qmd", "typed", None)];
        assert!(drafts_left(&rows, OWN, &alive, true).is_empty());
    }

    #[test]
    fn a_draft_the_file_already_holds_is_dropped() {
        let dir = std::env::temp_dir().join(format!("emaki-draft-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("same.md");
        std::fs::write(&path, "saved words").unwrap();
        let rows = [row(&path.to_string_lossy(), "saved words", Some(GONE as u64))];
        assert!(drafts_left(&rows, OWN, &alive, false).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_write_keeps_what_is_not_its_own() {
        let on_disk = [
            row("/a", "live other", Some(LIVE as u64)),
            row("/b", "ended other, not taken", Some(GONE as u64)),
            row("/c", "taken up here", Some(GONE as u64)),
            row("/d", "written here before", Some(OWN as u64)),
        ];
        let taken = vec![row_key(&on_disk[2])];
        let mine: HashMap<PathBuf, Draft> = [(PathBuf::from("/c"), Draft { text: "taken up here".into(), base: None })].into();
        let rows = drafts_rows(&mine, &on_disk, OWN, &taken);
        let keys: Vec<_> = rows.iter().map(row_key).collect();
        assert!(keys.contains(&(Some(LIVE as u64), Some("/a".into()))));
        assert!(keys.contains(&(Some(GONE as u64), Some("/b".into()))), "an ended Emaki's rows wait for its next launch");
        assert!(!keys.contains(&(Some(GONE as u64), Some("/c".into()))), "a row taken up leaves under its old writer");
        assert!(keys.contains(&(Some(OWN as u64), Some("/c".into()))), "and is written as this Emaki's");
        assert!(!keys.contains(&(Some(OWN as u64), Some("/d".into()))), "this Emaki's old rows are replaced by what it has now");
    }
}
