//! Math drawn as the caller typesets it (Quarto's `$…$` and `$$…$$`).
//! The view has no typesetter of its own: a [`MathFace`] given to the
//! markdown extensions sets each formula as an SVG while the text is
//! parsed, off the window's thread.
//!
//! - A formula stays a piece of its paragraph's text. It holds its width
//!   with no-break spaces set in the caller's math face, whose widths the
//!   face states, so it wraps as one word and selects, finds and copies
//!   with the words around it; the picture is painted over that room, on
//!   its line's baseline, in the color of the words.
//! - A formula on a line of its own is a line of its own whose height is
//!   the picture's, centered, its `\tag{n}` drawn as `(n)` at the right.
//! - Copied, a formula is its source between dollars.
//! - Without a face, or where the face cannot set a formula, it is drawn
//!   as Unicode in italics (`quarto::tex`).
//!
//! (Emaki addition.)

use std::sync::Arc;

use gpui::SharedString;

/// A formula set by the caller: an SVG, drawn as one color, and its size
/// and baseline (from its top) in ems of the text it stands in.
#[derive(Clone)]
pub struct MathPicture {
    pub svg: Arc<[u8]>,
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

impl PartialEq for MathPicture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.svg, &other.svg) && self.width == other.width && self.height == other.height && self.baseline == other.baseline
    }
}

impl std::fmt::Debug for MathPicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MathPicture({}×{} em)", self.width, self.height)
    }
}

/// What sets a formula: given its source and whether it stands on a line
/// of its own, its picture, or None.
pub type MathSetFn = dyn Fn(&str, bool) -> Option<MathPicture> + Send + Sync;

/// The caller's typesetter, and the face the room a formula takes is set
/// in: its family, and the widths in ems of its no-break space (U+00A0)
/// and narrow no-break space (U+202F), the two spaces gpui never breaks a
/// line at.
#[derive(Clone)]
pub struct MathFace {
    pub family: SharedString,
    pub glue: (f32, f32),
    pub set: Arc<MathSetFn>,
}

/// A formula in a paragraph: what it was written as, and how it is drawn.
#[derive(Debug, Clone)]
pub(crate) struct MathSource {
    pub(crate) src: SharedString,
    /// On a line of its own.
    pub(crate) display: bool,
    /// The number a display formula carries (`\tag{n}`).
    pub(crate) tag: Option<SharedString>,
    /// Set by the face; None, it is its words.
    pub(crate) picture: Option<MathPicture>,
}

impl PartialEq for MathSource {
    fn eq(&self, other: &Self) -> bool {
        self.src == other.src && self.display == other.display && self.tag == other.tag && self.picture.is_some() == other.picture.is_some()
    }
}

impl MathSource {
    /// The formula as written, for a copy.
    pub(crate) fn markdown(&self) -> String {
        if self.display { format!("$$\n{}\n$$", self.src) } else { format!("${}$", self.src) }
    }
}

/// The room for a picture `width` ems wide: the no-break spaces whose
/// widths come nearest it, at least one.
pub(crate) fn room(width: f32, glue: (f32, f32)) -> String {
    let (wide, narrow) = glue;
    if wide <= 0. || narrow <= 0. {
        return "\u{A0}".into();
    }
    let mut best = (f32::MAX, 0usize, 1usize);
    for a in 0..=((width / wide).ceil() as usize + 1) {
        let rest = width - a as f32 * wide;
        // At least one space: a picture narrower than the narrow one
        // still takes that.
        let b = ((rest / narrow).round().max(0.) as usize).max(usize::from(a == 0));
        let miss = (a as f32 * wide + b as f32 * narrow - width).abs();
        if miss < best.0 {
            best = (miss, a, b);
        }
    }
    let mut out = "\u{A0}".repeat(best.1);
    out.push_str(&"\u{202F}".repeat(best.2));
    out
}

/// `\tag{n}` taken out of a display formula: the rest, and `n`.
pub(crate) fn untag(src: &str) -> (String, Option<SharedString>) {
    let Some(at) = src.find("\\tag") else { return (src.trim().to_string(), None) };
    let after = src[at + 4..].trim_start_matches('*');
    let Some(body) = after.strip_prefix('{') else { return (src.trim().to_string(), None) };
    let Some(end) = body.find('}') else { return (src.trim().to_string(), None) };
    let tag = body[..end].trim().to_string();
    let rest = format!("{}{}", &src[..at], &body[end + 1..]);
    (rest.trim().to_string(), Some(tag.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_room_is_as_wide_as_the_picture_within_a_step() {
        let glue = (0.332, 0.2);
        for width in [0.05_f32, 0.57, 1.0, 2.3, 7.81] {
            let room = room(width, glue);
            let wide = room.matches('\u{A0}').count() as f32 * glue.0 + room.matches('\u{202F}').count() as f32 * glue.1;
            assert!((wide - width).abs() <= 0.16, "{width}: {wide}");
            assert!(!room.is_empty());
        }
        let room = room(1.0, glue);
        let wide = room.matches('\u{A0}').count() as f32 * glue.0 + room.matches('\u{202F}').count() as f32 * glue.1;
        assert!((wide - 1.0).abs() < 0.04, "{wide}");
    }

    #[test]
    fn a_tag_comes_out_of_the_formula() {
        assert_eq!(untag("a = b \\tag{2}"), ("a = b".to_string(), Some("2".into())));
        assert_eq!(untag("\\tag*{A} x"), ("x".to_string(), Some("A".into())));
        assert_eq!(untag("x^2"), ("x^2".to_string(), None));
    }
}
