//! A Typst document read for showing: written as the Quarto markdown the
//! file's pane reads, so it is drawn in the window's own look and not as
//! Typst's PDF. The source is parsed by Typst's own parser
//! (`typst_syntax`) and not run: what is code writes nothing, and what is
//! content is read as far as the source says. What this writes goes
//! through `quarto::expand` like any `.qmd`, and its math is set later by
//! Typst from the Typst math left in it.
//!
//! What each construct becomes:
//! - `#set document(title:, author:, date:)`, and a template's arguments
//!   (`#show: project.with(title:, authors:, date:, abstract:)`), are the
//!   YAML header's `title`, `author` (one line: "A, B and C") and `date`;
//!   an author is a string or a dictionary's `name`. The abstract is a
//!   `::: {.abstract}` div at the start of the body, and so is the text
//!   under a first heading that reads "Abstract".
//!   `#set heading(numbering: ..)` is `number-sections: true`.
//! - `= A` is `# A`, a `#` for each `=`; a label after it is `{#sec-…}`, and
//!   `#heading(numbering: none)` is `{.unnumbered}`.
//! - A paragraph is one line, and paragraphs stand apart by a blank line.
//!   `*a*` is `**a**`, `_a_` is `*a*`, raw is code, `#strike` is `~~`,
//!   `#underline` `<u>`, `#highlight` `<mark>`. `#upper` and `#lower`
//!   change the case; `#sub` and `#super` are Unicode's small letters
//!   where it has one for every character; `#text`, `#box`, `#smallcaps`
//!   and their like give their text.
//! - A link is `[text](url)`, a bare one `<url>`. `\` at a line's end is a
//!   hard break. Quotes are curly; `--`, `---`, `~`, `...` are their
//!   characters, `-?` nothing; an escape is its character.
//! - Text markdown would read as syntax is escaped.
//! - `- a`, `+ a` (counted up, or from its own number) and `/ T: d`
//!   (`- **T**: d`) are lists, what is nested indented four spaces.
//! - `#quote(block: true)` is a blockquote, its attribution the last
//!   line; inline, it stands in curly quotes.
//! - A raw block is a fenced code block with its language.
//! - `#align(center)` and `#align(right)` are a div of that `text-align`;
//!   `#grid` and a left-to-right `#stack` a `layout-ncol` div, a block a
//!   cell. `#block`, `#pad`, `#rect` and the other layout functions are
//!   their content, `#hide` nothing.
//! - Math is left as Typst wrote it: `$x$` inline, and a display equation
//!   (space just inside both dollars) between `$$` lines. A label after
//!   it closes it `$$ {#eq-…}`, and with equation numbering set, every
//!   display equation gets an id.
//! - A figure of a picture is `![Cap](path){#fig-… width=…}`; of a table,
//!   a pipe table and `: Cap {#tbl-…}`; of code (or a kind of its own),
//!   its content and "Listing 1: Cap". Its kind is Typst's: the first
//!   picture, table or code in it, or what `kind:` says (Quarto's own
//!   `quarto-float-*` kinds too). A figure with a caption and no label
//!   gets an id of its own, so it is numbered as Typst numbers it.
//! - A table is a pipe table, its header `table.header` or its first row,
//!   its cells set as Typst sets them past what a spanning cell covers.
//! - `#footnote` is `[^n]`, the notes listed at the end.
//! - A label is an id by what it is on (`fig-`, `tbl-`, `eq-`, `sec-`),
//!   and `@label` is `@id`. An `@word` that is no label, and `#cite`, are
//!   citations (`[@key]`); `#bibliography` is `::: {#refs}` where it
//!   stands and the header's `bibliography`.
//! - Rules, imports, comments, page and column breaks and spacing write
//!   nothing. `#let x = …` binds a name, read where `#x` stands. A code
//!   block (`#{ … }`, `#context { … }`) writes the content it makes, as
//!   Quarto's tables with notes under them are written; its values,
//!   loops and conditions write nothing. A function not known here writes
//!   the content it is given. `#include` pastes the file in, at most eight
//!   deep, and the paths in it are made relative to the main document.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use typst_syntax::ast::{self, Arg, AstNode, Expr};
use typst_syntax::{SyntaxKind, SyntaxNode};

/// The most includes nested in one another that are followed.
const DEPTH: usize = 8;

/// How deep elements nested in one another are read; deeper is left out,
/// so no input runs the stack out.
const NESTING: usize = 64;

/// The most a count in the source is taken as: words of `#lorem`, a
/// table's columns, a cell's span.
const MOST: usize = 1000;

/// How many times names are read in one document: a value built from
/// itself again and again would double each time.
const EXPANSIONS: usize = 10_000;

/// A reference stands as these until every label is known: the target,
/// then its supplement after `REF_SUPP`.
const REF_OPEN: char = '\u{E000}';
const REF_SUPP: char = '\u{E001}';
const REF_CLOSE: char = '\u{E002}';

/// The words `#lorem` writes, cycled.
const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

/// Functions that make a value and not content: they write nothing, and
/// as an argument they are not a body.
const VALUES: &[&str] = &[
    "rgb", "rgba", "luma", "cmyk", "oklab", "oklch", "color", "gradient", "stroke", "datetime", "duration", "str", "int", "float", "decimal", "bool", "range", "array", "dictionary", "bytes", "label",
    "selector", "type", "repr", "numbering", "counter", "state", "query", "measure", "here", "locate", "layout", "metadata", "read", "json", "yaml", "toml", "csv", "xml", "cbor", "eval", "panic", "assert",
    "regex", "version", "symbol", "plugin", "calc", "sys", "outline", "line", "path", "polygon", "curve",
];

/// `text`, the Typst document at `file`, as Quarto markdown.
pub fn to_markdown(text: &str, file: &Path) -> String {
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut doc = Doc { dir, seen: vec![canonical(file)], ..Doc::default() };
    let mut flow = Flow::top();
    doc.source(text, &mut flow);
    doc.finish(flow)
}

/// What a label names, for its references.
enum Target {
    /// An id Quarto numbers: `fig-a`.
    Id(String),
    /// Words written as they are: "Listing 2".
    Words(String),
}

/// What a figure holds, by Typst's own reading of its body.
enum Figure {
    Picture,
    Table,
    /// Code, or a kind of the document's own: its counter's key and the
    /// word its number follows.
    Other { key: String, word: String },
}

/// The document as it is read: what goes in the header, and what the
/// references need once all of it has been read.
#[derive(Default)]
struct Doc {
    /// The main document's folder.
    dir: PathBuf,
    /// The folder of the file being read, from `dir`.
    rel: PathBuf,
    /// The files being read, the main one first.
    seen: Vec<PathBuf>,
    title: Option<String>,
    authors: Vec<String>,
    date: Option<String>,
    bibliography: Option<String>,
    number_sections: bool,
    numbered_equations: bool,
    abstract_blocks: Option<Vec<Block>>,
    /// Where the text under a heading "Abstract" starts in the top flow.
    abstract_at: Option<usize>,
    headed: bool,
    labels: HashMap<String, Target>,
    footnotes: Vec<String>,
    /// A footnote's label and its number.
    notes: HashMap<String, usize>,
    /// The ids given to what has no label, by kind.
    autos: HashMap<&'static str, usize>,
    /// The figures of pictures given an id, in order: Quarto numbers them
    /// the same way.
    figures: usize,
    /// The other figures by kind.
    counts: HashMap<String, usize>,
    /// What `#let` bound, the latest last: the value as it is written,
    /// None for a function or what is not one name.
    locals: Vec<(String, Option<SyntaxNode>)>,
    /// How many times a name has been read (`EXPANSIONS`).
    expansions: usize,
    /// How deep the element being read is.
    nesting: usize,
}

/// The markdown being written: the blocks so far and the paragraph open.
#[derive(Default)]
struct Flow {
    blocks: Vec<Block>,
    para: String,
    /// Text alone, no marks: a header value.
    plain: bool,
    case: Option<Case>,
    /// The document's own flow, not one inside an element.
    top: bool,
    /// The number the next item of a numbered list takes.
    next_number: u64,
    /// A blank line has come since the last block.
    gap: bool,
}

#[derive(Clone)]
struct Block {
    md: String,
    kind: BlockKind,
    /// It came right under the block before, with no blank line in the
    /// source between them.
    tight: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum BlockKind {
    Para,
    List(List),
    Other,
}

#[derive(Clone, Copy, PartialEq)]
enum List {
    Bullet,
    Number,
    Term,
}

#[derive(Clone, Copy)]
enum Case {
    Upper,
    Lower,
}

impl Flow {
    fn top() -> Self {
        Flow { top: true, ..Flow::default() }
    }

    /// A flow for what is inside an element, read as this one reads.
    fn sub(&self) -> Self {
        Flow { plain: self.plain, case: self.case, ..Flow::default() }
    }

    fn plain() -> Self {
        Flow { plain: true, ..Flow::default() }
    }

    /// Text of the document, escaped where markdown would read it.
    fn text(&mut self, text: &str) {
        let text = match self.case {
            Some(Case::Upper) => text.to_uppercase(),
            Some(Case::Lower) => text.to_lowercase(),
            None => text.to_string(),
        };
        if self.plain {
            self.para.push_str(&text);
        } else {
            escape_into(&mut self.para, &text);
        }
    }

    fn char(&mut self, c: char) {
        self.text(c.encode_utf8(&mut [0; 4]));
    }

    /// Markdown as it is.
    fn md(&mut self, md: &str) {
        self.para.push_str(md);
    }

    fn space(&mut self) {
        if self.para.chars().last().is_some_and(|c| !c.is_whitespace()) {
            self.para.push(' ');
        }
    }

    fn linebreak(&mut self) {
        if self.plain {
            return self.space();
        }
        let kept = self.para.trim_end_matches([' ', '\t']).len();
        self.para.truncate(kept);
        // Markdown has no break on a line of its own.
        if self.para.trim().is_empty() || self.para.ends_with('\n') {
            return;
        }
        self.para.push_str("\\\n");
    }

    /// A curly quote, opening after a space or an opening bracket as
    /// Typst's smart quotes do, else closing.
    fn quote(&mut self, double: bool) {
        let open = self.para.chars().last().is_none_or(|c| c.is_whitespace() || "([{“‘—–".contains(c));
        self.para.push(match (double, open) {
            (true, true) => '“',
            (true, false) => '”',
            (false, true) => '‘',
            (false, false) => '’',
        });
    }

    fn end_para(&mut self) {
        let para = std::mem::take(&mut self.para);
        let mut p = para.as_str();
        // A break at the paragraph's end would be a backslash left there.
        loop {
            let t = p.trim_end_matches([' ', '\t']);
            match t.strip_suffix("\\\n") {
                Some(rest) => p = rest,
                None => {
                    p = t;
                    break;
                }
            }
        }
        let p = p.trim();
        if p.is_empty() {
            return;
        }
        let md = if self.plain { p.to_string() } else { p.split('\n').map(guard).collect::<Vec<_>>().join("\n") };
        self.push(md, BlockKind::Para);
    }

    /// A blank line in the source.
    fn parbreak(&mut self) {
        self.end_para();
        self.gap = true;
    }

    fn push(&mut self, md: String, kind: BlockKind) {
        let tight = !std::mem::take(&mut self.gap);
        self.blocks.push(Block { md, kind, tight });
    }

    fn block(&mut self, md: String) {
        self.end_para();
        if !md.trim().is_empty() {
            self.push(md, BlockKind::Other);
        }
    }

    /// Blocks read elsewhere, set in this flow.
    fn extend(&mut self, blocks: Vec<Block>) {
        self.end_para();
        self.blocks.extend(blocks);
    }

