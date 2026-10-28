//! A Quarto document made ready to read without `quarto render`: what
//! its shortcodes pull in from other files is pulled in here, so the
//! window can draw the one document as it is written, with the rest of
//! the syntax (fenced divs, attributes, chunks) drawn by the view.
//!
//! Quarto's own rules, from quarto.org/docs/authoring:
//! - `{{< include path >}}` pastes the file in. A path with a leading
//!   slash is from the project root (the folder of `_quarto.yml`); any
//!   other is relative to the main document, not to the included file,
//!   and so are the paths inside what is included. An included file's
//!   header is metadata, not text, and is left out.
//! - `{{< var a.b >}}` is a value of the project's `_variables.yml`,
//!   markdown or plain; `{{< meta a.b >}}` one of the document's header,
//!   else of `_quarto.yml`; `{{< env NAME "fallback" >}}` an environment
//!   variable. A dotted key walks into nested maps, and a number into a
//!   list (from 1).
//! - `{{{< ... >}}}` is the shortcode shown, not run.
//!
//! - Cross-references: a label (`{#fig-x}` on a picture or a div,
//!   `{#tbl-x}` on a table's caption line, `{#sec-x}` on a heading,
//!   `{#eq-x}` after display math, `#thm-`, `#lem-`… on a div, and a
//!   chunk's `#| label: fig-x`) is numbered by kind in the order it
//!   comes, and `@fig-x` reads "Figure 1", `[-@fig-x]` "1". A label not
//!   found reads `?@fig-x` in bold, as Quarto writes it. A caption gets
//!   its number ("Figure 1: …"), an equation `(1)`. A section is its
//!   number with `number-sections`, else its title.
//! - Citations, when the header (or `_quarto.yml`) names a
//!   `bibliography` (BibTeX or CSL JSON): `[@key, p. 3; @other]` reads
//!   "(Smith 2020, p. 3; Doe and Roe 2021)", `@key` "Smith (2020)",
//!   `[-@key]` "(2020)", a key not found `key?` in bold, and the works
//!   cited are listed at the end, under a heading or in `::: {#refs}`.
//!   With no bibliography an `@word` is left alone: it is no citation.
//! - A theorem-like div (`::: {#thm-x name="Name"}`) opens with its lead
//!   in bold, "Theorem 1 (Name)"; a callout with an id of its kind
//!   (`#tip-x`) is titled "Tip 1: …". With `number-sections` each heading
//!   carries its number, `{.unnumbered}` or `{-}` none.
//! - A span `[words]{.aside}` is a margin note: it leaves its paragraph
//!   and stands just before it as `::: {.column-margin}`, which the view
//!   sets level with the paragraph.
//! - The words these use ("Figure", "References") are Quarto's: its
//!   English, or what the header or `_quarto.yml` sets
//!   (`crossref: fig-title:`, `language: crossref-fig-title:`).
//!
//! What cannot be resolved (a file not there, a key unknown, a shortcode
//! of another kind) is left as written, and the view draws it as one.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use regex::Regex;

/// The most includes nested in one another that are followed.
const DEPTH: usize = 8;

/// The folder a Quarto project is in: the nearest ancestor of `file`
/// holding `_quarto.yml`. None for a document outside any project.
pub fn project_root(file: &Path) -> Option<PathBuf> {
    file.ancestors().skip(1).find(|d| d.join("_quarto.yml").is_file() || d.join("_quarto.yaml").is_file()).map(Path::to_path_buf)
}

/// `text`, the document at `file`, with its includes pasted in and its
/// variables and metadata filled in.
pub fn expand(text: &str, file: &Path) -> String {
    let root = project_root(file);
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let variables = root.as_ref().and_then(|r| std::fs::read_to_string(r.join("_variables.yml")).ok()).unwrap_or_default();
    let project = root.as_ref().and_then(|r| std::fs::read_to_string(r.join("_quarto.yml")).or_else(|_| std::fs::read_to_string(r.join("_quarto.yaml"))).ok()).unwrap_or_default();
    let header = header_text(text).to_string();
    let doc = Doc { dir, root, variables, project, header };
    let mut seen = HashSet::new();
    seen.insert(file.to_path_buf());
    let text = doc.expand(text, 0, &mut seen);
    doc.references(&text)
}

struct Doc {
    dir: PathBuf,
    root: Option<PathBuf>,
    variables: String,
    project: String,
    header: String,
}

