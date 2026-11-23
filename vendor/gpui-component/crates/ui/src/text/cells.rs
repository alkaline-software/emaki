//! A table's cells in the view's selection. (Emaki addition.)
//!
//! The view keeps a selection as two points and takes every letter in the
//! band between them, line by line across the whole width. In a table that
//! band runs through the cells beside the one dragged in, and a point kept
//! in the window lands on other words once the table is scrolled sideways.
//! Here a table says where it and each of its cells are painted, and a run
//! of text in a cell asks what of its cell is selected:
//!
//! - both ends in its cell: the band, in that cell alone;
//! - the ends in two cells: every cell of the block between them, whole,
//!   as a spreadsheet or a word processor selects;
//! - one end outside the table: whole rows, from the other end's row to
//!   the table's edge that way.
//!
//! An end put at a table's height is kept with the table's scroll as it
//! was then, and moved by as much as the table has scrolled since, so it
//! stays on its words.

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, Entity, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, ScrollHandle, Window, point, px,
};

use crate::global_state::UiGlobalState;

/// One end of a selection as a table last saw it: the point the selection
/// keeps for it, which stays the same until the end is put elsewhere, and
/// the table's scroll when it was put at the table's height.
#[derive(Clone, Copy, PartialEq)]
struct End {
    key: Point<Pixels>,
    at: Option<Pixels>,
}

/// What a table keeps between frames about the selection's ends.
#[derive(Default)]
pub(crate) struct TableSel {
    anchor: Option<End>,
    cursor: Option<End>,
    click: Option<End>,
}

impl TableSel {
    /// How far an end has moved with the table's scroll since it was put:
    /// `None` when it is not at this table's height. `live` is an end that
    /// still follows the pointer, which has not moved with anything.
    fn moved(kept: &mut Option<End>, key: Option<(Point<Pixels>, bool)>, live: bool, now: Pixels) -> Option<Pixels> {
        let Some((key, within)) = key else {
            *kept = None;
            return None;
        };
        if live || kept.map(|e| e.key) != Some(key) {
            *kept = Some(End { key, at: within.then_some(now) });
        }
        kept.and_then(|e| e.at).map(|at| now - at)
    }
}

/// Where a table is painted, and its ends' moves, for the runs of text
/// inside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TableSpot {
    /// The rows' top and bottom, and the left and right of what shows.
    top: Pixels,
    bottom: Pixels,
    left: Pixels,
    right: Pixels,
    anchor: Option<Pixels>,
    cursor: Option<Pixels>,
    click: Option<Pixels>,
    /// The cell being painted.
    cell: Option<Bounds<Pixels>>,
}

/// What of a cell is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    /// The letters in the band between the two points.
    Band,
    Whole,
    Nothing,
}

impl TableSpot {
    /// The selection's two ends where they are against this table now: an
    /// end at the table's height is held to what shows of it, then moved
    /// by the table's scroll since.
    pub(crate) fn ends(&self, anchor: Point<Pixels>, cursor: Point<Pixels>) -> (Point<Pixels>, Point<Pixels>) {
        (self.put(anchor, self.anchor), self.put(cursor, self.cursor))
    }

    /// A double click's point, moved with the table's scroll since.
    pub(crate) fn click(&self, at: Point<Pixels>) -> Point<Pixels> {
        match self.click {
            Some(by) => point(at.x + by, at.y),
            None => at,
        }
    }

    fn put(&self, at: Point<Pixels>, moved: Option<Pixels>) -> Point<Pixels> {
        match moved {
            Some(by) => point(at.x.max(self.left).min((self.right - px(1.)).max(self.left)) + by, at.y),
            None => at,
        }
    }