    /// A list's item: on the line under the one before it of the same
    /// list, else a list of its own.
    fn item(&mut self, list: List, md: String) {
        self.end_para();
        match self.blocks.last_mut() {
            Some(last) if last.kind == BlockKind::List(list) => {
                last.md.push('\n');
                last.md.push_str(&md);
            }
            _ => self.push(md, BlockKind::List(list)),
        }
        self.gap = false;
    }

    fn last_list(&self) -> Option<List> {
        match self.blocks.last()?.kind {
            BlockKind::List(l) if self.para.trim().is_empty() => Some(l),
            _ => None,
        }
    }

    /// All of it as one run of inline markdown.
    fn flatten(mut self) -> String {
        self.end_para();
        self.blocks.iter().map(|b| b.md.as_str()).collect::<Vec<_>>().join(" ")
    }
}

impl Doc {
    fn source(&mut self, text: &str, flow: &mut Flow) {
        let root = typst_syntax::parse(text);
        if let Some(markup) = root.cast::<ast::Markup>() {
            self.markup(markup, flow);
        }
    }

    /// A run of markup. A label is on the element before it, spaces
    /// between them aside, as Typst attaches it.
    fn markup(&mut self, markup: ast::Markup, flow: &mut Flow) {
        let scope = self.locals.len();
        let exprs: Vec<Expr> = markup.exprs().collect();
        let mut used = vec![false; exprs.len()];
        for (i, &e) in exprs.iter().enumerate() {
            if used[i] {
                continue;
            }
            let label = if labelled(e) { label_after(&exprs, i) } else { None };
            if let Some((j, _)) = label {
                for k in i + 1..=j {
                    used[k] = !matches!(exprs[k], Expr::Parbreak(_));
                }
            }
            self.expr(e, label.map(|(_, l)| l), flow);
        }
        self.locals.truncate(scope);
    }

    fn expr(&mut self, e: Expr, label: Option<&str>, flow: &mut Flow) {
        if self.nesting >= NESTING {
            return;
        }
        self.nesting += 1;
        self.element(e, label, flow);
        self.nesting -= 1;
    }

    fn element(&mut self, e: Expr, label: Option<&str>, flow: &mut Flow) {
        match e {
            Expr::Text(t) => flow.text(t.get()),
            Expr::Space(_) => flow.space(),
            Expr::Linebreak(_) => flow.linebreak(),
            Expr::Parbreak(_) => flow.parbreak(),
            Expr::Escape(x) => flow.char(x.get()),
            Expr::Shorthand(s) => match s.get() {
                '\u{ad}' => {}
                c => flow.char(c),
            },
            Expr::SmartQuote(q) => flow.quote(q.double()),
            Expr::Strong(s) => self.wrap("**", "**", flow, |d, f| d.markup(s.body(), f)),
            Expr::Emph(s) => self.wrap("*", "*", flow, |d, f| d.markup(s.body(), f)),
            Expr::Raw(r) => {
                let text = r.lines().map(|l| l.get().as_str()).collect::<Vec<_>>().join("\n");
                let lang = r.lang().map(|l| l.get().to_lowercase());
                raw(&text, lang.as_deref(), r.block(), flow);
            }
            Expr::Link(l) => self.link(l.get(), None, flow),
            Expr::Ref(r) => {
                let supplement = r.supplement().map(|s| self.line(flow, |d, f| d.markup(s.body(), f)));
                reference(r.target(), supplement, flow);
            }
            Expr::Heading(h) => self.heading(h.depth().get(), false, label, flow, |d, f| d.markup(h.body(), f)),
            Expr::ListItem(item) => self.list_item(List::Bullet, None, None, flow, |d, f| d.markup(item.body(), f)),
            Expr::EnumItem(item) => self.list_item(List::Number, item.number(), None, flow, |d, f| d.markup(item.body(), f)),
            Expr::TermItem(item) => {
                let term = self.line(flow, |d, f| d.markup(item.term(), f));
                self.list_item(List::Term, None, Some(term), flow, |d, f| d.markup(item.description(), f));
            }
            Expr::Equation(eq) => self.equation(eq, label, flow),
            Expr::FuncCall(call) => self.call(call, label, flow),
            Expr::ContentBlock(c) => self.markup(c.body(), flow),
            Expr::Str(s) => flow.text(&s.get()),
            Expr::Int(i) => flow.text(&i.get().to_string()),
            Expr::Float(x) => flow.text(&x.get().to_string()),
            Expr::Ident(id) => self.name(id.as_str(), flow),
            Expr::Parenthesized(p) => self.expr(p.expr(), None, flow),
            Expr::CodeBlock(b) => self.code(b.body(), flow),
            Expr::Contextual(c) => self.expr(c.body(), None, flow),
            Expr::LetBinding(l) => self.binding(l),
            Expr::SetRule(s) => self.set_rule(s),
            Expr::ShowRule(s) => self.show_rule(s),
            Expr::ModuleInclude(i) => self.include(i, flow),
            _ => {}
        }
    }

    /// What `body` writes, as one run of inline markdown.
    fn inline(&mut self, flow: &Flow, body: impl FnOnce(&mut Self, &mut Flow)) -> String {
        let mut sub = flow.sub();
        body(self, &mut sub);
        sub.flatten()
    }

    /// What `body` writes, on one line.
    fn line(&mut self, flow: &Flow, body: impl FnOnce(&mut Self, &mut Flow)) -> String {
        one_line(&self.inline(flow, body))
    }

    /// What `body` writes, as blocks.
    fn blocks(&mut self, flow: &Flow, body: impl FnOnce(&mut Self, &mut Flow)) -> Vec<Block> {
        let mut sub = flow.sub();
        body(self, &mut sub);
        sub.end_para();
        sub.blocks
    }

    /// The plain text of a value: a string, or content without its marks.
    fn plain(&mut self, e: Expr) -> Option<String> {
        if matches!(e, Expr::None(_) | Expr::Auto(_)) {
            return None;
        }
        let mut flow = Flow::plain();
        self.expr(e, None, &mut flow);
        let text = one_line(&flow.flatten()).split_whitespace().collect::<Vec<_>>().join(" ");
        (!text.is_empty()).then_some(text)
    }

    /// `body` between two marks, the marks kept tight to its words.
    fn wrap(&mut self, open: &str, close: &str, flow: &mut Flow, body: impl FnOnce(&mut Self, &mut Flow)) {
        let inner = self.inline(flow, body);
        let inner = inner.trim();
        if inner.is_empty() {
            return;
        }
        if flow.plain {
            flow.md(inner);
        } else {
            flow.md(&format!("{open}{inner}{close}"));
        }
    }

    fn link(&mut self, url: &str, body: Option<String>, flow: &mut Flow) {
        match body.filter(|b| !b.trim().is_empty()) {
            Some(text) if flow.plain => flow.md(&text),
            Some(text) => flow.md(&format!("[{}]({})", text.trim(), destination(url))),
            None if flow.plain => flow.md(url),
            // Typst shows a mail or phone link as the address alone.
            None if url.starts_with("mailto:") || url.starts_with("tel:") => {
                let mut text = String::new();
                escape_into(&mut text, url.split_once(':').map_or(url, |(_, rest)| rest));
                flow.md(&format!("[{text}]({})", destination(url)));
            }
            None if has_scheme(url) => flow.md(&format!("<{}>", url_safe(url))),
            None => {
                let mut text = String::new();
                escape_into(&mut text, url);
                flow.md(&format!("[{text}]({})", destination(url)));
            }
        }
    }

    fn heading(&mut self, depth: usize, unnumbered: bool, label: Option<&str>, flow: &mut Flow, body: impl FnOnce(&mut Self, &mut Flow)) {
        let text = self.line(flow, body);
        if flow.plain {
            flow.space();
            flow.md(&text);
            return;
        }
        if flow.top {
            self.close_abstract(flow);
            let first = !self.headed;
            self.headed = true;
            let words = text.trim().trim_end_matches(['.', ':']);
            if first && label.is_none() && self.abstract_blocks.is_none() && words.eq_ignore_ascii_case("abstract") {
                flow.end_para();
                self.abstract_at = Some(flow.blocks.len());
                return;
            }
        }
        let mut attrs = Vec::new();
        if let Some(l) = label {
            attrs.push(format!("#{}", self.label(l, "sec")));
        }
        if unnumbered {
            attrs.push(".unnumbered".to_string());
        }
        let mut line = format!("{} {}", "#".repeat(depth.clamp(1, 6)), text);
        if !attrs.is_empty() {
            line.push_str(&format!(" {{{}}}", attrs.join(" ")));
        }
        flow.block(line);
    }

    /// The text under a first heading "Abstract", up to the next heading,
    /// taken out of the body to stand at its start.
    fn close_abstract(&mut self, flow: &mut Flow) {
        if let Some(at) = self.abstract_at.take() {
            flow.end_para();
            let taken = flow.blocks.split_off(at.min(flow.blocks.len()));
            if !taken.is_empty() {
                self.abstract_blocks = Some(taken);
            }
        }
    }

    fn list_item(&mut self, list: List, number: Option<u64>, term: Option<String>, flow: &mut Flow, body: impl FnOnce(&mut Self, &mut Flow)) {
        flow.end_para();
        let mut blocks = self.blocks(flow, body);
        if flow.plain {
            flow.space();
            if let Some(term) = term {
                flow.md(&term);
                flow.md(": ");
            }
            flow.md(&blocks.iter().map(|b| b.md.as_str()).collect::<Vec<_>>().join(" "));
            return;
        }
        if let Some(term) = term {
            let head = format!("**{term}**:");
            match blocks.first_mut() {
                Some(b) if b.kind == BlockKind::Para => b.md = format!("{head} {}", b.md),
                _ => blocks.insert(0, Block { md: head, kind: BlockKind::Para, tight: true }),
            }
        }
        let marker = match list {
            List::Number => {
                let n = number.unwrap_or(if flow.last_list() == Some(List::Number) { flow.next_number } else { 1 });
                flow.next_number = n.saturating_add(1);
                format!("{n}. ")
            }
            _ => "- ".to_string(),
        };
        flow.item(list, indent_item(&marker, &blocks));
    }

    fn equation(&mut self, eq: ast::Equation, label: Option<&str>, flow: &mut Flow) {
        let source = eq.body().to_untyped().full_text();
        let source = source.trim();
        if source.is_empty() {
            return;
        }
        if !eq.block() || flow.plain {
            let math = one_line(source);
            if flow.plain {
                flow.md(&math);
            } else {
                flow.md(&format!("${math}$"));
            }
            return;
        }
        let id = match label {
            Some(l) => Some(self.label(l, "eq")),
            None if self.numbered_equations => Some(self.auto("eq")),
            None => None,
        };
        let close = id.map_or("$$".to_string(), |id| format!("$$ {{#{id}}}"));
        let lines = source.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
        flow.block(format!("$$\n{lines}\n{close}"));
    }

    /// The id a label gets on what it is on, kept for its references.
    fn label(&mut self, label: &str, kind: &str) -> String {
        let id = label_id(kind, label);
        self.labels.entry(label.to_string()).or_insert_with(|| Target::Id(id.clone()));
        id
    }

    /// An id of its own for what has no label: `fig-auto-1`.
    fn auto(&mut self, kind: &'static str) -> String {
        let n = self.autos.entry(kind).or_insert(0);
        *n += 1;
        format!("{kind}-auto-{n}")
    }