impl Doc {
    fn expand(&self, text: &str, depth: usize, seen: &mut HashSet<PathBuf>) -> String {
        static CODE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"\{\{\{<(.*?)>\}\}\}|\{\{<\s*([^\s>]+)\s*(.*?)\s*>\}\}").unwrap());
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for cap in CODE.captures_iter(text) {
            let whole = cap.get(0).unwrap();
            out.push_str(&text[at..whole.start()]);
            at = whole.end();
            if let Some(escaped) = cap.get(1) {
                out.push_str(&format!("{{{{<{}>}}}}", escaped.as_str()));
                continue;
            }
            let (name, args) = (cap.get(2).map_or("", |m| m.as_str()), cap.get(3).map_or("", |m| m.as_str()));
            match self.resolve(name, args, depth, seen) {
                Some(value) => out.push_str(&value),
                None => out.push_str(whole.as_str()),
            }
        }
        out.push_str(&text[at..]);
        out
    }

    fn resolve(&self, name: &str, args: &str, depth: usize, seen: &mut HashSet<PathBuf>) -> Option<String> {
        let words: Vec<String> = split_args(args);
        match name {
            "include" => {
                let path = self.path_of(words.first()?);
                if depth >= DEPTH || !seen.insert(path.clone()) {
                    return None;
                }
                let text = std::fs::read_to_string(&path).ok()?;
                let body = match crate::files::front_matter(&text) {
                    Some((_, rest)) => rest,
                    None => text.as_str(),
                };
                let expanded = self.expand(body.trim_end_matches('\n'), depth + 1, seen);
                seen.remove(&path);
                Some(expanded)
            }
            "var" => yaml_lookup(&self.variables, words.first()?),
            "meta" => {
                let key = words.first()?;
                yaml_lookup(&self.header, key).or_else(|| yaml_lookup(&self.project, key))
            }
            "env" => {
                let key = words.first()?;
                std::env::var(key).ok().or_else(|| words.get(1).cloned())
            }
            _ => None,
        }
    }

    /// Where an included file is: from the project root with a leading
    /// slash, else beside the main document.
    fn path_of(&self, given: &str) -> PathBuf {
        match given.strip_prefix('/') {
            Some(rest) => self.root.clone().unwrap_or_else(|| self.dir.clone()).join(rest),
            None => self.dir.join(given),
        }
    }
}

/// A cross-reference's kind: its label's prefix and Quarto's word for it.
const KINDS: &[(&str, &str)] = &[
    ("fig", "Figure"),
    ("tbl", "Table"),
    ("sec", "Section"),
    ("eq", "Equation"),
    ("lst", "Listing"),
    ("thm", "Theorem"),
    ("lem", "Lemma"),
    ("cor", "Corollary"),
    ("prp", "Proposition"),
    ("cnj", "Conjecture"),
    ("def", "Definition"),
    ("exm", "Example"),
    ("exr", "Exercise"),
    ("nte", "Note"),
    ("tip", "Tip"),
    ("wrn", "Warning"),
    ("imp", "Important"),
    ("cau", "Caution"),
];

/// The kinds a theorem-like div takes its lead from ("**Theorem 1
/// (Name)**"), and those a callout takes its title from ("Tip 1: …").
const THEOREMS: &[&str] = &["thm", "lem", "cor", "prp", "cnj", "def", "exm", "exr"];
const CALLOUTS: &[&str] = &["nte", "tip", "wrn", "imp", "cau"];

/// A work in the bibliography: who, when, and what, as cited and listed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Work {
    pub key: String,
    /// Family names, in order.
    pub authors: Vec<String>,
    /// Full names as listed: "Family, G.".
    pub listed: Vec<String>,
    pub year: String,
    pub title: String,
    pub container: String,
    pub volume: String,
    pub issue: String,
    pub pages: String,
    pub doi: String,
    pub url: String,
}

impl Work {
    /// How it is named in the text: one author, two, or the first and
    /// "et al.".
    fn named(&self) -> String {
        match self.authors.as_slice() {
            [] => self.title.clone(),
            [a] => a.clone(),
            [a, b] => format!("{a} and {b}"),
            [a, ..] => format!("{a} et al."),
        }
    }

    /// Its line in the list of works cited, in markdown.
    fn entry(&self) -> String {
        let who = match self.listed.as_slice() {
            [] => String::new(),
            [a] => a.clone(),
            [rest @ .., last] => format!("{}, & {last}", rest.join(", ")),
        };
        let mut out = format!("{who} ({}). {}.", self.year, self.title.trim_end_matches('.'));
        if !self.container.is_empty() {
            out.push_str(&format!(" *{}*", self.container));
            if !self.volume.is_empty() {
                out.push_str(&format!(", {}", self.volume));
                if !self.issue.is_empty() {
                    out.push_str(&format!("({})", self.issue));
                }
            }
            if !self.pages.is_empty() {
                out.push_str(&format!(", {}", self.pages.replace("--", "–")));
            }
            out.push('.');
        }
        if !self.doi.is_empty() {
            out.push_str(&format!(" <https://doi.org/{}>", self.doi.trim_start_matches("https://doi.org/")));
        } else if !self.url.is_empty() {
            out.push_str(&format!(" <{}>", self.url));
        }
        out
    }
}

impl Doc {
    /// A setting of the header, else of `_quarto.yml`.
    fn setting(&self, key: &str) -> Option<String> {
        yaml_lookup(&self.header, key).or_else(|| yaml_lookup(&self.project, key))
    }

    /// Quarto's word for a kind, or the one the document sets.
    fn word(&self, kind: &str, default: &str) -> String {
        self.setting(&format!("crossref.{kind}-title")).or_else(|| self.setting(&format!("language.crossref-{kind}-title"))).unwrap_or_else(|| default.to_string())
    }

