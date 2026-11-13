//! The live-preview editor: markdown drawn as it reads, with the marks of
//! whatever the caret is in shown as written, as Obsidian's editor does.
//! One element draws every line itself (`EditorElement`): the text shaped
//! by gpui, the bullets, boxes, bars, plates and rules as quads, and the
//! caret and selection over them. The toolkit's input was not used: it
//! lays every line out in one face at one size, and a heading is larger
//! than the words under it.
//!
//! The text is `lines::Doc`; what each line is comes from
//! `markdown::analyze`, read again whole after every change. A line is
//! laid out when its text, its hidden marks or the width change
//! (`Cache`), so a keystroke lays out the lines it touched and not the
//! text. Typing arrives through gpui's input handler, so an input method
//! composes in place; keys that move or change arrive as actions.
//!
//! From Onyx's `editor.rs`. What a note has and a field does not is cut:
//! pictures, files in frames, cards, the vault. The view around the
//! editor gives it its look (`Look`), draws its placeholder, and may put
//! styles of its own over the words (`set_marks`, `set_issues`).

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use emaki_core::check::{self, Issue, Kind};
use gpui::*;
use gpui_component::highlighter::HighlightTheme;
use gpui_component::input;

use super::layout::{Cache, Reader, Rhythm, Row, Target, TextRow, Widget, CELL_X};
use super::lines::{list_line, next_marker, quote_line, Doc, ListKind, Pos, Sel};
use super::markdown::{self, Analysis, LineKind, SpanKind};
use super::rhythm;

mod cells;

pub(crate) use cells::{CellAt, Scope, TableFacts};

actions!(
    live,
    [Enter, NewLine, Tab, Outdent, Backspace, Delete, DeleteWordBack, DeleteLineBack, Left, Right, Up, Down, SelectLeft, SelectRight, SelectUp, SelectDown, WordLeft, WordRight, SelectWordLeft, SelectWordRight, LineStart, LineEnd, SelectLineStart, SelectLineEnd, DocStart, DocEnd, SelectDocStart, SelectDocEnd, PageUp, PageDown, Bold, Italic, Code, ToggleTask, Escape]
);

/// The key context of the editor's element.
pub(crate) const LIVE_CONTEXT: &str = "LiveEditor";