    fn call(&mut self, call: ast::FuncCall, label: Option<&str>, flow: &mut Flow) {
        // A method's call (`counter(page).display()`) is code.
        let Some(name) = dotted(call.callee()) else { return };
        let args = call.args();
        match name.as_str() {
            "strong" => self.wrap_body("**", "**", args, flow),
            "emph" => self.wrap_body("*", "*", args, flow),
            "strike" => self.wrap_body("~~", "~~", args, flow),
            "underline" => self.wrap_body("<u>", "</u>", args, flow),
            "highlight" => self.wrap_body("<mark>", "</mark>", args, flow),
            "text" | "box" | "smallcaps" | "overline" | "move" | "scale" | "rotate" | "skew" | "repeat" | "table.cell" | "grid.cell" | "figure.caption" => self.body_of(args, flow),
            "upper" | "lower" => {
                let saved = flow.case;
                flow.case = Some(if name == "upper" { Case::Upper } else { Case::Lower });
                self.body_of(args, flow);
                flow.case = saved;
            }
            "sub" | "super" => self.script(name == "super", args, flow),
            "block" | "rect" | "square" | "circle" | "ellipse" | "pad" | "place" | "columns" | "par" => {
                flow.end_para();
                self.body_of(args, flow);
                flow.end_para();
            }
            "hide" => {}
            "align" => self.align(args, flow),
            "grid" => self.grid(args, flow),
            "stack" => self.stack(args, flow),
            "link" => {
                let pos = positional(args);
                match pos.first() {
                    Some(Expr::Str(url)) => {
                        let text = pos.get(1).map(|&b| self.inline(flow, |d, f| d.expr(b, None, f)));
                        self.link(&url.get(), text, flow);
                    }
                    // A link to a label is its words.
                    Some(Expr::Label(_)) => {
                        if let Some(&b) = pos.get(1) {
                            self.expr(b, None, flow);
                        }
                    }
                    _ => {}
                }
            }
            "footnote" => self.footnote(args, label, flow),
            "cite" => self.cite(args, flow),
            "ref" => {
                let target = positional(args).into_iter().find_map(|e| match e {
                    Expr::Label(l) => Some(l.get()),
                    _ => None,
                });
                let supplement = named(args, "supplement").map(|s| self.line(flow, |d, f| d.expr(s, None, f)));
                if let Some(t) = target {
                    reference(t, supplement, flow);
                }
            }
            "quote" => self.quote(args, flow),
            "raw" => {
                let Some(text) = positional(args).first().and_then(|&e| str_of(e)) else { return };
                let lang = named(args, "lang").and_then(str_of).map(|l| l.to_lowercase());
                let block = named(args, "block").is_some_and(is_true);
                raw(&text, lang.as_deref(), block, flow);
            }
            "image" => {
                if !flow.plain {
                    if let Some(md) = self.image(args, None, None) {
                        flow.md(&md);
                    }
                }
            }
            "figure" => self.figure(args, label, flow),
            "table" => {
                let md = self.table(args, flow);
                flow.block(md);
            }
            "heading" => {
                let depth = named(args, "level").or_else(|| named(args, "depth")).and_then(int).unwrap_or(1).max(1) as usize;
                let unnumbered = named(args, "numbering").is_some_and(|e| matches!(e, Expr::None(_)));
                let body = body(args);
                self.heading(depth, unnumbered, label, flow, |d, f| {
                    if let Some(b) = body {
                        d.expr(b, None, f);
                    }
                });
            }
            "list" => self.list_call(List::Bullet, args, flow),
            "enum" => self.list_call(List::Number, args, flow),
            "terms" => self.list_call(List::Term, args, flow),
            "lorem" => {
                if let Some(n) = positional(args).first().and_then(|&e| int(e)) {
                    flow.text(&lorem((n.max(0) as usize).min(MOST * 10)));
                }
            }
            "linebreak" => flow.linebreak(),
            "parbreak" | "pagebreak" | "colbreak" | "v" => flow.end_para(),
            "h" => flow.space(),
            "bibliography" => self.bibliography(args, flow),
            // Typst's own title element shows the document's title, which
            // is the header's; one given content names it.
            "title" => {
                if self.title.is_none() {
                    if let Some(b) = body(args) {
                        self.title = self.plain(b);
                    }
                }
            }
            _ if is_value(&name) => {}
            _ => self.unknown(args, flow),
        }
    }

    /// The content a function is given: its last positional argument,
    /// written where the call stands.
    fn body_of(&mut self, args: ast::Args, flow: &mut Flow) {
        if let Some(b) = body(args) {
            self.expr(b, None, flow);
        }
    }

    fn wrap_body(&mut self, open: &str, close: &str, args: ast::Args, flow: &mut Flow) {
        if let Some(b) = body(args) {
            self.wrap(open, close, flow, |d, f| d.expr(b, None, f));
        }
    }

    /// `#sub` and `#super`: Unicode's small characters when it has one for
    /// each, else the text as it is.
    fn script(&mut self, up: bool, args: ast::Args, flow: &mut Flow) {
        let Some(b) = body(args) else { return };
        let plain = self.plain(b).unwrap_or_default();
        match plain.chars().map(|c| small(c, up)).collect::<Option<String>>() {
            Some(s) if !s.is_empty() => flow.text(&s),
            _ => self.expr(b, None, flow),
        }
    }

    fn align(&mut self, args: ast::Args, flow: &mut Flow) {
        let Some(b) = body(args) else { return };
        let how = positional(args).into_iter().find_map(halign);
        let blocks = self.blocks(flow, |d, f| d.expr(b, None, f));
        match how {
            Some(side @ ('c' | 'r')) if !flow.plain => {
                let side = if side == 'c' { "center" } else { "right" };
                if !blocks.is_empty() {
                    flow.block(format!("::: {{style=\"text-align: {side}\"}}\n\n{}\n\n:::", join(&blocks)));
                }
            }
            _ => flow.extend(blocks),
        }
    }

    fn grid(&mut self, args: ast::Args, flow: &mut Flow) {
        let cols = named(args, "columns").map_or(1, count);
        let mut cells = Vec::new();
        for e in positional(args) {
            self.grid_cell(e, flow, &mut cells);
        }
        layout(cols, cells, flow);
    }

    fn grid_cell(&mut self, e: Expr, flow: &Flow, cells: &mut Vec<String>) {
        if let Expr::FuncCall(c) = e {
            match dotted(c.callee()).as_deref() {
                Some("grid.header" | "grid.footer" | "table.header" | "table.footer") => {
                    for x in positional(c.args()) {
                        self.grid_cell(x, flow, cells);
                    }
                    return;
                }
                Some("grid.hline" | "grid.vline" | "table.hline" | "table.vline") => return,
                _ => {}
            }
        }
        if !is_content(e) {
            return;
        }
        let blocks = self.blocks(flow, |d, f| d.expr(e, None, f));
        if !blocks.is_empty() {
            cells.push(join(&blocks));
        }
    }

    fn stack(&mut self, args: ast::Args, flow: &mut Flow) {
        let across = named(args, "dir").is_some_and(|d| matches!(dotted(d).as_deref(), Some("ltr" | "rtl")));
        let mut cells = Vec::new();
        for e in positional(args) {
            self.grid_cell(e, flow, &mut cells);
        }
        let cols = if across { cells.len() } else { 1 };
        layout(cols, cells, flow);
    }

    fn footnote(&mut self, args: ast::Args, label: Option<&str>, flow: &mut Flow) {
        if flow.plain {
            return;
        }
        let pos = positional(args);
        // `#footnote(<note>)` is the note of that label again.
        if let Some(Expr::Label(l)) = pos.first() {
            if let Some(n) = self.notes.get(l.get()) {
                flow.md(&format!("[^{n}]"));
            }
            return;
        }
        let Some(b) = body(args) else { return };
        // The number is taken first, so a note inside counts after it.
        self.footnotes.push(String::new());
        let n = self.footnotes.len();
        let blocks = self.blocks(flow, |d, f| d.expr(b, None, f));
        self.footnotes[n - 1] = one_line(&blocks.iter().map(|b| b.md.as_str()).collect::<Vec<_>>().join(" "));
        if let Some(l) = label {
            self.notes.insert(l.to_string(), n);
        }
        flow.md(&format!("[^{n}]"));
    }

    fn cite(&mut self, args: ast::Args, flow: &mut Flow) {
        let keys: Vec<String> = positional(args)
            .into_iter()
            .filter_map(|e| match e {
                Expr::Label(l) => Some(l.get().to_string()),
                Expr::Str(s) => Some(s.get().to_string()),
                _ => None,
            })
            .collect();
        if keys.is_empty() {
            return;
        }
        if flow.plain {
            flow.md(&keys.join(", "));
            return;
        }
        let form = match named(args, "form") {
            Some(Expr::None(_)) => return,
            Some(e) => str_of(e).unwrap_or_default(),
            None => String::new(),
        };
        let supplement = named(args, "supplement").map(|s| self.line(flow, |d, f| d.expr(s, None, f))).filter(|s| !s.is_empty());
        let after = supplement.map(|s| format!(", {s}")).unwrap_or_default();
        let md = match form.as_str() {
            "prose" | "author" => keys.iter().map(|k| format!("@{k}")).collect::<Vec<_>>().join(", "),
            "year" => format!("[{}{after}]", keys.iter().map(|k| format!("-@{k}")).collect::<Vec<_>>().join("; ")),
            _ => format!("[{}{after}]", keys.iter().map(|k| format!("@{k}")).collect::<Vec<_>>().join("; ")),
        };
        flow.md(&md);
    }

    fn quote(&mut self, args: ast::Args, flow: &mut Flow) {
        let Some(b) = body(args) else { return };
        let block = named(args, "block").is_some_and(is_true);
        let attribution = named(args, "attribution").map(|a| match a {
            Expr::Label(l) => format!("[@{}]", l.get()),
            a => self.line(flow, |d, f| d.expr(a, None, f)),
        });
        if block && !flow.plain {
            let blocks = self.blocks(flow, |d, f| d.expr(b, None, f));
            let mut text = join(&blocks);
            if let Some(a) = attribution.filter(|a| !a.is_empty()) {
                text.push_str(&format!("\n\n— {a}"));
            }
            let quoted = text.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n");
            flow.block(quoted);
        } else {
            let inner = self.inline(flow, |d, f| d.expr(b, None, f));
            flow.md(&format!("“{}”", inner.trim()));
        }
    }

    /// A picture's markdown: `![alt](path){#id width=…}`.
    fn image(&self, args: ast::Args, caption: Option<&str>, id: Option<&str>) -> Option<String> {
        let path = positional(args).first().and_then(|&e| str_of(e))?;
        let alt = match caption {
            Some(c) => c.to_string(),
            None => {
                let mut alt = String::new();
                escape_into(&mut alt, &named(args, "alt").and_then(str_of).unwrap_or_default());
                alt
            }
        };
        let mut attrs = Vec::new();
        if let Some(id) = id {
            attrs.push(format!("#{id}"));
        }
        for key in ["width", "height"] {
            if let Some(Expr::Numeric(n)) = named(args, key) {
                attrs.push(format!("{key}={}", n.to_untyped().full_text()));
            }
        }
        let attrs = if attrs.is_empty() { String::new() } else { format!("{{{}}}", attrs.join(" ")) };
        Some(format!("![{alt}]({}){attrs}", path_destination(&self.asset(&path))))
    }