    /// The works of the bibliography the document names, by key.
    fn works(&self) -> Option<std::collections::HashMap<String, Work>> {
        let named = yaml_lookup(&self.header, "bibliography").map(|v| (v, self.dir.clone())).or_else(|| yaml_lookup(&self.project, "bibliography").map(|v| (v, self.root.clone().unwrap_or_else(|| self.dir.clone()))))?;
        let (value, base) = named;
        let mut works = std::collections::HashMap::new();
        for line in value.lines() {
            let path = unquote(line.trim().trim_start_matches("- ").trim());
            if path.is_empty() {
                continue;
            }
            let path = match path.strip_prefix('/') {
                Some(rest) => self.root.clone().unwrap_or_else(|| base.clone()).join(rest),
                None => base.join(path),
            };
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let found = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) { csl_json(&text) } else { bibtex(&text) };
            for w in found {
                works.insert(w.key.clone(), w);
            }
        }
        Some(works)
    }

    /// Cross-references and citations resolved (see the module's head).
    fn references(&self, text: &str) -> String {
        let (head, body) = text.split_at(header_len(text));
        let body = &asides(body);
        let labels = self.labels(body);
        let works = self.works();
        let mut cited: Vec<String> = Vec::new();
        let mut out = String::with_capacity(body.len());
        let mut fence: Option<String> = None;
        static FIG: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"!\[((?:[^\[\]]|\[[^\[\]]*\])*)\](\([^)]*\)\{[^}]*#((?:fig)-[\w-]+)[^}]*\})").unwrap());
        static TBL: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^(?:Table)?:\s+(.*?)\s*\{[^}]*#(tbl-[\w-]+)[^}]*\}\s*$").unwrap());
        static EQ: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"\$\$\s*\{#(eq-[\w-]+)\}\s*$").unwrap());
        static FENCE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^(\s*:::+\s*\{)(.*)\}\s*$").unwrap());
        static DIV_ID: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"#((\w+)-[\w-]+)").unwrap());
        static NAME: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r#"\b(name|title)="([^"]*)""#).unwrap());
        static HEAD: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.*?)\s*(\{[^}]*\})?\s*$").unwrap());
        let numbered = self.setting("number-sections").is_some_and(|v| v == "true");
        let top = top_level(body);
        let mut sections: Vec<usize> = Vec::new();
        // A theorem's lead, for the first line of words in it; a callout's
        // number, for a heading that is its title.
        let mut theorem: Option<String> = None;
        let mut titled: Option<String> = None;
        let titles = callout_titles(body);
        for (ix, line) in body.split_inclusive('\n').enumerate() {
            let bare = line.trim_end_matches(['\n', '\r']);
            let nl = &line[bare.len()..];
            let lead = bare.trim_start();
            // Code is left as it is, a chunk's options and all.
            if let Some(open) = &fence {
                if lead.starts_with(open.as_str()) && lead.trim_start_matches(open.chars().next().unwrap()).trim().is_empty() {
                    fence = None;
                }
                out.push_str(line);
                continue;
            }
            if lead.starts_with("```") || lead.starts_with("~~~") {
                let c = lead.chars().next().unwrap();
                fence = Some(lead.chars().take_while(|x| *x == c).collect());
                out.push_str(line);
                continue;
            }
            let mut bare = bare.to_string();
            if let Some(attrs) = FENCE.captures(&bare).map(|c| c[2].to_string()) {
                if let Some(id) = DIV_ID.captures(&attrs) {
                    let kind = id[2].to_string();
                    let given = NAME.captures(&attrs).map(|n| (n[1].to_string(), n[2].to_string()));
                    if let Some((word, n)) = labels.get(&id[1]) {
                        if THEOREMS.contains(&kind.as_str()) {
                            let name = given.filter(|(k, _)| k == "name").map(|(_, v)| format!(" ({v})")).unwrap_or_default();
                            theorem = Some(format!("**{word} {n}{name}**"));
                        } else if CALLOUTS.contains(&kind.as_str()) {
                            match given.filter(|(k, _)| k == "title") {
                                Some((_, title)) => bare = bare.replacen(&format!("title=\"{title}\""), &format!("title=\"{word} {n}: {title}\""), 1),
                                None => titled = Some(format!("{word} {n}")),
                            }
                        }
                    }
                }
                out.push_str(&bare);
                out.push_str(nl);
                continue;
            }
            // A callout titled by its number alone: the first heading
            // inside takes the number before its words, and with none the
            // fence above takes it as its title.
            if let Some(t) = titled.take() {
                if lead_line(&bare) {
                    match HEAD.captures(&bare) {
                        Some(h) => bare = format!("{} {t}: {}{}", &h[1], &h[2], h.get(3).map(|a| format!(" {}", a.as_str())).unwrap_or_default()),
                        None => {
                            // The title goes on the fence line above.
                            if let Some(at) = out.trim_end_matches('\n').rfind('\n').map(|i| i + 1).or(Some(0)) {
                                let fence = out[at..].trim_end_matches('\n').to_string();
                                if let Some(c) = FENCE.captures(&fence) {
                                    let new = format!("{}{} title=\"{t}\"}}", &c[1], c[2].trim_end());
                                    out.replace_range(at..at + fence.len(), &new);
                                }
                            }
                        }
                    }
                } else {
                    titled = Some(t);
                }
            }
            if let Some(l) = theorem.take() {
                if lead_line(&bare) {
                    if bare.trim_start().starts_with(|c: char| c.is_alphanumeric() || "*_$[(`\\\"'".contains(c)) {
                        bare = format!("{l} {}", bare.trim_start());
                    } else {
                        out.push_str(&format!("{l}\n\n"));
                    }
                } else {
                    theorem = Some(l);
                }
            }
            if let Some(h) = HEAD.captures(&bare).filter(|_| numbered && !titles.contains(&ix)) {
                let attrs = h.get(3).map(|a| a.as_str()).unwrap_or("");
                if !unnumbered(attrs) {
                    let number = section_number(&mut sections, h[1].len(), top);
                    bare = format!("{} {number} {}{}", &h[1], &h[2], h.get(3).map(|a| format!(" {}", a.as_str())).unwrap_or_default());
                }
            }
            // A caption takes its number.
            bare = FIG
                .replace_all(&bare, |c: &regex::Captures| match labels.get(&c[3]) {
                    Some((word, n)) => {
                        let cap = &c[1];
                        let sep = if cap.trim().is_empty() { "" } else { ": " };
                        format!("![{word} {n}{sep}{cap}]{}", &c[2])
                    }
                    None => c[0].to_string(),
                })
                .into_owned();
            if let Some(c) = TBL.captures(&bare) {
                if let Some((word, n)) = labels.get(&c[2]) {
                    bare = format!("*{word} {n}: {}*", &c[1]);
                }
            }
            if let Some(c) = EQ.captures(&bare) {
                if let Some((_, n)) = labels.get(&c[1]) {
                    // The closing `$$` keeps a line of its own, or the
                    // math never closes.
                    let at = c.get(0).unwrap().start();
                    bare = if bare[..at].trim().is_empty() { format!("\\tag{{{n}}}\n$$") } else { format!("{}\\tag{{{n}}} $$", &bare[..at]) };
                }
            }
            out.push_str(&outside_code(&bare, |part| self.cite(part, &labels, works.as_ref(), &mut cited)));
            out.push_str(nl);
        }
        // The works cited, in the document's `#refs` div or at its end.
        if let Some(works) = works.as_ref().filter(|_| !cited.is_empty()) {
            let mut list: Vec<&Work> = cited.iter().filter_map(|k| works.get(k)).collect();
            list.sort_by(|a, b| a.authors.first().cmp(&b.authors.first()).then(a.year.cmp(&b.year)));
            list.dedup_by(|a, b| a.key == b.key);
            let entries = list.iter().map(|w| w.entry()).collect::<Vec<_>>().join("\n\n");
            static REFS: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"(?m)^:::+\s*\{#refs[^}]*\}\s*\n").unwrap());
            if let Some(m) = REFS.find(&out) {
                out.insert_str(m.end(), &format!("{entries}\n"));
            } else {
                let title = self.setting("language.section-title-references").unwrap_or_else(|| "References".to_string());
                out = format!("{}\n\n## {title}\n\n{entries}\n", out.trim_end());
            }
        }
        format!("{head}{out}")
    }

    /// Every label in `body` with its kind's word and its number.
    fn labels(&self, body: &str) -> std::collections::HashMap<String, (String, String)> {
        static ID: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"#((fig|tbl|sec|eq|lst|thm|lem|cor|prp|cnj|def|exm|exr|nte|tip|wrn|imp|cau)-[\w-]+)").unwrap());
        static CHUNK: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^\s*#\|\s*label:\s*['\x22]?((fig|tbl)-[\w-]+)").unwrap());
        static HEAD: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.*?)\s*(\{[^}]*\})?\s*$").unwrap());
        let numbered = self.setting("number-sections").is_some_and(|v| v == "true");
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let mut out = std::collections::HashMap::new();
        let mut sections: Vec<usize> = Vec::new();
        let top = top_level(body);
        let mut fence: Option<char> = None;
        let take = |label: &str, kind: &str, out: &mut std::collections::HashMap<String, (String, String)>, counts: &mut std::collections::HashMap<&str, usize>| {
            let Some(&(k, word)) = KINDS.iter().find(|(k, _)| *k == kind) else { return };
            if out.contains_key(label) {
                return;
            }
            let n = counts.entry(k).or_insert(0);
            *n += 1;
            out.insert(label.to_string(), (self.word(k, word), n.to_string()));
        };
        let titles = callout_titles(body);
        for (ix, line) in body.lines().enumerate() {
            let lead = line.trim_start();
            if let Some(c) = fence {
                if lead.starts_with(c) && lead.trim_start_matches(c).trim().is_empty() {
                    fence = None;
                } else if let Some(cap) = CHUNK.captures(line) {
                    take(&cap[1], &cap[2], &mut out, &mut counts);
                }
                continue;
            }
            if lead.starts_with("```") || lead.starts_with("~~~") {
                fence = lead.chars().next();
                continue;
            }
            if let Some(h) = HEAD.captures(line).filter(|_| !titles.contains(&ix)) {
                let attrs = h.get(3).map(|a| a.as_str()).unwrap_or("");
                let number = (!unnumbered(attrs)).then(|| section_number(&mut sections, h[1].len(), top));
                if let Some(id) = ID.captures(attrs).filter(|c| &c[2] == "sec") {
                    let number = match number.filter(|_| numbered) {
                        Some(n) => n,
                        None => format!("“{}”", &h[2]),
                    };
                    out.insert(id[1].to_string(), (self.word("sec", "Section"), number));
                }
                continue;
            }
            for id in ID.captures_iter(line) {
                if &id[2] != "sec" {
                    take(&id[1], &id[2], &mut out, &mut counts);
                }
            }
        }
        out
    }

    /// The citations and cross-references of a run of words outside code.
    fn cite(&self, text: &str, labels: &std::collections::HashMap<String, (String, String)>, works: Option<&std::collections::HashMap<String, Work>>, cited: &mut Vec<String>) -> String {
        static GROUP: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"\[([^\[\]]*@[^\[\]]*)\]").unwrap());
        static BARE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"(^|[^\w@.\\])@(\w(?:[\w:.#$%&+?<>~/-]*\w)?)").unwrap());
        static PART: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^\s*(.*?)(-?)@(\w(?:[\w:.#$%&+?<>~/-]*\w)?)(.*?)\s*$").unwrap());
        let crossref = |key: &str| KINDS.iter().any(|(k, _)| key.strip_prefix(k).is_some_and(|r| r.starts_with('-')));
        // `[@a; @b]`, not a link (`](`), not a span (`]{`).
        let mut out = String::new();
        let mut at = 0;
        for m in GROUP.captures_iter(text) {
            let whole = m.get(0).unwrap();
            let after = &text[whole.end()..];
            if after.starts_with('(') || after.starts_with('{') || after.starts_with('[') {
                continue;
            }
            let parts: Vec<Option<(String, bool, String, String)>> = m[1].split(';').map(|p| PART.captures(p).map(|c| (c[1].to_string(), !c[2].is_empty(), c[3].to_string(), c[4].to_string()))).collect();
            if parts.iter().any(Option::is_none) {
                continue;
            }
            let parts: Vec<_> = parts.into_iter().flatten().collect();
            let refs = parts.iter().all(|(_, _, key, _)| crossref(key));
            if !refs && works.is_none() {
                continue;
            }
            out.push_str(&text[at..whole.start()]);
            at = whole.end();
            if refs {
                let said: Vec<String> = parts.iter().map(|(pre, bare, key, post)| format!("{pre}{}{post}", self.xref(key, *bare, labels))).collect();
                out.push_str(&said.join(", "));
                continue;
            }
            let works = works.unwrap();
            let said: Vec<String> = parts
                .iter()
                .map(|(pre, year_only, key, post)| {
                    if crossref(key) {
                        return format!("{pre}{}{post}", self.xref(key, false, labels));
                    }
                    match works.get(key) {
                        Some(w) => {
                            cited.push(key.clone());
                            let who = if *year_only { w.year.clone() } else { format!("{} {}", w.named(), w.year) };
                            let pre = if pre.trim().is_empty() { String::new() } else { format!("{} ", pre.trim()) };
                            format!("{pre}{who}{post}")
                        }
                        None => format!("**{key}?**"),
                    }
                })
                .collect();
            out.push_str(&format!("({})", said.join("; ")));
        }
        out.push_str(&text[at..]);
        // `@a` in the line: "Smith (2020)", or a cross-reference.
        BARE.replace_all(&out, |c: &regex::Captures| {
            let key = &c[2];
            if crossref(key) {
                return format!("{}{}", &c[1], self.xref(key, false, labels));
            }
            match works.map(|w| w.get(key)) {
                Some(Some(w)) => {
                    cited.push(key.to_string());
                    format!("{}{} ({})", &c[1], w.named(), w.year)
                }
                Some(None) => format!("{}**{key}?**", &c[1]),
                None => c[0].to_string(),
            }
        })
        .into_owned()
    }

    /// A cross-reference's words: "Figure 1", or "1" alone.
    fn xref(&self, key: &str, number_only: bool, labels: &std::collections::HashMap<String, (String, String)>) -> String {
        match labels.get(key) {
            Some((_, n)) if number_only => n.clone(),
            Some((word, n)) => format!("{word} {n}"),
            None => format!("**?@{key}**"),
        }
    }
}

/// `text` with the attributes taken off its headings (`# Title {#sec-x}`
/// is `# Title`), code left as it is: for a document written in another
/// language, whose ids the conversion made up and which, once the
/// references are read, are nothing to show.
pub fn bare_headings(text: &str) -> String {
    static ATTRS: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^(#{1,6}\s.*?)\s*\{[^{}]*\}\s*$").unwrap());
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<char> = None;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let lead = bare.trim_start();
        if let Some(c) = fence {
            if lead.starts_with(c) && lead.trim_start_matches(c).trim().is_empty() {
                fence = None;
            }
        } else if lead.starts_with("```") || lead.starts_with("~~~") {
            fence = lead.chars().next();
        } else if let Some(c) = ATTRS.captures(bare) {
            out.push_str(&c[1]);
            out.push_str(&line[bare.len()..]);
            continue;
        }
        out.push_str(line);
    }
    out
}

/// Each `[words]{.aside}` taken out of its paragraph and put before it as
/// a margin note of its own. Code is left as it is.
fn asides(body: &str) -> String {
    static ASIDE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"[ \t]?\[((?:[^\[\]]|\[[^\[\]]*\])*)\]\{[^}]*\.aside\b[^}]*\}").unwrap());
    if !body.contains(".aside") {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    let mut block = String::new();
    let mut fence: Option<char> = None;
    let flush = |block: &mut String, out: &mut String| {
        let notes: Vec<String> = ASIDE.captures_iter(block).map(|c| c[1].trim().to_string()).collect();
        if !notes.is_empty() {
            for note in notes {
                out.push_str(&format!("::: {{.column-margin}}\n{note}\n:::\n\n"));
            }
            let rest = ASIDE.replace_all(block, "");
            out.push_str(&rest);
        } else {
            out.push_str(block);
        }
        block.clear();
    };
    for line in body.split_inclusive('\n') {
        let lead = line.trim_start();
        if let Some(c) = fence {
            if lead.starts_with(c) && lead.trim_start_matches(c).trim().is_empty() {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if lead.starts_with("```") || lead.starts_with("~~~") {
            flush(&mut block, &mut out);
            fence = lead.chars().next();
            out.push_str(line);
            continue;
        }
        if line.trim().is_empty() || lead.starts_with(":::") {
            flush(&mut block, &mut out);
            out.push_str(line);
            continue;
        }
        block.push_str(line);
    }
    flush(&mut block, &mut out);
    out
}

/// The level of the highest heading outside code: Quarto numbers
/// sections from it.
fn top_level(body: &str) -> usize {
    static HEAD: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^(#{1,6})\s").unwrap());
    let mut fence: Option<char> = None;
    let mut top = 6;
    for line in body.lines() {
        let lead = line.trim_start();
        if let Some(c) = fence {
            if lead.starts_with(c) && lead.trim_start_matches(c).trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if lead.starts_with("```") || lead.starts_with("~~~") {
            fence = lead.chars().next();
            continue;
        }
        if let Some(h) = HEAD.captures(line) {
            top = top.min(h[1].len());
        }
    }
    top
}

/// A heading's number at `level`, counted in `sections`: "2.1".
fn section_number(sections: &mut Vec<usize>, level: usize, top: usize) -> String {
    let depth = level.saturating_sub(top);
    sections.truncate(depth + 1);
    while sections.len() < depth + 1 {
        sections.push(0);
    }
    sections[depth] += 1;
    sections.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(".")
}

/// The lines of headings that title a callout: the first words inside
/// one are its title when they are a heading, and no section.
fn callout_titles(body: &str) -> HashSet<usize> {
    let mut out = HashSet::new();
    let mut open = false;
    for (ix, line) in body.lines().enumerate() {
        let l = line.trim();
        if l.starts_with(":::") {
            open = l.contains(".callout");
            continue;
        }
        if open && !l.is_empty() {
            if l.starts_with('#') {
                out.insert(ix);
            }
            open = false;
        }
    }
    out
}

/// Attributes that leave a heading unnumbered: `.unnumbered`, `-`.
fn unnumbered(attrs: &str) -> bool {
    let inner = attrs.trim().trim_start_matches('{').trim_end_matches('}');
    inner.split_whitespace().any(|w| w == ".unnumbered" || w == "-")
}

/// A line that starts the words of a div: not blank, not a fence.
fn lead_line(line: &str) -> bool {
    let l = line.trim();
    !l.is_empty() && !l.starts_with(":::")
}

/// How long the YAML header is, its fences counted; 0 when there is none.
fn header_len(text: &str) -> usize {
    let bom = if text.starts_with('\u{feff}') { '\u{feff}'.len_utf8() } else { 0 };
    let body = &text[bom..];
    let Some(first) = body.split_inclusive('\n').next().filter(|l| l.trim_end() == "---") else { return 0 };
    let mut at = first.len();
    for line in body[at..].split_inclusive('\n') {
        at += line.len();
        let bare = line.trim_end();
        if bare == "---" || bare == "..." {
            return bom + at;
        }
    }
    0
}

/// `line` with `f` run over what is not inline code: a run of backticks
/// and what stands up to the same run again is left as it is.
fn outside_code(line: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        let ticks = rest[start..].chars().take_while(|c| *c == '`').count();
        let run = "`".repeat(ticks);
        let after = &rest[start + ticks..];
        match after.find(&run) {
            Some(end) => {
                out.push_str(&f(&rest[..start]));
                out.push_str(&rest[start..start + ticks + end + ticks]);
                rest = &after[end + ticks..];
            }
            None => break,
        }
    }
    out.push_str(&f(rest));
    out
}

/// The works of a BibTeX file.
pub fn bibtex(text: &str) -> Vec<Work> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        rest = &rest[at + 1..];
        let Some(open) = rest.find(['{', '(']) else { break };
        let kind = rest[..open].trim().to_ascii_lowercase();
        let body_start = open + 1;
        // The entry runs to its closing brace.
        let mut depth = 1;
        let mut end = None;
        for (i, c) in rest[body_start..].char_indices() {
            match c {
                '{' | '(' => depth += 1,
                '}' | ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(body_start + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        let body = &rest[body_start..end];
        rest = &rest[end..];
        if matches!(kind.as_str(), "comment" | "string" | "preamble") {
            continue;
        }
        let Some((key, fields)) = body.split_once(',') else { continue };
        let fields = bib_fields(fields);
        let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
        let names: Vec<(String, String)> = get("author").split(" and ").map(str::trim).filter(|a| !a.is_empty()).map(|a| match a.split_once(',') {
            Some((family, given)) => (family.trim().to_string(), given.trim().to_string()),
            None => match a.rsplit_once(' ') {
                Some((given, family)) => (family.to_string(), given.to_string()),
                None => (a.to_string(), String::new()),
            },
        }).collect();
        let year = Some(get("year")).filter(|y| !y.is_empty()).or_else(|| get("date").get(..4).map(str::to_string)).unwrap_or_else(|| "n.d.".into());
        let container = [get("journal"), get("journaltitle"), get("booktitle"), get("publisher")].into_iter().find(|c| !c.is_empty()).unwrap_or_default();
        out.push(Work {
            key: key.trim().to_string(),
            authors: names.iter().map(|(f, _)| f.clone()).collect(),
            listed: names.iter().map(|(f, g)| listed(f, g)).collect(),
            year,
            title: get("title"),
            container,
            volume: get("volume"),
            issue: get("number"),
            pages: get("pages"),
            doi: get("doi"),
            url: get("url"),
        });
    }
    out
}

/// "Family, G. H." from a family name and the given names.
fn listed(family: &str, given: &str) -> String {
    let initials: Vec<String> = given.split([' ', '-']).filter(|g| !g.is_empty()).filter_map(|g| g.chars().next()).map(|c| format!("{c}.")).collect();
    if initials.is_empty() { family.to_string() } else { format!("{family}, {}", initials.join(" ")) }
}

/// A BibTeX entry's `name = {value}` fields, names lowercased, braces and
/// quotes taken off the values.
fn bib_fields(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        let start = i;
        while i < chars.len() && chars[i] != '=' {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let name: String = chars[start..i].iter().collect::<String>().trim().to_ascii_lowercase();
        i += 1;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        match chars.get(i) {
            Some('{') => {
                let mut depth = 0;
                while i < chars.len() {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        c => value.push(c),
                    }
                    i += 1;
                }
            }
            Some('"') => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] != '{' && chars[i] != '}' {
                        value.push(chars[i]);
                    }
                    i += 1;
                }
                i += 1;
            }
            _ => {
                while i < chars.len() && chars[i] != ',' {
                    value.push(chars[i]);
                    i += 1;
                }
            }
        }
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push((name, value.trim().to_string()));
    }
    out
}

