//! The editor's vertical measures, in one place, from Onyx's
//! `onyx_core::rhythm`.
//!
//! A line's height goes by what kind of line it is and never by what
//! stands in it. Space belongs to the block it stands around, never to
//! its neighbor, so a word typed on a blank line moves nothing.

/// The body's line, as a share of the words' size, where the view around
/// the editor names none of its own (`Look::of`).
pub const LINE: f32 = 1.6;
/// Code's line, as a share of code's size.
pub const CODE_LINE: f32 = 1.5;
/// A table's words, as a share of the body's size, and their line.
pub const CELL: f32 = 0.875;
pub const CELL_LINE: f32 = 1.5;
/// The caret, as a share of the size of the words it stands in: the
/// words' own height, without the room between lines. It goes by the size
/// alone, so it is one height on a blank line, in Latin and in Chinese.
pub const CARET: f32 = 1.2;

/// A heading's measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heading {
    /// Its size, as a share of the words'.
    pub scale: f32,
    /// Its line, as a share of its size.
    pub line: f32,
    /// The room over it, as a share of the body's line.
    pub above: f32,
}

/// The six levels, in Craft's steps down to the words' size (its title,
/// subtitle, heading and strong text: 24, 22, 20 and 18 over 17), so a
/// heading leads its section without shouting over it; the last two are
/// the words' size, apart by weight.
pub const HEADINGS: [Heading; 6] = [
    Heading { scale: 1.412, line: 1.25, above: 0.5 },
    Heading { scale: 1.294, line: 1.25, above: 0.5 },
    Heading { scale: 1.176, line: 1.3, above: 0.4 },
    Heading { scale: 1.059, line: 1.35, above: 0.3 },
    Heading { scale: 1.0, line: 1.4, above: 0.2 },
    Heading { scale: 1.0, line: 1.5, above: 0.2 },
];

/// A table's widths, as a person set them: each column's dashes in its
/// separator row, adding up to 100. Any other separator says nothing, and
/// the columns go by their words.
pub fn shares_of(separator: &str) -> Option<Vec<f32>> {
    let row = separator.trim_start_matches(|c: char| c == '>' || c.is_whitespace()).trim_end();
    let row = row.strip_prefix('|').unwrap_or(row);
    let row = row.strip_suffix('|').unwrap_or(row);
    let dashes: Vec<u32> = row.split('|').map(|c| c.chars().filter(|c| *c == '-').count() as u32).collect();
    (dashes.len() > 1 && dashes.iter().sum::<u32>() == 100).then(|| dashes.iter().map(|d| *d as f32).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_smaller_heading_never_asks_for_more_room() {
        for pair in HEADINGS.windows(2) {
            assert!(pair[0].scale >= pair[1].scale && pair[0].above >= pair[1].above);
        }
    }
}
