//! A LaTeX document read for showing: written as the Quarto markdown the
//! file's pane reads, in the window's own look and not the PDF's.
//! `quarto::expand` and the markdown view do the rest (the numbers, the
//! citations, the math). What becomes what:
//!
//! - `\input`, `\include` and `\import` paste the file in before anything
//!   else, as TeX does: from the document's folder, `.tex` added when the
//!   name lacks it, at most 8 deep, never round a cycle. A file not there
//!   is left out. A file with a preamble of its own gives its body.
//! - A comment (`%` to the line's end) goes, and a line ending in one
//!   joins the next with no space. Code environments (`verbatim`,
//!   `lstlisting`, `minted`, `Verbatim`) and `\verb` are put aside first,
//!   so nothing in them is read as TeX.
//! - Macros (`\newcommand`, `\renewcommand`, `\providecommand`, `\def`,
//!   `\let`, `\DeclareMathOperator`, `\NewDocumentCommand`,
//!   `\newenvironment`) are expanded where they are used, in text and math
//!   alike, at most 8 levels deep, so a recursive one stops.
//! - The preamble writes nothing. `\title`, `\author` (split at `\and`,
//!   each name up to its first `\\`, `\thanks` dropped), `\date` (`\today`
//!   is today), the bibliography and whether sections are numbered go in
//!   the YAML header. `\maketitle` writes nothing: the header is drawn as
//!   a band.
//! - `\section` is `#`, `\subsection` `##`, and so on; with `\chapter` or
//!   `\part` in the document, `\chapter` is `#`. A starred one is
//!   `{.unnumbered}`, a label after one its `#sec-` id. `\paragraph{T}` is
//!   `**T.**` run into the paragraph that follows.
//! - A paragraph is one line; `\\` is a hard break. `\emph` is `*…*`,
//!   `\textbf` `**…**`, `\texttt` and `\verb` code, `\sout` `~~…~~`,
//!   `\underline` `<u>…</u>`, `\hl` `<mark>…</mark>`, `\href` and `\url`
//!   links. Characters markdown reads as syntax are escaped. `~` is a
//!   no-break space, `--` and `---` dashes, quotes curly, and accents and
//!   named symbols their Unicode characters.
//! - Lists are markdown's; `quote`, `quotation` and `verse` blockquotes;
//!   `center` a centered div; `abstract` a `.abstract` div; code fenced,
//!   with its language.
//! - Math stays LaTeX: `$…$` in a line, a display between lines of `$$`,
//!   an `align` as `aligned`. A numbered display closes with
//!   `$$ {#eq-…}`; `\nonumber` and `\notag` take the number off.
//! - A figure is `![caption](path){#fig-… width=…}`, several pictures a
//!   `layout-ncol` div with the caption under it; a table a pipe table and
//!   its caption line `: caption {#tbl-…}`; a theorem a `#thm-…` div, one
//!   of the other declared kinds a `.remark` div, a proof a `.proof` div
//!   ending in ∎.
//! - A label becomes a Quarto id by what it is on (`fig:x` on a figure is
//!   `fig-x`, `intro` on a section `sec-intro`), and `\ref`, `\cref` and
//!   `\eqref` the references Quarto reads; `\nameref` is the section's
//!   title, as LaTeX writes it. A label on nothing reads `**??**`, as
//!   LaTeX's own.
//! - `\cite` and natbib's and biblatex's forms are Pandoc citations;
//!   `\bibliography` and `\printbibliography` the `#refs` div. A
//!   `thebibliography` is a numbered list, and a citation of one of its
//!   entries is its number.
//! - Footnotes are markdown's, defined at the document's end.
//! - Anything else: a command keeps the words of its last braced
//!   argument, an environment its content as it is. What draws a picture
//!   (TikZ) is left out.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

/// The most files nested in one another that are followed, and the most
/// levels a macro is expanded inside another.
const DEPTH: usize = 8;

/// Characters of Unicode's private use that mark, while the document is
/// converted, where a reference (resolved once the whole document is read)
/// and a piece of code put aside stand. Taken out of the input first.
const REF_OPEN: char = '\u{E000}';
const REF_CLOSE: char = '\u{E001}';
const VERB_OPEN: char = '\u{E002}';
const VERB_CLOSE: char = '\u{E003}';

/// `text`, the LaTeX document at `file`, as Quarto markdown.
pub fn to_markdown(text: &str, file: &Path) -> String {
    crate::on_deep_stack(|| read(text, file))
}

fn read(text: &str, file: &Path) -> String {
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut verbs = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(real(file));
    let full = load(&clean(text), &dir, &dir, 0, &mut seen, &mut verbs);
    let (full, defs, meta) = collect(&full);
    let body = expand(body_of(&full), &defs);
    Conv::new(dir, verbs, defs, meta, &body).document(&body)
}

/// Line ends as `\n`, and none of the marking characters.
fn clean(text: &str) -> String {
    text.replace("\r\n", "\n").chars().filter(|c| !matches!(c, '\u{E000}'..='\u{E003}')).collect()
}

fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// What stands between `\begin{document}` and `\end{document}`: all of a
/// file that has neither.
fn body_of(text: &str) -> &str {
    const BEGIN: &str = "\\begin{document}";
    let start = text.find(BEGIN).map_or(0, |k| k + BEGIN.len());
    let end = text[start..].find("\\end{document}").map_or(text.len(), |k| start + k);
    &text[start..end]
}

// ---------------------------------------------------------------------
// Loading: comments, code put aside, files pasted in.

/// A piece of code put aside: kept byte for byte.
#[derive(Clone, Debug)]
struct Verb {
    block: bool,
    env: String,
    lang: String,
    body: String,
    caption: Option<String>,
}

impl Verb {
    fn inline(body: &str) -> Verb {
        Verb { block: false, env: String::new(), lang: String::new(), body: body.to_string(), caption: None }
    }

    /// A code environment's content, its options read off its start. The
    /// line `\begin` stands on and the one `\end` stands on are not code.
    fn block(env: &str, raw: &str) -> Verb {
        let mut rest = raw;
        let mut lang = String::new();
        let mut caption = None;
        if env == "lstlisting" {
            if let Some((o, e)) = opt_here(rest, 0) {
                lang = option(o, "language").unwrap_or_default();
                caption = option(o, "caption");
                rest = &rest[e..];
            }
        } else if env == "minted" {
            if let Some((_, e)) = opt_here(rest, 0) {
                rest = &rest[e..];
            }
            if let Some((l, e)) = group_at(rest, 0) {
                lang = l.to_string();
                rest = &rest[e..];
            }
        } else if env.contains("Verbatim") {
            if let Some((_, e)) = opt_here(rest, 0) {
                rest = &rest[e..];
            }
        }
        let rest = match rest.find('\n') {
            Some(k) if rest[..k].trim().is_empty() => &rest[k + 1..],
            _ => rest,
        };
        Verb { block: true, env: env.to_string(), lang, body: rest.trim_end().to_string(), caption }
    }
}

/// Environments whose content is code.
const VERBATIM: &[&str] = &["verbatim", "verbatim*", "Verbatim", "Verbatim*", "BVerbatim", "LVerbatim", "lstlisting", "minted"];
/// Environments that write nothing at all.
const SILENT: &[&str] = &["comment", "filecontents", "filecontents*"];

/// `text` with its comments gone, its code put aside in `verbs`, and the
/// files it inputs pasted in, theirs likewise. `root` is the document's
/// folder, `here` the folder of the file being read.
fn load(text: &str, root: &Path, here: &Path, depth: usize, seen: &mut HashSet<PathBuf>, verbs: &mut Vec<Verb>) -> String {
    static INPUT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\\(input|include|subfile)\s*\{([^{}]*)\}|\\input[ \t]+([^\s{}\\%]+)|\\(?:sub)?(?:import|inputfrom|includefrom)\*?\s*\{([^{}]*)\}\s*\{([^{}]*)\}").unwrap()
    });
    let text = protect(text, verbs);
    INPUT
        .replace_all(&text, |c: &regex::Captures| {
            let (name, base, include) = match (c.get(2), c.get(3)) {
                (Some(n), _) => (n.as_str(), None, &c[1] == "include"),
                (None, Some(n)) => (n.as_str(), None, false),
                _ => (c.get(5).map_or("", |m| m.as_str()), c.get(4).map(|m| m.as_str()), false),
            };
            if depth >= DEPTH {
                return String::new();
            }
            let Some(path) = find_input(name, base, root, here) else { return String::new() };
            let key = real(&path);
            if !seen.insert(key.clone()) {
                return String::new();
            }
            let inner = std::fs::read_to_string(&path).map(|t| clean(&t)).unwrap_or_default();
            let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let loaded = load(&inner, root, &folder, depth + 1, seen, verbs);
            seen.remove(&key);
            let loaded = body_of(&loaded);
            // `\include` starts a page: a paragraph of its own.
            if include {
                format!("\n\n{loaded}\n\n")
            } else {
                loaded.to_string()
            }
        })
        .into_owned()
}

/// The file `\input{name}` means: `name.tex`, else `name`, from the
/// document's folder, else from the folder of the file that inputs it.
fn find_input(name: &str, base: Option<&str>, root: &Path, here: &Path) -> Option<PathBuf> {
    let name = name.trim().trim_matches('"');
    if name.is_empty() {
        return None;
    }
    let rel = match base {
        Some(b) => Path::new(b.trim()).join(name),
        None => PathBuf::from(name),
    };
    let mut names = Vec::new();
    if !name.ends_with(".tex") {
        names.push(PathBuf::from(format!("{}.tex", rel.display())));
    }
    names.push(rel);
    for dir in [root, here] {
        for n in &names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// `text` with its comments taken out and its code put aside, each piece
/// marked by its number in `verbs`.
fn protect(text: &str, verbs: &mut Vec<Verb>) -> String {
    protect_in(text, verbs, 0)
}

fn protect_in(text: &str, verbs: &mut Vec<Verb>, depth: usize) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                let (name, end) = name_at(text, i + 1);
                // `\iffalse … \fi` is a comment; an `\else` branch stays.
                if (name == "iffalse" || name == "iftrue") && depth < 64 {
                    let (first, second, after) = conditional(text, end);
                    let kept = if name == "iftrue" { first } else { second };
                    out.push_str(&protect_in(kept, verbs, depth + 1));
                    // As after any control word, the line's end after `\fi`
                    // is no space.
                    i = after + text[after..].len() - text[after..].trim_start_matches([' ', '\t']).len();
                    if text[i..].starts_with('\n') {
                        i += 1;
                    }
                    continue;
                }
                if let Some((verb, after)) = verbatim_at(text, name, end) {
                    if let Some(v) = verb {
                        out.push(VERB_OPEN);
                        out.push_str(&verbs.len().to_string());
                        out.push(VERB_CLOSE);
                        verbs.push(v);
                    }
                    i = after;
                    continue;
                }
                // A link's address keeps its `%` and `#`.
                if matches!(name, "url" | "href") {
                    if let Some((_, e)) = group_at(text, end) {
                        out.push_str(&text[i..e]);
                        i = e;
                        continue;
                    }
                }
                // Copied whole, so `\%` is no comment and `\\%` is one.
                out.push_str(&text[i..end]);
                i = end;
            }
            b'%' => i = comment(text, i, &mut out),
            _ => {
                let next = text[i..].find(['\\', '%']).map_or(text.len(), |k| i + k);
                out.push_str(&text[i..next]);
                i = next;
            }
        }
    }
    out
}

/// The code a control sequence `\name` (ending at `end`) opens, and where
/// it ends: None when it opens none. A silent environment gives no code.
fn verbatim_at(text: &str, name: &str, end: usize) -> Option<(Option<Verb>, usize)> {
    let b = text.as_bytes();
    // `\verb|x|`: the same character closes it, on the same line.
    let delimited = |k: usize| -> Option<(Option<Verb>, usize)> {
        let d = text[k..].chars().next().filter(|d| !d.is_alphanumeric() && !d.is_whitespace())?;
        let open = k + d.len_utf8();
        let line = text[open..].find('\n').map_or(text.len(), |x| open + x);
        let close = text[open..line].find(d).map(|x| open + x)?;
        Some((Some(Verb::inline(&text[open..close])), close + d.len_utf8()))
    };
    match name {
        "begin" => {
            let (env, start) = group_at(text, end)?;
            let env = env.trim();
            let silent = SILENT.contains(&env);
            if !silent && !VERBATIM.contains(&env) {
                return None;
            }
            let close = format!("\\end{{{env}}}");
            let stop = text[start..].find(&close).map_or(text.len(), |k| start + k);
            let after = (stop + close.len()).min(text.len());
            if silent {
                return Some((None, after));
            }
            Some((Some(Verb::block(env, &text[start..stop])), after))
        }
        "verb" => delimited(if b.get(end) == Some(&b'*') { end + 1 } else { end }),
        "lstinline" | "mintinline" => {
            let mut k = end;
            if let Some((_, e)) = opt_here(text, k) {
                k = e;
            }
            if name == "mintinline" {
                k = group_at(text, k)?.1;
            }
            if b.get(k) == Some(&b'{') {
                let (body, e) = group_at(text, k)?;
                return Some((Some(Verb::inline(body)), e));
            }
            delimited(k)
        }
        _ => None,
    }
}

/// The two branches of a conditional whose test ends at `end`, the one
/// before `\else` and the one after it, and where its `\fi` ends. Inner
/// conditionals are counted; one not closed runs to the end.
fn conditional(text: &str, end: usize) -> (&str, &str, usize) {
    let mut depth = 1;
    let mut split = None;
    let mut i = end;
    while let Some(off) = text[i..].find('\\') {
        let at = i + off;
        let (name, e) = name_at(text, at + 1);
        if name == "fi" {
            depth -= 1;
            if depth == 0 {
                return match split {
                    Some((a, b)) => (&text[end..a], &text[b..at], e),
                    None => (&text[end..at], "", e),
                };
            }
        } else if name == "else" && depth == 1 && split.is_none() {
            split = Some((at, e));
        } else if name.starts_with("if") && name.bytes().all(|c| c.is_ascii_alphabetic()) {
            depth += 1;
        }
        i = e;
    }
    match split {
        Some((a, b)) => (&text[end..a], &text[b..], text.len()),
        None => (&text[end..], "", text.len()),
    }
}

/// A comment at `i` taken out as TeX takes it: with the line's end, and
/// the next line's leading spaces, so the two lines join. A line holding
/// only a comment goes whole; a blank line after one stays, so a
/// paragraph still ends there. Where to read on.
fn comment(text: &str, i: usize, out: &mut String) -> usize {
    let line_start = out.rfind('\n').map_or(0, |k| k + 1);
    if out[line_start..].trim().is_empty() {
        out.truncate(line_start);
    }
    let Some(eol) = text[i..].find('\n').map(|k| i + k) else { return text.len() };
    let next = eol + 1;
    let next_end = text[next..].find('\n').map_or(text.len(), |k| next + k);
    if text[next..next_end].trim().is_empty() {
        return eol;
    }
    let b = text.as_bytes();
    let mut k = next;
    while k < b.len() && matches!(b[k], b' ' | b'\t') {
        k += 1;
    }
    k
}

// ---------------------------------------------------------------------
// The preamble: macros, theorems, the title's pieces.

/// A macro: how many arguments it takes, the first one's default when
/// that one is optional, and what it stands for.
#[derive(Clone, Debug)]
struct Macro {
    args: usize,
    default: Option<String>,
    body: String,
}

#[derive(Default)]
struct Defs {
    macros: HashMap<String, Macro>,
    /// An environment of the document's: what `\begin` stands for (with
    /// its arguments), and what `\end` does.
    envs: HashMap<String, (Macro, String)>,
}

/// What the header and the conversion take from the whole document.
#[derive(Default)]
struct Meta {
    title: Option<String>,
    subtitle: Option<String>,
    authors: Vec<String>,
    date: Option<String>,
    bib: Option<String>,
    graphics: Vec<String>,
    lst_lang: Option<String>,
    secnumdepth: Option<i32>,
    /// A declared theorem-like environment: its word, and whether it is
    /// numbered.
    theorems: HashMap<String, (String, bool)>,
}

/// Commands the document may redefine that are read here as LaTeX's own,
/// never expanded: expanding them would leave their low-level insides.
const KEPT: &[&str] = &[
    "maketitle", "section", "subsection", "subsubsection", "chapter", "part", "paragraph", "subparagraph", "caption", "label", "ref", "eqref", "cite", "footnote", "item", "begin", "end", "emph", "textbf", "textit", "texttt",
    "includegraphics", "tableofcontents", "title", "author", "date", "thanks", "href", "url", "bibliography", "printbibliography", "and", "today", "par", "newline",
];
const KEPT_ENVS: &[&str] = &["document", "figure", "table", "itemize", "enumerate", "description", "abstract", "proof", "equation", "align", "verbatim", "thebibliography", "center", "quote", "tabular"];

/// The document with its definitions and title pieces taken out, and what
/// they define.
fn collect(text: &str) -> (String, Defs, Meta) {
    let mut defs = Defs::default();
    let mut meta = Meta::default();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(off) = text[i..].find('\\') {
        let at = i + off;
        out.push_str(&text[i..at]);
        let (name, end) = name_at(text, at + 1);
        match definition(text, name, end, &mut defs, &mut meta) {
            Some(after) => i = after,
            None => {
                out.push_str(&text[at..end]);
                i = end;
            }
        }
    }
    out.push_str(&text[i..]);
    (out, defs, meta)
}

fn define(defs: &mut Defs, name: String, m: Macro) {
    if !name.is_empty() && !name.contains('@') && !KEPT.contains(&name.as_str()) {
        defs.macros.insert(name, m);
    }
}