    fn figure(&mut self, args: ast::Args, label: Option<&str>, flow: &mut Flow) {
        let Some(body) = positional(args).into_iter().next() else { return };
        if flow.plain {
            return self.expr(body, None, flow);
        }
        let caption = named(args, "caption").map(|c| self.line(flow, |d, f| d.expr(c, None, f))).filter(|c| !c.is_empty());
        let kind = match named(args, "kind") {
            Some(Expr::Ident(i)) if i.as_str() == "table" => Figure::Table,
            Some(Expr::Ident(i)) if i.as_str() == "raw" => Figure::Other { key: "raw".into(), word: "Listing".into() },
            // What Quarto writes for its own figures, tables and listings.
            Some(Expr::Str(s)) if s.get() == "quarto-float-tbl" => Figure::Table,
            Some(Expr::Str(s)) if s.get() == "quarto-float-fig" => Figure::Picture,
            Some(Expr::Str(s)) if s.get() == "quarto-float-lst" => Figure::Other { key: "raw".into(), word: "Listing".into() },
            Some(Expr::Str(s)) => {
                let key = s.get().to_string();
                let word = named(args, "supplement").and_then(|e| self.plain(e)).unwrap_or_else(|| capitalized(&key));
                Figure::Other { key, word }
            }
            Some(_) => Figure::Picture,
            None => self.kind_of(body),
        };
        flow.end_para();
        match kind {
            Figure::Picture => {
                let id = label.map(|l| self.label(l, "fig")).or_else(|| caption.is_some().then(|| self.auto("fig")));
                if id.is_some() {
                    self.figures += 1;
                }
                if let Some(img) = single_image(body) {
                    if let Some(md) = self.image(img.args(), Some(caption.as_deref().unwrap_or("")), id.as_deref()) {
                        flow.block(md);
                    }
                    return;
                }
                let blocks = self.blocks(flow, |d, f| d.expr(body, None, f));
                flow.extend(blocks);
                // An empty span holds the id: Quarto counts it, the view
                // draws nothing of it, and the number is written here.
                match (id, caption) {
                    (Some(id), Some(c)) => flow.block(format!("*Figure {}: {c}*[]{{#{id}}}", self.figures)),
                    (Some(id), None) => flow.block(format!("[]{{#{id}}}")),
                    _ => {}
                }
            }
            Figure::Table => {
                match find_call(body.to_untyped(), "table") {
                    Some(t) => {
                        let md = self.table(t.args(), flow);
                        flow.block(md);
                    }
                    None => {
                        let blocks = self.blocks(flow, |d, f| d.expr(body, None, f));
                        flow.extend(blocks);
                    }
                }
                let id = label.map(|l| self.label(l, "tbl")).or_else(|| caption.is_some().then(|| self.auto("tbl")));
                if let Some(id) = id {
                    flow.block(format!(": {} {{#{id}}}", caption.unwrap_or_default()));
                }
            }
            Figure::Other { key, word } => {
                let n = self.counts.entry(key.clone()).or_insert(0);
                *n += 1;
                let words = format!("{word} {n}");
                if let Some(l) = label {
                    self.labels.entry(l.to_string()).or_insert_with(|| Target::Words(words.clone()));
                }
                let caption = caption.map(|c| format!("{words}: {c}"));
                // A picture of a kind of its own: its caption under it, as
                // a figure's, with the number written here.
                if let Some(img) = single_image(body) {
                    if let Some(md) = self.image(img.args(), Some(caption.as_deref().unwrap_or("")), None) {
                        flow.block(md);
                    }
                    return;
                }
                match (key.as_str(), find_raw(body.to_untyped())) {
                    ("raw", Some((text, lang))) => raw(&text, lang.as_deref(), true, flow),
                    _ => {
                        let blocks = self.blocks(flow, |d, f| d.expr(body, None, f));
                        flow.extend(blocks);
                    }
                }
                if let Some(c) = caption {
                    flow.block(format!("*{c}*"));
                }
            }
        }
    }

    /// A figure's kind from its body, looked for in the file when the body
    /// is an `#include`.
    fn kind_of(&self, body: Expr) -> Figure {
        if let Expr::ModuleInclude(include) = body {
            if let Expr::Str(s) = include.source() {
                if let Ok(text) = std::fs::read_to_string(self.dir.join(self.included(&s.get()))) {
                    return figure_kind(&typst_syntax::parse(&text)).unwrap_or(Figure::Picture);
                }
            }
        }
        if let Expr::Ident(id) = body {
            let bound = self.locals.iter().rev().find(|(n, _)| n == id.as_str()).and_then(|(_, node)| node.clone());
            if let Some(node) = bound {
                return figure_kind(&node).unwrap_or(Figure::Picture);
            }
        }
        figure_kind(body.to_untyped()).unwrap_or(Figure::Picture)
    }

    /// A table as a pipe table: its header `table.header`, else its first
    /// row. Cells are set as Typst sets them, row by row and past what a
    /// cell above spans; one that spans is its text, and empty cells over
    /// the rest.
    fn table(&mut self, args: ast::Args, flow: &Flow) -> String {
        let cols = named(args, "columns").map_or(1, count);
        let (aligns, all) = match named(args, "align") {
            Some(Expr::Array(a)) => (
                a.items()
                    .map(|i| match i {
                        ast::ArrayItem::Pos(e) => halign(e),
                        _ => None,
                    })
                    .collect(),
                None,
            ),
            Some(e) => (Vec::new(), halign(e)),
            None => (Vec::new(), None),
        };
        let mut grid = Grid { cols, ..Grid::default() };
        for e in positional(args) {
            if let Expr::FuncCall(c) = e {
                if matches!(dotted(c.callee()).as_deref(), Some("table.header" | "table.footer")) {
                    grid.next_row();
                    for x in positional(c.args()) {
                        self.cell(x, flow, &mut grid);
                    }
                    grid.next_row();
                    continue;
                }
            }
            self.cell(e, flow, &mut grid);
        }
        let mut rows = grid.rows.into_iter().map(|r| r.into_iter().map(Option::unwrap_or_default).collect::<Vec<_>>());
        let Some(head) = rows.next() else { return String::new() };
        let line = |row: &[String]| format!("| {} |", row.join(" | "));
        let rule = (0..cols)
            .map(|i| match aligns.get(i).copied().flatten().or(all) {
                Some('l') => ":---",
                Some('c') => ":---:",
                Some('r') => "---:",
                _ => "---",
            })
            .collect::<Vec<_>>()
            .join("|");
        let mut out = vec![line(&head), format!("|{rule}|")];
        out.extend(rows.map(|r| line(&r)));
        out.join("\n")
    }

    /// A table's cell on one line, its bars escaped, set in the grid.
    fn cell(&mut self, e: Expr, flow: &Flow, grid: &mut Grid) {
        let (mut colspan, mut rowspan) = (1, 1);
        if let Expr::FuncCall(c) = e {
            match dotted(c.callee()).as_deref() {
                Some("table.hline" | "table.vline") => return,
                Some("table.cell") => {
                    let span = |key: &str| named(c.args(), key).and_then(int).unwrap_or(1).clamp(1, MOST as i64) as usize;
                    colspan = span("colspan");
                    rowspan = span("rowspan");
                }
                _ => {}
            }
        }
        let text = self.line(flow, |d, f| d.expr(e, None, f)).replace('|', "\\|");
        grid.place(text, colspan, rowspan);
    }

    fn list_call(&mut self, list: List, args: ast::Args, flow: &mut Flow) {
        let mut start = named(args, "start").and_then(int).map(|n| n.max(0) as u64);
        for e in positional(args) {
            let mut number = None;
            let mut item = e;
            let mut term = None;
            if let Expr::FuncCall(c) = e {
                match dotted(c.callee()).as_deref() {
                    Some("list.item" | "enum.item") => {
                        let pos = positional(c.args());
                        number = pos.first().and_then(|&x| int(x)).map(|n| n.max(0) as u64);
                        match pos.last() {
                            Some(&b) => item = b,
                            None => continue,
                        }
                    }
                    Some("terms.item") => {
                        let pos = positional(c.args());
                        let (Some(&t), Some(&d)) = (pos.first(), pos.get(1)) else { continue };
                        term = Some(t);
                        item = d;
                    }
                    _ => {}
                }
            }
            if list == List::Term && term.is_none() {
                // A term list's item may also be a pair: `([T], [d])`.
                if let Expr::Array(a) = e {
                    let pair: Vec<Expr> = a
                        .items()
                        .filter_map(|i| match i {
                            ast::ArrayItem::Pos(x) => Some(x),
                            _ => None,
                        })
                        .collect();
                    if let [t, d] = pair[..] {
                        term = Some(t);
                        item = d;
                    }
                }
            }
            if term.is_none() && !is_content(item) {
                continue;
            }
            let term = term.map(|t| self.line(flow, |d, f| d.expr(t, None, f)));
            let number = number.or(start.take());
            self.list_item(list, number, term, flow, |d, f| d.expr(item, None, f));
        }
    }

    fn bibliography(&mut self, args: ast::Args, flow: &mut Flow) {
        let path = positional(args).into_iter().find_map(|e| match e {
            Expr::Str(s) => Some(s.get().to_string()),
            Expr::Array(a) => a.items().find_map(|i| match i {
                ast::ArrayItem::Pos(Expr::Str(s)) => Some(s.get().to_string()),
                _ => None,
            }),
            _ => None,
        });
        if let Some(p) = path {
            if self.bibliography.is_none() {
                self.bibliography = Some(self.asset(&p));
            }
        }
        if !flow.plain {
            flow.block("::: {#refs}\n:::".to_string());
        }
    }

    /// A function not known here: the content it is given, in order.
    fn unknown(&mut self, args: ast::Args, flow: &mut Flow) {
        let mut first = true;
        for a in args.items() {
            let e = match a {
                Arg::Pos(e) => e,
                Arg::Named(n) => n.expr(),
                Arg::Spread(_) => continue,
            };
            let content = match e {
                Expr::ContentBlock(_) | Expr::Equation(_) | Expr::Raw(_) | Expr::Ident(_) => true,
                Expr::FuncCall(_) => is_content(e),
                _ => false,
            };
            if !content {
                continue;
            }
            if !first {
                flow.space();
            }
            first = false;
            self.expr(e, None, flow);
        }
    }

    /// `#let name = …`: the value is kept as written, and read where the
    /// name stands, with what was bound before it as Typst reads it.
    fn binding(&mut self, binding: ast::LetBinding) {
        match binding.kind() {
            ast::LetBindingKind::Normal(ast::Pattern::Normal(Expr::Ident(id))) => {
                let init = binding.init().map(|e| e.to_untyped().clone());
                self.locals.push((id.as_str().to_string(), init));
            }
            ast::LetBindingKind::Closure(id) => self.locals.push((id.as_str().to_string(), None)),
            _ => {}
        }
    }

    /// What a name is bound to, written where it stands.
    fn name(&mut self, name: &str, flow: &mut Flow) {
        let Some(at) = self.locals.iter().rposition(|(n, _)| n == name) else { return };
        if self.expansions >= EXPANSIONS {
            return;
        }
        self.expansions += 1;
        let later = self.locals.split_off(at);
        if let Some(node) = later[0].1.clone() {
            if let Some(e) = node.cast::<Expr>() {
                self.expr(e, None, flow);
            }
        }
        self.locals.truncate(at);
        self.locals.extend(later);
    }

    /// A code block, read for the content it makes, as Typst joins it:
    /// its bindings are kept for the names after them, its rules read,
    /// and a call, content, a string or a name written. Values, loops
    /// and conditions write nothing.
    fn code(&mut self, code: ast::Code, flow: &mut Flow) {
        let scope = self.locals.len();
        for e in code.exprs() {
            if matches!(
                e,
                Expr::LetBinding(_) | Expr::SetRule(_) | Expr::ShowRule(_) | Expr::FuncCall(_) | Expr::ContentBlock(_) | Expr::Str(_) | Expr::Ident(_) | Expr::CodeBlock(_) | Expr::Contextual(_) | Expr::ModuleInclude(_)
            ) {
                self.expr(e, None, flow);
            }
        }
        self.locals.truncate(scope);
    }

    fn set_rule(&mut self, rule: ast::SetRule) {
        let Some(name) = dotted(rule.target()) else { return };
        let args = rule.args();
        match name.as_str() {
            "document" => self.meta(args, false),
            "heading" => {
                if let Some(n) = named(args, "numbering") {
                    self.number_sections = !matches!(n, Expr::None(_));
                }
            }
            "math.equation" => {
                if let Some(n) = named(args, "numbering") {
                    self.numbered_equations = !matches!(n, Expr::None(_));
                }
            }
            _ => {}
        }
    }

