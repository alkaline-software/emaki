//! A table's lines changed as a whole: a row or a column in, out or
//! moved, an alignment, a sort, the columns' widths. Pure text, so what
//! it does is tested with no window; the editor makes each change one
//! step of undo (`Editor::table_do`).
//!
//! Each row keeps its cells as written, spaces and all, so a table laid
//! out by hand in the file stays as it was laid, and only what a change
//! touches changes.
//!
//! A table's widths are kept in the file, in its separator row. A
//! person's widths are written as dashes that add up to 100, each
//! column's share of the room: a legal table to every other reader, and
//! Pandoc reads the same dashes the same way.

use std::cmp::Ordering;

use super::lines::quote_line;
use super::markdown::Align;

/// A column's least share, in dashes: fewer is not a separator to some
/// readers.
const LEAST_SHARE: u32 = 3;

/// Where a row begins in its line: past a quote's marks and the indent.
pub fn body_start(line: &str) -> usize {
    let (_, at) = quote_line(line).unwrap_or((0, 0));
    at + (line[at..].len() - line[at..].trim_start().len())
}

/// A separator row's shares, when they say a person set them.
pub use super::rhythm::shares_of;

/// What stands between a line's bars, spaces and all.
fn segments(row: &str) -> Vec<String> {
    let t = row.trim_end();
    let t = t.strip_prefix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cell.push(c);
                if let Some(next) = chars.next() {
                    cell.push(next);
                }
            }
            '|' => out.push(std::mem::take(&mut cell)),
            _ => cell.push(c),
        }
    }
    if !cell.is_empty() {
        out.push(cell);
    }
    out
}

/// A table's lines, taken apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// What stands before the first bar on each line: a quote's marks, an indent.
    lead: Vec<String>,
    /// The rows, the head first, each as many cells as there are columns.
    rows: Vec<Vec<String>>,
    separator: Vec<String>,
}

impl Sheet {
    /// `lines`: the head, the separator, then the rest.
    pub fn read(lines: &[String]) -> Sheet {
        let split = |l: &String| {
            let at = body_start(l);
            (l[..at].to_string(), segments(&l[at..]))
        };
        let mut lead = Vec::new();
        let mut rows = Vec::new();
        let mut separator = Vec::new();
        for (ix, line) in lines.iter().enumerate() {
            let (before, cells) = split(line);
            if ix == 1 {
                separator = cells;
            } else {
                lead.push(before);
                rows.push(cells);
            }
        }
        let columns = separator.len().max(1);
        if separator.is_empty() {
            separator.push(" --- ".to_string());
        }
        for row in &mut rows {
            row.resize(columns, "  ".to_string());
        }
        Sheet { lead, rows, separator }
    }