/// Each key as the Mac has it, bound as `secondary-<key>` where ⌘ is
/// meant (Ctrl off a Mac). Copy, Cut, Paste, Select All, Undo and Redo
/// are the toolkit's own actions, so the Edit menu serves a field and
/// the editor alike. Escape is bound and not taken here: it is for the
/// view around the editor to hear (`capture_action`), and goes on to the
/// window when that view does nothing with it.
pub(crate) fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(LIVE_CONTEXT);
    vec![
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("shift-enter", NewLine, c),
        KeyBinding::new("tab", Tab, c),
        KeyBinding::new("shift-tab", Outdent, c),
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("shift-backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("alt-backspace", DeleteWordBack, c),
        KeyBinding::new("secondary-backspace", DeleteLineBack, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("secondary-left", LineStart, c),
        KeyBinding::new("secondary-right", LineEnd, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("secondary-shift-left", SelectLineStart, c),
        KeyBinding::new("secondary-shift-right", SelectLineEnd, c),
        KeyBinding::new("shift-home", SelectLineStart, c),
        KeyBinding::new("shift-end", SelectLineEnd, c),
        KeyBinding::new("secondary-up", DocStart, c),
        KeyBinding::new("secondary-down", DocEnd, c),
        KeyBinding::new("secondary-shift-up", SelectDocStart, c),
        KeyBinding::new("secondary-shift-down", SelectDocEnd, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("secondary-b", Bold, c),
        KeyBinding::new("secondary-i", Italic, c),
        KeyBinding::new("secondary-shift-c", Code, c),
        KeyBinding::new("secondary-enter", ToggleTask, c),
        KeyBinding::new("escape", Escape, c),
        KeyBinding::new("secondary-a", input::SelectAll, c),
        KeyBinding::new("secondary-c", input::Copy, c),
        KeyBinding::new("secondary-x", input::Cut, c),
        KeyBinding::new("secondary-v", input::Paste, c),
        KeyBinding::new("secondary-z", input::Undo, c),
        KeyBinding::new("secondary-shift-z", input::Redo, c),
    ]
}

/// How the text is shown: written in, or read only.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Mode {
    /// As it reads, with the marks of what the caret is in shown.
    #[default]
    Live,
    /// As it reads only, with nothing to type in.
    Reading,
}

/// A link a right click was on: where it leads, its words, and the
/// whole of it as written.
#[derive(Clone, Debug)]
pub(crate) struct MenuLink {
    pub url: String,
    pub words: String,
    pub raw: String,
}

/// A spelling or grammar mark a right click was on.
#[derive(Clone, Debug)]
pub(crate) struct MenuIssue {
    pub range: Range<usize>,
    pub words: String,
    pub fixes: Vec<String>,
    pub spelling: bool,
    pub rule: String,
}

/// What a right click was on, as the view around the editor needs it to
/// make a menu.
#[derive(Clone, Debug, Default)]
pub(crate) struct MenuAt {
    pub link: Option<MenuLink>,
    pub issue: Option<MenuIssue>,
    pub table: Option<(CellAt, Scope, TableFacts)>,
    /// On a line with nothing on it, where a table can stand.
    pub blank: bool,
}

/// A line's own color with nothing of it showing, the fill under a line
/// drawn alone: gpui blends a border into its fill in plain color as it
/// nears the fill, so a fill of clear black darkens the soft inside of a
/// round corner into a second, darker line.
pub(crate) fn clear_of(line: Hsla) -> Hsla {
    line.opacity(0.)
}

/// What the editor tells the view around it.
pub(crate) enum EditorEvent {
    /// The text changed.
    Changed,
    /// A drag that selects went past the editor's top or its foot.
    /// (Emaki: a markdown file's field takes in the blocks there.)
    DragPast,
    /// The keyboard came to the editor.
    Focused,
    /// The keyboard left it.
    Blurred,
    /// A link was clicked: its address as written.
    OpenLink(String),
    /// A right click, or a press on a table's handle: where, and what it
    /// was on. The view around the editor makes the menu.
    Menu(Point<Pixels>, MenuAt),
}

/// The inks the editor draws in, each from the view around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Colors {
    /// The words.
    pub ink: Hsla,
    /// What is quiet: a mark shown, a bullet, a number, a struck word.
    pub muted: Hsla,
    /// A link's words, and code among words.
    pub link: Hsla,
    /// A done task's box, a quote's bar, the tint behind code among
    /// words, the ring around a table's cell.
    pub accent: Hsla,
    /// The check on a done task's box.
    pub on_accent: Hsla,
    /// The ground of a code block and of a table's head.
    pub plate: Hsla,
    /// A plate's edge, a rule, an indent guide, a table's lines.
    pub border: Hsla,
    pub selection: Hsla,
    pub caret: Hsla,
    /// The wash behind `==words==`.
    pub highlight: Hsla,
    /// The line under a misspelling, and under what grammar marks.
    pub spelling: Hsla,
    pub grammar: Hsla,
    /// What the editor stands on, for a table's handles.
    pub ground: Hsla,
    /// Whether the ground is dark: a tint is stronger on one.
    pub dark: bool,
}

impl Colors {
    /// The toolkit's theme, read for the editor.
    pub fn of(theme: &gpui_component::Theme) -> Colors {
        Colors {
            ink: theme.foreground,
            muted: theme.muted_foreground,
            link: theme.link,
            accent: theme.primary,
            on_accent: theme.primary_foreground,
            plate: theme.muted,
            border: theme.border,
            selection: theme.selection,
            caret: theme.caret,
            highlight: theme.warning,
            spelling: theme.danger,
            grammar: theme.info,
            ground: theme.background,
            dark: theme.mode.is_dark(),
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        let gray = |l: f32| hsla(0., 0., l, 1.);
        Colors { ink: gray(0.1), muted: gray(0.5), link: hsla(0.6, 0.7, 0.45, 1.), accent: hsla(0.6, 0.7, 0.5, 1.), on_accent: gray(1.), plate: gray(0.95), border: gray(0.88), selection: hsla(0.6, 0.7, 0.5, 0.3), caret: gray(0.1), highlight: hsla(0.13, 0.9, 0.55, 1.), spelling: hsla(0., 0.75, 0.5, 1.), grammar: hsla(0.58, 0.75, 0.5, 1.), ground: gray(1.), dark: false }
    }
}

/// Everything about how the text looks, from the view around the editor:
/// the faces with what stands behind each for the characters it lacks,
/// the sizes, the body's line, the inks, and code's colors.
#[derive(Clone, PartialEq)]
pub(crate) struct Look {
    /// The headings' measures where the view around has its own (Emaki: a
    /// markdown file as it reads), else Onyx's (`rhythm::HEADINGS`).
    pub headings: Option<[super::rhythm::Heading; 6]>,
    /// A table's words, as a share of the body's size, and their line,
    /// and whether its first row is set as the others are, where the view
    /// around has its own (Emaki). Else Onyx's.
    pub cells: Option<(f32, f32)>,
    pub plain_head: bool,
    /// How tall a blank line is while the caret is off it, after a
    /// heading and after anything else, where the view around sets
    /// blocks apart by its own room (Emaki). Else a line's.
    pub blank: Option<(f32, f32)>,
    /// How heavy strong words are: Onyx's bold, or the semibold the
    /// conversation's view sets them in (Emaki).
    pub strong: FontWeight,
    pub text_family: SharedString,
    /// The faces behind the words' face (`fonts::cjk_fallbacks`).
    pub text_fallbacks: Option<FontFallbacks>,
    /// Code's face, in a block and among words.
    pub mono_family: SharedString,
    pub mono_fallbacks: Option<FontFallbacks>,
    /// Code's face among words, where it is not `mono_family`: a smaller,
    /// lighter cut of it (`fonts::inline_mono_family`), so code stands no
    /// larger or heavier than the words around it (Emaki).
    pub inline_family: Option<SharedString>,
    /// The face of a code block's language name, the window's own words.
    pub ui_family: SharedString,
    pub text_px: f32,
    /// Code's size in a block. Code among words is laid out at the words'
    /// size, in `inline_family` where there is one.
    pub mono_px: f32,
    /// The body's line, in pixels. A heading's and code's go by their
    /// own sizes (`rhythm.rs`).
    pub line_px: f32,
    pub colors: Colors,
    /// The inks a code block's language is colored in.
    pub syntax: Arc<HighlightTheme>,
}

impl Look {
    /// The toolkit's theme as a look: its faces, sizes, inks and code
    /// colors, with the body's line at the rhythm's own share of the
    /// words' size. A view changes what it sets for itself.
    pub fn of(theme: &gpui_component::Theme) -> Look {
        let text_px = f32::from(theme.font_size);
        Look {
            headings: None,
            cells: None,
            plain_head: false,
            blank: None,
            strong: FontWeight::BOLD,
            text_family: theme.font_family.clone(),
            text_fallbacks: None,
            mono_family: theme.mono_font_family.clone(),
            mono_fallbacks: None,
            inline_family: None,
            ui_family: theme.font_family.clone(),
            text_px,
            mono_px: f32::from(theme.mono_font_size),
            line_px: (text_px * rhythm::LINE).round(),
            colors: Colors::of(theme),
            syntax: theme.highlight_theme.clone(),
        }
    }
}

impl Default for Look {
    fn default() -> Self {
        Self { headings: None, cells: None, plain_head: false, blank: None, strong: FontWeight::BOLD, text_family: "Inter".into(), text_fallbacks: None, mono_family: "JetBrains Mono".into(), mono_fallbacks: None, inline_family: None, ui_family: "Inter".into(), text_px: 16., mono_px: 14., line_px: 26., colors: Colors::default(), syntax: HighlightTheme::default_light() }
    }
}

/// A table's corners, as a code plate's.
const TABLE_R: f32 = 8.;
const BLINK: Duration = Duration::from_millis(530);
/// The caret's width.
const CARET_W: f32 = 1.5;

/// Lines of the text the view around the editor draws, as Onyx draws a
/// picture or a file's frame over its row (Emaki: what a markdown file
/// has that the editor does not draw: a picture, a formula, a callout,
/// the YAML header's band). The first line's row is as tall as what is
/// drawn, measured once at a width and kept; the others have none. A
/// press on it, or a caret moved into it, opens it: its lines show as
/// written, the caret where the press was, until the caret leaves them.
/// A drag that reaches it takes it whole, so it stays drawn.
#[derive(Clone)]
pub(crate) struct Piece {
    pub first: usize,
    pub last: usize,
    /// What it draws, for its height: the same key, the same height.
    pub key: u64,
    /// How far it stands out past the editor's left, its right and its
    /// top: a band out to the pane's edges.
    pub out: (f32, f32, f32),
    pub draw: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>,
}

pub(crate) struct Editor {
    doc: Doc,
    /// What the view around draws (`Piece`), the heights they were
    /// measured at by key with the width, the one open, and those shut
    /// as the layout reads them: first line, last, height.
    pieces: Vec<Piece>,
    piece_h: std::collections::HashMap<u64, (f32, f32)>,
    open_piece: Option<usize>,
    shut: Vec<(usize, usize, f32)>,
    sel: Sel,
    analysis: Analysis,
    mode: Mode,
    plain: bool,
    /// Lines of dashes at the top are a YAML header: a markdown file's,
    /// which a message does not have (Emaki).
    header: bool,
    /// The language the text is written in when it is not markdown: a
    /// `.typ` or `.tex` file read as its own markup (`dialect.rs`).
    dialect: Option<super::dialect::Dialect>,
    look: Look,
    focus: FocusHandle,
    /// Counts the text's readings (`reanalyze`), for `version`.
    version: u64,
    /// The scroller the view around the editor puts it in.
    pub scroll: ScrollHandle,
    /// How far the caret is kept from the top and foot of what shows: the
    /// height of the scroller's edge fade, where it has one.
    scroll_margin: f32,
    cache: Rc<RefCell<Cache>>,
    /// An input method's composition: what it has put in so far.
    marked: Option<(Pos, Pos)>,
    blink_at: Instant,
    blinking: bool,
    focused: bool,
    /// The window is behind another app's: the caret is not drawn and
    /// does not blink, as a field's does not.
    away: bool,
    /// The window's word when the keyboard comes and goes, asked for at
    /// the first drawing, where there is a window to ask.
    focus_subs: Option<(Subscription, Subscription)>,
    /// A drag under way: how the press began (a caret, a word, a line),
    /// and the range that press selected.
    drag: Option<(Grain, Pos, Pos)>,
    /// A link pressed as the text reads, followed at the release unless
    /// the press became a selection, so a selection can begin on a link.
    pressed_link: Option<Target>,
    /// A shut piece just opened by a press or an arrow, and the point the
    /// caret goes to once its lines are laid out as written (Emaki).
    reach: Option<(usize, f32, f32, f32)>,
    want_scroll: bool,
    /// The column ↑ and ↓ keep to, in pixels from the words' left.
    preferred_x: Option<f32>,
    /// Where the element was last painted, for an input method's windows,
    /// and whether it has been.
    origin: Point<Pixels>,
    painted: bool,
    /// The pointer's shape over the page, and where it last was.
    pointer: CursorStyle,
    pointer_at: (f32, f32),
    read_only: bool,
    /// The phrase a find looks for, every place it is in the text, and
    /// which of them the find stands on. The one stood on is the
    /// selection; the rest are washed.
    find_query: String,
    find: Rc<Vec<(Pos, Pos)>>,
    find_at: usize,
    /// The spelling and grammar marks the view handed over, in order, and
    /// its own styles over the words, both in bytes of `decor_for`, the
    /// text they stand on: they move with the words as it changes.
    issues: Vec<Issue>,
    marks: Vec<(Range<usize>, Mark)>,
    decor_for: String,
    /// Where the pointer is in a table, and a drag of its edges or handles.
    hover: Option<cells::Hover>,
    table_drag: Option<cells::TableDrag>,
    /// A table's columns as a drag has them: its first line, and each width.
    hold: Option<(usize, Vec<f32>)>,
}

/// A style the view lays over some words (`Editor::set_marks`): a color,
/// a weight and the rest of a highlight; `mono` sets them in code's face,
/// and `code` as code among words is set, in its face and ink on its
/// plate, as a command or a file the prompt names is (Emaki).
#[derive(Clone, Copy, Debug, Default, PartialEq, Hash)]
pub struct Mark {
    pub style: HighlightStyle,
    pub mono: bool,
    pub code: bool,
}

impl From<HighlightStyle> for Mark {
    fn from(style: HighlightStyle) -> Self {
        Mark { style, ..Default::default() }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Grain {
    Char,
    Word,
    Line,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A caret's place on the page, from the element's top left.
#[derive(Clone, Copy)]
struct CaretAt {
    x: f32,
    /// The line it stands on: what ↑ and ↓ step from and the view keeps in sight.
    y: f32,
    h: f32,
    /// The caret itself, in the middle of that line.
    bar_y: f32,
    bar_h: f32,
}

impl CaretAt {
    fn new(x: f32, y: f32, h: f32, bar_h: f32) -> Self {
        Self { x, y, h, bar_y: y + ((h - bar_h) / 2.).floor(), bar_h }
    }
}

/// Every place `q` is in `text`, whatever the case, as byte ranges of
/// `text`. The two are folded to lower case for the look; a fold that
/// changes a byte count, as some letters do, leaves the case as written.
fn find_all(text: &str, q: &str) -> Vec<Range<usize>> {
    let (hay, needle) = (text.to_lowercase(), q.to_lowercase());
    let (hay, needle) = if hay.len() == text.len() && needle.len() == q.len() { (hay, needle) } else { (text.to_string(), q.to_string()) };
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let mut from = 0;
    while let Some(at) = hay.get(from..).and_then(|rest| rest.find(&needle)) {
        let start = from + at;
        out.push(start..start + needle.len());
        from = start + needle.len();
        if from >= hay.len() {
            break;
        }
    }
    out
}

/// Where two texts differ: the start, and the end in each, each on a
/// character's start in both.
fn changed(old: &str, new: &str) -> (usize, usize, usize) {
    let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let (old_rest, new_rest) = (&old.as_bytes()[start..], &new.as_bytes()[start..]);
    let mut tail = old_rest.iter().rev().zip(new_rest.iter().rev()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(old.len() - tail) || !new.is_char_boundary(new.len() - tail) {
        tail -= 1;
    }
    (start, old.len() - tail, new.len() - tail)
}

/// The view's styles carried over a change of the text, until it sets
/// them again: those before the change stay, those after it move with
/// the words, and those it touched go.
fn carry_marks<T: Copy>(old: &str, new: &str, marks: &[(Range<usize>, T)]) -> Vec<(Range<usize>, T)> {
    let (start, old_end, new_end) = changed(old, new);
    marks
        .iter()
        .filter_map(|(r, style)| {
            if r.end <= start {
                Some((r.clone(), *style))
            } else if r.start >= old_end {
                Some((r.start + new_end - old_end..r.end + new_end - old_end, *style))
            } else {
                None
            }
        })
        .collect()
}

// -- the editor -------------------------------------------------------------

impl Editor {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            doc: Doc::default(),
            pieces: Vec::new(),
            piece_h: std::collections::HashMap::new(),
            open_piece: None,
            shut: Vec::new(),
            sel: Sel::default(),
            analysis: markdown::analyze(""),
            mode: Mode::Live,
            plain: false,
            header: false,
            dialect: None,
            look: Look::default(),
            focus: cx.focus_handle(),
            version: 0,
            scroll: ScrollHandle::new(),
            scroll_margin: 0.,
            cache: Rc::new(RefCell::new(Cache::default())),
            marked: None,
            blink_at: Instant::now(),
            blinking: false,
            focused: false,
            away: false,
            focus_subs: None,
            drag: None,
            pressed_link: None,
            reach: None,
            want_scroll: false,
            preferred_x: None,
            origin: Point::default(),
            painted: false,
            pointer: CursorStyle::IBeam,
            pointer_at: (0., 0.),
            read_only: false,
            find_query: String::new(),
            find: Rc::new(Vec::new()),
            find_at: 0,
            issues: Vec::new(),
            marks: Vec::new(),
            decor_for: String::new(),
            hover: None,
            table_drag: None,
            hold: None,
        }
    }

    /// What a layout goes by, as things stand.
    fn reader(&self) -> Reader<'_> {
        let revealing = self.revealing();
        Reader {
            doc: &self.doc,
            analysis: &self.analysis,
            look: &self.look,
            rhythm: Rhythm::of(&self.look),
            mode: self.mode,
            sel: revealing.then(|| self.sel.range()),
            issues: if self.locked() { &[] } else { &self.issues },
            quiet: None,
            marks: &self.marks,
            hold: self.hold.as_ref(),
            pieces: &self.shut,
            version: self.version,
        }
    }

    // -- the text -------------------------------------------------------------

    /// Another text: the history, the selection and the scroll start over,
    /// and the view's marks and styles go with the text they stood on.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.doc = Doc::from_text(text);
        self.sel = Sel::default();
        self.marked = None;
        self.drag = None;
        self.preferred_x = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.reanalyze();
        self.decor_afresh();
        if !self.find_query.is_empty() {
            self.refind();
        }
        cx.notify();
    }

    /// The same text from elsewhere, when nothing was typed: the caret is
    /// kept where the text still has it. The history starts over.
    pub fn replace_text_quietly(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.doc.text() == text {
            return;
        }
        let keep = self.sel;
        self.doc = Doc::from_text(text);
        self.sel = Sel { anchor: self.doc.clamp(keep.anchor), head: self.doc.clamp(keep.head) };
        self.marked = None;
        self.reanalyze();
        self.decor_afresh();
        if !self.find_query.is_empty() {
            self.refind();
        }
        cx.notify();
    }

    pub fn text(&self) -> String {
        self.doc.text()
    }

    pub fn is_empty(&self) -> bool {
        self.doc.line_count() == 1 && self.doc.line(0).is_empty()
    }

    /// Changes made from outside, at byte ranges of the text as it is:
    /// one step of undo. The selection moves with the words it stands
    /// in: past a change before it, and to the end of one made around
    /// it, so a word completed at the caret leaves the caret after it.
    /// A range that begins past the text, over an earlier one or where an
    /// earlier one begins is left out.
    pub fn apply_edits(&mut self, edits: Vec<(Range<usize>, String)>, cx: &mut Context<Self>) {
        let len = self.doc.text().len();
        let mut edits: Vec<(Range<usize>, String)> = edits.into_iter().filter(|(r, _)| r.start <= r.end && r.start <= len).map(|(r, t)| (r.start..r.end.min(len), t.replace("\r\n", "\n").replace('\r', "\n"))).collect();
        edits.sort_by_key(|(r, _)| (r.start, r.end));
        // Each as places, held to characters' starts, none over another
        // and no two from one place, so the order they are made in is the
        // text's.
        let mut changes: Vec<(Pos, Pos, String)> = Vec::with_capacity(edits.len());
        let mut spans: Vec<(usize, usize, usize)> = Vec::with_capacity(edits.len());
        let mut last: Option<(usize, usize)> = None;
        for (r, text) in edits {
            let (a, b) = (self.doc.pos_of(r.start), self.doc.pos_of(r.end));
            let (from, to) = (self.doc.offset_of(a), self.doc.offset_of(b));
            if last.is_some_and(|(start, end)| from < end || from == start) || (from == to && text.is_empty()) {
                continue;
            }
            last = Some((from, to));
            spans.push((from, to, text.len()));
            changes.push((a, b, text));
        }
        if changes.is_empty() {
            return;
        }
        // Where a byte of the text as it was stands after the changes: past
        // each change that ends at or before it, and at the end of what a
        // change around it put in.
        let moved = |at: usize| {
            let mut shift = 0isize;
            for &(from, to, put) in &spans {
                if at >= to {
                    shift += put as isize - (to - from) as isize;
                } else if at > from {
                    return (from as isize + shift + put as isize).max(0) as usize;
                } else {
                    break;
                }
            }
            (at as isize + shift).max(0) as usize
        };
        let before = self.sel;
        let (anchor, head) = (moved(self.doc.offset_of(before.anchor)), moved(self.doc.offset_of(before.head)));
        self.doc.end_step();
        self.doc.replace_many(changes, before, before);
        self.doc.end_step();
        self.sel = Sel { anchor: self.doc.pos_of(anchor), head: self.doc.pos_of(head) };
        self.marked = None;
        self.reanalyze();
        if !self.find_query.is_empty() {
            self.refind();
        }
        self.text_changed();
        self.want_scroll = true;
        self.preferred_x = None;
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    // -- the selection ----------------------------------------------------------

    /// The byte of the text the caret is at.
    pub fn caret_byte(&self) -> usize {
        self.doc.offset_of(self.sel.head)
    }

    /// The selection in bytes of the text, in order; empty for a caret.
    pub fn selection_bytes(&self) -> Range<usize> {
        let (a, b) = self.sel.range();
        self.doc.offset_of(a)..self.doc.offset_of(b)
    }

    /// The selection put at bytes of the text, from its start to the
    /// caret at its end, each held to the text and to a character's start;
    /// the view is brought to it. The keyboard stays where it is.
    pub fn set_selection_bytes(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let (a, b) = (self.doc.pos_of(range.start.min(range.end)), self.doc.pos_of(range.end.max(range.start)));
        self.sel = Sel { anchor: a, head: b };
        self.preferred_x = None;
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.notify();
    }

    pub fn has_selection(&self) -> bool {
        let (a, b) = self.sel.range();
        a != b
    }

    pub fn selection_text(&self) -> String {
        let (a, b) = self.sel.range();
        self.doc.slice(a, b)
    }

    /// Where the caret was last drawn, in the window: its bar, whether or
    /// not it shows at this moment of its blink. None before the first
    /// drawing.
    pub fn caret_bounds(&self) -> Option<Bounds<Pixels>> {
        if !self.painted {
            return None;
        }
        let at = self.caret_at(self.sel.head)?;
        Some(Bounds::new(point(self.origin.x + px(at.x), self.origin.y + px(at.bar_y)), size(px(CARET_W), px(at.bar_h))))
    }

    /// The byte of the text under a point of the window, from the last
    /// drawing. (Emaki: a press on a markdown file as it reads lands the
    /// caret where it was.)
    pub fn offset_at(&self, at: Point<Pixels>) -> Option<usize> {
        if !self.painted {
            return None;
        }
        let local = (f32::from(at.x - self.origin.x), f32::from(at.y - self.origin.y));
        let pos = self.pos_at(local.0, local.1.max(0.), &self.cache.borrow())?;
        Some(self.doc.offset_of(pos))
    }

    /// Whether the caret is on the first row the editor shows, and
    /// whether on its last: an arrow there leaves it. (Emaki addition.)
    pub fn caret_rows(&self) -> (bool, bool) {
        let Some(at) = self.caret_at(self.sel.head) else { return (true, true) };
        let total = self.cache.borrow().total;
        (at.y < 1., at.y + at.h + 1. >= total)
    }

    /// The byte at `x` on the first row, or the last: where an arrow
    /// that came from over or under the editor lands. (Emaki addition.)
    pub fn row_offset(&self, x: Pixels, last: bool) -> Option<usize> {
        if !self.painted {
            return None;
        }
        let total = self.cache.borrow().total;
        let y = if last { (total - 1.).max(0.) } else { 1. };
        let pos = self.pos_at(f32::from(x - self.origin.x), y, &self.cache.borrow())?;
        Some(self.doc.offset_of(pos))
    }

    /// Whether it has been drawn yet.
    pub fn painted(&self) -> bool {
        self.painted
    }

    /// Whether an input method has words in the making here.
    pub fn composing(&self) -> bool {
        self.marked.is_some()
    }

    /// The selection carried a row down or up (⇧↓, ⇧↑), or to the text's
    /// end or start (⇧⌘↓, ⇧⌘↑). (Emaki addition.)
    pub fn select_on(&mut self, down: bool, all: bool, cx: &mut Context<Self>) {
        match (all, down) {
            (false, _) => self.vertical(down, true, cx),
            (true, true) => {
                let end = self.doc.end();
                self.move_to(end, true, cx)
            }
            (true, false) => self.move_to(Pos::default(), true, cx),
        }
    }

    /// A drag that selects begins at the caret, as a press there begins
    /// one: the pointer's moves carry it from here. (Emaki: a drag that
    /// began on a markdown file's drawing before this field was open.)
    pub fn begin_drag(&mut self) {
        let at = self.sel.head;
        self.sel = Sel { anchor: at, head: at };
        self.drag = Some((Grain::Char, at, at));
    }

    /// All of the text selected, as ⌘A selects it. (Emaki addition.)
    pub fn select_whole(&mut self, cx: &mut Context<Self>) {
        self.sel = Sel { anchor: Pos::default(), head: self.doc.end() };
        cx.notify();
    }

    /// Where a line's row stands in the window, from the last drawing,
    /// and where the text ends. (Emaki: a markdown file's structure, and
    /// a press under its last line.)
    pub fn line_top(&self, line: usize) -> Option<Pixels> {
        if !self.painted {
            return None;
        }
        Some(self.origin.y + px(*self.cache.borrow().tops.get(line)?))
    }

    pub fn foot(&self) -> Option<Pixels> {
        self.painted.then(|| self.origin.y + px(self.cache.borrow().total))
    }

    /// Whether a drag that selects is under way. (Emaki addition.)
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Where a point of the window stands to a drag under way: past the
    /// editor's foot (true), over its top (false), or neither (None).
    /// (Emaki addition.)
    pub fn drag_beyond(&self, at: Point<Pixels>) -> Option<bool> {
        self.drag?;
        let y = f32::from(at.y - self.origin.y);
        if y < 0. {
            Some(false)
        } else if y > self.cache.borrow().total {
            Some(true)
        } else {
            None
        }
    }

    /// A drag under way carried to a point of the window, as the pointer
    /// moving there would carry it. (Emaki addition.)
    pub fn drag_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (x, y) = (f32::from(at.x - self.origin.x), f32::from(at.y - self.origin.y));
        self.drag_at(x, y, cx);
    }

    fn drag_at(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some((grain, a, b)) = self.drag else { return };
        let Some(p) = self.pos_at(x, y.max(0.), &self.cache.borrow()) else { return };
        let (pa, pb) = self.grain_range(p, grain);
        let (anchor, head) = if pa < a { (b, pa) } else { (a, pb.max(b)) };
        let head = self.held_to(anchor, head);
        if self.sel != (Sel { anchor, head }) {
            self.sel = Sel { anchor, head };
            self.want_scroll = true;
            if anchor != head {
                self.pressed_link = None;
            }
            cx.notify();
        }
    }

    /// Words put before the text and after it, outside the history: the
    /// selection and a drag under way stay on the words they were on.
    /// (Emaki: a markdown file's field taking in the blocks around it.)
    pub fn grow(&mut self, before: &str, after: &str, cx: &mut Context<Self>) {
        let doc = &self.doc;
        let (anchor, head) = (doc.offset_of(self.sel.anchor), doc.offset_of(self.sel.head));
        let drag = self.drag.map(|(grain, a, b)| (grain, doc.offset_of(a), doc.offset_of(b)));
        let text = format!("{before}{}{after}", doc.text());
        self.doc = Doc::from_text(&text);
        let n = before.len();
        let doc = &self.doc;
        self.sel = Sel { anchor: doc.pos_of(anchor + n), head: doc.pos_of(head + n) };
        self.drag = drag.map(|(grain, a, b)| (grain, doc.pos_of(a + n), doc.pos_of(b + n)));
        self.marked = None;
        self.reanalyze();
        self.decor_afresh();
        if !self.find_query.is_empty() {
            self.refind();
        }
        cx.notify();
    }

    /// The caret at the end, and the view with it.
    pub fn move_to_end(&mut self, cx: &mut Context<Self>) {
        let end = self.doc.end();
        self.move_to(end, false, cx);
    }

    // -- how it shows -------------------------------------------------------------

    /// A change of how the text is shown. The keyboard stays where it is.
    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode != mode {
            let was_reading = self.mode == Mode::Reading;
            self.mode = mode;
            self.marked = None;
            self.want_scroll = true;
            if was_reading && self.focused {
                self.kick_blink(cx);
            }
            cx.notify();
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        if self.look != look {
            self.look = look;
            cx.notify();
        }
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    /// A text that takes no change, and still selects, copies and finds.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only != read_only {
            self.read_only = read_only;
            cx.notify();
        }
    }

    /// How far the caret is kept from the top and foot of the scroller
    /// around the editor: the height of its edge fade, where it has one.
    pub fn set_scroll_margin(&mut self, margin: f32) {
        self.scroll_margin = margin.max(0.);
    }

    /// The view's own styles over the words, by bytes of the text as it
    /// is now, drawn over the editor's own: a color, a weight, an italic,
    /// a ground, an underline, a strike, code's face (`Mark`). They move with the words as the
    /// text changes, until they are set again.
    pub fn set_marks(&mut self, marks: Vec<(Range<usize>, Mark)>, cx: &mut Context<Self>) {
        let mut marks: Vec<(Range<usize>, Mark)> = marks.into_iter().filter(|(r, _)| r.start < r.end).collect();
        marks.sort_by_key(|(r, _)| (r.start, r.end));
        if self.marks != marks {
            self.marks = marks;
            cx.notify();
        }
    }

    /// The spelling and grammar marks to draw, by bytes of the text as it
    /// is now: a wavy line under each, in the look's two inks. They move
    /// with the words as the text changes, until they are set again, and
    /// do not show on a text that takes no change.
    pub fn set_issues(&mut self, issues: Vec<Issue>, cx: &mut Context<Self>) {
        let mut issues: Vec<Issue> = issues.into_iter().filter(|i| i.range.start < i.range.end).collect();
        issues.sort_by_key(|i| (i.range.start, i.range.end));
        if self.issues != issues {
            self.issues = issues;
            cx.notify();
        }
    }

    /// The marks as they stand now, moved with the words since they were set.
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }

    /// What of the text is prose, for a check of its writing: the whole
    /// text with all that is not prose blanked byte for byte (code, marks,
    /// addresses, a table's bars), so a place in it is a place in the
    /// text, and its runs, one to a block.
    pub fn prose(&self) -> (String, Vec<markdown::Prose>) {
        markdown::prose(&self.doc.text(), &self.analysis)
    }

    /// A text the marks and styles were not made on: they go.
    fn decor_afresh(&mut self) {
        self.issues.clear();
        self.marks.clear();
        self.decor_for = self.doc.text();
    }

    /// One change of the text: the marks and styles move with the words.
    fn text_changed(&mut self) {
        let new = self.doc.text();
        if !self.issues.is_empty() {
            self.issues = check::carry(&self.decor_for, &new, &self.issues);
        }
        if !self.marks.is_empty() {
            self.marks = carry_marks(&self.decor_for, &new, &self.marks);
        }
        self.decor_for = new;
    }

    /// The mark at a place, as its menu needs it.
    pub fn issue_at(&self, offset: usize) -> Option<MenuIssue> {
        if self.locked() {
            return None;
        }
        let i = self.issues.iter().find(|i| i.range.start <= offset && offset <= i.range.end)?;
        let words = self.decor_for.get(i.range.clone())?.to_string();
        Some(MenuIssue { range: i.range.clone(), words, fixes: i.fixes.clone(), spelling: i.kind == Kind::Spelling, rule: i.rule.clone() })
    }

    /// A correction where the marked words were, one step of undo. The
    /// range was read when the menu opened: it must still be whole
    /// characters of the text there is now.
    pub fn fix_issue(&mut self, range: Range<usize>, with: &str, cx: &mut Context<Self>) {
        if self.locked() || self.doc.text().get(range.clone()).is_none() {
            return;
        }
        let (a, b) = (self.doc.pos_of(range.start), self.doc.pos_of(range.end));
        self.doc.end_step();
        self.replace_range(a, b, with, cx);
        self.doc.end_step();
    }

    // -- find ---------------------------------------------------------------

    /// Every place the phrase is in the text, and the find on the first
    /// at or after the caret, which becomes the selection. The count.
    pub fn set_find(&mut self, query: &str, cx: &mut Context<Self>) -> usize {
        self.find_query = query.trim().to_string();
        let was = self.sel.range().0;
        self.refind();
        self.find_at = self.find.iter().position(|(a, _)| *a >= was).unwrap_or(0);
        if self.find.is_empty() {
            cx.notify();
        } else {
            self.find_go(cx);
        }
        self.find.len()
    }

    pub fn clear_find(&mut self, cx: &mut Context<Self>) {
        self.find_query.clear();
        self.find = Rc::new(Vec::new());
        self.find_at = 0;
        cx.notify();
    }

    /// The matches found again, as the text is now.
    fn refind(&mut self) {
        let found: Vec<(Pos, Pos)> = if self.find_query.is_empty() {
            Vec::new()
        } else {
            let text = self.doc.text();
            find_all(&text, &self.find_query).into_iter().map(|r| (self.doc.pos_of(r.start), self.doc.pos_of(r.end))).collect()
        };
        self.find = Rc::new(found);
        if self.find_at >= self.find.len() {
            self.find_at = 0;
        }
    }

    fn find_go(&mut self, cx: &mut Context<Self>) {
        let Some(&(a, b)) = self.find.get(self.find_at) else { return };
        self.sel = Sel { anchor: a, head: b };
        self.preferred_x = None;
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.notify();
    }

    /// The next match, or the one before, round the ends.
    pub fn find_step(&mut self, delta: i64, cx: &mut Context<Self>) {
        let n = self.find.len() as i64;
        if n == 0 {
            return;
        }
        self.find_at = (self.find_at as i64 + delta).rem_euclid(n) as usize;
        self.find_go(cx);
    }

    pub fn find_count(&self) -> usize {
        self.find.len()
    }

    pub fn find_at(&self) -> usize {
        self.find_at
    }

    /// The text is read, not written.
    pub(crate) fn reads(&self) -> bool {
        self.mode == Mode::Reading
    }

    /// Whether the text takes no change: one that is read only, or one
    /// shown as it reads. Nothing else about it is less: it selects,
    /// copies and finds as any text does, so the lock is on what changes
    /// the text, each change checking it, and never on the pointer or
    /// the keyboard. A task's box is the one thing ticked through it
    /// (`toggle_task_on`).
    pub(crate) fn locked(&self) -> bool {
        self.read_only || self.mode == Mode::Reading
    }

    /// Whether the marks around the caret are shown: only while the
    /// keyboard is in the editor, and never as the text reads.
    fn revealing(&self) -> bool {
        self.focused && self.mode != Mode::Reading
    }

    /// The keyboard came or went: the marks around the caret show only
    /// while it is here, the caret begins to blink as it arrives, and the
    /// view around is told.
    fn set_focused(&mut self, now: bool, notify: bool, cx: &mut Context<Self>) {
        if self.focused == now {
            return;
        }
        self.focused = now;
        if now {
            // The keyboard back with the caret in a piece: it opens again,
            // as it was when the keyboard left (Emaki).
            if self.sel.is_caret() {
                let line = self.sel.head.line;
                if let Some(first) = self.pieces.iter().find(|p| p.first <= line && line <= p.last).map(|p| p.first) {
                    self.open_piece = Some(first);
                }
            }
            self.kick_blink(cx);
            cx.emit(EditorEvent::Focused);
        } else {
            cx.emit(EditorEvent::Blurred);
        }
        if notify {
            cx.notify();
        }
    }

    /// The link at a point of the page, as its menu needs it.
    fn link_at(&self, x: f32, y: f32) -> Option<MenuLink> {
        let Some(Hit::Link(Target::Link(url))) = self.hit(x, y) else { return None };
        let p = self.pos_at(x, y, &self.cache.borrow())?;
        let at = self.doc.offset_of(p);
        let span = self.analysis.line_spans.get(p.line)?.iter().map(|&ix| &self.analysis.spans[ix]).find(|s| matches!(s.kind, SpanKind::Link(_)) && s.range.start <= at && at <= s.range.end);
        let slice = |a: usize, b: usize| self.doc.slice(self.doc.pos_of(a), self.doc.pos_of(b));
        let (words, raw) = span.map(|s| (slice(s.open.end, s.close.start), slice(s.range.start, s.range.end))).unwrap_or_default();
        Some(MenuLink { url, words, raw })
    }

    /// Whether the caret or the selection is in the link at a point of
    /// the page: it is being written, so a press there puts the caret, and
    /// ⌘ and a press follow it.
    fn writing_link(&self, x: f32, y: f32) -> bool {
        let Some(p) = self.pos_at(x, y, &self.cache.borrow()) else { return false };
        let at = self.doc.offset_of(p);
        let (a, b) = self.sel.range();
        let (ga, gb) = (self.doc.offset_of(a), self.doc.offset_of(b));
        self.analysis.line_spans.get(p.line).is_some_and(|spans| {
            spans.iter().map(|&ix| &self.analysis.spans[ix]).any(|s| matches!(s.kind, SpanKind::Link(_)) && s.range.start <= at && at <= s.range.end && ga <= s.range.end && gb >= s.range.start)
        })
    }

    /// A right click off a link: a press inside the selection leaves it,
    /// one on a word selects the word, as a Mac's text does, and one on
    /// nothing puts the caret there, for a paste.
    fn select_for_menu(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some(p) = self.pos_at(x, y, &self.cache.borrow()) else { return };
        let (a, b) = self.sel.range();
        if a != b && a <= p && p <= b {
            return;
        }
        let (from, to) = self.grain_range(p, Grain::Word);
        self.sel = if from != to && !self.doc.slice(from, to).trim().is_empty() { Sel { anchor: from, head: to } } else { Sel::caret(p) };
        self.preferred_x = None;
        self.kick_blink(cx);
    }

    fn follow(&mut self, target: Target, cx: &mut Context<Self>) {
        match target {
            Target::Link(url) => cx.emit(EditorEvent::OpenLink(url)),
        }
    }

    /// What ⌘C takes: the text's own words, or as the text reads the
    /// words that show and nothing that does not.
    pub fn copied_text(&self) -> String {
        if self.mode == Mode::Reading {
            self.shown_text()
        } else {
            self.selection_text()
        }
    }

    /// The selection as it reads: each line by the rule that hides its
    /// marks in the drawing (`hidden_on`), with the lines drawn as no
    /// words (a fence, a rule, a heading's underline, a table's bar row)
    /// left out, and a table's rows as their cells, a tab between them,
    /// whole, since a table is selected a line at a time.
    fn shown_text(&self) -> String {
        let (a, b) = self.sel.range();
        if a == b {
            return String::new();
        }
        let mut lines = Vec::new();
        for line in a.line..=b.line {
            let text = self.doc.line(line);
            let from = if line == a.line { a.col } else { 0 };
            let to = if line == b.line { b.col } else { text.len() };
            match &self.analysis.lines[line].kind {
                LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::SetextUnderline | LineKind::Rule => {}
                LineKind::TableFirst(t) | LineKind::TableRow(t) if a.line != b.line || self.cell_of(a).is_none() => {
                    let table = &self.analysis.tables[*t];
                    if let Some(cells) = table.row_of(line).and_then(|r| table.rows.get(r)) {
                        lines.push(cells.join("\t"));
                    }
                }
                _ => lines.push(markdown::visible_part(text, from, to, &self.hidden_on(line))),
            }
        }
        lines.join("\n")
    }

    /// The text as plain lines, nothing of markdown read in it, or as
    /// markdown again. (Emaki: the composer with live markdown off.)
    pub fn set_plain(&mut self, plain: bool, cx: &mut Context<Self>) {
        if self.plain != plain {
            self.plain = plain;
            self.reanalyze();
            self.decor_afresh();
            cx.notify();
        }
    }

    /// The runs of a Typst or LaTeX document's own code, to be drawn
    /// folded (`dialect::folds`).
    pub fn folds(&self) -> Vec<super::dialect::Fold> {
        // Markdown with no raw block of either has none.
        if self.dialect.is_none() && !self.analysis.code_blocks.iter().any(|b| b.markup.is_some()) {
            return Vec::new();
        }
        super::dialect::folds(&self.doc.text(), &self.analysis)
    }

    /// The language the text is read in: markdown, or Typst's or LaTeX's
    /// markup.
    pub fn set_dialect(&mut self, dialect: Option<super::dialect::Dialect>, cx: &mut Context<Self>) {
        if self.dialect != dialect {
            self.dialect = dialect;
            self.reanalyze();
            cx.notify();
        }
    }

    /// Whether lines of dashes at the top are read as a YAML header.
    pub fn set_header(&mut self, header: bool, cx: &mut Context<Self>) {
        if self.header != header {
            self.header = header;
            self.reanalyze();
            cx.notify();
        }
    }

    fn reanalyze(&mut self) {
        let text = self.doc.text();
        self.analysis = match (self.plain, self.dialect) {
            (true, _) => markdown::plain(&text),
            (false, Some(d)) => super::dialect::analyze(&text, d),
            (false, None) => markdown::analyze_with(&text, self.header),
        };
        self.version += 1;
    }

    /// A count that moves whenever the text may have changed, by any way.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// After every change: the text read again, numbered lists put
    /// right, the caret shown, the view around told.
    fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.reanalyze();
        self.renumber();
        if !self.find_query.is_empty() {
            self.refind();
        }
        self.text_changed();
        self.want_scroll = true;
        self.preferred_x = None;
        self.kick_blink(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.notify();
    }

    /// Numbered lists count from their first item, as Obsidian keeps
    /// them, whatever was typed; one step of undo with the change before it.
    fn renumber(&mut self) {
        let mut changes = Vec::new();
        for list in self.analysis.lists.iter().filter(|l| l.ordered) {
            let mut expected: Option<u64> = None;
            for &line in &list.items {
                let Some(mark) = self.analysis.lines[line].list.as_ref() else { continue };
                let ListKind::Ordered(n) = mark.kind else { continue };
                let want = expected.unwrap_or(n);
                if n != want {
                    let text = self.doc.line(line);
                    let Some(written) = text.get(mark.marker.start..) else { continue };
                    let digits = written.chars().take_while(|c| c.is_ascii_digit()).count();
                    changes.push((Pos::new(line, mark.marker.start), Pos::new(line, mark.marker.start + digits), want.to_string()));
                }
                expected = Some(want + 1);
            }
        }
        if changes.is_empty() {
            return;
        }
        // The caret stays with its line, moved by what its own number grew.
        let mut sel = self.sel;
        for (from, to, text) in &changes {
            for p in [&mut sel.anchor, &mut sel.head] {
                if p.line == from.line && p.col >= to.col {
                    p.col = p.col + text.len() - (to.col - from.col);
                }
            }
        }
        let before = self.sel;
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.reanalyze();
    }

    fn kick_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_at = Instant::now();
        if self.blinking {
            return;
        }
        self.blinking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let go_on = this
                    .update(cx, |e, cx| {
                        cx.notify();
                        e.focused && !e.away && e.mode != Mode::Reading
                    })
                    .unwrap_or(false);
                if !go_on {
                    let _ = this.update(cx, |e, _| e.blinking = false);
                    break;
                }
            }
        })
        .detach();
    }

    fn caret_shown(&self) -> bool {
        !self.away && self.revealing() && ((self.blink_at.elapsed().as_millis() / BLINK.as_millis()) % 2 == 0)
    }

    // -- edits --------------------------------------------------------------

    /// `text` in place of the selection.
    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let (a, b) = self.sel.range();
        let text = self.cell_words(text);
        let end = self.doc.replace(a, b, &text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
    }

    fn replace_range(&mut self, a: Pos, b: Pos, text: &str, cx: &mut Context<Self>) -> Pos {
        let end = self.doc.replace(a, b, text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
        end
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        // A piece selected whole opens (Emaki).
        if let Some(first) = self.pieces.iter().filter(|p| Some(p.first) != self.open_piece).find(|p| self.sel.range() == self.piece_ends(p)).map(|p| p.first) {
            self.open_piece_at(first, cx);
            return;
        }
        if self.in_cell() {
            self.cell_down(cx);
            return;
        }
        if !self.sel.is_caret() {
            self.insert("\n", cx);
            return;
        }
        let at = self.sel.head;
        let text = self.doc.line(at.line).to_string();
        let (quote_depth, body_start) = quote_line(&text).unwrap_or((0, 0));
        let quote_prefix = "> ".repeat(quote_depth);
        if let Some(item) = list_line(&text[body_start..]) {
            let marker_end = body_start + item.indent.len() + item.marker.len();
            if at.col >= marker_end {
                if item.body.trim().is_empty() && at.col == text.len() {
                    // An empty item ends the list: its marker goes.
                    self.replace_range(Pos::new(at.line, 0), Pos::new(at.line, text.len()), &quote_prefix, cx);
                } else {
                    let line = format!("\n{quote_prefix}{}{}", item.indent, next_marker(&item));
                    self.insert(&line, cx);
                }
                self.doc.end_step();
                return;
            }
        } else if quote_depth > 0 && at.col >= body_start {
            if text[body_start..].trim().is_empty() && at.col == text.len() {
                self.replace_range(Pos::new(at.line, 0), Pos::new(at.line, text.len()), "", cx);
            } else {
                self.insert(&format!("\n{quote_prefix}"), cx);
            }
            self.doc.end_step();
            return;
        }
        self.insert("\n", cx);
        self.doc.end_step();
    }

    fn new_line(&mut self, _: &NewLine, _: &mut Window, cx: &mut Context<Self>) {
        self.insert("\n", cx);
        self.doc.end_step();
    }

    /// The lines the selection touches.
    fn selected_lines(&self) -> Range<usize> {
        let (a, b) = self.sel.range();
        a.line..b.line + 1
    }

    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if self.in_cell() {
            self.cell_next(cx);
            return;
        }
        let lines: Vec<usize> = self.selected_lines().filter(|l| list_line(self.doc.line(*l)).is_some()).collect();
        if lines.is_empty() {
            self.insert("\t", cx);
            return;
        }
        let before = self.sel;
        let mut sel = self.sel;
        for p in [&mut sel.anchor, &mut sel.head] {
            if lines.contains(&p.line) {
                p.col += 1;
            }
        }
        let changes = lines.iter().map(|&l| (Pos::new(l, 0), Pos::new(l, 0), "\t".to_string())).collect();
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.after_edit(cx);
    }

    fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if self.in_cell() {
            self.cell_prev(cx);
            return;
        }
        let mut changes = Vec::new();
        let mut sel = self.sel;
        for l in self.selected_lines() {
            let text = self.doc.line(l);
            if list_line(text).is_none() {
                continue;
            }
            let cut = if text.starts_with('\t') { 1 } else { (text.len() - text.trim_start_matches(' ').len()).min(4) };
            if cut == 0 {
                continue;
            }
            changes.push((Pos::new(l, 0), Pos::new(l, cut), String::new()));
            for p in [&mut sel.anchor, &mut sel.head] {
                if p.line == l {
                    p.col = p.col.saturating_sub(cut);
                }
            }
        }
        if changes.is_empty() {
            return;
        }
        let before = self.sel;
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.after_edit(cx);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        if let Some(from) = self.back_stop(at) {
            if from != at {
                self.replace_range(from, at, "", cx);
            }
            return;
        }
        let text = self.doc.line(at.line);
        // Right after a list marker with nothing typed yet, or where an
        // item's words begin with the marker drawn in the margin: the
        // marker goes, and not a character of it no one sees.
        if let Some(item) = list_line(text) {
            let marker_end = item.indent.len() + item.marker.len();
            if at.col == marker_end && (item.body.is_empty() || self.floor(at.line) == marker_end) {
                self.replace_range(Pos::new(at.line, item.indent.len()), Pos::new(at.line, marker_end), "", cx);
                self.doc.end_step();
                return;
            }
        }
        let from = self.doc.prev_grapheme(at);
        if from != at {
            self.replace_range(from, at, "", cx);
        }
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        let to = self.forward_stop(at).unwrap_or_else(|| self.doc.next_grapheme(at));
        if to != at {
            self.replace_range(at, to, "", cx);
        }
    }

    fn delete_word_back(&mut self, _: &DeleteWordBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        let from = self.in_this_cell(at, self.doc.word_left(at));
        if from != at {
            self.replace_range(from, at, "", cx);
        }
        self.doc.end_step();
    }

    fn delete_line_back(&mut self, _: &DeleteLineBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            self.doc.end_step();
            return;
        }
        let at = self.sel.head;
        let from = if at.col > 0 { Pos::new(at.line, 0) } else { self.doc.prev_grapheme(at) };
        let from = self.in_this_cell(at, from);
        if from != at {
            self.replace_range(from, at, "", cx);
            self.doc.end_step();
        }
    }

    /// ⌘A takes a cell first, its words being a text of their own; again,
    /// the whole.
    fn select_all(&mut self, _: &input::SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let cell = self.cell_of(self.sel.head).map(|at| self.cell_span(at));
        self.sel = match cell {
            Some((a, b)) if self.sel.range() != (a, b) => Sel { anchor: a, head: b },
            _ => Sel { anchor: Pos::default(), head: self.doc.end() },
        };
        cx.notify();
    }

    fn copy(&mut self, _: &input::Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.copied_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &input::Cut, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.copied_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            if !self.locked() {
                self.insert("", cx);
                self.doc.end_step();
            }
        }
    }

    /// The clipboard's words in place of the selection. Files and a
    /// picture on the clipboard are the view's to take, before this
    /// (`capture_action`).
    fn paste(&mut self, _: &input::Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else { return };
        if let Some(text) = item.text() {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            self.insert(&text, cx);
            self.doc.end_step();
        }
    }

    fn undo(&mut self, _: &input::Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if let Some(sel) = self.doc.undo() {
            self.went_in_history(sel, cx);
        }
    }

    fn redo(&mut self, _: &input::Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if let Some(sel) = self.doc.redo() {
            self.went_in_history(sel, cx);
        }
    }

    /// A step taken back or made again: the text read, the selection the
    /// step kept, the view around told.
    fn went_in_history(&mut self, sel: Sel, cx: &mut Context<Self>) {
        self.sel = Sel { anchor: self.doc.clamp(sel.anchor), head: self.doc.clamp(sel.head) };
        self.marked = None;
        self.reanalyze();
        self.sel = self.settled_sel();
        if !self.find_query.is_empty() {
            self.refind();
        }
        self.text_changed();
        self.want_scroll = true;
        self.preferred_x = None;
        self.kick_blink(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    /// ⌘B, ⌘I, ⇧⌘C, ⌘U and ⇧⌘X: a span of the kind around or under the
    /// caret is unwrapped; a selection is wrapped; a caret on a word wraps
    /// the word; a caret on nothing opens an empty pair to type into, and
    /// in such a pair closes it again. The spans are the markdown's
    /// whether or not it is drawn as it reads, so a key pressed again
    /// takes off what it put on, with live markdown off too.
    fn toggle_wrap(&mut self, open: &str, close: &str, kind: SpanKind, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        // In Typst or LaTeX, that language's marks.
        let (open, close) = match self.dialect {
            Some(d) => match d.wrap(&kind) {
                Some(marks) => marks,
                None => return,
            },
            None => (open, close),
        };
        let (a, b) = self.sel.range();
        let (ga, gb) = (self.doc.offset_of(a), self.doc.offset_of(b));
        let read;
        let spans = if self.plain {
            read = markdown::analyze(&self.doc.text()).spans;
            &read
        } else {
            &self.analysis.spans
        };
        let around = spans.iter().filter(|s| s.kind == kind && s.range.start <= ga && gb <= s.range.end).min_by_key(|s| s.range.len()).cloned();
        if let Some(s) = around {
            let (open, close) = (self.doc.pos_of(s.open.start), self.doc.pos_of(s.close.start));
            let (open_end, close_end) = (self.doc.pos_of(s.open.end), self.doc.pos_of(s.close.end));
            let before = self.sel;
            let shift = |p: Pos| -> Pos {
                let mut p = p;
                if p.line == close.line && p.col >= close_end.col {
                    p.col -= close_end.col - close.col;
                }
                if p.line == open.line && p.col >= open_end.col {
                    p.col -= open_end.col - open.col;
                } else if p.line == open.line && p.col > open.col {
                    p.col = open.col;
                }
                p
            };
            let after = Sel { anchor: shift(before.anchor), head: shift(before.head) };
            self.doc.replace_many(vec![(open, open_end, String::new()), (close, close_end, String::new())], before, after);
            self.sel = Sel { anchor: self.doc.clamp(after.anchor), head: self.doc.clamp(after.head) };
            self.after_edit(cx);
            return;
        }
        if a != b {
            let text = self.doc.slice(a, b);
            let lead = text.len() - text.trim_start().len();
            let trail = text.len() - text.trim_end().len();
            let (ta, tb) = (self.doc.pos_of(ga + lead), self.doc.pos_of(gb - trail));
            if ta >= tb {
                return;
            }
            let before = self.sel;
            self.doc.replace_many(vec![(ta, ta, open.to_string()), (tb, tb, close.to_string())], before, before);
            let inner_a = Pos::new(ta.line, ta.col + open.len());
            let inner_b = if tb.line == ta.line { Pos::new(tb.line, tb.col + open.len()) } else { tb };
            self.sel = Sel { anchor: inner_a, head: inner_b };
            self.after_edit(cx);
            return;
        }
        let word = self.doc.word_at(a);
        if word.start < word.end {
            let (wa, wb) = (Pos::new(a.line, word.start), Pos::new(a.line, word.end));
            let before = self.sel;
            let caret = Pos::new(a.line, a.col + open.len());
            self.doc.replace_many(vec![(wa, wa, open.to_string()), (wb, wb, close.to_string())], before, Sel::caret(caret));
            self.sel = Sel::caret(caret);
            self.after_edit(cx);
            return;
        }
        let line = self.doc.line(a.line);
        if line[..a.col].ends_with(open) && line[a.col..].starts_with(close) {
            let from = Pos::new(a.line, a.col - open.len());
            let to = Pos::new(a.line, a.col + close.len());
            self.replace_range(from, to, "", cx);
            return;
        }
        let caret = Pos::new(a.line, a.col + open.len());
        self.doc.replace(a, a, &format!("{open}{close}"), self.sel);
        self.sel = Sel::caret(caret);
        self.after_edit(cx);
    }

    fn bold(&mut self, _: &Bold, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_wrap("**", "**", SpanKind::Strong, cx);
    }

    fn italic(&mut self, _: &Italic, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_wrap("*", "*", SpanKind::Em, cx);
    }

    /// ⇧⌘C, as Slack has it: words made code, and code made words again.
    fn code(&mut self, _: &Code, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_wrap("`", "`", SpanKind::Code, cx);
    }

    /// ⌘U: words underlined with `<u>`, or underlined no more.
    pub fn underline(&mut self, cx: &mut Context<Self>) {
        self.toggle_wrap("<u>", "</u>", SpanKind::Underline, cx);
    }

    /// ⇧⌘X: words struck through with `~~`, or struck no more.
    pub fn strike(&mut self, cx: &mut Context<Self>) {
        self.toggle_wrap("~~", "~~", SpanKind::Strike, cx);
    }

    /// ⌘↩: a task's box ticked or cleared; a bullet made a task; a plain
    /// line made a task.
    fn toggle_task(&mut self, _: &ToggleTask, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let line = self.sel.head.line;
        self.toggle_task_on(line, cx);
    }

    /// A task's box ticked. It is the one change a text that reads lets
    /// in, by a press on the box alone, as a box on a page is there to be
    /// ticked; ⌘↩ is the keyboard's and is locked (`toggle_task`).
    fn toggle_task_on(&mut self, line: usize, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let text = self.doc.line(line).to_string();
        let (_, body_start) = quote_line(&text).unwrap_or((0, 0));
        let before = self.sel;
        match list_line(&text[body_start..]) {
            Some(item) => {
                let start = body_start + item.indent.len();
                match item.kind {
                    ListKind::Task(done) => {
                        let bx = start + 2;
                        let new = if done { "[ ]" } else { "[x]" };
                        self.doc.replace_many(vec![(Pos::new(line, bx), Pos::new(line, bx + 3), new.to_string())], before, before);
                    }
                    ListKind::Bullet => {
                        let at = Pos::new(line, start + item.marker.len());
                        let mut after = before;
                        for p in [&mut after.anchor, &mut after.head] {
                            if p.line == line && p.col >= at.col {
                                p.col += 4;
                            }
                        }
                        self.doc.replace_many(vec![(at, at, "[ ] ".to_string())], before, after);
                        self.sel = after;
                    }
                    ListKind::Ordered(_) => return,
                }
            }
            None => {
                let at = Pos::new(line, body_start);
                let mut after = before;
                for p in [&mut after.anchor, &mut after.head] {
                    if p.line == line && p.col >= at.col {
                        p.col += 6;
                    }
                }
                self.doc.replace_many(vec![(at, at, "- [ ] ".to_string())], before, after);
                self.sel = after;
            }
        }
        self.after_edit(cx);
    }

    // -- moves ----------------------------------------------------------------

    fn move_to(&mut self, p: Pos, select: bool, cx: &mut Context<Self>) {
        let p = self.settled(self.doc.clamp(p), select);
        // A piece the caret is moved into opens, as a press opens it
        // (Emaki); a caret the text was set with leaves it shut.
        if !select {
            if let Some(first) = self.shut_piece_at(p.line).map(|piece| piece.first) {
                self.open_piece = Some(first);
            }
        }
        self.sel = if select { Sel { anchor: self.sel.anchor, head: p } } else { Sel::caret(p) };
        self.after_move(cx);
    }

    fn left(&mut self, select: bool, cx: &mut Context<Self>) {
        let p = if !select && !self.sel.is_caret() { self.sel.start() } else { self.step_back(self.sel.head) };
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    fn right(&mut self, select: bool, cx: &mut Context<Self>) {
        let p = if !select && !self.sel.is_caret() { self.sel.end() } else { self.step_on(self.sel.head) };
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    /// ⌥←: a list's marker is not a word, so from where an item's words
    /// begin it is the line before.
    fn word_left(&mut self, select: bool, cx: &mut Context<Self>) {
        let at = self.sel.head;
        let floor = self.floor(at.line);
        let p = if at.col <= floor {
            self.doc.word_left(Pos::new(at.line, 0))
        } else {
            let q = self.doc.word_left(at);
            if q.line == at.line && q.col < floor { Pos::new(at.line, floor) } else { q }
        };
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    fn word_right(&mut self, select: bool, cx: &mut Context<Self>) {
        let p = self.past_marker(self.doc.word_right(self.past_marker(self.sel.head)));
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    /// Home: the start of the words, before the line's marks; pressed
    /// there, the line's very start, unless that is inside a list's
    /// marker, which never shows.
    fn line_start(&mut self, select: bool, cx: &mut Context<Self>) {
        let at = self.sel.head;
        let text = self.doc.line(at.line);
        let (_, qs) = quote_line(text).unwrap_or((0, 0));
        let content = list_line(&text[qs..]).map_or(qs, |i| qs + i.indent.len() + i.marker.len());
        let p = if at.col > content || self.floor(at.line) > 0 { content } else { 0 };
        if let Some(cell) = self.cell_of(at) {
            let (from, _) = self.cell_span(cell);
            self.preferred_x = None;
            self.move_to(from, select, cx);
            return;
        }
        self.preferred_x = None;
        self.move_to(Pos::new(at.line, p), select, cx);
    }

    fn line_end(&mut self, select: bool, cx: &mut Context<Self>) {
        let at = self.sel.head;
        if let Some(cell) = self.cell_of(at) {
            let (_, to) = self.cell_span(cell);
            self.preferred_x = None;
            self.move_to(to, select, cx);
            return;
        }
        self.preferred_x = None;
        self.move_to(Pos::new(at.line, self.doc.line(at.line).len()), select, cx);
    }

    /// ↑ and ↓: a wrapped row up or down in the same line, or the same
    /// cell, else the line before or after, at the column the first press
    /// began in.
    fn vertical(&mut self, down: bool, select: bool, cx: &mut Context<Self>) {
        let head = self.sel.head;
        let Some(at) = self.caret_at(head) else { return };
        let x = *self.preferred_x.get_or_insert(at.x);
        let cache = self.cache.borrow();
        let Some((top, bottom)) = self.flow_span(head, &cache) else { return };
        let target_y = if down { at.y + at.h + 1. } else { at.y - 1. };
        let target = if target_y >= top && target_y < bottom {
            self.pos_at(x, target_y, &cache)
        } else {
            // The next line that has height: a table's separator has none.
            let step = |l: usize| if down { Some(l + 1).filter(|n| *n < self.doc.line_count()) } else { l.checked_sub(1) };
            let mut next = step(head.line);
            while let Some(l) = next.filter(|l| cache.entries.get(*l).is_some_and(|e| e.height <= 0.)) {
                next = step(l);
            }
            match next.map(|line| (line, cache.tops.get(line), cache.entries.get(line))) {
                // A shut piece: it opens, the caret at its top going down,
                // at its foot going up, under the column it came from.
                Some((line, Some(top), Some(entry))) if !select && self.shut.iter().any(|(f, _, _)| *f == line) => {
                    let y = if down { top + 1. } else { top + entry.height - 1. };
                    drop(cache);
                    self.open_piece_toward(line, x, y.max(0.), cx);
                    return;
                }
                Some((_, Some(top), Some(entry))) => {
                    let y = if down { top + 1. } else { top + entry.height - 1. };
                    self.pos_at(x, y.max(0.), &cache)
                }
                // A line the layout has not met yet: its start.
                Some((line, _, _)) => Some(Pos::new(line, 0)),
                None if down => Some(self.doc.end()),
                None => Some(Pos::default()),
            }
        };
        drop(cache);
        if let Some(p) = target {
            self.move_to(p, select, cx);
        }
    }

    fn page(&mut self, down: bool, cx: &mut Context<Self>) {
        let head = self.sel.head;
        let Some(at) = self.caret_at(head) else { return };
        let view = f32::from(self.scroll.bounds().size.height).max(100.);
        let x = *self.preferred_x.get_or_insert(at.x);
        let y = if down { at.y + view } else { at.y - view };
        let target = { self.pos_at(x, y.max(0.), &self.cache.borrow()) };
        if let Some(p) = target {
            self.move_to(p, false, cx);
        }
    }

    // -- the layout ---------------------------------------------------------

    fn hidden_on(&self, line: usize) -> Vec<Range<usize>> {
        self.reader().hidden_on(line)
    }

    /// The whole text laid out at `width`.
    fn relayout(&mut self, width: f32, window: &mut Window) {
        self.settle_pieces(width);
        let mut cache = self.cache.borrow_mut();
        self.reader().lay_out_note(&mut cache, width, window);
    }

    // -- what the view around draws (Emaki) ------------------------------------

    /// The lines the view around draws, as it reads the text now: set
    /// again whenever the text changes.
    pub fn set_pieces(&mut self, pieces: Vec<Piece>, cx: &mut Context<Self>) {
        if self.open_piece.is_some_and(|f| !pieces.iter().any(|p| p.first == f)) {
            self.open_piece = None;
        }
        self.pieces = pieces;
        cx.notify();
    }

    /// The bytes a piece covers, start and end.
    fn piece_ends(&self, p: &Piece) -> (Pos, Pos) {
        (Pos::new(p.first, 0), Pos::new(p.last, self.doc.line(p.last).len()))
    }

    /// Before a layout: an open piece the caret has left shuts; the
    /// selection takes a shut piece whole, its end in one carried past
    /// it; and the shut pieces are read with their heights.
    fn settle_pieces(&mut self, width: f32) {
        let n = self.doc.line_count();
        self.pieces.retain(|p| p.first <= p.last && p.last < n);
        let head = self.sel.head;
        // Esc, or the keyboard gone elsewhere: the file reads whole, the
        // piece the caret was in shut (Emaki).
        if !self.focused {
            self.open_piece = None;
        }
        if let Some(f) = self.open_piece {
            let inside = self.pieces.iter().any(|p| p.first == f && p.first <= head.line && head.line <= p.last);
            if !inside {
                self.open_piece = None;
            }
        }
        let open = self.open_piece;
        let shut: Vec<(Pos, Pos)> = self.pieces.iter().filter(|p| Some(p.first) != open).map(|p| self.piece_ends(p)).collect();
        let inside = |at: Pos, (a, b): (Pos, Pos)| a < at && at < b;
        for (a, b) in shut {
            let (anchor, head) = (self.sel.anchor, self.sel.head);
            if self.sel.is_caret() {
                continue;
            }
            let anchor = if inside(anchor, (a, b)) { if head >= anchor { a } else { b } } else { anchor };
            let head = if inside(head, (a, b)) { if head >= anchor { b } else { a } } else { head };
            self.sel = Sel { anchor, head };
        }
        let line = self.look.line_px;
        self.shut = self.pieces.iter().filter(|p| Some(p.first) != open).map(|p| {
            let h = self.piece_h.get(&p.key).filter(|(w, _)| (*w - width).abs() < 0.5).map_or((p.last - p.first + 1) as f32 * line, |(_, h)| *h);
            (p.first, p.last, h)
        }).collect();
    }

    /// Each shut piece laid out at the editor's width less what it stands
    /// out by; those in sight placed over their rows. A height that
    /// changed is kept and the text laid out again.
    fn lay_out_pieces(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> (Vec<AnyElement>, Vec<Bounds<Pixels>>) {
        let view = window.content_mask().bounds;
        let width = f32::from(bounds.size.width);
        let (sa, sb) = self.sel.range();
        let mut out = Vec::new();
        let mut held = Vec::new();
        let mut changed = false;
        let shut = self.shut.clone();
        for (first, _, _) in shut {
            let Some(piece) = self.pieces.iter().find(|p| p.first == first).cloned() else { continue };
            let Some(top) = self.cache.borrow().tops.get(first).copied() else { continue };
            let w = width + piece.out.0 + piece.out.1;
            let known = self.piece_h.get(&piece.key).filter(|(at, _)| (*at - width).abs() < 0.5).map(|(_, h)| *h);
            let y = bounds.origin.y + px(top - piece.out.2);
            let h = known.unwrap_or(0.);
            let seen = y + px(h + piece.out.2) >= view.top() && y <= view.bottom();
            if known.is_some() && !seen {
                continue;
            }
            let mut el = (piece.draw)(window, cx);
            let laid = el.layout_as_root(size(AvailableSpace::Definite(px(w)), AvailableSpace::MinContent), window, cx);
            let tall = (f32::from(laid.height) - piece.out.2).max(1.);
            if known.is_none_or(|h| (h - tall).abs() > 0.5) {
                if std::env::var_os("EMAKI_PIECE_DEBUG").is_some() {
                    eprintln!("emaki: piece {}..{} at {} measured {} (was {:?}) top {}", piece.first, piece.last, width, tall, known, top);
                }
                self.piece_h.insert(piece.key, (width, tall));
                changed = true;
            }
            if !seen {
                continue;
            }
            el.prepaint_at(point(bounds.origin.x - px(piece.out.0), y), window, cx);
            out.push(el);
            let (a, b) = self.piece_ends(&piece);
            if sa <= a && b <= sb && sa != sb {
                held.push(Bounds::new(point(bounds.origin.x - px(piece.out.0), y), size(px(w), px(tall))));
            }
        }
        if changed {
            cx.notify();
        }
        (out, held)
    }

    /// The shut piece a line is the first of.
    fn shut_piece_at(&self, line: usize) -> Option<&Piece> {
        self.shut.iter().find(|(f, l, _)| *f <= line && line <= *l).and_then(|(f, _, _)| self.pieces.iter().find(|p| p.first == *f))
    }

    /// A shut piece opened: its lines show as written, the caret at
    /// their start.
    fn open_piece_at(&mut self, first: usize, cx: &mut Context<Self>) {
        self.open_piece = Some(first);
        self.sel = Sel::caret(Pos::new(first, 0));
        self.want_scroll = true;
        cx.notify();
    }

    /// A shut piece opened by a press or an arrow at a point of the page:
    /// the caret goes where that point falls once its lines are laid out
    /// as written (`take_reach`), so nothing scrolls and the place pressed
    /// is the place written in.
    fn open_piece_toward(&mut self, first: usize, x: f32, y: f32, cx: &mut Context<Self>) {
        // How far down the drawing the point is, for a line as far down
        // the lines as written.
        let down = {
            let cache = self.cache.borrow();
            let top = cache.tops.get(first).copied().unwrap_or(0.);
            let h = self.shut.iter().find(|(f, _, _)| *f == first).map_or(1., |(_, _, h)| *h).max(1.);
            ((y - top) / h).clamp(0., 0.999)
        };
        self.open_piece = Some(first);
        self.sel = Sel::caret(Pos::new(first, 0));
        self.reach = Some((first, x, y, down));
        self.preferred_x = Some(x);
        cx.notify();
    }

    /// The caret put where a piece just opened was pressed, its lines now
    /// laid out. One line (a paragraph with a formula) takes the point
    /// itself, its words where they were. More take the line as far down
    /// the lines as the point was down the drawing, leaving out a fence
    /// line, which the drawing does not show (a callout's `:::`, a
    /// header's `---`), and the place along it under the point.
    fn take_reach(&mut self) {
        let Some((first, x, y, down)) = self.reach.take() else { return };
        let Some(last) = self.pieces.iter().find(|p| p.first == first).map(|p| p.last) else { return };
        let fence = |l: usize| {
            let t = self.doc.line(l).trim_start();
            t.starts_with(":::") || t.starts_with("---") || t.starts_with("```") || t.starts_with("~~~")
        };
        let (a, b) = if last > first + 1 && fence(first) && fence(last) { (first + 1, last - 1) } else { (first, last) };
        let line = (a + (down * (b - a + 1) as f32) as usize).min(b);
        let p = {
            let cache = self.cache.borrow();
            let (Some(top), Some(entry)) = (cache.tops.get(line).copied(), cache.entries.get(line)) else { return };
            let y = if a == b { y.clamp(top, top + (entry.height - 1.).max(0.)) } else { top + 1. };
            self.pos_at(x, y, &cache)
        };
        let Some(p) = p else { return };
        let p = if p.line == line { p } else { Pos::new(line, 0) };
        let p = self.past_marker(p);
        self.sel = Sel::caret(p);
        if let Some((grain, _, _)) = self.drag {
            self.drag = Some((grain, p, p));
        }
    }

    // -- places on the page -------------------------------------------------

    /// Where the caret at `p` stands, from the element's top left.
    fn caret_at(&self, p: Pos) -> Option<CaretAt> {
        let cache = self.cache.borrow();
        let entry = cache.entries.get(p.line)?;
        let top = *cache.tops.get(p.line)?;
        Some(match &*entry.row {
            Row::Text(r) => {
                let (x, y) = r.flow.caret(p.col);
                CaretAt::new(r.inset + x, top + r.top_pad + y, r.flow.line_h, r.flow.caret_h)
            }
            Row::Table(t) => {
                let c = t.column_of(p.col);
                let flow = t.cells.get(c)?;
                let (x, y) = flow.caret(p.col);
                CaretAt::new(t.inset + t.grid.xs.get(c)? + CELL_X + x, top + t.cell_y + y, flow.line_h, flow.caret_h)
            }
            Row::Rule { inset, line_h } => CaretAt::new(*inset, top, *line_h, Rhythm::caret(self.look.text_px, *line_h)),
            // A short blank line's caret is a line's, over its middle.
            Row::Pad { h, .. } if self.look.blank.is_some() && self.doc.line(p.line).is_empty() => {
                let line = Rhythm::of(&self.look).line;
                CaretAt::new(0., top + (*h - line) / 2., line, Rhythm::caret(self.look.text_px, line))
            }
            Row::Pad { h, .. } => CaretAt::new(0., top, *h, Rhythm::caret(self.look.text_px, *h)),
            Row::Hidden => {
                let line = entry.height.max(Rhythm::of(&self.look).line);
                CaretAt::new(0., top, line, Rhythm::caret(self.look.text_px, line))
            }
        })
    }

    /// The top and the foot of the words a place stands in, from the
    /// element's top: its row, or its cell.
    fn flow_span(&self, p: Pos, cache: &Cache) -> Option<(f32, f32)> {
        let entry = cache.entries.get(p.line)?;
        let top = *cache.tops.get(p.line)?;
        Some(match &*entry.row {
            Row::Table(t) => {
                let flow = t.cells.get(t.column_of(p.col))?;
                (top + t.cell_y, top + t.cell_y + flow.height)
            }
            _ => (top, top + entry.height),
        })
    }

    /// The line under `y`: a line of no height is passed by for the one
    /// that has it.
    fn line_at(&self, y: f32, cache: &Cache) -> usize {
        let line = cache.tops.partition_point(|t| *t <= y).saturating_sub(1).min(cache.entries.len().saturating_sub(1));
        let mut line = line;
        while line > 0 && cache.entries[line].height <= 0. {
            line -= 1;
        }
        line
    }

    /// The place in the text at a point of the page, from the element's
    /// top left: the row under `y`, the character nearest `x` in it.
    fn pos_at(&self, x: f32, y: f32, cache: &Cache) -> Option<Pos> {
        if cache.entries.is_empty() || cache.tops.len() < cache.entries.len() {
            return None;
        }
        let line = self.line_at(y, cache);
        // The layout may be a text's that has since lost lines.
        if line >= self.doc.line_count() || line >= self.analysis.line_spans.len() {
            return Some(self.doc.end());
        }
        let entry = &cache.entries[line];
        let text_len = self.doc.line(line).len();
        let top = cache.tops[line];
        let p = match &*entry.row {
            Row::Text(r) => {
                let mut col = r.flow.col_at(x - r.inset, y - top - r.top_pad);
                // A click at a hidden span's right edge stands past the span.
                let start = self.analysis.line_starts[line];
                let hidden_col = r.flow.paras.iter().find(|p| p.from <= col && col <= p.to).is_some_and(|p| p.shown.is_hidden(col));
                for &six in &self.analysis.line_spans[line] {
                    let s = &self.analysis.spans[six];
                    if s.close.start >= start && s.close.start - start == col && s.close.end > s.close.start && hidden_col {
                        col = s.close.end - start;
                    }
                }
                Pos::new(line, col.min(text_len))
            }
            Row::Table(t) => {
                let c = t.column_at(x - t.inset);
                let flow = t.cells.get(c)?;
                Pos::new(line, flow.col_at(x - t.inset - t.grid.xs.get(c)? - CELL_X, y - top - t.cell_y).min(text_len))
            }
            // A piece's row: a place inside it, which takes it whole.
            Row::Pad { .. } if self.shut.iter().any(|(f, _, _)| *f == line) => Pos::new(line, self.doc.line(line).chars().next().map_or(0, |c| c.len_utf8())),
            _ => Pos::new(line, 0),
        };
        Some(self.doc.clamp(p))
    }

    /// What a press lands on besides words: a task's box, a link.
    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let cache = self.cache.borrow();
        if cache.entries.is_empty() || cache.tops.len() < cache.entries.len() {
            return None;
        }
        let line = self.line_at(y, &cache);
        let entry = cache.entries.get(line)?;
        let top = cache.tops[line];
        match &*entry.row {
            Row::Text(r) => {
                for w in &r.widgets {
                    if let Widget::Task { bounds, .. } = w {
                        let b = Bounds::new(point(bounds.origin.x - px(3.), bounds.origin.y + px(top) - px(3.)), bounds.size + size(px(6.), px(6.)));
                        if b.contains(&point(px(x), px(y))) {
                            return Some(Hit::Task(line));
                        }
                    }
                }
                r.flow.link_at(x - r.inset, y - top - r.top_pad).map(Hit::Link)
            }
            Row::Table(t) => {
                let c = t.column_at(x - t.inset);
                t.cells.get(c)?.link_at(x - t.inset - t.grid.xs.get(c)? - CELL_X, y - top - t.cell_y).map(Hit::Link)
            }
            _ => None,
        }
    }

    /// Whether the scroller was moved.
    fn scroll_to_caret(&mut self, bounds: Bounds<Pixels>, window: &Window) -> bool {
        self.want_scroll = false;
        let Some((y, h)) = self.caret_at(self.sel.head).map(|at| (at.y, at.h)) else { return false };
        let view = window.content_mask().bounds;
        let caret_top = f32::from(bounds.top()) + y;
        let caret_bottom = caret_top + h;
        let (vt, vb) = (f32::from(view.top()), f32::from(view.bottom()));
        // No more margin than leaves the caret's line room between the two.
        let margin = self.scroll_margin.min(((vb - vt - h) / 2.).max(0.));
        let offset = self.scroll.offset();
        let mut dy = 0.;
        if caret_top < vt + margin {
            dy = vt + margin - caret_top;
        } else if caret_bottom > vb - margin {
            dy = (vb - margin) - caret_bottom;
        }
        if dy.abs() > 0.5 {
            let max = f32::from(self.scroll.max_offset().y);
            let y = (f32::from(offset.y) + dy).clamp(-max.max(0.), 0.);
            if (y - f32::from(offset.y)).abs() > 0.5 {
                self.scroll.set_offset(point(offset.x, px(y)));
                return true;
            }
        }
        false
    }
}

enum Hit {
    Task(usize),
    Link(Target),
}

// -- the input method -------------------------------------------------------

impl EntityInputHandler for Editor {
    /// False, so a key goes to the bindings before an input method. gpui
    /// on a Mac hands every printable key, ⌥ chords among them, to an
    /// active Chinese, Japanese or Korean input method first while the
    /// handler says it takes text, and the method swallows the ⌃ and ⌥
    /// chords the window binds. Nothing of typing changes: a key no
    /// binding takes still reaches the method and composes, and this is
    /// read nowhere else on a Mac.
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        false
    }

    fn text_for_range(&mut self, range_utf16: Range<usize>, adjusted_range: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let a = self.doc.pos_of_utf16(range_utf16.start);
        let b = self.doc.pos_of_utf16(range_utf16.end);
        *adjusted_range = Some(self.doc.utf16_of(a)..self.doc.utf16_of(b));
        Some(self.doc.slice(a, b))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let (a, b) = self.sel.range();
        Some(UTF16Selection { range: self.doc.utf16_of(a)..self.doc.utf16_of(b), reversed: self.sel.head < self.sel.anchor })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.map(|(a, b)| self.doc.utf16_of(a)..self.doc.utf16_of(b))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let (a, b) = match range_utf16 {
            Some(r) => (self.doc.pos_of_utf16(r.start), self.doc.pos_of_utf16(r.end)),
            None => match self.marked {
                Some((a, b)) => (a, b),
                None => self.sel.range(),
            },
        };
        self.marked = None;
        let text = self.cell_words(text);
        let end = self.doc.replace(a, b, &text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, new_selected_range: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let (a, b) = match range_utf16 {
            Some(r) => (self.doc.pos_of_utf16(r.start), self.doc.pos_of_utf16(r.end)),
            None => match self.marked {
                Some((a, b)) => (a, b),
                None => self.sel.range(),
            },
        };
        let end = self.doc.replace(a, b, new_text, self.sel);
        let start_u = self.doc.utf16_of(a);
        self.marked = (a != end).then_some((a, end));
        self.sel = match new_selected_range {
            Some(r) => Sel { anchor: self.doc.pos_of_utf16(start_u + r.start), head: self.doc.pos_of_utf16(start_u + r.end) },
            None => Sel::caret(end),
        };
        self.reanalyze();
        if !self.find_query.is_empty() {
            self.refind();
        }
        self.text_changed();
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let at = self.caret_at(self.doc.pos_of_utf16(range_utf16.start))?;
        Some(Bounds::new(point(self.origin.x + px(at.x), self.origin.y + px(at.y)), size(px(2.), px(at.h))))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let local = (f32::from(point.x - self.origin.x), f32::from(point.y - self.origin.y));
        let p = self.pos_at(local.0, local.1, &self.cache.borrow())?;
        Some(self.doc.utf16_of(p))
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.doc.utf16_len())
    }
}

// -- the view ---------------------------------------------------------------

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The window says when the keyboard comes and goes, so a view kept
        // between frames hears it too.
        if self.focus_subs.is_none() {
            let came = cx.on_focus(&self.focus, window, |this, _, cx| this.set_focused(true, true, cx));
            let went = cx.on_blur(&self.focus, window, |this, _, cx| this.set_focused(false, true, cx));
            self.focus_subs = Some((came, went));
        }
        div()
            .id("live-editor")
            .key_context(LIVE_CONTEXT)
            .track_focus(&self.focus)
            .w_full()
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::new_line))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_line_back))
            .on_action(cx.listener(|this, _: &Left, _, cx| this.left(false, cx)))
            .on_action(cx.listener(|this, _: &Right, _, cx| this.right(false, cx)))
            .on_action(cx.listener(|this, _: &Up, _, cx| this.vertical(false, false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.vertical(true, false, cx)))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.left(true, cx)))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.right(true, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.vertical(false, true, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.vertical(true, true, cx)))
            .on_action(cx.listener(|this, _: &WordLeft, _, cx| this.word_left(false, cx)))
            .on_action(cx.listener(|this, _: &WordRight, _, cx| this.word_right(false, cx)))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| this.word_left(true, cx)))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| this.word_right(true, cx)))
            .on_action(cx.listener(|this, _: &LineStart, _, cx| this.line_start(false, cx)))
            .on_action(cx.listener(|this, _: &LineEnd, _, cx| this.line_end(false, cx)))
            .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| this.line_start(true, cx)))
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| this.line_end(true, cx)))
            .on_action(cx.listener(|this, _: &DocStart, _, cx| this.move_to(Pos::default(), false, cx)))
            .on_action(cx.listener(|this, _: &DocEnd, _, cx| {
                let end = this.doc.end();
                this.move_to(end, false, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectDocStart, _, cx| this.move_to(Pos::default(), true, cx)))
            .on_action(cx.listener(|this, _: &SelectDocEnd, _, cx| {
                let end = this.doc.end();
                this.move_to(end, true, cx)
            }))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(false, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(true, cx)))
            .on_action(cx.listener(Self::bold))
            .on_action(cx.listener(Self::italic))
            .on_action(cx.listener(Self::code))
            .on_action(cx.listener(Self::toggle_task))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .child(EditorElement { editor: cx.entity().clone() })
            .into_any_element()
    }
}