    /// `#show: project.with(..)`, `#show: conf(..)` or
    /// `#show: doc => conf(.., doc)`: a template, read for its arguments.
    fn show_rule(&mut self, rule: ast::ShowRule) {
        if rule.selector().is_some() {
            return;
        }
        let call = match rule.transform() {
            Expr::FuncCall(c) => Some(c),
            Expr::Closure(c) => match c.body() {
                Expr::FuncCall(c) => Some(c),
                _ => None,
            },
            _ => None,
        };
        if let Some(c) = call {
            self.meta(c.args(), true);
        }
    }

    /// The header's values from a document's or a template's arguments;
    /// what is set first is kept.
    fn meta(&mut self, args: ast::Args, template: bool) {
        for a in args.items() {
            let Arg::Named(n) = a else { continue };
            let v = n.expr();
            match n.name().as_str() {
                "title" if self.title.is_none() => self.title = self.plain(v),
                "author" | "authors" if self.authors.is_empty() => self.authors = self.names(v),
                "date" if self.date.is_none() => {
                    self.date = match v {
                        Expr::Str(_) | Expr::ContentBlock(_) => self.plain(v),
                        v => date(v),
                    }
                }
                // A template's `bibliography: bibliography("refs.bib")`.
                "bibliography" if template && self.bibliography.is_none() => {
                    let path = match v {
                        Expr::FuncCall(c) if dotted(c.callee()).as_deref() == Some("bibliography") => positional(c.args()).first().and_then(|&e| str_of(e)),
                        v => str_of(v),
                    };
                    self.bibliography = path.map(|p| self.asset(&p));
                }
                "abstract" if template && self.abstract_blocks.is_none() => {
                    if matches!(v, Expr::None(_) | Expr::Auto(_)) {
                        continue;
                    }
                    let mut flow = Flow::default();
                    self.expr(v, None, &mut flow);
                    flow.end_para();
                    if !flow.blocks.is_empty() {
                        self.abstract_blocks = Some(flow.blocks);
                    }
                }
                _ => {}
            }
        }
    }

    /// Authors' names: a string, content, a dictionary's `name`, or a list
    /// of them.
    fn names(&mut self, e: Expr) -> Vec<String> {
        match e {
            Expr::Str(_) | Expr::ContentBlock(_) | Expr::Ident(_) => self.plain(e).into_iter().collect(),
            Expr::Array(a) => {
                let items: Vec<Expr> = a
                    .items()
                    .filter_map(|i| match i {
                        ast::ArrayItem::Pos(x) => Some(x),
                        _ => None,
                    })
                    .collect();
                items.into_iter().flat_map(|x| self.names(x)).collect()
            }
            Expr::Dict(d) => {
                let name = d.items().find_map(|i| match i {
                    ast::DictItem::Named(n) if n.name().as_str() == "name" => Some(n.expr()),
                    _ => None,
                });
                name.and_then(|n| self.plain(n)).into_iter().collect()
            }
            Expr::Parenthesized(p) => self.names(p.expr()),
            _ => Vec::new(),
        }
    }

    /// An included file's path from the main document's folder: from the
    /// including file's folder, or from the root with a leading slash.
    fn included(&self, written: &str) -> PathBuf {
        match written.strip_prefix('/') {
            Some(r) => PathBuf::from(r),
            None => self.rel.join(written),
        }
    }

    fn include(&mut self, include: ast::ModuleInclude, flow: &mut Flow) {
        let Expr::Str(s) = include.source() else { return };
        let rel = self.included(&s.get());
        let path = self.dir.join(&rel);
        let key = canonical(&path);
        if self.seen.len() > DEPTH || self.seen.contains(&key) {
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        flow.end_para();
        let saved = std::mem::replace(&mut self.rel, rel.parent().map(Path::to_path_buf).unwrap_or_default());
        self.seen.push(key);
        self.source(&text, flow);
        self.seen.pop();
        self.rel = saved;
        flow.end_para();
    }

    /// A path as the main document reaches it: one written in an included
    /// file is from that file's folder.
    fn asset(&self, written: &str) -> String {
        if self.rel.as_os_str().is_empty() || written.starts_with('/') || has_scheme(written) {
            return written.to_string();
        }
        slashed(&self.rel.join(written))
    }

    fn finish(mut self, mut flow: Flow) -> String {
        flow.end_para();
        self.close_abstract(&mut flow);
        let mut parts = Vec::new();
        let header = self.header();
        if !header.is_empty() {
            parts.push(header);
        }
        if let Some(blocks) = self.abstract_blocks.take() {
            let body = match blocks.first() {
                Some(first) if first.kind == BlockKind::Para => {
                    let mut blocks = blocks;
                    blocks[0].md = format!("**Abstract.** {}", blocks[0].md);
                    join(&blocks)
                }
                _ => format!("**Abstract.**\n\n{}", join(&blocks)),
            };
            parts.push(format!("::: {{.abstract}}\n\n{body}\n\n:::"));
        }
        let body = join(&flow.blocks);
        if !body.is_empty() {
            parts.push(body);
        }
        if !self.footnotes.is_empty() {
            parts.push(self.footnotes.iter().enumerate().map(|(i, n)| format!("[^{}]: {n}", i + 1)).collect::<Vec<_>>().join("\n\n"));
        }
        let text = self.resolve(&parts.join("\n\n"));
        if text.is_empty() { text } else { text + "\n" }
    }

    fn header(&self) -> String {
        let mut lines = Vec::new();
        if let Some(t) = &self.title {
            lines.push(format!("title: {}", yaml(t)));
        }
        if !self.authors.is_empty() {
            lines.push(format!("author: {}", yaml(&and_list(&self.authors))));
        }
        if let Some(d) = &self.date {
            lines.push(format!("date: {}", yaml(d)));
        }
        if let Some(b) = &self.bibliography {
            lines.push(format!("bibliography: {}", yaml(b)));
        }
        if self.number_sections {
            lines.push("number-sections: true".to_string());
        }
        if lines.is_empty() { String::new() } else { format!("---\n{}\n---", lines.join("\n")) }
    }

    /// Every reference written: `@id` for a label, its words for a figure
    /// Quarto has no kind for, its note for a footnote's, else a citation.
    /// Citations with only spaces between them are one group, as Typst
    /// groups them: `[@a; @b]`.
    fn resolve(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        let mut cited: Option<usize> = None;
        while let Some(at) = rest.find(REF_OPEN) {
            out.push_str(&rest[..at]);
            let after = &rest[at + REF_OPEN.len_utf8()..];
            let Some(end) = after.find(REF_CLOSE) else {
                rest = after;
                continue;
            };
            let inner = &after[..end];
            rest = &after[end + REF_CLOSE.len_utf8()..];
            let (target, supplement) = inner.split_once(REF_SUPP).unwrap_or((inner, ""));
            match (self.labels.get(target), self.notes.get(target)) {
                (Some(Target::Id(id)), _) => out.push_str(&format!("@{id}")),
                (Some(Target::Words(w)), _) => out.push_str(w),
                (None, Some(n)) => out.push_str(&format!("[^{n}]")),
                (None, None) => {
                    let cite = if supplement.is_empty() { format!("@{target}") } else { format!("@{target}, {supplement}") };
                    match cited.filter(|&e| e <= out.len() && out[e..].trim().is_empty()) {
                        Some(e) => {
                            out.truncate(e - 1);
                            out.push_str(&format!("; {cite}]"));
                        }
                        None => out.push_str(&format!("[{cite}]")),
                    }
                    cited = Some(out.len());
                }
            }
        }
        out.push_str(rest);
        out
    }
}

/// A table's cells in rows, set as Typst sets them: each in the next
/// place no cell above covers.
#[derive(Default)]
struct Grid {
    cols: usize,
    rows: Vec<Vec<Option<String>>>,
    r: usize,
    c: usize,
}

impl Grid {
    fn place(&mut self, text: String, colspan: usize, rowspan: usize) {
        loop {
            if self.c >= self.cols {
                self.r += 1;
                self.c = 0;
            }
            self.reach(self.r);
            if self.rows[self.r][self.c].is_none() {
                break;
            }
            self.c += 1;
        }
        let colspan = colspan.clamp(1, self.cols - self.c);
        for dr in 0..rowspan.max(1) {
            self.reach(self.r + dr);
            for dc in 0..colspan {
                self.rows[self.r + dr][self.c + dc] = Some(if dr == 0 && dc == 0 { text.clone() } else { String::new() });
            }
        }
        self.c += colspan;
    }

    /// A header or a footer starts a row of its own, and so does what
    /// follows it.
    fn next_row(&mut self) {
        if self.c > 0 {
            self.r += 1;
            self.c = 0;
        }
    }

    fn reach(&mut self, r: usize) {
        while self.rows.len() <= r {
            self.rows.push(vec![None; self.cols]);
        }
    }
}

/// Whether a label after this element is its own.
fn labelled(e: Expr) -> bool {
    match e {
        Expr::Heading(_) | Expr::Equation(_) => true,
        Expr::FuncCall(c) => matches!(dotted(c.callee()).as_deref(), Some("figure" | "heading" | "footnote")),
        _ => false,
    }
}

/// The label after the element at `i`, past spaces, and where it is.
fn label_after<'a>(exprs: &[Expr<'a>], i: usize) -> Option<(usize, &'a str)> {
    for (j, e) in exprs.iter().enumerate().skip(i + 1) {
        match e {
            Expr::Space(_) | Expr::Parbreak(_) => continue,
            Expr::Label(l) => return Some((j, l.get())),
            _ => return None,
        }
    }
    None
}

/// The id Quarto reads for a label on an element of `kind`: the kind's
/// prefix, then the label with any prefix of its own taken off and what
/// an id cannot hold turned into `-`.
fn label_id(kind: &str, label: &str) -> String {
    let mut name = label;
    for prefix in ["fig-", "fig:", "tbl-", "tbl:", "tab-", "tab:", "eq-", "eq:", "sec-", "sec:"] {
        if let Some(rest) = name.strip_prefix(prefix).filter(|r| !r.is_empty()) {
            name = rest;
            break;
        }
    }
    let clean: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect();
    format!("{kind}-{clean}")
}

/// A reference, held until every label is known (`Doc::resolve`).
fn reference(target: &str, supplement: Option<String>, flow: &mut Flow) {
    if flow.plain {
        flow.md(target);
        return;
    }
    let mut s = String::new();
    s.push(REF_OPEN);
    s.push_str(target);
    if let Some(supp) = supplement.filter(|s| !s.is_empty()) {
        s.push(REF_SUPP);
        s.push_str(&supp);
    }
    s.push(REF_CLOSE);
    flow.md(&s);
}

/// Raw text: a fenced block, or a code span, each with more backticks
/// than the text has in a row.
fn raw(text: &str, lang: Option<&str>, block: bool, flow: &mut Flow) {
    if flow.plain {
        flow.md(&one_line(text));
        return;
    }
    let ticks = longest_run(text, '`') + 1;
    if block {
        let fence = "`".repeat(ticks.max(3));
        let lang = lang.unwrap_or("");
        let md = if text.is_empty() { format!("{fence}{lang}\n{fence}") } else { format!("{fence}{lang}\n{text}\n{fence}") };
        flow.block(md);
    } else {
        let text = one_line(text);
        if text.is_empty() {
            return;
        }
        let fence = "`".repeat(ticks);
        let pad = if text.starts_with('`') || text.ends_with('`') { " " } else { "" };
        flow.md(&format!("{fence}{pad}{text}{pad}{fence}"));
    }
}

fn longest_run(text: &str, c: char) -> usize {
    let (mut best, mut run) = (0, 0);
    for x in text.chars() {
        run = if x == c { run + 1 } else { 0 };
        best = best.max(run);
    }
    best
}