/// The works of a CSL JSON file.
pub fn csl_json(text: &str) -> Vec<Work> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let s = |v: &serde_json::Value, k: &str| v.get(k).map(|x| match x { serde_json::Value::String(s) => s.clone(), other => other.to_string() }).unwrap_or_default();
    items
        .iter()
        .map(|it| {
            let names: Vec<(String, String)> = it.get("author").and_then(|a| a.as_array()).map(|a| a.iter().map(|p| (s(p, "family").if_empty(|| s(p, "literal")), s(p, "given"))).collect()).unwrap_or_default();
            let year = it.pointer("/issued/date-parts/0/0").map(|y| y.to_string()).unwrap_or_else(|| "n.d.".into());
            Work {
                key: s(it, "id"),
                authors: names.iter().map(|(f, _)| f.clone()).collect(),
                listed: names.iter().map(|(f, g)| listed(f, g)).collect(),
                year,
                title: s(it, "title"),
                container: s(it, "container-title"),
                volume: s(it, "volume"),
                issue: s(it, "issue"),
                pages: s(it, "page"),
                doi: s(it, "DOI"),
                url: s(it, "URL"),
            }
        })
        .collect()
}

trait IfEmpty {
    fn if_empty(self, f: impl FnOnce() -> String) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, f: impl FnOnce() -> String) -> String {
        if self.is_empty() { f() } else { self }
    }
}