// -- the element ------------------------------------------------------------

/// The element that lays the text out and paints it, one child of the
/// scroller the view around it draws. It is as tall as the text.
struct EditorElement {
    editor: Entity<Editor>,
}

struct Prepainted {
    hitbox: Hitbox,
    /// The hitbox with room each side for a table's handles, which stand
    /// out in the margin.
    wide: Hitbox,
    /// The pieces in sight, laid out over their rows, and those the
    /// selection holds whole, to be washed in its color.
    pieces: Vec<AnyElement>,
    held: Vec<Bounds<Pixels>>,
}

/// How much of a table's handles to paint, and where.
struct Controls {
    table: cells::TableBox,
    row: (f32, f32),
    col: usize,
    /// Which handle or edge the pointer is on.
    on: Option<cells::Control>,
    opacity: f32,
}

/// What paint takes out of the editor before it draws, since drawing
/// needs the app the editor is read through.
struct PaintInfo {
    focus: FocusHandle,
    pointer: CursorStyle,
    inks: Colors,
    cache: Rc<RefCell<Cache>>,
    /// Each code block's first and last line, and where its plate begins.
    blocks: Vec<(usize, usize, f32)>,
    /// Each table's first and last line.
    tables: Vec<(usize, usize)>,
    sel: Sel,
    marked: Option<(Pos, Pos)>,
    caret: Option<Bounds<Pixels>>,
    find: Rc<Vec<(Pos, Pos)>>,
    /// The cell the caret is in, from the element's top left.
    cell: Option<Bounds<Pixels>>,
    controls: Option<Controls>,
    /// Where a dragged row or column would go: a line, from the element's top left.
    drop_line: Option<Bounds<Pixels>>,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// Words, with what stands behind and over them: chips, what a find
/// found, the selection, and an input method's underline.
fn paint_flow(flow: &super::layout::Flow, origin: Point<Pixels>, line: usize, past_end: bool, info: &PaintInfo, window: &mut Window, cx: &mut App) {
    let inks = &info.inks;
    let clear = transparent_black();
    let lh = flow.line_h;
    for (b, color) in flow.chips(origin) {
        let b = Bounds::new(point(b.origin.x - px(flow.pad), b.origin.y + px(lh * 0.08)), size(b.size.width + px(flow.pad * 2.), px(lh * 0.84)));
        window.paint_quad(quad(b, px(4.), color, px(0.), clear, BorderStyle::default()));
    }
    // The find's matches on this line, washed; the one the find stands on
    // is the selection, painted after.
    if !info.find.is_empty() {
        let wash = Hsla { s: 0.1, ..inks.selection };
        for &(fa, fb) in info.find.iter().filter(|(a, b)| a.line <= line && line <= b.line) {
            let from = if fa.line == line { fa.col } else { 0 };
            let to = if fb.line == line { fb.col } else { usize::MAX };
            for b in flow.boxes(from, to, origin) {
                window.paint_quad(quad(b, px(2.), wash, px(0.), clear, BorderStyle::default()));
            }
        }
    }
    // The selection, as far as it lies on this line.
    let (sa, sb) = info.sel.range();
    if sa != sb && sa.line <= line && line <= sb.line {
        let from = if sa.line == line { sa.col } else { 0 };
        let to = if sb.line == line { sb.col } else { usize::MAX };
        let mut boxes = flow.boxes(from, to, origin);
        if past_end && sb.line > line {
            // The line's end is selected too: a little past the last word.
            if let Some(last) = boxes.last_mut() {
                last.size.width += px(6.);
            } else {
                boxes.push(Bounds::new(origin, size(px(6.), px(lh))));
            }
        }
        for b in boxes {
            window.paint_quad(fill(b, inks.selection));
        }
    }
    let mut marked_boxes = Vec::new();
    if let Some((ma, mb)) = info.marked.filter(|(ma, _)| ma.line == line) {
        let to = if mb.line == line { mb.col } else { usize::MAX };
        marked_boxes = flow.boxes(ma.col, to, origin);
    }
    flow.paint(origin, window, cx);
    // An input method's composition, underlined.
    for b in marked_boxes {
        window.paint_quad(fill(Bounds::new(point(b.origin.x, b.origin.y + px(lh) - px(2.)), size(b.size.width, px(1.))), inks.ink));
    }
}

/// A code block's language mark: how far in from the plate's top and
/// right it sits, its icon's size, and its name's (`layout.rs` shapes the
/// name at `MARK_PX`).
const MARK_IN: f32 = 9.;
const MARK_ICON: f32 = 13.;
/// The room the icons leave inside their box, and a letter after its ink.
const MARK_ICON_PAD: f32 = 1.5;
const MARK_BEARING: f32 = 0.5;
pub(crate) const MARK_PX: f32 = 11.5;

/// The ground and the outline of a table, the head's tint inside it. The
/// lines between its cells are each row's.
fn paint_plate(left: f32, top: f32, width: f32, height: f32, head: f32, inks: &Colors, window: &mut Window) {
    let r = px(TABLE_R);
    let corners = Corners { top_left: r, top_right: r, bottom_left: px(0.), bottom_right: px(0.) };
    window.paint_quad(quad(Bounds::new(point(px(left), px(top)), size(px(width), px(head))), corners, inks.plate.opacity(0.7), px(0.), transparent_black(), BorderStyle::default()));
    window.paint_quad(quad(Bounds::new(point(px(left), px(top)), size(px(width), px(height))), r, clear_of(inks.border), px(1.), inks.border, BorderStyle::default()));
}

/// One row at a place on the page.
#[allow(clippy::too_many_arguments)]
fn paint_row(row: &Row, ox: f32, oy: f32, height: f32, full_w: Pixels, line: usize, info: &PaintInfo, window: &mut Window, cx: &mut App) {
    let inks = &info.inks;
    match row {
        Row::Text(r) => paint_text(r, ox, oy, height, line, info, window, cx),
        Row::Pad { lang, blank, .. } => {
            // A blank line in the selection, its end taken: a strip as
            // wide as the one past a line of words, its whole height.
            let (sa, sb) = info.sel.range();
            if *blank && sa != sb && sa.line <= line && line < sb.line {
                window.paint_quad(fill(Bounds::new(point(px(ox), px(oy)), size(px(6.), px(height))), inks.selection));
            }
            if let Some((l, icon, color)) = lang {
                // The mark sits in the plate's corner as far from its top as
                // from its right, measured to what shows: the icon's top and
                // the name's last letter. The name's capitals are centered on
                // the icon, and its descenders stay in the fence's row.
                let name_w = f32::from(l.width());
                let x = f32::from(full_w) - name_w - (MARK_IN - MARK_BEARING);
                let icon_top = oy + MARK_IN - MARK_ICON_PAD;
                let cap = 0.727 * MARK_PX;
                let baseline = icon_top + MARK_ICON / 2. + cap / 2.;
                let tall = l.ascent + l.descent;
                let _ = l.paint(point(px(ox + x), px(baseline) - l.ascent), tall, TextAlign::Left, None, window, cx);
                let ib = Bounds::new(point(px(ox + x - MARK_ICON - 5.), px(icon_top)), size(px(MARK_ICON), px(MARK_ICON)));
                let _ = window.paint_svg(ib, SharedString::from(*icon), None, TransformationMatrix::unit(), *color, cx);
            }
        }
        Row::Hidden => {}
        Row::Rule { inset, line_h } => {
            let b = Bounds::new(point(px(ox + inset), px(oy + line_h / 2. - 1.)), size(full_w - px(*inset), px(2.)));
            window.paint_quad(fill(b, inks.border));
        }
        Row::Table(t) => {
            let left = ox + t.inset;
            let total = t.grid.width;
            if !t.head {
                window.paint_quad(fill(Bounds::new(point(px(left), px(oy)), size(px(total), px(1.))), inks.border));
            }
            for (c, flow) in t.cells.iter().enumerate() {
                let Some(at) = t.grid.xs.get(c) else { continue };
                let x = left + at;
                if c > 0 {
                    window.paint_quad(fill(Bounds::new(point(px(x), px(oy)), size(px(1.), px(t.height))), inks.border));
                }
                paint_flow(flow, point(px(x + CELL_X), px(oy + t.cell_y)), line, false, info, window, cx);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_text(r: &TextRow, ox: f32, oy: f32, height: f32, line: usize, info: &PaintInfo, window: &mut Window, cx: &mut App) {
    let inks = &info.inks;
    let clear = transparent_black();
    let text_origin = point(px(ox + r.inset), px(oy + r.top_pad));
    paint_flow(&r.flow, text_origin, line, true, info, window, cx);
    let lh = px(r.flow.line_h);
    for w in &r.widgets {
        match w {
            Widget::Bullet { x, y, r: radius } => {
                let b = Bounds::new(point(px(ox + x - radius), px(oy + y - radius)), size(px(radius * 2.), px(radius * 2.)));
                window.paint_quad(quad(b, px(*radius), inks.muted, px(0.), clear, BorderStyle::default()));
            }
            Widget::Number { line: shaped, x, y } => {
                let _ = shaped.paint(point(px(ox + x), px(oy + y)), lh, TextAlign::Left, None, window, cx);
            }
            Widget::Task { bounds: b, done } => {
                let b = Bounds::new(point(px(ox) + b.origin.x, px(oy) + b.origin.y), b.size);
                if *done {
                    window.paint_quad(quad(b, px(4.), inks.accent, px(0.), clear, BorderStyle::default()));
                    let inner = Bounds::new(point(b.origin.x + b.size.width * 0.15, b.origin.y + b.size.height * 0.15), size(b.size.width * 0.7, b.size.height * 0.7));
                    let _ = window.paint_svg(inner, "icons/check.svg".into(), None, TransformationMatrix::unit(), inks.on_accent, cx);
                } else {
                    window.paint_quad(quad(b, px(4.), clear_of(inks.muted), px(1.5), inks.muted.opacity(0.7), BorderStyle::default()));
                }
            }
            Widget::Guide { x } => window.paint_quad(fill(Bounds::new(point(px(ox + x), px(oy)), size(px(1.), px(height))), inks.border)),
            Widget::Bar { x } => window.paint_quad(fill(Bounds::new(point(px(ox + x), px(oy)), size(px(2.), px(height))), inks.accent.opacity(0.7))),
        }
    }
}

/// A table's handles and the strips that add to it, over the page.
fn paint_controls(c: &Controls, ox: f32, oy: f32, inks: &Colors, window: &mut Window, cx: &mut App) {
    use cells::{Control, ADD, ADD_GAP, GRIP_LONG, GRIP_SHORT};
    let k = c.opacity;
    let b = &c.table;
    let clear = transparent_black();
    // A handle: its dots, two rows of three along its long side.
    let grip = |bounds: Bounds<Pixels>, upright: bool, on: bool, window: &mut Window| {
        let ground = if on { inks.accent } else { inks.ground };
        let ink = if on { inks.on_accent } else { inks.muted };
        window.paint_quad(quad(bounds, px(4.), ground.opacity(k), px(1.), (if on { inks.accent } else { inks.border }).opacity(k), BorderStyle::default()));
        let (cx0, cy0) = (bounds.origin.x + bounds.size.width / 2., bounds.origin.y + bounds.size.height / 2.);
        for long in [-4.5, 0., 4.5] {
            for short in [-2., 2.] {
                let (dx, dy) = if upright { (short, long) } else { (long, short) };
                let dot = Bounds::new(point(cx0 + px(dx - 1.), cy0 + px(dy - 1.)), size(px(2.), px(2.)));
                window.paint_quad(quad(dot, px(1.), ink.opacity(k), px(0.), clear, BorderStyle::default()));
            }
        }
    };
    let strip = |bounds: Bounds<Pixels>, on: bool, window: &mut Window, cx: &mut App| {
        let ground = if on { inks.accent.opacity(0.14 * k) } else { inks.plate.opacity(0.6 * k) };
        window.paint_quad(quad(bounds, px(4.), ground, px(0.), clear, BorderStyle::default()));
        let side = px(12.);
        let at = point(bounds.origin.x + (bounds.size.width - side) / 2., bounds.origin.y + (bounds.size.height - side) / 2.);
        let ink = if on { inks.accent } else { inks.muted };
        let _ = window.paint_svg(Bounds::new(at, size(side, side)), "icons/plus.svg".into(), None, TransformationMatrix::unit(), ink.opacity(k), cx);
    };
    let (left, top, right, bottom) = (ox + b.left, oy + b.top, ox + b.right, oy + b.bottom);
    strip(Bounds::new(point(px(right + ADD_GAP), px(top)), size(px(ADD), px(bottom - top))), c.on == Some(Control::AddColumn), window, cx);
    strip(Bounds::new(point(px(left), px(bottom + ADD_GAP)), size(px(right - left), px(ADD))), c.on == Some(Control::AddRow), window, cx);
    let row_mid = oy + (c.row.0 + c.row.1) / 2.;
    grip(Bounds::new(point(px(left - GRIP_SHORT / 2.), px(row_mid - GRIP_LONG / 2.)), size(px(GRIP_SHORT), px(GRIP_LONG))), true, matches!(c.on, Some(Control::RowGrip(_))), window);
    if let (Some(x), Some(w)) = (b.grid.xs.get(c.col), b.grid.cols.get(c.col)) {
        let mid = left + x + w / 2.;
        grip(Bounds::new(point(px(mid - GRIP_LONG / 2.), px(top - GRIP_SHORT / 2.)), size(px(GRIP_LONG), px(GRIP_SHORT))), false, matches!(c.on, Some(Control::ColumnGrip(_))), window);
    }
    if let Some(Control::Edge(col)) = c.on {
        if let (Some(x), Some(w)) = (b.grid.xs.get(col), b.grid.cols.get(col)) {
            window.paint_quad(fill(Bounds::new(point(px(left + x + w - 1.), px(top)), size(px(2.), px(bottom - top))), inks.accent));
        }
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        // Whether the keyboard is in the editor, known before the layout,
        // since the marks around the caret show only while it is; and the
        // caret begins to blink as the keyboard arrives.
        self.editor.update(cx, |e, cx| {
            let now = e.focus.is_focused(window);
            // The window coming back to the front: the caret shows and
            // blinks again. Behind another app's it stops, so the window
            // is not drawn twice a second for it there.
            let away = !window.is_window_active();
            let back = e.away && !away;
            e.away = away;
            e.set_focused(now, false, cx);
            if back && e.focused {
                e.kick_blink(cx);
            }
        });
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let editor = self.editor.clone();
        // The height is the text's at the width it is given.
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            // Asked with no width (the layout measuring what the editor
            // would take, before it gives it a width): the last layout's
            // answer. Laid out at a stand-in width, a long file was laid out
            // whole at it and again at its own, several times a frame.
            if width.is_none() {
                let c = editor.read(cx).cache.borrow();
                if !c.entries.is_empty() {
                    return size(px(c.width), px(c.total.max(1.)));
                }
            }
            let width = f32::from(width.unwrap_or(px(600.))).max(80.);
            let total = editor.update(cx, |e, _| {
                e.relayout(width, window);
                e.cache.borrow().total
            });
            size(px(width), px(total.max(1.)))
        });
        (id, ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Prepainted {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let far = px(cells::ADD + cells::ADD_GAP + 4.);
        let wide = window.insert_hitbox(Bounds::new(point(bounds.origin.x - far, bounds.origin.y), size(bounds.size.width + far * 2., bounds.size.height + far)), HitboxBehavior::Normal);
        let moved = self.editor.update(cx, |e, _| {
            e.origin = bounds.origin;
            e.painted = true;
            e.take_reach();
            e.want_scroll && e.scroll_to_caret(bounds, window)
        });
        // The scroller read its offset before its child was laid out, so a
        // move made here shows at the next frame, which is asked for: a
        // notify made while a frame is drawn asks for none.
        if moved {
            window.request_animation_frame();
        }
        // Each piece over its row, after the editor's own hitboxes, so a
        // press on a piece's fold or link is the piece's. Every piece is
        // measured at a width once, in sight or not, so nothing moves
        // when one comes into sight.
        let (pieces, held) = self.editor.update(cx, |e, cx| e.lay_out_pieces(bounds, window, cx));
        Prepainted { hitbox, wide, pieces, held }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), prepaint: &mut Prepainted, window: &mut Window, cx: &mut App) {
        let editor = self.editor.clone();
        let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let info = editor.read(cx).paint_info();
        let inks = info.inks;
        window.handle_input(&info.focus, ElementInputHandler::new(bounds, editor.clone()), cx);
        window.set_cursor_style(info.pointer, if info.controls.is_some() { &prepaint.wide } else { &prepaint.hitbox });

        let cache = info.cache.borrow();
        let view = window.content_mask().bounds;
        let visible = |top: f32, h: f32| oy + top + h >= f32::from(view.top()) - 1. && oy + top <= f32::from(view.bottom()) + 1.;
        let first = cache.tops.partition_point(|t| *t <= f32::from(view.top()) - oy).saturating_sub(1);

        // The plates behind code blocks and tables, before anything on them.
        for &(first_line, last_line, inset) in &info.blocks {
            let (Some(top), Some(last), Some(last_top)) = (cache.tops.get(first_line), cache.entries.get(last_line), cache.tops.get(last_line)) else { continue };
            let bottom = last_top + last.height;
            if !visible(*top, bottom - top) {
                continue;
            }
            let b = Bounds::new(point(px(ox + inset), px(oy + top)), size(bounds.size.width - px(inset), px(bottom - top)));
            window.paint_quad(quad(b, px(8.), inks.plate, px(1.), inks.border, BorderStyle::default()));
        }
        for &(first_line, last_line) in &info.tables {
            let (Some(top), Some(last), Some(last_top), Some(head)) = (cache.tops.get(first_line), cache.entries.get(last_line), cache.tops.get(last_line), cache.entries.get(first_line)) else { continue };
            let bottom = last_top + last.height;
            if let (Row::Table(t), true) = (&*head.row, visible(*top, bottom - top)) {
                paint_plate(ox + t.inset, oy + top, t.grid.width, bottom - top, head.height, &inks, window);
            }
        }

        for line in first..cache.entries.len().min(cache.tops.len()) {
            let entry = &cache.entries[line];
            let top = cache.tops[line];
            if oy + top > f32::from(view.bottom()) + 1. {
                break;
            }
            if entry.height <= 0. || !visible(top, entry.height) {
                continue;
            }
            paint_row(&entry.row, ox, oy + top, entry.height, bounds.size.width, line, &info, window, cx);
        }
        drop(cache);
        if let Some(b) = info.cell {
            window.paint_quad(quad(Bounds::new(point(px(ox) + b.origin.x, px(oy) + b.origin.y), b.size), px(3.), clear_of(inks.accent), px(1.5), inks.accent, BorderStyle::default()));
        }
        if let Some(c) = &info.controls {
            window.with_content_mask(None, |window| paint_controls(c, ox, oy, &inks, window, cx));
            if c.opacity < 1. {
                window.request_animation_frame();
            }
        }
        if let Some(b) = info.drop_line {
            window.paint_quad(fill(Bounds::new(point(px(ox) + b.origin.x, px(oy) + b.origin.y), b.size), inks.accent));
        }
        if let Some(b) = info.caret {
            window.paint_quad(fill(b, inks.caret));
        }
        for piece in &mut prepaint.pieces {
            piece.paint(window, cx);
        }
        for b in &prepaint.held {
            window.paint_quad(fill(*b, inks.selection));
        }

        // The pointer: a press places the caret or takes a widget, a drag
        // selects, a press on a link follows it.
        let (hitbox, wide) = (prepaint.hitbox.clone(), prepaint.wide.clone());
        let ed = editor.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || ev.button != MouseButton::Left || !wide.is_hovered(window) {
                return;
            }
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y));
            let taken = ed.update(cx, |e, cx| e.press_control(x, y, ev.position, cx));
            if taken {
                crate::workbench::swallow_click(window, cx);
                return;
            }
            if !hitbox.is_hovered(window) {
                return;
            }
            // The press is the editor's: the view around it hears a press
            // off the words only.
            ed.update(cx, |e, cx| {
                // As the text reads a press selects as anywhere else, for
                // ⌘C and the find to work on, and a link waits for the
                // release, so a selection can begin on one.
                let reading = e.mode == Mode::Reading;
                if !e.focus.is_focused(window) {
                    window.focus(&e.focus, cx);
                }
                match e.hit(x, y) {
                    Some(Hit::Task(line)) if !e.read_only => {
                        e.toggle_task_on(line, cx);
                        e.doc.end_step();
                    }
                    Some(Hit::Link(target)) if !reading && (ev.modifiers.secondary() || (ev.click_count == 1 && !e.writing_link(x, y))) => e.follow(target, cx),
                    hit => {
                        e.pressed_link = match hit {
                            Some(Hit::Link(target)) if reading && (ev.modifiers.secondary() || ev.click_count == 1) => Some(target),
                            _ => None,
                        };
                        let Some(p) = e.pos_at(x, y, &e.cache.borrow()) else { return };
                        // A press on a piece opens it, the caret where the
                        // press was (Emaki); ⇧ and a press take it whole.
                        if !ev.modifiers.shift {
                            if let Some(first) = e.shut_piece_at(p.line).map(|piece| piece.first) {
                                e.open_piece_toward(first, x, y, cx);
                                e.drag = Some((Grain::Char, e.sel.head, e.sel.head));
                                e.kick_blink(cx);
                                return;
                            }
                        }
                        let grain = match ev.click_count {
                            1 => Grain::Char,
                            2 => Grain::Word,
                            _ => Grain::Line,
                        };
                        let (a, b) = e.grain_range(p, grain);
                        if ev.modifiers.shift && grain == Grain::Char {
                            e.sel.head = e.held_to(e.sel.anchor, p);
                        } else {
                            e.sel = Sel { anchor: a, head: b };
                        }
                        e.drag = Some((grain, a, b));
                        e.preferred_x = None;
                        e.kick_blink(cx);
                        cx.notify();
                    }
                }
            });
            crate::workbench::swallow_click(window, cx);
        });
        let (hitbox, wide, ed) = (prepaint.hitbox.clone(), prepaint.wide.clone(), editor.clone());
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || ev.button != MouseButton::Right || !wide.is_hovered(window) {
                return;
            }
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y));
            let on_control = ed.read(cx).control_at(x, y).is_some_and(|c| matches!(c, cells::Control::RowGrip(_) | cells::Control::ColumnGrip(_)));
            if !on_control && !hitbox.is_hovered(window) {
                return;
            }
            cx.stop_propagation();
            ed.update(cx, |e, cx| {
                let at = e.right_press(x, y, window, cx);
                cx.emit(EditorEvent::Menu(ev.position, at));
                cx.notify();
            });
        });
        let ed = editor.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y));
            ed.update(cx, |e, cx| {
                if ev.pressed_button == Some(MouseButton::Left) && e.drag_control(x, y, cx) {
                    return;
                }
                if let (Some(_), Some(MouseButton::Left)) = (e.drag, ev.pressed_button) {
                    e.drag_at(x, y, cx);
                    if y < 0. || y > e.cache.borrow().total {
                        cx.emit(EditorEvent::DragPast);
                    }
                } else {
                    e.pointer_at = (x, y);
                    e.point_at_table(x, y, cx);
                    let inside = window.mouse_position().x >= bounds.origin.x;
                    let pointer = match e.control_at(x, y) {
                        Some(cells::Control::Edge(_)) => CursorStyle::ResizeColumn,
                        Some(_) => CursorStyle::PointingHand,
                        None => match e.hit(x, y) {
                            Some(Hit::Link(_)) if inside && (e.mode == Mode::Reading || ev.modifiers.secondary() || !e.writing_link(x, y)) => CursorStyle::PointingHand,
                            Some(Hit::Task(_)) if inside && !e.read_only => CursorStyle::PointingHand,
                            _ => CursorStyle::IBeam,
                        },
                    };
                    if pointer != e.pointer {
                        e.pointer = pointer;
                        cx.notify();
                    }
                }
            });
        });
        let ed = editor;
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                ed.update(cx, |e, cx| {
                    if e.drop_control(cx) {
                        return;
                    }
                    if let Some(target) = e.pressed_link.take() {
                        e.follow(target, cx);
                    }
                    // A line half out of sight takes a press on its half in
                    // sight. It is brought in here, not at the press, so
                    // the words do not move under a drag about to begin.
                    // A piece a press opened puts the caret where the
                    // press was, in sight already.
                    if e.drag.take().is_some() && e.reach.is_none() {
                        e.want_scroll = true;
                        cx.notify();
                    }
                });
            }
        });
    }
}

