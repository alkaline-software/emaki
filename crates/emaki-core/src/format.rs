//! A file's text put in its language's standard form when it is saved
//! from the file's pane.
//!
//! R is Air's (`posit-dev/air`, the formatter behind Posit's VS Code
//! extension), built in, with Air's defaults: what it does to
//! indentation, alignment, spacing and line breaks is its to decide. A
//! file that does not parse is left as it is and the reason given, as
//! Air does. No other language has a formatter here.

/// Whether saving a file of this language formats it.
pub fn formats(lang: &str) -> bool {
    lang == "r"
}

/// `source` as its formatter writes it: `Ok(None)` when it is already
/// so or the language has no formatter, `Err` with the reason when the
/// text cannot be formatted (it does not parse).
pub fn format(lang: &str, source: &str) -> Result<Option<String>, String> {
    if !formats(lang) {
        return Ok(None);
    }
    let parse = air_r_parser::parse(source, air_r_parser::RParserOptions::default());
    if parse.has_error() {
        return Err("it does not parse as R".to_string());
    }
    let formatted = air_r_formatter::format_node(air_r_formatter::context::RFormatOptions::default(), &parse.syntax()).map_err(|e| e.to_string())?;
    let code = formatted.print().map_err(|e| e.to_string())?.into_code();
    Ok((code != source).then_some(code))
}
