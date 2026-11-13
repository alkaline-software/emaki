//! A text laid out: what each line of it is on the page. `Reader` holds
//! what a layout goes by and nothing that can change under it. The editor
//! keeps the result (`Cache`) and paints it.
//!
//! All the vertical measures come from `Rhythm`, and from `rhythm.rs`
//! behind it. A row's height goes by its kind and how its words wrap,
//! not by what its neighbors hold, so a word typed on a blank line moves
//! nothing, and a line stands at the same height in both modes. A blank
//! line is the one exception, by the blank lines beside it (`blank_room`).
//!
//! From Onyx's `layout.rs`. A picture, a file in a frame, a card and a
//! block of HTML as it reads are not laid out here: each shows as the
//! text it is written as.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use emaki_core::check::{Issue, Kind};
use gpui::*;

use super::editor::{Look, Mark, Mode};
use super::html::Rgb;
use super::lines::{Doc, ListKind, Pos};
use super::markdown::{self, Align, Analysis, LineKind, Shown, SpanKind, Table};
use super::rhythm;

/// Each level of a list, and each level of a quote, stands this far in.
pub(crate) const LIST_INDENT: f32 = 26.;
pub(crate) const QUOTE_INDENT: f32 = 18.;
/// How far code stands in from its plate's edge.
pub(crate) const PLATE_X: f32 = 12.;
pub(crate) const CELL_X: f32 = 10.;
/// A column is never narrower.
const LEAST_COLUMN: f32 = 48.;

/// Every vertical measure of a text at its sizes, in whole pixels, so
/// the words' baselines stand on the pixel grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rhythm {
    pub line: f32,
    pub code_line: f32,
    pub cell_px: f32,
    pub cell_line: f32,
    pub cell_y: f32,
}

impl Rhythm {
    pub fn of(look: &Look) -> Rhythm {
        let (cell, line) = look.cells.unwrap_or((rhythm::CELL, rhythm::CELL_LINE));
        let cell_px = look.text_px * cell;
        let cell_line = (cell_px * line).round();
        Rhythm { line: look.line_px.round().max(1.), code_line: (look.mono_px * rhythm::CODE_LINE).round(), cell_px, cell_line, cell_y: (cell_line * 0.3).round() }
    }

    /// The caret's height in words of a size, on a line of a height.
    pub fn caret(font_px: f32, line_h: f32) -> f32 {
        (font_px * rhythm::CARET).round().min(line_h)
    }

    /// A heading's size, its line and the room over it.
    fn heading(&self, look: &Look, level: u8) -> (f32, f32, f32) {
        let h = look.headings.as_ref().map_or(rhythm::HEADINGS[(level as usize).clamp(1, 6) - 1], |own| own[(level as usize).clamp(1, 6) - 1]);
        let size = look.text_px * h.scale;
        (size, (size * h.line).round(), (self.line * h.above).round())
    }
}

/// Where a link leads.
#[derive(Clone)]
pub(crate) enum Target {
    Link(String),
}

/// Words from one `<br>` to the next, laid out in a box.
pub(crate) struct Para {
    /// The bytes of the line it holds.
    pub from: usize,
    pub to: usize,
    pub shown: Shown,
    pub line: WrappedLine,
    /// Where it begins, from the flow's top.
    pub top: f32,
    /// Where each wrapped row begins, past the box's left, when the box
    /// is not set from the left; empty when it is.
    pub rows_x: Vec<f32>,
    /// Which words are links, by their shown range, and where each leads.
    pub links: Vec<(Range<usize>, Target)>,
    /// Shown ranges with a plate behind them (code), or a wash (a highlight).
    pub chips: Vec<(Range<usize>, Hsla)>,
}

impl Para {
    fn x_of(&self, row: usize) -> f32 {
        self.rows_x.get(row).copied().unwrap_or(0.)
    }

    fn rows(&self) -> usize {
        self.line.wrap_boundaries().len() + 1
    }
}

/// The words of a line, or of a cell: one or more paragraphs in a box.
pub(crate) struct Flow {
    pub paras: Vec<Para>,
    pub line_h: f32,
    /// How far a plate reaches past its words at each end.
    pub pad: f32,
    /// The caret's height here, standing in the middle of a line.
    pub caret_h: f32,
    pub height: f32,
    pub width: f32,
    pub align: TextAlign,
}

impl Flow {
    fn para_of(&self, col: usize) -> Option<&Para> {
        self.paras.iter().find(|p| col <= p.to).or(self.paras.last())
    }

    /// Where a byte of the line stands, from the box's top left.
    pub fn caret(&self, col: usize) -> (f32, f32) {
        let Some(p) = self.para_of(col) else { return (0., 0.) };
        let lh = px(self.line_h);
        let at = p.line.position_for_index(p.shown.to_shown(col), lh).unwrap_or_default();
        let row = (f32::from(at.y) / self.line_h) as usize;
        (f32::from(at.x) + p.x_of(row), p.top + f32::from(at.y))
    }

    fn para_at(&self, y: f32) -> Option<&Para> {
        self.paras.iter().rev().find(|p| p.top <= y).or(self.paras.first())
    }

    /// The byte of the line nearest a point of the box.
    pub fn col_at(&self, x: f32, y: f32) -> usize {
        let Some(p) = self.para_at(y) else { return 0 };
        let local_y = (y - p.top).max(0.);
        let row = ((local_y / self.line_h) as usize).min(p.rows() - 1);
        let local = point(px(x - p.x_of(row)), px(local_y));
        let ix = match p.line.closest_index_for_position(local, px(self.line_h)) {
            Ok(i) | Err(i) => i,
        };
        p.shown.to_source(ix.min(p.shown.text.len()), p.to).clamp(p.from, p.to)
    }

    /// The link under a point of the box.
    pub fn link_at(&self, x: f32, y: f32) -> Option<Target> {
        let p = self.para_at(y)?;
        let local_y = (y - p.top).max(0.);
        let row = ((local_y / self.line_h) as usize).min(p.rows() - 1);
        let ix = p.line.index_for_position(point(px(x - p.x_of(row)), px(local_y)), px(self.line_h)).ok()?;
        p.links.iter().find(|(r, _)| r.contains(&ix)).map(|(_, t)| t.clone())
    }

    /// The boxes a run of the line's bytes takes, one a wrapped row.
    pub fn boxes(&self, from: usize, to: usize, origin: Point<Pixels>) -> Vec<Bounds<Pixels>> {
        let mut out = Vec::new();
        for p in &self.paras {
            let (a, b) = (from.max(p.from), to.min(p.to));
            if a > b {
                continue;
            }
            self.para_boxes(p, p.shown.to_shown(a)..p.shown.to_shown(b), origin, &mut out);
        }
        out
    }

    fn para_boxes(&self, p: &Para, range: Range<usize>, origin: Point<Pixels>, out: &mut Vec<Bounds<Pixels>>) {
        let top = origin.y + px(p.top);
        for mut b in text_boxes(&p.line, range, self.line_h, point(origin.x, top)) {
            let row = (f32::from(b.origin.y - top) / self.line_h).round() as usize;
            b.origin.x += px(p.x_of(row));
            out.push(b);
        }
    }

    /// Each paragraph's chips, as boxes.
    pub fn chips(&self, origin: Point<Pixels>) -> Vec<(Bounds<Pixels>, Hsla)> {
        let mut out = Vec::new();
        for p in &self.paras {
            for (range, color) in &p.chips {
                let mut boxes = Vec::new();
                self.para_boxes(p, range.clone(), origin, &mut boxes);
                out.extend(boxes.into_iter().map(|b| (b, *color)));
            }
        }
        out
    }

    pub fn paint(&self, origin: Point<Pixels>, window: &mut Window, cx: &mut App) {
        for p in &self.paras {
            let at = point(origin.x, origin.y + px(p.top));
            let bounds = (self.align != TextAlign::Left).then(|| Bounds::new(at, size(px(self.width), px(self.height))));
            let _ = p.line.paint(at, px(self.line_h), self.align, bounds, window, cx);
        }
    }
}

