use std::sync::Arc;

use gpui::{App, FontWeight, HighlightStyle, Hsla, Pixels, Rems, SharedString, StyleRefinement, px, rems};

use crate::{ActiveTheme as _, highlighter::HighlightTheme};

/// TextViewStyle used to customize the style for [`TextView`].
#[derive(Clone)]
pub struct TextViewStyle {
    /// Gap of each paragraphs, default is 1 rem.
    pub paragraph_gap: Rems,
    /// Base font size for headings, default is 14px.
    pub heading_base_font_size: Pixels,
    /// Function to calculate heading font size based on heading level (1-6).
    ///
    /// The first parameter is the heading level (1-6), the second parameter is the base font size.
    /// The second parameter is the base font size.
    pub heading_font_size: Option<Arc<dyn Fn(u8, Pixels) -> Pixels + Send + Sync + 'static>>,
    /// Highlight theme for code blocks. Default: [`HighlightTheme::default_light()`]
    pub highlight_theme: Arc<HighlightTheme>,
    /// The style refinement for code blocks.
    pub code_block: StyleRefinement,
    /// Style refinement applied to the table container (the bordered wrapper
    /// in wrap mode, the scroll viewport in horizontal-scroll mode).
    ///
    /// Set `overflow_x: scroll` here for adaptive table layout: columns fit
    /// their content when space allows, shrink (wrapping cell text) down to a
    /// per-column floor when the frame is narrower, and below that the table
    /// scrolls horizontally instead of squeezing further, e.g.
    /// `TextViewStyle::default().table({ let mut s = StyleRefinement::default(); s.overflow.x = Some(Overflow::Scroll); s })`.
    pub table: StyleRefinement,
    /// Style refinement applied to each table cell.
    ///
    /// With the scroll layout, set `white_space: nowrap` here to keep cells
    /// on a single line — columns then never shrink and the table scrolls as
    /// soon as the content is wider than the frame.
    pub table_cell: StyleRefinement,
    /// The highlight style for inline code.
    ///
    /// Default is [`HighlightStyle::default()`], the `background_color` will
    /// fallback to `cx.theme().accent`, if it is `None`.
    pub inline_code: HighlightStyle,
    /// The font family for inline code spans; `None` keeps the paragraph's.
    /// (Emaki: added so inline code can be set in the mono face.)
    pub inline_code_font_family: Option<SharedString>,
    /// A rounded plate behind inline code, as (fill, border); with it the
    /// flat background [`Self::inline_code`] falls back to is not drawn.
    /// (Emaki addition: a highlight's background is a square-cornered box
    /// the full height of the line.)
    pub inline_code_chip: Option<(Hsla, Hsla)>,
    /// The weight strong (bold) text is drawn at; `None` is [`FontWeight::BOLD`].
    /// (Emaki: added so strong text can be a semibold, as the Claude app sets it.)
    pub strong_font_weight: Option<FontWeight>,
    pub is_dark: bool,
}

/// The space set around an inline code span, twice at each end: the inner
/// one is the padding of the plate drawn behind the span, the outer one
/// the plate's margin. A text run has no padding of its own. It is the
/// narrow no-break space because gpui's line wrapper treats that as part
/// of a word, so the spaces stay with the span at a line's end; a
/// breaking space left the next line starting with a gap. (Emaki addition.)
pub(crate) const CODE_PAD: char = '\u{202F}';

impl PartialEq for TextViewStyle {
    fn eq(&self, other: &Self) -> bool {
        self.paragraph_gap == other.paragraph_gap
            && self.heading_base_font_size == other.heading_base_font_size
            && match (&self.heading_font_size, &other.heading_font_size) {
                (Some(left), Some(right)) => (1..=6).all(|level| {
                    left(level, self.heading_base_font_size)
                        == right(level, other.heading_base_font_size)
                }),
                (None, None) => true,
                _ => false,
            }
            && self.highlight_theme == other.highlight_theme
            && self.code_block == other.code_block
            && self.table == other.table
            && self.table_cell == other.table_cell
            && self.inline_code == other.inline_code
            && self.inline_code_font_family == other.inline_code_font_family
            && self.inline_code_chip == other.inline_code_chip
            && self.strong_font_weight == other.strong_font_weight
            && self.is_dark == other.is_dark
    }
}

