//! A markdown file written in where it reads: the text side of it, with
//! no window.
//!
//! The pane draws a file from a text that is not always the file's own
//! (`Map`): a picture's path is made whole, a Quarto document has its
//! includes pasted in and its variables filled. A block of what is drawn
//! is found in the file through the stretches the two share, and a block
//! that is not the file's own words (an include's) is found nowhere, so
//! it is not written in.

use std::ops::Range;
use std::time::Duration;

/// A stretch the file and the shown text share: where it begins in each,
/// and how long it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    source: usize,
    shown: usize,
    len: usize,
}

/// Where the shown text's bytes are in the file, and the file's in the
/// shown text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Map {
    runs: Vec<Run>,
}

impl Map {
    pub fn new(source: &str, shown: &str) -> Self {
        if source == shown {
            return Self { runs: vec![Run { source: 0, shown: 0, len: source.len() }] };
        }
        let diff = similar::TextDiff::configure().timeout(Duration::from_millis(300)).diff_lines(source, shown);
        let starts = |lines: &[&str]| {
            let mut at = 0;
            let mut out = Vec::with_capacity(lines.len() + 1);
            for l in lines {
                out.push(at);
                at += l.len();
            }
            out.push(at);
            out
        };
        let (old, new) = (diff.old_slices(), diff.new_slices());
        let (old_at, new_at) = (starts(old), starts(new));
        let mut runs = Vec::new();
        for op in diff.ops() {
            match *op {
                similar::DiffOp::Equal { old_index, new_index, len } => runs.push(Run { source: old_at[old_index], shown: new_at[new_index], len: old_at[old_index + len] - old_at[old_index] }),
                // Lines changed one for one: each keeps what it opens
                // and closes with (a picture's path made whole is the
                // middle of its line).
                similar::DiffOp::Replace { old_index, old_len, new_index, new_len } if old_len == new_len => {
                    for i in 0..old_len {
                        let (a, b) = (old[old_index + i], new[new_index + i]);
                        let head = a.char_indices().zip(b.chars()).find(|((_, x), y)| x != y).map_or(a.len().min(b.len()), |((at, _), _)| at);
                        let head = (0..=head).rev().find(|n| a.is_char_boundary(*n) && b.is_char_boundary(*n)).unwrap_or(0);
                        if head > 0 {
                            runs.push(Run { source: old_at[old_index + i], shown: new_at[new_index + i], len: head });
                        }
                        let most = a.len().min(b.len()) - head;
                        let tail = a.bytes().rev().zip(b.bytes().rev()).take(most).take_while(|(x, y)| x == y).count();
                        let tail = (0..=tail).rev().find(|n| a.is_char_boundary(a.len() - n) && b.is_char_boundary(b.len() - n)).unwrap_or(0);
                        if tail > 0 {
                            runs.push(Run { source: old_at[old_index + i] + a.len() - tail, shown: new_at[new_index + i] + b.len() - tail, len: tail });
                        }
                    }
                }
                _ => {}
            }
        }
        Self { runs }
    }

    fn find(&self, at: usize, end: bool, key: impl Fn(&Run) -> usize, other: impl Fn(&Run) -> usize) -> Option<usize> {
        // The runs stand in order on both sides, so the one a byte is in,
        // and the one it closes, are found by halving: a rendered file
        // asks this of every piece it has, at every key.
        let inside = || self.runs.get(self.runs.partition_point(|r| key(r) + r.len <= at)).filter(|r| key(r) <= at && at < key(r) + r.len);
        let closes = || self.runs.get(self.runs.partition_point(|r| key(r) + r.len < at)).filter(|r| at == key(r) + r.len);
        let run = if end { closes().or_else(inside) } else { inside().or_else(closes) }?;
        Some(other(run) + (at - key(run)))
    }

    /// Where a byte of the shown text is in the file; `end` for the end
    /// of a stretch, which is taken as the close of what stands before
    /// it. None for a byte of words the file does not have.
    pub fn to_source(&self, shown: usize, end: bool) -> Option<usize> {
        self.find(shown, end, |r| r.shown, |r| r.source)
    }

    pub fn to_shown(&self, source: usize, end: bool) -> Option<usize> {
        self.find(source, end, |r| r.source, |r| r.shown)
    }

    /// A stretch of the shown text, in the file. None when either end is
    /// in words the file does not have.
    pub fn range_to_source(&self, shown: Range<usize>) -> Option<Range<usize>> {
        let start = self.to_source(shown.start, false)?;
        let end = self.to_source(shown.end, true)?;
        (start <= end).then_some(start..end)
    }

    pub fn range_to_shown(&self, source: Range<usize>) -> Option<Range<usize>> {
        let start = self.to_shown(source.start, false)?;
        let end = self.to_shown(source.end, true)?;
        (start <= end).then_some(start..end)
    }
}