/// The boxes a range of a wrapped line's text takes, one a wrapped row.
pub(crate) fn text_boxes(line: &WrappedLine, range: Range<usize>, line_h: f32, origin: Point<Pixels>) -> Vec<Bounds<Pixels>> {
    let mut out = Vec::new();
    if range.start > range.end {
        return out;
    }
    let lh = px(line_h);
    // The rows' starts, by the glyph each wrap begins at.
    let mut starts: Vec<usize> = vec![0];
    for wb in line.wrap_boundaries() {
        let Some(glyph) = line.runs().get(wb.run_ix).and_then(|run| run.glyphs.get(wb.glyph_ix)) else { continue };
        starts.push(glyph.index);
    }
    let len = line.len();
    for (ri, &row_start) in starts.iter().enumerate() {
        let row_end = starts.get(ri + 1).copied().unwrap_or(len);
        let a = range.start.max(row_start);
        let b = range.end.min(row_end);
        if a > b || (a == b && !(range.start == range.end && a == row_start)) {
            continue;
        }
        // A row's first byte is also the row before's end, and gpui answers
        // for that one.
        let xa = if a == row_start { px(0.) } else { line.position_for_index(a, lh).map(|p| p.x).unwrap_or_default() };
        let xb = if b == row_end && ri + 1 < starts.len() {
            // To the row's end: the x of its last glyph plus its width.
            line.position_for_index(b, lh).map(|p| if (p.y / lh) as usize > ri { px(f32::from(line.width())) } else { p.x }).unwrap_or(line.width())
        } else {
            line.position_for_index(b, lh).map(|p| p.x).unwrap_or(line.width())
        };
        let w = (xb - xa).max(px(0.));
        out.push(Bounds::new(point(origin.x + xa, origin.y + lh * ri as f32), size(w, lh)));
    }
    out
}

/// A line laid out at a width: its words and the widgets beside them.
pub(crate) struct TextRow {
    pub flow: Flow,
    /// Where the words begin, from the element's left.
    pub inset: f32,
    pub top_pad: f32,
    pub widgets: Vec<Widget>,
}

pub(crate) enum Widget {
    /// A bullet's disc, centered here.
    Bullet { x: f32, y: f32, r: f32 },
    /// A number before its item.
    Number { line: ShapedLine, x: f32, y: f32 },
    /// A task's box.
    Task { bounds: Bounds<Pixels>, done: bool },
    /// An indent guide: a hairline at `x` down the whole row.
    Guide { x: f32 },
    /// A quote's bar at `x`.
    Bar { x: f32 },
}

/// How a table's columns stand: each column's width and left edge, from
/// the table's left, and how its words stand in it.
pub(crate) struct Grid {
    pub cols: Vec<f32>,
    pub xs: Vec<f32>,
    pub width: f32,
}

/// One row of a table on a line of its own.
pub(crate) struct TableRow {
    pub grid: Rc<Grid>,
    /// Whether this is the table's head.
    pub head: bool,
    /// Each cell's words, and where they stand in the line.
    pub cells: Vec<Flow>,
    pub spots: Vec<Range<usize>>,
    pub height: f32,
    pub inset: f32,
    pub cell_y: f32,
}

impl TableRow {
    /// The column a place in the line stands in.
    pub fn column_of(&self, col: usize) -> usize {
        self.spots.iter().position(|s| col <= s.end).unwrap_or(self.spots.len().saturating_sub(1))
    }

    /// The column under `x`, from the table's left.
    pub fn column_at(&self, x: f32) -> usize {
        self.grid.xs.iter().rposition(|at| *at <= x).unwrap_or(0).min(self.cells.len().saturating_sub(1))
    }
}

pub(crate) enum Row {
    Text(TextRow),
    /// Room with nothing to draw, a line's or a fence's, which holds its
    /// height whether what stands there shows or not. On a hidden opening
    /// fence, the language's mark: its name shaped, its icon and color.
    /// `blank`: a blank line, which a selection across it marks.
    Pad { lang: Option<(ShapedLine, &'static str, Hsla)>, h: f32, blank: bool },
    /// A line drawn as part of another, of no height.
    Hidden,
    Rule { inset: f32, line_h: f32 },
    Table(TableRow),
}

impl Row {
    pub fn height(&self) -> f32 {
        match self {
            Row::Text(r) => r.top_pad + r.flow.height,
            Row::Pad { h, .. } => *h,
            Row::Hidden => 0.,
            Row::Rule { line_h, .. } => *line_h,
            Row::Table(t) => t.height,
        }
    }
}

pub(crate) struct Entry {
    pub key: u64,
    pub height: f32,
    pub row: Rc<Row>,
}

#[derive(Default)]
pub(crate) struct Cache {
    pub width: f32,
    pub look_key: u64,
    pub entries: Vec<Entry>,
    pub tops: Vec<f32>,
    pub total: f32,
    /// The key of what the last layout read (`Reader::inputs_key`).
    pub read: Option<u64>,
    /// Each code block's colors, by its first line: the key of the text
    /// they were read from, and a list a line of (bytes, ink).
    pub code_colors: HashMap<usize, (u64, Vec<Vec<(Range<usize>, Hsla)>>)>,
    /// A highlighter a language, kept: making one reads its grammar.
    pub highlighters: HashMap<String, gpui_component::highlighter::SyntaxHighlighter>,
    /// Each table's grid, by its first line, with the key it was made for.
    pub grids: HashMap<usize, (u64, Rc<Grid>)>,
}

/// The narrow no-break space set around code among words, as the reading
/// view sets it: gpui's line wrapper keeps it with its word.
const CODE_PAD: char = '\u{202F}';
/// A hidden backtick's room: the plate's padding and its margin.
const PADS: &str = "\u{202F}\u{202F}";
/// A space before marked code, and one after it, each with the margin.
const SPACE_PAD: &str = " \u{202F}";
const PAD_SPACE: &str = "\u{202F} ";

/// A style a byte of the source.
#[derive(Clone, Copy, PartialEq)]
struct St {
    weight: FontWeight,
    italic: bool,
    strike: bool,
    mono: bool,
    /// Code among words, set in the look's smaller cut of the mono face.
    inline: bool,
    color: Hsla,
    chip: Option<Hsla>,
    underline: Option<Hsla>,
    wavy: Option<Hsla>,
    /// Up (above 0) or down, by the face's own figures.
    raise: i8,
}

fn ink_of(c: Rgb) -> Hsla {
    Rgba { r: c.0 as f32 / 255., g: c.1 as f32 / 255., b: c.2 as f32 / 255., a: 1. }.into()
}

/// What a layout goes by, and nothing that can change under it.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    pub doc: &'a Doc,
    pub analysis: &'a Analysis,
    pub look: &'a Look,
    pub rhythm: Rhythm,
    pub mode: Mode,
    /// The selection, while the marks around it show.
    pub sel: Option<(Pos, Pos)>,
    /// What the check marked, in order, and the offset of a word still
    /// being typed, which waits for its mark.
    pub issues: &'a [Issue],
    pub quiet: Option<usize>,
    /// The host's own styles, by bytes of the whole text, drawn over the
    /// editor's.
    pub marks: &'a [(Range<usize>, Mark)],
    /// A table's columns as a drag has them: its first line, and each width.
    pub hold: Option<&'a (usize, Vec<f32>)>,
    /// The lines the view around draws, shut: first, last, and the
    /// height of what is drawn (Emaki, `editor::Piece`).
    pub pieces: &'a [(usize, usize, f32)],
    /// The editor's count of its readings of the text (`Editor::version`):
    /// each new text, or a new way of reading it, has a new one.
    pub version: u64,
}

impl<'a> Reader<'a> {
    /// Whether the selection touches a line.
    pub fn on_line(&self, line: usize) -> bool {
        self.sel.is_some_and(|(a, b)| a.line <= line && line <= b.line)
    }

    fn touches(&self, r: &Range<usize>, inclusive: bool) -> bool {
        let Some((a, b)) = self.sel else { return false };
        // By the reading's line starts: summing the lines before each end,
        // at every span of every line, cost a long file milliseconds a
        // layout.
        let at = |p: Pos| self.analysis.line_starts.get(p.line).map_or_else(|| self.doc.offset_of(p), |s| s + p.col);
        let (ga, gb) = (at(a), at(b));
        if inclusive { ga <= r.end && gb >= r.start } else { ga < r.end && gb > r.start }
    }

    /// Whether a blank line is the room between two blocks, the short
    /// height the view around sets them apart by. Of a run of blank lines
    /// one is, and the others are lines of their own, as tall as a line of
    /// words: a line made with ↩ is one at once, and a word typed on it
    /// moves nothing. The room is the run's last, or its first while the
    /// caret stands on the last, so the caret's line is a whole one and
    /// the run as tall as before.
    pub fn blank_room(&self, line: usize) -> bool {
        // Asked of every blank line: only the run's first walks the run.
        let blank = |l: usize| self.analysis.lines.get(l).is_some_and(|i| matches!(i.kind, LineKind::Blank));
        let caret = self.sel.filter(|(a, b)| a == b).map(|(a, _)| a.line);
        let above = line > 0 && blank(line - 1);
        if !blank(line + 1) {
            return !(above && caret == Some(line));
        }
        if above {
            return false;
        }
        let mut last = line + 1;
        while blank(last + 1) {
            last += 1;
        }
        caret == Some(last)
    }

