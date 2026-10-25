use gpui::Corners;
use std::{
    ops::Range,
    rc::Rc,
    sync::{Arc, Mutex},
};

use gpui::{
    App, BorderStyle, Bounds, ClickEvent, CursorStyle, Edges, Element, ElementId, GlobalElementId,
    Half, HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseClickEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    SharedString, StyledText, TextLayout, Window, point, px, quad,
};

use crate::{
    ActiveTheme,
    global_state::UiGlobalState,
    input::Selection,
    text::TextViewMultiClickKind,
    text::node::LinkMark,
    text::selection::word_range_at,
    text::text_view::{LinkClickHandlerFn, handle_link_click},
};

/// A inline element used to render a inline text and support selectable.
///
/// All text in TextView (including the CodeBlock) used this for text rendering.
pub(super) struct Inline {
    id: ElementId,
    text: SharedString,
    links: Rc<Vec<(Range<usize>, LinkMark)>>,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    /// A font family and the ranges (inline code spans) drawn in it.
    /// (Emaki addition.)
    mono: Option<(SharedString, Vec<Range<usize>>)>,
    /// A plate, as (fill, border), and the ranges (inline code spans) it
    /// is painted behind. (Emaki addition.)
    chip: Option<((Hsla, Hsla), Vec<Range<usize>>)>,
    /// The plates behind Quarto's attribute chips, in their own colors.
    /// (Emaki addition.)
    attr_chip: Option<((Hsla, Hsla), Vec<Range<usize>>)>,
    /// Formulas: the math face their room is set in, and each one's room
    /// and picture (`text/math.rs`). (Emaki addition.)
    math: Option<(SharedString, Vec<(Range<usize>, super::math::MathPicture, bool)>)>,
    styled_text: StyledText,
    link_click_handler: Option<Arc<LinkClickHandlerFn>>,

    state: Arc<Mutex<InlineState>>,
}

/// The inline text state, used RefCell to keep the selection state.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct InlineState {
    /// The text that actually rendering, matched with selection.
    pub(super) text: SharedString,
    pub(super) selection: Option<Selection>,
}

impl InlineState {
    /// Save actually rendered text for selected text to use.
    pub(crate) fn set_text(&mut self, text: SharedString) {
        self.text = text;
    }
}

impl Inline {
    pub(super) fn new(
        id: impl Into<ElementId>,
        state: Arc<Mutex<InlineState>>,
        links: Vec<(Range<usize>, LinkMark)>,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
        link_click_handler: Option<Arc<LinkClickHandlerFn>>,
    ) -> Self {
        let text = state
            .lock()
            .map(|state| state.text.clone())
            .unwrap_or_default();

        Self {
            id: id.into(),
            links: Rc::new(links),
            highlights,
            mono: None,
            chip: None,
            attr_chip: None,
            math: None,
            text: text.clone(),
            styled_text: StyledText::new(text),
            link_click_handler,
            state,
        }
    }

    /// Draw the highlighted runs inside `ranges` in `family` (inline code in
    /// the mono face). Nothing changes without a family. (Emaki addition.)
    pub(super) fn with_mono(mut self, family: Option<SharedString>, ranges: Vec<Range<usize>>) -> Self {
        if let Some(family) = family {
            if !ranges.is_empty() {
                self.mono = Some((family, ranges));
            }
        }
        self
    }

    /// Paint a rounded plate behind the text inside `ranges` (inline code).
    /// Nothing changes without colours. (Emaki addition.)
    pub(super) fn with_chip(mut self, colors: Option<(Hsla, Hsla)>, ranges: Vec<Range<usize>>) -> Self {
        if let Some(colors) = colors {
            if !ranges.is_empty() {
                self.chip = Some((colors, ranges));
            }
        }
        self
    }

    /// A plate behind the text inside `ranges` in the quiet colors of an
    /// attribute chip. (Emaki addition.)
    pub(super) fn with_attr_chip(mut self, colors: (Hsla, Hsla), ranges: Vec<Range<usize>>) -> Self {
        if !ranges.is_empty() {
            self.attr_chip = Some((colors, ranges));
        }
        self
    }

    /// Formulas in `family`: each a room of no-break spaces, its picture
    /// painted over it, on its line's baseline or, on a line of its own,
    /// centered in it. (Emaki addition.)
    pub(super) fn with_math(mut self, family: Option<SharedString>, items: Vec<(Range<usize>, super::math::MathPicture, bool)>) -> Self {
        if let Some(family) = family {
            if !items.is_empty() {
                self.math = Some((family, items));
            }
        }
        self
    }