/// `\name` followed by a definition or a piece of the title from `end`
/// on: recorded, and where it ends. None when it is neither, so it stays
/// in the text.
fn definition(text: &str, name: &str, end: usize, defs: &mut Defs, meta: &mut Meta) -> Option<usize> {
    let b = text.as_bytes();
    let star = b.get(end) == Some(&b'*');
    let j = if star { end + 1 } else { end };
    match name {
        "newcommand" | "renewcommand" | "providecommand" | "DeclareRobustCommand" => {
            let (raw, mut k) = arg_at(text, j)?;
            let key = raw.trim().strip_prefix('\\')?.trim().to_string();
            let mut args = 0;
            let mut default = None;
            if let Some((n, e)) = opt_at(text, k) {
                args = n.trim().parse().ok()?;
                k = e;
            }
            if let Some((d, e)) = opt_at(text, k) {
                default = Some(d.to_string());
                k = e;
            }
            let (body, e) = arg_at(text, k)?;
            if !(name == "providecommand" && defs.macros.contains_key(&key)) {
                define(defs, key, Macro { args, default, body: body.to_string() });
            }
            Some(e)
        }
        "def" | "gdef" | "edef" | "xdef" => {
            let (key, k) = control_word(text, j)?;
            let open = text[k..].find('{').map(|x| k + x)?;
            let params = &text[k..open];
            let (body, e) = group_at(text, open)?;
            define(defs, key, Macro { args: params.matches('#').count(), default: None, body: body.to_string() });
            Some(e)
        }
        "let" => {
            let (key, k) = control_word(text, j)?;
            let mut k = skip_space(text, k);
            if b.get(k) == Some(&b'=') {
                k = skip_space(text, k + 1);
            }
            let (target, e) = arg_at(text, k)?;
            define(defs, key, Macro { args: 0, default: None, body: target.to_string() });
            Some(e)
        }
        "DeclareMathOperator" => {
            let (raw, k) = arg_at(text, j)?;
            let key = raw.trim().strip_prefix('\\')?.to_string();
            let (body, e) = arg_at(text, k)?;
            let op = if star { "\\operatorname*" } else { "\\operatorname" };
            define(defs, key, Macro { args: 0, default: None, body: format!("{op}{{{body}}}") });
            Some(e)
        }
        "NewDocumentCommand" | "RenewDocumentCommand" | "ProvideDocumentCommand" | "DeclareDocumentCommand" => {
            let (raw, k) = arg_at(text, j)?;
            let key = raw.trim().strip_prefix('\\')?.to_string();
            let (spec, k) = arg_at(text, k)?;
            let (body, e) = arg_at(text, k)?;
            let (args, default) = xparse(spec);
            define(defs, key, Macro { args, default, body: body.to_string() });
            Some(e)
        }
        "newenvironment" | "renewenvironment" | "provideenvironment" => {
            let (env, mut k) = arg_at(text, j)?;
            let mut args = 0;
            let mut default = None;
            if let Some((n, e)) = opt_at(text, k) {
                args = n.trim().parse().ok()?;
                k = e;
            }
            if let Some((d, e)) = opt_at(text, k) {
                default = Some(d.to_string());
                k = e;
            }
            let (begin, k) = arg_at(text, k)?;
            let (finish, e) = arg_at(text, k)?;
            let env = env.trim();
            if !KEPT_ENVS.contains(&env) {
                defs.envs.insert(env.to_string(), (Macro { args, default, body: begin.to_string() }, finish.to_string()));
            }
            Some(e)
        }
        "newtheorem" => {
            let (env, mut k) = arg_at(text, j)?;
            if let Some((_, e)) = opt_at(text, k) {
                k = e;
            }
            let (word, mut e) = arg_at(text, k)?;
            if let Some((_, e2)) = opt_here(text, e) {
                e = e2;
            }
            meta.theorems.insert(env.trim().to_string(), (word.trim().to_string(), !star));
            Some(e)
        }
        "declaretheorem" => {
            let mut k = j;
            let mut opts = String::new();
            if let Some((o, e)) = opt_at(text, k) {
                opts = o.to_string();
                k = e;
            }
            let (env, mut e) = arg_at(text, k)?;
            if let Some((o, e2)) = opt_here(text, e) {
                opts = o.to_string();
                e = e2;
            }
            let env = env.trim();
            let word = option(&opts, "name").unwrap_or_else(|| capitalized(env));
            let numbered = option(&opts, "numbered").map_or(true, |v| v != "no");
            meta.theorems.insert(env.to_string(), (word, numbered));
            Some(e)
        }
        "title" | "subtitle" | "date" | "author" => {
            let mut k = j;
            if let Some((_, e)) = opt_here(text, k) {
                k = e;
            }
            let (value, e) = arg_at(text, k)?;
            let value = value.to_string();
            match name {
                "title" => meta.title = Some(value),
                "subtitle" => meta.subtitle = Some(value),
                "date" => meta.date = Some(value),
                _ => meta.authors.push(value),
            }
            Some(e)
        }
        "addbibresource" => {
            let mut k = j;
            if let Some((_, e)) = opt_at(text, k) {
                k = e;
            }
            let (path, e) = arg_at(text, k)?;
            meta.bib.get_or_insert_with(|| path.trim().to_string());
            Some(e)
        }
        "bibliography" => {
            // It stays where it is, for the list of works; the header
            // takes its first file.
            if let Some((paths, _)) = arg_at(text, j) {
                meta.bib.get_or_insert_with(|| paths.split(',').next().unwrap_or("").trim().to_string());
            }
            None
        }
        "graphicspath" => {
            let (paths, e) = arg_at(text, j)?;
            let mut k = 0;
            while let Some((p, e2)) = group_at(paths, skip_space(paths, k)) {
                meta.graphics.push(p.trim().to_string());
                k = e2;
            }
            Some(e)
        }
        "lstset" => {
            let (opts, e) = arg_at(text, j)?;
            if let Some(l) = option(opts, "language") {
                meta.lst_lang = Some(l);
            }
            Some(e)
        }
        "setcounter" => {
            if let Some((c, k)) = arg_at(text, j) {
                if c.trim() == "secnumdepth" {
                    if let Some((n, _)) = arg_at(text, k) {
                        meta.secnumdepth = n.trim().parse().ok();
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// The control word right after `\def` or `\let`: its name, letters and
/// `@` (a package's internals), and where it ends.
fn control_word(text: &str, j: usize) -> Option<(String, usize)> {
    let k = skip_space(text, j);
    let b = text.as_bytes();
    if b.get(k) != Some(&b'\\') {
        return None;
    }
    let mut e = k + 1;
    while e < b.len() && (b[e].is_ascii_alphabetic() || b[e] == b'@') {
        e += 1;
    }
    if e == k + 1 {
        let (n, e) = name_at(text, k + 1);
        return Some((n.to_string(), e));
    }
    Some((text[k + 1..e].to_string(), e))
}

/// An xparse argument spec read as `\newcommand` would have it: how many
/// arguments, and the first one's default when it is optional.
fn xparse(spec: &str) -> (usize, Option<String>) {
    let b = spec.as_bytes();
    let mut args = 0;
    let mut default = None;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'm' => args += 1,
            b'o' if args == 0 && default.is_none() => {
                args += 1;
                default = Some(String::new());
            }
            b'O' if args == 0 && default.is_none() => {
                args += 1;
                if let Some((d, e)) = group_at(spec, i + 1) {
                    default = Some(d.to_string());
                    i = e;
                    continue;
                }
            }
            b'{' => {
                if let Some((_, e)) = group_at(spec, i) {
                    i = e;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (args, default)
}

/// `text` with the document's macros and environments expanded where they
/// are used. What a use expands to is read again in place, so a macro may
/// take its arguments from the text after it, as in TeX; each piece is
/// expanded at most `DEPTH` levels deep, and the whole at most so many
/// times, so a recursive macro ends.
fn expand(text: &str, defs: &Defs) -> String {
    if defs.macros.is_empty() && defs.envs.is_empty() {
        return text.to_string();
    }
    let mut s = text.to_string();
    let cap = text.len() * 16 + (1 << 20);
    let mut budget = 100_000usize;
    // Each expansion's extent, by its end's distance from the text's end
    // (which edits before it leave alone), and its depth.
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while let Some(off) = s[i..].find('\\') {
        i += off;
        while regions.last().is_some_and(|&(dist, _)| i >= s.len().saturating_sub(dist)) {
            regions.pop();
        }
        let depth = regions.last().map_or(0, |r| r.1);
        let (name, end) = name_at(&s, i + 1);
        let found = if depth < DEPTH && budget > 0 && s.len() < cap { use_at(&s, name, end, defs) } else { None };
        match found {
            Some((rep, after)) => {
                budget -= 1;
                s.replace_range(i..after, &rep);
                regions.push((s.len() - (i + rep.len()), depth + 1));
            }
            None => i = end,
        }
    }
    s
}

/// What the use of `\name` (ending at `end`) expands to, and where the use
/// ends, its arguments counted. None when it is not the document's.
fn use_at(s: &str, name: &str, end: usize, defs: &Defs) -> Option<(String, usize)> {
    if let Some(m) = defs.macros.get(name) {
        let (args, after) = macro_args(s, end, m);
        return Some((substitute(&m.body, &args), after));
    }
    if name == "begin" || name == "end" {
        let (env, e) = group_at(s, end)?;
        let (m, finish) = defs.envs.get(env.trim())?;
        if name == "end" {
            return Some((finish.clone(), e));
        }
        let (args, after) = macro_args(s, e, m);
        return Some((substitute(&m.body, &args), after));
    }
    None
}

fn macro_args(s: &str, end: usize, m: &Macro) -> (Vec<String>, usize) {
    let mut args = Vec::new();
    let mut j = end;
    let mut left = m.args;
    if let Some(d) = &m.default {
        if left > 0 {
            left -= 1;
            match opt_at(s, j) {
                Some((o, e)) => {
                    args.push(o.to_string());
                    j = e;
                }
                None => args.push(d.clone()),
            }
        }
    }
    for _ in 0..left {
        match arg_at(s, j) {
            Some((a, e)) => {
                args.push(a.to_string());
                j = e;
            }
            None => break,
        }
    }
    (args, j)
}

/// A macro's body with `#1`…`#9` its arguments and `##` a `#`.
fn substitute(body: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '#' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('#') => {
                chars.next();
                out.push('#');
            }
            Some(d @ '1'..='9') => {
                chars.next();
                if let Some(a) = args.get(d as usize - '1' as usize) {
                    out.push_str(a);
                }
            }
            _ => out.push('#'),
        }
    }
    out
}

// ---------------------------------------------------------------------
// Reading TeX: names, groups, arguments.

/// The name of the control sequence whose backslash stands just before
/// `i`: a run of letters, or one other character; and where it ends.
fn name_at(s: &str, i: usize) -> (&str, usize) {
    let b = s.as_bytes();
    match b.get(i) {
        None => ("", i),
        Some(c) if c.is_ascii_alphabetic() => {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_alphabetic() {
                j += 1;
            }
            (&s[i..j], j)
        }
        Some(_) => {
            let len = s[i..].chars().next().map_or(1, char::len_utf8);
            (&s[i..i + len], i + len)
        }
    }
}

/// The braced group opening at `i`: its inside and where it ends. One not
/// closed runs to the end.
fn group_at(s: &str, i: usize) -> Option<(&str, usize)> {
    let b = s.as_bytes();
    if b.get(i) != Some(&b'{') {
        return None;
    }
    let mut depth = 0;
    let mut j = i;
    while j < b.len() {
        match b[j] {
            b'\\' => {
                j += 2;
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[i + 1..j], j + 1));
                }
            }
            _ => {}
        }
        j += 1;
    }
    Some((&s[i + 1..], s.len()))
}

/// Spaces and tabs from `i` on, and one line end at most, as TeX skips
/// them before an argument.
fn skip_space(s: &str, i: usize) -> usize {
    let b = s.as_bytes();
    let mut i = i;
    let mut lines = 0;
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\r' => i += 1,
            b'\n' if lines == 0 => {
                lines += 1;
                i += 1;
            }
            _ => break,
        }
    }
    i
}

/// An optional argument `[…]` right at `i`: its inside and where it ends.
fn opt_here(s: &str, i: usize) -> Option<(&str, usize)> {
    let b = s.as_bytes();
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let (mut depth, mut brace, mut k) = (0i32, 0i32, i);
    while k < b.len() {
        match b[k] {
            b'\\' => {
                k += 2;
                continue;
            }
            b'{' => brace += 1,
            b'}' => brace -= 1,
            b'[' if brace == 0 => depth += 1,
            b']' if brace == 0 => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[i + 1..k], k + 1));
                }
            }
            // Never across a blank line: that `[` was words.
            b'\n' if b.get(k + 1) == Some(&b'\n') => return None,
            _ => {}
        }
        k += 1;
    }
    None
}

/// An optional argument after spaces.
fn opt_at(s: &str, i: usize) -> Option<(&str, usize)> {
    opt_here(s, skip_space(s, i))
}

/// A mandatory argument after spaces: a braced group's inside, or one
/// token.
fn arg_at(s: &str, i: usize) -> Option<(&str, usize)> {
    let j = skip_space(s, i);
    let b = s.as_bytes();
    match *b.get(j)? {
        b'{' => group_at(s, j),
        b'}' | b'\n' => None,
        b'\\' => {
            let (_, e) = name_at(s, j + 1);
            Some((&s[j..e], e))
        }
        _ => {
            let len = s[j..].chars().next().map_or(1, char::len_utf8);
            Some((&s[j..j + len], j + len))
        }
    }
}

/// `(…)` right at `i` (`\cmidrule(lr)`): where it ends.
fn paren_here(s: &str, i: usize) -> Option<usize> {
    if s.as_bytes().get(i) != Some(&b'(') {
        return None;
    }
    s[i..].find(')').map(|k| i + k + 1)
}

/// Past a command's arguments from `j`: a star, optional ones wherever
/// they stand, and `n` mandatory ones.
fn skip_args(s: &str, j: usize, n: usize) -> usize {
    let mut j = j;
    if s.as_bytes().get(j) == Some(&b'*') {
        j += 1;
    }
    for _ in 0..n {
        loop {
            if let Some((_, e)) = opt_at(s, j) {
                j = e;
            } else if let Some(e) = paren_here(s, skip_space(s, j)) {
                j = e;
            } else {
                break;
            }
        }
        match arg_at(s, j) {
            Some((_, e)) => j = e,
            None => break,
        }
    }
    while let Some((_, e)) = opt_here(s, j) {
        j = e;
    }
    j
}

/// Where `\end{name}` closes the environment whose content starts at
/// `from`: the content's end, and the index after it. One not closed runs
/// to the end.
fn env_end(s: &str, name: &str, from: usize) -> (usize, usize) {
    let open = format!("\\begin{{{name}}}");
    let close = format!("\\end{{{name}}}");
    let mut depth = 1;
    let mut i = from;
    loop {
        let o = s[i..].find(&open).map(|x| x + i);
        let c = s[i..].find(&close).map(|x| x + i);
        match (o, c) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                i = o + open.len();
            }
            (_, Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    return (c, c + close.len());
                }
                i = c + close.len();
            }
            _ => return (s.len(), s.len()),
        }
    }
}

