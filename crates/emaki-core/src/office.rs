//! Microsoft Office files, read only, for the file's pane. A Word document
//! and a PowerPoint deck come out as markdown, which the window already
//! draws; a workbook as its sheets' rows, which it draws as a table.
//!
//! The three modern formats are zips of XML. A workbook is read by calamine,
//! which also takes the old binary `.xls`. A document and a deck are read
//! here: only their text and its structure, no pictures, no layout. Nothing
//! is written, and no other program is asked: this builds the same on a Mac,
//! Windows and Linux.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use calamine::{Data, Reader as _, SheetType, SheetVisible, Xls, XlsError, Xlsx, XlsxError};
use quick_xml::events::Event;

/// What an Office file holds, in a form the window already draws.
#[derive(Debug, Clone, PartialEq)]
pub enum Office {
    /// A document or a deck as markdown: headings, paragraphs, lists, tables; a deck as "## Slide n" sections.
    Markdown(String),
    /// A workbook: each sheet's name and its rows of cells as text (at most `most_rows` rows per sheet), and whether rows were left out.
    Sheets(Vec<Sheet>),
}

/// One sheet of a workbook. Every row has the same number of cells, and a
/// cell is where the sheet has it: rows and columns left empty before the
/// first value are kept, those after the last are not.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    /// The sheet has rows past the ones here.
    pub more: bool,
}

/// The most of one part of the zip that is read. A document's XML is a few
/// megabytes at the outside; a zip made to unpack into gigabytes stops here.
const MAX_PART: u64 = 256 * 1024 * 1024;
/// How an old binary Office file, and an encrypted new one, begins.
const CFB: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Whether a file of this extension (lowercase, no dot) is one `read` takes.
/// The binary `.doc` and `.ppt` are not: there is no small reader for them,
/// and the window offers the system's app for what this refuses.
pub fn reads(ext: &str) -> bool {
    matches!(ext, "docx" | "pptx" | "xlsx" | "xlsm" | "xls")
}

/// Read the file. Err is a short plain reason in lowercase.
pub fn read(path: &Path, most_rows: usize) -> Result<Office, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !reads(&ext) {
        return Err("it is not an Office file this reads".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "it could not be read".to_string())?;
    // A file with a password is not a zip: Office wraps the encrypted zip
    // in the old binary container, under this stream's name.
    if bytes.starts_with(CFB) && holds_utf16(&bytes, "EncryptedPackage") {
        return Err("it is protected by a password".into());
    }
    match ext.as_str() {
        "docx" => {
            let mut zip = open_zip(bytes).ok_or("it is not a Word file")?;
            word(&mut zip).map(Office::Markdown).ok_or_else(|| "it is not a Word file".to_string())
        }
        "pptx" => {
            let mut zip = open_zip(bytes).ok_or("it is not a PowerPoint file")?;
            deck(&mut zip).map(Office::Markdown).ok_or_else(|| "it is not a PowerPoint file".to_string())
        }
        _ => book(bytes, most_rows).map(Office::Sheets),
    }
}

fn holds_utf16(bytes: &[u8], word: &str) -> bool {
    let needle: Vec<u8> = word.encode_utf16().flat_map(u16::to_le_bytes).collect();
    bytes.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------- the zip

type Zip = zip::ZipArchive<Cursor<Vec<u8>>>;

fn open_zip(bytes: Vec<u8>) -> Option<Zip> {
    zip::ZipArchive::new(Cursor::new(bytes)).ok()
}

/// One part of the package as text, or None when it is not there.
fn part(zip: &mut Zip, name: &str) -> Option<String> {
    let file = zip.by_name(name).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_PART).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(text.trim_start_matches('\u{feff}').to_string())
}

