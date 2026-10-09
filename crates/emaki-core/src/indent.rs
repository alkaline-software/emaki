//! Where a new line starts in the file's editor, and what one step of
//! indent is.
//!
//! The toolkit's editor gives a new line the indent of the line above
//! it. That is right inside a block and wrong at its first line: after
//! Python's `:` or an opening bracket the next line is a step deeper,
//! and Python reads the indent as the program. `after` is that rule, by
//! the words of the line and no parser: it has to answer at a keystroke
//! on a file that does not parse yet.

/// The indent of the line that Enter starts, with the caret at the end
/// of `before` (the line up to the caret). `above` is the last line
/// with words over that one, `unit` one step of indent.
pub fn after(lang: &str, before: &str, above: &str, unit: &str) -> String {
    let base: String = before.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let code = code_of(lang, before);
    let deeper = || format!("{base}{unit}");
    let shallower = || base.strip_suffix(unit).unwrap_or(&base).to_string();
    if code.is_empty() {
        return base;
    }
    if code.ends_with(['{', '[', '(']) {
        return deeper();
    }
    match lang {
        "python" => {
            if code.ends_with(':') {
                return deeper();
            }
            // A line that ends its block: the next one is the block's
            // parent's.
            let first = code.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
            if matches!(first, "return" | "pass" | "break" | "continue" | "raise") {
                return shallower();
            }
        }
        "yaml" if code.ends_with(':') => return deeper(),
        // A pipe, a `+` of ggplot's or an assignment left open goes on
        // on the next line, a step in; the lines after it stay there.
        "r" if continues(code) => return if continues(code_of(lang, above)) { base } else { deeper() },
        _ => {}
    }
    base
}

/// Whether a line of R is left open at its end: an operator with
/// nothing after it (`%>%` and its kind end in `%`).
fn continues(code: &str) -> bool {
    ["|>", "<-", "+", "-", "*", "/", "=", ",", "&", "|", "~"].iter().any(|op| code.ends_with(op)) || code.ends_with('%')
}

/// The line without what is at its left and right: the indent, and a
/// comment at its end when the line has no quote to hide one in.
fn code_of<'a>(lang: &str, line: &'a str) -> &'a str {
    let line = line.trim();
    let mark = match lang {
        "python" | "r" | "yaml" | "bash" | "toml" | "ruby" => "#",
        _ => "//",
    };
    match line.find(mark) {
        Some(at) if !line[..at].contains(['"', '\'', '`']) => line[..at].trim_end(),
        _ => line,
    }
}

/// One step of indent for a file: how many columns, and whether it is
/// a tab. The file's own way when it has one (the smallest indent any
/// of its lines starts with), else the language's habit.
pub fn unit(lang: &str, text: &str) -> (usize, bool) {
    let mut least: Option<usize> = None;
    for line in text.lines().take(2000) {
        if line.starts_with('\t') {
            return (4, true);
        }
        let spaces = line.len() - line.trim_start_matches(' ').len();
        // One space is an alignment or a comment's, not a step.
        if spaces >= 2 && !line.trim().is_empty() {
            least = Some(least.map_or(spaces, |n| n.min(spaces)));
        }
    }
    match (least, lang) {
        (Some(n), _) if n <= 8 => (n, false),
        (_, "go" | "makefile") => (4, true),
        (_, "python" | "rust" | "c" | "cpp" | "java" | "kotlin" | "swift" | "csharp" | "php") => (4, false),
        _ => (2, false),
    }
}