/// Where a press on drawn words falls in the markdown they were drawn
/// from: `run` is a run of words as the view drew them, `at` the byte of
/// it under the pointer, and `raw` the block's markdown. The drawn words
/// are the markdown with its marks taken out, so the place is the one in
/// `raw` whose letters before and after are most like those around `at`,
/// and of several the one as far along.
pub fn locate(raw: &str, run: &str, at: usize) -> usize {
    // The view sets inline code between spaces of its own.
    let pad = |c: char| c == '\u{202f}';
    let at = at.min(run.len());
    let before: Vec<char> = run[..(0..=at).rev().find(|n| run.is_char_boundary(*n)).unwrap_or(0)].chars().filter(|c| !pad(*c)).collect();
    let after: Vec<char> = run[(at..=run.len()).find(|n| run.is_char_boundary(*n)).unwrap_or(run.len())..].chars().filter(|c| !pad(*c)).collect();
    if after.iter().all(|c| c.is_whitespace()) {
        return raw.trim_end().len();
    }
    const MOST: usize = 24;
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    let along = before.len() as f32 / (before.len() + after.len()).max(1) as f32;
    let mut best = (0usize, f32::MAX, 0usize);
    for ix in 0..=chars.len() {
        let back = chars[..ix].iter().rev().zip(before.iter().rev()).take(MOST).take_while(|((_, a), b)| a == *b).count();
        let fore = chars[ix..].iter().zip(after.iter()).take(MOST).take_while(|((_, a), b)| a == *b).count();
        let score = back + fore;
        let far = (ix as f32 / chars.len().max(1) as f32 - along).abs();
        if score > best.0 || (score == best.0 && far < best.1) {
            best = (score, far, chars.get(ix).map_or(raw.len(), |(at, _)| *at));
        }
    }
    best.2
}

/// The lines of a file's YAML header, between its two fences and without
/// the line end before the closing one. None as `files::front_matter` has
/// none.
pub fn header_range(source: &str) -> Option<Range<usize>> {
    crate::files::front_matter(source)?;
    let lead = source.len() - source.strip_prefix('\u{feff}').unwrap_or(source).len();
    let body = &source[lead..];
    let first = body.split_inclusive('\n').next()?;
    let mut at = first.len();
    loop {
        let line = body[at..].split_inclusive('\n').next()?;
        if matches!(line.trim_end(), "---" | "...") {
            let start = lead + first.len();
            let end = lead + at;
            return Some(start..end.saturating_sub(1).max(start));
        }
        at += line.len();
    }
}

/// What stands on a block's own line before it, from the line's start:
/// an indent, a quote's marks, a list item's marker.
pub fn lead(source: &str, at: usize) -> (usize, &str) {
    let at = at.min(source.len());
    let start = source[..at].rfind('\n').map_or(0, |n| n + 1);
    (start, &source[start..at])
}

static MARKER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"^((?:[ \t]*>)*[ \t]*)(?:([-*+])|(\d{1,9})([.)]))([ \t]+)(\[[ xX]\][ \t]+)?$").unwrap());

/// A list item's marker, when `lead` (what stands on a line before a
/// paragraph) is one: what stands before the marker, and what the next
/// item of the list opens with, its number one more and its box empty.
pub fn next_item(lead: &str) -> Option<(String, String)> {
    let c = MARKER.captures(lead)?;
    let before = c[1].to_string();
    let marker = match (c.get(2), c.get(3), c.get(4)) {
        (Some(bullet), _, _) => bullet.as_str().to_string(),
        (_, Some(n), Some(dot)) => format!("{}{}", n.as_str().parse::<u64>().ok()?.saturating_add(1), dot.as_str()),
        _ => return None,
    };
    let task = if c.get(6).is_some() { "[ ] " } else { "" };
    Some((before, format!("{marker} {task}")))
}

/// Where an item with nothing in it takes its words: the byte after its
/// marker and the space that follows (and its box), from `at`, where the
/// marker stands; and whether a space is still owed.
pub fn item_room(source: &str, at: usize) -> Option<(usize, bool)> {
    static OPEN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"^(?:[-*+]|\d{1,9}[.)])([ \t]+\[[ xX]\])?([ \t]*)").unwrap());
    let c = OPEN.captures(source.get(at..)?)?;
    Some((at + c.get(0)?.end(), c.get(2).is_none_or(|s| s.is_empty())))
}