/// The package's main part, as its own list of relationships names it:
/// `word/document.xml` and `ppt/presentation.xml` in every file Office
/// writes, but the name is the package's to choose.
fn main_part(zip: &mut Zip, usual: &str) -> String {
    let mut found = None;
    if let Some(rels) = part(zip, "_rels/.rels") {
        walk(&rels, |node| {
            if let Node::Open("Relationship", attrs) = node {
                if attrs.get("Type").is_some_and(|t| t.ends_with("/officeDocument")) && found.is_none() {
                    found = attrs.get("Target").map(|t| t.trim_start_matches('/').to_string());
                }
            }
        });
    }
    found.filter(|name| zip.by_name(name).is_ok()).unwrap_or_else(|| usual.to_string())
}

/// The folder a part is in, with its slash: "word/" of "word/document.xml".
fn folder_of(name: &str) -> &str {
    &name[..name.rfind('/').map(|i| i + 1).unwrap_or(0)]
}

/// A relationship's target as a name in the zip: from the package's top
/// when it starts with a slash, else from the folder of the part that
/// holds the relationship, with any ".." taken out.
fn resolve(folder: &str, target: &str) -> String {
    let joined = match target.strip_prefix('/') {
        Some(top) => top.to_string(),
        None => format!("{folder}{target}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for piece in joined.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

// ---------------------------------------------------------------- the XML

/// An element's attributes, by the names the file writes them under.
struct Attrs(Vec<(String, String)>);

impl Attrs {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// One step through a part: an element opening, one closing, or text. An
/// element with nothing in it opens and closes.
enum Node<'a> {
    Open(&'a str, &'a Attrs),
    Close(&'a str),
    Text(&'a str),
}

/// Walk a part in order. Names are as written, prefix and all ("w:p"):
/// the prefixes are not fixed by the format, but every writer uses the
/// same ones. A part that breaks off gives what came before the break.
///
/// What is under `mc:Fallback` is passed over: it is the same content
/// again in an older form, for a reader that lacks the newer one, and
/// reading both shows a text box twice.
fn walk(xml: &str, mut f: impl FnMut(Node)) {
    let mut reader = quick_xml::Reader::from_str(xml);
    let decoder = reader.decoder();
    let attrs_of = |e: &quick_xml::events::BytesStart| {
        Attrs(e.attributes().with_checks(false).flatten().map(|a| (String::from_utf8_lossy(a.key.as_ref()).into_owned(), a.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, decoder).map(|v| v.into_owned()).unwrap_or_default())).collect())
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "mc:Fallback" {
                    if reader.read_to_end(e.name()).is_err() {
                        break;
                    }
                    continue;
                }
                f(Node::Open(&name, &attrs_of(&e)));
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                f(Node::Open(&name, &attrs_of(&e)));
                f(Node::Close(&name));
            }
            Ok(Event::End(e)) => f(Node::Close(&String::from_utf8_lossy(e.name().as_ref()))),
            Ok(Event::Text(t)) => {
                if let Ok(text) = t.decode() {
                    f(Node::Text(&text));
                }
            }
            Ok(Event::CData(t)) => {
                if let Ok(text) = t.decode() {
                    f(Node::Text(&text));
                }
            }
            // "&amp;" and "&#233;" arrive on their own, between texts.
            Ok(Event::GeneralRef(r)) => {
                let ch = match r.resolve_char_ref() {
                    Ok(Some(ch)) => Some(ch),
                    _ => match &*r {
                        b"amp" => Some('&'),
                        b"lt" => Some('<'),
                        b"gt" => Some('>'),
                        b"quot" => Some('"'),
                        b"apos" => Some('\''),
                        _ => None,
                    },
                };
                if let Some(ch) = ch {
                    f(Node::Text(ch.encode_utf8(&mut [0; 4])));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

/// Whether an on/off property is on: `<w:b/>` is, `<w:b w:val="0"/>` is not.
fn is_on(attrs: &Attrs) -> bool {
    !matches!(attrs.get("w:val"), Some("0" | "false" | "off"))
}

// ------------------------------------------------------------ the markdown

/// One block of the markdown. Items of a list follow one another on the
/// next line; anything else has an empty line before it.
struct Block {
    text: String,
    item: bool,
}

fn join(blocks: &[Block]) -> String {
    let mut out = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str(if block.item && blocks[i - 1].item { "\n" } else { "\n\n" });
        }
        out.push_str(&block.text);
    }
    out
}

/// A list's items as they are numbered and set in: what both readers keep
/// between one paragraph and the next.
#[derive(Default)]
struct Lists {
    /// The level of the item before, None after anything that is no item.
    last: Option<usize>,
    /// Where each numbered list has got to, a count a level, by the list's id.
    counts: HashMap<String, Vec<usize>>,
}

impl Lists {
    /// An item's line. `ordered` carries the id of the list it counts in.
    /// An item is never set in more than one step past the one before:
    /// markdown reads a first item four spaces in as code.
    fn item(&mut self, level: usize, ordered: Option<&str>, text: &str) -> Block {
        let level = level.min(self.last.map(|l| l + 1).unwrap_or(0));
        self.last = Some(level);
        let text = text.replace('\n', " ");
        Block { text: format!("{}{}{}", "    ".repeat(level), self.mark(level, ordered), text), item: true }
    }

    fn mark(&mut self, level: usize, ordered: Option<&str>) -> String {
        let Some(id) = ordered else { return "- ".into() };
        let counts = self.counts.entry(id.to_string()).or_default();
        counts.resize(level + 1, 0);
        counts[level] += 1;
        format!("{}. ", counts[level])
    }

    fn end(&mut self) {
        self.last = None;
    }
}

/// Rows as a GitHub table, the first row its head. None when no cell holds
/// anything.
fn table(rows: &[Vec<String>]) -> Option<String> {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if rows.iter().flatten().all(|c| c.trim().is_empty()) {
        return None;
    }
    let line = |row: &[String]| {
        let mut out = String::from("|");
        for i in 0..width {
            let cell = row.get(i).map(|c| c.trim().replace('|', "\\|").replace('\n', " ")).unwrap_or_default();
            out.push_str(&format!(" {cell} |"));
        }
        out
    };
    let mut lines = vec![line(&rows[0]), format!("|{}", " --- |".repeat(width))];
    lines.extend(rows[1..].iter().map(|r| line(r)));
    Some(lines.join("\n"))
}

// ------------------------------------------------------------------- Word

/// A paragraph while it is being read.
#[derive(Default)]
struct Para {
    style: Option<String>,
    /// The list it is an item of, by id, and how deep.
    list: Option<String>,
    level: usize,
    /// The run being read: bold, italic. None between runs, which is how
    /// a property of the paragraph's own mark is told from a run's.
    run: Option<(bool, bool)>,
    /// The text, a piece a run, with whether it is bold and italic.
    spans: Vec<(String, bool, bool)>,
}

impl Para {
    fn push(&mut self, text: &str) {
        let (bold, italic) = self.run.unwrap_or((false, false));
        match self.spans.last_mut() {
            Some((last, b, i)) if *b == bold && *i == italic => last.push_str(text),
            _ => self.spans.push((text.to_string(), bold, italic)),
        }
    }

    /// The text, with `**` and `*` around what is bold and italic when
    /// `marked`. The marks go inside the spaces at a piece's ends, where
    /// markdown takes them, and a piece with a star of its own gets none.
    fn text(&self, marked: bool) -> String {
        let mut out = String::new();
        for (text, bold, italic) in &self.spans {
            let core = text.trim();
            let mark = match (bold, italic) {
                _ if !marked || core.is_empty() || core.contains('*') => "",
                (true, true) => "***",
                (true, false) => "**",
                (false, true) => "*",
                (false, false) => "",
            };
            if mark.is_empty() {
                out.push_str(text);
                continue;
            }
            let lead = text.len() - text.trim_start().len();
            out.push_str(&text[..lead]);
            out.push_str(mark);
            out.push_str(core);
            out.push_str(mark);
            out.push_str(&text[lead + core.len()..]);
        }
        out.trim().to_string()
    }
}

/// A table while it is being read. A table in a cell is a frame over that
/// cell's.
enum Frame {
    Table(Vec<Vec<String>>),
    Row(Vec<String>),
    Cell(Vec<String>),
}

/// The level of heading a style is, by its name: "heading 2" and "Title"
/// are the names Word keeps in every language, whatever the style's id.
fn heading_level(name: &str) -> Option<usize> {
    let name = name.to_ascii_lowercase().replace(' ', "");
    if name == "title" {
        return Some(1);
    }
    let n: usize = name.strip_prefix("heading")?.parse().ok()?;
    (1..=9).contains(&n).then_some(n.min(6))
}

/// Style id to heading level, from the document's styles.
fn heading_styles(styles: &str) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    let mut id: Option<String> = None;
    walk(styles, |node| match node {
        Node::Open("w:style", attrs) => id = attrs.get("w:styleId").map(str::to_string),
        Node::Close("w:style") => id = None,
        Node::Open("w:name", attrs) => {
            if let (Some(id), Some(level)) = (&id, attrs.get("w:val").and_then(heading_level)) {
                out.insert(id.clone(), level);
            }
        }
        _ => {}
    });
    out
}

/// Which lists count: (list id, level) for every level whose format is a
/// number or a letter and not a bullet.
fn ordered_lists(numbering: &str) -> std::collections::HashSet<(String, usize)> {
    let mut formats: HashMap<(String, usize), bool> = HashMap::new();
    let mut of_list: Vec<(String, String)> = Vec::new();
    let (mut abs, mut level, mut num): (Option<String>, usize, Option<String>) = (None, 0, None);
    walk(numbering, |node| match node {
        Node::Open("w:abstractNum", attrs) => abs = attrs.get("w:abstractNumId").map(str::to_string),
        Node::Close("w:abstractNum") => abs = None,
        Node::Open("w:lvl", attrs) => level = attrs.get("w:ilvl").and_then(|v| v.parse().ok()).unwrap_or(0),
        Node::Open("w:numFmt", attrs) => {
            if let Some(abs) = &abs {
                formats.insert((abs.clone(), level), !matches!(attrs.get("w:val"), Some("bullet" | "none") | None));
            }
        }
        Node::Open("w:num", attrs) => num = attrs.get("w:numId").map(str::to_string),
        Node::Close("w:num") => num = None,
        Node::Open("w:abstractNumId", attrs) => {
            if let (Some(num), Some(abs)) = (&num, attrs.get("w:val")) {
                of_list.push((num.clone(), abs.to_string()));
            }
        }
        _ => {}
    });
    let mut out = std::collections::HashSet::new();
    for (num, abs) in of_list {
        for ((a, level), ordered) in &formats {
            if *a == abs && *ordered {
                out.insert((num.clone(), *level));
            }
        }
    }
    out
}

fn word(zip: &mut Zip) -> Option<String> {
    let main = main_part(zip, "word/document.xml");
    let xml = part(zip, &main)?;
    let folder = folder_of(&main).to_string();
    let headings = part(zip, &format!("{folder}styles.xml")).map(|s| heading_styles(&s)).unwrap_or_default();
    let ordered = part(zip, &format!("{folder}numbering.xml")).map(|s| ordered_lists(&s)).unwrap_or_default();

    let mut blocks: Vec<Block> = Vec::new();
    // A text box is paragraphs inside a paragraph, hence a stack.
    let mut paras: Vec<Para> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut lists = Lists::default();
    let (mut in_text, mut in_run_props, mut in_list_props, mut is_document) = (false, false, false, false);

    walk(&xml, |node| match node {
        Node::Open("w:document", _) => is_document = true,
        Node::Open("w:p", _) => paras.push(Para::default()),
        Node::Close("w:p") => {
            let Some(para) = paras.pop() else { return };
            let in_cell = matches!(frames.last(), Some(Frame::Cell(_)));
            let level = para.style.as_ref().and_then(|s| headings.get(s).copied().or_else(|| heading_level(s)));
            let text = para.text(level.is_none());
            if text.is_empty() {
                return;
            }
            let block = match (level, para.list.as_deref().filter(|id| *id != "0")) {
                (Some(level), _) => Block { text: format!("{} {}", "#".repeat(level), para.text(false).replace('\n', " ")), item: false },
                (None, Some(id)) => {
                    if in_cell {
                        lists.end();
                    }
                    let counts = ordered.contains(&(id.to_string(), para.level));
                    lists.item(para.level, counts.then_some(id), &text)
                }
                // Two spaces before a line break keep it one in markdown.
                (None, None) => Block { text: text.replace('\n', "  \n"), item: false },
            };
            if let Some(Frame::Cell(cell)) = frames.last_mut() {
                cell.push(block.text.trim().to_string());
                return;
            }
            if !block.item {
                lists.end();
            }
            blocks.push(block);
        }
        Node::Open("w:pStyle", attrs) => {
            if let Some(para) = paras.last_mut() {
                para.style = attrs.get("w:val").map(str::to_string);
            }
        }
        Node::Open("w:numPr", _) => in_list_props = true,
        Node::Close("w:numPr") => in_list_props = false,
        Node::Open("w:numId", attrs) if in_list_props => {
            if let Some(para) = paras.last_mut() {
                para.list = attrs.get("w:val").map(str::to_string);
            }
        }
        Node::Open("w:ilvl", attrs) if in_list_props => {
            if let Some(para) = paras.last_mut() {
                para.level = attrs.get("w:val").and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }
        Node::Open("w:r", _) => {
            if let Some(para) = paras.last_mut() {
                para.run = Some((false, false));
            }
        }
        Node::Close("w:r") => {
            if let Some(para) = paras.last_mut() {
                para.run = None;
            }
        }
        Node::Open("w:rPr", _) => in_run_props = true,
        Node::Close("w:rPr") => in_run_props = false,
        Node::Open(name @ ("w:b" | "w:i"), attrs) if in_run_props => {
            if let Some(run) = paras.last_mut().and_then(|p| p.run.as_mut()) {
                match name {
                    "w:b" => run.0 = is_on(attrs),
                    _ => run.1 = is_on(attrs),
                }
            }
        }
        Node::Open("w:t", _) => in_text = true,
        Node::Close("w:t") => in_text = false,
        Node::Text(text) if in_text => {
            if let Some(para) = paras.last_mut() {
                para.push(text);
            }
        }
        // Inside a run only: `w:tab` is also how a paragraph lists its
        // tab stops.
        Node::Open(name @ ("w:tab" | "w:br" | "w:cr" | "w:noBreakHyphen"), attrs) => {
            if let Some(para) = paras.last_mut().filter(|p| p.run.is_some()) {
                match name {
                    "w:tab" => para.push("\t"),
                    "w:noBreakHyphen" => para.push("-"),
                    // A break to the next page or column is layout.
                    _ if matches!(attrs.get("w:type"), Some("page" | "column")) => {}
                    _ => para.push("\n"),
                }
            }
        }
        Node::Open("w:tbl", _) => frames.push(Frame::Table(Vec::new())),
        Node::Open("w:tr", _) => frames.push(Frame::Row(Vec::new())),
        Node::Open("w:tc", _) => frames.push(Frame::Cell(Vec::new())),
        Node::Close("w:tc") => {
            if let Some(Frame::Cell(_)) = frames.last() {
                let Some(Frame::Cell(cell)) = frames.pop() else { return };
                if let Some(Frame::Row(row)) = frames.last_mut() {
                    row.push(cell.join(" "));
                }
            }
        }
        Node::Close("w:tr") => {
            if let Some(Frame::Row(_)) = frames.last() {
                let Some(Frame::Row(row)) = frames.pop() else { return };
                if let Some(Frame::Table(rows)) = frames.last_mut() {
                    rows.push(row);
                }
            }
        }
        Node::Close("w:tbl") => {
            if let Some(Frame::Table(_)) = frames.last() {
                let Some(Frame::Table(rows)) = frames.pop() else { return };
                // Markdown has no table in a cell: the inner one's words
                // go into the cell it is in.
                if let Some(Frame::Cell(cell)) = frames.last_mut() {
                    cell.extend(rows.into_iter().flatten().filter(|c| !c.is_empty()));
                } else if let Some(text) = table(&rows) {
                    lists.end();
                    blocks.push(Block { text, item: false });
                }
            }
        }
        _ => {}
    });
    is_document.then(|| join(&blocks))
}

// ------------------------------------------------------------- PowerPoint

/// The deck's slides as names in the zip, in the order they are shown:
/// the presentation lists them by relationship id. A deck whose list
/// cannot be read gives its slide files by their numbers.
fn slide_parts(zip: &mut Zip) -> Vec<String> {
    let main = main_part(zip, "ppt/presentation.xml");
    let folder = folder_of(&main).to_string();
    let rels_name = format!("{folder}_rels/{}.rels", &main[folder.len()..]);
    let mut targets: HashMap<String, String> = HashMap::new();
    if let Some(rels) = part(zip, &rels_name) {
        walk(&rels, |node| {
            if let Node::Open("Relationship", attrs) = node {
                if let (Some(id), Some(target)) = (attrs.get("Id"), attrs.get("Target")) {
                    targets.insert(id.to_string(), resolve(&folder, target));
                }
            }
        });
    }
    let mut out = Vec::new();
    if let Some(xml) = part(zip, &main) {
        walk(&xml, |node| {
            if let Node::Open("p:sldId", attrs) = node {
                if let Some(name) = attrs.get("r:id").and_then(|id| targets.get(id)) {
                    out.push(name.clone());
                }
            }
        });
    }
    out.retain(|name| zip.by_name(name).is_ok());
    if out.is_empty() {
        let mut numbered: Vec<(usize, String)> = zip.file_names().filter_map(|name| Some((name.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok()?, name.to_string()))).collect();
        numbered.sort();
        out = numbered.into_iter().map(|(_, name)| name).collect();
    }
    out
}

/// A paragraph of a slide while it is being read.
#[derive(Default)]
struct SlidePara {
    level: usize,
    /// What the paragraph itself says of its bullet: Some(None) for none,
    /// Some(Some(numbered)) for one. None leaves it to the shape.
    bullet: Option<Option<bool>>,
    text: String,
}

/// One slide's blocks, and its title when a title placeholder has one.
fn slide(xml: &str) -> (Option<String>, Vec<Block>) {
    let mut title: Option<String> = None;
    let mut blocks: Vec<Block> = Vec::new();
    let mut lists = Lists::default();
    // The shape being read: its placeholder's type when it is one ("" for
    // a placeholder that names none, which is the slide's content) and
    // the paragraphs so far. Its number among the slide's shapes is the
    // list its numbered paragraphs count in.
    let mut shape: Option<(Option<String>, Vec<SlidePara>)> = None;
    let mut shapes = 0usize;
    let mut para: Option<SlidePara> = None;
    let (mut rows, mut row, mut cell): (Option<Vec<Vec<String>>>, Vec<String>, Option<Vec<String>>) = (None, Vec::new(), None);
    let mut in_text = false;

    let mut flush = |shape: (Option<String>, Vec<SlidePara>), n: usize, title: &mut Option<String>, blocks: &mut Vec<Block>| {
        let (kind, paras) = shape;
        let paras: Vec<SlidePara> = paras.into_iter().filter(|p| !p.text.trim().is_empty()).collect();
        let bulleted = match kind.as_deref() {
            Some("title" | "ctrTitle") => {
                let text = paras.iter().map(|p| p.text.trim().replace('\n', " ")).collect::<Vec<_>>().join(" ");
                if title.is_none() && !text.is_empty() {
                    *title = Some(text);
                    return;
                }
                false
            }
            // The date, the footer and the slide's number are furniture.
            Some("dt" | "ftr" | "sldNum" | "hdr") => return,
            Some("subTitle") | None => false,
            // A content placeholder's paragraphs are bullets by the
            // layout, which is not read; this is what a deck mostly is.
            Some(_) => true,
        };
        lists.end();
        for p in paras {
            let text = p.text.trim();
            match p.bullet.unwrap_or(bulleted.then_some(false)) {
                Some(numbered) => blocks.push(lists.item(p.level, numbered.then(|| n.to_string()).as_deref(), text)),
                None => {
                    lists.end();
                    blocks.push(Block { text: text.replace('\n', "  \n"), item: false });
                }
            }
        }
    };

    walk(xml, |node| match node {
        Node::Open("p:sp", _) => {
            shapes += 1;
            shape = Some((None, Vec::new()));
        }
        Node::Close("p:sp") => {
            if let Some(done) = shape.take() {
                flush(done, shapes, &mut title, &mut blocks);
            }
        }
        Node::Open("p:ph", attrs) => {
            if let Some((kind, _)) = shape.as_mut() {
                *kind = Some(attrs.get("type").unwrap_or("").to_string());
            }
        }
        Node::Open("a:p", _) => para = Some(SlidePara::default()),
        Node::Open("a:pPr", attrs) => {
            if let Some(p) = para.as_mut() {
                p.level = attrs.get("lvl").and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }
        Node::Open(name @ ("a:buNone" | "a:buChar" | "a:buAutoNum" | "a:buBlip"), _) => {
            if let Some(p) = para.as_mut() {
                p.bullet = Some(match name {
                    "a:buNone" => None,
                    "a:buAutoNum" => Some(true),
                    _ => Some(false),
                });
            }
        }
        Node::Open("a:t", _) => in_text = true,
        Node::Close("a:t") => in_text = false,
        Node::Text(text) if in_text => {
            if let Some(p) = para.as_mut() {
                p.text.push_str(text);
            }
        }
        Node::Open("a:br", _) => {
            if let Some(p) = para.as_mut() {
                p.text.push('\n');
            }
        }
        Node::Close("a:p") => {
            let Some(p) = para.take() else { return };
            if let Some(cell) = cell.as_mut() {
                if !p.text.trim().is_empty() {
                    cell.push(p.text.trim().to_string());
                }
            } else if let Some((_, paras)) = shape.as_mut() {
                paras.push(p);
            } else {
                // Text of something that is no shape, a connector's say.
                shapes += 1;
                flush((None, vec![p]), shapes, &mut title, &mut blocks);
            }
        }
        Node::Open("a:tbl", _) => rows = Some(Vec::new()),
        Node::Open("a:tr", _) => row = Vec::new(),
        Node::Open("a:tc", _) => cell = Some(Vec::new()),
        Node::Close("a:tc") => {
            if let Some(cell) = cell.take() {
                row.push(cell.join(" "));
            }
        }
        Node::Close("a:tr") => {
            if let Some(rows) = rows.as_mut() {
                rows.push(std::mem::take(&mut row));
            }
        }
        Node::Close("a:tbl") => {
            if let Some(text) = rows.take().and_then(|rows| table(&rows)) {
                blocks.push(Block { text, item: false });
            }
        }
        _ => {}
    });
    (title, blocks)
}

fn deck(zip: &mut Zip) -> Option<String> {
    let names = slide_parts(zip);
    // A deck with no slides yet, a template say, is still a deck.
    if names.is_empty() {
        let main = main_part(zip, "ppt/presentation.xml");
        zip.by_name(&main).ok()?;
    }
    let mut out: Vec<String> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let n = i + 1;
        let (title, blocks) = part(zip, name).map(|xml| slide(&xml)).unwrap_or_default();
        let head = match title {
            Some(title) => format!("## {n}. {title}"),
            None => format!("## Slide {n}"),
        };
        out.push(if blocks.is_empty() { head } else { format!("{head}\n\n{}", join(&blocks)) });
    }
    Some(out.join("\n\n"))
}

// ------------------------------------------------------------------ Excel

/// A workbook's sheets. The kind is told by how the file begins and not by
/// its name: an `.xls` that is a zip is read as one.
fn book(bytes: Vec<u8>, most_rows: usize) -> Result<Vec<Sheet>, String> {
    let not_excel = || "it is not an Excel file".to_string();
    let locked = || "it is protected by a password".to_string();
    if bytes.starts_with(CFB) {
        let book = Xls::new(Cursor::new(bytes)).map_err(|e| if matches!(e, XlsError::Password) { locked() } else { not_excel() })?;
        Ok(sheets(book, most_rows))
    } else if bytes.starts_with(b"PK") {
        let book = Xlsx::new(Cursor::new(bytes)).map_err(|e| if matches!(e, XlsxError::Password) { locked() } else { not_excel() })?;
        Ok(sheets(book, most_rows))
    } else {
        Err(not_excel())
    }
}

/// The sheets Excel shows: worksheets that are not hidden, or every
/// worksheet when all are. A sheet that cannot be read is left out.
fn sheets<R: calamine::Reader<Cursor<Vec<u8>>>>(mut book: R, most_rows: usize) -> Vec<Sheet> {
    let all: Vec<calamine::Sheet> = book.sheets_metadata().iter().filter(|s| s.typ == SheetType::WorkSheet).cloned().collect();
    let mut shown: Vec<&calamine::Sheet> = all.iter().filter(|s| s.visible == SheetVisible::Visible).collect();
    if shown.is_empty() {
        shown = all.iter().collect();
    }
    let mut out = Vec::new();
    for sheet in shown {
        let Ok(range) = book.worksheet_range(&sheet.name) else { continue };
        // calamine's range begins at the first cell with a value; the
        // rows and columns before it are put back, so a cell is where
        // the sheet has it.
        let (top, left) = range.start().map(|(r, c)| (r as usize, c as usize)).unwrap_or((0, 0));
        let is_empty = |d: &Data| matches!(d, Data::Empty) || matches!(d, Data::String(s) if s.is_empty());
        let filled = range.rows().rposition(|row| !row.iter().all(is_empty)).map(|i| top + i + 1).unwrap_or(0);
        let mut rows: Vec<Vec<String>> = vec![Vec::new(); top.min(filled).min(most_rows)];
        for row in range.rows().take(filled.min(most_rows).saturating_sub(top)) {
            let mut cells = vec![String::new(); left];
            cells.extend(row.iter().map(cell_text));
            rows.push(cells);
        }
        let width = rows.iter().map(|r| r.iter().rposition(|c| !c.is_empty()).map(|i| i + 1).unwrap_or(0)).max().unwrap_or(0);
        for row in &mut rows {
            row.resize(width, String::new());
        }
        out.push(Sheet { name: sheet.name.clone(), rows, more: filled > most_rows });
    }
    out
}

/// A cell as text: the value, not the format the sheet shows it in. A
/// whole number has no ".0", a date is ISO, and an error is as Excel
/// writes it ("#DIV/0!").
fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::Float(f) => format!("{f}"),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::Error(e) => e.to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::DateTime(when) => {
            let days = when.as_f64();
            if when.is_duration() {
                let seconds = (days.abs() * 86_400.0).round() as i64;
                let sign = if days < 0.0 { "-" } else { "" };
                return format!("{sign}{}:{:02}:{:02}", seconds / 3600, seconds % 3600 / 60, seconds % 60);
            }
            let (year, month, day, hour, minute, second, _) = when.to_ymd_hms_milli();
            let date = format!("{year:04}-{month:02}-{day:02}");
            let time = format!("{hour:02}:{minute:02}:{second:02}");
            // A serial under one is a time of day with no date; a whole
            // one is a date with no time.
            if (0.0..1.0).contains(&days) {
                time
            } else if days.fract() == 0.0 {
                date
            } else {
                format!("{date} {time}")
            }
        }
    }
}