impl Editor {
    /// What the drawing needs, read once.
    fn paint_info(&self) -> PaintInfo {
        let (sa, _) = self.sel.range();
        let origin = self.origin;
        let shift = |b: Bounds<Pixels>| Bounds::new(point(origin.x + b.origin.x, origin.y + b.origin.y), b.size);
        let caret = (self.sel.is_caret() && self.caret_shown()).then(|| self.caret_at(sa)).flatten().map(|c| shift(Bounds::new(point(px(c.x), px(c.bar_y)), size(px(CARET_W), px(c.bar_h)))));
        let on = self.hover.and_then(|_| self.control_at(self.pointer_at.0, self.pointer_at.1));
        let cache = self.cache.borrow();
        let span = |line: usize| Some((*cache.tops.get(line)?, cache.entries.get(line)?.height));
        let cell = self.revealing().then(|| self.cell_of(self.sel.head)).flatten().and_then(|at| {
            let t = self.table(at.table)?;
            let b = cells::table_box(t, &cache)?;
            let (top, height) = span(t.line_of(at.row))?;
            let (x, w) = (*b.grid.xs.get(at.col)?, *b.grid.cols.get(at.col)?);
            Some(Bounds::new(point(px(b.left + x), px(top)), size(px(w), px(height + 1.))))
        });
        let controls = self.hover.and_then(|h| {
            let t = self.table(h.first)?;
            let table = cells::table_box(t, &cache)?;
            let (top, height) = span(t.line_of(h.row))?;
            let opacity = (h.since.elapsed().as_secs_f32() / 0.12).min(1.);
            Some(Controls { table, row: (top, top + height), col: h.col, on, opacity })
        });
        let drop_line = self.table_drag.as_ref().and_then(|d| match d {
            cells::TableDrag::Row { first, row, to, moved: true, .. } => {
                let t = self.table(*first)?;
                let b = cells::table_box(t, &cache)?;
                let (top, height) = span(t.line_of(*to))?;
                let y = if to > row { top + height } else { top };
                Some(Bounds::new(point(px(b.left), px(y - 1.)), size(px(b.right - b.left), px(2.))))
            }
            cells::TableDrag::Column { first, col, to, moved: true, .. } => {
                let b = cells::table_box(self.table(*first)?, &cache)?;
                let x = b.left + b.grid.xs.get(*to)? + if to > col { *b.grid.cols.get(*to)? } else { 0. };
                Some(Bounds::new(point(px(x - 1.), px(b.top)), size(px(2.), px(b.bottom - b.top))))
            }
            _ => None,
        });
        let reader = self.reader();
        PaintInfo {
            focus: self.focus.clone(),
            pointer: self.pointer,
            inks: self.look.colors,
            cache: self.cache.clone(),
            blocks: self.analysis.code_blocks.iter().filter(|b| b.markup.is_none()).filter_map(|b| Some((b.first, b.last, reader.base_inset(self.analysis.lines.get(b.first)?)))).collect(),
            tables: self.analysis.tables.iter().map(|t| (t.first, t.last)).collect(),
            sel: self.sel,
            marked: self.marked,
            caret,
            find: self.find.clone(),
            cell,
            controls,
            drop_line,
        }
    }