    /// The marks hidden on a line, in the line's bytes, by the mode and
    /// where the caret is.
    pub fn hidden_on(&self, line: usize) -> Vec<Range<usize>> {
        let info = &self.analysis.lines[line];
        let in_table = matches!(info.kind, LineKind::TableFirst(_) | LineKind::TableRow(_));
        // A cell is a line of its own: the row's bars never show.
        let on_line = self.on_line(line) && !in_table;
        let start = self.analysis.line_starts[line];
        let mut hidden = Vec::new();
        if !on_line {
            hidden.extend(info.marks.iter().cloned());
        }
        if let Some(m) = &info.list {
            hidden.push(0..m.marker.end);
        } else if info.quote_depth > 0 && !on_line {
            // The quote's marks are in `marks`; the room before them goes too.
            if let Some(first) = info.marks.first() {
                hidden.push(0..first.start);
            }
        }
        for &ix in &self.analysis.line_spans[line] {
            let s = &self.analysis.spans[ix];
            // A picture is not drawn: it shows as it is written.
            if matches!(s.kind, SpanKind::Image(_)) {
                continue;
            }
            let inclusive = !matches!(s.kind, SpanKind::Link(_) | SpanKind::Break);
            if self.touches(&s.range, inclusive) {
                continue;
            }
            for r in [&s.open, &s.close] {
                let (a, b) = (r.start.max(start), r.end.min(start + self.doc.line(line).len()));
                if a < b {
                    hidden.push(a - start..b - start);
                }
            }
        }
        hidden
    }

    fn block_revealed(&self, first: usize, last: usize) -> bool {
        self.sel.is_some_and(|(a, b)| a.line <= last && b.line >= first)
    }

    /// Whether a line shows as it is written: a block's lines go together.
    fn revealed(&self, line: usize) -> bool {
        match &self.analysis.lines[line].kind {
            LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::Code => self.analysis.code_block_at(line).is_none_or(|b| self.block_revealed(b.first, b.last)),
            LineKind::TableFirst(_) | LineKind::TableRow(_) => false,
            _ => self.on_line(line),
        }
    }

    fn issues_on(&self, line: usize) -> &'a [Issue] {
        let from = self.analysis.line_starts[line];
        let to = from + self.doc.line(line).len();
        let a = self.issues.partition_point(|i| i.range.end <= from);
        let b = self.issues.partition_point(|i| i.range.start < to.max(from + 1));
        self.issues.get(a..b.max(a)).unwrap_or(&[])
    }