/// `s` cut at each `sep` standing outside every group and environment. A
/// control word as `sep` (`\item`) is matched whole.
fn split_top<'a>(s: &'a str, sep: &str) -> Vec<&'a str> {
    let b = s.as_bytes();
    let pat = sep.as_bytes();
    let word = pat.len() > 1 && pat[0] == b'\\' && pat[1..].iter().all(u8::is_ascii_alphabetic);
    let mut parts = Vec::new();
    let (mut start, mut depth, mut i) = (0, 0i32, 0);
    while i < b.len() {
        if depth <= 0 && b[i..].starts_with(pat) && !(word && b.get(i + pat.len()).is_some_and(u8::is_ascii_alphabetic)) {
            parts.push(&s[start..i]);
            i += pat.len();
            start = i;
            continue;
        }
        match b[i] {
            b'\\' => {
                let (name, end) = name_at(s, i + 1);
                if (name == "begin" || name == "end") && b.get(end) == Some(&b'{') {
                    depth += if name == "begin" { 1 } else { -1 };
                    i = group_at(s, end).map_or(end, |(_, e)| e);
                } else {
                    i = end;
                }
                continue;
            }
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

/// The value of `key` in a list of options (`language=Python,
/// caption={A, B}`), its braces off.
fn option(opts: &str, key: &str) -> Option<String> {
    split_top(opts, ",").into_iter().find_map(|part| {
        let (k, v) = part.split_once('=')?;
        (k.trim() == key).then(|| {
            let v = v.trim();
            v.strip_prefix('{').and_then(|x| x.strip_suffix('}')).unwrap_or(v).trim().to_string()
        })
    })
}

/// `s` with each `\name` of `cmds` taken out, with its arguments: the
/// optional ones and the number of braced ones given.
fn strip_cmds(s: &str, cmds: &[(&str, usize)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(off) = s[i..].find('\\') {
        let at = i + off;
        out.push_str(&s[i..at]);
        let (name, end) = name_at(s, at + 1);
        match cmds.iter().find(|(c, _)| *c == name) {
            Some(&(_, n)) => i = skip_args(s, end, n),
            None => {
                out.push_str(&s[at..end]);
                i = end;
            }
        }
    }
    out.push_str(&s[i..]);
    out
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

static LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\label\s*\{([^{}]*)\}").unwrap());

/// `s` with its `\label`s taken out, and the labels.
fn take_labels(s: &str) -> (String, Vec<String>) {
    let labels = LABEL.captures_iter(s).map(|c| c[1].trim().to_string()).collect();
    (LABEL.replace_all(s, "").into_owned(), labels)
}

// ---------------------------------------------------------------------
// Words and signs.

/// Named symbols in text, as their characters.
const SYMBOLS: &[(&str, &str)] = &[
    ("ldots", "…"), ("dots", "…"), ("textellipsis", "…"), ("LaTeX", "LaTeX"), ("TeX", "TeX"), ("LaTeXe", "LaTeX2ε"), ("BibTeX", "BibTeX"), ("XeTeX", "XeTeX"), ("XeLaTeX", "XeLaTeX"), ("LuaTeX", "LuaTeX"),
    ("LuaLaTeX", "LuaLaTeX"), ("pdfTeX", "pdfTeX"), ("pdfLaTeX", "pdfLaTeX"), ("textbackslash", "\\"), ("S", "§"), ("P", "¶"), ("textsection", "§"), ("textparagraph", "¶"), ("copyright", "©"), ("textcopyright", "©"),
    ("textregistered", "®"), ("texttrademark", "™"), ("textdegree", "°"), ("degree", "°"), ("pounds", "£"), ("textsterling", "£"), ("euro", "€"), ("texteuro", "€"), ("EUR", "€"), ("yen", "¥"), ("textyen", "¥"),
    ("dag", "†"), ("ddag", "‡"), ("textdagger", "†"), ("textdaggerdbl", "‡"), ("textbullet", "•"), ("textperiodcentered", "·"), ("textendash", "–"), ("textemdash", "—"), ("textquoteleft", "‘"),
    ("textquoteright", "’"), ("textquotedblleft", "“"), ("textquotedblright", "”"), ("lq", "‘"), ("rq", "’"), ("guillemotleft", "«"), ("guillemotright", "»"), ("guillemetleft", "«"), ("guillemetright", "»"),
    ("textless", "<"), ("textgreater", ">"), ("textbar", "|"), ("textasciitilde", "~"), ("textasciicircum", "^"), ("textunderscore", "_"), ("textbraceleft", "{"), ("textbraceright", "}"), ("textdollar", "$"),
    ("textquotesingle", "'"), ("textquotedbl", "\""), ("textasteriskcentered", "*"), ("checkmark", "✓"), ("textmu", "µ"), ("textpm", "±"), ("texttimes", "×"), ("textdiv", "÷"), ("textonehalf", "½"),
    ("textonequarter", "¼"), ("textthreequarters", "¾"), ("textcent", "¢"), ("textexclamdown", "¡"), ("textquestiondown", "¿"), ("textordfeminine", "ª"), ("textordmasculine", "º"), ("textvisiblespace", "␣"),
    ("slash", "/"), ("ss", "ß"), ("SS", "SS"), ("i", "ı"), ("j", "ȷ"), ("o", "ø"), ("O", "Ø"), ("aa", "å"), ("AA", "Å"), ("ae", "æ"), ("AE", "Æ"), ("oe", "œ"), ("OE", "Œ"), ("l", "ł"), ("L", "Ł"),
    ("nobreakspace", "\u{a0}"), ("thinspace", "\u{2009}"), ("enspace", " "), ("enskip", " "), ("quad", " "), ("qquad", " "), ("space", " "), ("newblock", " "),
];

/// Commands that write nothing, with how many braced arguments they take
/// (optional ones are skipped wherever they stand).
const DROP: &[(&str, usize)] = &[
    ("noindent", 0), ("indent", 0), ("centering", 0), ("raggedright", 0), ("raggedleft", 0), ("RaggedRight", 0), ("vfill", 0), ("hfill", 0), ("hfil", 0), ("vfil", 0), ("hss", 0), ("smallskip", 0), ("medskip", 0),
    ("bigskip", 0), ("newpage", 0), ("clearpage", 0), ("cleardoublepage", 0), ("pagebreak", 0), ("nopagebreak", 0), ("linebreak", 0), ("nolinebreak", 0), ("tableofcontents", 0), ("listoffigures", 0),
    ("listoftables", 0), ("listofalgorithms", 0), ("maketitle", 0), ("appendix", 0), ("frontmatter", 0), ("mainmatter", 0), ("backmatter", 0), ("sloppy", 0), ("fussy", 0), ("protect", 0), ("relax", 0),
    ("makeatletter", 0), ("makeatother", 0), ("ignorespaces", 0), ("phantomsection", 0), ("FloatBarrier", 0), ("balance", 0), ("normalsize", 0), ("small", 0), ("footnotesize", 0), ("scriptsize", 0),
    ("tiny", 0), ("large", 0), ("Large", 0), ("LARGE", 0), ("huge", 0), ("Huge", 0), ("selectfont", 0), ("normalfont", 0), ("rmfamily", 0), ("sffamily", 0), ("mdseries", 0), ("upshape", 0), ("scshape", 0),
    ("rm", 0), ("sf", 0), ("sc", 0), ("doublespacing", 0), ("onehalfspacing", 0), ("singlespacing", 0), ("allowdisplaybreaks", 0), ("qedhere", 0), ("qed", 0), ("null", 0), ("strut", 0), ("leavevmode", 0),
    ("unskip", 0), ("break", 0), ("nobreak", 0), ("hline", 0), ("toprule", 0), ("midrule", 0), ("bottomrule", 0), ("addlinespace", 0), ("printindex", 0), ("printglossary", 0), ("printglossaries", 0),
    ("makeindex", 0), ("restoregeometry", 0), ("bibliographystyle", 1), ("vspace", 1), ("hspace", 1), ("addvspace", 1), ("phantom", 1), ("hphantom", 1), ("vphantom", 1),
    ("pagestyle", 1), ("thispagestyle", 1), ("pagenumbering", 1), ("linespread", 1), ("nocite", 1), ("index", 1), ("glossary", 1), ("color", 1), ("pagecolor", 1), ("hypersetup", 1), ("geometry", 1),
    ("newgeometry", 1), ("captionsetup", 1), ("usetikzlibrary", 1), ("theoremstyle", 1), ("urlstyle", 1), ("sisetup", 1), ("tikzset", 1), ("pgfplotsset", 1), ("setstretch", 1), ("enlargethispage", 1),
    ("hyphenation", 1), ("markright", 1), ("DeclareGraphicsExtensions", 1), ("newcounter", 1), ("stepcounter", 1), ("refstepcounter", 1), ("includeonly", 1), ("thanks", 1), ("keywords", 1),
    ("affiliation", 1), ("affil", 1), ("email", 1), ("institute", 1), ("address", 1), ("orcid", 1), ("institution", 1), ("inst", 1), ("cline", 1), ("cmidrule", 1), ("rowcolor", 1), ("cellcolor", 1),
    ("columncolor", 1), ("arrayrulecolor", 1), ("usepackage", 1), ("RequirePackage", 1), ("documentclass", 1), ("setmainfont", 1), ("setsansfont", 1), ("setmonofont", 1), ("setmainlanguage", 1),
    ("selectlanguage", 1), ("graphicspath", 1), ("lstset", 1), ("addbibresource", 1), ("subjclass", 1), ("ccsdesc", 1), ("received", 1), ("accepted", 1), ("setcopyright", 1), ("copyrightyear", 1),
    ("acmYear", 1), ("acmDOI", 1), ("acmISBN", 1), ("acmJournal", 1), ("acmVolume", 1), ("acmNumber", 1), ("acmArticle", 1), ("acmMonth", 1), ("acmPrice", 1), ("symbol", 1), ("ding", 1),
    ("setlength", 2), ("addtolength", 2), ("settowidth", 2), ("settoheight", 2), ("setcounter", 2), ("addtocounter", 2), ("numberwithin", 2), ("counterwithin", 2), ("counterwithout", 2),
    ("fontsize", 2), ("markboth", 2), ("nomenclature", 2), ("rule", 2), ("pdfbookmark", 2), ("newfontfamily", 2), ("definecolor", 3), ("newacronym", 3), ("crefname", 3), ("Crefname", 3),
    ("addcontentsline", 3), ("acmConference", 3),
];

/// What a table's rules and a long table's repeated heads are: taken out.
const RULES: &[(&str, usize)] = &[
    ("hline", 0), ("toprule", 0), ("midrule", 0), ("bottomrule", 0), ("cline", 1), ("cmidrule", 1), ("addlinespace", 0), ("specialrule", 3), ("hhline", 1), ("morecmidrules", 0), ("rowcolor", 1),
    ("arrayrulecolor", 1), ("noalign", 1), ("hdashline", 0), ("cdashline", 1), ("Xhline", 1), ("firsthline", 0), ("lasthline", 0), ("tabucline", 1), ("rowfont", 1), ("bottomline", 0), ("topline", 0),
    ("midline", 0), ("svhline", 0), ("centering", 0),
];

/// Accents written with a letter (`\c{c}`, `\v s`).
const ACCENT_LETTERS: &[&str] = &["u", "v", "H", "c", "k", "r", "d", "b", "t"];

/// `base` with the accent `mark` on it, as one composed letter where
/// Unicode has one. `\i` and `\j` take an accent as `i` and `j`.
fn accented(mark: &str, base: &str) -> String {
    let combining = match mark {
        "'" => '\u{301}',
        "`" => '\u{300}',
        "^" => '\u{302}',
        "\"" => '\u{308}',
        "~" => '\u{303}',
        "=" => '\u{304}',
        "." => '\u{307}',
        "u" => '\u{306}',
        "v" => '\u{30C}',
        "H" => '\u{30B}',
        "c" => '\u{327}',
        "k" => '\u{328}',
        "r" => '\u{30A}',
        "d" => '\u{323}',
        "b" => '\u{331}',
        "t" => '\u{361}',
        _ => return base.to_string(),
    };
    let base = match base {
        "\\i" => "i".to_string(),
        "\\j" => "j".to_string(),
        b => match b.strip_prefix('\\') {
            Some(name) => SYMBOLS.iter().find(|(n, _)| *n == name).map_or(String::new(), |(_, s)| s.to_string()),
            None => b.to_string(),
        },
    };
    let mut chars = base.chars();
    let Some(first) = chars.next() else {
        // An accent on nothing is the accent itself.
        return match mark {
            "'" => "´",
            "`" => "`",
            "^" => "^",
            "\"" => "¨",
            "~" => "~",
            "=" => "¯",
            "." => "˙",
            _ => "",
        }
        .to_string();
    };
    let mut s = String::new();
    s.push(first);
    s.push(combining);
    s.extend(chars);
    s.nfc().collect()
}

const SUP: &[(char, char)] = &[
    ('0', '⁰'), ('1', '¹'), ('2', '²'), ('3', '³'), ('4', '⁴'), ('5', '⁵'), ('6', '⁶'), ('7', '⁷'), ('8', '⁸'), ('9', '⁹'), ('+', '⁺'), ('-', '⁻'), ('=', '⁼'), ('(', '⁽'), (')', '⁾'), ('a', 'ᵃ'), ('b', 'ᵇ'),
    ('c', 'ᶜ'), ('d', 'ᵈ'), ('e', 'ᵉ'), ('f', 'ᶠ'), ('g', 'ᵍ'), ('h', 'ʰ'), ('i', 'ⁱ'), ('j', 'ʲ'), ('k', 'ᵏ'), ('l', 'ˡ'), ('m', 'ᵐ'), ('n', 'ⁿ'), ('o', 'ᵒ'), ('p', 'ᵖ'), ('r', 'ʳ'), ('s', 'ˢ'), ('t', 'ᵗ'),
    ('u', 'ᵘ'), ('v', 'ᵛ'), ('w', 'ʷ'), ('x', 'ˣ'), ('y', 'ʸ'), ('z', 'ᶻ'), (' ', ' '),
];
const SUB: &[(char, char)] = &[
    ('0', '₀'), ('1', '₁'), ('2', '₂'), ('3', '₃'), ('4', '₄'), ('5', '₅'), ('6', '₆'), ('7', '₇'), ('8', '₈'), ('9', '₉'), ('+', '₊'), ('-', '₋'), ('=', '₌'), ('(', '₍'), (')', '₎'), ('a', 'ₐ'), ('e', 'ₑ'),
    ('h', 'ₕ'), ('i', 'ᵢ'), ('j', 'ⱼ'), ('k', 'ₖ'), ('l', 'ₗ'), ('m', 'ₘ'), ('n', 'ₙ'), ('o', 'ₒ'), ('p', 'ₚ'), ('r', 'ᵣ'), ('s', 'ₛ'), ('t', 'ₜ'), ('u', 'ᵤ'), ('v', 'ᵥ'), ('x', 'ₓ'), (' ', ' '),
];

/// `s` in raised or lowered characters, when Unicode has each.
fn scripted(s: &str, table: &[(char, char)]) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    s.chars().map(|c| table.iter().find(|(a, _)| *a == c).map(|(_, b)| *b)).collect()
}

/// Characters markdown would read as syntax, each behind a backslash.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '$' | '#' | '@' | '|' | '~' | '&') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A paragraph's lines kept from starting what markdown reads as a list,
/// a heading's underline or a div's fence.
fn guard_lines(text: &str) -> String {
    static ORDERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{1,9})([.)])(\s|$)").unwrap());
    text.split('\n')
        .map(|l| {
            let l = l.trim_start();
            let b = l.as_bytes();
            let bullet = matches!(b.first(), Some(b'-' | b'+')) && (b.len() == 1 || matches!(b[1], b' ' | b'\t'));
            let underline = !b.is_empty() && b.iter().all(|c| *c == b'=');
            if bullet || underline || b.first() == Some(&b':') {
                return format!("\\{l}");
            }
            match ORDERED.captures(l) {
                Some(c) => format!("{}\\{}", &c[1], &l[c[1].len()..]),
                None => l.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Inline code: in as many backticks as it does not hold.
fn code_span(text: &str, cell: bool) -> String {
    let text = if cell { text.replace('|', "\\|") } else { text.to_string() };
    let fence = "`".repeat(longest_run(&text, '`') + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') { " " } else { "" };
    format!("{fence}{pad}{text}{pad}{fence}")
}

fn code_block(lang: &str, body: &str) -> String {
    let fence = "`".repeat((longest_run(body, '`') + 1).max(3));
    format!("{fence}{lang}\n{body}\n{fence}")
}

fn longest_run(s: &str, c: char) -> usize {
    let (mut best, mut run) = (0, 0);
    for x in s.chars() {
        run = if x == c { run + 1 } else { 0 };
        best = best.max(run);
    }
    best
}

/// A listing's language as Quarto names it: lowercase, `c++` as `cpp`,
/// listings' dialects (`[Sharp]C`) read.
fn language(raw: &str) -> String {
    let raw = raw.trim().trim_matches(['{', '}']);
    let (dialect, base) = match raw.strip_prefix('[').and_then(|r| r.split_once(']')) {
        Some((d, b)) => (d.trim().to_lowercase(), b.trim().to_lowercase()),
        None => (String::new(), raw.to_lowercase()),
    };
    match (dialect.as_str(), base.as_str()) {
        ("sharp", "c") | (_, "c#") => "csharp".into(),
        ("latex", "tex") => "latex".into(),
        (_, "c++") => "cpp".into(),
        (_, b) => b.replace(' ', ""),
    }
}

/// A link's address as written in `\url`: its escapes read, spaces coded.
fn url_text(u: &str) -> String {
    let mut s = u.trim().replace("\\string~", "~").replace("\\textasciitilde", "~").replace("\\~{}", "~");
    for c in ['#', '%', '&', '_', '$', '~', '{', '}'] {
        s = s.replace(&format!("\\{c}"), &c.to_string());
    }
    s.replace('\n', "").replace(' ', "%20")
}

/// A picture as Quarto writes one, its id and width in its attributes.
fn picture(path: &str, width: Option<&str>, alt: &str, id: Option<&str>) -> String {
    let dest = if path.contains([' ', '(', ')', '<', '>']) { format!("<{path}>") } else { path.to_string() };
    let mut attrs = Vec::new();
    if let Some(id) = id {
        attrs.push(format!("#{id}"));
    }
    if let Some(w) = width {
        attrs.push(format!("width={w}"));
    }
    let attrs = if attrs.is_empty() { String::new() } else { format!("{{{}}}", attrs.join(" ")) };
    format!("![{alt}]({dest}){attrs}")
}

static RELATIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]*\.?[0-9]*)\s*\\(?:textwidth|linewidth|columnwidth|hsize|paperwidth|textheight)$").unwrap());

/// A length as a share of the line: `0.5\textwidth` is 0.5.
fn fraction(v: &str) -> Option<f64> {
    let c = RELATIVE.captures(v.trim())?;
    if c[1].is_empty() {
        Some(1.0)
    } else {
        c[1].parse().ok()
    }
}

fn percent(f: f64) -> String {
    let p = f * 100.0;
    if (p - p.round()).abs() < 0.05 {
        format!("{}%", p.round() as i64)
    } else {
        format!("{p:.1}%")
    }
}

/// A picture's width from `\includegraphics`' options: a share of the line
/// as a percent (and the share), a length as it is, a scale as a percent.
fn width_of(opts: &str) -> (Option<String>, Option<f64>) {
    static LENGTH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]*\.?[0-9]+)\s*(cm|mm|in|pt|px|em|ex|bp|pc)$").unwrap());
    for part in split_top(opts, ",") {
        let Some((k, v)) = part.split_once('=') else { continue };
        let v = v.trim().trim_matches(['{', '}']).trim();
        match k.trim() {
            "width" => {
                if let Some(f) = fraction(v) {
                    return (Some(percent(f)), Some(f));
                }
                if let Some(c) = LENGTH.captures(v) {
                    return (Some(format!("{}{}", &c[1], &c[2])), None);
                }
            }
            "scale" => {
                if let Ok(f) = v.parse::<f64>() {
                    return (Some(percent(f)), None);
                }
            }
            _ => {}
        }
    }
    (None, None)
}

/// A table's column spec as alignments: `l`, `c`, `r`; a paragraph column
/// (`p{}`, `X`) is `l`.
fn columns(spec: &str) -> Vec<char> {
    let b = spec.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        i += 1;
        match c {
            b'l' | b'L' | b'J' => out.push('l'),
            b'c' | b'C' | b'S' | b's' => out.push('c'),
            b'r' | b'R' => out.push('r'),
            b'D' => {
                out.push('r');
                i = skip_args(spec, i, 3);
            }
            b'p' | b'm' | b'b' | b'X' | b'Q' | b'P' | b'M' | b'B' => {
                let mut align = 'l';
                if let Some((o, e)) = opt_here(spec, i) {
                    let words: Vec<&str> = o.split([',', ' ']).map(str::trim).collect();
                    align = if words.contains(&"c") { 'c' } else if words.contains(&"r") { 'r' } else { 'l' };
                    i = e;
                }
                if let Some((_, e)) = group_at(spec, i) {
                    i = e;
                }
                out.push(align);
            }
            b'@' | b'!' | b'>' | b'<' => {
                if let Some((_, e)) = group_at(spec, skip_space(spec, i)) {
                    i = e;
                }
            }
            b'*' => {
                let Some((n, e1)) = arg_at(spec, i) else { continue };
                let Some((inner, e2)) = arg_at(spec, e1) else { continue };
                let n: usize = n.trim().parse().unwrap_or(1);
                let one = columns(inner);
                for _ in 0..n.min(64) {
                    out.extend(&one);
                }
                i = e2;
            }
            b'{' => {
                if let Some((_, e)) = group_at(spec, i - 1) {
                    i = e;
                }
            }
            _ => {}
        }
    }
    out
}

/// A long table's rows with its repeated heads and feet left out: its
/// first head, then its body.
fn longtable_body(s: &str) -> String {
    static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\end(?:firsthead|head|foot|lastfoot)\b").unwrap());
    let parts: Vec<&str> = MARK.split(s).collect();
    if parts.len() < 2 {
        return s.to_string();
    }
    format!("{}\\\\{}", parts[0], parts[parts.len() - 1])
}

fn pipe_table(rows: &[Vec<String>], aligns: &[char]) -> String {
    let Some(n) = rows.iter().map(Vec::len).max() else { return String::new() };
    let line = |cells: &[String]| {
        let mut l = String::from("|");
        for k in 0..n {
            l.push(' ');
            l.push_str(cells.get(k).map_or("", String::as_str));
            l.push_str(" |");
        }
        l
    };
    let mut out = vec![line(&rows[0])];
    let rule: String = (0..n)
        .map(|k| match aligns.get(k) {
            Some('c') => ":---:|",
            Some('r') => "---:|",
            _ => ":---|",
        })
        .collect();
    out.push(format!("|{rule}"));
    out.extend(rows[1..].iter().map(|r| line(r)));
    out.join("\n")
}

/// "a", "a and b", "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A YAML value: quoted when YAML would read it as something else.
fn yaml(v: &str) -> String {
    let lead = v.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c));
    if v.is_empty() || lead || v.contains([':', '#', '"', '\\']) || v != v.trim() {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

fn today() -> String {
    chrono::Local::now().format("%B %-d, %Y").to_string()
}

/// The prefixes a label's own kind is written with (`fig:`), taken off
/// it before Quarto's kind is put on.
const PREFIXES: &[&str] = &[
    "fig", "figure", "tab", "table", "tbl", "eq", "eqn", "equation", "sec", "section", "ssec", "subsec", "subsection", "sssec", "ch", "chap", "chapter", "part", "app", "appendix", "thm", "theorem", "lem",
    "lemma", "cor", "corollary", "prop", "prp", "proposition", "cnj", "conj", "conjecture", "def", "defn", "definition", "ex", "exm", "exa", "example", "exr", "exer", "exercise", "lst", "listing", "alg", "rem",
    "remark",
];

/// A label's id on a thing of `kind`: the kind, then the label with its
/// own prefix off and every character outside `[A-Za-z0-9_-]` a hyphen.
fn id_for(label: &str, kind: &str) -> String {
    let label = label.trim();
    let rest = match label.find([':', '-', '_']) {
        Some(k) if k > 0 && k + 1 < label.len() && PREFIXES.contains(&label[..k].to_ascii_lowercase().as_str()) => &label[k + 1..],
        _ => label,
    };
    let clean: String = rest.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect();
    format!("{kind}-{clean}")
}

/// The Quarto kind of a theorem-like environment, by its word or its name.
fn kind_of(word: &str) -> Option<&'static str> {
    Some(match word.trim().to_lowercase().as_str() {
        "theorem" | "thm" | "theo" => "thm",
        "lemma" | "lem" => "lem",
        "corollary" | "cor" | "coro" | "corol" => "cor",
        "proposition" | "prop" | "prp" => "prp",
        "conjecture" | "conj" | "cnj" => "cnj",
        "definition" | "defn" | "def" | "dfn" => "def",
        "example" | "ex" | "exm" | "exa" => "exm",
        "exercise" | "exer" | "exr" => "exr",
        _ => return None,
    })
}

/// A caption's word for what a reference names, which a reference in a
/// picture's caption says itself (`Figure~\ref{x}` reads "Figure 1").
fn strip_kind_word(para: &mut String) {
    static WORD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:\b(?:figures?|figs?\.|tables?|tabs?\.|equations?|eqs?\.|eqns?\.|sections?|secs?\.|chapters?|chaps?\.|theorems?|thms?\.|lemmas?|corollar(?:y|ies)|propositions?|props?\.|definitions?|defs?\.|examples?|exercises?|conjectures?)|§)\s*$").unwrap()
    });
    if let Some(m) = WORD.find(para) {
        para.truncate(m.start());
    }
}