impl Default for TextViewStyle {
    fn default() -> Self {
        Self {
            paragraph_gap: rems(1.),
            heading_base_font_size: px(14.),
            heading_font_size: None,
            highlight_theme: HighlightTheme::default_light().clone(),
            code_block: StyleRefinement::default(),
            table: StyleRefinement::default(),
            table_cell: StyleRefinement::default(),
            inline_code: HighlightStyle::default(),
            inline_code_font_family: None,
            inline_code_chip: None,
            strong_font_weight: None,
            is_dark: false,
        }
    }
}

impl TextViewStyle {
    /// Set paragraph gap, default is 1 rem.
    pub fn paragraph_gap(mut self, gap: Rems) -> Self {
        self.paragraph_gap = gap;
        self
    }

    pub fn heading_font_size<F>(mut self, f: F) -> Self
    where
        F: Fn(u8, Pixels) -> Pixels + Send + Sync + 'static,
    {
        self.heading_font_size = Some(Arc::new(f));
        self
    }

    /// Set style for code blocks.
    pub fn code_block(mut self, style: StyleRefinement) -> Self {
        self.code_block = style;
        self
    }

    /// Set style for inline code spans.
    pub fn inline_code(mut self, style: HighlightStyle) -> Self {
        self.inline_code = style;
        self
    }

    /// Set the font family for inline code spans. (Emaki addition.)
    pub fn inline_code_font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.inline_code_font_family = Some(family.into());
        self
    }

    /// Set the weight strong text is drawn at. (Emaki addition.)
    pub fn strong_font_weight(mut self, weight: FontWeight) -> Self {
        self.strong_font_weight = Some(weight);
        self
    }

    /// Set extra style for the table container.
    ///
    /// Set `overflow_x: scroll` on the refinement for adaptive layout: cells
    /// wrap as the frame narrows, and once columns reach their minimum width
    /// the table scrolls horizontally instead of shrinking further.
    pub fn table(mut self, style: StyleRefinement) -> Self {
        self.table = style;
        self
    }

    /// Set extra style for each table cell.
    ///
    /// With the scroll table layout, `white_space: nowrap` here keeps cells
    /// on a single line and the table scrolls whenever the content is wider
    /// than the frame.
    pub fn table_cell(mut self, style: StyleRefinement) -> Self {
        self.table_cell = style;
        self
    }

    /// Returns the [`HighlightStyle`] to use for inline code,
    /// fallback `background_color` to `cx.theme().accent`, if it is `None`.
    pub(crate) fn inline_code_highlight(&self, cx: &App) -> HighlightStyle {
        let mut style = self.inline_code;
        // The plate replaces the flat background. (Emaki addition.)
        if style.background_color.is_none() && self.inline_code_chip.is_none() {
            style.background_color = Some(cx.theme().accent);
        }
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_layout_fingerprint_covers_callback_table_and_theme_fields() {
        let base = TextViewStyle::default();
        let heading = base.clone().heading_font_size(|_, size| size);
        assert!(heading == base.clone().heading_font_size(|_, size| size));
        assert!(heading != base.clone().heading_font_size(|_, size| size * 2.));

        let mut table = StyleRefinement::default();
        table.text.white_space = Some(gpui::WhiteSpace::Nowrap);
        assert!(base != base.clone().table_cell(table));

        let mut dark = base.clone();
        dark.is_dark = true;
        assert!(base != dark);
    }

    #[test]
    fn cloning_preserves_the_same_heading_callback_fingerprint() {
        let style = TextViewStyle::default().heading_font_size(|_, size| size);
        assert!(style == style.clone());
    }
}