    /// The host's styles that touch a line.
    fn marks_on(&self, line: usize) -> impl Iterator<Item = &'a (Range<usize>, Mark)> {
        let from = self.analysis.line_starts[line];
        let to = from + self.doc.line(line).len();
        self.marks.iter().filter(move |(r, _)| r.start < to && r.end > from)
    }

    pub fn look_key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        let look = self.look;
        look.text_family.hash(&mut h);
        look.mono_family.hash(&mut h);
        look.inline_family.hash(&mut h);
        look.ui_family.hash(&mut h);
        look.text_fallbacks.hash(&mut h);
        look.mono_fallbacks.hash(&mut h);
        look.text_px.to_bits().hash(&mut h);
        look.mono_px.to_bits().hash(&mut h);
        look.line_px.to_bits().hash(&mut h);
        look.blank.map(|(a, b)| (a.to_bits(), b.to_bits())).hash(&mut h);
        look.strong.0.to_bits().hash(&mut h);
        look.colors.hash(&mut h);
        (std::sync::Arc::as_ptr(&look.syntax) as usize).hash(&mut h);
        (self.mode as u8).hash(&mut h);
        h.finish()
    }

    /// What a layout reads, as one key: the text's reading, the
    /// selection, the look and the width, and what the host lays over the
    /// text (marks, issues, pieces, a table's columns held in a drag).
    fn inputs_key(&self, look_key: u64, width: f32) -> u64 {
        let mut h = DefaultHasher::new();
        (self.version, self.doc.revision, look_key, width.to_bits(), self.sel, self.quiet).hash(&mut h);
        for i in self.issues {
            (&i.range, i.kind == Kind::Spelling).hash(&mut h);
        }
        self.marks.hash(&mut h);
        for (first, last, tall) in self.pieces {
            (first, last, tall.to_bits()).hash(&mut h);
        }
        self.hold.map(|(first, cols)| (first, cols.iter().map(|c| c.to_bits()).collect::<Vec<_>>())).hash(&mut h);
        h.finish()
    }

    /// The whole text laid out at `width`, line by line, each only when
    /// its key changed. The tops and the total follow.
    pub fn lay_out_note(&self, cache: &mut Cache, width: f32, window: &mut Window) {
        let look_key = self.look_key();
        let n = self.doc.line_count();
        // Nothing a layout reads has changed: the layout stands. It is
        // asked for at every frame the editor is drawn, and more than once
        // in one.
        let read = self.inputs_key(look_key, width);
        if cache.read == Some(read) {
            return;
        }

        cache.read = Some(read);
        if cache.width != width || cache.look_key != look_key {
            cache.entries.clear();
            cache.width = width;
            cache.look_key = look_key;
        }
        // Lines come or go: a line's row is found by its key where the
        // line now stands elsewhere, so what is under an edit is not laid
        // out again.
        let moved: HashMap<u64, (f32, Rc<Row>)> = if cache.entries.len() != n { cache.entries.iter().map(|e| (e.key, (e.height, e.row.clone()))).collect() } else { HashMap::new() };
        cache.entries.truncate(n);
        self.color_code(cache);
        self.make_grids(&mut cache.grids, width, window);
        let mut tops = Vec::with_capacity(n);
        let mut y = 0.;
        for line in 0..n {
            let hidden = self.hidden_on(line);
            let colors: Vec<(Range<usize>, Hsla)> = code_colors_of(self.analysis, cache, line).to_vec();
            let revealed = self.revealed(line);
            let text = self.doc.line(line);
            let info = &self.analysis.lines[line];
            let piece = self.pieces.iter().find(|(f, l, _)| *f <= line && line <= *l).map(|(f, _, h)| (*f == line, *h));
            let mut h = DefaultHasher::new();
            piece.map(|(first, tall)| (first, tall.to_bits())).hash(&mut h);
            text.hash(&mut h);
            for (r, c) in &colors {
                r.hash(&mut h);
                c.hash(&mut h);
            }
            hidden.hash(&mut h);
            info.kind.hash_key(&mut h);
            info.task_done.hash(&mut h);
            info.list_depth.hash(&mut h);
            info.quote_depth.hash(&mut h);
            revealed.hash(&mut h);
            (line == 0).hash(&mut h);
            (line > 0 && matches!(self.analysis.lines[line - 1].kind, LineKind::Heading(_))).hash(&mut h);
            (matches!(info.kind, LineKind::Blank) && self.blank_room(line)).hash(&mut h);
            // Ranges by the line's own bytes, as the row is made from them:
            // by the whole text's, a letter typed changed the key of every
            // line under it.
            let start = self.analysis.line_starts[line];
            let local = |r: &Range<usize>| r.start.saturating_sub(start).min(text.len())..r.end.saturating_sub(start).min(text.len());
            for i in self.issues_on(line) {
                local(&i.range).hash(&mut h);
                (i.kind == Kind::Spelling).hash(&mut h);
                (self.quiet == Some(i.range.end)).hash(&mut h);
            }
            for (r, style) in self.marks_on(line) {
                local(r).hash(&mut h);
                style.hash(&mut h);
            }
            for &ix in &self.analysis.line_spans[line] {
                let s = &self.analysis.spans[ix];
                for r in [&s.range, &s.open, &s.close] {
                    local(r).hash(&mut h);
                }
            }
            if let LineKind::TableFirst(t) | LineKind::TableRow(t) = info.kind {
                let first = self.analysis.tables[t].first;
                cache.grids.get(&first).map(|(k, _)| *k).hash(&mut h);
            }
            let key = h.finish();
            let fresh = cache.entries.get(line).is_none_or(|e| e.key != key);
            let found = if fresh { moved.get(&key) } else { None };
            if let Some((height, row)) = found {
                let entry = Entry { key, height: *height, row: row.clone() };
                if line < cache.entries.len() {
                    cache.entries[line] = entry;
                } else {
                    cache.entries.push(entry);
                }
            } else if fresh {
                let row = match piece {
                    // A piece's first line holds its height; the rest none.
                    Some((true, tall)) => Row::Pad { lang: None, h: tall, blank: false },
                    Some((false, _)) => Row::Hidden,
                    None => self.lay_out(line, &hidden, &colors, revealed, width, &cache.grids, window),
                };
                let height = row.height();
                let entry = Entry { key, height, row: Rc::new(row) };
                if line < cache.entries.len() {
                    cache.entries[line] = entry;
                } else {
                    cache.entries.push(entry);
                }
            }
            tops.push(y);
            y += cache.entries[line].height;
        }
        cache.tops = tops;
        cache.total = y;
    }

    fn font(&self, family: &SharedString, weight: FontWeight, italic: bool) -> Font {
        let fallbacks = if *family == self.look.mono_family || self.look.inline_family.as_ref() == Some(family) { self.look.mono_fallbacks.clone() } else { self.look.text_fallbacks.clone() };
        Font { family: family.clone(), features: FontFeatures::default(), fallbacks, weight, style: if italic { FontStyle::Italic } else { FontStyle::Normal } }
    }

    /// Each code block's colors read again where its text changed: the
    /// toolkit's tree-sitter highlighter for the fence's language, with
    /// the inks the look's `syntax` names. A block with no language, or
    /// one the toolkit has no grammar for, keeps the plain ink, and so
    /// does one whose reading fails: the highlighter is a third party's,
    /// and runs where a panic is caught.
    fn color_code(&self, cache: &mut Cache) {
        use gpui_component::highlighter::SyntaxHighlighter;
        // Each block's colors carried to where it stands now, found by its
        // first line or, where lines came or went above it, by its text's
        // key; read again only where its text changed.
        let mut was = std::mem::take(&mut cache.code_colors);
        for b in &self.analysis.code_blocks {
            if b.lang.is_empty() || b.markup.is_some() {
                continue;
            }
            let last = if b.closed { b.last.saturating_sub(1) } else { b.last };
            let lines: Vec<&str> = (b.first + 1..=last).filter(|&l| l < self.doc.line_count()).map(|l| self.doc.line(l)).collect();
            let code = lines.join("\n");
            let mut h = DefaultHasher::new();
            b.lang.hash(&mut h);
            code.hash(&mut h);
            (std::sync::Arc::as_ptr(&self.look.syntax) as usize).hash(&mut h);
            let key = h.finish();
            let kept = match was.get(&b.first) {
                Some((k, _)) if *k == key => was.remove(&b.first),
                _ => was.iter().find(|(_, (k, _))| *k == key).map(|(f, _)| *f).and_then(|f| was.remove(&f)),
            };
            if let Some(kept) = kept {
                cache.code_colors.insert(b.first, kept);
                continue;
            }
            let syntax = self.look.syntax.clone();
            let highlighters = &mut cache.highlighters;
            let read = catch_unwind(AssertUnwindSafe(|| {
                let hl = highlighters.entry(b.lang.clone()).or_insert_with(|| SyntaxHighlighter::new(&b.lang));
                let rope = ropey::Rope::from_str(&code);
                hl.update(None, &rope, None);
                hl.styles(&(0..code.len()), syntax.as_ref())
            }));
            let Ok(styled) = read else {
                // What a panic left of it is not read again.
                cache.highlighters.remove(&b.lang);
                cache.code_colors.insert(b.first, (key, vec![Vec::new(); lines.len()]));
                continue;
            };
            // The block's bytes to each line's own.
            let mut per_line: Vec<Vec<(Range<usize>, Hsla)>> = vec![Vec::new(); lines.len()];
            let mut starts = Vec::with_capacity(lines.len());
            let mut at = 0;
            for l in &lines {
                starts.push(at);
                at += l.len() + 1;
            }
            for (range, style) in styled {
                let Some(color) = style.color else { continue };
                for (ix, l) in lines.iter().enumerate() {
                    let (ls, le) = (starts[ix], starts[ix] + l.len());
                    let (a, bb) = (range.start.max(ls), range.end.min(le));
                    if a < bb {
                        per_line[ix].push((a - ls..bb - ls, color));
                    }
                }
            }
            cache.code_colors.insert(b.first, (key, per_line));
        }
    }

    /// Each byte's style on a line, and where its links lead.
    fn styled(&self, line: usize, base_weight: FontWeight, mono_line: bool, muted_line: bool, colors: &[(Range<usize>, Hsla)]) -> (Vec<St>, Vec<(Range<usize>, Target)>) {
        let info = &self.analysis.lines[line];
        let text = self.doc.line(line);
        let inks = &self.look.colors;
        let dark = inks.dark;
        let muted = inks.muted;
        let ink = inks.ink;
        let link = inks.link;
        let base = St { weight: base_weight, italic: false, strike: info.task_done, mono: mono_line, inline: false, color: if muted_line || info.task_done { muted } else { ink }, chip: None, underline: None, wavy: None, raise: 0 };
        let mut styles = vec![base; text.len()];
        let paint = |styles: &mut Vec<St>, r: Range<usize>, f: &dyn Fn(&mut St)| {
            for s in styles.iter_mut().take(r.end.min(text.len())).skip(r.start.min(text.len())) {
                f(s);
            }
        };
        // Code's own colors, from its language's grammar.
        for (r, c) in colors {
            paint(&mut styles, r.clone(), &|s| s.color = *c);
        }
        // Marks shown are quiet.
        for m in &info.marks {
            paint(&mut styles, m.clone(), &|s| {
                s.color = muted;
                s.weight = FontWeight::NORMAL;
            });
        }
        if let Some(m) = &info.list {
            paint(&mut styles, 0..m.marker.end, &|s| {
                s.color = muted;
                s.strike = false;
            });
        }
        let start = self.analysis.line_starts[line];
        let local = |r: &Range<usize>| r.start.saturating_sub(start).min(text.len())..r.end.saturating_sub(start).min(text.len());
        let chip_bg = inks.accent.opacity(if dark { 0.16 } else { 0.10 });
        let highlight_bg = inks.highlight.opacity(if dark { 0.35 } else { 0.4 });
        let mut links: Vec<(Range<usize>, Target)> = Vec::new();
        let mut spans: Vec<&markdown::Span> = self.analysis.line_spans[line].iter().map(|&ix| &self.analysis.spans[ix]).collect();
        // The outer first, so an inner span paints over it.
        spans.sort_by_key(|s| std::cmp::Reverse(s.range.len()));
        for s in spans {
            let inner = local(&s.inner());
            let (open, close) = (local(&s.open), local(&s.close));
            let marks_muted = |st: &mut St| {
                st.color = muted;
                st.weight = base_weight;
                st.italic = false;
            };
            match &s.kind {
                SpanKind::Strong => paint(&mut styles, inner.clone(), &|st| st.weight = self.look.strong),
                SpanKind::Em => paint(&mut styles, inner.clone(), &|st| st.italic = true),
                SpanKind::Strike => paint(&mut styles, inner.clone(), &|st| {
                    st.strike = true;
                    st.color = muted;
                }),
                SpanKind::Code => paint(&mut styles, inner.clone(), &|st| {
                    st.mono = true;
                    st.color = link;
                    st.chip = Some(chip_bg);
                }),
                SpanKind::Math => paint(&mut styles, inner.clone(), &|st| {
                    st.mono = true;
                    st.color = link;
                }),
                SpanKind::Highlight => paint(&mut styles, inner.clone(), &|st| st.chip = Some(highlight_bg)),
                SpanKind::Link(url) => {
                    paint(&mut styles, inner.clone(), &|st| {
                        st.color = link;
                        st.underline = Some(link.opacity(0.5));
                    });
                    links.push((inner.clone(), Target::Link(url.clone())));
                }
                // A picture's words, as they stand among its marks: it
                // leads nowhere and nothing of it hides.
                SpanKind::Image(_) => paint(&mut styles, inner.clone(), &|st| {
                    st.color = link;
                    st.italic = true;
                }),
                SpanKind::Footnote => paint(&mut styles, local(&s.range), &|st| st.color = link),
                SpanKind::Underline => paint(&mut styles, inner.clone(), &|st| st.underline = Some(st.color)),
                SpanKind::Sub => paint(&mut styles, inner.clone(), &|st| st.raise = -1),
                SpanKind::Sup => paint(&mut styles, inner.clone(), &|st| st.raise = 1),
                SpanKind::Small => paint(&mut styles, inner.clone(), &|st| st.color = muted),
                SpanKind::Color(color, wash) => paint(&mut styles, inner.clone(), &|st| {
                    if let Some(c) = color {
                        st.color = ink_of(*c);
                    }
                    if let Some(w) = wash {
                        st.chip = Some(ink_of(*w).opacity(0.5));
                    }
                }),
                SpanKind::Source => paint(&mut styles, local(&s.range), &|st| {
                    st.mono = true;
                    st.color = muted;
                }),
                SpanKind::Break | SpanKind::Quiet => paint(&mut styles, local(&s.range), &|st| {
                    st.mono = true;
                    st.color = muted;
                }),
            }
            paint(&mut styles, open, &marks_muted);
            paint(&mut styles, close, &marks_muted);
        }
        // The host's styles, over the editor's own.
        for (r, mark) in self.marks_on(line) {
            let (style, mark) = (mark.style, *mark);
            paint(&mut styles, local(r), &|st| {
                if mark.mono || mark.code {
                    st.mono = true;
                }
                if mark.code {
                    st.color = link;
                    st.chip = Some(chip_bg);
                }
                if let Some(c) = style.color {
                    st.color = c;
                }
                if let Some(w) = style.font_weight {
                    st.weight = w;
                }
                if let Some(s) = style.font_style {
                    st.italic = s != FontStyle::Normal;
                }
                if let Some(bg) = style.background_color {
                    st.chip = Some(bg);
                }
                if let Some(u) = style.underline {
                    st.underline = Some(u.color.unwrap_or(st.color));
                }
                if style.strikethrough.is_some() {
                    st.strike = true;
                }
                if let Some(f) = style.fade_out {
                    st.color.fade_out(f);
                }
            });
        }
        for issue in self.issues_on(line) {
            if issue.kind == Kind::Spelling && self.quiet == Some(issue.range.end) {
                continue;
            }
            let color = if issue.kind == Kind::Spelling { inks.spelling } else { inks.grammar };
            paint(&mut styles, local(&issue.range), &|st| st.wavy = Some(color));
        }
        // Code among words, in a line that is not code, is in the cut.
        if !mono_line {
            for st in styles.iter_mut().filter(|st| st.mono) {
                st.inline = true;
            }
        }
        (styles, links)
    }

    /// Where a run of a line breaks at each `<br>` that does not show.
    fn para_ranges(&self, line: usize, range: Range<usize>, hidden: &[Range<usize>]) -> Vec<Range<usize>> {
        let start = self.analysis.line_starts[line];
        let mut out = Vec::new();
        let mut at = range.start;
        for &ix in &self.analysis.line_spans[line] {
            let s = &self.analysis.spans[ix];
            if s.kind != SpanKind::Break || s.range.start < start + range.start || s.range.end > start + range.end {
                continue;
            }
            let (a, b) = (s.range.start - start, s.range.end - start);
            if hidden.iter().any(|h| h.start <= a && b <= h.end) {
                out.push(at..a);
                at = b;
            }
        }
        out.push(at..range.end);
        out
    }

    /// The words of a run of a line in a box of `wrap` wide.
    #[allow(clippy::too_many_arguments)]
    fn flow(&self, line: usize, range: Range<usize>, styles: &[St], links: &[(Range<usize>, Target)], hidden: &[Range<usize>], font_px: f32, line_h: f32, wrap: f32, align: TextAlign, ink: Hsla, window: &mut Window) -> Flow {
        let text = self.doc.line(line);
        let mut paras = Vec::new();
        let mut top = 0.;
        let pads = self.code_pads(line, hidden);
        for r in self.para_ranges(line, range, hidden) {
            let mut out: Vec<Range<usize>> = hidden.to_vec();
            out.push(0..r.start);
            out.push(r.end..text.len());
            // A hidden backtick stands as the room around code's plate,
            // and is no longer hidden.
            let pads: Vec<&(usize, &str)> = pads.iter().filter(|(at, _)| r.contains(at)).collect();
            for (at, _) in &pads {
                out = out.into_iter().flat_map(|h| if h.contains(at) { vec![h.start..*at, at + 1..h.end] } else { vec![h] }).filter(|h| h.start < h.end).collect();
            }
            let mut swaps = raised_chars(text, r.clone(), styles, &out);
            swaps.extend(pads.iter().map(|(at, with)| (*at..at + 1, with.to_string())));
            swaps.sort_by_key(|(r, _)| r.start);
            swaps.dedup_by(|b, a| b.0.start < a.0.end);
            let shown = Shown::with(text, &out, &swaps);
            let mut runs: Vec<TextRun> = Vec::new();
            let mut chips: Vec<(Range<usize>, Hsla)> = Vec::new();
            for &(d, s, len, out_len) in &shown.pieces {
                let mut at = s;
                while at < s + len {
                    let Some(&st) = styles.get(at) else { break };
                    let mut end = at + 1;
                    // A run ends where the style changes, and only at a
                    // character's start: a style from outside (a mark of
                    // the host's, a spelling mark carried over an edit)
                    // may begin or end inside one.
                    while end < s + len && (styles.get(end) == Some(&st) || !text.is_char_boundary(end)) {
                        end += 1;
                    }
                    let family = match (st.inline, &self.look.inline_family) {
                        (true, Some(cut)) => cut,
                        _ if st.mono => &self.look.mono_family,
                        _ => &self.look.text_family,
                    };
                    let underline = match (st.wavy, st.underline) {
                        (Some(c), _) => Some(UnderlineStyle { thickness: px(1.), color: Some(c), wavy: true }),
                        (None, Some(c)) => Some(UnderlineStyle { thickness: px(1.), color: Some(c), wavy: false }),
                        _ => None,
                    };
                    let swapped = len != out_len;
                    if swapped {
                        end = s + len;
                    }
                    runs.push(TextRun { len: if swapped { out_len } else { end - at }, font: self.font(family, st.weight, st.italic), color: st.color, background_color: None, underline, strikethrough: st.strike.then(|| StrikethroughStyle { thickness: px(1.), color: Some(st.color) }) });
                    if let Some(c) = st.chip {
                        let dr = if swapped { d..d + out_len } else { d + (at - s)..d + (end - s) };
                        match chips.last_mut() {
                            Some((r, col)) if r.end == dr.start && *col == c => r.end = dr.end,
                            _ => chips.push((dr, c)),
                        }
                    }
                    at = end;
                }
            }
            let shown_links: Vec<(Range<usize>, Target)> = links
                .iter()
                .filter_map(|(l, t)| {
                    let (a, b) = (shown.to_shown(l.start.max(r.start)), shown.to_shown(l.end.min(r.end)));
                    (a < b).then(|| (a..b, t.clone()))
                })
                .collect();
            let mut lines = window.text_system().shape_text(SharedString::from(shown.text.clone()), px(font_px), &runs, Some(px(wrap)), None).unwrap_or_default();
            let wrapped = lines.pop().unwrap_or_else(|| {
                let run = TextRun { len: 1, font: self.font(&self.look.text_family, FontWeight::NORMAL, false), color: ink, background_color: None, underline: None, strikethrough: None };
                window.text_system().shape_text(" ".into(), px(font_px), &[run], None, None).ok().and_then(|mut v| v.pop()).unwrap_or_default()
            });
            let mut para = Para { from: r.start, to: r.end, shown, line: wrapped, top, rows_x: Vec::new(), links: shown_links, chips };
            if align != TextAlign::Left {
                let len = para.line.len();
                para.rows_x = text_boxes(&para.line, 0..len, line_h, point(px(0.), px(0.)))
                    .iter()
                    .map(|b| match align {
                        TextAlign::Center => (wrap - f32::from(b.size.width)) / 2.,
                        _ => wrap - f32::from(b.size.width),
                    })
                    .collect();
            }
            top += para.rows() as f32 * line_h;
            paras.push(para);
        }
        // A plate reaches past its code by one narrow space, the inner of
        // the two each end has (`code_pads`).
        let font = self.font(&self.look.text_family, FontWeight::NORMAL, false);
        let pad = window.text_system().advance(window.text_system().resolve_font(&font), px(font_px), CODE_PAD).map_or(2., |s| f32::from(s.width));
        Flow { paras, line_h, caret_h: Rhythm::caret(font_px, line_h), height: top, width: wrap, align, pad }
    }

    /// Where on a line code among words has room around its plate, as
    /// the reading view gives it (`CODE_PAD`, the vendored toolkit's
    /// `style.rs`): by the line's bytes, what is drawn there in place of
    /// the one byte. A code span's backticks, while they are hidden, stand
    /// as two narrow spaces each, the plate's padding and its margin; and
    /// a space beside code the view marks (a command, a file named) has a
    /// narrow one beside it, toward the code.
    fn code_pads(&self, line: usize, hidden: &[Range<usize>]) -> Vec<(usize, &'static str)> {
        let text = self.doc.line(line);
        let start = self.analysis.line_starts[line];
        let local = |r: &Range<usize>| r.start.saturating_sub(start).min(text.len())..r.end.saturating_sub(start).min(text.len());
        let hid = |r: &Range<usize>| !r.is_empty() && hidden.iter().any(|h| h.start <= r.start && r.end <= h.end);
        let mut pads = Vec::new();
        for &ix in &self.analysis.line_spans[line] {
            let s = &self.analysis.spans[ix];
            if s.kind != SpanKind::Code {
                continue;
            }
            for mark in [local(&s.open), local(&s.close)] {
                if hid(&mark) {
                    pads.push((mark.start, PADS));
                }
            }
        }
        for (r, mark) in self.marks_on(line) {
            if !mark.code {
                continue;
            }
            let r = local(r);
            if r.start > 0 && text.as_bytes()[r.start - 1] == b' ' && !hidden.iter().any(|h| h.contains(&(r.start - 1))) {
                pads.push((r.start - 1, SPACE_PAD));
            }
            if text.as_bytes().get(r.end) == Some(&b' ') && !hidden.iter().any(|h| h.contains(&r.end)) {
                pads.push((r.end, PAD_SPACE));
            }
        }
        pads.sort();
        pads.dedup_by_key(|(at, _)| *at);
        pads
    }

    /// One line laid out: its words with the hidden marks taken out, in
    /// runs of the faces and inks its spans ask for, and its widgets.
    #[allow(clippy::too_many_arguments)]
    fn lay_out(&self, line: usize, hidden: &[Range<usize>], colors: &[(Range<usize>, Hsla)], revealed: bool, width: f32, grids: &HashMap<usize, (u64, Rc<Grid>)>, window: &mut Window) -> Row {
        let info = &self.analysis.lines[line];
        let rhythm = self.rhythm;
        let px_text = self.look.text_px;
        let muted = self.look.colors.muted;

        // What stands in place of the line, when the caret is away.
        if let LineKind::TableFirst(t) | LineKind::TableRow(t) = &info.kind {
            let table = &self.analysis.tables[*t];
            let (Some(row), Some((_, grid))) = (table.row_of(line), grids.get(&table.first)) else { return Row::Hidden };
            return self.table_row(table, row, grid.clone(), hidden, width, window);
        }
        // A blank line as the view around sets blocks apart, one height
        // whether the selection is on it or not, so a drag over it moves
        // nothing. The others of a run of them are lines of their own, as
        // tall as a line of words (`blank_room`).
        if let (LineKind::Blank, Some((heading, other))) = (&info.kind, self.look.blank) {
            if !self.blank_room(line) {
                return Row::Pad { lang: None, h: rhythm.line, blank: true };
            }
            let after_heading = line > 0 && matches!(self.analysis.lines[line - 1].kind, LineKind::Heading(_));
            return Row::Pad { lang: None, h: if after_heading { heading } else { other }, blank: true };
        }
        if !revealed {
            match &info.kind {
                LineKind::FenceOpen(lang) => {
                    // The language's mark at the plate's top right: its icon
                    // in its color and its name.
                    let lang = (!lang.is_empty()).then(|| {
                        let mark = super::langs::of(lang);
                        let font = Font { family: self.look.ui_family.clone(), features: FontFeatures::default(), fallbacks: None, weight: FontWeight::NORMAL, style: FontStyle::Normal };
                        let run = TextRun { len: mark.name.len(), font, color: muted, background_color: None, underline: None, strikethrough: None };
                        let line = window.text_system().shape_line(SharedString::from(mark.name.clone()), px(super::editor::MARK_PX), &[run], None);
                        (line, mark.icon, gpui::rgb(mark.rgb).into())
                    });
                    return Row::Pad { lang, h: rhythm.code_line, blank: false };
                }
                LineKind::FenceClose => return Row::Pad { lang: None, h: rhythm.code_line, blank: false },
                LineKind::SetextUnderline => return Row::Pad { lang: None, h: rhythm.line, blank: false },
                LineKind::Rule => return Row::Rule { inset: self.inset_of(info), line_h: rhythm.line },
                _ => {}
            }
        }
        // The size, weight and line of the words.
        let (font_px, base_weight, line_h, top_pad) = match &info.kind {
            LineKind::Heading(level) => {
                let (size, line_h, above) = rhythm.heading(self.look, *level);
                let weight = if *level == 1 { FontWeight::BOLD } else { FontWeight::SEMIBOLD };
                (size, weight, line_h, if line == 0 { 0. } else { above })
            }
            LineKind::Code | LineKind::FrontMatter | LineKind::FrontMatterEdge | LineKind::Html | LineKind::Source | LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::TableFirst(_) | LineKind::TableRow(_) => (self.look.mono_px, FontWeight::NORMAL, rhythm.code_line, 0.),
            _ => (px_text, FontWeight::NORMAL, rhythm.line, 0.),
        };
        // A block of HTML shows as it is written, as code does.
        let mono_line = matches!(info.kind, LineKind::Code | LineKind::FrontMatter | LineKind::FrontMatterEdge | LineKind::Html | LineKind::Source | LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::TableFirst(_) | LineKind::TableRow(_));
        let muted_line = matches!(info.kind, LineKind::FrontMatter | LineKind::FrontMatterEdge | LineKind::Html | LineKind::Source | LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::SetextUnderline);
        let (styles, links) = self.styled(line, base_weight, mono_line, muted_line, colors);

        let inset = self.inset_of(info);
        let on_plate = matches!(info.kind, LineKind::Code | LineKind::FenceOpen(_) | LineKind::FenceClose);
        let wrap = (width - inset - if on_plate { PLATE_X } else { 0. }).max(40.);
        let len = self.doc.line(line).len();
        let flow = self.flow(line, 0..len, &styles, &links, hidden, font_px, line_h, wrap, TextAlign::Left, self.look.colors.ink, window);
        let widgets = self.widgets(line, inset, top_pad, line_h, window);
        Row::Text(TextRow { flow, inset, top_pad, widgets })
    }

    /// The widgets beside a line's words.
    fn widgets(&self, line: usize, inset: f32, top_pad: f32, line_h: f32, window: &mut Window) -> Vec<Widget> {
        let info = &self.analysis.lines[line];
        let text = self.doc.line(line);
        let px_text = self.look.text_px;
        let muted = self.look.colors.muted;
        let mut widgets = Vec::new();
        if let Some(m) = &info.list {
            let depth = m.depth as f32;
            let x_mark = inset - LIST_INDENT * 0.5;
            match m.kind {
                ListKind::Bullet => widgets.push(Widget::Bullet { x: x_mark, y: top_pad + line_h / 2., r: (px_text * 0.17).max(2.5) }),
                ListKind::Ordered(n) => {
                    // Typst's `+` and LaTeX's `\item` are numbered by their
                    // place, as written.
                    let written = text.get(m.marker.start..m.marker.end).unwrap_or("").trim();
                    let number = if written.starts_with(|c: char| c.is_ascii_digit()) { written.to_string() } else { format!("{n}.") };
                    let run = TextRun { len: number.len(), font: self.font(&self.look.text_family, FontWeight::NORMAL, false), color: muted, background_color: None, underline: None, strikethrough: None };
                    let shaped = window.text_system().shape_line(number.into(), px(px_text), &[run], None);
                    let x = inset - 6. - f32::from(shaped.width());
                    widgets.push(Widget::Number { line: shaped, x, y: top_pad });
                }
                ListKind::Task(done) => {
                    let side = (px_text * 0.95).round();
                    let bounds = Bounds::new(point(px(x_mark - side / 2.), px(top_pad + (line_h - side) / 2.)), size(px(side), px(side)));
                    widgets.push(Widget::Task { bounds, done });
                }
            }
            for d in 0..depth as usize {
                widgets.push(Widget::Guide { x: self.base_inset(info) + LIST_INDENT * d as f32 + LIST_INDENT * 0.5 });
            }
        } else if info.list_depth > 0 && info.kind != LineKind::Blank {
            for d in 0..info.list_depth {
                widgets.push(Widget::Guide { x: self.base_inset(info) + LIST_INDENT * d as f32 + LIST_INDENT * 0.5 });
            }
        }
        for d in 0..info.quote_depth {
            widgets.push(Widget::Bar { x: QUOTE_INDENT * d as f32 + 2. });
        }
        widgets
    }

    /// Where a line's words begin: in by its quotes and its list levels.
    pub fn base_inset(&self, info: &markdown::LineInfo) -> f32 {
        QUOTE_INDENT * info.quote_depth as f32
    }

    /// Code and its fences stand in from the plate's edge.
    fn inset_of(&self, info: &markdown::LineInfo) -> f32 {
        let on_plate = matches!(info.kind, LineKind::Code | LineKind::FenceOpen(_) | LineKind::FenceClose);
        let base = self.base_inset(info);
        match &info.list {
            Some(m) => base + LIST_INDENT * (m.depth as f32 + 1.),
            None if info.list_depth > 0 => base + LIST_INDENT * info.list_depth as f32,
            None => base + if on_plate { PLATE_X } else { 0. },
        }
    }

    // -- tables -------------------------------------------------------------

    /// Each table's columns made again where its words changed, as they
    /// stand with no caret near, so a cell's marks showing never moves
    /// the grid. As far as the room goes each column is as wide as its
    /// longest line asks; past that each keeps its longest word and the
    /// room left is shared by what the rest ask, as a browser sets a
    /// table, so a word is broken only where the room gives out.
    fn make_grids(&self, grids: &mut HashMap<usize, (u64, Rc<Grid>)>, width: f32, window: &mut Window) {
        // As the code's colors: carried by first line or by key.
        let mut was = std::mem::take(grids);
        for t in &self.analysis.tables {
            let mut h = DefaultHasher::new();
            width.to_bits().hash(&mut h);
            // Where it stands in its line, which its room goes by.
            self.inset_of(&self.analysis.lines[t.first]).to_bits().hash(&mut h);
            self.look.text_family.hash(&mut h);
            self.look.text_px.to_bits().hash(&mut h);
            t.rows.hash(&mut h);
            t.shares.iter().flatten().for_each(|s| s.to_bits().hash(&mut h));
            let held = self.hold.filter(|(first, _)| *first == t.first);
            held.iter().flat_map(|(_, c)| c).for_each(|c| c.to_bits().hash(&mut h));
            let key = h.finish();
            let kept = match was.get(&t.first) {
                Some((k, _)) if *k == key => was.remove(&t.first),
                _ => was.iter().find(|(_, (k, _))| *k == key).map(|(f, _)| *f).and_then(|f| was.remove(&f)),
            };
            if let Some(kept) = kept {
                grids.insert(t.first, kept);
                continue;
            }
            let room = (width - self.inset_of(&self.analysis.lines[t.first])).max(80.);
            let cols = match held {
                Some((_, c)) if c.len() == t.columns() => c.clone(),
                _ => self.columns_of(t, room, window),
            };
            let mut xs = Vec::with_capacity(cols.len());
            let mut x = 0.;
            for c in &cols {
                xs.push(x);
                x += c;
            }
            grids.insert(t.first, (key, Rc::new(Grid { cols, xs, width: x })));
        }
    }

    fn columns_of(&self, t: &Table, room: f32, window: &mut Window) -> Vec<f32> {
        let n = t.columns();
        if let Some(shares) = t.shares.as_ref().filter(|s| s.len() == n) {
            let total: f32 = shares.iter().sum();
            return shares.iter().map(|s| (s / total * room).round().max(LEAST_COLUMN)).collect();
        }
        let at_rest = Reader { sel: None, ..*self };
        let px_cell = self.rhythm.cell_px;
        let ink = self.look.colors.ink;
        let (mut most, mut least) = (vec![0f32; n], vec![0f32; n]);
        let measure = |text: &str, head: bool, window: &mut Window| -> f32 {
            let font = self.font(&self.look.text_family, if head && !self.look.plain_head { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }, false);
            let run = TextRun { len: text.len(), font, color: ink, background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(SharedString::from(text.to_string()), px(px_cell), &[run], None).width())
        };
        for (ri, spots) in t.cells.iter().enumerate() {
            let line = t.line_of(ri);
            let hidden = at_rest.hidden_on(line);
            let text = self.doc.line(line);
            for (ci, spot) in spots.iter().enumerate().take(n) {
                for para in at_rest.para_ranges(line, spot.clone(), &hidden) {
                    let mut out = hidden.clone();
                    out.push(0..para.start);
                    out.push(para.end..text.len());
                    let shown = Shown::new(text, &out);
                    if shown.text.is_empty() {
                        continue;
                    }
                    most[ci] = most[ci].max(measure(&shown.text, ri == 0, window));
                    if let Some(word) = longest_word(&shown.text) {
                        least[ci] = least[ci].max(measure(word, ri == 0, window));
                    }
                }
            }
        }
        let pad = CELL_X * 2. + 1.;
        let most: Vec<f32> = most.iter().map(|w| (w + pad).max(LEAST_COLUMN)).collect();
        let least: Vec<f32> = least.iter().zip(&most).map(|(w, m)| (w + pad).max(LEAST_COLUMN).min(*m)).collect();
        shared(&most, &least, room)
    }

    #[allow(clippy::too_many_arguments)]
    fn table_row(&self, t: &Table, row: usize, grid: Rc<Grid>, hidden: &[Range<usize>], _width: f32, window: &mut Window) -> Row {
        let line = t.line_of(row);
        let rhythm = self.rhythm;
        let weight = if row == 0 && !self.look.plain_head { FontWeight::SEMIBOLD } else { FontWeight::NORMAL };
        let (styles, links) = self.styled(line, weight, false, false, &[]);
        let spots: Vec<Range<usize>> = t.cells.get(row).cloned().unwrap_or_default();
        let mut cells = Vec::with_capacity(grid.cols.len());
        let mut tallest = rhythm.cell_line;
        for (ci, col) in grid.cols.iter().enumerate() {
            let range = spots.get(ci).cloned().unwrap_or_else(|| {
                let end = self.doc.line(line).len();
                end..end
            });
            let align = match t.aligns.get(ci) {
                Some(Align::Center) => TextAlign::Center,
                Some(Align::Right) => TextAlign::Right,
                _ => TextAlign::Left,
            };
            let wrap = (col - CELL_X * 2.).max(10.);
            let flow = self.flow(line, range, &styles, &links, hidden, rhythm.cell_px, rhythm.cell_line, wrap, align, self.look.colors.ink, window);
            tallest = tallest.max(flow.height);
            cells.push(flow);
        }
        let inset = self.inset_of(&self.analysis.lines[line]);
        Row::Table(TableRow { grid, head: row == 0, cells, spots, height: tallest + rhythm.cell_y * 2., inset, cell_y: rhythm.cell_y })
    }
}