// ---------------------------------------------------------------------
// The markdown written.

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Para,
    List,
    Other,
}

#[derive(Clone, Debug)]
struct Block {
    kind: Kind,
    text: String,
}

impl Block {
    fn para(text: String) -> Block {
        Block { kind: Kind::Para, text }
    }
}

fn join(blocks: &[Block]) -> String {
    blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n")
}

/// A fenced div around `blocks`, its fence longer than any inside it.
fn div(attrs: &str, blocks: &[Block]) -> String {
    let inner = join(blocks);
    let deepest = inner
        .lines()
        .map(|l| {
            let t = l.trim_start();
            t.len() - t.trim_start_matches(':').len()
        })
        .filter(|n| *n >= 3)
        .max()
        .unwrap_or(2);
    let fence = ":".repeat((deepest + 1).max(3));
    if inner.is_empty() {
        format!("{fence} {attrs}\n{fence}")
    } else {
        format!("{fence} {attrs}\n{inner}\n{fence}")
    }
}

/// `lead` at the start of the first paragraph, or a paragraph of its own.
fn lead_in(blocks: &mut Vec<Block>, lead: &str) {
    match blocks.first_mut() {
        Some(b) if b.kind == Kind::Para && !b.text.starts_with("![") => b.text = format!("{lead} {}", b.text),
        _ => blocks.insert(0, Block::para(lead.to_string())),
    }
}

/// `tail` at the end of the last paragraph, or a paragraph of its own.
fn tail_out(blocks: &mut Vec<Block>, tail: &str) {
    match blocks.last_mut() {
        Some(b) if b.kind == Kind::Para && !b.text.starts_with("![") => b.text = format!("{} {tail}", b.text),
        _ => blocks.push(Block::para(tail.to_string())),
    }
}

/// A list item: its marker, its first block on the marker's line, the
/// rest indented under it.
fn item_text(marker: &str, blocks: &[Block]) -> String {
    let mut text = String::new();
    for (k, b) in blocks.iter().enumerate() {
        if k > 0 {
            text.push_str(if b.kind == Kind::List { "\n" } else { "\n\n" });
        }
        text.push_str(&b.text);
    }
    let indent = " ".repeat(marker.len().max(4));
    let mut lines = text.split('\n');
    let mut out = format!("{marker}{}", lines.next().unwrap_or("")).trim_end().to_string();
    for l in lines {
        out.push('\n');
        if !l.is_empty() {
            out.push_str(&indent);
            out.push_str(l);
        }
    }
    out
}

/// Where converted text goes: finished blocks, and the paragraph being
/// written.
#[derive(Default)]
struct Emit {
    blocks: Vec<Block>,
    para: String,
    /// A line break asked for and not yet written: written only when words
    /// follow, as markdown has none at a paragraph's end.
    broken: bool,
    /// A run-in heading waiting for its paragraph.
    lead: Option<String>,
    /// For a heading, a cell, a caption: blocks become words on one line.
    one_line: bool,
}

impl Emit {
    fn line() -> Emit {
        Emit { one_line: true, ..Default::default() }
    }

    fn push(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if let Some(lead) = self.lead.take() {
            self.para = format!("{lead}{}", self.para.trim_start());
        }
        if self.broken {
            self.broken = false;
            let keep = self.para.trim_end().len();
            self.para.truncate(keep);
            self.para.push_str(if self.one_line { " " } else { "\\\n" });
        }
        self.para.push_str(s);
    }

    fn space(&mut self) {
        if !self.broken && !self.para.is_empty() && !self.para.ends_with([' ', '\n']) {
            self.para.push(' ');
        }
    }

    fn trim_end(&mut self) {
        let keep = self.para.trim_end_matches(' ').len();
        self.para.truncate(keep);
    }

    fn line_break(&mut self) {
        if !self.para.trim().is_empty() {
            self.broken = true;
        }
    }

    fn par(&mut self) {
        self.broken = false;
        let text = self.para.trim();
        if !text.is_empty() {
            let text = if self.one_line { text.to_string() } else { guard_lines(text) };
            self.blocks.push(Block::para(text));
        }
        self.para.clear();
    }

    fn flush_lead(&mut self) {
        if let Some(lead) = self.lead.take() {
            self.blocks.push(Block::para(lead.trim().to_string()));
        }
    }

    fn block(&mut self, kind: Kind, text: String) {
        self.par();
        self.flush_lead();
        if !text.trim().is_empty() {
            self.blocks.push(Block { kind, text });
        }
    }

    fn push_block(&mut self, b: Block) {
        self.block(b.kind, b.text);
    }

    fn finish(mut self) -> Vec<Block> {
        self.par();
        self.flush_lead();
        self.blocks
    }
}

// ---------------------------------------------------------------------
// The conversion.