    /// Each formula's picture over its room, in the color of the words.
    /// The room's end is where it is looked for: an index at a wrap is
    /// reported at the end of the line above, and a formula cannot start
    /// a line and end on the one above it. (Emaki addition.)
    fn paint_math(&self, text_layout: &TextLayout, bounds: &Bounds<Pixels>, window: &mut Window, cx: &App) {
        use std::hash::{Hash, Hasher};
        let Some((_, items)) = &self.math else { return };
        let color = window.text_style().color;
        let line_height = text_layout.line_height();
        for (range, picture, display) in items {
            let (Some(start), Some(end), Some(line)) = (
                text_layout.position_for_index(range.start),
                text_layout.position_for_index(range.end.min(self.text.len())),
                text_layout.line_layout_for_index(range.end.min(self.text.len())),
            ) else {
                continue;
            };
            let layout = &line.unwrapped_layout;
            let size = layout.font_size;
            let (w, h) = (size * picture.width, size * picture.height);
            let left = if start.y == end.y { start.x } else { bounds.left() };
            let x = (left + end.x - w) / 2.;
            let y = if *display {
                end.y + (line_height - h) / 2.
            } else {
                let baseline = end.y + (line_height - layout.ascent - layout.descent) / 2. + layout.ascent;
                baseline - size * picture.baseline
            };
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            picture.svg.hash(&mut hasher);
            let key = SharedString::from(format!("__math_svg__{}", hasher.finish()));
            let _ = window.paint_svg(Bounds::new(point(x, y), gpui::size(w, h)), key, Some(&picture.svg), gpui::TransformationMatrix::unit(), color, cx);
        }
    }

    /// The plates behind inline code: one per line a span is on, a little
    /// taller than the letters and centred on the line. A plate takes in
    /// the nearer of the two spaces set at each end of the span (`CODE_PAD`).
    /// Painted before the text. (Emaki addition.)
    fn paint_code_chips(&self, text_layout: &TextLayout, bounds: &Bounds<Pixels>, window: &mut Window) {
        for chip in [&self.chip, &self.attr_chip].into_iter().flatten() {
            self.paint_chips(chip, text_layout, bounds, window);
        }
    }

    fn paint_chips(&self, chip: &((Hsla, Hsla), Vec<Range<usize>>), text_layout: &TextLayout, bounds: &Bounds<Pixels>, window: &mut Window) {
        use crate::text::style::CODE_PAD;
        let ((fill, border), ranges) = chip;
        let line_height = text_layout.line_height();
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let height = (font_size * 1.35).min(line_height);
        let inset = (line_height - height).half();
        let pad_len = CODE_PAD.len_utf8();
        for range in ranges {
            let mut start = range.start;
            let mut end = range.end.min(self.text.len());
            if self.text.get(..start).is_some_and(|t| t.ends_with(CODE_PAD)) {
                start -= pad_len;
            }
            if self.text.get(end..).is_some_and(|t| t.starts_with(CODE_PAD)) {
                end += pad_len;
            }
            let Some(span) = self.text.get(start..end) else {
                continue;
            };
            // One box per line, and whether a letter of the code is on it:
            // a line holding only the padding space gets no plate.
            let mut lines: Vec<(Bounds<Pixels>, bool)> = Vec::new();
            let mut offset = start;
            let mut spots = Spots::new(text_layout);
            let mut here = spots.at(start);
            for c in span.chars() {
                let next_offset = offset + c.len_utf8();
                let next = spots.at(next_offset);
                let (Some(pos), Some(next_pos)) = (std::mem::replace(&mut here, next), next) else {
                    offset = next_offset;
                    continue;
                };
                // An index at a wrap is reported at the end of the line
                // above, so the first letter of a wrapped line is found by
                // where the next index is: it runs from the line's start.
                let (left, top) = if next_pos.y == pos.y { (pos.x, pos.y) } else { (bounds.left(), next_pos.y) };
                let letter = Bounds::from_corners(
                    point(left, top + inset),
                    point(next_pos.x, top + inset + height),
                );
                let is_code = c != CODE_PAD;
                match lines.last_mut() {
                    Some((line, any)) if line.origin.y == letter.origin.y => {
                        *line = line.union(&letter);
                        *any |= is_code;
                    }
                    _ => lines.push((letter, is_code)),
                }
                offset = next_offset;
            }
            for (line, any) in lines {
                if !any {
                    continue;
                }
                // A hair wider than the padding space, out of the margin.
                let plate = Bounds::from_corners(
                    point(line.left() - px(1.), line.top()),
                    point(line.right() + px(1.), line.bottom()),
                );
                window.paint_quad(quad(
                    plate,
                    px(6.),
                    *fill,
                    px(0.5),
                    *border,
                    BorderStyle::default(),
                ));
            }
        }
    }