/// A shortcode's arguments: words, with a quoted one kept whole and its
/// quotes dropped.
fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = args.trim();
    while !rest.is_empty() {
        let (word, after) = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => match rest[1..].find(q) {
                Some(end) => (rest[1..1 + end].to_string(), &rest[2 + end..]),
                None => (rest[1..].to_string(), ""),
            },
            _ => {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                (rest[..end].to_string(), &rest[end..])
            }
        };
        out.push(word);
        rest = after.trim_start();
    }
    out
}

/// The lines of the YAML header `text` opens with, none when it has none.
fn header_text(text: &str) -> &str {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(after) = body.strip_prefix("---\n").or_else(|| body.strip_prefix("---\r\n")) else { return "" };
    let mut at = 0;
    for line in after.split_inclusive('\n') {
        let bare = line.trim_end();
        if bare == "---" || bare == "..." {
            return &after[..at];
        }
        at += line.len();
    }
    ""
}

/// The value at a dotted `key` in a YAML `text`, as the simple YAML a
/// Quarto project writes: maps by indent, a list as `- ` items, a
/// quoted or plain scalar, a folded (`>`) or kept (`|`) block. A map at
/// the key, or a list, comes back as its lines; nothing at the key is
/// None.
pub fn yaml_lookup(text: &str, key: &str) -> Option<String> {
    let mut block: Vec<String> = text.lines().filter(|l| !l.trim_start().starts_with('#')).map(str::to_string).collect();
    let segments: Vec<String> = split_key(key);
    for (ix, seg) in segments.iter().enumerate() {
        let last = ix + 1 == segments.len();
        let (head, more) = if let Ok(n) = seg.parse::<usize>() { list_item(&block, n)? } else { map_entry(&block, seg)? };
        if last {
            return Some(scalar(&head, &more));
        }
        if !head.trim().is_empty() && !head.trim_start().starts_with(['>', '|']) {
            // A scalar has nothing under it to walk into.
            return None;
        }
        block = more;
    }
    None
}