    /// A right click: what it was on, with the selection put as a Mac's
    /// text puts it.
    fn right_press(&mut self, x: f32, y: f32, window: &mut Window, cx: &mut Context<Self>) -> MenuAt {
        if let Some(control) = self.control_at(x, y) {
            let hover = self.hover;
            let scope = match control {
                cells::Control::RowGrip(_) => Some(Scope::Row),
                cells::Control::ColumnGrip(_) => Some(Scope::Column),
                _ => None,
            };
            if let (Some(scope), Some(h)) = (scope, hover) {
                let cell = CellAt { table: h.first, row: h.row, col: h.col };
                return MenuAt { table: self.table_facts(cell).map(|f| (cell, scope, f)), ..Default::default() };
            }
        }
        // On a link nothing moves: the keyboard taken here would wake the
        // marks on the caret's line.
        let link = self.link_at(x, y);
        if link.is_some() {
            return MenuAt { link, ..Default::default() };
        }
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let place = self.pos_at(x, y, &self.cache.borrow());
        let issue = place.and_then(|p| self.issue_at(self.doc.offset_of(p)));
        if issue.is_some() {
            if let Some(p) = place {
                self.sel = Sel::caret(p);
                self.kick_blink(cx);
            }
        } else {
            self.select_for_menu(x, y, cx);
        }
        let table = place.and_then(|p| self.cell_of(p)).and_then(|c| Some((c, Scope::Cell, self.table_facts(c)?)));
        let blank = !self.locked() && self.doc.line(self.sel.head.line).trim().is_empty() && table.is_none();
        MenuAt { link: None, issue, table, blank }
    }

