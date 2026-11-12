//! A table on the canvas: the caret goes from cell to cell, and the
//! table's edges and handles change it. A cell's words are bytes of their
//! row's line, so a keystroke in one is a keystroke in the text like any
//! other, and what cannot be typed in a cell is written so it can: a bar
//! as `\|`, a line break as `<br>`.
//!
//! A change of the table's shape goes through `tables::Sheet` and is one
//! step of undo.
//!
//! From Onyx's `editor/cells.rs`. A press on a handle, and a right click
//! in a cell, say what they were on (`EditorEvent::Menu`); the view
//! around the editor makes the menu and answers with `table_do`.

use std::rc::Rc;
use std::time::Instant;

use gpui::*;

use super::Editor;
use crate::live::layout::{Cache, Grid, Row};
use crate::live::lines::{Pos, Sel};
use crate::live::markdown::{cell_ranges, Align, SpanKind, Table};
use crate::live::tables::{body_start, Sheet};

/// A handle's long side and its short one, and the strips that add a row
/// or a column: their width and how far off the table they stand.
pub(super) const GRIP_LONG: f32 = 24.;
pub(super) const GRIP_SHORT: f32 = 12.;
pub(super) const ADD: f32 = 16.;
pub(super) const ADD_GAP: f32 = 5.;
/// How far from a column's edge a press takes the edge.
const EDGE: f32 = 3.;
/// How far a handle is dragged before it is a move and not a press.
const DRAG_FROM: f32 = 4.;
const LEAST_COLUMN: f32 = 48.;

/// A cell, by its table's first line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellAt {
    pub table: usize,
    pub row: usize,
    pub col: usize,
}

/// What a table's menu is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Cell,
    Row,
    Column,
}

/// What the menu needs to know of the table it opens on.
#[derive(Clone, Debug)]
pub struct TableFacts {
    pub rows: usize,
    pub columns: usize,
    pub align: Align,
}

/// Where the pointer is in a table, and since when it has been there.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hover {
    pub first: usize,
    pub row: usize,
    pub col: usize,
    pub since: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Control {
    RowGrip(usize),
    ColumnGrip(usize),
    /// A column's right edge.
    Edge(usize),
    AddRow,
    AddColumn,
}

pub(super) enum TableDrag {
    Width { first: usize, col: usize, x0: f32, cols: Vec<f32> },
    Row { first: usize, row: usize, y0: f32, to: usize, moved: bool, at: Point<Pixels> },
    Column { first: usize, col: usize, x0: f32, to: usize, moved: bool, at: Point<Pixels> },
}

/// A table's box on the page, from the element's top left.
pub(super) struct TableBox {
    pub left: f32,
    pub top: f32,
    pub bottom: f32,
    pub right: f32,
    pub grid: Rc<Grid>,
}

/// The rows' tops and foots of a table, by row.
fn row_span(t: &Table, row: usize, cache: &Cache) -> Option<(f32, f32)> {
    let line = t.line_of(row);
    Some((*cache.tops.get(line)?, cache.tops[line] + cache.entries.get(line)?.height))
}

pub(super) fn table_box(t: &Table, cache: &Cache) -> Option<TableBox> {
    let Row::Table(r) = &*cache.entries.get(t.first)?.row else { return None };
    let last = cache.entries.get(t.last)?;
    let (left, top) = (r.inset, cache.tops[t.first]);
    Some(TableBox { left, top, bottom: cache.tops[t.last] + last.height, right: left + r.grid.width, grid: r.grid.clone() })
}

impl Editor {
    pub(super) fn table(&self, first: usize) -> Option<&Table> {
        self.analysis.tables.iter().find(|t| t.first == first)
    }

    /// The cell a place stands in.
    pub(crate) fn cell_of(&self, p: Pos) -> Option<CellAt> {
        let t = self.analysis.table_at(p.line)?;
        let row = t.row_of(p.line)?;
        let spots = t.cells.get(row)?;
        let col = spots.iter().position(|s| p.col <= s.end).unwrap_or(spots.len().saturating_sub(1));
        Some(CellAt { table: t.first, row, col })
    }