    /// What the cell being painted has of a selection between `anchor` and
    /// `cursor`, as `ends` gave them.
    pub(crate) fn taken(&self, anchor: Point<Pixels>, cursor: Point<Pixels>) -> Taken {
        let Some(cell) = self.cell else { return Taken::Band };
        let within = |p: Point<Pixels>| p.y >= self.top && p.y < self.bottom;
        // An end against the cell's row and column: before, in, after.
        let row = |p: Point<Pixels>| if p.y < cell.top() { -1 } else if p.y >= cell.bottom() { 1 } else { 0 };
        let col = |p: Point<Pixels>| if p.x < cell.left() { -1 } else if p.x >= cell.right() { 1 } else { 0 };
        let whole = |yes: bool| if yes { Taken::Whole } else { Taken::Nothing };
        match (within(anchor), within(cursor)) {
            // The band covers the table or misses it.
            (false, false) => Taken::Band,
            (true, true) => {
                let (rows, cols) = ((row(anchor), row(cursor)), (col(anchor), col(cursor)));
                if rows == (0, 0) && cols == (0, 0) { Taken::Band } else { whole(rows.0 * rows.1 <= 0 && cols.0 * cols.1 <= 0) }
            }
            // Whole rows, from the end in the table to its edge toward
            // the end outside.
            (inside, _) => {
                let (inner, outer) = if inside { (anchor, cursor) } else { (cursor, anchor) };
                whole(if outer.y >= self.bottom { row(inner) <= 0 } else { row(inner) >= 0 })
            }
        }
    }
}

/// Around a table's rows, or one of its cells: says where it is painted
/// while what is inside it paints.
pub(crate) struct CellMark {
    child: AnyElement,
    table: Option<(Entity<TableSel>, ScrollHandle)>,
}

/// Around what scrolls of a table: its rows' room.
pub(crate) fn table_mark(sel: Entity<TableSel>, scroll: ScrollHandle, child: impl IntoElement) -> CellMark {
    CellMark { child: child.into_any_element(), table: Some((sel, scroll)) }
}

/// Around one cell of a table inside a `table_mark`.
pub(crate) fn cell_mark(child: impl IntoElement) -> CellMark {
    CellMark { child: child.into_any_element(), table: None }
}

impl IntoElement for CellMark {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for CellMark {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut Self::RequestLayoutState, _: &mut Self::PrepaintState, window: &mut Window, cx: &mut App) {
        let Some((sel, scroll)) = self.table.clone() else {
            // A cell: named while its words paint.
            let outer = UiGlobalState::global_mut(cx).table_spots.last_mut().map(|spot| spot.cell.replace(bounds));
            self.child.paint(window, cx);
            if let (Some(outer), Some(spot)) = (outer, UiGlobalState::global_mut(cx).table_spots.last_mut()) {
                spot.cell = outer;
            }
            return;
        };
        let now = scroll.offset().x;
        let mask = window.content_mask().bounds;
        let (left, right) = (bounds.left().max(mask.left()), bounds.right().min(mask.right()));
        let within = |p: Point<Pixels>| p.y >= bounds.top() && p.y < bounds.bottom();
        let view = UiGlobalState::global(cx).text_view_state().cloned();
        let (ends, click) = view.map(|view| {
            let view = view.read(cx);
            (view.selection_ends(cx), view.multi_click_points())
        }).unwrap_or_default();
        let spot = sel.update(cx, |sel, _| {
            let selecting = ends.is_some_and(|(_, selecting)| selecting);
            let end = |ix: usize| ends.map(|(ends, _)| (ends[ix].0, within(ends[ix].1)));
            TableSpot {
                top: bounds.top(),
                bottom: bounds.bottom(),
                left,
                right,
                anchor: TableSel::moved(&mut sel.anchor, end(0), false, now),
                cursor: TableSel::moved(&mut sel.cursor, end(1), selecting, now),
                click: TableSel::moved(&mut sel.click, click.map(|(key, at)| (key, bounds.contains(&at))), false, now),
                cell: None,
            }
        });
        UiGlobalState::global_mut(cx).table_spots.push(spot);
        self.child.paint(window, cx);
        UiGlobalState::global_mut(cx).table_spots.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    /// A table of two rows and three columns, each cell 100 by 40, at
    /// (0, 100).
    fn spot(row: usize, col: usize) -> TableSpot {
        TableSpot {
            top: px(100.),
            bottom: px(180.),
            left: px(0.),
            right: px(300.),
            anchor: Some(px(0.)),
            cursor: Some(px(0.)),
            click: None,
            cell: Some(Bounds::new(point(px(col as f32 * 100.), px(100. + row as f32 * 40.)), size(px(100.), px(40.)))),
        }
    }

    fn taken(row: usize, col: usize, anchor: (f32, f32), cursor: (f32, f32)) -> Taken {
        let spot = spot(row, col);
        let (a, c) = spot.ends(point(px(anchor.0), px(anchor.1)), point(px(cursor.0), px(cursor.1)));
        spot.taken(a, c)
    }