    /// A place, past the hidden marks it stands before.
    fn past_hidden(&self, p: Pos) -> Pos {
        let mut hidden = self.hidden_on(p.line);
        hidden.sort_by_key(|r| r.start);
        let col = hidden.iter().fold(p.col, |col, r| if r.start <= col && col < r.end { r.end } else { col });
        self.doc.clamp(Pos { line: p.line, col })
    }

    /// What a press selects at `p`: the place, the word, the line.
    fn grain_range(&self, p: Pos, grain: Grain) -> (Pos, Pos) {
        // A press on a word's first letter maps to the end of what stands
        // before it, which may be a hidden mark. The word is what shows.
        let p = if grain == Grain::Word { self.past_hidden(p) } else { p };
        match grain {
            // Not inside a list's marker, which shows nothing to stand in.
            Grain::Char => {
                let p = self.past_marker(p);
                (p, p)
            }
            Grain::Word => {
                let w = self.doc.word_at(p);
                if w.start == w.end {
                    (p, self.doc.next_grapheme(p))
                } else {
                    (Pos::new(p.line, w.start), Pos::new(p.line, w.end))
                }
            }
            Grain::Line => match self.cell_of(p) {
                Some(cell) => self.cell_span(cell),
                None => (Pos::new(p.line, 0), if p.line + 1 < self.doc.line_count() { Pos::new(p.line + 1, 0) } else { self.doc.end() }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{carry_marks, changed, find_all, Mark};

    #[test]
    fn a_phrase_is_found_whatever_its_case() {
        assert_eq!(find_all("One one ONE", "one"), vec![0..3, 4..7, 8..11]);
        assert_eq!(find_all("费城 and 费城", "费城"), vec![0..6, 11..17]);
        assert!(find_all("abc", "").is_empty());
        assert!(find_all("", "a").is_empty());
    }

    #[test]
    fn a_change_is_told_on_characters_starts() {
        assert_eq!(changed("one two", "one new two"), (4, 4, 8));
        assert_eq!(changed("abc", "abc"), (3, 3, 3));
        // 费 and 贺 share their first two bytes: the change is the whole character.
        assert_eq!(changed("a费b", "a贺b"), (1, 4, 4));
        assert_eq!(changed("", "x"), (0, 0, 1));
    }

    #[test]
    fn a_style_moves_with_its_words() {
        let style = Mark::default();
        let marks = vec![(0..3, style), (4..7, style), (8..12, style)];
        // "one two /cmd" with "new " typed before "two".
        let carried = carry_marks("one two /cmd", "one new two /cmd", &marks);
        assert_eq!(carried.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(), vec![0..3, 8..11, 12..16]);
        // A change inside a mark takes it.
        let carried = carry_marks("one two /cmd", "one tXwo /cmd", &marks);
        assert_eq!(carried.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(), vec![0..3, 9..13]);
    }
}
