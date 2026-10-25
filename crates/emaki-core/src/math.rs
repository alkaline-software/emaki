//! Math typeset for reading: a formula from a document, set by Typst's
//! own compiler and handed back as an SVG with its size and baseline, for
//! the file's pane to draw in the line or on a line of its own.
//!
//! - LaTeX math (markdown's `$…$`, a `.tex` file's) is written as Typst's
//!   by MiTeX (`mitex::convert_math`) and evaluated in MiTeX's own scope,
//!   the Typst files of its package kept in `mitex/` (Apache-2.0). Its
//!   long arrows came from another package, `xarrow`; Typst's `stretch`
//!   stands in for it there. A `.typ` file's math is Typst's already.
//! - The face is New Computer Modern Math ("NewComputerModern Math" by
//!   its name table), the one face Typst is given:
//!   its upright Latin is the words of `\text{…}` too. The window hands
//!   it over (`use_font`); the font is the window's, with its license, in
//!   `assets/fonts`.
//! - A formula is set at `SIZE` and measured in ems of it, so the view
//!   scales it to the words around it. The page is cut to the formula and
//!   the text's edges are the glyphs' bounds: with Typst's own edges an
//!   inline fraction's lower half stood below the page.
//! - Each formula is set once (`formula` keeps every answer, a failure
//!   too). The view asks while it parses, off the window's thread.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::layout::{Frame, FrameItem, Point};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

/// The size a formula is set at, in points; what is handed back is in
/// ems of it.
const SIZE: f64 = 16.;

/// Whose math a formula is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// LaTeX's, as markdown and `.tex` write it.
    Tex,
    /// Typst's, as a `.typ` file writes it.
    Typst,
}

/// A formula set: the picture, and its size and baseline (from the top)
/// in ems of the text it stands in.
#[derive(Clone, PartialEq)]
pub struct Formula {
    pub svg: Arc<[u8]>,
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

impl std::fmt::Debug for Formula {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Formula {{ {}×{} em, baseline {} }}", self.width, self.height, self.baseline)
    }
}

static FONT: OnceLock<&'static [u8]> = OnceLock::new();

/// The math face, from the window, before any formula is set. Without it
/// nothing is set, and the view draws math as words.
pub fn use_font(bytes: &'static [u8]) {
    let _ = FONT.set(bytes);
}

/// MiTeX's scope, at the paths its files import one another by.
const SCOPE: &[(&str, &str)] = &[
    ("/mitex/mod.typ", include_str!("mitex/mod.typ")),
    ("/mitex/prelude.typ", include_str!("mitex/prelude.typ")),
    ("/mitex/latex/standard.typ", include_str!("mitex/latex/standard.typ")),
];

struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    scope: HashMap<FileId, Source>,
}

static SHARED: LazyLock<Option<Shared>> = LazyLock::new(|| {
    let bytes = FONT.get()?;
    let fonts: Vec<Font> = Font::iter(Bytes::new(*bytes)).collect();
    if fonts.is_empty() {
        return None;
    }
    let scope = SCOPE
        .iter()
        .filter_map(|(path, text)| {
            let id = RootedPath::new(VirtualRoot::Project, VirtualPath::new(path).ok()?).intern();
            Some((id, Source::new(id, text.to_string())))
        })
        .collect();
    Some(Shared { library: LazyHash::new(Library::builder().build()), book: LazyHash::new(FontBook::from_fonts(&fonts)), fonts, scope })
});

/// One formula's world: the shared library, face and scope, and the page
/// that holds the formula.
struct Page<'a> {
    shared: &'a Shared,
    main: Source,
}