/// Cells laid side by side, `cols` to a row; one column is the cells one
/// after another.
fn layout(cols: usize, cells: Vec<String>, flow: &mut Flow) {
    if cols > 1 && cells.len() > 1 {
        flow.block(format!("::: {{layout-ncol={cols}}}\n\n{}\n\n:::", cells.join("\n\n")));
    } else {
        for c in cells {
            flow.block(c);
        }
    }
}

/// An item's markdown: the marker, then its blocks with every line after
/// the first indented four spaces. A first block that is not a paragraph
/// starts on the next line, so its indent is the rest's.
fn indent_item(marker: &str, blocks: &[Block]) -> String {
    let body = join(blocks);
    if body.is_empty() {
        return marker.trim_end().to_string();
    }
    let pad = " ".repeat(marker.len().max(4));
    let inline = blocks.first().is_some_and(|b| b.kind == BlockKind::Para);
    let mut out = if inline { String::from(marker) } else { format!("{}\n", marker.trim_end()) };
    for (i, line) in body.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if inline && i == 0 {
            out.push_str(line);
        } else if !line.is_empty() {
            out.push_str(&pad);
            out.push_str(line);
        }
    }
    out
}

/// Blocks apart by a blank line. A list or code right under a
/// paragraph's line in the source stays under it, so the list around
/// them stays tight; markdown reads it so when what follows may break
/// into a paragraph (a bullet, a count from 1, a fence).
fn join(blocks: &[Block]) -> String {
    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            let breaks_in = match b.kind {
                BlockKind::List(_) => b.md.starts_with("- ") || b.md.starts_with("1. "),
                BlockKind::Other => b.md.starts_with("```"),
                BlockKind::Para => false,
            };
            let under = b.tight && blocks[i - 1].kind == BlockKind::Para && breaks_in;
            out.push_str(if under { "\n" } else { "\n\n" });
        }
        out.push_str(&b.md);
    }
    out
}

/// `text` on one line: a hard break or a newline is a space.
fn one_line(text: &str) -> String {
    text.replace("\\\n", " ").split('\n').map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Text with what markdown reads as syntax escaped. `#` is escaped at a
/// line's start (`guard`), `|` in a table's cell.
fn escape_into(out: &mut String, text: &str) {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '$' | '@' | '~' => {
                out.push('\\');
                out.push(c);
            }
            // `&amp;` and `&#38;` would be read as the character they name.
            '&' if chars.peek().is_some_and(|n| n.is_ascii_alphanumeric() || *n == '#') => out.push_str("\\&"),
            c => out.push(c),
        }
    }
}

/// A paragraph's line, escaped where its start would open a block: a
/// heading, a list's item, a rule, a div's fence.
fn guard(line: &str) -> String {
    let b = line.as_bytes();
    let blank = |i: usize| b.get(i).is_none_or(|c| *c == b' ' || *c == b'\t');
    let at = match b.first() {
        Some(b'#') => Some(0),
        Some(&c @ (b'-' | b'+' | b'=')) if blank(1) || line.bytes().all(|x| x == c || x == b' ') => Some(0),
        Some(b':') if line.starts_with(":::") => Some(0),
        Some(b'0'..=b'9') => {
            let n = b.iter().take_while(|c| c.is_ascii_digit()).count();
            (n <= 9 && matches!(b.get(n), Some(b'.' | b')')) && blank(n + 1)).then_some(n)
        }
        _ => None,
    };
    match at {
        Some(at) => format!("{}\\{}", &line[..at], &line[at..]),
        None => line.to_string(),
    }
}

fn has_scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(s, _)| s.len() > 1 && s.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)))
}

/// A URL that markdown and `quarto::expand` leave whole: no space, no
/// angle bracket, and no `@` read as a citation.
fn url_safe(url: &str) -> String {
    let mail = url.starts_with("mailto:");
    url.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '<' => "%3C".to_string(),
            '>' => "%3E".to_string(),
            '@' if !mail => "%40".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// A link's destination.
fn destination(url: &str) -> String {
    let url = url_safe(url);
    if url.contains(['(', ')']) { format!("<{url}>") } else { url }
}

/// A picture's path as a destination: in angle brackets when it has a
/// space or a bracket.
fn path_destination(path: &str) -> String {
    if path.contains([' ', '(', ')']) { format!("<{path}>") } else { path.to_string() }
}

/// A path with `/` between its parts, `..` taken back where it can be.
fn slashed(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::ParentDir if parts.last().is_some_and(|p| p != "..") => {
                parts.pop();
            }
            Component::ParentDir => parts.push("..".to_string()),
            _ => {}
        }
    }
    parts.join("/")
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A YAML value, in double quotes when YAML would read it as more than
/// plain words.
fn yaml(value: &str) -> String {
    let special = value.is_empty()
        || value.contains([':', '#'])
        || value.starts_with(|c: char| c.is_whitespace() || "-?:,[]{}#&*!|>'\"%@`".contains(c))
        || value.ends_with(char::is_whitespace);
    if special { format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")) } else { value.to_string() }
}

/// "A", "A and B", "A, B and C".
fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `n` words of lorem ipsum, ending a sentence.
fn lorem(n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let words: Vec<&str> = LOREM.split(' ').collect();
    let mut text = (0..n).map(|i| words[i % words.len()]).collect::<Vec<_>>().join(" ");
    let kept = text.trim_end_matches([',', ';', ':']).len();
    text.truncate(kept);
    if !text.ends_with(['.', '?', '!']) {
        text.push('.');
    }
    text
}

/// A character's Unicode superscript or subscript.
fn small(c: char, up: bool) -> Option<char> {
    const SUPER: &[(char, char)] = &[
        ('0', '⁰'), ('1', '¹'), ('2', '²'), ('3', '³'), ('4', '⁴'), ('5', '⁵'), ('6', '⁶'), ('7', '⁷'), ('8', '⁸'), ('9', '⁹'), ('+', '⁺'), ('-', '⁻'), ('=', '⁼'), ('(', '⁽'), (')', '⁾'), ('a', 'ᵃ'),
        ('b', 'ᵇ'), ('c', 'ᶜ'), ('d', 'ᵈ'), ('e', 'ᵉ'), ('f', 'ᶠ'), ('g', 'ᵍ'), ('h', 'ʰ'), ('i', 'ⁱ'), ('j', 'ʲ'), ('k', 'ᵏ'), ('l', 'ˡ'), ('m', 'ᵐ'), ('n', 'ⁿ'), ('o', 'ᵒ'), ('p', 'ᵖ'), ('r', 'ʳ'),
        ('s', 'ˢ'), ('t', 'ᵗ'), ('u', 'ᵘ'), ('v', 'ᵛ'), ('w', 'ʷ'), ('x', 'ˣ'), ('y', 'ʸ'), ('z', 'ᶻ'),
    ];
    const SUB: &[(char, char)] = &[
        ('0', '₀'), ('1', '₁'), ('2', '₂'), ('3', '₃'), ('4', '₄'), ('5', '₅'), ('6', '₆'), ('7', '₇'), ('8', '₈'), ('9', '₉'), ('+', '₊'), ('-', '₋'), ('=', '₌'), ('(', '₍'), (')', '₎'), ('a', 'ₐ'),
        ('e', 'ₑ'), ('h', 'ₕ'), ('i', 'ᵢ'), ('j', 'ⱼ'), ('k', 'ₖ'), ('l', 'ₗ'), ('m', 'ₘ'), ('n', 'ₙ'), ('o', 'ₒ'), ('p', 'ₚ'), ('r', 'ᵣ'), ('s', 'ₛ'), ('t', 'ₜ'), ('u', 'ᵤ'), ('v', 'ᵥ'), ('x', 'ₓ'),
    ];
    if c == ' ' {
        return Some(' ');
    }
    let table = if up { SUPER } else { SUB };
    table.iter().find(|(from, _)| *from == c).map(|(_, to)| *to)
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// A callee's name, dotted: `table.cell`. None for a method's call.
fn dotted(e: Expr) -> Option<String> {
    match e {
        Expr::Ident(i) => Some(i.as_str().to_string()),
        Expr::FieldAccess(f) => Some(format!("{}.{}", dotted(f.target())?, f.field().as_str())),
        _ => None,
    }
}

fn is_value(name: &str) -> bool {
    let head = name.split('.').next().unwrap_or(name);
    VALUES.contains(&name) || VALUES.contains(&head) || name.ends_with(".with")
}

/// Whether an argument is content: a block, a string, math, raw text, a
/// name (what it is bound to, if anything), or a call that makes content.
fn is_content(e: Expr) -> bool {
    match e {
        Expr::ContentBlock(_) | Expr::Str(_) | Expr::Equation(_) | Expr::Raw(_) | Expr::Ident(_) => true,
        Expr::FuncCall(c) => dotted(c.callee()).is_some_and(|n| !is_value(&n)),
        _ => false,
    }
}

fn named<'a>(args: ast::Args<'a>, name: &str) -> Option<Expr<'a>> {
    args.items().find_map(|a| match a {
        Arg::Named(n) if n.name().as_str() == name => Some(n.expr()),
        _ => None,
    })
}

fn positional<'a>(args: ast::Args<'a>) -> Vec<Expr<'a>> {
    args.items()
        .filter_map(|a| match a {
            Arg::Pos(e) => Some(e),
            _ => None,
        })
        .collect()
}

/// A function's content: its last positional argument, when content.
fn body<'a>(args: ast::Args<'a>) -> Option<Expr<'a>> {
    positional(args).last().copied().filter(|&e| is_content(e))
}

fn str_of(e: Expr) -> Option<String> {
    match e {
        Expr::Str(s) => Some(s.get().to_string()),
        _ => None,
    }
}

fn int(e: Expr) -> Option<i64> {
    match e {
        Expr::Int(i) => Some(i.get()),
        _ => None,
    }
}

fn is_true(e: Expr) -> bool {
    matches!(e, Expr::Bool(b) if b.get())
}

/// A horizontal alignment: `l`, `c` or `r`, from `center`, `right + top`…
fn halign(e: Expr) -> Option<char> {
    halign_in(e, 0)
}

fn halign_in(e: Expr, depth: usize) -> Option<char> {
    if depth > 8 {
        return None;
    }
    match e {
        Expr::Ident(i) => match i.as_str() {
            "left" | "start" => Some('l'),
            "center" => Some('c'),
            "right" | "end" => Some('r'),
            _ => None,
        },
        Expr::Binary(b) => halign_in(b.lhs(), depth + 1).or_else(|| halign_in(b.rhs(), depth + 1)),
        Expr::Parenthesized(p) => halign_in(p.expr(), depth + 1),
        _ => None,
    }
}

/// How many columns `columns:` gives: a count, or a list of widths.
fn count(e: Expr) -> usize {
    let n = match e {
        Expr::Int(i) => i.get().clamp(1, MOST as i64) as usize,
        Expr::Array(a) => a.items().take(MOST).count().max(1),
        // `(1fr,) * 3`
        Expr::Binary(b) if b.op() == ast::BinOp::Mul => match (b.lhs(), int(b.rhs())) {
            (Expr::Array(a), Some(times)) => a.items().take(MOST).count().max(1).saturating_mul(times.clamp(1, MOST as i64) as usize),
            _ => 1,
        },
        Expr::Parenthesized(p) => match p.expr() {
            Expr::Int(i) => i.get().clamp(1, MOST as i64) as usize,
            _ => 1,
        },
        _ => 1,
    };
    n.min(MOST)
}

