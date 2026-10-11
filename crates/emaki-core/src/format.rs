//! A file's text put in its language's standard form when it is saved
//! from the file's pane.
//!
//! Ten languages, each by a formatter that is the language's own or the
//! one its people use, at that formatter's defaults: what it does to
//! indentation, spacing and line breaks is its to decide. None is
//! written here.
//!
//! - R is Air's (`posit-dev/air`, the formatter behind Posit's VS Code
//!   extension), built in.
//! - TypeScript, JavaScript, JSON, Markdown and TOML are dprint's
//!   (`dprint/dprint`), its plugins built in as the libraries they are.
//! - Python, Rust, Go and C and C++ have one formatter each that is
//!   theirs and is a program, not a library: `ruff format`, `rustfmt`,
//!   `gofmt`, `clang-format`. It is run when the machine has it, with
//!   the text on its standard input, and a file is saved as typed when
//!   it does not.
//!
//! A file that does not parse is left as it is and the reason given.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A language Settings lists, in one of its two lists.
pub struct Lang {
    /// What `config.json` names it by (`app.format_langs`).
    pub key: &'static str,
    pub name: &'static str,
    /// Whether it is in the list of code, or of documents and data.
    pub code: bool,
    /// A file name of it, for its icon.
    pub file: &'static str,
    /// Whose formatter it is, as Settings names it.
    pub by: &'static str,
    /// The editor's names for it (`render_md::lang_for_path`).
    langs: &'static [&'static str],
}

pub const LANGS: &[Lang] = &[
    Lang { key: "r", name: "R", code: true, file: "a.R", by: "Air", langs: &["r"] },
    Lang { key: "python", name: "Python", code: true, file: "a.py", by: "Ruff", langs: &["python"] },
    Lang { key: "rust", name: "Rust", code: true, file: "a.rs", by: "rustfmt", langs: &["rust"] },
    Lang { key: "go", name: "Go", code: true, file: "a.go", by: "gofmt", langs: &["go"] },
    Lang { key: "cpp", name: "C and C++", code: true, file: "a.cpp", by: "clang-format", langs: &["c", "cpp"] },
    Lang { key: "typescript", name: "TypeScript", code: false, file: "a.ts", by: "dprint", langs: &["typescript", "tsx"] },
    Lang { key: "javascript", name: "JavaScript", code: false, file: "a.js", by: "dprint", langs: &["javascript", "jsx"] },
    Lang { key: "json", name: "JSON", code: false, file: "a.json", by: "dprint", langs: &["json"] },
    Lang { key: "markdown", name: "Markdown", code: false, file: "a.md", by: "dprint", langs: &["markdown"] },
    Lang { key: "toml", name: "TOML", code: false, file: "a.toml", by: "dprint", langs: &["toml"] },
];

/// Every language's key: what is formatted before anyone has chosen.
pub fn all_keys() -> Vec<String> {
    LANGS.iter().map(|l| l.key.to_string()).collect()
}

/// The listed language the editor's `lang` is one of.
pub fn listed(lang: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.langs.contains(&lang))
}

/// The program that formats a language that has no library here, and
/// what it is given to read the text from its standard input.
fn program(key: &str, name: &str) -> Option<(&'static str, Vec<String>)> {
    Some(match key {
        "python" => ("ruff", vec!["format".into(), "--stdin-filename".into(), name.into(), "-".into()]),
        "rust" => ("rustfmt", vec!["--edition".into(), "2021".into(), "--emit".into(), "stdout".into()]),
        "go" => ("gofmt", Vec::new()),
        "cpp" => ("clang-format", vec![format!("--assume-filename={name}")]),
        _ => return None,
    })
}

fn find_program(name: &str) -> Option<PathBuf> {
    let suffixes: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ""] } else { &[""] };
    crate::agents::search_dirs().iter().find_map(|dir| suffixes.iter().map(|s| dir.join(format!("{name}{s}"))).find(|p| p.is_file()))
}

/// The program a language's formatter is, when it is one and is not on
/// this machine: what Settings says of a language it cannot format here.
pub fn missing_program(key: &str) -> Option<&'static str> {
    let (name, _) = program(key, "")?;
    find_program(name).is_none().then_some(name)
}