impl World for Page<'_> {
    fn library(&self) -> &LazyHash<Library> {
        &self.shared.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.shared.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        self.shared.scope.get(&id).cloned().ok_or_else(|| FileError::NotFound(id.vpath().get_without_slash().into()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.source(id).map(|s| Bytes::from_string(s.text().to_string()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.shared.fonts.get(index).cloned()
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// `src` as a string of Typst code.
fn quoted(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 2);
    out.push('"');
    for c in src.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The height of the box set in a formula to find its baseline by: a box
/// in math stands on the baseline, so its foot is the baseline wherever
/// the frames of the formula are put (a lone fraction has no line of its
/// own to read one off).
const MARK: f64 = 0.0137;

/// The Typst document that sets `src` alone on a page cut to it, with the
/// mark at its start.
fn document(src: &str, dialect: Dialect, display: bool) -> Result<String, String> {
    let page = format!("#set page(width: auto, height: auto, margin: 0pt, fill: none)\n#set text(size: {SIZE}pt, font: \"NewComputerModern Math\", top-edge: \"bounds\", bottom-edge: \"bounds\")\n#let mark = box(width: 0pt, height: {MARK}pt)\n");
    Ok(match dialect {
        Dialect::Tex => {
            // MiTeX writes an escaped sign with a space after it, which in
            // `\text{…}` is a space; in math a space is nothing.
            static ESCAPED: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(\\[_&%#$]) ").unwrap());
            let typ = mitex::convert_math(src, None)?;
            let typ = ESCAPED.replace_all(&typ, "$1");
            format!("#import \"/mitex/mod.typ\": mitex-scope\n{page}#math.equation(block: {display}, mark + eval({}, mode: \"math\", scope: mitex-scope))\n", quoted(&typ))
        }
        Dialect::Typst => {
            let body = src.trim();
            // The mark ends at `;`: a space after it would be set as one,
            // and a `(` straight after it would call it.
            if display { format!("{page}$ #mark;{body} $\n") } else { format!("{page}$#mark;{body}$\n") }
        }
    })
}

/// Where the formula's baseline is on its page: the foot of the mark.
fn baseline(frame: &Frame, at: Point) -> Option<f64> {
    for (pos, item) in frame.items() {
        if let FrameItem::Group(g) = item {
            let here = at + *pos;
            let size = g.frame.size();
            if size.x.to_pt().abs() < 1e-9 && (size.y.to_pt() - MARK).abs() < 1e-6 {
                return Some((here.y + size.y).to_pt());
            }
            if let Some(b) = baseline(&g.frame, here) {
                return Some(b);
            }
        }
    }
    None
}

/// `src` set by Typst, or why it could not be.
pub fn typeset(src: &str, dialect: Dialect, display: bool) -> Result<Formula, String> {
    let shared = (*SHARED).as_ref().ok_or("no math face")?;
    let main = Source::new(RootedPath::new(VirtualRoot::Project, VirtualPath::new("/formula.typ").map_err(|e| format!("{e:?}"))?).intern(), document(src, dialect, display)?);
    let world = Page { shared, main };
    let doc = typst::compile::<PagedDocument>(&world).output.map_err(|errors| errors.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))?;
    let page = doc.pages().first().ok_or("nothing set")?;
    let size = page.frame.size();
    if size.x.to_pt() <= 0. || size.y.to_pt() <= 0. {
        return Err("nothing set".into());
    }
    let base = baseline(&page.frame, Point::zero()).unwrap_or(size.y.to_pt());
    let svg = typst_svg::svg(page, &Default::default());
    Ok(Formula { svg: Arc::from(svg.into_bytes().into_boxed_slice()), width: (size.x.to_pt() / SIZE) as f32, height: (size.y.to_pt() / SIZE) as f32, baseline: (base / SIZE) as f32 })
}

type Key = (Dialect, bool, String);

static SET: LazyLock<Mutex<HashMap<Key, Option<Arc<Formula>>>>> = LazyLock::new(Default::default);

/// `src` set, from what was set before when it was; None when Typst
/// cannot set it, and the view draws it as words.
pub fn formula(src: &str, dialect: Dialect, display: bool) -> Option<Arc<Formula>> {
    let key = (dialect, display, src.to_string());
    if let Some(done) = SET.lock().unwrap().get(&key) {
        return done.clone();
    }
    // Typst lays a formula out a level a nesting, deep enough for a
    // thread's own stack on Windows (`on_deep_stack`).
    let done = crate::on_deep_stack(|| typeset(src, dialect, display)).ok().map(Arc::new);
    SET.lock().unwrap().insert(key, done.clone());
    done
}

/// The widths, in ems, of the math face's no-break space (U+00A0) and
/// narrow no-break space (U+202F): what the room a formula takes in a
/// line of words is made of. A face without them is taken at the widths
/// New Computer Modern Math has.
pub fn glue() -> (f32, f32) {
    let face = FONT.get().and_then(|bytes| ttf_parser::Face::parse(bytes, 0).ok());
    let width = |c: char| -> Option<f32> {
        let face = face.as_ref()?;
        Some(face.glyph_hor_advance(face.glyph_index(c)?)? as f32 / face.units_per_em() as f32)
    };
    (width('\u{A0}').unwrap_or(0.332), width('\u{202F}').unwrap_or(0.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face() {
        static BYTES: LazyLock<&'static [u8]> = LazyLock::new(|| Box::leak(std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../emaki-app/assets/fonts/NewCMMath-Regular.otf")).unwrap().into_boxed_slice()));
        use_font(*BYTES);
    }

    /// A formula nested as deep as one can be written ends, set or not,
    /// on a thread's stack the size Windows gives (`RUST_MIN_STACK` of a
    /// megabyte for the test's own thread shows it here).
    #[test]
    fn a_deep_formula_does_not_take_the_stack() {
        face();
        let deep = format!("{}x{}", r"\frac{1}{".repeat(200), "}".repeat(200));
        let _ = formula(&deep, Dialect::Tex, true);
        let deep = format!("{}x{}", "(".repeat(400), ")".repeat(400));
        let _ = formula(&deep, Dialect::Typst, false);
    }

    #[test]
    fn latex_and_typst_math_are_set_with_a_baseline_inside() {
        face();
        for (src, dialect, display) in [(r"\frac{a}{b} + \sqrt{x_i^2}", Dialect::Tex, false), (r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", Dialect::Tex, true), (r"f(x) = \begin{cases} 1 & \text{if } x > 0 \\ 0 & \text{otherwise} \end{cases}", Dialect::Tex, true), (r"x \xrightarrow{f} y", Dialect::Tex, false), ("sum_(i=1)^n x_i", Dialect::Typst, true)] {
            let f = typeset(src, dialect, display).unwrap_or_else(|e| panic!("{src}: {e}"));
            assert!(f.width > 0. && f.height > 0., "{src}");
            assert!(f.baseline > 0. && f.baseline < f.height, "{src}: the baseline {} is inside the height {}", f.baseline, f.height);
            assert!(std::str::from_utf8(&f.svg).unwrap().starts_with("<svg"));
        }
        // The room a formula takes is made of the face's two spaces.
        let (wide, narrow) = glue();
        assert!((wide - 0.332).abs() < 0.001 && (narrow - 0.2).abs() < 0.001, "{wide} {narrow}");
        // A fraction stands below the line as well as above it.
        let frac = typeset(r"\frac{a}{b}", Dialect::Tex, false).unwrap();
        assert!(frac.height - frac.baseline > 0.2, "{frac:?}");
    }

    #[test]
    fn a_letter_is_as_wide_in_either_dialect() {
        face();
        // The mark that finds the baseline adds no room of its own, and a
        // formula opening with a parenthesis is not taken as a call on it.
        let tex = typeset(r"\beta", Dialect::Tex, false).unwrap();
        let typ = typeset("#sym.beta", Dialect::Typst, false).unwrap();
        assert!((tex.width - typ.width).abs() < 0.01, "{tex:?} {typ:?}");
        assert!(typeset("(a+b)/2", Dialect::Typst, false).is_ok());
    }

    #[test]
    fn an_escaped_sign_in_words_has_no_space_after_it() {
        face();
        // MiTeX writes `\_` with a space after it, which in words is one.
        let joined = typeset(r"x^{\text{min\_threshold}}", Dialect::Tex, false).unwrap();
        let spaced = typeset(r"x^{\text{min\_ threshold}}", Dialect::Tex, false).unwrap();
        assert!(joined.width < spaced.width, "{joined:?} {spaced:?}");
    }

    #[test]
    fn what_cannot_be_set_says_why() {
        face();
        assert!(typeset("#panic()", Dialect::Typst, false).is_err());
        assert!(formula("x + undefined_function(y)", Dialect::Typst, false).is_none());
    }
}