/// What of a run stands raised or lowered and has a character of its own
/// for it: text has one size to a line, so `<sup>2</sup>` is drawn as `²`.
/// Hidden bytes are passed by, and a word with no such character stands
/// as it is.
fn raised_chars(text: &str, range: Range<usize>, styles: &[St], hidden: &[Range<usize>]) -> Vec<(Range<usize>, String)> {
    let Some(words) = text.get(range.clone()) else { return Vec::new() };
    let mut swaps = Vec::new();
    for (i, c) in words.char_indices() {
        let at = range.start + i;
        let raise = styles.get(at).map_or(0, |s| s.raise);
        if raise == 0 || hidden.iter().any(|h| h.contains(&at)) {
            continue;
        }
        if let Some(to) = raised(c, raise > 0) {
            swaps.push((at..at + c.len_utf8(), to.to_string()));
        }
    }
    swaps
}

/// A character's raised or lowered form, where Unicode has one.
fn raised(c: char, up: bool) -> Option<char> {
    const UP: &[(char, char)] = &[('0', '⁰'), ('1', '¹'), ('2', '²'), ('3', '³'), ('4', '⁴'), ('5', '⁵'), ('6', '⁶'), ('7', '⁷'), ('8', '⁸'), ('9', '⁹'), ('+', '⁺'), ('-', '⁻'), ('=', '⁼'), ('(', '⁽'), (')', '⁾'), ('a', 'ᵃ'), ('b', 'ᵇ'), ('c', 'ᶜ'), ('d', 'ᵈ'), ('e', 'ᵉ'), ('f', 'ᶠ'), ('g', 'ᵍ'), ('h', 'ʰ'), ('i', 'ⁱ'), ('j', 'ʲ'), ('k', 'ᵏ'), ('l', 'ˡ'), ('m', 'ᵐ'), ('n', 'ⁿ'), ('o', 'ᵒ'), ('p', 'ᵖ'), ('r', 'ʳ'), ('s', 'ˢ'), ('t', 'ᵗ'), ('u', 'ᵘ'), ('v', 'ᵛ'), ('w', 'ʷ'), ('x', 'ˣ'), ('y', 'ʸ'), ('z', 'ᶻ')];
    const DOWN: &[(char, char)] = &[('0', '₀'), ('1', '₁'), ('2', '₂'), ('3', '₃'), ('4', '₄'), ('5', '₅'), ('6', '₆'), ('7', '₇'), ('8', '₈'), ('9', '₉'), ('+', '₊'), ('-', '₋'), ('=', '₌'), ('(', '₍'), (')', '₎'), ('a', 'ₐ'), ('e', 'ₑ'), ('h', 'ₕ'), ('i', 'ᵢ'), ('j', 'ⱼ'), ('k', 'ₖ'), ('l', 'ₗ'), ('m', 'ₘ'), ('n', 'ₙ'), ('o', 'ₒ'), ('p', 'ₚ'), ('r', 'ᵣ'), ('s', 'ₛ'), ('t', 'ₜ'), ('u', 'ᵤ'), ('v', 'ᵥ'), ('x', 'ₓ')];
    (if up { UP } else { DOWN }).iter().find(|(from, _)| *from == c).map(|(_, to)| *to)
}