fn split_key(key: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = key.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'\\') => {
                chars.next();
                if chars.peek() == Some(&'.') {
                    chars.next();
                    out.last_mut().unwrap().push('.');
                }
            }
            '.' => out.push(String::new()),
            c => out.last_mut().unwrap().push(c),
        }
    }
    out
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Whether a line's body is a map's `key: value`.
fn entry_of(body: &str) -> Option<(&str, &str)> {
    let (k, v) = body.split_once(':')?;
    (v.is_empty() || v.starts_with(' ')).then(|| (unquote(k.trim()), v.trim()))
}

/// The lines nested under the line at `ix` of `block`, deeper than
/// `base`, taken from `ix` on.
fn nested(block: &[String], ix: &mut usize, base: usize) -> Vec<String> {
    let mut more = Vec::new();
    while *ix < block.len() && (block[*ix].trim().is_empty() || indent_of(&block[*ix]) > base) {
        more.push(block[*ix].clone());
        *ix += 1;
    }
    more
}

/// The entry `key` of the map `block` holds at its own indent: what
/// follows the colon on its line, and the lines nested under it.
fn map_entry(block: &[String], key: &str) -> Option<(String, Vec<String>)> {
    let base = block.iter().find(|l| !l.trim().is_empty()).map(|l| indent_of(l))?;
    let mut ix = 0;
    while ix < block.len() {
        let line = &block[ix];
        ix += 1;
        if line.trim().is_empty() || indent_of(line) != base {
            continue;
        }
        let Some((k, v)) = entry_of(line.trim_start()) else { continue };
        if k != key {
            continue;
        }
        let more = nested(block, &mut ix, base);
        return Some((v.to_string(), more));
    }
    None
}