    /// Get link at given mouse position.
    fn link_for_position(
        layout: &TextLayout,
        links: &Vec<(Range<usize>, LinkMark)>,
        position: Point<Pixels>,
    ) -> Option<LinkMark> {
        let offset = layout.index_for_position(position).ok()?;
        for (range, link) in links.iter() {
            if range.contains(&offset) {
                return Some(link.clone());
            }
        }

        None
    }

    /// Paint selected bounds for debug.
    #[allow(unused)]
    fn paint_selected_bounds(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        window.paint_quad(gpui::PaintQuad {
            bounds,
            background: cx.theme().blue.alpha(0.01).into(),
            corner_radii: Corners::default(),
            border_color: gpui::transparent_black(),
            border_style: BorderStyle::default(),
            border_widths: gpui::Edges::all(px(0.)),
        });
    }

    fn layout_selections(
        &self,
        text_layout: &TextLayout,
        bounds: &Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> (bool, bool, Option<Selection>) {
        let Some(text_view_state) = UiGlobalState::global(cx).text_view_state() else {
            return (false, false, None);
        };

        let text_view_state = text_view_state.read(cx);
        let is_selectable = text_view_state.is_selectable();
        if !is_selectable {
            return (false, false, None);
        }

        if text_view_state.is_all_selected() {
            return (is_selectable, true, Some((0..self.text.len()).into()));
        }

        if let Some(selection) = text_view_state.multi_click_selection() {
            return (
                is_selectable,
                true,
                selection_for_multi_click(
                    &self.text,
                    &self.links,
                    text_layout,
                    *bounds,
                    selection.pos,
                    selection.kind,
                )
                .map(Selection::from),
            );
        }

        let Some((selection_start, selection_end)) = text_view_state.selection_points(cx) else {
            return (is_selectable, false, None);
        };
        let line_height = window.line_height();

        // Use for debug selection bounds
        // self.paint_selected_bounds(Bounds::from_corners(selection_start, selection_end), window, cx);

        // NOTE: the selection is computed purely from the geometric band
        // (`selection_start`..`selection_end`), NOT from what is currently
        // visible. Every glyph of a *painted* element is laid out (its
        // `position_for_index` is valid) even when it is scrolled out of, or
        // clipped by, an ancestor's viewport — the content mask only clips the
        // painted pixels. Because the copied text is derived from
        // `InlineState.selection`, gating the selection on `content_mask` here
        // used to drop scrolled-out-but-selected glyphs, so a selection taller
        // than the viewport (e.g. a long chat message, or a drag with
        // auto-scroll) copied only the portion that happened to be on screen.
        //
        // This does not resurrect the #2156 clipped-hit-testing behavior: a
        // selection can only START on visible text (window selection resolves
        // endpoints with hitbox hover testing against visible Inline bounds),
        // so the band's endpoints are always anchored to on-screen text.
        // Content that is merely `overflow_hidden`
        // (not scrolled) lies outside that band and is still excluded, while
        // the highlight quads painted for off-screen glyphs are clipped away by
        // GPUI's content mask as before.
        let mut selection: Option<Selection> = None;
        let mut offset = 0;
        let mut spots = Spots::new(text_layout);
        let mut here = spots.at(0);
        for c in self.text.chars() {
            let next_offset = offset + c.len_utf8();
            let next = spots.at(next_offset);
            let Some(pos) = std::mem::replace(&mut here, next) else {
                offset = next_offset;
                continue;
            };

            let mut char_width = line_height.half();
            if let Some(next_pos) = next {
                if next_pos.y == pos.y {
                    char_width = next_pos.x - pos.x;
                }
            }

            if point_in_text_selection(pos, char_width, selection_start, selection_end, line_height)
            {
                if selection.is_none() {
                    selection = Some((offset..offset).into());
                }

                if let Some(selection) = selection.as_mut() {
                    selection.end = next_offset;
                }
            }

            offset = next_offset;
        }

        (true, true, selection)
    }

    fn text_line_bounds(
        &self,
        text_layout: &TextLayout,
        line_height: Pixels,
        mask_bounds: Bounds<Pixels>,
    ) -> Vec<Bounds<Pixels>> {
        let mut line_bounds = Vec::new();
        // A run wholly above or below what is shown has no box in it.
        // (Emaki addition.)
        let shown = text_layout.bounds();
        if shown.bottom() + line_height < mask_bounds.top() || shown.top() > mask_bounds.bottom() {
            return line_bounds;
        }
        let mut current_line_y = None;
        let mut current_bounds: Option<Bounds<Pixels>> = None;
        let mut offset = 0;
        let mut spots = Spots::new(text_layout);
        let mut here = spots.at(0);

        for c in self.text.chars() {
            let next_offset = offset + c.len_utf8();
            let next = spots.at(next_offset);
            let Some(pos) = std::mem::replace(&mut here, next) else {
                offset = next_offset;
                continue;
            };

            let mut char_width = line_height.half();
            if let Some(next_pos) = next {
                if next_pos.y == pos.y {
                    char_width = next_pos.x - pos.x;
                }
            }

            let bounds = Bounds::from_corners(pos, point(pos.x + char_width, pos.y + line_height))
                .intersect(&mask_bounds);
            if bounds.size.width > px(0.) && bounds.size.height > px(0.) {
                if current_line_y == Some(pos.y) {
                    if let Some(current) = current_bounds.as_mut() {
                        *current = current.union(&bounds);
                    }
                } else {
                    if let Some(current) = current_bounds.take() {
                        line_bounds.push(current);
                    }
                    current_line_y = Some(pos.y);
                    current_bounds = Some(bounds);
                }
            }

            offset = next_offset;
        }

        if let Some(current) = current_bounds {
            line_bounds.push(current);
        }

        line_bounds
    }

    /// Emaki: a found range's ground, a box a line it is on; the first
    /// line's box, where the range begins.
    fn paint_range(range: &Range<usize>, color: Hsla, text_layout: &TextLayout, bounds: &Bounds<Pixels>, window: &mut Window) -> Option<Bounds<Pixels>> {
        let start = text_layout.position_for_index(range.start)?;
        let end = text_layout.position_for_index(range.end)?;
        let line_height = text_layout.line_height();
        let fill = |b: Bounds<Pixels>, window: &mut Window| window.paint_quad(quad(b, px(2.), color, Edges::default(), gpui::transparent_black(), BorderStyle::default()));
        if start.y == end.y {
            let b = Bounds::from_corners(start, point(end.x, end.y + line_height));
            fill(b, window);
            return Some(b);
        }
        let first = Bounds::from_corners(start, point(bounds.right(), start.y + line_height));
        fill(first, window);
        if end.y > start.y + line_height {
            fill(Bounds::from_corners(point(bounds.left(), start.y + line_height), point(bounds.right(), end.y)), window);
        }
        fill(Bounds::from_corners(point(bounds.left(), end.y), point(end.x, end.y + line_height)), window);
        Some(first)
    }

    /// Paint the selection background.
    fn paint_selection(
        selection: &Selection,
        text_layout: &TextLayout,
        bounds: &Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut start = selection.start;
        let mut end = selection.end;
        if end < start {
            std::mem::swap(&mut start, &mut end);
        }
        let Some(start_position) = text_layout.position_for_index(start) else {
            return;
        };
        let Some(end_position) = text_layout.position_for_index(end) else {
            return;
        };

        let line_height = text_layout.line_height();
        if start_position.y == end_position.y {
            window.paint_quad(quad(
                Bounds::from_corners(
                    start_position,
                    point(end_position.x, end_position.y + line_height),
                ),
                px(0.),
                cx.theme().selection,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        } else {
            window.paint_quad(quad(
                Bounds::from_corners(
                    start_position,
                    point(bounds.right(), start_position.y + line_height),
                ),
                px(0.),
                cx.theme().selection,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));

            if end_position.y > start_position.y + line_height {
                window.paint_quad(quad(
                    Bounds::from_corners(
                        point(bounds.left(), start_position.y + line_height),
                        point(bounds.right(), end_position.y),
                    ),
                    px(0.),
                    cx.theme().selection,
                    Edges::default(),
                    gpui::transparent_black(),
                    BorderStyle::default(),
                ));
            }

            window.paint_quad(quad(
                Bounds::from_corners(
                    point(bounds.left(), end_position.y),
                    point(end_position.x, end_position.y + line_height),
                ),
                px(0.),
                cx.theme().selection,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        }
    }
}

impl IntoElement for Inline {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Inline {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_element_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let text_style = window.text_style();

        let mut runs = Vec::new();
        let mut ix = 0;
        for (range, highlight) in self.highlights.iter() {
            if ix < range.start {
                runs.push(text_style.clone().to_run(range.start - ix));
            }
            let mut run_style = text_style.clone();
            if let Some((family, ranges)) = &self.mono {
                // `combine_highlights` only splits at mark edges, so a run is
                // either inside a code span or outside it.
                if ranges.iter().any(|r| r.start <= range.start && range.end <= r.end) {
                    run_style.font_family = family.clone();
                }
            }
            // A formula's room is set in the math face, whose spaces are as
            // wide as the face says.
            if let Some((family, items)) = &self.math {
                if items.iter().any(|(r, _, _)| r.start <= range.start && range.end <= r.end) {
                    run_style.font_family = family.clone();
                    run_style.font_style = gpui::FontStyle::Normal;
                    run_style.font_weight = gpui::FontWeight::NORMAL;
                }
            }
            runs.push(run_style.highlight(*highlight).to_run(range.len()));
            ix = range.end;
        }
        if ix < self.text.len() {
            runs.push(text_style.to_run(self.text.len() - ix));
        }

        self.styled_text = StyledText::new(self.text.clone()).with_runs(runs);
        let (layout_id, _) =
            self.styled_text
                .request_layout(global_element_id, inspector_id, window, cx);

        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current_view = window.current_view();
        let hitbox = prepaint;
        let Ok(mut state) = self.state.lock() else {
            return;
        };

        let text_layout = self.styled_text.layout().clone();
        self.paint_code_chips(&text_layout, &bounds, window);
        // Emaki: words found in the view, behind the letters, as the
        // code editor marks them: the current one in the selection's
        // colour, the others in a grey of it.
        let find = UiGlobalState::global(cx).text_view_state().and_then(|s| s.read(cx).find.clone());
        if let Some(find) = find {
            let selection = cx.theme().selection;
            let other = Hsla { s: 0.1, ..selection };
            for (range, ix, current) in find.take_matches(&self.text) {
                if let Some(at) = Self::paint_range(&range, if current { selection } else { other }, &text_layout, &bounds, window) {
                    if current {
                        find.painted_current(ix, at);
                    }
                }
            }
        }
        self.styled_text
            .paint(global_id, None, bounds, &mut (), &mut (), window, cx);
        self.paint_math(&text_layout, &bounds, window, cx);

        // layout selections
        let (is_selectable, is_selection, selection) =
            self.layout_selections(&text_layout, &bounds, window, cx);

        state.selection = selection;

        if is_selection || is_selectable {
            window.set_cursor_style(CursorStyle::IBeam, &hitbox);
        }

        // link cursor pointer
        let mouse_position = window.mouse_position();
        if let Some(_) = Self::link_for_position(&text_layout, &self.links, mouse_position) {
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
        }

        if let Some(selection) = &state.selection {
            Self::paint_selection(selection, &text_layout, &bounds, window, cx);
        }

        if is_selectable {
            if let Some(text_view_state) = UiGlobalState::global(cx).text_view_state().cloned() {
                let text_bounds = self.text_line_bounds(
                    &text_layout,
                    text_layout.line_height(),
                    window.content_mask().bounds,
                );
                text_view_state.update(cx, |state, _| {
                    state.selection_adapter.register_inline(text_bounds);
                });
            }

            window.on_mouse_event({
                let hitbox = hitbox.clone();
                let text_layout = text_layout.clone();
                let inline_state = self.state.clone();
                let text = self.text.clone();
                let text_view_state = UiGlobalState::global(cx).text_view_state().cloned();
                // EMAKI: what of this run is selected as it is painted,
                // for the right click below.
                let selected = state.selection.as_ref().map(|s| (s.start.min(s.end), s.start.max(s.end)));
                let links = self.links.clone();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if !phase.bubble() || !hitbox.is_hovered(window) {
                        return;
                    }

                    // EMAKI: a right click selects the word under it, as
                    // a double click does, so a menu has something to
                    // copy; inside a selection there is, it changes
                    // nothing.
                    // EMAKI: a right click on a link takes the whole link,
                    // and the link is kept for the menu it opens
                    // (`right_clicked_link`).
                    let kind = match (event.button, event.click_count) {
                        (MouseButton::Right, _) => {
                            let at = text_layout.index_for_position(event.position).ok();
                            let link = at.and_then(|at| links.iter().find(|(r, _)| r.start <= at && at < r.end)).cloned();
                            note_right_clicked_link(link.as_ref().map(|(r, l)| (l.clone(), SharedString::from(text.get(r.clone()).unwrap_or("").to_string()))));
                            if selected.zip(at).is_some_and(|((start, end), at)| start < end && at >= start && at <= end) {
                                return;
                            }
                            gpui_base::TextSelection::clear(window, cx);
                            if link.is_some() { TextViewMultiClickKind::Link } else { TextViewMultiClickKind::Word }
                        }
                        (MouseButton::Left, 2) => TextViewMultiClickKind::Word,
                        (MouseButton::Left, 3) => TextViewMultiClickKind::Paragraph,
                        _ => return,
                    };

                    let Some(range) = selection_for_multi_click(
                        &text,
                        &links,
                        &text_layout,
                        hitbox.bounds,
                        event.position,
                        kind,
                    ) else {
                        return;
                    };

                    let selected_text = text[range.clone()].to_string();

                    // This renderer owns multi-click selection. Prevent the
                    // window selection layer from handling the same press.
                    gpui_base::GlobalState::suppress_text_selection(cx);

                    if let Ok(mut inline_state) = inline_state.lock() {
                        inline_state.selection = Some(range.into());
                    }
                    if let Some(text_view_state) = &text_view_state {
                        text_view_state.update(cx, |state, cx| {
                            state.set_multi_click_selection(
                                event.position,
                                kind,
                                selected_text,
                                cx,
                            );
                        });
                    }
                    cx.notify(current_view);
                }
            });
        }

        // mouse move, update hovered link
        // EMAKI: drawn again only when the pointer goes onto a link, off
        // one or from one to another, for the link's cursor. The letter
        // under the pointer was compared, so each letter it crossed drew
        // the whole view again.
        window.on_mouse_event({
            let hitbox = hitbox.clone();
            let text_layout = text_layout.clone();
            let links = self.links.clone();
            let link_at = move |position: Point<Pixels>| {
                let index = text_layout.index_for_position(position).ok()?;
                links.iter().position(|(range, _)| range.contains(&index))
            };
            let hovered = hitbox.is_hovered(window).then(|| link_at(mouse_position)).flatten();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if !phase.bubble() || !hitbox.is_hovered(window) {
                    return;
                }
                if link_at(event.position) != hovered {
                    cx.notify(current_view);
                }
            }
        });

        if !is_selection {
            // click to open link
            window.on_mouse_event({
                let links = self.links.clone();
                let text_layout = text_layout.clone();
                let hitbox = hitbox.clone();
                let text_view_state = UiGlobalState::global(cx).text_view_state().cloned();
                let link_click_handler = self.link_click_handler.clone();

                move |event: &MouseUpEvent, phase, window, cx| {
                    if !phase.bubble() || !hitbox.is_hovered(window) {
                        return;
                    }
                    if text_view_state
                        .as_ref()
                        .is_some_and(|state| state.read(cx).has_selection(cx))
                    {
                        return;
                    }

                    if let Some(link) =
                        Self::link_for_position(&text_layout, &links, event.position)
                    {
                        gpui_base::TextSelection::end(window, cx);
                        cx.stop_propagation();
                        let click = ClickEvent::Mouse(MouseClickEvent {
                            down: MouseDownEvent {
                                button: event.button,
                                position: event.position,
                                modifiers: event.modifiers,
                                click_count: event.click_count,
                                first_mouse: false,
                            },
                            up: event.clone(),
                        });
                        handle_link_click(&link_click_handler, link.url, click, window, cx);
                    }
                }
            });
        }
    }
}

fn selection_for_multi_click(
    text: &str,
    links: &[(Range<usize>, LinkMark)],
    text_layout: &TextLayout,
    bounds: Bounds<Pixels>,
    pos: Point<Pixels>,
    kind: TextViewMultiClickKind,
) -> Option<std::ops::Range<usize>> {
    if !bounds.contains(&pos) {
        return None;
    }

    let offset = text_layout.index_for_position(pos).ok()?;

    match kind {
        TextViewMultiClickKind::Word => word_range_at(text, offset),
        TextViewMultiClickKind::Link => links.iter().find(|(r, _)| r.start <= offset && offset < r.end).map(|(r, _)| r.clone()).or_else(|| word_range_at(text, offset)),
        // Known limitation: a paragraph maps to a single Inline run here. When a
        // paragraph embeds an inline image it is split into multiple Inline runs,
        // so triple-click only selects the run on the clicked side of the image.
        TextViewMultiClickKind::Paragraph => (!text.is_empty()).then_some(0..text.len()),
    }
}

/// Where a run's indices are, as `TextLayout::position_for_index` gives
/// them, for indices asked in rising order, in one walk of its lines.
/// That method walks a line from its start for each index, so asking it
/// once a character, as the selection and its text boxes do every frame,
/// took a paragraph's length squared. A line whose glyphs are not in the
/// text's order (right to left) is asked of the method. (Emaki addition.)
struct Spots {
    layout: TextLayout,
    lines: smallvec::SmallVec<[Arc<gpui::WrappedLineLayout>; 1]>,
    origin: Point<Pixels>,
    line_height: Pixels,
    /// The line (between two `\n`) the walk is in, its first index and
    /// its top.
    hard: usize,
    start: usize,
    top: Pixels,
    /// The wrapped line within it, and the x it starts at.
    visual: usize,
    left: Pixels,
    /// The glyph the walk is at.
    run: usize,
    glyph: usize,
    in_order: bool,
}

impl Spots {
    fn new(layout: &TextLayout) -> Self {
        let mut spots = Self {
            layout: layout.clone(),
            lines: layout.line_layouts(),
            origin: layout.bounds().origin,
            line_height: layout.line_height(),
            hard: 0,
            start: 0,
            top: px(0.),
            visual: 0,
            left: px(0.),
            run: 0,
            glyph: 0,
            in_order: true,
        };
        spots.enter_line();
        spots
    }

    fn enter_line(&mut self) {
        (self.visual, self.run, self.glyph) = (0, 0, 0);
        let Some(line) = self.lines.get(self.hard) else { return };
        self.in_order = line.runs().iter().flat_map(|r| &r.glyphs).map(|g| g.index).is_sorted();
        self.left = self.x_at(0);
    }

    /// `LineLayout::x_for_index` from where the walk is: the x of the
    /// first glyph at or after `index`, or the line's end.
    fn x_at(&mut self, index: usize) -> Pixels {
        let line = &self.lines[self.hard];
        let runs = line.runs();
        while let Some(run) = runs.get(self.run) {
            match run.glyphs.get(self.glyph) {
                Some(g) if g.index >= index => return g.position.x,
                Some(_) => self.glyph += 1,
                None => (self.run, self.glyph) = (self.run + 1, 0),
            }
        }
        line.unwrapped_layout.width
    }

    fn at(&mut self, index: usize) -> Option<Point<Pixels>> {
        loop {
            let line = self.lines.get(self.hard)?.clone();
            let end = self.start + line.len();
            if index < self.start {
                return None;
            }
            if index > end {
                self.top += line.size(self.line_height).height;
                self.start = end + 1;
                self.hard += 1;
                self.enter_line();
                continue;
            }
            if !self.in_order {
                return self.layout.position_for_index(index);
            }
            let i = index - self.start;
            let bounds = line.wrap_boundaries();
            let end_of = |k: usize| bounds.get(k).map_or(line.len(), |b| line.runs()[b.run_ix].glyphs[b.glyph_ix].index);
            while i > end_of(self.visual) {
                let from = end_of(self.visual);
                self.visual += 1;
                self.left = self.x_at(from);
            }
            let x = self.x_at(i) - self.left;
            return Some(self.origin + point(x, self.top + self.line_height * self.visual as f32));
        }
    }
}

/// Check if a `pos` is within a `bounds`, considering multi-line selections.
fn point_in_text_selection(
    pos: Point<Pixels>,
    char_width: Pixels,
    selection_start: Point<Pixels>,
    selection_end: Point<Pixels>,
    line_height: Pixels,
) -> bool {
    let point_in_line = |point: Point<Pixels>| point.y >= pos.y && point.y < pos.y + line_height;
    let top = selection_start.y.min(selection_end.y);
    let bottom = selection_start.y.max(selection_end.y);
    let x = pos.x + char_width.half();

    // Out of the vertical bounds
    if pos.y + line_height <= top || pos.y > bottom {
        return false;
    }

    // Treat the selection as single-line when both drag points fall within the
    // same rendered line, even if their y coordinates differ inside that line.
    if point_in_line(selection_start) && point_in_line(selection_end) {
        let left = selection_start.x.min(selection_end.x);
        let right = selection_start.x.max(selection_end.x);
        return x >= left && x <= right;
    }

    let (top_point, bottom_point) = if selection_start.y < selection_end.y {
        (selection_start, selection_end)
    } else {
        (selection_end, selection_start)
    };
    let is_top_line = point_in_line(top_point);
    let is_bottom_line = point_in_line(bottom_point);

    if is_top_line {
        return x >= top_point.x;
    } else if is_bottom_line {
        return x <= bottom_point.x;
    } else {
        return true;
    }
}

#[cfg(test)]
mod tests {
    use super::{Spots, point_in_text_selection};
    use gpui::{AvailableSpace, ParentElement as _, StyledText, Styled as _, TestAppContext, div, point, px, size};

    #[gpui::test]
    fn the_walk_puts_each_index_where_the_layout_does(cx: &mut TestAppContext) {
        let text = "a few words that wrap over several lines here\nand a second line, longer than the first, that wraps too\n\nafter an empty one";
        let words = StyledText::new(text);
        let layout = words.layout().clone();
        let cx = cx.add_empty_window();
        cx.draw(point(px(10.), px(20.)), size(AvailableSpace::Definite(px(140.)), AvailableSpace::MinContent), |_, _| div().w(px(140.)).child(words));
        assert!(layout.line_layouts().iter().any(|l| !l.wrap_boundaries().is_empty()), "the text should wrap");
        let mut spots = Spots::new(&layout);
        for i in 0..=text.len() + 1 {
            assert_eq!(spots.at(i), layout.position_for_index(i), "index {i}");
        }
    }

    #[test]
    fn test_point_in_text_selection() {
        let line_height = px(20.);
        let char_width = px(10.);
        let start = point(px(50.), px(50.));
        let end = point(px(150.), px(150.));

        // First line but haft line height, true
        // | p --------|
        // | selection |
        // |-----------|
        assert!(point_in_text_selection(
            point(px(50.), px(40.)),
            char_width,
            start,
            end,
            line_height
        ));

        // First line in selection, true
        // | p --------|
        // | selection |
        // |-----------|
        assert!(point_in_text_selection(
            point(px(50.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        // First line, but left out of selection, false
        // p |-----------|
        //   | selection |
        //   |-----------|
        assert!(!point_in_text_selection(
            point(px(40.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        // First line but right out of selection, true
        // |-----------| p
        // | selection |
        // |-----------|
        assert!(point_in_text_selection(
            point(px(160.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));

        // Middle line in selection, true
        // |-----------|
        // |     p     |
        // |-----------|
        assert!(point_in_text_selection(
            point(px(100.), px(70.)),
            char_width,
            start,
            end,
            line_height
        ));
        // Middle line, but left out of selection, true
        //   |-----------|
        // p | selection |
        //   |-----------|
        assert!(point_in_text_selection(
            point(px(40.), px(70.)),
            char_width,
            start,
            end,
            line_height
        ));
        // Middle line, but right out of selection, true
        // |-----------|
        // | selection | p
        // |-----------|
        assert!(point_in_text_selection(
            point(px(160.), px(70.)),
            char_width,
            start,
            end,
            line_height
        ));

        // Last line in selection, true
        // |-----------|
        // | selection |
        // |------- p -|
        assert!(point_in_text_selection(
            point(px(100.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));
        // Last line, but left out of selection, true
        //
        //   |-----------|
        //   | selection |
        // p |-----------|
        assert!(point_in_text_selection(
            point(px(40.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));
        // Last line, but right out of selection, false
        // |-----------|
        // | selection |
        // |-----------| p
        assert!(!point_in_text_selection(
            point(px(160.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));

        // Out of vertical bounds (top), false
        //       p
        // |-----------|
        // | selection |
        // |-----------|
        assert!(!point_in_text_selection(
            point(px(100.), px(20.)),
            char_width,
            start,
            end,
            line_height
        ));
        // Out of vertical bounds (bottom), false
        // |-----------|
        // | selection |
        // |-----------|
        //       p
        assert!(!point_in_text_selection(
            point(px(100.), px(160.)),
            char_width,
            start,
            end,
            line_height
        ));
    }

    #[test]
    fn test_point_in_text_selection_reversed_drag_direction() {
        let line_height = px(20.);
        let char_width = px(10.);

        // Mouse down on lower line then drag upward to x=150.
        // Top line should follow current mouse x, bottom line should keep anchor x.
        let start = point(px(80.), px(150.));
        let end = point(px(150.), px(50.));

        // On top line, selection starts from top cursor x (150), so x=140 should be excluded.
        assert!(!point_in_text_selection(
            point(px(140.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(150.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));

        // On bottom line, selection ends at anchor x (80), so x=90 should be excluded.
        assert!(point_in_text_selection(
            point(px(75.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(80.), px(140.)),
            char_width,
            start,
            end,
            line_height
        ));
    }

    #[test]
    fn test_point_in_text_selection_same_visual_line_with_different_y() {
        let line_height = px(20.);
        let char_width = px(10.);
        let start = point(px(100.), px(55.));
        let end = point(px(60.), px(58.));

        assert!(!point_in_text_selection(
            point(px(40.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(70.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(110.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
    }

    #[test]
    fn test_point_in_text_selection_same_visual_line_with_reversed_y() {
        let line_height = px(20.);
        let char_width = px(10.);
        let start = point(px(60.), px(58.));
        let end = point(px(100.), px(55.));

        assert!(!point_in_text_selection(
            point(px(40.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(point_in_text_selection(
            point(px(70.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
        assert!(!point_in_text_selection(
            point(px(110.), px(50.)),
            char_width,
            start,
            end,
            line_height
        ));
    }
}

thread_local! {
    /// The link a right click was last on, its words, and when. (Emaki
    /// addition.)
    static RIGHT_CLICKED_LINK: std::cell::RefCell<Option<(LinkMark, SharedString, std::time::Instant)>> = const { std::cell::RefCell::new(None) };
}

fn note_right_clicked_link(link: Option<(LinkMark, SharedString)>) {
    RIGHT_CLICKED_LINK.with(|l| *l.borrow_mut() = link.map(|(m, w)| (m, w, std::time::Instant::now())));
}

/// The link the right click just made was on, and the words it shows,
/// taken: for the menu that click opens, which offers to copy the link,
/// its words, or the markdown it was written as. None when the click was
/// not on a link. (Emaki addition.)
pub fn right_clicked_link() -> Option<(LinkMark, SharedString)> {
    RIGHT_CLICKED_LINK.with(|l| l.borrow_mut().take()).filter(|(_, _, at)| at.elapsed() < std::time::Duration::from_secs(2)).map(|(m, w, _)| (m, w))
}