/// `source`, the text of the file at `path` in the editor's `lang`, as
/// its formatter writes it: `Ok(None)` when it is already so, when the
/// language is not one of `on` (keys of `LANGS`), has no formatter, or
/// its formatter is a program this machine does not have; `Err` with
/// the reason when the text cannot be formatted (it does not parse).
pub fn format(lang: &str, path: &Path, source: &str, on: &[String]) -> Result<Option<String>, String> {
    let Some(listed) = listed(lang).filter(|l| on.iter().any(|k| k == l.key)) else { return Ok(None) };
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| listed.file.to_string());
    let formed = match listed.key {
        "r" => air(source)?,
        "typescript" | "javascript" => {
            let config = dprint_plugin_typescript::configuration::ConfigurationBuilder::new().build();
            let options = dprint_plugin_typescript::FormatTextOptions { path, extension: None, text: source.to_string(), config: &config, external_formatter: None };
            dprint_plugin_typescript::format_text(options).map_err(|e| first_line(&e.to_string()))?
        }
        "json" => {
            let config = dprint_plugin_json::configuration::ConfigurationBuilder::new().build();
            dprint_plugin_json::format_text(path, source, &config).map_err(|e| first_line(&e.to_string()))?
        }
        // Markdown's own files only. Quarto's and R Markdown's are read
        // as markdown by the editor and are more than it: a formatter
        // that does not know their blocks would rewrite them.
        "markdown" if !name.to_ascii_lowercase().ends_with(".md") => None,
        "markdown" => {
            let config = dprint_plugin_markdown::configuration::ConfigurationBuilder::new().build();
            dprint_plugin_markdown::format_text(source, &config, |_, _, _| Ok(None)).map_err(|e| first_line(&e.to_string()))?
        }
        "toml" => {
            let config = dprint_plugin_toml::configuration::ConfigurationBuilder::new().build();
            dprint_plugin_toml::format_text(path, source, &config).map_err(|e| first_line(&e.to_string()))?
        }
        key => match program(key, &name) {
            Some((tool, args)) => match find_program(tool) {
                Some(bin) => run(&bin, &args, source).map_err(|why| if why.is_empty() { format!("{tool} could not format it") } else { why })?,
                None => None,
            },
            None => None,
        },
    };
    Ok(formed.filter(|text| text != source))
}

fn air(source: &str) -> Result<Option<String>, String> {
    let parse = air_r_parser::parse(source, air_r_parser::RParserOptions::default());
    if parse.has_error() {
        return Err("it does not parse as R".to_string());
    }
    let formatted = air_r_formatter::format_node(air_r_formatter::context::RFormatOptions::default(), &parse.syntax()).map_err(|e| e.to_string())?;
    Ok(Some(formatted.print().map_err(|e| e.to_string())?.into_code()))
}

/// A formatter's reason, as one line of it.
fn first_line(why: &str) -> String {
    why.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("it does not parse").chars().take(160).collect()
}

/// How long a formatter that is a program may take over one file.
const TOOL_TIME: Duration = Duration::from_secs(8);

/// Run a formatter with `source` on its standard input and answer what
/// it wrote. One that fails says why on its standard error, and the
/// first line of that is the reason.
fn run(bin: &Path, args: &[String], source: &str) -> Result<Option<String>, String> {
    let mut child = Command::new(bin).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or_else(String::new)?;
    let text = source.to_string();
    // Written and read on threads: a formatter that writes before it has
    // read everything would otherwise stand still with its pipe full.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(text.as_bytes());
    });
    let (mut out, mut err) = (child.stdout.take().ok_or_else(String::new)?, child.stderr.take().ok_or_else(String::new)?);
    let read_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut out, &mut buf);
        buf
    });
    let read_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut err, &mut buf);
        buf
    });
    let deadline = Instant::now() + TOOL_TIME;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let _ = writer.join();
    let (out, err) = (read_out.join().unwrap_or_default(), read_err.join().unwrap_or_default());
    match status {
        Some(s) if s.success() => Ok(String::from_utf8(out).ok().filter(|t| !t.is_empty())),
        Some(_) => Err(first_line(&String::from_utf8_lossy(&err))),
        None => Err("its formatter took too long".to_string()),
    }
}