/// The `n`th item (from 1) of the list `block` holds: what follows its
/// dash, and the lines nested under it. An item that opens a map on its
/// dash line (`- name: Bo`) is that map, its first entry a line of it.
fn list_item(block: &[String], n: usize) -> Option<(String, Vec<String>)> {
    let base = block.iter().find(|l| !l.trim().is_empty()).map(|l| indent_of(l))?;
    let mut count = 0;
    let mut ix = 0;
    while ix < block.len() {
        let line = &block[ix];
        ix += 1;
        if line.trim().is_empty() || indent_of(line) != base || !line.trim_start().starts_with('-') {
            continue;
        }
        count += 1;
        if count != n {
            continue;
        }
        let head = line.trim_start()[1..].trim().to_string();
        let mut more = nested(block, &mut ix, base);
        if entry_of(&head).is_some() {
            let indent = more.iter().find(|l| !l.trim().is_empty()).map(|l| indent_of(l)).unwrap_or(base + 2);
            more.insert(0, format!("{}{head}", " ".repeat(indent)));
            return Some((String::new(), more));
        }
        return Some((head, more));
    }
    None
}

/// A value as it reads: a plain or quoted scalar as its words, a block
/// scalar folded or kept, and a nested map or list as its lines, dedented.
fn scalar(head: &str, more: &[String]) -> String {
    let dedent = |lines: &[String]| -> Vec<String> {
        let indent = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| indent_of(l)).min().unwrap_or(0);
        lines.iter().map(|l| l.get(indent..).unwrap_or("").to_string()).collect()
    };
    let mut lines = dedent(more);
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if head.starts_with('>') {
        return lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
    }
    if head.starts_with('|') {
        return lines.join("\n");
    }
    if head.is_empty() {
        return lines.join("\n");
    }
    unquote(head).to_string()
}