/// The widths of columns that want `most` and hold at `least`, in `room`.
pub(crate) fn shared(most: &[f32], least: &[f32], room: f32) -> Vec<f32> {
    let (top, bottom): (f32, f32) = (most.iter().sum(), least.iter().sum());
    if top <= room {
        return most.to_vec();
    }
    if bottom >= room {
        let k = room / bottom.max(1.);
        return least.iter().map(|w| (w * k).floor()).collect();
    }
    let k = (room - bottom) / (top - bottom).max(1.);
    least.iter().zip(most).map(|(l, m)| (l + (m - l) * k).floor()).collect()
}

/// The longest run that cannot break: a word; a wide character breaks
/// anywhere, so it is its own.
pub(crate) fn longest_word(text: &str) -> Option<&str> {
    let wide = |c: char| (c as u32) >= 0x2E80;
    let mut best: Option<&str> = None;
    let mut start = None;
    let mut consider = |a: usize, b: usize| {
        if best.is_none_or(|w| w.len() < b - a) {
            best = text.get(a..b);
        }
    };
    for (i, c) in text.char_indices() {
        if c.is_whitespace() || wide(c) {
            if let Some(s) = start.take() {
                consider(s, i);
            }
            if wide(c) {
                consider(i, i + c.len_utf8());
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        consider(s, text.len());
    }
    best
}

/// The colors of a code line, from its block's reading.
fn code_colors_of<'c>(analysis: &Analysis, cache: &'c Cache, line: usize) -> &'c [(Range<usize>, Hsla)] {
    let Some(b) = analysis.code_block_at(line) else { return &[] };
    let Some((_, per_line)) = cache.code_colors.get(&b.first) else { return &[] };
    line.checked_sub(b.first + 1).and_then(|ix| per_line.get(ix)).map(|v| v.as_slice()).unwrap_or(&[])
}