/// What a label names: a Quarto id, or the words of a `\tag`.
#[derive(Clone, Debug)]
enum Target {
    Id(String),
    Text(String),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Form {
    /// `\ref`: the number alone.
    Number,
    /// `\cref`: the kind's word and the number.
    Named,
    /// `\eqref`: the number in parentheses.
    Paren,
    /// `\nameref`: a section's title, as LaTeX writes it.
    Title,
    /// Any, in a picture's caption, whose text cannot hold brackets.
    Caption,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum CiteForm {
    Paren,
    Text,
    Year,
    Caption,
}

/// What a reference placeholder stands for, said once all is read.
#[derive(Clone, Debug)]
enum Ref {
    /// A heading's attributes: its id when a label finds it.
    Head { id: Option<String>, unnumbered: bool },
    Xref { keys: Vec<String>, form: Form },
    Cite { keys: Vec<String>, pre: String, post: String, form: CiteForm },
}

/// What a `\label` standing on its own names: a heading, or the theorem it
/// is in.
#[derive(Clone, Copy, Debug)]
enum Anchor {
    Head(usize),
    Slot(usize),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mark {
    Em,
    Strong,
    Code,
    Strike,
    Under,
    High,
}

#[derive(Clone, Debug)]
struct Pic {
    path: String,
    width: Option<String>,
    frac: Option<f64>,
    caption: Option<String>,
}

/// A figure or table being read: what it holds, gathered until its end.
#[derive(Debug)]
struct Float {
    kind: &'static str,
    caption: Option<String>,
    numbered: bool,
    own: Vec<String>,
    subs: Vec<String>,
    pics: Vec<Pic>,
    /// How many pictures stand before the first line break among them.
    row: Option<usize>,
    /// Figures or tables of their own inside it (side by side minipages,
    /// each with its caption), as LaTeX numbers each caption; and the
    /// share of the line the first one takes.
    units: Vec<Vec<Block>>,
    unit_frac: Option<f64>,
}

impl Float {
    fn new(kind: &'static str) -> Float {
        Float { kind, caption: None, numbered: true, own: Vec::new(), subs: Vec::new(), pics: Vec::new(), row: None, units: Vec::new(), unit_frac: None }
    }
}

const CITES: &[(&str, CiteForm)] = &[
    ("cite", CiteForm::Paren), ("citep", CiteForm::Paren), ("parencite", CiteForm::Paren), ("Parencite", CiteForm::Paren), ("autocite", CiteForm::Paren), ("Autocite", CiteForm::Paren),
    ("footcite", CiteForm::Paren), ("supercite", CiteForm::Paren), ("smartcite", CiteForm::Paren), ("citealp", CiteForm::Paren), ("Cite", CiteForm::Paren), ("citenum", CiteForm::Paren),
    ("cites", CiteForm::Paren), ("parencites", CiteForm::Paren), ("autocites", CiteForm::Paren), ("citet", CiteForm::Text), ("Citet", CiteForm::Text), ("textcite", CiteForm::Text),
    ("Textcite", CiteForm::Text), ("textcites", CiteForm::Text), ("citealt", CiteForm::Text), ("citeauthor", CiteForm::Text), ("Citeauthor", CiteForm::Text), ("citetitle", CiteForm::Text),
    ("citeyear", CiteForm::Year), ("citeyearpar", CiteForm::Year), ("nocite", CiteForm::Paren),
];

/// Environments that take mandatory arguments before their content.
const ENV_ARGS: &[(&str, usize)] = &[
    ("minipage", 1), ("multicols", 1), ("spacing", 1), ("adjustbox", 1), ("otherlanguage", 1), ("column", 1), ("block", 1), ("alertblock", 1), ("exampleblock", 1), ("lrbox", 1), ("tabbing", 0),
];

struct Conv {
    dir: PathBuf,
    verbs: Vec<Verb>,
    defs: Defs,
    meta: Meta,
    chapters: bool,
    numbered: bool,
    labels: HashMap<String, Target>,
    /// Each heading's words, by its place among the references, for a
    /// `\nameref` to it.
    head_titles: HashMap<usize, String>,
    taken: HashSet<String>,
    autos: HashMap<&'static str, usize>,
    notes: Vec<String>,
    /// The note a `\footnotemark` opened, for its `\footnotetext`.
    mark: Option<usize>,
    refs: Vec<Ref>,
    bibitems: HashMap<String, usize>,
    refs_div: bool,
    anchor: Option<Anchor>,
    slots: Vec<Vec<String>>,
    floats: Vec<Float>,
    marks: Vec<Mark>,
    /// Plain words: for the header, with no markdown at all.
    plain: bool,
    /// Inside code: no escapes, no ligatures.
    code: bool,
    /// Inside a picture's caption, whose text cannot hold brackets.
    caption: bool,
    /// Inside a table cell: `|` escaped even in code and math.
    cell: bool,
    /// How many walks stand inside one another: past `NESTING`, the rest
    /// is written as it is, so no file can run the stack out.
    depth: usize,
}

const NESTING: usize = 200;

impl Conv {
    fn new(dir: PathBuf, verbs: Vec<Verb>, defs: Defs, meta: Meta, body: &str) -> Conv {
        static CHAPTERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(?:chapter|part)\*?\s*[\[{]").unwrap());
        static NUMBERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(?:chapter|section|subsection|subsubsection)\s*[\[{]").unwrap());
        let chapters = CHAPTERS.is_match(body);
        let numbered = NUMBERED.is_match(body) && meta.secnumdepth.map_or(true, |d| d > 0 || (d == 0 && chapters));
        Conv {
            dir,
            verbs,
            defs,
            meta,
            chapters,
            numbered,
            labels: HashMap::new(),
            head_titles: HashMap::new(),
            taken: HashSet::new(),
            autos: HashMap::new(),
            notes: Vec::new(),
            mark: None,
            refs: Vec::new(),
            bibitems: HashMap::new(),
            refs_div: false,
            anchor: None,
            slots: Vec::new(),
            floats: Vec::new(),
            marks: Vec::new(),
            plain: false,
            code: false,
            caption: false,
            cell: false,
            depth: 0,
        }
    }

    fn document(mut self, body: &str) -> String {
        let blocks = self.blocks(body);
        let mut text = join(&blocks);
        if !self.notes.is_empty() {
            let notes: Vec<String> = self.notes.iter().enumerate().map(|(k, n)| format!("[^{}]: {n}", k + 1)).collect();
            text = format!("{}\n\n{}", text.trim_end(), notes.join("\n\n"));
        }
        let head = self.header();
        let text = self.resolve(&text);
        format!("{head}{}\n", text.trim())
    }

    fn blocks(&mut self, src: &str) -> Vec<Block> {
        let mut e = Emit::default();
        self.walk(src, &mut e);
        e.finish()
    }

    /// `src` as words on one line.
    fn inline(&mut self, src: &str) -> String {
        let mut e = Emit::line();
        self.walk(src, &mut e);
        let parts: Vec<String> = e
            .finish()
            .into_iter()
            .map(|b| b.text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "))
            .filter(|t| !t.is_empty())
            .collect();
        parts.join(" ")
    }

    fn plain_text(&mut self, src: &str) -> String {
        let saved = std::mem::replace(&mut self.plain, true);
        let t = self.inline(src);
        self.plain = saved;
        t.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn caption_text(&mut self, src: &str) -> String {
        let saved = std::mem::replace(&mut self.caption, true);
        let t = self.inline(src);
        self.caption = saved;
        t
    }

    /// Words of the text, escaped unless plain or code.
    fn text(&self, s: &str, out: &mut Emit) {
        if self.plain || self.code {
            out.push(s);
        } else {
            out.push(&escape(s));
        }
    }

    /// A word the document may rename (`\renewcommand{\proofname}{…}`).
    fn word(&mut self, name: &str, default: &str) -> String {
        match self.defs.macros.get(name).map(|m| m.body.clone()) {
            Some(body) => {
                let w = self.plain_text(&body);
                if w.is_empty() {
                    default.to_string()
                } else {
                    w
                }
            }
            None => default.to_string(),
        }
    }

    // Labels and ids.

    fn attach(&mut self, label: &str, kind: &'static str) -> String {
        let label = label.trim();
        if let Some(Target::Id(id)) = self.labels.get(label) {
            return id.clone();
        }
        let base = id_for(label, kind);
        let mut id = base.clone();
        let mut n = 2;
        while self.taken.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        self.taken.insert(id.clone());
        self.labels.insert(label.to_string(), Target::Id(id.clone()));
        id
    }

    /// An id of its own for what is numbered and has no label.
    fn auto(&mut self, kind: &'static str) -> String {
        loop {
            let n = self.autos.entry(kind).or_insert(0);
            *n += 1;
            let id = format!("{kind}-auto-{n}");
            if self.taken.insert(id.clone()) {
                return id;
            }
        }
    }

    fn alias(&mut self, label: &str, id: &str) {
        self.labels.entry(label.trim().to_string()).or_insert_with(|| Target::Id(id.to_string()));
    }

    /// A `\label` on its own: the float it is in, else the heading or the
    /// theorem it follows.
    fn stray_label(&mut self, label: &str) {
        if label.is_empty() {
            return;
        }
        if let Some(f) = self.floats.last_mut() {
            f.own.push(label.to_string());
            return;
        }
        match self.anchor {
            Some(Anchor::Head(r)) => {
                let existing = match &self.refs[r] {
                    Ref::Head { id, .. } => id.clone(),
                    _ => None,
                };
                match existing {
                    Some(id) => self.alias(label, &id),
                    None => {
                        let id = self.attach(label, "sec");
                        if let Ref::Head { id: slot, .. } = &mut self.refs[r] {
                            *slot = Some(id);
                        }
                    }
                }
            }
            Some(Anchor::Slot(s)) => self.slots[s].push(label.to_string()),
            None => {}
        }
    }

    fn place(&mut self, r: Ref, out: &mut Emit) {
        let n = self.refs.len();
        self.refs.push(r);
        out.push(&format!("{REF_OPEN}{n}{REF_CLOSE}"));
    }

    fn resolve(&self, text: &str) -> String {
        static PLACE: LazyLock<Regex> = LazyLock::new(|| Regex::new("\u{E000}(\\d+)\u{E001}").unwrap());
        PLACE.replace_all(text, |c: &regex::Captures| c[1].parse::<usize>().ok().and_then(|n| self.refs.get(n)).map_or(String::new(), |r| self.say(r))).into_owned()
    }

    /// The words of the heading that carries `id`.
    fn title_of(&self, id: &str) -> Option<String> {
        let at = self.refs.iter().position(|r| matches!(r, Ref::Head { id: Some(h), .. } if h == id))?;
        self.head_titles.get(&at).filter(|t| !t.is_empty()).cloned()
    }

    fn say(&self, r: &Ref) -> String {
        match r {
            Ref::Head { id, unnumbered } => {
                let mut attrs = Vec::new();
                if let Some(id) = id {
                    attrs.push(format!("#{id}"));
                }
                if *unnumbered {
                    attrs.push(".unnumbered".to_string());
                }
                if attrs.is_empty() {
                    String::new()
                } else {
                    format!(" {{{}}}", attrs.join(" "))
                }
            }
            Ref::Xref { keys, form } => {
                let each: Vec<String> = keys
                    .iter()
                    .map(|k| match (self.labels.get(k), form) {
                        (Some(Target::Id(id)), Form::Number) => format!("[-@{id}]"),
                        (Some(Target::Id(id)), Form::Paren) => format!("([-@{id}])"),
                        (Some(Target::Id(id)), Form::Title) => self.title_of(id).unwrap_or_else(|| format!("@{id}")),
                        (Some(Target::Id(id)), _) => format!("@{id}"),
                        (Some(Target::Text(t)), Form::Paren) => format!("({t})"),
                        (Some(Target::Text(t)), _) => t.clone(),
                        (None, Form::Caption) => "??".to_string(),
                        (None, Form::Paren) => "(**??**)".to_string(),
                        (None, _) => "**??**".to_string(),
                    })
                    .collect();
                and_list(&each)
            }
            Ref::Cite { keys, pre, post, form } => self.say_cite(keys, pre, post, *form),
        }
    }

    fn say_cite(&self, keys: &[String], pre: &str, post: &str, form: CiteForm) -> String {
        let notes = |inner: String| {
            let inner = if pre.is_empty() { inner } else { format!("{pre} {inner}") };
            if post.is_empty() {
                inner
            } else {
                format!("{inner}, {post}")
            }
        };
        // Entries of the document's own list: their numbers.
        if keys.iter().all(|k| self.bibitems.contains_key(k)) {
            let nums: Vec<String> = keys.iter().map(|k| self.bibitems[k].to_string()).collect();
            let inner = notes(nums.join(", "));
            return if form == CiteForm::Caption { format!("({inner})") } else { format!("[{inner}]") };
        }
        let at: Vec<String> = keys.iter().map(|k| format!("@{k}")).collect();
        match form {
            CiteForm::Paren => format!("[{}]", notes(at.join("; "))),
            CiteForm::Year => format!("[{}]", notes(keys.iter().map(|k| format!("-@{k}")).collect::<Vec<_>>().join("; "))),
            CiteForm::Text if !post.is_empty() => format!("{} [{post}]", and_list(&at)),
            CiteForm::Text | CiteForm::Caption => and_list(&at),
        }
    }

    // The header.

    fn header(&mut self) -> String {
        let mut fields: Vec<(&str, String)> = Vec::new();
        for (key, raw) in [("title", self.meta.title.clone()), ("subtitle", self.meta.subtitle.clone())] {
            if let Some(raw) = raw {
                let raw = expand(&raw, &self.defs);
                let v = self.plain_text(&raw);
                if !v.is_empty() {
                    fields.push((key, yaml(&v)));
                }
            }
        }
        let names = self.author_names();
        if !names.is_empty() {
            fields.push(("author", yaml(&and_list(&names))));
        }
        if let Some(raw) = self.meta.date.clone() {
            let raw = expand(&raw, &self.defs);
            let v = self.plain_text(&raw);
            if !v.is_empty() {
                fields.push(("date", yaml(&v)));
            }
        }
        if let Some(bib) = self.meta.bib.clone() {
            let bib = bib.trim();
            if !bib.is_empty() {
                let bib = if bib.ends_with(".bib") { bib.to_string() } else { format!("{bib}.bib") };
                fields.push(("bibliography", yaml(&bib)));
            }
        }
        if self.numbered {
            fields.push(("number-sections", "true".to_string()));
        }
        if fields.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = fields.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        format!("---\n{}\n---\n\n", lines.join("\n"))
    }

    /// The authors' names: each `\author` split at `\and`, each name up to
    /// its first `\\` (what follows is an affiliation), notes and marks off.
    fn author_names(&mut self) -> Vec<String> {
        const MARKS: &[(&str, usize)] = &[
            ("thanks", 1), ("footnote", 1), ("inst", 1), ("textsuperscript", 1), ("IEEEauthorblockA", 1), ("affil", 1), ("affiliation", 1), ("email", 1), ("orcid", 1), ("orcidlink", 1), ("institute", 1),
            ("footnotemark", 0), ("IEEEauthorrefmark", 1), ("authornote", 1), ("address", 1),
        ];
        static AND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(?:And|AND)\b").unwrap());
        static MATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$[^$]*\$").unwrap());
        let mut names = Vec::new();
        for raw in self.meta.authors.clone() {
            let raw = expand(&raw, &self.defs);
            let raw = AND.replace_all(&raw, "\\and ");
            for part in split_top(&raw, "\\and") {
                let first = split_top(part, "\\\\")[0];
                let first = strip_cmds(first, MARKS);
                let first = MATH.replace_all(&first, "");
                let name = self.plain_text(&first);
                let name = name.trim().trim_matches(',').trim();
                if !name.is_empty() {
                    names.push(name.to_string());
                }
            }
        }
        names
    }

    // The walk.

    fn walk(&mut self, src: &str, out: &mut Emit) {
        if self.depth >= NESTING {
            self.text(src.trim(), out);
            return;
        }
        self.depth += 1;
        self.walk_in(src, out);
        self.depth -= 1;
    }

    fn walk_in(&mut self, src: &str, out: &mut Emit) {
        let b = src.as_bytes();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\\' => i = self.command(src, i, out),
                b'{' => {
                    let (inner, end) = group_at(src, i).unwrap_or((&src[i + 1..], src.len()));
                    self.walk(inner, out);
                    i = end;
                }
                b'}' => i += 1,
                b'$' => i = self.dollar(src, i, out),
                b'\n' => {
                    let mut k = i + 1;
                    while k < b.len() && matches!(b[k], b' ' | b'\t' | b'\r') {
                        k += 1;
                    }
                    if k < b.len() && b[k] == b'\n' {
                        while k < b.len() && b[k].is_ascii_whitespace() {
                            k += 1;
                        }
                        self.row_break();
                        out.par();
                    } else {
                        out.space();
                    }
                    i = k;
                }
                b' ' | b'\t' | b'\r' => {
                    out.space();
                    i += 1;
                }
                b'~' => {
                    out.push("\u{a0}");
                    i += 1;
                }
                b'-' if !self.code => {
                    let (s, n) = if src[i..].starts_with("---") {
                        ("—", 3)
                    } else if src[i..].starts_with("--") {
                        ("–", 2)
                    } else {
                        ("-", 1)
                    };
                    self.text(s, out);
                    i += n;
                }
                b'`' if !self.code => {
                    let double = src[i..].starts_with("``");
                    out.push(if double { "“" } else { "‘" });
                    i += if double { 2 } else { 1 };
                }
                b'\'' if !self.code => {
                    let double = src[i..].starts_with("''");
                    out.push(if double { "”" } else { "’" });
                    i += if double { 2 } else { 1 };
                }
                _ => {
                    let c = src[i..].chars().next().unwrap_or(' ');
                    if c == VERB_OPEN {
                        i = self.verbatim(src, i, out);
                        continue;
                    }
                    // A run of plain characters at once.
                    let end = src[i..].find(|x: char| matches!(x, '\\' | '{' | '}' | '$' | '\n' | ' ' | '\t' | '\r' | '~' | '-' | '`' | '\'' | VERB_OPEN)).map_or(src.len(), |k| i + k);
                    let end = if end == i { i + c.len_utf8() } else { end };
                    self.text(&src[i..end], out);
                    i = end;
                }
            }
        }
    }

    /// A line break or a blank line among a figure's pictures ends a row.
    fn row_break(&mut self) {
        if let Some(f) = self.floats.last_mut() {
            if f.kind == "fig" && f.row.is_none() && !f.pics.is_empty() {
                f.row = Some(f.pics.len());
            }
        }
    }

    /// A control sequence at `at`: converted, and where to read on.
    fn command(&mut self, src: &str, at: usize, out: &mut Emit) -> usize {
        let b = src.as_bytes();
        let (name, j) = name_at(src, at + 1);
        match name {
            "" => return j,
            "\\" => {
                let mut j = j;
                if b.get(j) == Some(&b'*') {
                    j += 1;
                }
                if let Some((_, e)) = opt_here(src, j) {
                    j = e;
                }
                self.row_break();
                out.line_break();
                return j;
            }
            "[" => {
                let (math, e) = until(src, j, "\\]");
                self.display(math, "", out);
                return e;
            }
            "(" => {
                let (math, e) = until(src, j, "\\)");
                self.inline_math(math, out);
                return e;
            }
            ")" | "]" | "!" | "/" | "-" | "@" | "*" => return j,
            "&" | "%" | "#" | "_" | "{" | "}" | "$" => {
                self.text(name, out);
                return j;
            }
            " " | "\n" | "\t" | ";" | ":" | ">" => {
                out.space();
                return j;
            }
            "," => {
                out.push("\u{2009}");
                return j;
            }
            "'" | "`" | "^" | "\"" | "~" | "=" | "." => return self.accent(name, src, j, out),
            _ => {}
        }
        if ACCENT_LETTERS.contains(&name) {
            if let Some(e) = self.letter_accent(name, src, j, out) {
                return e;
            }
        }
        let star = b.get(j) == Some(&b'*');
        let after_star = if star { j + 1 } else { j };
        match name {
            "begin" => return self.environment(src, j, out),
            "end" => return group_at(src, skip_space(src, j)).map_or(j, |(_, e)| e),
            "par" => {
                self.row_break();
                out.par();
                return j;
            }
            "newline" => {
                out.line_break();
                return j;
            }
            "item" => {
                out.par();
                if let Some((l, e)) = opt_at(src, j) {
                    let t = self.inline(l);
                    out.push(&format!("**{t}** "));
                    return e;
                }
                return j;
            }
            "part" | "chapter" | "section" | "subsection" | "subsubsection" | "addchap" | "addsec" => return self.heading(name, src, after_star, star, out),
            "paragraph" | "subparagraph" => return self.run_in(src, after_star, out),
            "label" => {
                let Some((l, e)) = arg_at(src, j) else { return j };
                self.stray_label(l.trim());
                return e;
            }
            "ref" | "pageref" | "vref" | "subref" | "labelcref" | "autoref" | "Autoref" | "cref" | "Cref" | "cpageref" | "Cpageref" | "nameref" | "Nameref" | "eqref" => {
                return self.reference(name, src, after_star, out);
            }
            "footnote" | "tablefootnote" => return self.footnote(src, j, out),
            "footnotemark" => {
                let e = opt_here(src, j).map_or(j, |(_, e)| e);
                if self.plain || self.caption {
                    return e;
                }
                let n = self.notes.len();
                self.notes.push(String::new());
                self.mark = Some(n);
                out.trim_end();
                out.push(&format!("[^{}]", n + 1));
                return e;
            }
            "footnotetext" => {
                let mut k = j;
                if let Some((_, e)) = opt_here(src, k) {
                    k = e;
                }
                let Some((text, e)) = arg_at(src, k) else { return k };
                if let Some(n) = self.mark.take() {
                    self.notes[n] = self.inline(text);
                }
                return e;
            }
            "emph" | "textit" | "textsl" => return self.styled_arg(src, j, Mark::Em, out),
            "textbf" => return self.styled_arg(src, j, Mark::Strong, out),
            "texttt" => return self.styled_arg(src, j, Mark::Code, out),
            "sout" | "st" | "xout" | "strike" => return self.styled_arg(src, j, Mark::Strike, out),
            "underline" | "uline" | "ul" | "uuline" | "uwave" => return self.styled_arg(src, j, Mark::Under, out),
            "hl" | "highlight" => return self.styled_arg(src, j, Mark::High, out),
            "bf" | "bfseries" => {
                self.styled(&src[j..], Mark::Strong, out);
                return src.len();
            }
            "it" | "itshape" | "em" | "sl" | "slshape" => {
                self.styled(&src[j..], Mark::Em, out);
                return src.len();
            }
            "tt" | "ttfamily" => {
                self.styled(&src[j..], Mark::Code, out);
                return src.len();
            }
            "textsuperscript" | "textsubscript" => {
                let Some((inner, e)) = arg_at(src, j) else { return j };
                let words = self.plain_text(inner);
                match scripted(&words, if name == "textsuperscript" { SUP } else { SUB }) {
                    Some(s) => self.text(&s, out),
                    None => self.walk(inner, out),
                }
                return e;
            }
            "url" | "nolinkurl" | "path" => {
                let Some((u, e)) = arg_at(src, j) else { return j };
                let u = url_text(u);
                if name == "path" && !self.plain && !self.code {
                    out.push(&code_span(&u, self.cell));
                } else if self.plain || self.code || self.caption || name != "url" {
                    self.text(&u, out);
                } else if u.contains("://") || u.starts_with("mailto:") {
                    out.push(&format!("<{u}>"));
                } else {
                    let dest = if u.starts_with("www.") { format!("https://{u}") } else { u.clone() };
                    out.push(&format!("[{}]({dest})", escape(&u)));
                }
                return e;
            }
            "href" => {
                let Some((u, k)) = arg_at(src, j) else { return j };
                let Some((t, e)) = arg_at(src, k) else { return k };
                let u = url_text(u);
                if self.plain || self.code || self.caption {
                    self.walk(t, out);
                    return e;
                }
                let words = self.inline(t);
                let words = if words.is_empty() { escape(&u) } else { words };
                let dest = if u.contains(['(', ')', '<', '>']) { format!("<{u}>") } else { u };
                out.push(&format!("[{words}]({dest})"));
                return e;
            }
            "enquote" | "textquote" => {
                let Some((inner, e)) = arg_at(src, j) else { return j };
                out.push("“");
                self.walk(inner, out);
                out.push("”");
                return e;
            }
            "MakeUppercase" | "uppercase" | "MakeTextUppercase" | "MakeLowercase" | "lowercase" | "MakeTextLowercase" => {
                let Some((inner, e)) = arg_at(src, j) else { return j };
                let t = self.inline(inner);
                out.push(&if name.contains("pper") { t.to_uppercase() } else { t.to_lowercase() });
                return e;
            }
            "texorpdfstring" => {
                let Some((tex, k)) = arg_at(src, j) else { return j };
                self.walk(tex, out);
                return arg_at(src, k).map_or(k, |(_, e)| e);
            }
            "ensuremath" => {
                let Some((m, e)) = arg_at(src, j) else { return j };
                self.inline_math(m, out);
                return e;
            }
            "includegraphics" | "includesvg" => return self.includegraphics(src, after_star, out),
            "caption" | "captionof" | "subcaption" => return self.caption_cmd(name, src, after_star, star, out),
            "subfloat" | "subfigure" | "subtable" | "subcaptionbox" => return self.subfloat_cmd(name, src, j, out),
            "multirow" => {
                let mut k = j;
                if let Some((_, e)) = opt_at(src, k) {
                    k = e;
                }
                for _ in 0..2 {
                    let Some((_, e)) = arg_at(src, k) else { return k };
                    k = e;
                    if let Some((_, e)) = opt_at(src, k) {
                        k = e;
                    }
                }
                let Some((text, e)) = arg_at(src, k) else { return k };
                self.walk(text, out);
                return e;
            }
            "today" => {
                self.text(&today(), out);
                return j;
            }
            "bibliography" | "printbibliography" => {
                let e = if name == "bibliography" { arg_at(src, j).map_or(j, |(_, e)| e) } else { skip_args(src, j, 0) };
                if !self.refs_div && !out.one_line {
                    self.refs_div = true;
                    out.block(Kind::Other, "::: {#refs}\n:::".to_string());
                }
                return e;
            }
            "bibitem" => {
                let mut k = j;
                if let Some((_, e)) = opt_at(src, k) {
                    k = e;
                }
                return arg_at(src, k).map_or(k, |(_, e)| e);
            }
            "lstinputlisting" | "inputminted" => return self.code_file(name, src, j, out),
            "abstract" if b.get(skip_space(src, j)) == Some(&b'{') => {
                let Some((body, e)) = arg_at(src, j) else { return j };
                self.abstract_block(body, out);
                return e;
            }
            "lettrine" => {
                let mut k = j;
                if let Some((_, e)) = opt_at(src, k) {
                    k = e;
                }
                for _ in 0..2 {
                    let Some((part, e)) = arg_at(src, k) else { return k };
                    self.walk(part, out);
                    k = e;
                }
                return k;
            }
            "vskip" | "hskip" | "kern" | "mskip" => {
                static DIM: LazyLock<Regex> = LazyLock::new(|| {
                    Regex::new(r"^\s*[-+]?(?:\d*\.?\d+)\s*(?:pt|em|ex|cm|mm|in|bp|pc|sp|mu|\\[a-zA-Z]+)(?:\s*(?:plus|minus)\s*[-+]?(?:\d*\.?\d+)\s*(?:pt|em|ex|cm|mm|in|bp|pc|sp|mu|fil+|\\[a-zA-Z]+))*").unwrap()
                });
                return j + DIM.find(&src[j..]).map_or(0, |m| m.end());
            }
            _ => {}
        }
        if let Some(&(_, form)) = CITES.iter().find(|(n, _)| *n == name) {
            return self.cite(name, form, src, after_star, out);
        }
        if let Some((_, sym)) = SYMBOLS.iter().find(|(n, _)| *n == name) {
            if *sym == " " {
                out.space();
            } else {
                self.text(sym, out);
            }
            return j;
        }
        if let Some(&(_, n)) = DROP.iter().find(|(c, _)| *c == name) {
            return skip_args(src, j, n);
        }
        if !name.bytes().all(|c| c.is_ascii_alphabetic()) {
            return j;
        }
        self.unknown(src, j, out)
    }

    /// A command this file does not know: its optional arguments go, and
    /// the words of its last braced one stay.
    fn unknown(&mut self, src: &str, j: usize, out: &mut Emit) -> usize {
        let b = src.as_bytes();
        let mut j = j;
        if b.get(j) == Some(&b'*') {
            j += 1;
        }
        let mut last = None;
        loop {
            if let Some((_, e)) = opt_here(src, j) {
                j = e;
                continue;
            }
            let k = j + src[j..].len() - src[j..].trim_start_matches([' ', '\t']).len();
            match group_at(src, k) {
                Some((g, e)) => {
                    last = Some(g);
                    j = e;
                }
                None => break,
            }
        }
        if let Some(g) = last {
            self.walk(g, out);
        }
        j
    }

    fn accent(&mut self, mark: &str, src: &str, j: usize, out: &mut Emit) -> usize {
        let b = src.as_bytes();
        let (base, end) = match b.get(j) {
            Some(b'{') => {
                let (g, e) = group_at(src, j).unwrap_or(("", j + 1));
                (g.trim().to_string(), e)
            }
            Some(b'\\') => {
                let (n, e) = name_at(src, j + 1);
                (format!("\\{n}"), e)
            }
            Some(c) if !c.is_ascii_whitespace() => {
                let ch = src[j..].chars().next().unwrap_or(' ');
                (ch.to_string(), j + ch.len_utf8())
            }
            _ => (String::new(), j),
        };
        self.text(&accented(mark, &base), out);
        end
    }

    /// `\c{c}` or `\c c`: None when no letter follows, so it is no accent.
    fn letter_accent(&mut self, mark: &str, src: &str, j: usize, out: &mut Emit) -> Option<usize> {
        let b = src.as_bytes();
        if b.get(j) == Some(&b'{') {
            return Some(self.accent(mark, src, j, out));
        }
        let k = j + src[j..].len() - src[j..].trim_start_matches([' ', '\t']).len();
        if k > j && b.get(k).is_some_and(u8::is_ascii_alphabetic) {
            self.text(&accented(mark, &src[k..k + 1]), out);
            return Some(k + 1);
        }
        None
    }

    // Structure.

    fn heading(&mut self, name: &str, src: &str, j: usize, star: bool, out: &mut Emit) -> usize {
        let mut j = j;
        if let Some((_, e)) = opt_at(src, j) {
            j = e;
        }
        let Some((title, e)) = arg_at(src, j) else { return j };
        if out.one_line {
            self.walk(title, out);
            return e;
        }
        let deeper = usize::from(self.chapters);
        let level = match name {
            "part" | "chapter" | "addchap" => 1,
            "section" | "addsec" => 1 + deeper,
            "subsection" => 2 + deeper,
            _ => 3 + deeper,
        };
        let unnumbered = star || name == "part" || name.starts_with("add");
        let r = self.refs.len();
        self.refs.push(Ref::Head { id: None, unnumbered });
        self.anchor = Some(Anchor::Head(r));
        let words = self.plain_text(title);
        self.head_titles.insert(r, words.trim().to_string());
        let text = self.inline(title);
        out.block(Kind::Other, format!("{} {text}{REF_OPEN}{r}{REF_CLOSE}", "#".repeat(level.min(6))));
        e
    }

    /// `\paragraph{T}`: `**T.**` at the start of the paragraph that follows.
    fn run_in(&mut self, src: &str, j: usize, out: &mut Emit) -> usize {
        let mut j = j;
        if let Some((_, e)) = opt_at(src, j) {
            j = e;
        }
        let Some((title, e)) = arg_at(src, j) else { return j };
        let t = self.inline(title);
        if out.one_line {
            out.push(&t);
            return e;
        }
        out.par();
        let t = t.trim();
        if !t.is_empty() {
            out.lead = Some(if t.ends_with(['.', '?', '!', ':']) { format!("**{t}** ") } else { format!("**{t}.** ") });
        }
        e
    }

    fn reference(&mut self, name: &str, src: &str, j: usize, out: &mut Emit) -> usize {
        let Some((keys, e)) = arg_at(src, j) else { return j };
        if self.plain {
            return e;
        }
        let keys: Vec<String> = keys.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
        let mut form = match name {
            "ref" | "pageref" | "vref" | "subref" | "labelcref" => Form::Number,
            "eqref" => Form::Paren,
            "nameref" | "Nameref" => Form::Title,
            _ => Form::Named,
        };
        if self.caption {
            if form == Form::Number {
                strip_kind_word(&mut out.para);
            }
            form = Form::Caption;
        }
        self.place(Ref::Xref { keys, form }, out);
        e
    }

    fn cite(&mut self, name: &str, form: CiteForm, src: &str, j: usize, out: &mut Emit) -> usize {
        let b = src.as_bytes();
        let multi = name.ends_with("cites");
        let mut j = j;
        while let Some(e) = paren_here(src, j).filter(|_| multi) {
            j = e;
        }
        let (mut keys, mut pre, mut post) = (Vec::new(), String::new(), String::new());
        loop {
            let (o1, k) = match opt_at(src, j) {
                Some((o, e)) => (Some(o), e),
                None => (None, j),
            };
            let (o2, k) = match o1.and_then(|_| opt_here(src, k)) {
                Some((o, e)) => (Some(o), e),
                None => (None, k),
            };
            if b.get(skip_space(src, k)) != Some(&b'{') {
                break;
            }
            let Some((list, e)) = arg_at(src, k) else { break };
            let (p, q) = match (o1, o2) {
                (Some(a), Some(b)) => (a, b),
                (Some(a), None) => ("", a),
                _ => ("", ""),
            };
            if keys.is_empty() {
                pre = self.inline(p).replace('\u{a0}', " ");
            }
            let q = self.inline(q).replace('\u{a0}', " ");
            if !q.is_empty() {
                post = q;
            }
            keys.extend(list.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()));
            j = e;
            if !multi || !matches!(b.get(skip_space(src, j)), Some(b'[' | b'{')) {
                break;
            }
        }
        if name == "nocite" || self.plain || keys.is_empty() {
            return j;
        }
        let form = if self.caption { CiteForm::Caption } else { form };
        self.place(Ref::Cite { keys, pre, post, form }, out);
        j
    }

    fn footnote(&mut self, src: &str, j: usize, out: &mut Emit) -> usize {
        let mut j = j;
        if let Some((_, e)) = opt_here(src, j) {
            j = e;
        }
        let Some((text, e)) = arg_at(src, j) else { return j };
        if self.plain || self.caption {
            return e;
        }
        let n = self.notes.len();
        self.notes.push(String::new());
        self.notes[n] = self.inline(text);
        out.trim_end();
        out.push(&format!("[^{}]", n + 1));
        e
    }

    fn styled_arg(&mut self, src: &str, j: usize, mark: Mark, out: &mut Emit) -> usize {
        let Some((inner, e)) = arg_at(src, j) else { return j };
        self.styled(inner, mark, out);
        e
    }

    /// `src` in `mark`. Plain words, a mark inside the same mark, and what
    /// holds a paragraph's end or a block go unmarked.
    fn styled(&mut self, src: &str, mark: Mark, out: &mut Emit) {
        static BLOCKISH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n[ \t]*\n|\\(?:(?:par|begin|item|part|chapter|section|subsection|subsubsection|paragraph)\b|\[)|\$\$").unwrap());
        if self.plain || (self.code && mark != Mark::Code) || self.marks.contains(&mark) || BLOCKISH.is_match(src) || src.contains(VERB_OPEN) && mark != Mark::Code {
            self.walk(src, out);
            return;
        }
        self.marks.push(mark);
        let code = self.code;
        if mark == Mark::Code {
            self.code = true;
        }
        let inner = self.inline(src);
        self.code = code;
        self.marks.pop();
        if src.starts_with(char::is_whitespace) {
            out.space();
        }
        if !inner.is_empty() {
            let t = match mark {
                Mark::Em => format!("*{inner}*"),
                Mark::Strong => format!("**{inner}**"),
                Mark::Code => code_span(&inner, self.cell),
                Mark::Strike => format!("~~{inner}~~"),
                Mark::Under => format!("<u>{inner}</u>"),
                Mark::High => format!("<mark>{inner}</mark>"),
            };
            out.push(&t);
        }
        if src.ends_with(char::is_whitespace) {
            out.space();
        }
    }

    // Math.

    fn dollar(&mut self, src: &str, i: usize, out: &mut Emit) -> usize {
        static BLANK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n[ \t]*\n").unwrap());
        if src.as_bytes().get(i + 1) == Some(&b'$') {
            return match find_math_end(src, i + 2, "$$") {
                Some(k) => {
                    self.display(&src[i + 2..k], "", out);
                    k + 2
                }
                None => {
                    self.text("$$", out);
                    i + 2
                }
            };
        }
        match find_math_end(src, i + 1, "$") {
            // Math in a line never runs across a paragraph's end.
            Some(k) if !BLANK.is_match(&src[i + 1..k]) => {
                self.inline_math(&src[i + 1..k], out);
                k + 1
            }
            _ => {
                self.text("$", out);
                i + 1
            }
        }
    }

    fn inline_math(&mut self, math: &str, out: &mut Emit) {
        let m = LABEL.replace_all(math, "");
        let m = m.split_whitespace().collect::<Vec<_>>().join(" ");
        if m.is_empty() {
            return;
        }
        let m = if self.cell { m.replace("\\|", "\\Vert ").replace('|', "\\vert ") } else { m };
        out.push(&format!("${m}$"));
    }

    /// Display math as a block: a line `$$`, its lines, a line `$$`, with
    /// the id that numbers it on the closing line. `env` is the
    /// environment it was written in, empty for `\[…\]` and `$$…$$`.
    fn display(&mut self, math: &str, env: &str, out: &mut Emit) {
        static NONUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(?:nonumber|notag)\b").unwrap());
        static QED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\qedhere\b").unwrap());
        static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\tag\*?\s*\{([^{}]*)\}").unwrap());
        if out.one_line {
            self.inline_math(math, out);
            return;
        }
        let base = env.trim_end_matches('*');
        let starred = env.ends_with('*');
        let (wrap, numbered) = match base {
            "equation" | "dmath" => (None, !starred),
            "align" | "flalign" | "alignat" | "xalignat" | "xxalignat" | "eqnarray" | "multline" => (Some("aligned"), !starred),
            "gather" => (Some("gathered"), !starred),
            _ => (None, false),
        };
        let mut math = math.to_string();
        if base.ends_with("alignat") {
            if let Some((_, e)) = arg_at(&math, 0) {
                math = math[e..].to_string();
            }
        }
        let (stripped, labels) = take_labels(&math);
        math = stripped;
        // `\nonumber` on every line, or anywhere in one equation, means no
        // number; an align keeps one for the whole while any line has one.
        let rows: Vec<&str> = split_top(&math, "\\\\").into_iter().filter(|r| !r.trim().is_empty()).collect();
        let off = rows.iter().filter(|r| NONUMBER.is_match(r)).count();
        let all_off = off > 0 && (wrap.is_none() || off >= rows.len());
        math = NONUMBER.replace_all(&math, "").into_owned();
        math = QED.replace_all(&math, "").into_owned();
        let tag = TAG.captures(&math).map(|c| c[1].to_string());
        if wrap.is_some() && tag.is_some() {
            // A tag cannot stand inside `aligned`: it goes after it.
            math = TAG.replace_all(&math, "").into_owned();
        }
        let mut body = math.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
        if body.is_empty() {
            return;
        }
        if let Some(w) = wrap {
            body = format!("\\begin{{{w}}}\n{body}\n\\end{{{w}}}");
            if let Some(t) = &tag {
                body.push_str(&format!("\n\\tag{{{t}}}"));
            }
        }
        let mut close = "$$".to_string();
        if numbered && !all_off && tag.is_none() {
            let id = match labels.first() {
                Some(l) => self.attach(l, "eq"),
                None => self.auto("eq"),
            };
            for l in labels.iter().skip(1) {
                self.alias(l, &id);
            }
            close = format!("$$ {{#{id}}}");
        } else if let Some(t) = &tag {
            let words = self.plain_text(t);
            for l in &labels {
                self.labels.entry(l.clone()).or_insert_with(|| Target::Text(words.clone()));
            }
        }
        out.block(Kind::Other, format!("$$\n{body}\n{close}"));
    }

    fn verbatim(&mut self, src: &str, i: usize, out: &mut Emit) -> usize {
        let start = i + VERB_OPEN.len_utf8();
        let end = src[start..].find(VERB_CLOSE).map_or(src.len(), |k| start + k);
        let after = (end + VERB_CLOSE.len_utf8()).min(src.len());
        let Some(v) = src[start..end].parse::<usize>().ok().and_then(|n| self.verbs.get(n)).cloned() else { return after };
        if v.block && !out.one_line && !self.plain {
            if let Some(c) = &v.caption {
                let t = self.inline(c);
                if !t.is_empty() {
                    out.block(Kind::Para, format!("*{t}*"));
                }
            }
            let lang = if v.lang.is_empty() && v.env == "lstlisting" { self.meta.lst_lang.clone().unwrap_or_default() } else { v.lang.clone() };
            out.block(Kind::Other, code_block(&language(&lang), &v.body));
        } else if self.plain || self.code {
            out.push(&v.body.replace('\n', " "));
        } else {
            out.push(&code_span(&v.body.replace('\n', " "), self.cell));
        }
        after
    }

    /// `\lstinputlisting{file}` and `\inputminted{lang}{file}`: the file's
    /// code, as a block.
    fn code_file(&mut self, name: &str, src: &str, j: usize, out: &mut Emit) -> usize {
        let mut k = j;
        let mut lang = String::new();
        if let Some((o, e)) = opt_at(src, k) {
            lang = option(o, "language").unwrap_or_default();
            k = e;
        }
        if name == "inputminted" {
            let Some((l, e)) = arg_at(src, k) else { return k };
            lang = l.to_string();
            k = e;
        }
        let Some((path, e)) = arg_at(src, k) else { return k };
        let path = self.dir.join(path.trim());
        if let Ok(code) = std::fs::read_to_string(&path) {
            if lang.is_empty() {
                lang = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_string();
            }
            if !self.plain && !out.one_line {
                out.block(Kind::Other, code_block(&language(&lang), code.trim_end()));
            }
        }
        e
    }

    // Environments.

    fn environment(&mut self, src: &str, j: usize, out: &mut Emit) -> usize {
        let k = skip_space(src, j);
        let Some((name, start)) = group_at(src, k) else { return j };
        let name = name.trim().to_string();
        let (stop, after) = env_end(src, &name, start);
        let body = &src[start..stop];
        let base = name.trim_end_matches('*');
        if let Some((word, kind)) = self.theorem_of(&name) {
            self.theorem(word, kind, body, out);
            return after;
        }
        match base {
            "itemize" | "enumerate" | "description" | "compactitem" | "compactenum" | "compactdesc" | "asparaitem" | "asparaenum" | "asparadesc" | "inparaitem" | "inparaenum" | "tablenotes" => {
                self.list(base, body, out)
            }
            "quote" | "quotation" | "verse" | "displayquote" => self.quote(body, out),
            "center" => self.div_env("{style=\"text-align: center\"}", body, out),
            "flushright" => self.div_env("{style=\"text-align: right\"}", body, out),
            "abstract" => self.abstract_block(body, out),
            "proof" => self.proof(body, out),
            "figure" | "wrapfigure" | "SCfigure" | "sidewaysfigure" => self.float("fig", base, body, out),
            "listing" => self.float("lst", base, body, out),
            "table" | "wraptable" | "sidewaystable" | "SCtable" => self.float("tbl", base, body, out),
            "subfigure" | "subtable" => {
                let mut k = 0;
                if let Some((_, e)) = opt_at(body, 0) {
                    k = e;
                }
                let mut frac = None;
                if let Some((w, e)) = arg_at(body, k) {
                    if let Some(f) = fraction(w) {
                        frac = Some(f);
                        k = e;
                    }
                }
                if self.floats.is_empty() {
                    self.float(if base == "subtable" { "tbl" } else { "fig" }, base, &body[k..], out);
                } else {
                    self.sub_float(None, &body[k..], frac, false, out);
                }
            }
            "minipage" if !self.floats.is_empty() => {
                let mut k = 0;
                while let Some((_, e)) = opt_at(body, k) {
                    k = e;
                }
                let mut frac = None;
                if let Some((w, e)) = arg_at(body, k) {
                    frac = fraction(w);
                    k = e;
                }
                self.sub_float(None, &body[k..], frac, true, out);
            }
            "tabular" | "tabularx" | "tabulary" | "tabu" | "tblr" | "array" | "supertabular" | "xtabular" | "mpsupertabular" => {
                let t = self.tabular(&name, body);
                out.block(Kind::Other, t);
            }
            "longtable" | "longtabu" | "xltabular" | "longtblr" => {
                if self.floats.is_empty() {
                    // A long table is its own float: its caption is a row.
                    self.floats.push(Float::new("tbl"));
                    let t = self.tabular(&name, body);
                    let f = self.floats.pop().unwrap_or_else(|| Float::new("tbl"));
                    self.table_out(f, vec![Block { kind: Kind::Other, text: t }], out);
                } else {
                    let t = self.tabular(&name, body);
                    out.block(Kind::Other, t);
                }
            }
            "equation" | "align" | "flalign" | "alignat" | "xalignat" | "xxalignat" | "eqnarray" | "gather" | "multline" | "displaymath" | "dmath" => self.display(body, &name, out),
            "math" => self.inline_math(body, out),
            "thebibliography" => self.thebibliography(body, out),
            // Drawings: nothing here can draw them.
            "tikzpicture" | "tikzcd" | "pgfpicture" | "picture" | "circuitikz" | "forest" | "pspicture" => {}
            _ => {
                let mut k = 0;
                while let Some((_, e)) = opt_here(body, k) {
                    k = e;
                }
                let args = ENV_ARGS.iter().find(|(n, _)| *n == base).map_or(0, |(_, n)| *n);
                if args > 0 {
                    k = skip_args(body, k, args);
                }
                self.walk(&body[k..], out);
            }
        }
        after
    }

    fn theorem_of(&self, env: &str) -> Option<(String, Option<&'static str>)> {
        const NUMBERED: &[&str] = &["theorem", "lemma", "corollary", "proposition", "conjecture", "definition", "example", "exercise"];
        const OTHERS: &[&str] = &["remark", "note", "claim", "problem", "solution", "observation", "assumption", "notation", "hypothesis", "fact", "question", "answer", "property"];
        if let Some((word, numbered)) = self.meta.theorems.get(env) {
            let kind = if *numbered { kind_of(word).or_else(|| kind_of(env)) } else { None };
            return Some((word.clone(), kind));
        }
        let base = env.trim_end_matches('*');
        let starred = base.len() != env.len();
        if let Some((word, _)) = self.meta.theorems.get(base) {
            return starred.then(|| (word.clone(), None));
        }
        if NUMBERED.contains(&base) {
            return Some((capitalized(base), if starred { None } else { kind_of(base) }));
        }
        OTHERS.contains(&base).then(|| (capitalized(base), None))
    }

    fn theorem(&mut self, word: String, kind: Option<&'static str>, body: &str, out: &mut Emit) {
        let (name, rest) = match opt_at(body, 0) {
            Some((o, e)) => (Some(o.to_string()), &body[e..]),
            None => (None, body),
        };
        let slot = self.slots.len();
        self.slots.push(Vec::new());
        let saved = self.anchor.replace(Anchor::Slot(slot));
        let mut blocks = self.blocks(rest);
        self.anchor = saved;
        let labels = std::mem::take(&mut self.slots[slot]);
        match kind {
            Some(kind) => {
                let id = match labels.first() {
                    Some(l) => self.attach(l, kind),
                    None => self.auto(kind),
                };
                for l in labels.iter().skip(1) {
                    self.alias(l, &id);
                }
                let mut attrs = format!("#{id}");
                if let Some(n) = name {
                    let n = self.plain_text(&n).replace('"', "'");
                    if !n.is_empty() {
                        attrs.push_str(&format!(" name=\"{n}\""));
                    }
                }
                out.block(Kind::Other, div(&format!("{{{attrs}}}"), &blocks));
            }
            None => {
                let lead = match name {
                    Some(n) => format!("**{word} ({}).**", self.inline(&n)),
                    None => format!("**{word}.**"),
                };
                lead_in(&mut blocks, &lead);
                out.block(Kind::Other, div("{.remark}", &blocks));
            }
        }
    }

    fn proof(&mut self, body: &str, out: &mut Emit) {
        let (word, rest) = match opt_at(body, 0) {
            Some((o, e)) => (self.inline(o), &body[e..]),
            None => (self.word("proofname", "Proof"), body),
        };
        let word = word.trim().trim_end_matches('.').to_string();
        let mut blocks = self.blocks(rest);
        lead_in(&mut blocks, &format!("*{word}.*"));
        tail_out(&mut blocks, "∎");
        out.block(Kind::Other, div("{.proof}", &blocks));
    }

    fn abstract_block(&mut self, body: &str, out: &mut Emit) {
        let word = self.word("abstractname", "Abstract");
        let mut blocks = self.blocks(body);
        if blocks.is_empty() {
            return;
        }
        lead_in(&mut blocks, &format!("**{word}.**"));
        out.block(Kind::Other, div("{.abstract}", &blocks));
    }

    fn div_env(&mut self, attrs: &str, body: &str, out: &mut Emit) {
        let blocks = self.blocks(body);
        if blocks.is_empty() {
            return;
        }
        if out.one_line {
            for b in blocks {
                out.push_block(b);
            }
            return;
        }
        out.block(Kind::Other, div(attrs, &blocks));
    }

    fn quote(&mut self, body: &str, out: &mut Emit) {
        let blocks = self.blocks(body);
        if blocks.is_empty() {
            return;
        }
        let text = join(&blocks);
        out.block(Kind::Other, text.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n"));
    }

    fn list(&mut self, env: &str, body: &str, out: &mut Emit) {
        let numbered = matches!(env, "enumerate" | "compactenum" | "asparaenum" | "inparaenum");
        let described = matches!(env, "description" | "compactdesc" | "asparadesc");
        let saved = self.anchor.take();
        let mut items = Vec::new();
        let mut loose = false;
        for (n, part) in split_top(body, "\\item").into_iter().skip(1).enumerate() {
            let (label, content) = match opt_at(part, 0) {
                Some((l, e)) => (Some(l), &part[e..]),
                None => (None, part),
            };
            self.anchor = None;
            let mut blocks = self.blocks(content);
            if let Some(l) = label {
                let l = self.inline(l);
                if !l.is_empty() {
                    let lead = if described && !l.ends_with(':') { format!("**{l}**:") } else { format!("**{l}**") };
                    lead_in(&mut blocks, &lead);
                }
            }
            loose |= blocks.iter().skip(1).any(|b| b.kind != Kind::List);
            let marker = if numbered { format!("{}. ", n + 1) } else { "- ".to_string() };
            items.push(item_text(&marker, &blocks));
        }
        self.anchor = saved;
        if !items.is_empty() {
            out.block(Kind::List, items.join(if loose { "\n\n" } else { "\n" }));
        }
    }

    fn thebibliography(&mut self, body: &str, out: &mut Emit) {
        let start = arg_at(body, 0).map_or(0, |(_, e)| e);
        let mut lines = Vec::new();
        for part in split_top(&body[start..], "\\bibitem").into_iter().skip(1) {
            let mut k = 0;
            if let Some((_, e)) = opt_at(part, 0) {
                k = e;
            }
            let Some((key, e)) = arg_at(part, k) else { continue };
            let n = lines.len() + 1;
            self.bibitems.insert(key.trim().to_string(), n);
            let text = self.inline(&part[e..]);
            lines.push(format!("{n}. {text}"));
        }
        let word = self.word("refname", "References");
        out.block(Kind::Other, format!("# {word} {{.unnumbered}}"));
        out.block(Kind::List, lines.join("\n"));
    }

    // Floats.

    fn float(&mut self, kind: &'static str, env: &str, body: &str, out: &mut Emit) {
        let k = if env.starts_with("wrap") {
            skip_args(body, 0, 2)
        } else {
            opt_at(body, 0).map_or(0, |(_, e)| e)
        };
        self.floats.push(Float::new(kind));
        let mut inner = Emit::default();
        self.walk(&body[k..], &mut inner);
        let f = self.floats.pop().unwrap_or_else(|| Float::new(kind));
        let blocks = inner.finish();
        if kind == "tbl" {
            self.table_out(f, blocks, out);
        } else {
            self.figure_out(f, blocks, out);
        }
    }

    /// A subfigure or subtable, read into the float it is in: its pictures
    /// with its caption, its labels the float's. A `separate` one (a
    /// minipage) with a caption of its own is a figure or table of its own.
    fn sub_float(&mut self, caption: Option<String>, body: &str, frac: Option<f64>, separate: bool, out: &mut Emit) {
        let kind = self.floats.last().map_or("fig", |f| f.kind);
        let mut sub = Float::new(kind);
        sub.caption = caption;
        self.floats.push(sub);
        let mut inner = Emit::default();
        self.walk(body, &mut inner);
        let Some(mut sub) = self.floats.pop() else { return };
        let blocks = inner.finish();
        if separate && sub.caption.is_some() {
            for p in &mut sub.pics {
                p.frac = None;
            }
            let mut unit = Emit::default();
            if kind == "tbl" {
                self.table_out(sub, blocks, &mut unit);
            } else {
                self.figure_out(sub, blocks, &mut unit);
            }
            if let Some(parent) = self.floats.last_mut() {
                parent.units.push(unit.finish());
                parent.unit_frac = parent.unit_frac.or(frac);
            }
            return;
        }
        let caption = {
            let Some(parent) = self.floats.last_mut() else { return };
            parent.subs.extend(sub.own);
            parent.subs.extend(sub.subs);
            if sub.pics.is_empty() {
                sub.caption
            } else {
                let mut pics = sub.pics;
                if pics[0].caption.is_none() {
                    pics[0].caption = sub.caption;
                }
                for p in &mut pics {
                    p.frac = frac.or(p.frac);
                }
                parent.pics.extend(pics);
                None
            }
        };
        for b in blocks {
            out.push_block(b);
        }
        if let Some(c) = caption {
            let t = self.inline(&c);
            if !t.is_empty() {
                out.block(Kind::Para, format!("*{t}*"));
            }
        }
    }

    fn includegraphics(&mut self, src: &str, j: usize, out: &mut Emit) -> usize {
        let mut j = j;
        let mut opts = String::new();
        while let Some((o, e)) = opt_at(src, j) {
            if o.contains('=') || opts.is_empty() {
                opts = o.to_string();
            }
            j = e;
        }
        let Some((path, e)) = arg_at(src, j) else { return j };
        let path = self.picture_path(path);
        let (width, frac) = width_of(&opts);
        match self.floats.last_mut() {
            Some(f) => f.pics.push(Pic { path, width, frac, caption: None }),
            None if self.plain => {}
            None => out.push(&picture(&path, width.as_deref(), "", None)),
        }
        e
    }

    /// A picture's path as written, or found under `\graphicspath` when it
    /// is not beside the document.
    fn picture_path(&self, path: &str) -> String {
        let path = path.trim().trim_matches('"');
        let exists = |p: &Path| p.is_file() || ["pdf", "png", "jpg", "jpeg", "eps", "svg", "gif", "webp"].iter().any(|e| PathBuf::from(format!("{}.{e}", p.display())).is_file());
        if self.meta.graphics.is_empty() || exists(&self.dir.join(path)) {
            return path.to_string();
        }
        for g in &self.meta.graphics {
            let candidate = if g.ends_with('/') { format!("{g}{path}") } else { format!("{g}/{path}") };
            if exists(&self.dir.join(&candidate)) {
                return candidate;
            }
        }
        path.to_string()
    }

    fn caption_cmd(&mut self, name: &str, src: &str, j: usize, star: bool, out: &mut Emit) -> usize {
        let mut j = j;
        if name == "captionof" {
            match arg_at(src, j) {
                Some((_, e)) => j = e,
                None => return j,
            }
        }
        if let Some((_, e)) = opt_at(src, j) {
            j = e;
        }
        let Some((text, e)) = arg_at(src, j) else { return j };
        let (text, labels) = take_labels(text);
        match self.floats.last_mut() {
            Some(f) => {
                f.own.extend(labels);
                if f.caption.is_none() {
                    f.caption = Some(text);
                    f.numbered = !star;
                } else if let Some(p) = f.pics.last_mut() {
                    // A second caption (side by side minipages) names the
                    // picture before it.
                    if p.caption.is_none() {
                        p.caption = Some(text);
                    }
                }
            }
            None => {
                let t = self.inline(&text);
                if !t.is_empty() {
                    out.block(Kind::Para, format!("*{t}*"));
                }
                for l in labels {
                    self.stray_label(&l);
                }
            }
        }
        e
    }

    fn subfloat_cmd(&mut self, name: &str, src: &str, j: usize, out: &mut Emit) -> usize {
        let mut j = j;
        let mut caption = None;
        if name == "subcaptionbox" {
            let Some((c, e)) = arg_at(src, j) else { return j };
            caption = Some(c.to_string());
            j = e;
            if let Some((_, e)) = opt_at(src, j) {
                j = e;
            }
        } else {
            if let Some((c, e)) = opt_at(src, j) {
                caption = Some(c.to_string());
                j = e;
            }
            if let Some((_, e)) = opt_here(src, j) {
                j = e;
            }
        }
        let Some((body, e)) = arg_at(src, j) else { return j };
        if self.floats.is_empty() {
            self.walk(body, out);
        } else {
            self.sub_float(caption, body, None, false, out);
        }
        e
    }

    fn figure_out(&mut self, f: Float, blocks: Vec<Block>, out: &mut Emit) {
        let kind = f.kind;
        if !f.units.is_empty() {
            return self.units_out(f, blocks, out);
        }
        let single = f.pics.len() == 1;
        let captioned = (f.caption.is_some() && f.numbered) || (single && f.pics[0].caption.is_some());
        let id = if let Some(l) = f.own.first() {
            Some(self.attach(l, kind))
        } else if captioned {
            Some(self.auto(kind))
        } else {
            f.subs.first().map(|l| self.attach(l, kind))
        };
        if let Some(id) = &id {
            for l in f.own.iter().chain(&f.subs) {
                self.alias(l, id);
            }
        }
        if f.pics.is_empty() {
            let mut inner = blocks;
            if let Some(c) = &f.caption {
                let t = self.inline(c);
                if !t.is_empty() {
                    inner.push(Block::para(format!("*{t}*")));
                }
            }
            match id {
                Some(id) if !inner.is_empty() => out.block(Kind::Other, div(&format!("{{#{id}}}"), &inner)),
                _ => {
                    for b in inner {
                        out.push_block(b);
                    }
                }
            }
            return;
        }
        if single {
            let p = &f.pics[0];
            let alt = match f.caption.as_ref().or(p.caption.as_ref()) {
                Some(c) => self.caption_text(c),
                None => String::new(),
            };
            out.block(Kind::Para, picture(&p.path, p.width.as_deref(), &alt, id.as_deref()));
            for b in blocks {
                out.push_block(b);
            }
            return;
        }
        // Several pictures: in rows, the caption under them.
        let count = f.pics.len();
        let n = match (f.row, f.pics[0].frac) {
            (Some(r), _) if r >= 1 && r < count => r,
            (_, Some(fr)) if fr > 0.0 => ((1.0 / fr + 1e-6).floor() as usize).clamp(1, count),
            _ if count <= 3 => count,
            _ if count == 4 => 2,
            _ => 3,
        };
        let mut cells = Vec::new();
        for p in &f.pics {
            let alt = match &p.caption {
                Some(c) => self.caption_text(c),
                None => String::new(),
            };
            cells.push(Block::para(picture(&p.path, p.width.as_deref(), &alt, None)));
        }
        let attrs = match &id {
            Some(id) => format!("{{#{id} layout-ncol={n}}}"),
            None => format!("{{layout-ncol={n}}}"),
        };
        out.block(Kind::Other, div(&attrs, &cells));
        if let Some(c) = &f.caption {
            let t = self.inline(c);
            if !t.is_empty() {
                out.block(Kind::Para, format!("*{t}*"));
            }
        }
        for b in blocks {
            out.push_block(b);
        }
    }

    /// A figure holding figures of their own: side by side, each with its
    /// number, the figure's own caption (if it has one) under them.
    fn units_out(&mut self, mut f: Float, blocks: Vec<Block>, out: &mut Emit) {
        let mut cells: Vec<Block> = std::mem::take(&mut f.units).into_iter().flatten().collect();
        cells.extend(f.pics.iter().map(|p| Block::para(picture(&p.path, p.width.as_deref(), "", None))));
        let count = cells.len();
        let n = match f.unit_frac {
            Some(fr) if fr > 0.0 => ((1.0 / fr + 1e-6).floor() as usize).clamp(1, count),
            _ => count.clamp(1, 3),
        };
        let id = match f.own.first() {
            Some(l) => Some(self.attach(l, f.kind)),
            None if f.caption.is_some() && f.numbered => Some(self.auto(f.kind)),
            None => None,
        };
        if let Some(id) = &id {
            for l in f.own.iter().chain(&f.subs) {
                self.alias(l, id);
            }
        }
        if count == 1 && id.is_none() {
            out.push_block(cells.remove(0));
        } else {
            let attrs = match &id {
                Some(id) => format!("{{#{id} layout-ncol={n}}}"),
                None => format!("{{layout-ncol={n}}}"),
            };
            out.block(Kind::Other, div(&attrs, &cells));
        }
        if let Some(c) = &f.caption {
            let t = self.inline(c);
            if !t.is_empty() {
                out.block(Kind::Para, format!("*{t}*"));
            }
        }
        for b in blocks {
            out.push_block(b);
        }
    }

    fn table_out(&mut self, mut f: Float, blocks: Vec<Block>, out: &mut Emit) {
        for b in std::mem::take(&mut f.units).into_iter().flatten() {
            out.push_block(b);
        }
        for b in blocks {
            out.push_block(b);
        }
        for p in &f.pics {
            out.block(Kind::Para, picture(&p.path, p.width.as_deref(), "", None));
        }
        let Some(c) = &f.caption else { return };
        let t = self.inline(c);
        if !f.numbered {
            if !t.is_empty() {
                out.block(Kind::Para, format!("*{t}*"));
            }
            return;
        }
        let id = match f.own.first() {
            Some(l) => self.attach(l, "tbl"),
            None => self.auto("tbl"),
        };
        for l in f.own.iter().chain(&f.subs) {
            self.alias(l, &id);
        }
        out.block(Kind::Other, format!(": {t} {{#{id}}}"));
    }

    // Tables.

    fn tabular(&mut self, env: &str, body: &str) -> String {
        let base = env.trim_end_matches('*');
        let mut j = 0;
        if env == "tabular*" || matches!(base, "tabularx" | "tabulary" | "xltabular") {
            if let Some((_, e)) = arg_at(body, j) {
                j = e;
            }
        }
        if matches!(base, "tabu" | "longtabu") {
            if let Some(k) = body.find('{') {
                if !body[..k].contains('\n') {
                    j = k;
                }
            }
        }
        while let Some((_, e)) = opt_at(body, j) {
            j = e;
        }
        let (spec, e) = arg_at(body, j).unwrap_or(("", j));
        j = e;
        let spec = if spec.contains('=') { option(spec, "colspec").unwrap_or_default() } else { spec.to_string() };
        let aligns = columns(&spec);
        let content = longtable_body(&body[j..]).replace("\\tabularnewline", "\\\\");
        let content = strip_cmds(&content, RULES);
        let mut rows: Vec<Vec<String>> = Vec::new();
        for raw in split_top(&content, "\\\\") {
            let mut raw = raw.strip_prefix('*').unwrap_or(raw);
            if raw.trim_start().starts_with('[') {
                if let Some((_, e)) = opt_at(raw, 0) {
                    raw = &raw[e..];
                }
            }
            if raw.trim().is_empty() {
                continue;
            }
            let mut cells = Vec::new();
            for c in split_top(raw, "&") {
                let (text, span) = self.cell(c);
                cells.push(text);
                for _ in 1..span {
                    cells.push(String::new());
                }
            }
            // A row that held only a caption, a label or a rule.
            if cells.len() == 1 && cells[0].is_empty() {
                continue;
            }
            rows.push(cells);
        }
        pipe_table(&rows, &aligns)
    }

    /// A cell's words on one line, and how many columns it spans.
    fn cell(&mut self, src: &str) -> (String, usize) {
        let t = src.trim();
        if let Some(rest) = t.strip_prefix("\\multicolumn") {
            if let Some((n, e1)) = arg_at(rest, 0) {
                if let Some((_, e2)) = arg_at(rest, e1) {
                    if let Some((text, e3)) = arg_at(rest, e2) {
                        let span = n.trim().parse::<usize>().unwrap_or(1).clamp(1, 64);
                        let content = format!("{text}{}", &rest[e3..]);
                        return (self.cell_text(&content), span);
                    }
                }
            }
        }
        (self.cell_text(t), 1)
    }

    fn cell_text(&mut self, src: &str) -> String {
        let saved = std::mem::replace(&mut self.cell, true);
        let s = self.inline(src);
        self.cell = saved;
        s
    }
}

/// Where math opened before `from` closes with `close`, a backslash's
/// pair skipped.
fn find_math_end(s: &str, from: usize, close: &str) -> Option<usize> {
    let b = s.as_bytes();
    let c = close.as_bytes();
    let mut k = from;
    while k < b.len() {
        if b[k] == b'\\' {
            k += 2;
            continue;
        }
        if b[k..].starts_with(c) {
            return Some(k);
        }
        k += 1;
    }
    None
}

/// What stands from `from` to `close` (`\]`), and where `close` ends. A
/// backslash's pair is skipped, so `\\]` is no close.
fn until<'a>(s: &'a str, from: usize, close: &str) -> (&'a str, usize) {
    let b = s.as_bytes();
    let c = close.as_bytes();
    let mut k = from;
    while k < b.len() {
        if b[k..].starts_with(c) {
            return (&s[from..k], k + c.len());
        }
        k += if b[k] == b'\\' { 2 } else { 1 };
    }
    (&s[from.min(s.len())..], s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The markdown of a document standing in no folder of note.
    fn md(src: &str) -> String {
        to_markdown(src, Path::new("/nonexistent/doc.tex"))
    }

    /// The markdown of a document's body alone.
    fn body(src: &str) -> String {
        md(&format!("\\documentclass{{article}}\n\\begin{{document}}\n{src}\n\\end{{document}}\n"))
    }

    #[test]
    fn a_small_article_has_its_header_abstract_and_sections() {
        let out = md(r"\documentclass{article}
\usepackage{amsmath}
\title{A Study: of Things}
\author{Ada Lovelace\thanks{Funded by nobody.} \\ University of London \and Alan Turing \\ Cambridge \and Grace Hopper}
\date{\today}
\begin{document}
\maketitle
\begin{abstract}
We study things.

More things.
\end{abstract}
\section{Introduction}\label{sec:intro}
Text here, as \autoref{sec:intro} says.
\subsection{Detail}
\section*{Acknowledgments}
Thanks.
\end{document}
Nothing after the end.");
        let today = today();
        assert!(out.starts_with(&format!("---\ntitle: \"A Study: of Things\"\nauthor: Ada Lovelace, Alan Turing and Grace Hopper\ndate: {today}\nnumber-sections: true\n---\n\n")), "{out}");
        assert!(out.contains("::: {.abstract}\n**Abstract.** We study things.\n\nMore things.\n:::"), "{out}");
        assert!(out.contains("\n# Introduction {#sec-intro}\n"), "{out}");
        assert!(out.contains("as @sec-intro says."), "{out}");
        assert!(out.contains("\n## Detail\n"), "{out}");
        assert!(out.contains("\n# Acknowledgments {.unnumbered}\n"), "{out}");
        assert!(!out.contains("maketitle") && !out.contains("Nothing after") && !out.contains("amsmath"), "{out}");
    }

    #[test]
    fn chapters_move_sections_down_a_level_and_starred_ones_alone_are_not_numbered() {
        let out = body("\\chapter{One}\n\\section{Two \\texorpdfstring{$\\alpha$}{alpha}}\n\\subsection{Three}\n\\paragraph{Run in}\n\nThe words.");
        assert!(out.contains("# One\n\n## Two $\\alpha$\n\n### Three\n\n**Run in.** The words."), "{out}");
        let out = body("\\section*{Only}\nText.");
        assert!(!out.contains("number-sections"), "{out}");
    }

    #[test]
    fn emphasis_and_markdown_signs_are_written_and_escaped() {
        let out = body(r"\emph{word} and \textbf{bold} and \texttt{a\_b} and \sout{gone} and \underline{under} and \hl{marked}.
Five * three is 5\_3 or \$15 for user@example.com, 50\%.

\#1 starts this line. % a comment that goes
Joined%
words, a~b, 1--2, yes---no, ``quoted'' and `single' and it's.

- not a list, 2024. not a list either");
        assert!(out.contains("*word* and **bold** and `a_b` and ~~gone~~ and <u>under</u> and <mark>marked</mark>."), "{out}");
        assert!(out.contains(r"Five \* three is 5\_3 or \$15 for user\@example.com, 50%."), "{out}");
        assert!(out.contains("\\#1 starts this line. Joinedwords, a\u{a0}b, 1–2, yes—no, “quoted” and ‘single’ and it’s."), "{out}");
        assert!(out.contains("\\- not a list, 2024. not a list either"), "{out}");
        assert!(!out.contains("comment"), "{out}");
        let out = body("2024. That year.");
        assert!(out.contains("2024\\. That year."), "{out}");
    }

    #[test]
    fn accents_and_symbols_are_their_characters() {
        let out = body(r#"Caf\'e, na\"{\i}ve, \c{c}a, \v s, \~n, \ss, \aa, \o, \S 2, \copyright\ \LaTeX, \ldots, \textbackslash."#);
        assert!(out.contains("Café, naïve, ça, š, ñ, ß, å, ø, § 2, © LaTeX, …, \\\\."), "{out}");
    }

    #[test]
    fn a_line_break_is_a_hard_break_inside_a_paragraph_only() {
        let out = body("One\\\\\ntwo\\\\[2pt] three\\\\");
        assert!(out.contains("One\\\ntwo\\\nthree"), "{out}");
        assert!(!out.contains("three\\"), "{out}");
    }

    #[test]
    fn lists_nest_with_four_spaces() {
        let out = body(r"\begin{enumerate}
\item First
  \begin{itemize}
  \item a
  \item b
  \end{itemize}
\item Second
\end{enumerate}
\begin{description}
\item[Term] its meaning
\end{description}
\begin{itemize}
\item[x] marked
\end{itemize}");
        assert!(out.contains("1. First\n    - a\n    - b\n2. Second"), "{out}");
        assert!(out.contains("- **Term**: its meaning"), "{out}");
        assert!(out.contains("- **x** marked"), "{out}");
    }

    #[test]
    fn quotes_center_and_unknown_things() {
        let out = body(r"\begin{quote}
Said once.

Said twice.
\end{quote}
\begin{center}
Middle
\end{center}
\foo{bar} \foo[x]{a}{b} \baz{} end.
\begin{myenv}[opt]
inside
\end{myenv}
\begin{tikzpicture}\draw (0,0) -- (1,1);\end{tikzpicture}");
        assert!(out.contains("> Said once.\n>\n> Said twice."), "{out}");
        assert!(out.contains("::: {style=\"text-align: center\"}\nMiddle\n:::"), "{out}");
        assert!(out.contains("bar b end."), "{out}");
        assert!(out.contains("bar b end. inside"), "{out}");
        assert!(!out.contains("draw") && !out.contains("myenv") && !out.contains("opt"), "{out}");
    }

    #[test]
    fn a_figure_is_a_picture_with_its_caption_width_and_id() {
        let out = body(r"\begin{figure}[ht]
\centering
\includegraphics[width=0.5\textwidth]{plots/result.png}
\caption{The result of \emph{it}.}
\label{fig:result}
\end{figure}
See Figure~\ref{fig:result} and \autoref{fig:result}, not \ref{fig:none}.
\begin{figure}
\includegraphics[width=8cm]{b}
\caption{Unlabeled.}
\end{figure}
\includegraphics[scale=0.5]{inline.png}");
        assert!(out.contains("![The result of *it*.](plots/result.png){#fig-result width=50%}"), "{out}");
        assert!(out.contains("See Figure\u{a0}[-@fig-result] and @fig-result, not **??**."), "{out}");
        assert!(out.contains("![Unlabeled.](b){#fig-auto-1 width=8cm}"), "{out}");
        assert!(out.contains("![](inline.png){width=50%}"), "{out}");
    }

    #[test]
    fn subfigures_are_a_row_with_the_caption_under_it() {
        let out = body(r"\begin{figure}
\begin{subfigure}[b]{0.45\textwidth}
\includegraphics[width=\textwidth]{a.png}
\caption{Left}\label{fig:a}
\end{subfigure}
\hfill
\begin{subfigure}[b]{0.45\textwidth}
\includegraphics[width=\textwidth]{b.png}
\caption{Right}
\end{subfigure}
\caption{Both.}\label{fig:both}
\end{figure}
As in \cref{fig:a}.");
        assert!(out.contains("::: {#fig-both layout-ncol=2}\n![Left](a.png){width=100%}\n\n![Right](b.png){width=100%}\n:::\n\n*Both.*"), "{out}");
        assert!(out.contains("As in @fig-both."), "{out}");
    }

    #[test]
    fn a_table_is_a_pipe_table_with_its_caption_line() {
        let out = body(r"\begin{table}[h]
\centering
\caption{Data, \textit{measured}.}\label{tab:data}
\begin{tabular}{l|cr}
\toprule
Name & \multicolumn{2}{c}{Values} \\
\midrule
a & 1 & $|x|$ \\
b\textsuperscript{2} & 3 & \texttt{a|b} \\ \cmidrule(lr){2-3}
\bottomrule
\end{tabular}
\end{table}
Table~\ref{tab:data}.
\begin{tabular}{cc}
x & y \\
\end{tabular}");
        assert!(out.contains("| Name | Values |  |\n|:---|:---:|---:|\n| a | 1 | $\\vert x\\vert $ |\n| b² | 3 | `a\\|b` |\n\n: Data, *measured*. {#tbl-data}"), "{out}");
        assert!(out.contains("Table\u{a0}[-@tbl-data]."), "{out}");
        assert!(out.contains("| x | y |\n|:---:|:---:|"), "{out}");
    }

    #[test]
    fn equations_are_numbered_by_their_labels() {
        let out = body(r"\begin{equation}\label{eq:energy}
E = mc^2
\end{equation}
By \eqref{eq:energy} and \ref{eq:energy}, with $ a+b $ inline.
\[ a + b \]
\begin{align}
x &= 1 \nonumber \\
y &= 2 \label{eq:y}
\end{align}
\begin{align*}
p &= q
\end{align*}
\begin{equation}
z = 1
\end{equation}
\begin{equation}
w = 2 \nonumber
\end{equation}
\begin{equation}
v = 3 \tag{$\star$} \label{eq:star}
\end{equation}
See \eqref{eq:y} and \eqref{eq:star}.");
        assert!(out.contains("$$\nE = mc^2\n$$ {#eq-energy}"), "{out}");
        assert!(out.contains("By ([-@eq-energy]) and [-@eq-energy], with $a+b$ inline."), "{out}");
        assert!(out.contains("$$\na + b\n$$\n"), "{out}");
        assert!(out.contains("$$\n\\begin{aligned}\nx &= 1  \\\\\ny &= 2\n\\end{aligned}\n$$ {#eq-y}"), "{out}");
        assert!(out.contains("$$\n\\begin{aligned}\np &= q\n\\end{aligned}\n$$\n"), "{out}");
        assert!(out.contains("$$\nz = 1\n$$ {#eq-auto-1}"), "{out}");
        assert!(out.contains("$$\nw = 2\n$$\n"), "{out}");
        assert!(out.contains("v = 3 \\tag{$\\star$}\n$$\n"), "{out}");
        assert!(out.contains("See ([-@eq-y]) and ($\\star$)."), "{out}");
    }

    #[test]
    fn theorems_remarks_and_proofs_are_divs() {
        let out = md(r"\documentclass{article}
\newtheorem{thm}{Theorem}
\newtheorem{lem}[thm]{Lemma}
\newtheorem{remark}{Remark}
\newtheorem*{claim}{Claim}
\begin{document}
\begin{thm}[Main result]\label{thm:main}
Every thing is a thing.
\end{thm}
\begin{proof}
Obvious.
\end{proof}
\begin{lem}
Small.
\end{lem}
\begin{remark}
Note this.
\end{remark}
\begin{claim}[Strong]
Said.
\end{claim}
\begin{proof}[Proof of \cref{thm:main}]
Done.

\[ x = y \]
\end{proof}
\begin{definition}
Undeclared.
\end{definition}
By \cref{thm:main}.
\end{document}");
        assert!(out.contains("::: {#thm-main name=\"Main result\"}\nEvery thing is a thing.\n:::"), "{out}");
        assert!(out.contains("::: {.proof}\n*Proof.* Obvious. ∎\n:::"), "{out}");
        assert!(out.contains("::: {#lem-auto-1}\nSmall.\n:::"), "{out}");
        assert!(out.contains("::: {.remark}\n**Remark.** Note this.\n:::"), "{out}");
        assert!(out.contains("::: {.remark}\n**Claim (Strong).** Said.\n:::"), "{out}");
        assert!(out.contains("::: {.proof}\n*Proof of @thm-main.* Done.\n\n$$\nx = y\n$$\n\n∎\n:::"), "{out}");
        assert!(out.contains("::: {#def-auto-1}\nUndeclared.\n:::"), "{out}");
        assert!(out.contains("By @thm-main."), "{out}");
    }

    #[test]
    fn footnotes_are_defined_at_the_end() {
        let out = body("Text\\footnote{A \\emph{note}.} more \\footnote{Second.}.\n\nLast paragraph.");
        assert!(out.contains("Text[^1] more[^2]."), "{out}");
        assert!(out.ends_with("Last paragraph.\n\n[^1]: A *note*.\n\n[^2]: Second.\n"), "{out}");
    }

    #[test]
    fn citations_take_pandoc_forms_and_the_bibliography_the_refs_div() {
        let out = body(r"\cite{a} \citep{a,b} \citep[p.~5]{a} \citep[see][p.~5]{a} \citep[see][]{a}
\citet{a} \citet{a,b} \textcite{a} \citeauthor{a} \citeyear{a} \parencite{b}.\nocite{c}
\bibliographystyle{plain}
\bibliography{refs,more}");
        assert!(out.contains("bibliography: refs.bib\n"), "{out}");
        assert!(out.contains("[@a] [@a; @b] [@a, p. 5] [see @a, p. 5] [see @a] @a @a and @b @a @a [-@a] [@b]."), "{out}");
        assert!(out.contains("::: {#refs}\n:::"), "{out}");
        assert!(!out.contains("plain"), "{out}");
        let out = md("\\documentclass{article}\n\\usepackage{biblatex}\n\\addbibresource{works.bib}\n\\begin{document}\n\\autocite{k}\n\\printbibliography\n\\end{document}");
        assert!(out.contains("bibliography: works.bib\n") && out.contains("[@k]") && out.contains("::: {#refs}\n:::"), "{out}");
    }

    #[test]
    fn a_bibliography_of_its_own_is_a_numbered_list_cited_by_number() {
        let out = body(r"As shown in \cite{knuth} and \cite{knuth,lamport}.
\begin{thebibliography}{9}
\bibitem{knuth} D. Knuth. \emph{The \TeX book}. 1984.
\bibitem[L94]{lamport} L. Lamport.
\newblock \LaTeX. 1994.
\end{thebibliography}");
        assert!(out.contains("As shown in [1] and [1, 2]."), "{out}");
        assert!(out.contains("# References {.unnumbered}\n\n1. D. Knuth. *The TeX book*. 1984.\n2. L. Lamport. LaTeX. 1994."), "{out}");
        assert!(!out.contains("bibliography:"), "{out}");
    }

    #[test]
    fn macros_are_expanded_in_text_and_math() {
        let out = md(r"\documentclass{article}
\newcommand{\R}{\mathbb{R}}
\newcommand{\norm}[1]{\left\| #1 \right\|}
\newcommand{\pair}[2][0]{(#1, #2)}
\def\name{Emaki}
\DeclareMathOperator{\argmax}{arg\,max}
\newcommand{\loop}{\loop x}
\newenvironment{note}{\begin{quote}\textbf{Note:}}{\end{quote}}
\begin{document}
\newcommand{\inbody}{body words}
$\norm{x} \in \R^n$, $\pair{y}$ and $\pair[1]{y}$, $\argmax_x f$. \name{} and \inbody.
\begin{note}
Careful.
\end{note}
\loop
\end{document}");
        assert!(out.contains(r"$\left\| x \right\| \in \mathbb{R}^n$, $(0, y)$ and $(1, y)$, $\operatorname{arg\,max}_x f$. Emaki and body words."), "{out}");
        assert!(out.contains("> **Note:** Careful."), "{out}");
        assert!(!out.contains("newcommand") && !out.contains("inbody"), "{out}");
    }

    #[test]
    fn code_is_left_as_it_is() {
        let out = md(r"\documentclass{article}
\lstset{language=R}
\begin{document}
\begin{verbatim}
a_b *c* $d$ -- % not a comment
\end{verbatim}
\begin{lstlisting}[language=Python, caption={A listing}]
def f(x): return x**2  # ``quoted''
\end{lstlisting}
\begin{lstlisting}
x <- 1
\end{lstlisting}
\begin{minted}{C++}
int main() {}
\end{minted}
Inline \verb|a_b%c| and \lstinline{x_1}.
% \begin{verbatim}
\end{document}");
        assert!(out.contains("```\na_b *c* $d$ -- % not a comment\n```"), "{out}");
        assert!(out.contains("*A listing*\n\n```python\ndef f(x): return x**2  # ``quoted''\n```"), "{out}");
        assert!(out.contains("```r\nx <- 1\n```"), "{out}");
        assert!(out.contains("```cpp\nint main() {}\n```"), "{out}");
        assert!(out.contains("Inline `a_b%c` and `x_1`."), "{out}");
    }

    #[test]
    fn links_are_links() {
        let out = body(r"\href{https://example.com/a\_b?x=1\#top}{the \emph{site}} and \url{https://example.com/100%} and \url{www.example.com}.");
        assert!(out.contains("[the *site*](https://example.com/a_b?x=1#top) and <https://example.com/100%> and [www.example.com](https://www.example.com)."), "{out}");
    }

    #[test]
    fn input_pastes_a_file_in_and_never_loops() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("parts")).unwrap();
        std::fs::write(dir.path().join("parts/chapter.tex"), "\\section{From the chapter}\nChapter words.%\n\\input{main}\n\\input{parts/deeper}").unwrap();
        std::fs::write(dir.path().join("parts/deeper.tex"), "Deeper \\emph{words}.").unwrap();
        let main = dir.path().join("main.tex");
        let src = "\\documentclass{article}\n\\begin{document}\nBefore.\n\n\\input{parts/chapter}\n\n\\include{missing}\nAfter.\n\\end{document}\n";
        std::fs::write(&main, src).unwrap();
        let out = to_markdown(src, &main);
        assert!(out.contains("Before.\n\n# From the chapter\n\nChapter words. Deeper *words*.\n\nAfter."), "{out}");
        assert_eq!(out.matches("Before.").count(), 1, "{out}");
    }

    #[test]
    fn a_nameref_is_the_sections_title() {
        let out = to_markdown("\\documentclass{article}\n\\begin{document}\n\\section{Model \\emph{Fit}}\\label{sec:fit}\nSee the \\nameref{sec:fit} section and \\autoref{sec:fit}.\n\\end{document}\n", Path::new("/no/such/doc.tex"));
        assert!(out.contains("See the Model Fit section and @sec-fit."), "{out}");
    }

    #[test]
    fn cross_references_through_quarto_read_figure_1_equation_1_and_the_citation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("refs.bib"), "@article{smith2020,\n  author = {Smith, John},\n  title = {A Paper},\n  journal = {Journal},\n  year = {2020}\n}\n").unwrap();
        let file = dir.path().join("paper.tex");
        let src = r"\documentclass{article}
\title{Paper}
\begin{document}
\section{Results}\label{sec:results}
\begin{figure}
\includegraphics{plot.png}
\caption{A plot.}\label{fig:plot}
\end{figure}
\begin{equation}\label{eq:main}
a = b
\end{equation}
As \autoref{fig:plot} and \eqref{eq:main} show \citep{smith2020}, per \cref{sec:results}.
\bibliography{refs}
\end{document}";
        let md = to_markdown(src, &file);
        let out = crate::quarto::expand(&md, &file);
        assert!(out.contains("![Figure 1: A plot.](plot.png)"), "{out}");
        assert!(out.contains("\\tag{1}"), "{out}");
        assert!(out.contains("As Figure 1 and (1) show (Smith 2020), per Section 1."), "{out}");
        assert!(out.contains("Smith, J. (2020). A Paper. *Journal*."), "{out}");
    }
    #[test]
    fn iffalse_blocks_and_footnote_marks() {
        let out = body("Shown.\n\\iffalse\nHidden \\ifx a b nested \\fi too.\n\\fi\n\\iffalse gone \\else kept \\fi.\nMark\\footnotemark{} here.\\footnotetext{The mark's text.}");
        assert!(out.contains("Shown. kept . Mark[^1] here."), "{out}");
        assert!(!out.contains("Hidden") && !out.contains("gone"), "{out}");
        assert!(out.ends_with("[^1]: The mark’s text.\n"), "{out}");
    }

    #[test]
    fn side_by_side_minipages_are_figures_of_their_own() {
        let out = body(r"\begin{figure}
\begin{minipage}{0.48\textwidth}
\includegraphics[width=\linewidth]{cat.png}
\caption{A cat.}\label{fig:cat}
\end{minipage}\hfill
\begin{minipage}{0.48\textwidth}
\includegraphics[width=\linewidth]{dog.png}
\caption{A dog.}\label{fig:dog}
\end{minipage}
\end{figure}
\begin{figure}
\subfloat[First]{\includegraphics{p1}}
\subfloat[Second]{\includegraphics{p2}}\\
\subfloat[Third]{\includegraphics{p3}}
\caption{Three.}
\end{figure}
See \cref{fig:cat,fig:dog}.");
        assert!(out.contains("::: {layout-ncol=2}\n![A cat.](cat.png){#fig-cat width=100%}\n\n![A dog.](dog.png){#fig-dog width=100%}\n:::"), "{out}");
        assert!(out.contains("::: {#fig-auto-1 layout-ncol=2}\n![First](p1)\n\n![Second](p2)\n\n![Third](p3)\n:::\n\n*Three.*"), "{out}");
        assert!(out.contains("See @fig-cat and @fig-dog."), "{out}");
    }

    #[test]
    fn a_reference_in_a_pictures_caption_keeps_the_caption_free_of_brackets() {
        let out = body(r"\begin{figure}
\includegraphics{a.png}
\caption{As in Fig.~\ref{fig:b} and \cite{smith}.}
\label{fig:a}
\end{figure}
\begin{figure}
\includegraphics{b.png}
\caption{B.\footnote{Dropped.}}
\label{fig:b}
\end{figure}");
        assert!(out.contains("![As in @fig-b and @smith.](a.png){#fig-a}"), "{out}");
        assert!(out.contains("![B.](b.png){#fig-b}"), "{out}");
    }

    #[test]
    fn a_long_table_is_its_own_float() {
        let out = body(r"\begin{longtable}{lc}
\caption{Long one.}\label{tab:long}\\
\hline
H1 & H2 \\
\hline
\endfirsthead
H1 & H2 (cont.) \\
\endhead
x & 1 \\
\end{longtable}
\begin{table}
\begin{tabularx}{\linewidth}{Xr}
a & b \\
\end{tabularx}
\caption*{Not numbered.}
\end{table}
See \cref{tab:long}.");
        assert!(out.contains("| H1 | H2 |\n|:---|:---:|\n| x | 1 |\n\n: Long one. {#tbl-long}"), "{out}");
        assert!(!out.contains("cont."), "{out}");
        assert!(out.contains("| a | b |\n|:---|---:|\n\n*Not numbered.*"), "{out}");
        assert!(out.contains("See @tbl-long."), "{out}");
    }

    #[test]
    fn pictures_are_found_on_the_graphics_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("figures")).unwrap();
        std::fs::write(dir.path().join("figures/cat.png"), b"").unwrap();
        let file = dir.path().join("doc.tex");
        let out = to_markdown("\\graphicspath{{figures/}}\n\\begin{document}\n\\includegraphics{cat}\n\\includegraphics{elsewhere}\n\\end{document}", &file);
        assert!(out.contains("![](figures/cat) ![](elsewhere)"), "{out}");
    }

    #[test]
    fn inputs_nest_at_most_eight_deep() {
        let dir = tempfile::tempdir().unwrap();
        for n in 1..=10 {
            std::fs::write(dir.path().join(format!("c{n}.tex")), format!("L{n} \\input{{c{}}}", n + 1)).unwrap();
        }
        let file = dir.path().join("main.tex");
        let out = to_markdown("\\input{c1}", &file);
        assert!(out.contains("L1 L2 L3 L4 L5 L6 L7 L8"), "{out}");
        assert!(!out.contains("L9"), "{out}");
    }

    #[test]
    fn nothing_runs_forever_or_runs_the_stack_out() {
        let out = md("\\newcommand{\\a}{\\b x}\\newcommand{\\b}{\\a y}\\begin{document}\\a and $\\a$\\end{document}");
        assert!(out.contains("and"), "{out}");
        let deep = format!("{}deep{}", "{".repeat(5000), "}".repeat(5000));
        assert!(body(&deep).contains("deep"));
        let lists = format!("{}x{}", "\\begin{itemize}\\item ".repeat(300), "\\end{itemize}".repeat(300));
        assert!(body(&lists).contains('x'));
        let emph = format!("{}y{}", "\\emph{\\textbf{".repeat(300), "}}".repeat(300));
        assert!(body(&emph).contains('y'));
        let open = body("\\begin{figure}\\caption{unclosed \\textbf{x");
        assert!(open.contains('x'), "{open}");
        for bad in ["$", "$$", "\\[", "\\", "{", "}", "\\begin{", "\\verb|", "\\item[", "&", "\\\\", "%", "\\iffalse", "\\section", "\\href{a}", "\\'", "\\begin{tabular}", "\\multicolumn{2}"] {
            let _ = body(bad);
        }
    }

    #[test]
    fn yaml_values_are_quoted_when_yaml_would_misread_them() {
        assert_eq!(yaml("Plain title"), "Plain title");
        assert_eq!(yaml("A: B"), "\"A: B\"");
        assert_eq!(yaml("#1 \"best\" \\o/"), "\"#1 \\\"best\\\" \\\\o/\"");
        assert_eq!(yaml("- dash"), "\"- dash\"");
    }

    #[test]
    fn labels_take_quarto_ids_by_what_they_are_on() {
        assert_eq!(id_for("fig:results", "fig"), "fig-results");
        assert_eq!(id_for("tab:data", "tbl"), "tbl-data");
        assert_eq!(id_for("intro", "sec"), "sec-intro");
        assert_eq!(id_for("eq1", "eq"), "eq-eq1");
        assert_eq!(id_for("results:main.v2", "fig"), "fig-results-main-v2");
    }
}