/// `datetime(year: 2026, month: 3, day: 1)` as "2026-03-01".
fn date(e: Expr) -> Option<String> {
    let Expr::FuncCall(c) = e else { return None };
    if dotted(c.callee()).as_deref() != Some("datetime") {
        return None;
    }
    let part = |k: &str| named(c.args(), k).and_then(int);
    let year = part("year")?;
    Some(match (part("month"), part("day")) {
        (Some(m), Some(d)) => format!("{year:04}-{m:02}-{d:02}"),
        (Some(m), None) => format!("{year:04}-{m:02}"),
        _ => year.to_string(),
    })
}

/// A figure's kind as Typst reads it: of the first picture, table or raw
/// text in its body. None when there is none, and it is a picture.
fn figure_kind(node: &SyntaxNode) -> Option<Figure> {
    figure_kind_in(node, 0)
}

fn figure_kind_in(node: &SyntaxNode, depth: usize) -> Option<Figure> {
    if depth > NESTING {
        return None;
    }
    let listing = || Figure::Other { key: "raw".into(), word: "Listing".into() };
    if node.kind() == SyntaxKind::Raw {
        return Some(listing());
    }
    if let Some(c) = node.cast::<ast::FuncCall>() {
        match dotted(c.callee()).as_deref() {
            Some("image") => return Some(Figure::Picture),
            Some("table") => return Some(Figure::Table),
            Some("raw") => return Some(listing()),
            _ => {}
        }
    }
    node.children().find_map(|child| figure_kind_in(child, depth + 1))
}

/// The first call of `name` in `node`, itself or within.
fn find_call<'a>(node: &'a SyntaxNode, name: &str) -> Option<ast::FuncCall<'a>> {
    find_call_in(node, name, 0)
}

fn find_call_in<'a>(node: &'a SyntaxNode, name: &str, depth: usize) -> Option<ast::FuncCall<'a>> {
    if depth > NESTING {
        return None;
    }
    if let Some(c) = node.cast::<ast::FuncCall>() {
        if dotted(c.callee()).as_deref() == Some(name) {
            return Some(c);
        }
    }
    node.children().find_map(|child| find_call_in(child, name, depth + 1))
}

/// The first raw text in `node`: its text and its language.
fn find_raw(node: &SyntaxNode) -> Option<(String, Option<String>)> {
    find_raw_in(node, 0)
}

fn find_raw_in(node: &SyntaxNode, depth: usize) -> Option<(String, Option<String>)> {
    if depth > NESTING {
        return None;
    }
    if let Some(r) = node.cast::<ast::Raw>() {
        let text = r.lines().map(|l| l.get().as_str()).collect::<Vec<_>>().join("\n");
        return Some((text, r.lang().map(|l| l.get().as_str().to_lowercase())));
    }
    if let Some(c) = node.cast::<ast::FuncCall>() {
        if dotted(c.callee()).as_deref() == Some("raw") {
            let args = c.args();
            let text = positional(args).first().and_then(|&e| str_of(e))?;
            return Some((text, named(args, "lang").and_then(str_of).map(|l| l.to_lowercase())));
        }
    }
    node.children().find_map(|child| find_raw_in(child, depth + 1))
}

/// The one picture a figure's body is, through a block around it.
fn single_image(e: Expr) -> Option<ast::FuncCall> {
    single_image_in(e, 0)
}