fn unquote(v: &str) -> &str {
    let v = v.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_values_are_found_by_dotted_keys() {
        let y = "quarto: \"[Quarto](https://quarto.org/)\"\nemail:\n  info: info@example.com\n  # a comment\n  sales: 'sales@example.com'\nauthor:\n  - Ann\n  - name: Bo\n    role: editor\nnote: >\n  folded\n  words\nkept: |\n  line one\n  line two\n";
        assert_eq!(yaml_lookup(y, "quarto").as_deref(), Some("[Quarto](https://quarto.org/)"));
        assert_eq!(yaml_lookup(y, "email.info").as_deref(), Some("info@example.com"));
        assert_eq!(yaml_lookup(y, "email.sales").as_deref(), Some("sales@example.com"));
        assert_eq!(yaml_lookup(y, "author.1").as_deref(), Some("Ann"));
        assert_eq!(yaml_lookup(y, "author.2.name").as_deref(), Some("Bo"));
        assert_eq!(yaml_lookup(y, "note").as_deref(), Some("folded words"));
        assert_eq!(yaml_lookup(y, "kept").as_deref(), Some("line one\nline two"));
        assert_eq!(yaml_lookup(y, "email").as_deref(), Some("info: info@example.com\nsales: 'sales@example.com'"));
        assert_eq!(yaml_lookup(y, "email.phone"), None);
        assert_eq!(yaml_lookup(y, "quarto.x"), None);
        assert_eq!(split_key("a\\\\.b.c"), ["a.b", "c"]);
    }

    #[test]
    fn shortcode_arguments_split_as_words_and_quoted_strings() {
        assert_eq!(split_args(r#"PRODUCT "a b" c"#), ["PRODUCT", "a b", "c"]);
        assert_eq!(split_args("  /a/b.qmd  "), ["/a/b.qmd"]);
        assert!(split_args("").is_empty());
    }
}