    #[test]
    fn a_drag_inside_a_cell_stays_in_it() {
        // Two lines down the middle cell of the first row.
        let (a, c) = ((110., 105.), (160., 135.));
        assert_eq!(taken(0, 1, a, c), Taken::Band);
        assert_eq!(taken(0, 0, a, c), Taken::Nothing);
        assert_eq!(taken(0, 2, a, c), Taken::Nothing);
        assert_eq!(taken(1, 1, a, c), Taken::Nothing);
    }

    #[test]
    fn a_drag_across_cells_takes_the_block_between() {
        // From the first row's middle cell to the second row's last.
        let (a, c) = ((150., 110.), (250., 150.));
        for (row, col, want) in [(0, 0, Taken::Nothing), (0, 1, Taken::Whole), (0, 2, Taken::Whole), (1, 0, Taken::Nothing), (1, 1, Taken::Whole), (1, 2, Taken::Whole)] {
            assert_eq!(taken(row, col, a, c), want, "cell {row},{col}");
            assert_eq!(taken(row, col, c, a), want, "cell {row},{col}, dragged back");
        }
    }

    #[test]
    fn a_drag_out_of_the_table_takes_whole_rows() {
        // From the second row down past the table.
        let (a, c) = ((150., 150.), (20., 400.));
        assert_eq!(taken(0, 1, a, c), Taken::Nothing);
        assert_eq!(taken(1, 0, a, c), Taken::Whole);
        assert_eq!(taken(1, 2, a, c), Taken::Whole);
        // From the first row up past it.
        let (a, c) = ((150., 110.), (20., 10.));
        assert_eq!(taken(0, 2, a, c), Taken::Whole);
        assert_eq!(taken(1, 0, a, c), Taken::Nothing);
        // Both ends outside: the band's own business.
        assert_eq!(taken(0, 0, (0., 10.), (0., 400.)), Taken::Band);
    }

    #[test]
    fn a_pointer_past_the_side_is_the_column_at_that_edge() {
        // From the middle cell out past the table's right, on its row.
        assert_eq!(taken(0, 2, (150., 110.), (900., 115.)), Taken::Whole);
        // In the last cell and out past the right: still that cell's.
        assert_eq!(taken(0, 2, (250., 110.), (900., 115.)), Taken::Band);
    }

    #[test]
    fn an_end_goes_with_the_scroll() {
        // The anchor was put in the middle cell; the table has since
        // scrolled 100 to the left, so its words are now where the first
        // cell was, and that is where this cell is painted.
        let mut spot = spot(0, 1);
        spot.anchor = Some(px(-100.));
        spot.cursor = Some(px(-100.));
        spot.cell = Some(Bounds::new(point(px(0.), px(100.)), size(px(100.), px(40.))));
        let (a, c) = spot.ends(point(px(110.), px(105.)), point(px(160.), px(135.)));
        assert_eq!((a.x, c.x), (px(10.), px(60.)));
        assert_eq!(spot.taken(a, c), Taken::Band);
        // The cell now painted where the press was has none of it.
        spot.cell = Some(Bounds::new(point(px(100.), px(100.)), size(px(100.), px(40.))));
        assert_eq!(spot.taken(a, c), Taken::Nothing);
    }

    #[test]
    fn a_kept_end_moves_and_a_live_one_does_not() {
        let key = point(px(5.), px(5.));
        let mut kept = None;
        assert_eq!(TableSel::moved(&mut kept, Some((key, true)), false, px(0.)), Some(px(0.)));
        assert_eq!(TableSel::moved(&mut kept, Some((key, true)), false, px(-40.)), Some(px(-40.)));
        // Still following the pointer: where it is, whatever scrolled.
        assert_eq!(TableSel::moved(&mut kept, Some((key, true)), true, px(-60.)), Some(px(0.)));
        // Let go there, then scrolled on.
        assert_eq!(TableSel::moved(&mut kept, Some((key, true)), false, px(-90.)), Some(px(-30.)));
        // Put elsewhere, off the table's height.
        assert_eq!(TableSel::moved(&mut kept, Some((point(px(9.), px(9.)), false)), false, px(-90.)), None);
    }
}