    /// A cell's words, as places.
    pub(crate) fn cell_span(&self, at: CellAt) -> (Pos, Pos) {
        let spot = self.table(at.table).and_then(|t| Some((t.line_of(at.row), t.cells.get(at.row)?.get(at.col)?.clone())));
        match spot {
            Some((line, r)) => (Pos::new(line, r.start), Pos::new(line, r.end)),
            None => (Pos::default(), Pos::default()),
        }
    }

    /// Whether the selection stands in one cell, for typing in.
    pub(super) fn in_cell(&self) -> bool {
        let (a, b) = self.sel.range();
        let cell = self.cell_of(a);
        cell.is_some() && cell == self.cell_of(b)
    }

    /// Words as a cell can hold them.
    pub(super) fn cell_words(&self, text: &str) -> String {
        if !self.in_cell() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if chars.peek() == Some(&'|') => {
                    out.push_str("\\|");
                    chars.next();
                }
                '|' => out.push_str("\\|"),
                '\r' => {}
                '\n' => out.push_str("<br>"),
                _ => out.push(c),
            }
        }
        out
    }

    /// A place in a table's line, in the cell nearest it: nothing stands
    /// between the cells, or on the separator.
    fn into_cell(&self, p: Pos) -> Pos {
        let Some(t) = self.analysis.table_at(p.line) else { return p };
        let row = t.row_of(p.line).unwrap_or(0);
        let Some(spots) = t.cells.get(row).filter(|s| !s.is_empty()) else { return p };
        let c = spots.iter().position(|s| p.col <= s.end).unwrap_or(spots.len() - 1);
        Pos::new(t.line_of(row), p.col.clamp(spots[c].start, spots[c].end))
    }

    /// Where a selection from `anchor` may end: inside the cell it began
    /// in, or, begun outside a table, past the whole of one.
    pub(super) fn held_to(&self, anchor: Pos, p: Pos) -> Pos {
        if let Some(cell) = self.cell_of(anchor) {
            let (a, b) = self.cell_span(cell);
            return if p < a { a } else if p > b { b } else { p };
        }
        match self.analysis.table_at(p.line) {
            Some(t) if p >= anchor => Pos::new(t.last, self.doc.line(t.last).len()),
            Some(t) => Pos::new(t.first, 0),
            None => p,
        }
    }

    pub(super) fn settled(&self, p: Pos, select: bool) -> Pos {
        if select { self.held_to(self.sel.anchor, p) } else { self.into_cell(p) }
    }

    pub(super) fn settled_sel(&self) -> Sel {
        if self.sel.is_caret() { Sel::caret(self.into_cell(self.sel.head)) } else { self.sel }
    }

    /// The `<br>` a place stands inside or against, as a whole.
    fn break_at(&self, p: Pos) -> Option<(usize, usize)> {
        let start = self.analysis.line_starts[p.line];
        self.analysis.line_spans[p.line].iter().map(|&ix| &self.analysis.spans[ix]).filter(|s| s.kind == SpanKind::Break).map(|s| (s.range.start - start, s.range.end - start)).find(|(a, b)| *a <= p.col && p.col <= *b)
    }

    /// ← by one: a `<br>` is one step, and at a cell's start the end of
    /// the cell before it.
    pub(super) fn step_back(&self, p: Pos) -> Pos {
        if let Some(cell) = self.cell_of(p) {
            if p <= self.cell_span(cell).0 {
                let t = self.table(cell.table);
                return match (cell.col, cell.row) {
                    (0, 0) => t.and_then(|t| t.first.checked_sub(1)).map_or(p, |l| Pos::new(l, self.doc.line(l).len())),
                    (0, row) => self.cell_span(CellAt { row: row - 1, col: t.map_or(1, Table::columns) - 1, ..cell }).1,
                    (col, _) => self.cell_span(CellAt { col: col - 1, ..cell }).1,
                };
            }
        }
        let q = self.doc.prev_grapheme(p);
        match self.break_at(q) {
            Some((a, b)) if a < q.col && q.col < b => Pos::new(q.line, a),
            _ => q,
        }
    }

    /// → by one, as `step_back`.
    pub(super) fn step_on(&self, p: Pos) -> Pos {
        if let Some(cell) = self.cell_of(p) {
            if p >= self.cell_span(cell).1 {
                let Some(t) = self.table(cell.table) else { return p };
                return if cell.col + 1 < t.columns() {
                    self.cell_span(CellAt { col: cell.col + 1, ..cell }).0
                } else if cell.row + 1 < t.rows.len() {
                    self.cell_span(CellAt { row: cell.row + 1, col: 0, ..cell }).0
                } else if t.last + 1 < self.doc.line_count() {
                    Pos::new(t.last + 1, 0)
                } else {
                    p
                };
            }
        }
        let q = self.doc.next_grapheme(p);
        match self.break_at(q) {
            Some((a, b)) if a < q.col && q.col < b => Pos::new(q.line, b),
            _ => q,
        }
    }

    /// Where ⌫ stops, when it is not one character back: a cell's start
    /// holds it, and a `<br>` goes whole.
    pub(super) fn back_stop(&self, at: Pos) -> Option<Pos> {
        if let Some(cell) = self.cell_of(at) {
            if at <= self.cell_span(cell).0 {
                return Some(at);
            }
        }
        self.break_at(at).filter(|(_, b)| *b == at.col).map(|(a, _)| Pos::new(at.line, a))
    }

    /// Where ⌦ stops, as `back_stop`.
    pub(super) fn forward_stop(&self, at: Pos) -> Option<Pos> {
        if let Some(cell) = self.cell_of(at) {
            if at >= self.cell_span(cell).1 {
                return Some(at);
            }
        }
        self.break_at(at).filter(|(a, _)| *a == at.col).map(|(_, b)| Pos::new(at.line, b))
    }

    /// `to`, held inside the cell `at` stands in.
    pub(super) fn in_this_cell(&self, at: Pos, to: Pos) -> Pos {
        match self.cell_of(at) {
            Some(cell) => {
                let (a, b) = self.cell_span(cell);
                if to < a { a } else if to > b { b } else { to }
            }
            None => to,
        }
    }

    fn go_cell(&mut self, cell: CellAt, cx: &mut Context<Self>) {
        let (_, end) = self.cell_span(cell);
        self.preferred_x = None;
        self.sel = Sel::caret(end);
        self.doc.end_step();
        self.after_move(cx);
    }

    /// ↩: the cell under this one; in the last row, a new row.
    pub(super) fn cell_down(&mut self, cx: &mut Context<Self>) {
        let Some(cell) = self.cell_of(self.sel.head) else { return };
        let Some(t) = self.table(cell.table) else { return };
        if cell.row + 1 < t.rows.len() {
            self.go_cell(CellAt { row: cell.row + 1, ..cell }, cx);
        } else {
            self.table_do("row-below", cell, cx);
        }
    }

    /// ⇥: the next cell; past the last, a new row.
    pub(super) fn cell_next(&mut self, cx: &mut Context<Self>) {
        let Some(cell) = self.cell_of(self.sel.head) else { return };
        let Some(t) = self.table(cell.table) else { return };
        if cell.col + 1 < t.columns() {
            self.go_cell(CellAt { col: cell.col + 1, ..cell }, cx);
        } else if cell.row + 1 < t.rows.len() {
            self.go_cell(CellAt { row: cell.row + 1, col: 0, ..cell }, cx);
        } else {
            self.table_do("row-below", CellAt { col: 0, ..cell }, cx);
        }
    }

    /// ⇧⇥: the cell before.
    pub(super) fn cell_prev(&mut self, cx: &mut Context<Self>) {
        let Some(cell) = self.cell_of(self.sel.head) else { return };
        let Some(t) = self.table(cell.table) else { return };
        let columns = t.columns();
        match (cell.col, cell.row) {
            (0, 0) => {}
            (0, row) => self.go_cell(CellAt { row: row - 1, col: columns - 1, ..cell }, cx),
            (col, _) => self.go_cell(CellAt { col: col - 1, ..cell }, cx),
        }
    }

    pub(crate) fn table_facts(&self, at: CellAt) -> Option<TableFacts> {
        let t = self.table(at.table)?;
        Some(TableFacts { rows: t.rows.len(), columns: t.columns(), align: t.aligns.get(at.col).cloned().unwrap_or(Align::None) })
    }

    /// A choice from a table's menu, or a press on its handles, as one
    /// step of undo. The caret stands in the cell the change leaves it
    /// about.
    pub fn table_do(&mut self, what: &str, at: CellAt, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let Some(t) = self.table(at.table) else { return };
        let (first, last) = (t.first, t.last);
        let lines: Vec<String> = (first..=last).map(|l| self.doc.line(l).to_string()).collect();
        let mut s = Sheet::read(&lines);
        let (mut row, mut col) = (at.row, at.col);
        match what {
            "row-above" => s.insert_row(row),
            "row-below" => {
                s.insert_row(row + 1);
                row += 1;
            }
            "row-end" => {
                row = s.rows();
                s.insert_row(row);
            }
            "row-duplicate" => {
                s.duplicate_row(row);
                row += 1;
            }
            "row-up" if row > 0 => {
                s.move_row(row, row - 1);
                row -= 1;
            }
            "row-down" if row + 1 < s.rows() => {
                s.move_row(row, row + 1);
                row += 1;
            }
            "row-delete" => {
                if !s.delete_row(row) {
                    return self.table_gone(first, last, cx);
                }
                row = row.min(s.rows() - 1);
            }
            "col-left" => s.insert_col(col),
            "col-right" => {
                s.insert_col(col + 1);
                col += 1;
            }
            "col-end" => {
                col = s.columns();
                s.insert_col(col);
            }
            "col-move-left" if col > 0 => {
                s.move_col(col, col - 1);
                col -= 1;
            }
            "col-move-right" if col + 1 < s.columns() => {
                s.move_col(col, col + 1);
                col += 1;
            }
            "col-delete" => {
                if !s.delete_col(col) {
                    return self.table_gone(first, last, cx);
                }
                col = col.min(s.columns() - 1);
            }
            "align-left" => s.align(col, Align::Left),
            "align-center" => s.align(col, Align::Center),
            "align-right" => s.align(col, Align::Right),
            "sort-up" => s.sort(col, false),
            "sort-down" => s.sort(col, true),
            "table-delete" => return self.table_gone(first, last, cx),
            _ => return,
        }
        self.write_sheet(first, last, &s, row, col, cx);
    }

    /// A row dragged to another place, or a column.
    fn table_moved(&mut self, first: usize, from: usize, to: usize, column: bool, cx: &mut Context<Self>) {
        let Some(t) = self.table(first) else { return };
        let last = t.last;
        let lines: Vec<String> = (first..=last).map(|l| self.doc.line(l).to_string()).collect();
        let mut s = Sheet::read(&lines);
        if column {
            s.move_col(from, to);
            self.write_sheet(first, last, &s, 0, to, cx);
        } else {
            s.move_row(from, to);
            self.write_sheet(first, last, &s, to, 0, cx);
        }
    }

    fn write_sheet(&mut self, first: usize, last: usize, s: &Sheet, row: usize, col: usize, cx: &mut Context<Self>) {
        let lines = s.write();
        let line = if row == 0 { 0 } else { row + 1 };
        let spot = lines.get(line).and_then(|l| cell_ranges(l, body_start(l)).get(col).cloned()).unwrap_or(0..0);
        let caret = Pos::new(first + line, spot.end);
        let before = self.sel;
        let after = Sel::caret(caret);
        self.doc.replace_many(vec![(Pos::new(first, 0), Pos::new(last, self.doc.line(last).len()), lines.join("\n"))], before, after);
        self.sel = Sel::caret(self.doc.clamp(caret));
        self.preferred_x = None;
        self.after_edit(cx);
        self.doc.end_step();
    }

    /// The whole table out, with the line it stood on.
    fn table_gone(&mut self, first: usize, last: usize, cx: &mut Context<Self>) {
        let to = if last + 1 < self.doc.line_count() { Pos::new(last + 1, 0) } else { Pos::new(last, self.doc.line(last).len()) };
        self.replace_range(Pos::new(first, 0), to, "", cx);
        self.doc.end_step();
    }

    /// A table to write in, where the caret is, with the caret in its
    /// first cell.
    pub fn insert_table(&mut self, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let at = self.sel.head;
        let line = "|  |  |  |";
        let sep = "| --- | --- | --- |";
        let lead = if self.doc.line(at.line).trim().is_empty() { "" } else { "\n\n" };
        let text = format!("{lead}{line}\n{sep}\n{line}\n{line}\n");
        let end = self.doc.replace(self.sel.range().0, self.sel.range().1, &text, self.sel);
        let first = end.line - 4;
        self.sel = Sel::caret(Pos::new(first, 2));
        self.after_edit(cx);
        self.doc.end_step();
    }

    /// Where the pointer is in a table, kept for its handles; off every
    /// table, none.
    pub(super) fn point_at_table(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let found = {
            let cache = self.cache.borrow();
            let mut found = None;
            if !self.locked() {
                for t in &self.analysis.tables {
                    let Some(b) = table_box(t, &cache) else { continue };
                    let far = ADD + ADD_GAP + 4.;
                    if x < b.left - GRIP_SHORT || x > b.right + far || y < b.top - GRIP_SHORT || y > b.bottom + far {
                        continue;
                    }
                    let row = (0..t.rows.len()).rev().find(|r| row_span(t, *r, &cache).is_some_and(|(top, _)| top <= y)).unwrap_or(0);
                    let col = b.grid.xs.iter().rposition(|at| b.left + *at <= x).unwrap_or(0);
                    found = Some((t.first, row, col));
                    break;
                }
            }
            found
        };
        let same = |h: &Hover| Some((h.first, h.row, h.col)) == found;
        if !self.hover.as_ref().is_some_and(same) {
            let since = self.hover.filter(|h| found.is_some_and(|f| f.0 == h.first)).map_or_else(Instant::now, |h| h.since);
            self.hover = found.map(|(first, row, col)| Hover { first, row, col, since });
            cx.notify();
        }
    }

    /// The handle or edge at a point of the page.
    pub(super) fn control_at(&self, x: f32, y: f32) -> Option<Control> {
        let hover = self.hover?;
        let t = self.table(hover.first)?;
        let cache = self.cache.borrow();
        let b = table_box(t, &cache)?;
        if x >= b.right + ADD_GAP && x <= b.right + ADD_GAP + ADD && y >= b.top && y <= b.bottom {
            return Some(Control::AddColumn);
        }
        if y >= b.bottom + ADD_GAP && y <= b.bottom + ADD_GAP + ADD && x >= b.left && x <= b.right {
            return Some(Control::AddRow);
        }
        let (top, foot) = row_span(t, hover.row, &cache)?;
        if x >= b.left - GRIP_SHORT && x <= b.left + GRIP_SHORT / 2. && (y - (top + foot) / 2.).abs() <= (GRIP_LONG / 2. + 4.).min((foot - top) / 2. + 2.) {
            return Some(Control::RowGrip(hover.row));
        }
        let (cx0, cw) = (b.left + b.grid.xs.get(hover.col)?, *b.grid.cols.get(hover.col)?);
        if y >= b.top - GRIP_SHORT && y <= b.top + GRIP_SHORT / 2. && (x - (cx0 + cw / 2.)).abs() <= GRIP_LONG / 2. + 4. {
            return Some(Control::ColumnGrip(hover.col));
        }
        if y >= b.top && y <= b.bottom {
            for (c, (at, w)) in b.grid.xs.iter().zip(&b.grid.cols).enumerate() {
                if (x - (b.left + at + w)).abs() <= EDGE {
                    return Some(Control::Edge(c));
                }
            }
        }
        None
    }

    /// A press on a handle or an edge. False when it was on neither.
    pub(super) fn press_control(&mut self, x: f32, y: f32, at: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(control) = self.control_at(x, y) else { return false };
        let Some(hover) = self.hover else { return false };
        let cell = CellAt { table: hover.first, row: hover.row, col: hover.col };
        match control {
            Control::AddRow => self.table_do("row-end", cell, cx),
            Control::AddColumn => self.table_do("col-end", cell, cx),
            Control::Edge(col) => {
                let cols = self.table(hover.first).and_then(|t| Some(table_box(t, &self.cache.borrow())?.grid.cols.clone())).unwrap_or_default();
                self.hold = Some((hover.first, cols.clone()));
                self.table_drag = Some(TableDrag::Width { first: hover.first, col, x0: x, cols });
            }
            Control::RowGrip(row) => self.table_drag = Some(TableDrag::Row { first: hover.first, row, y0: y, to: row, moved: false, at }),
            Control::ColumnGrip(col) => self.table_drag = Some(TableDrag::Column { first: hover.first, col, x0: x, to: col, moved: false, at }),
        }
        cx.notify();
        true
    }

    /// A drag under way. True when there was one.
    pub(super) fn drag_control(&mut self, x: f32, y: f32, cx: &mut Context<Self>) -> bool {
        let room = self.cache.borrow().width;
        let Some(first) = self.table_drag.as_ref().map(|d| match d {
            TableDrag::Width { first, .. } | TableDrag::Row { first, .. } | TableDrag::Column { first, .. } => *first,
        }) else {
            return false;
        };
        let (row_to, col_to) = {
            let cache = self.cache.borrow();
            let t = self.table(first);
            let row = t.and_then(|t| (0..t.rows.len()).rev().find(|r| row_span(t, *r, &cache).is_some_and(|(top, _)| top <= y)));
            let col = t.and_then(|t| table_box(t, &cache)).map(|b| b.grid.xs.iter().rposition(|at| b.left + *at <= x).unwrap_or(0));
            (row.unwrap_or(0), col.unwrap_or(0))
        };
        match self.table_drag.as_mut() {
            Some(TableDrag::Width { col, x0, cols, .. }) => {
                let mut now = cols.clone();
                let col = *col;
                let d = x - *x0;
                if col + 1 < now.len() {
                    // The edge between two: what one gains the other gives.
                    let total = now[col] + now[col + 1];
                    now[col] = (now[col] + d).clamp(LEAST_COLUMN, (total - LEAST_COLUMN).max(LEAST_COLUMN));
                    now[col + 1] = total - now[col];
                } else {
                    let others: f32 = now[..col].iter().sum();
                    now[col] = (now[col] + d).clamp(LEAST_COLUMN, (room - others).max(LEAST_COLUMN));
                }
                self.hold = Some((first, now));
            }
            Some(TableDrag::Row { y0, to, moved, .. }) => {
                *moved |= (y - *y0).abs() > DRAG_FROM;
                *to = row_to;
            }
            Some(TableDrag::Column { x0, to, moved, .. }) => {
                *moved |= (x - *x0).abs() > DRAG_FROM;
                *to = col_to;
            }
            None => return false,
        }
        cx.notify();
        true
    }

    /// The end of a drag. A handle pressed and not moved opens its menu.
    pub(super) fn drop_control(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.table_drag.take() else { return false };
        match drag {
            TableDrag::Width { first, cols, .. } => {
                let now = self.hold.take().map(|(_, c)| c).unwrap_or_default();
                if now != cols {
                    if let Some(t) = self.table(first) {
                        let last = t.last;
                        let lines: Vec<String> = (first..=last).map(|l| self.doc.line(l).to_string()).collect();
                        let mut s = Sheet::read(&lines);
                        s.set_shares(&now);
                        let cell = self.cell_of(self.sel.head).filter(|c| c.table == first);
                        self.write_sheet(first, last, &s, cell.map_or(0, |c| c.row), cell.map_or(0, |c| c.col), cx);
                    }
                }
            }
            TableDrag::Row { first, row, to, moved, at, .. } => {
                if moved && to != row {
                    self.table_moved(first, row, to, false, cx);
                } else if !moved {
                    self.grip_menu(CellAt { table: first, row, col: 0 }, Scope::Row, at, cx);
                }
            }
            TableDrag::Column { first, col, to, moved, at, .. } => {
                if moved && to != col {
                    self.table_moved(first, col, to, true, cx);
                } else if !moved {
                    self.grip_menu(CellAt { table: first, row: 0, col }, Scope::Column, at, cx);
                }
            }
        }
        cx.notify();
        true
    }

    fn grip_menu(&mut self, cell: CellAt, scope: Scope, at: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(facts) = self.table_facts(cell) {
            cx.emit(super::EditorEvent::Menu(at, super::MenuAt { table: Some((cell, scope, facts)), ..Default::default() }));
        }
    }
}