trait HashKey {
    fn hash_key(&self, h: &mut DefaultHasher);
}

impl HashKey for LineKind {
    fn hash_key(&self, h: &mut DefaultHasher) {
        match self {
            LineKind::Text => 0u8.hash(h),
            LineKind::Blank => 1u8.hash(h),
            LineKind::Heading(l) => (2u8, l).hash(h),
            LineKind::SetextUnderline => 3u8.hash(h),
            LineKind::FenceOpen(l) => (4u8, l).hash(h),
            LineKind::FenceClose => 5u8.hash(h),
            LineKind::Code => 6u8.hash(h),
            LineKind::FrontMatterEdge => 7u8.hash(h),
            LineKind::FrontMatter => 8u8.hash(h),
            LineKind::Rule => 9u8.hash(h),
            LineKind::TableFirst(t) => (10u8, t).hash(h),
            LineKind::TableRow(t) => (11u8, t).hash(h),
            LineKind::Html => 12u8.hash(h),
            LineKind::Source => 13u8.hash(h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::editor::Look;
    use super::{longest_word, shared, Reader, Rhythm};
    use crate::live::lines::{Doc, Pos};
    use crate::live::markdown;

    #[test]
    fn columns_share_the_room_as_a_browser_shares_it() {
        // All fits: each as wide as it asks.
        assert_eq!(shared(&[100., 200.], &[50., 50.], 400.), vec![100., 200.]);
        // No room for all: each keeps its word, the rest shared by what it asks.
        assert_eq!(shared(&[100., 300.], &[100., 100.], 300.), vec![100., 200.]);
        // Not even for the words: down together, and never past the room.
        let w = shared(&[100., 300.], &[200., 200.], 300.);
        assert!(w.iter().sum::<f32>() <= 300.);
    }

    #[test]
    fn the_longest_word_is_what_cannot_break() {
        assert_eq!(longest_word("Pharmacy Medicines"), Some("Medicines"));
        assert_eq!(longest_word("酮康唑"), Some("酮"));
        assert_eq!(longest_word("OTC 口腔"), Some("OTC"));
        assert_eq!(longest_word("   "), None);
    }

    #[test]
    fn the_caret_is_the_words_height_and_never_past_its_line() {
        assert_eq!(Rhythm::caret(16., 26.), 19.);
        assert_eq!(Rhythm::caret(26., 32.), 31.);
        assert_eq!(Rhythm::caret(40., 32.), 32.);
    }

    #[test]
    fn a_rhythm_is_in_whole_pixels() {
        for text_px in [14., 16., 17., 18., 23.] {
            let look = Look { text_px, mono_px: text_px - 2., line_px: text_px * 1.6, ..Look::default() };
            let r = Rhythm::of(&look);
            for v in [r.line, r.code_line, r.cell_line, r.cell_y] {
                assert_eq!(v, v.round());
            }
            for level in 1..=6 {
                let (_, line, above) = r.heading(&look, level);
                assert_eq!((line, above), (line.round(), above.round()));
            }
        }
    }

    /// What `hidden_on` hides on the first line of `text`, as the words
    /// left showing, with the selection from `a` to `b` or none.
    fn showing(text: &str, sel: Option<(usize, usize)>) -> String {
        let doc = Doc::from_text(text);
        let analysis = markdown::analyze(text);
        let look = Look::default();
        let reader = Reader { doc: &doc, analysis: &analysis, look: &look, rhythm: Rhythm::of(&look), mode: Default::default(), sel: sel.map(|(a, b)| (Pos::new(0, a), Pos::new(0, b))), issues: &[], quiet: None, marks: &[], hold: None, pieces: &[], version: 0 };
        markdown::Shown::new(doc.line(0), &reader.hidden_on(0)).text
    }

    #[test]
    fn one_blank_line_of_a_run_is_the_room() {
        // Lines 1 to 3 are one run, 5 a run of one.
        let text = "a\n\n\n\nb\n\nc";
        let doc = Doc::from_text(text);
        let analysis = markdown::analyze(text);
        let look = Look::default();
        let room = |caret: Option<usize>| -> Vec<usize> {
            let reader = Reader { doc: &doc, analysis: &analysis, look: &look, rhythm: Rhythm::of(&look), mode: Default::default(), sel: caret.map(|l| (Pos::new(l, 0), Pos::new(l, 0))), issues: &[], quiet: None, marks: &[], hold: None, pieces: &[], version: 0 };
            (0..doc.line_count()).filter(|&l| analysis.lines[l].kind == markdown::LineKind::Blank && reader.blank_room(l)).collect()
        };
        assert_eq!(room(None), vec![3, 5]);
        assert_eq!(room(Some(2)), vec![3, 5]);
        // The caret on the run's last: the room is its first.
        assert_eq!(room(Some(3)), vec![1, 5]);
        assert_eq!(room(Some(5)), vec![3, 5]);
    }

    #[test]
    fn a_spans_marks_hide_unless_the_caret_touches_it() {
        let text = "a **bold** and `code` and [link](http://x) end";
        // The caret away: every mark is hidden.
        assert_eq!(showing(text, None), "a bold and code and link end");
        // The caret at the span's end, its marks included: they show.
        assert_eq!(showing(text, Some((10, 10))), "a **bold** and code and link end");
        assert_eq!(showing(text, Some((2, 2))), "a **bold** and code and link end");
        // A link's marks show only with the caret inside it, not at its end.
        assert_eq!(showing(text, Some((42, 42))), "a bold and code and link end");
        assert_eq!(showing(text, Some((30, 30))), "a bold and code and [link](http://x) end");
    }

    #[test]
    fn a_lines_marks_hide_unless_the_caret_is_on_it() {
        assert_eq!(showing("## Title", None), "Title");
        assert_eq!(showing("## Title", Some((8, 8))), "## Title");
        assert_eq!(showing("> quoted", None), "quoted");
        // A list's marker is always the widget's.
        assert_eq!(showing("- [ ] task", None), "task");
        assert_eq!(showing("- [ ] task", Some((10, 10))), "task");
        assert_eq!(showing("12. item", Some((4, 4))), "item");
    }

    #[test]
    fn a_picture_shows_as_it_is_written() {
        assert_eq!(showing("![alt](a.png)", None), "![alt](a.png)");
        assert_eq!(showing("see ![alt](a.png) and **b**", None), "see ![alt](a.png) and b");
    }
}