fn single_image_in(e: Expr, depth: usize) -> Option<ast::FuncCall> {
    if depth > 8 {
        return None;
    }
    match e {
        Expr::FuncCall(c) => match dotted(c.callee()).as_deref() {
            Some("image") => Some(c),
            Some("align" | "box" | "block" | "pad") => body(c.args()).and_then(|b| single_image_in(b, depth + 1)),
            _ => None,
        },
        Expr::ContentBlock(b) => {
            let mut exprs = b.body().exprs().filter(|e| !matches!(e, Expr::Space(_) | Expr::Parbreak(_)));
            let first = exprs.next()?;
            if exprs.next().is_some() {
                return None;
            }
            single_image_in(first, depth + 1)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(typst: &str) -> String {
        to_markdown(typst, Path::new("/nowhere/doc.typ"))
    }

    #[test]
    fn a_template_gives_the_header_and_the_abstract() {
        let typst = "#import \"template.typ\": *\n#show: project.with(\n  title: \"T\",\n  authors: ((name: \"A\", affiliation: \"U\"), (name: \"B\")),\n  date: \"2026\",\n  abstract: [We study _things_.],\n)\n\n= Intro\nText.\n";
        assert_eq!(md(typst), "---\ntitle: T\nauthor: A and B\ndate: 2026\n---\n\n::: {.abstract}\n\n**Abstract.** We study *things*.\n\n:::\n\n# Intro\n\nText.\n");
    }

    #[test]
    fn set_document_fills_the_header() {
        let typst = "#set document(title: [A: Study], author: (\"Ann\", \"Bo\", \"Cy\"), date: datetime(year: 2026, month: 3, day: 1))\n#set heading(numbering: \"1.\")\nBody.";
        assert_eq!(md(typst), "---\ntitle: \"A: Study\"\nauthor: Ann, Bo and Cy\ndate: 2026-03-01\nnumber-sections: true\n---\n\nBody.\n");
        assert_eq!(md("#show: conf.with(title: [Say \"hi\"], author: (name: [Dee]), bibliography: bibliography(\"refs.bib\"))"), "---\ntitle: Say “hi”\nauthor: Dee\nbibliography: refs.bib\n---\n");
    }

    #[test]
    fn headings_take_their_labels_and_references_name_them() {
        let typst = "= Introduction <intro>\n== Methods <sec:methods>\nSee @intro and @sec:methods.\n#heading(level: 3, numbering: none)[Extra]\n#heading(level: 2, outlined: false)[Quiet]";
        assert_eq!(md(typst), "# Introduction {#sec-intro}\n\n## Methods {#sec-methods}\n\nSee @sec-intro and @sec-methods.\n\n### Extra {.unnumbered}\n\n## Quiet\n");
    }

    #[test]
    fn inline_marks_and_links() {
        let typst = "*b* _e_ `x` ``` a`b ``` #strike[s] #underline[u] #highlight[h] #emph[m] #strong[g] #upper[up] #lower[LO] H#sub[2]O x#super[n] #smallcaps[Sc] #text(red)[t] #box[b]";
        assert_eq!(md(typst), "**b** *e* `x` ``a`b`` ~~s~~ <u>u</u> <mark>h</mark> *m* **g** UP lo H₂O xⁿ Sc t b\n");
        let typst = "#link(\"https://typst.app\")[Typst] #link(\"https://x.org\") and https://typst.org/docs.";
        assert_eq!(md(typst), "[Typst](https://typst.app) <https://x.org> and <https://typst.org/docs>.\n");
        assert_eq!(md("one \\\ntwo #linebreak() three \\ \\\nfour"), "one\\\ntwo\\\nthree\\\nfour\n");
        assert_eq!(md("#link(\"mailto:a@b.org\") and #link(\"https://x.org/@me\")[me]"), "[a\\@b.org](mailto:a@b.org) and [me](https://x.org/%40me)\n");
        // An escaped backslash at a cell's end stays.
        assert_eq!(md("#table(columns: 1, [a\\\\], [b])"), "| a\\\\ |\n|---|\n| b |\n");
    }

    #[test]
    fn text_that_markdown_reads_is_escaped() {
        let typst = "\\#1 is first.\n\nPrice: \\$5, mail\\@x, snake_case a*b, a < b > c, [x], \\~ and \\\\.\n\n2) not a list\n\nAn emoji \\u{1F600}.";
        assert_eq!(md(typst), "\\#1 is first.\n\nPrice: \\$5, mail\\@x, snake\\_case a\\*b, a \\< b \\> c, \\[x\\], \\~ and \\\\.\n\n2\\) not a list\n\nAn emoji 😀.\n");
    }

    #[test]
    fn quotes_and_dashes_are_typography() {
        assert_eq!(md("\"Hi,\" she said -- it's 1--2 --- done... a~b ex-?ample"), "“Hi,” she said – it’s 1–2 — done… a\u{a0}b example\n");
    }

    #[test]
    fn lists_of_every_kind_nest() {
        let typst = "- a\n  - b\n- c\n\n+ one\n+ two\n  / T: d\n3. three\n+ four\n\n/ Term: Its *words*.";
        assert_eq!(md(typst), "- a\n    - b\n- c\n\n1. one\n2. two\n    - **T**: d\n3. three\n4. four\n\n- **Term**: Its **words**.\n");
        assert_eq!(md("#enum(start: 3)[a][b]\n\n#list([x], [y])"), "3. a\n4. b\n\n- x\n- y\n");
        // A paragraph after the first in an item, and code in one.
        assert_eq!(md("- a\n\n  more\n- ```py\n  x = 1\n  ```"), "- a\n\n    more\n-\n    ```py\n    x = 1\n    ```\n");
        assert_eq!(md("- Run:\n  ```sh\n  make\n  ```\n- Done."), "- Run:\n    ```sh\n    make\n    ```\n- Done.\n");
    }

    #[test]
    fn a_figure_of_a_picture() {
        let typst = "#figure(image(\"a.png\", width: 60%), caption: [A cat.]) <fig-a>\n\n#figure(image(\"b.png\"), caption: [B])\n\n#figure(image(\"c d.png\"))\n\nSee @fig-a. #image(\"e.png\")";
        assert_eq!(md(typst), "![A cat.](a.png){#fig-a width=60%}\n\n![B](b.png){#fig-auto-1}\n\n![](<c d.png>)\n\nSee @fig-a. ![](e.png)\n");
        assert_eq!(md("#figure(rect[Boxed], caption: [Odd]) <odd>\nSee @odd."), "Boxed\n\n*Figure 1: Odd*[]{#fig-odd}\n\nSee @fig-odd.\n");
    }

    #[test]
    fn a_figure_of_a_table() {
        let typst = "#figure(\n  table(\n    columns: 2,\n    align: center,\n    table.header([Name], [Age]),\n    [Ann], [30],\n    [Bo|b], [4],\n  ),\n  caption: [People],\n) <people>\nSee @people.";
        assert_eq!(md(typst), "| Name | Age |\n|:---:|:---:|\n| Ann | 30 |\n| Bo\\|b | 4 |\n\n: People {#tbl-people}\n\nSee @tbl-people.\n");
        let typst = "#table(columns: (1fr, auto, auto), align: (left, right, center), [a], [b], [c], table.hline(), table.cell(colspan: 2)[wide], [z], [last])";
        assert_eq!(md(typst), "| a | b | c |\n|:---|---:|:---:|\n| wide |  | z |\n| last |  |  |\n");
        assert_eq!(md("#figure(table(columns: 1, [h], [v]), caption: [T])"), "| h |\n|---|\n| v |\n\n: T {#tbl-auto-1}\n");
    }

    #[test]
    fn a_figure_of_a_kind_of_its_own() {
        let typst = "#figure(image(\"s.png\", width: 95%), caption: [Car Ownership], kind: \"summary\", supplement: [Summary]) <s1>\n#figure(rect[Body], caption: [Two], kind: \"summary\")\nSee @s1.";
        assert_eq!(md(typst), "![Summary 1: Car Ownership](s.png){width=95%}\n\nBody\n\n*Summary 2: Two*\n\nSee Summary 1.\n");
    }

    #[test]
    fn a_figure_of_an_included_table() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("t.typ"), "#table(columns: 2, [a], [b], [c], [d])").unwrap();
        let file = dir.path().join("doc.typ");
        let out = to_markdown("#figure(include \"t.typ\", caption: \"Inc\") <t>\nSee @t.", &file);
        assert_eq!(out, "| a | b |\n|---|---|\n| c | d |\n\n: Inc {#tbl-t}\n\nSee @tbl-t.\n");
    }

    #[test]
    fn a_figure_of_code() {
        let typst = "#figure(\n```rust\nfn main() {}\n```,\n  caption: [Code],\n) <lst>\nSee @lst.";
        assert_eq!(md(typst), "```rust\nfn main() {}\n```\n\n*Listing 1: Code*\n\nSee Listing 1.\n");
    }

    #[test]
    fn math_is_left_as_typst_wrote_it() {
        let typst = "#set math.equation(numbering: \"(1)\")\nInline $a_1 * b^2$ here and $x +\n y$.\n$ E = m c^2 $ <energy>\n$ x &= 1 \\\n  &= y $\nSee @energy.";
        assert_eq!(md(typst), "Inline $a_1 * b^2$ here and $x + y$.\n\n$$\nE = m c^2\n$$ {#eq-energy}\n\n$$\nx &= 1 \\\n  &= y\n$$ {#eq-auto-1}\n\nSee @eq-energy.\n");
        assert_eq!(md("$ y $\n\n$ z $ <z>"), "$$\ny\n$$\n\n$$\nz\n$$ {#eq-z}\n");
    }

    #[test]
    fn footnotes_are_listed_at_the_end() {
        let typst = "Text#footnote[A note.] more#footnote[Second *one*.] <fn>.\n\nAgain#footnote(<fn>).";
        assert_eq!(md(typst), "Text[^1] more[^2].\n\nAgain[^2].\n\n[^1]: A note.\n\n[^2]: Second **one**.\n");
    }

    #[test]
    fn citations_in_each_form() {
        let typst = "#cite(<smith>) #cite(<smith>, supplement: [p. 7]) #cite(<smith>, form: \"prose\") #cite(<smith>, form: \"year\") and @smith, @smith[p. 3] @doe.";
        assert_eq!(md(typst), "[@smith] [@smith, p. 7] @smith [-@smith] and [@smith], [@smith, p. 3; @doe].\n");
        let typst = "As @smith shows.\n\n#bibliography(\"refs.bib\")";
        assert_eq!(md(typst), "---\nbibliography: refs.bib\n---\n\nAs [@smith] shows.\n\n::: {#refs}\n:::\n");
    }

    #[test]
    fn lorem_writes_its_words() {
        assert_eq!(md("#lorem(5)"), "Lorem ipsum dolor sit amet.\n");
        assert_eq!(lorem(70).split(' ').count(), 70);
        assert!(lorem(70).ends_with("laborum. Lorem."));
    }

    #[test]
    fn layout_functions_give_their_content() {
        assert_eq!(md("#align(center)[Centered *text*]"), "::: {style=\"text-align: center\"}\n\nCentered **text**\n\n:::\n");
        assert_eq!(md("#align(right, [R])\n#align(left)[L]"), "::: {style=\"text-align: right\"}\n\nR\n\n:::\n\nL\n");
        assert_eq!(md("#grid(columns: 2, gutter: 1em, [A], image(\"b.png\"))"), "::: {layout-ncol=2}\n\nA\n\n![](b.png)\n\n:::\n");
        assert_eq!(md("#stack(dir: ltr, [A], 1em, [B])\n#stack([C], [D])"), "::: {layout-ncol=2}\n\nA\n\nB\n\n:::\n\nC\n\nD\n");
        assert_eq!(md("#columns(2)[Left]\n#block(inset: 4pt)[In]#pad(x: 1em)[Pad]\n#hide[gone]#rect[R]"), "Left\n\nIn\n\nPad\n\nR\n");
    }

    #[test]
    fn quotes_as_blocks_and_inline() {
        assert_eq!(md("#quote(block: true, attribution: [Ann])[Words here.]"), "> Words here.\n>\n> — Ann\n");
        assert_eq!(md("She said #quote[hi] once."), "She said “hi” once.\n");
    }

    #[test]
    fn a_raw_block_is_left_alone() {
        let typst = "```py\nx = \"*a*\" # $5 @b\n  _y_ <z>\n```";
        assert_eq!(md(typst), "```py\nx = \"*a*\" # $5 @b\n  _y_ <z>\n```\n");
        assert_eq!(md("#raw(\"let a = 1\", lang: \"JS\", block: true)"), "```js\nlet a = 1\n```\n");
        assert_eq!(md("````md\n```inner```\n````"), "````md\n```inner```\n````\n");
    }

    #[test]
    fn code_writes_nothing() {
        let typst = "#let x = 1\n#set text(size: 11pt)\n#show heading: it => it.body\n// a comment\n/* a block\ncomment */\n#let name = \"Ann\"\n#let greet(who) = [Hi #who]\n#{ let y = 2 }\n#context counter(page).display()\n#counter(heading).update(0)\n#pagebreak()\n#v(1em)\n#outline()\nHello#h(1em)#name.";
        assert_eq!(md(typst), "Hello Ann.\n");
    }

    #[test]
    fn names_are_read_where_they_stand() {
        assert_eq!(md("#let who = [*Ann*]\n#let n = 3\nHi #who, #n times.\n#let who = [Bo]\n#who."), "Hi **Ann**, 3 times. Bo.\n");
        // A binding in a block is the block's own.
        assert_eq!(md("#[#let x = [in]\n#x] #x"), "in\n");
        // A value is read with what was bound before it.
        assert_eq!(md("#let x = [a]\n#let x = [#x#x]\n#x"), "aa\n");
        assert_eq!(md("#let t = [T]\n#show: conf.with(title: t, authors: (t,))"), "---\ntitle: T\nauthor: T\n---\n");
    }

    #[test]
    fn a_code_block_writes_the_content_it_makes() {
        // As Quarto writes a table with notes under it.
        let typst = "#align(center)[\n  #context {\n    let tbl = table(columns: 2, table.header([*A*], [*B*]), [1], [2])\n    let w = measure(tbl).width\n    grid(columns: (w,), tbl, align(left)[_Note._])\n  }\n]";
        assert_eq!(md(typst), "::: {style=\"text-align: center\"}\n\n| **A** | **B** |\n|---|---|\n| 1 | 2 |\n\n*Note.*\n\n:::\n");
        assert_eq!(md("#{ let x = 1; x + 2 }#{ for i in range(3) [#i] }#{ if true [yes] }#context here().page()"), "");
        assert_eq!(md("#figure(align(center)[#table(columns: 1, [h], [v])], caption: [Q], kind: \"quarto-float-tbl\", supplement: \"Table\") <tbl-q>"), "| h |\n|---|\n| v |\n\n: Q {#tbl-q}\n");
    }

    #[test]
    fn spanning_cells_are_placed_as_typst_places_them() {
        let typst = "#table(columns: 3, table.header([C], [V], [N]), table.cell(rowspan: 2)[Cars], [1], [10], [2], [20], [Bikes], table.cell(colspan: 2)[none])";
        assert_eq!(md(typst), "| C | V | N |\n|---|---|---|\n| Cars | 1 | 10 |\n|  | 2 | 20 |\n| Bikes | none |  |\n");
    }

    #[test]
    fn an_unknown_function_gives_its_content() {
        assert_eq!(md("#mybox(fill: red)[Hello] and #note(title: [A])[B] and #icon(\"github\")."), "Hello and A B and .\n");
    }

    #[test]
    fn a_first_heading_abstract_is_the_abstract() {
        assert_eq!(md("= Abstract\nWe do X.\n= Intro\nBody."), "::: {.abstract}\n\n**Abstract.** We do X.\n\n:::\n\n# Intro\n\nBody.\n");
    }

    #[test]
    fn hostile_input_ends_quickly() {
        let deep = format!("{}x{}", "#box[".repeat(300), "]".repeat(300));
        assert!(md(&deep).len() < 10);
        let chain = format!("#align({}center)[c]", "top + ".repeat(20_000));
        assert!(md(&chain).contains('c'));
        assert!(md("#lorem(1000000000)").split(' ').count() <= 10 * MOST);
        assert!(md("#table(columns: 100000000, table.cell(colspan: 999999999)[x])").len() < 20 * MOST);
        let doubling = format!("#let x = [ab]\n{}#x", "#let x = [#x#x]\n".repeat(40));
        assert!(md(&doubling).len() <= 4 * EXPANSIONS);
        assert_eq!(md("+ a\n18446744073709551615. b\n+ c"), "1. a\n18446744073709551615. b\n18446744073709551615. c\n");
    }

    #[test]
    fn labels_become_ids_by_what_they_are_on() {
        assert_eq!(label_id("fig", "fig-a"), "fig-a");
        assert_eq!(label_id("fig", "fig:results"), "fig-results");
        assert_eq!(label_id("tbl", "results"), "tbl-results");
        assert_eq!(label_id("tbl", "tab:x.y"), "tbl-x-y");
        assert_eq!(label_id("sec", "intro"), "sec-intro");
        assert_eq!(label_id("eq", "eq:e=mc²"), "eq-e-mc-");
    }

    #[test]
    fn yaml_values_are_quoted_when_they_must_be() {
        assert_eq!(yaml("Plain words"), "Plain words");
        assert_eq!(yaml("A: B"), "\"A: B\"");
        assert_eq!(yaml("#1"), "\"#1\"");
        assert_eq!(yaml("- dash"), "\"- dash\"");
        assert_eq!(yaml("say \"x\": \\"), "\"say \\\"x\\\": \\\\\"");
    }

    #[test]
    fn includes_are_pasted_in_with_their_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("ch")).unwrap();
        let main = dir.path().join("main.typ");
        std::fs::write(&main, "= Main\n#include \"ch/one.typ\"\n#include \"missing.typ\"\nEnd @one.").unwrap();
        // The cycle back to the main file is not followed.
        std::fs::write(dir.path().join("ch/one.typ"), "= One <one>\n#image(\"pic.png\")\n#include \"../main.typ\"\n#include \"two.typ\"").unwrap();
        std::fs::write(dir.path().join("ch/two.typ"), "Two.").unwrap();
        let text = std::fs::read_to_string(&main).unwrap();
        assert_eq!(to_markdown(&text, &main), "# Main\n\n# One {#sec-one}\n\n![](ch/pic.png)\n\nTwo.\n\nEnd @sec-one.\n");
    }

    #[test]
    fn includes_nest_no_deeper_than_eight() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..12 {
            std::fs::write(dir.path().join(format!("f{i}.typ")), format!("F{i}\n\n#include \"f{}.typ\"", i + 1)).unwrap();
        }
        let main = dir.path().join("f0.typ");
        let out = to_markdown(&std::fs::read_to_string(&main).unwrap(), &main);
        assert!(out.contains("F8\n"));
        assert!(!out.contains("F9"));
    }

    #[test]
    fn the_output_is_what_quarto_expand_reads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("refs.bib"), "@article{smith,\n  author = {Smith, John},\n  title = {On Things},\n  journal = {J. Things},\n  year = {2020},\n}\n").unwrap();
        let file = dir.path().join("doc.typ");
        let typst = "#set document(title: \"Paper\")\n#set math.equation(numbering: \"(1)\")\n= Intro <intro>\nAs @smith showed, see @fig-a, @tab and @energy.\n#figure(image(\"a.png\", width: 50%), caption: [A picture.]) <fig-a>\n#figure(table(columns: 2, [x], [y], [1], [2]), caption: [Numbers]) <tab>\n$ E = m c^2 $ <energy>\n#bibliography(\"refs.bib\")";
        let out = crate::quarto::expand(&to_markdown(typst, &file), &file);
        assert!(out.contains("see Figure 1, Table 1 and Equation 1."), "{out}");
        assert!(out.contains("![Figure 1: A picture.](a.png)"), "{out}");
        assert!(out.contains("*Table 1: Numbers*"), "{out}");
        assert!(out.contains("E = m c^2\n\\tag{1}\n$$"), "{out}");
        assert!(out.contains("As (Smith 2020) showed"), "{out}");
        assert!(out.contains("Smith, J. (2020). On Things."), "{out}");
        assert!(!out.contains('?'), "{out}");
    }
}