    pub fn write(&self) -> Vec<String> {
        let lead = |ix: usize| self.lead.get(ix).or(self.lead.first()).map_or("", String::as_str);
        let line = |lead: &str, cells: &[String]| format!("{lead}|{}|", cells.join("|"));
        let mut out = Vec::with_capacity(self.rows.len() + 1);
        for (ix, row) in self.rows.iter().enumerate() {
            out.push(line(lead(ix), row));
            if ix == 0 {
                out.push(line(lead(0), &self.separator));
            }
        }
        out
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    pub fn columns(&self) -> usize {
        self.separator.len()
    }

    /// An empty row, standing at `at`.
    pub fn insert_row(&mut self, at: usize) {
        let at = at.min(self.rows.len());
        self.rows.insert(at, vec!["  ".to_string(); self.columns()]);
        let lead = self.lead.first().cloned().unwrap_or_default();
        self.lead.insert(at.min(self.lead.len()), lead);
    }

    pub fn duplicate_row(&mut self, at: usize) {
        if let Some(row) = self.rows.get(at).cloned() {
            self.rows.insert(at + 1, row);
            self.lead.insert((at + 1).min(self.lead.len()), self.lead.get(at).cloned().unwrap_or_default());
        }
    }

    /// False for the last row of all, which leaves no table.
    pub fn delete_row(&mut self, at: usize) -> bool {
        if self.rows.len() < 2 || at >= self.rows.len() {
            return false;
        }
        self.rows.remove(at);
        if at < self.lead.len() {
            self.lead.remove(at);
        }
        true
    }

    pub fn move_row(&mut self, from: usize, to: usize) {
        if from < self.rows.len() && to < self.rows.len() {
            let row = self.rows.remove(from);
            self.rows.insert(to, row);
        }
    }

    /// An empty column, standing at `at`. Shares make room for it.
    pub fn insert_col(&mut self, at: usize) {
        let at = at.min(self.columns());
        let shares = self.shares();
        for row in &mut self.rows {
            row.insert(at, "  ".to_string());
        }
        self.separator.insert(at, " --- ".to_string());
        if let Some(mut s) = shares {
            let fresh = s.iter().sum::<f32>() / s.len() as f32;
            s.insert(at, fresh);
            self.set_shares(&s);
        }
    }

    /// False for the last column of all, which leaves no table.
    pub fn delete_col(&mut self, at: usize) -> bool {
        if self.columns() < 2 || at >= self.columns() {
            return false;
        }
        let shares = self.shares();
        for row in &mut self.rows {
            row.remove(at);
        }
        self.separator.remove(at);
        if let Some(mut s) = shares {
            s.remove(at);
            self.set_shares(&s);
        }
        true
    }

    pub fn move_col(&mut self, from: usize, to: usize) {
        if from >= self.columns() || to >= self.columns() {
            return;
        }
        for row in &mut self.rows {
            let cell = row.remove(from);
            row.insert(to, cell);
        }
        let mark = self.separator.remove(from);
        self.separator.insert(to, mark);
    }

    pub fn align_of(&self, col: usize) -> Align {
        let Some(mark) = self.separator.get(col).map(|m| m.trim()) else { return Align::None };
        match (mark.starts_with(':'), mark.ends_with(':')) {
            (true, true) => Align::Center,
            (true, false) => Align::Left,
            (false, true) => Align::Right,
            _ => Align::None,
        }
    }

    pub fn align(&mut self, col: usize, align: Align) {
        let Some(mark) = self.separator.get_mut(col) else { return };
        let dashes = mark.chars().filter(|c| *c == '-').count().max(1);
        *mark = dressed(dashes, &align);
    }

    fn shares(&self) -> Option<Vec<f32>> {
        shares_of(&format!("|{}|", self.separator.join("|")))
    }

    /// The columns' widths, as dashes adding up to 100.
    pub fn set_shares(&mut self, widths: &[f32]) {
        let n = self.columns();
        if widths.len() != n || n == 0 {
            return;
        }
        let total: f32 = widths.iter().sum();
        if total <= 0. {
            return;
        }
        let mut dashes: Vec<u32> = widths.iter().map(|w| ((w / total * 100.).round() as u32).max(LEAST_SHARE)).collect();
        // Rounding is put right on the widest, so they add up to 100.
        while dashes.iter().sum::<u32>() != 100 {
            let widest = dashes.iter().enumerate().max_by_key(|(_, d)| **d).map_or(0, |(i, _)| i);
            if dashes.iter().sum::<u32>() > 100 {
                if dashes[widest] <= LEAST_SHARE {
                    break;
                }
                dashes[widest] -= 1;
            } else {
                dashes[widest] += 1;
            }
        }
        for (col, d) in dashes.iter().enumerate() {
            let align = self.align_of(col);
            self.separator[col] = dressed(*d as usize, &align);
        }
    }

    /// The rows under the head by a column: numbers as numbers, words
    /// whatever their marks and case, an empty cell last either way.
    pub fn sort(&mut self, col: usize, descending: bool) {
        if col >= self.columns() || self.rows.len() < 3 {
            return;
        }
        let head = self.rows.remove(0);
        let mut keyed: Vec<(String, Vec<String>)> = self.rows.drain(..).map(|r| (plain(r.get(col).map_or("", String::as_str)), r)).collect();
        keyed.sort_by(|(a, _), (b, _)| match (a.is_empty(), b.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => {
                let by = match (number(a), number(b)) {
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    _ => a.to_lowercase().cmp(&b.to_lowercase()),
                };
                if descending { by.reverse() } else { by }
            }
        });
        self.rows.push(head);
        self.rows.extend(keyed.into_iter().map(|(_, r)| r));
    }
}

/// A separator's cell: its dashes, with its colons.
fn dressed(dashes: usize, align: &Align) -> String {
    let dashes = "-".repeat(dashes);
    match align {
        Align::Center => format!(" :{dashes}: "),
        Align::Left => format!(" :{dashes} "),
        Align::Right => format!(" {dashes}: "),
        Align::None => format!(" {dashes} "),
    }
}

/// A cell's words with their marks away, for sorting: a link's words,
/// no stars, no tags.
fn plain(cell: &str) -> String {
    let mut out = String::new();
    let mut chars = cell.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' | '~' | '=' | '[' => {}
            ']' => {
                if chars.peek() == Some(&'(') {
                    for inner in chars.by_ref() {
                        if inner == ')' {
                            break;
                        }
                    }
                }
            }
            '<' => {
                for inner in chars.by_ref() {
                    if inner == '>' {
                        break;
                    }
                }
            }
            '\\' => out.extend(chars.next()),
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// The number a cell opens with, past a currency's sign, with its thousands.
fn number(cell: &str) -> Option<f64> {
    let t = cell.trim_start_matches(['$', '€', '£', '¥', '+']).replace(',', "");
    let end = t.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-'))).unwrap_or(t.len());
    let rest = t[end..].trim();
    if end == 0 || !(rest.is_empty() || rest == "%") {
        return None;
    }
    t[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(text: &str) -> Sheet {
        Sheet::read(&text.lines().map(str::to_string).collect::<Vec<_>>())
    }

    fn text(s: &Sheet) -> String {
        s.write().join("\n")
    }

    const T: &str = "| a | b |\n|---|:-:|\n| 1 | 2 |\n| 3 | 4 |";

    #[test]
    fn a_table_read_and_written_is_the_same() {
        assert_eq!(text(&sheet(T)), T);
        let wide = "| 名字   | b |\n|--------|---|\n| 费城   | 2 |";
        assert_eq!(text(&sheet(wide)), wide);
        assert_eq!(text(&sheet("> | a |\n> |---|\n> | 1 |")), "> | a |\n> |---|\n> | 1 |");
    }

    #[test]
    fn a_row_with_too_few_cells_is_filled() {
        assert_eq!(text(&sheet("| a | b |\n|---|---|\n| 1 |")), "| a | b |\n|---|---|\n| 1 |  |");
        assert_eq!(text(&sheet("a | b\n---|---\n1 | 2")), "|a | b|\n|---|---|\n|1 | 2|");
    }

    #[test]
    fn rows_go_in_out_and_move() {
        let mut s = sheet(T);
        s.insert_row(2);
        assert_eq!(text(&s), "| a | b |\n|---|:-:|\n| 1 | 2 |\n|  |  |\n| 3 | 4 |");
        assert!(s.delete_row(2));
        assert_eq!(text(&s), T);
        s.move_row(2, 1);
        assert_eq!(text(&s), "| a | b |\n|---|:-:|\n| 3 | 4 |\n| 1 | 2 |");
        s.duplicate_row(1);
        assert_eq!(s.rows(), 4);
        let mut one = sheet("| a |\n|---|");
        assert!(!one.delete_row(0));
    }

    #[test]
    fn columns_go_in_out_and_move() {
        let mut s = sheet(T);
        s.insert_col(1);
        assert_eq!(text(&s), "| a |  | b |\n|---| --- |:-:|\n| 1 |  | 2 |\n| 3 |  | 4 |");
        assert!(s.delete_col(1));
        assert_eq!(text(&s), T);
        s.move_col(1, 0);
        assert_eq!(text(&s), "| b | a |\n|:-:|---|\n| 2 | 1 |\n| 4 | 3 |");
        let mut one = sheet("| a |\n|---|\n| 1 |");
        assert!(!one.delete_col(0));
    }

    #[test]
    fn an_alignment_is_the_separators() {
        let mut s = sheet(T);
        assert_eq!((s.align_of(0), s.align_of(1)), (Align::None, Align::Center));
        s.align(0, Align::Right);
        s.align(1, Align::None);
        assert_eq!(text(&s).lines().nth(1), Some("| ---: | - |"));
        assert_eq!(s.align_of(0), Align::Right);
    }

    #[test]
    fn a_persons_widths_are_dashes_adding_to_a_hundred() {
        let mut s = sheet(T);
        s.align(1, Align::Center);
        s.set_shares(&[300., 100.]);
        let sep = s.write()[1].clone();
        assert_eq!(sep.chars().filter(|c| *c == '-').count(), 100);
        assert_eq!(s.align_of(1), Align::Center);
        assert_eq!(shares_of(&sep), Some(vec![75., 25.]));
        // Three columns at a third each still add up.
        let mut t = sheet("| a | b | c |\n|---|---|---|");
        t.set_shares(&[1., 1., 1.]);
        assert_eq!(t.write()[1].chars().filter(|c| *c == '-').count(), 100);
        // A column that comes makes room; the rest keep their proportion.
        s.insert_col(1);
        assert_eq!(s.write()[1].chars().filter(|c| *c == '-').count(), 100);
        assert_eq!(shares_of("|---|---|"), None);
        assert_eq!(shares_of("| a |"), None);
    }

    #[test]
    fn a_sort_goes_by_what_a_cell_says() {
        let mut s = sheet("| n |\n|---|\n| **10** |\n|  |\n| 9 |\n| [b](http://x) |\n| A |");
        s.sort(0, false);
        assert_eq!(text(&s), "| n |\n|---|\n| 9 |\n| **10** |\n| A |\n| [b](http://x) |\n|  |");
        s.sort(0, true);
        assert_eq!(text(&s), "| n |\n|---|\n| [b](http://x) |\n| A |\n| **10** |\n| 9 |\n|  |");
    }
}