/// What goes before and after a new paragraph put at `at`, so that a
/// blank line stands between it and its neighbors and the file still
/// ends with a line end.
pub fn paragraph_room(source: &str, at: usize) -> (&'static str, &'static str) {
    let at = at.min(source.len());
    let (head, tail) = source.split_at(at);
    let before = match head.len() - head.trim_end_matches('\n').len() {
        _ if head.trim().is_empty() => "",
        0 => "\n\n",
        1 => "\n",
        _ => "",
    };
    let after = match tail.len() - tail.trim_start_matches('\n').len() {
        _ if tail.trim().is_empty() && tail.is_empty() => "\n",
        _ if tail.trim().is_empty() => "",
        0 => "\n\n",
        1 => "\n",
        _ => "",
    };
    (before, after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_shown_as_it_is_maps_onto_itself() {
        let m = Map::new("# A\n\ntext\n", "# A\n\ntext\n");
        assert_eq!(m.range_to_source(5..9), Some(5..9));
        assert_eq!(m.to_shown(10, true), Some(10));
    }

    #[test]
    fn a_path_made_whole_moves_what_follows_it() {
        let source = "one\n\n![alt](a.png) tail\n\nlast words\n";
        let shown = "one\n\n![alt](file:///x/y/a.png) tail\n\nlast words\n";
        let m = Map::new(source, shown);
        let para = shown.find("last").unwrap()..shown.find("last").unwrap() + "last words".len();
        let back = m.range_to_source(para).unwrap();
        assert_eq!(&source[back], "last words");
        // The line holding the picture is found whole, by its ends.
        let line = shown.find("![alt]").unwrap()..shown.find(" tail").unwrap() + 5;
        assert_eq!(&source[m.range_to_source(line).unwrap()], "![alt](a.png) tail");
    }

    #[test]
    fn every_byte_is_found_where_a_walk_through_the_runs_finds_it() {
        let source = "one\n\n![a](a.png) tail\n\nmid\n\n![b](b.png)\nx ![c](c.png) y\n\n{{< include z.qmd >}}\n\nlast\n";
        let shown = "one\n\n![a](file:///p/a.png) tail\n\nmid\n\n![b](file:///p/b.png)\nx ![c](file:///p/c.png) y\n\npasted\nwords\n\nlast\n";
        let m = Map::new(source, shown);
        assert!(m.runs.len() > 4);
        let walk = |at: usize, end: bool, key: &dyn Fn(&Run) -> usize, other: &dyn Fn(&Run) -> usize| {
            let inside = |r: &&Run| key(r) <= at && at < key(r) + r.len;
            let closes = |r: &&Run| at == key(r) + r.len;
            let run = if end { m.runs.iter().find(closes).or_else(|| m.runs.iter().find(inside)) } else { m.runs.iter().find(inside).or_else(|| m.runs.iter().find(closes)) };
            run.map(|r| other(r) + (at - key(r)))
        };
        for end in [false, true] {
            for at in 0..=shown.len() {
                assert_eq!(m.to_source(at, end), walk(at, end, &|r| r.shown, &|r| r.source), "shown {at} {end}");
            }
            for at in 0..=source.len() {
                assert_eq!(m.to_shown(at, end), walk(at, end, &|r| r.source, &|r| r.shown), "source {at} {end}");
            }
        }
    }

    #[test]
    fn words_pasted_in_are_found_nowhere() {
        let source = "a\n\n{{< include x.qmd >}}\n\nb\n";
        let shown = "a\n\nfrom the other file\nmore of it\n\nb\n";
        let m = Map::new(source, shown);
        let pasted = shown.find("more").unwrap();
        assert_eq!(m.range_to_source(pasted..pasted + 4), None);
        let b = shown.rfind('b').unwrap();
        assert_eq!(&source[m.range_to_source(b..b + 1).unwrap()], "b");
    }

    #[test]
    fn a_press_on_drawn_words_is_found_past_the_marks() {
        let raw = "Some **bold** and a [link](https://and.example/a) and more";
        let run = "Some bold and a link and more";
        assert_eq!(&raw[locate(raw, run, 7)..][..2], "ld");
        // The second "and", not the one inside the address.
        let at = run.rfind("and").unwrap();
        assert_eq!(locate(raw, run, at), raw.rfind("and").unwrap());
        assert_eq!(locate(raw, run, run.len()), raw.len());
        assert_eq!(locate("## Title", "Title", 0), 3);
    }

    #[test]
    fn the_header_is_its_lines_between_the_fences() {
        let source = "---\ntitle: A\nauthor: B\n---\n\ntext\n";
        assert_eq!(&source[header_range(source).unwrap()], "title: A\nauthor: B");
        assert_eq!(header_range("text\n"), None);
    }

    #[test]
    fn the_next_item_opens_as_this_one_does() {
        assert_eq!(next_item("- "), Some((String::new(), "- ".into())));
        assert_eq!(next_item("  3. "), Some(("  ".into(), "4. ".into())));
        assert_eq!(next_item("> * [x] "), Some(("> ".into(), "* [ ] ".into())));
        assert_eq!(next_item("text "), None);
        assert_eq!(item_room("- \nnext", 0), Some((2, false)));
        assert_eq!(item_room("-\n", 0), Some((1, true)));
    }

    #[test]
    fn a_new_paragraph_gets_a_blank_line_each_side() {
        assert_eq!(paragraph_room("a\n\nb\n", 3), ("", "\n\n"));
        assert_eq!(paragraph_room("a\n", 2), ("\n", "\n"));
        assert_eq!(paragraph_room("a", 1), ("\n\n", "\n"));
        assert_eq!(paragraph_room("", 0), ("", "\n"));
    }
}
