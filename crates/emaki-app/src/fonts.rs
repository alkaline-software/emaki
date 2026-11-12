//! The window's typefaces, every one built in (`assets/fonts`): four
//! serifs and four sans serifs for a conversation, four monospace faces
//! for code and the terminal (three built in, and the Mac's Monaco from
//! the system). Each built in is under the SIL Open Font License,
//! which lets an app carry it, with its license beside its files. Nothing
//! is loaded from another app or fetched: the Claude desktop app's own
//! faces were used when that app was installed, and are not, being
//! Anthropic's and not ours to use.
//!
//! A face is a file a weight (regular, semibold, bold and an italic; the
//! monospace ones a medium too), which the text system takes as bytes on
//! every platform. A variable font handed over as bytes is held as its
//! regular alone, and every bold drawn as regular.
//!
//! None of the Latin faces has a Chinese, Japanese or Korean character,
//! so behind the conversation's face and the titles' stands the CJK face
//! the person chose (`CJK`), as Onyx has it: under Auto, the one to begin
//! with, Noto Serif SC behind a serif and Noto Sans SC behind a sans
//! serif; then the system's faces of the chat face's kind for what it
//! lacks, and the system's emoji last (`cjk_fallbacks`). The window's own words
//! are the system's face and keep the system's Chinese.

use gpui::prelude::FluentBuilder as _;
use gpui::{div, px, App, Global, ParentElement as _, SharedString, Styled as _};

/// A face Settings offers: what `config.json` names it by, the name
/// shown, and the family the text system knows it by.
pub struct Face {
    pub key: &'static str,
    pub name: &'static str,
    pub family: &'static str,
}

pub const SERIFS: &[Face] = &[
    Face { key: "source-serif", name: "Source Serif", family: "Source Serif 4" },
    Face { key: "newsreader", name: "Newsreader", family: "Newsreader" },
    Face { key: "literata", name: "Literata", family: "Literata" },
    Face { key: "lora", name: "Lora", family: "Lora" },
];

pub const SANS: &[Face] = &[
    Face { key: "inter", name: "Inter", family: "Inter" },
    Face { key: "plex-sans", name: "IBM Plex Sans", family: "IBM Plex Sans" },
    Face { key: "source-sans", name: "Source Sans", family: "Source Sans 3" },
    Face { key: "geist", name: "Geist", family: "Geist" },
];

/// The monospace faces, JetBrains Mono first and the one a file that
/// names none gets. The second is the system's where it has one worth
/// having: Monaco on a Mac, which is Apple's and is used from the system
/// there, never carried. Elsewhere it is IBM Plex Mono, built in.
pub const MONOS: &[Face] = &[
    Face { key: "jetbrains", name: "JetBrains Mono", family: "JetBrains Mono" },
    if cfg!(target_os = "macos") { Face { key: "monaco", name: "Monaco", family: "Monaco" } } else { Face { key: "plex-mono", name: "IBM Plex Mono", family: "IBM Plex Mono" } },
    Face { key: "maple", name: "Maple Mono", family: "Maple Mono" },
    Face { key: "geist-mono", name: "Geist Mono", family: "Geist Mono" },
];

/// The faces Chinese, Japanese and Korean may be set in behind the
/// conversation's face, carried: Noto Serif SC and Noto Sans SC (Adobe
/// and Google's Source Han faces, the simplified Chinese subset), regular
/// and bold, and LXGW WenKai, a kai hand, regular and medium. `auto` is
/// none of them by name: a serif's behind a serif face and a sans serif's
/// behind a sans serif, and in a window in Japanese or traditional
/// Chinese the system's faces for it first.
pub const CJK: &[Face] = &[
    Face { key: AUTO, name: "Auto", family: "" },
    Face { key: "noto-serif-sc", name: "Noto Serif SC", family: "Noto Serif SC" },
    Face { key: "noto-sans-sc", name: "Noto Sans SC", family: "Noto Sans SC" },
    Face { key: "wenkai", name: "LXGW WenKai", family: "LXGW WenKai" },
];

/// The CJK font's first choice, and the one a file that names none gets.
pub const AUTO: &str = "auto";

/// What a carried CJK face is called in Chinese: the name the Source Han
/// faces go by for Noto's, whose files give none.
pub fn cjk_native(key: &str) -> Option<&'static str> {
    match key {
        "noto-serif-sc" => Some("思源宋体"),
        "noto-sans-sc" => Some("思源黑体"),
        "wenkai" => Some("霞鹜文楷"),
        _ => None,
    }
}

/// A face's name in its own language and in English, a dash between
/// ("仓耳今楷02 - TsangerJinKai02"); the English alone where it has no
/// other.
pub fn both_names(native: Option<&str>, name: &str) -> String {
    match native {
        Some(native) if !native.is_empty() && native != name => format!("{native} - {name}"),
        _ => name.to_string(),
    }
}

/// The key a face of the system's the person chose goes by, in the chat
/// font, the CJK font and the monospace font alike.
pub const CUSTOM: &str = "custom";

/// The faces of the system's the person chose, by family, and the one
/// style of each chosen, by its PostScript name (empty for the family as
/// it is). `chat_serif` is whether the chat's is a serif, which says
/// whether the system's serif or sans serif CJK faces stand behind it.
/// The `*_as` are the families the text system knows a chosen style by
/// (`resolve`), empty where it is the family itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Custom {
    pub chat: String,
    pub chat_serif: bool,
    pub cjk: String,
    pub mono: String,
    pub chat_style: String,
    pub cjk_style: String,
    pub mono_style: String,
    chat_as: String,
    cjk_as: String,
    mono_as: String,
}

impl Custom {
    pub fn of(app: &emaki_core::config::AppConfig, terminal: &emaki_core::config::Terminal) -> Self {
        Custom {
            chat_serif: is_serif_face(&app.chat_custom),
            chat: app.chat_custom.clone(),
            cjk: app.cjk_custom.clone(),
            mono: terminal.font_custom.clone(),
            chat_style: app.chat_custom_style.clone(),
            cjk_style: app.cjk_custom_style.clone(),
            mono_style: terminal.font_custom_style.clone(),
            ..Default::default()
        }
    }

    /// The family the chat's face is drawn as: its style's where one was
    /// chosen and could be found.
    pub fn chat_family(&self) -> &str {
        if self.chat_as.is_empty() { &self.chat } else { &self.chat_as }
    }

    pub fn cjk_family(&self) -> &str {
        if self.cjk_as.is_empty() { &self.cjk } else { &self.cjk_as }
    }

    pub fn mono_family(&self) -> &str {
        if self.mono_as.is_empty() { &self.mono } else { &self.mono_as }
    }

    /// Each style chosen as a family the text system can be handed
    /// (`style_family`).
    fn resolve(&mut self, cx: &mut App) {
        self.chat_as = style_family(&self.chat, &self.chat_style, cx).unwrap_or_default();
        self.cjk_as = style_family(&self.cjk, &self.cjk_style, cx).unwrap_or_default();
        self.mono_as = style_family(&self.mono, &self.mono_style, cx).unwrap_or_default();
    }
}

/// Whether a face of the system's is a serif, by the class its family
/// says it is of (old style, transitional, modern, Clarendon, slab or
/// freeform serifs).
#[cfg(target_os = "macos")]
pub fn is_serif_face(family: &str) -> bool {
    use core_text::font_descriptor::TraitAccessors as _;
    if family.is_empty() {
        return false;
    }
    std::panic::catch_unwind(|| {
        let faces = core_text::font_collection::create_for_family(family)?.get_descriptors()?;
        let class = faces.iter().next()?.traits().symbolic_traits() >> 28;
        Some((1..=7).contains(&class) && class != 6)
    })
    .ok()
    .flatten()
    .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
pub fn is_serif_face(_: &str) -> bool {
    false
}

/// The family a CJK face is known by: `custom` the face chosen, and
/// `auto` the one a serif or a sans serif chat face gets, which is also
/// what a key from before (or a custom face put away) reads as.
pub fn cjk_family(key: &str, custom: &Custom, serif: bool) -> String {
    match CJK.iter().find(|f| f.key == key) {
        Some(f) if !f.family.is_empty() => f.family.to_string(),
        _ if key == CUSTOM && !custom.cjk.is_empty() => custom.cjk_family().to_string(),
        _ => if serif { CJK[1].family } else { CJK[2].family }.to_string(),
    }
}

/// Whether a family is a serif: one of the serifs, or the chat's own
/// face of the system's when that is one.
pub fn is_serif(family: &str, custom: &Custom) -> bool {
    SERIFS.iter().any(|f| f.family == family) || (custom.chat_serif && custom.chat_family() == family)
}

/// The faces Chinese, Japanese and Korean fall back on behind `family`,
/// which has none of their characters: the CJK face chosen (or under Auto
/// a serif's behind a serif and a sans serif's behind the rest), then the
/// system's faces of that kind for whatever it lacks, and the system's
/// emoji last. Under Auto a window in Japanese or traditional Chinese has
/// the system's faces for it first, so kanji and traditional characters
/// take their own shapes and not the simplified ones'.
pub fn cjk_fallbacks(family: &str, fonts: &ChatFonts) -> gpui::FontFallbacks {
    let serif = is_serif(family, &fonts.custom);
    let system = system_fallbacks(serif);
    let mut faces: Vec<String> = Vec::new();
    let named = CJK.iter().any(|f| f.key == fonts.cjk && !f.family.is_empty()) || (fonts.cjk == CUSTOM && !fonts.custom.cjk.is_empty());
    if !named {
        // The system's faces for Japanese, then traditional Chinese, as
        // `system_fallbacks` lists them.
        let first: &[&str] = match crate::i18n::lang() {
            "ja" => &system[2..3],
            "zh-Hant" => &system[1..2],
            _ => &[],
        };
        faces.extend(first.iter().map(|f| f.to_string()));
    }
    faces.push(cjk_family(&fonts.cjk, &fonts.custom, serif));
    faces.extend(system.iter().map(|f| f.to_string()));
    gpui::FontFallbacks::from_fonts(faces)
}

/// The system's faces for what the carried ones lack, in one order:
/// simplified Chinese, traditional Chinese, Japanese, Korean, then emoji.
fn system_fallbacks(serif: bool) -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        if serif { &["Songti SC", "Songti TC", "Hiragino Mincho ProN", "AppleMyungjo", "Apple Color Emoji"] } else { &["PingFang SC", "PingFang TC", "Hiragino Sans", "Apple SD Gothic Neo", "Apple Color Emoji"] }
    } else if cfg!(target_os = "windows") {
        if serif { &["SimSun", "PMingLiU", "Yu Mincho", "Batang", "Segoe UI Emoji"] } else { &["Microsoft YaHei", "Microsoft JhengHei", "Yu Gothic", "Malgun Gothic", "Segoe UI Emoji"] }
    } else if serif {
        &["Noto Serif CJK SC", "Noto Serif CJK TC", "Noto Serif CJK JP", "Noto Serif CJK KR", "Noto Color Emoji"]
    } else {
        &["Noto Sans CJK SC", "Noto Sans CJK TC", "Noto Sans CJK JP", "Noto Sans CJK KR", "Noto Color Emoji"]
    }
}

/// Setting an element in one of the conversation's faces with its
/// Chinese face behind it: what every place that names the chat face or
/// the titles' face does, never `font_family` alone, or its Chinese is
/// the system's sans under a serif.
pub trait Faced: gpui::Styled + Sized {
    fn faced(mut self, family: impl Into<SharedString>, cx: &App) -> Self {
        let family = family.into();
        let fallbacks = cjk_fallbacks(&family, cx.global::<ChatFonts>());
        let style = self.text_style();
        style.font_family = Some(family);
        style.font_fallbacks = Some(fallbacks);
        self
    }
}

impl<T: gpui::Styled> Faced for T {}

/// The monospace faces this system offers.
pub fn monos() -> &'static [Face] {
    MONOS
}

/// The faces the name in the sidebar is tried in, most wanted first: a
/// light, elegant sans (Optima's flared strokes, else Avenir Next; both
/// come with macOS and are the system's, not carried here). With none of
/// them, the window's face.
const WORDMARK_FACES: &[&str] = &["Optima", "Avenir Next", "Avenir", WORDMARK_OWN];

/// The faces 絵巻 is set in beside the name, as the site has it: a Mincho,
/// the first the system has. Each system's own and never carried: a face
/// with kanji in it is a large file for two characters.
const KANJI_FACES: &[&str] = &["Hiragino Mincho ProN", "Hiragino Mincho Pro", "Yu Mincho", "YuMincho", "Noto Serif JP", "Noto Serif CJK JP"];

/// The face of that kind the window carries, for a system with none of
/// the Mac's: Belleza (etunni/belleza), a sans with Optima's flared
/// strokes, under the SIL Open Font License.
const WORDMARK_OWN: &str = "Belleza";

/// What the terminal falls back on for the letters its face lacks:
/// Kaku's own list. The symbols are built in.
pub const TERM_FALLBACKS: &[&str] = &["PingFang SC", "Apple Symbols", "Apple Color Emoji", "Symbols Nerd Font Mono"];

/// The face formulas are set in, by Typst (`emaki_core::math`) and in
/// the room each takes in a line: New Computer Modern Math, under the
/// GUST Font License (`NewCMMath-GUST.txt` beside it).
pub const MATH_FACE: &[u8] = include_bytes!("../assets/fonts/NewCMMath-Regular.otf");

/// The math face's family as the text system names it: its own name
/// table says "NewComputerModern Math" or "NewComputerModernMath", by
/// which name is read.
static MATH_FAMILY: std::sync::OnceLock<SharedString> = std::sync::OnceLock::new();

pub fn math_family() -> SharedString {
    MATH_FAMILY.get().cloned().unwrap_or_else(|| "NewComputerModern Math".into())
}

const FACES: &[&[u8]] = &[
    MATH_FACE,
    include_bytes!("../assets/fonts/Belleza-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Italic.ttf"),
    include_bytes!("../assets/fonts/Geist-Bold.ttf"),
    include_bytes!("../assets/fonts/Geist-Italic.ttf"),
    include_bytes!("../assets/fonts/Geist-Regular.ttf"),
    include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Bold.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Italic.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Italic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Literata-Bold.ttf"),
    include_bytes!("../assets/fonts/Literata-Italic.ttf"),
    include_bytes!("../assets/fonts/Literata-Regular.ttf"),
    include_bytes!("../assets/fonts/Literata-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Lora-Bold.ttf"),
    include_bytes!("../assets/fonts/Lora-Italic.ttf"),
    include_bytes!("../assets/fonts/Lora-Regular.ttf"),
    include_bytes!("../assets/fonts/Lora-SemiBold.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Bold.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Italic.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Medium.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Regular.ttf"),
    include_bytes!("../assets/fonts/MapleMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Newsreader-Bold.ttf"),
    include_bytes!("../assets/fonts/Newsreader-Italic.ttf"),
    include_bytes!("../assets/fonts/Newsreader-Regular.ttf"),
    include_bytes!("../assets/fonts/Newsreader-SemiBold.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Bold.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Italic.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Regular.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-SemiBold.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Bold.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Italic.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../assets/fonts/SourceSerif4-SemiBold.ttf"),
    include_bytes!("../assets/fonts/SymbolsNerdFontMono-Regular.ttf"),
];

/// The carried CJK faces (`CJK`). Handed to the system's text layout as
/// well as to gpui's: a face only gpui holds is never found by the names
/// a run's fallbacks list (`register_with_system`), and the CJK face is
/// only ever a fallback.
const CJK_FACES: &[&[u8]] = &[
    include_bytes!("../assets/fonts/NotoSerifSC-Regular.otf"),
    include_bytes!("../assets/fonts/NotoSerifSC-Bold.otf"),
    include_bytes!("../assets/fonts/NotoSansSC-Regular.otf"),
    include_bytes!("../assets/fonts/NotoSansSC-Bold.otf"),
    include_bytes!("../assets/fonts/LXGWWenKai-Regular.ttf"),
    include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf"),
];

/// The monospace faces code among words is cut from (`inline_cut`), by
/// the family each belongs to: those of `FACES`, and a light cut of each,
/// carried for this alone. The light one is the cut's regular.
const MONO_FACES: &[(&str, &[u8])] = &[
    ("IBM Plex Mono", include_bytes!("../assets/fonts/IBMPlexMono-Light.ttf")),
    ("IBM Plex Mono", include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")),
    ("IBM Plex Mono", include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf")),
    ("IBM Plex Mono", include_bytes!("../assets/fonts/IBMPlexMono-Italic.ttf")),
    ("Geist Mono", include_bytes!("../assets/fonts/GeistMono-Light.ttf")),
    ("Geist Mono", include_bytes!("../assets/fonts/GeistMono-Bold.ttf")),
    ("Geist Mono", include_bytes!("../assets/fonts/GeistMono-Italic.ttf")),
    ("Geist Mono", include_bytes!("../assets/fonts/GeistMono-Medium.ttf")),
    ("Geist Mono", include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
    ("Geist Mono", include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf")),
    ("JetBrains Mono", include_bytes!("../assets/fonts/JetBrainsMono-Light.ttf")),
    ("JetBrains Mono", include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf")),
    ("JetBrains Mono", include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")),
    ("JetBrains Mono", include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf")),
    ("Maple Mono", include_bytes!("../assets/fonts/MapleMono-Light.ttf")),
    ("Maple Mono", include_bytes!("../assets/fonts/MapleMono-Bold.ttf")),
    ("Maple Mono", include_bytes!("../assets/fonts/MapleMono-Italic.ttf")),
    ("Maple Mono", include_bytes!("../assets/fonts/MapleMono-Medium.ttf")),
    ("Maple Mono", include_bytes!("../assets/fonts/MapleMono-Regular.ttf")),
    ("Maple Mono", include_bytes!("../assets/fonts/MapleMono-SemiBold.ttf")),
];

/// How large code set among words is beside them: the words' size is an
/// anchor, and the code stands a little under it. A monospace face is
/// drawn large on its body, and on its plate it looked larger still: at
/// the words' own size it stood over them.
pub const INLINE_SCALE: f32 = 0.9;

/// How much lighter code among words is than the words around it, in the
/// weights' hundreds: a monospace letter is wider than a word's, and at
/// the same weight it read as heavier.
const INLINE_LIGHTER: u16 = 100;

/// How large a block of code in a file is set beside the file's words:
/// the reading view's and the rendered editor's, at Text Size's size for
/// them. As code among words, the words' size is an anchor.
pub fn code_px(words_px: f32) -> f32 {
    (words_px * INLINE_SCALE).round()
}

/// What the smaller cut of a monospace family is called after its name.
const INLINE_SUFFIX: &str = " Inline";

/// The monospace family code among words is set in: the smaller cut of
/// the one in use, or that one where no cut of it could be made.
pub fn inline_mono_family(cx: &App) -> SharedString {
    use gpui_component::ActiveTheme as _;
    let family = cx.theme().mono_font_family.clone();
    let cut = format!("{family}{INLINE_SUFFIX}");
    if cx.global::<ChatFonts>().inline.iter().any(|name| *name == cut) { cut.into() } else { family }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Where each face's table directory starts in a font file: the one face
/// of a font, or each of a collection's.
fn faces_in(font: &[u8]) -> Vec<usize> {
    match u32_at(font, 0) {
        Some(0x7474_6366) => (0..u32_at(font, 8).unwrap_or(0) as usize).filter_map(|ix| u32_at(font, 12 + ix * 4).map(|at| at as usize)).collect(),
        Some(_) => vec![0],
        None => Vec::new(),
    }
}

/// The tables of the face whose directory starts at `dir`, by tag.
fn tables_at(font: &[u8], dir: usize) -> Option<Vec<([u8; 4], Vec<u8>)>> {
    let count = u16_at(font, dir + 4)? as usize;
    let mut tables = Vec::with_capacity(count);
    for ix in 0..count {
        let rec = dir + 12 + ix * 16;
        let tag: [u8; 4] = font.get(rec..rec + 4)?.try_into().ok()?;
        let (at, len) = (u32_at(font, rec + 8)? as usize, u32_at(font, rec + 12)? as usize);
        tables.push((tag, font.get(at..at.checked_add(len)?)?.to_vec()));
    }
    Some(tables)
}

/// A font file of one face made of `tables`, under the header `head` (a
/// face's first twelve bytes), each table's sum and the file's set as a
/// font file must have them.
fn font_of(head: &[u8], tables: &mut [([u8; 4], Vec<u8>)]) -> Option<Vec<u8>> {
    let sum = |data: &[u8]| data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    });
    for (tag, data) in tables.iter_mut() {
        if tag == b"head" {
            // The file's own sum, set once the file is whole.
            data.get_mut(8..12)?.copy_from_slice(&[0; 4]);
        }
    }
    let mut out = head.get(..12)?.to_vec();
    let mut body = Vec::new();
    let mut head_at = None;
    for (tag, data) in tables.iter() {
        let at = 12 + tables.len() * 16 + body.len();
        if tag == b"head" {
            head_at = Some(at);
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&sum(data).to_be_bytes());
        out.extend_from_slice(&(at as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        body.resize(body.len().next_multiple_of(4), 0);
    }
    out.extend_from_slice(&body);
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(sum(&out));
    let head_at = head_at?;
    out.get_mut(head_at + 8..head_at + 12)?.copy_from_slice(&adjust.to_be_bytes());
    Some(out)
}

/// A font file again as a face of `family`, drawn `scale` times as large
/// at any size and `INLINE_LIGHTER` lighter: what `size-adjust` does to a
/// face in a style sheet, and a step down its weights. A paragraph is laid
/// out at one size, so code among words can be smaller only by being
/// another face.
///
/// The size is the file's own word: its glyphs are in units, and
/// `unitsPerEm` in `head` says how many make the size asked for. More
/// units to the em and everything in the file is that much smaller,
/// advances and line metrics with the outlines. The weight is the one
/// `OS/2` names, raised a step, so a family's light face is the cut's
/// regular and its regular the cut's medium: asked for at the words'
/// weight, the cut answers with the face a step under it. None for a file
/// that is not a TrueType font read whole.
fn inline_cut(font: &[u8], scale: f32, family: &str) -> Option<Vec<u8>> {
    if !matches!(u32_at(font, 0)?, 0x0001_0000 | 0x7472_7565) {
        return None;
    }
    let mut tables = tables_at(font, 0)?;
    let tight = INLINE_SUFFIX.replace(' ', "");
    for (tag, data) in tables.iter_mut() {
        match &*tag {
            b"head" => {
                let units = (u16_at(data, 18)? as f32 / scale).round().clamp(16., 16384.) as u16;
                data.get_mut(18..20)?.copy_from_slice(&units.to_be_bytes());
            }
            b"OS/2" => {
                let weight = (u16_at(data, 4)? + INLINE_LIGHTER).min(1000);
                data.get_mut(4..6)?.copy_from_slice(&weight.to_be_bytes());
            }
            b"name" => {
                *data = renamed(data, |id, text| match id {
                    // The family, the typographic family, the WWS family.
                    1 | 16 | 21 => Some(family.to_string()),
                    // The full, PostScript and unique names, and a variable
                    // font's prefix: before the style's part where there is
                    // one, so no two faces of the window share one.
                    3 | 4 | 6 | 25 => Some(tight_before_style(text, &tight)),
                    _ => None,
                })?
            }
            _ => {}
        }
    }
    font_of(font.get(..12)?, &mut tables)
}

/// `word` set into a face's name before its style's part (`Font-Bold`),
/// or after it where it has none.
fn tight_before_style(text: &str, word: &str) -> String {
    match text.find('-') {
        Some(at) => format!("{}{word}{}", &text[..at], &text[at..]),
        None => format!("{text}{word}"),
    }
}

/// The face of a font file whose PostScript name is `ps` (a collection
/// holds several), as a font of its own named `family`, at the regular
/// weight, upright: one style of a family handed over as a family by
/// itself (`style_family`).
fn face_alone(font: &[u8], ps: &str, family: &str) -> Option<Vec<u8>> {
    let dir = faces_in(font).into_iter().find(|dir| tables_at(font, *dir).is_some_and(|t| t.iter().any(|(tag, data)| tag == b"name" && name_of(data, 6, &[0x409]).as_deref() == Some(ps))))?;
    let mut tables = tables_at(font, dir)?;
    for (tag, data) in tables.iter_mut() {
        match &*tag {
            b"OS/2" => {
                data.get_mut(4..6)?.copy_from_slice(&400u16.to_be_bytes());
                // Regular, and neither bold nor italic.
                let selection = (u16_at(data, 62)? & !0b10_0001) | 0b100_0000;
                data.get_mut(62..64)?.copy_from_slice(&selection.to_be_bytes());
            }
            b"head" => {
                let style = u16_at(data, 44)? & !0b11;
                data.get_mut(44..46)?.copy_from_slice(&style.to_be_bytes());
            }
            b"name" => {
                *data = renamed(data, |id, text| match id {
                    1 | 4 | 16 | 21 => Some(family.to_string()),
                    2 | 17 => Some("Regular".to_string()),
                    3 | 6 | 25 => Some(tight_before_style(text, "Emaki")),
                    _ => None,
                })?
            }
            _ => {}
        }
    }
    font_of(font.get(dir..dir + 12)?, &mut tables)
}

/// A name of a `name` table: the record `id`, in the first of `langs`
/// (Windows language ids) it has, else in any.
fn name_of(name: &[u8], id: u16, langs: &[u16]) -> Option<String> {
    let (count, strings) = (u16_at(name, 2)? as usize, u16_at(name, 4)? as usize);
    let mut found: Vec<(usize, String)> = Vec::new();
    for ix in 0..count {
        let rec = 6 + ix * 12;
        let (platform, lang, nid) = (u16_at(name, rec)?, u16_at(name, rec + 4)?, u16_at(name, rec + 6)?);
        if nid != id || platform == 1 {
            continue;
        }
        let (len, at) = (u16_at(name, rec + 8)? as usize, strings + u16_at(name, rec + 10)? as usize);
        let Some(raw) = name.get(at..at + len) else { continue };
        let text = String::from_utf16_lossy(&raw.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect::<Vec<_>>());
        found.push((langs.iter().position(|l| *l == lang).unwrap_or(langs.len()), text));
    }
    found.into_iter().min_by_key(|(rank, _)| *rank).map(|(_, text)| text)
}

/// What a face's family is called in Chinese, Japanese or Korean, where
/// its file says: the window's language first where it is one of them.
/// None where the name has none of their characters, or is the English.
fn native_of(name: &[u8]) -> Option<String> {
    const HANS: &[u16] = &[0x804, 0x1004];
    const HANT: &[u16] = &[0x404, 0xC04, 0x1404];
    const JA: &[u16] = &[0x411];
    const KO: &[u16] = &[0x412];
    let order: Vec<u16> = match crate::i18n::lang() {
        "zh-Hant" => [HANT, HANS, JA, KO].concat(),
        "ja" => [JA, HANS, HANT, KO].concat(),
        "ko" => [KO, HANS, HANT, JA].concat(),
        _ => [HANS, HANT, JA, KO].concat(),
    };
    let english = name_of(name, 16, &[0x409]).or_else(|| name_of(name, 1, &[0x409]))?;
    let native = name_of(name, 16, &order).filter(|n| *n != english).or_else(|| name_of(name, 1, &order))?;
    let cjk = native.chars().any(|c| matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF));
    (cjk && native != english).then_some(native)
}

/// A `name` table with the names `edit` gives in place of theirs: it
/// gets each record's id and text, and answers with a new text or None
/// to keep it.
fn renamed(name: &[u8], edit: impl Fn(u16, &str) -> Option<String>) -> Option<Vec<u8>> {
    let (count, strings) = (u16_at(name, 2)? as usize, u16_at(name, 4)? as usize);
    let mut records = Vec::with_capacity(count);
    let mut storage: Vec<u8> = Vec::new();
    for ix in 0..count {
        let rec = 6 + ix * 12;
        let (platform, id) = (u16_at(name, rec)?, u16_at(name, rec + 6)?);
        let (len, at) = (u16_at(name, rec + 8)? as usize, strings + u16_at(name, rec + 10)? as usize);
        let raw = name.get(at..at + len)?;
        // Unicode and Windows names are UTF-16, a Mac's one byte a letter.
        let wide = platform != 1;
        let text: String = if wide { String::from_utf16_lossy(&raw.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect::<Vec<_>>()) } else { raw.iter().map(|b| *b as char).collect() };
        let text = edit(id, &text).unwrap_or(text);
        let bytes: Vec<u8> = if wide { text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect() } else { text.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect() };
        records.push((name.get(rec..rec + 8)?.to_vec(), bytes.len() as u16, storage.len() as u16));
        storage.extend_from_slice(&bytes);
    }
    // Format 0: the language tags of format 1 are not carried over.
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(count as u16).to_be_bytes());
    out.extend_from_slice(&((6 + count * 12) as u16).to_be_bytes());
    for (head, len, at) in records {
        out.extend_from_slice(&head);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&at.to_be_bytes());
    }
    out.extend_from_slice(&storage);
    Some(out)
}

/// The chat face `choice` names. A file from before the faces were built
/// in says `serif` or `sans`, which are the first of each.
pub fn chat_face(choice: &str) -> &'static Face {
    let choice = match choice {
        "sans" => SANS[0].key,
        "serif" | "" => SERIFS[0].key,
        other => other,
    };
    SERIFS.iter().chain(SANS).find(|f| f.key == choice).unwrap_or(&SERIFS[0])
}

/// The monospace face `choice` names: JetBrains Mono for anything else,
/// which is also what a file that chose a face no longer here gets.
pub fn mono_face(choice: &str) -> &'static Face {
    // The second place is one choice with a face a system: a file that
    // names the Mac's on a PC, or the PC's on a Mac, means that place.
    if matches!(choice, "monaco" | "plex-mono") {
        return &MONOS[1];
    }
    MONOS.iter().find(|f| f.key == choice).unwrap_or(&MONOS[0])
}

/// What the window keeps of its faces while it runs.
#[derive(Debug, Clone, Default)]
pub struct ChatFonts {
    /// The first of `WORDMARK_FACES` the system has.
    pub wordmark: Option<String>,
    /// The first of `KANJI_FACES` the system has.
    pub kanji: Option<String>,
    /// The family titles and the greeting are set in: the chat's.
    pub display: String,
    /// The smaller cuts of the monospace faces that were made
    /// (`inline_cut`), by family.
    pub inline: Vec<String>,
    /// The CJK face chosen, by its key in `CJK` or `custom`.
    pub cjk: String,
    /// The faces of the system's chosen, their styles found.
    pub custom: Custom,
}

impl Global for ChatFonts {}

/// The faces made here out of one style of a system's family
/// (`style_family`), by the family each was made as, its bytes kept for
/// as long as the app runs: the text system draws from them.
static MADE: std::sync::Mutex<Vec<(String, &'static [u8])>> = std::sync::Mutex::new(Vec::new());

/// The family each style chosen is drawn as, by its PostScript name,
/// once found.
static STYLED: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// Hand the built-in faces to the text system. `chat` is the chat font
/// `config.json` asks for, which titles follow.
pub fn install(chat: &str, cjk: &str, mut custom: Custom, cx: &mut App) {
    let _ = cx.text_system().add_fonts(FACES.iter().chain(CJK_FACES).map(|b| std::borrow::Cow::Borrowed(*b)).collect());
    for face in CJK_FACES {
        register_with_system(face);
    }
    emaki_core::math::use_font(MATH_FACE);
    // The smaller cut of each monospace face, for code among words: the
    // built-in ones, and the system's own where that is one of the
    // choices, read from the system and never carried.
    let system = if cfg!(target_os = "macos") { std::fs::read("/System/Library/Fonts/Monaco.ttf").ok() } else { None };
    let cuts: Vec<std::borrow::Cow<'static, [u8]>> = MONO_FACES.iter().copied().chain(system.as_deref().map(|font| ("Monaco", font))).filter_map(|(family, font)| inline_cut(font, INLINE_SCALE, &format!("{family}{INLINE_SUFFIX}"))).map(std::borrow::Cow::Owned).collect();
    let _ = cx.text_system().add_fonts(cuts);
    let names = cx.text_system().all_font_names();
    if let Some(name) = names.iter().find(|n| n.replace(' ', "") == "NewComputerModernMath") {
        let _ = MATH_FAMILY.set(name.clone().into());
    }
    let inline = names.iter().filter(|name| name.ends_with(INLINE_SUFFIX)).cloned().collect();
    // `EMAKI_WORDMARK=<family>` draws the name in that face, for a look
    // at the one Windows and Linux get.
    let asked = std::env::var("EMAKI_WORDMARK").ok();
    let wordmark = asked.or_else(|| WORDMARK_FACES.iter().find(|f| names.iter().any(|n| n == *f)).map(|f| f.to_string()));
    let kanji = KANJI_FACES.iter().find(|f| names.iter().any(|n| n == *f)).map(|f| f.to_string());
    custom.resolve(cx);
    let display = chat_name(chat, &custom);
    let mono = custom.mono_family().to_string();
    cx.set_global(ChatFonts { wordmark, kanji, display, inline, cjk: cjk.to_string(), custom });
    cut_custom_mono(&mono, cx);
}

/// The family TsangerJinKai is installed under, among `names`, for a
/// file that chose it when it was one of the CJK faces: it is a custom
/// face now, its second cut (TsangerJinKai02) first, else any of its cuts.
pub fn tsanger_in(names: &[String]) -> Option<String> {
    names.iter().find(|n| *n == "TsangerJinKai02").or_else(|| names.iter().find(|n| n.starts_with("TsangerJinKai"))).cloned()
}

/// The CJK face chosen.
pub fn set_cjk(cjk: &str, cx: &mut App) {
    cx.global_mut::<ChatFonts>().cjk = cjk.to_string();
}

/// The chat font changed: titles go with it.
pub fn set_chat(chat: &str, cx: &mut App) {
    let fonts = cx.global_mut::<ChatFonts>();
    fonts.display = chat_name(chat, &fonts.custom);
}

/// A face of the system's chosen for a custom place, or a style of one;
/// titles follow the chat font, and a monospace face gets its smaller cut
/// for code among words where one can be made.
pub fn set_custom(mut custom: Custom, chat: &str, cx: &mut App) {
    custom.resolve(cx);
    let mono = (cx.global::<ChatFonts>().custom.mono_family() != custom.mono_family()).then(|| custom.mono_family().to_string());
    let fonts = cx.global_mut::<ChatFonts>();
    fonts.display = chat_name(chat, &custom);
    fonts.custom = custom;
    if let Some(mono) = mono {
        cut_custom_mono(&mono, cx);
    }
}

/// The family one style of a system's family is drawn as, so that style
/// and no other is what the text system picks for it: asked for by its
/// family and the regular weight, it would pick the face nearest that
/// weight, and a family whose styles all say regular (TsangerJinKai02's
/// W01 to W05) gave its first whatever was chosen.
///
/// The style's own family name, where its file has one that names it
/// alone (`TsangerJinKai02 W03`, `Avenir Next Medium`): nothing is
/// loaded. Else the face is read from its file, made a family of its own
/// (`face_alone`), and handed to the text system and to the system's text
/// layout, which finds a fallback by name; its bytes stay while the app
/// runs. None for no style, or one not found.
#[cfg(target_os = "macos")]
fn style_family(family: &str, style: &str, cx: &mut App) -> Option<String> {
    use core_text::font_descriptor::{kCTFontItalicTrait, TraitAccessors as _};
    if family.is_empty() || style.is_empty() {
        return None;
    }
    if let Some((_, found)) = STYLED.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|(ps, _)| ps == style) {
        return Some(found.clone());
    }
    enum Way {
        Named(String),
        Made(std::path::PathBuf, String),
    }
    let way = std::panic::catch_unwind(|| {
        let faces = core_text::font_collection::create_for_family(family)?.get_descriptors()?;
        let face = faces.iter().find(|d| d.font_name() == style)?;
        let names = core_text::font::new_from_descriptor(&face, 12.).get_font_table(u32::from_be_bytes(*b"name")).map(|d| d.bytes().to_vec());
        if let Some(own) = names.as_deref().and_then(|n| name_of(n, 1, &[0x409])).filter(|own| own != family) {
            let upright: Option<Vec<String>> = core_text::font_collection::create_for_family(&own)
                .and_then(|c| c.get_descriptors())
                .map(|those| those.iter().filter(|d| d.traits().symbolic_traits() & kCTFontItalicTrait == 0).map(|d| d.font_name()).collect());
            if upright.is_some_and(|u| u == [style]) {
                return Some(Way::Named(own));
            }
        }
        Some(Way::Made(face.font_path()?, format!("{family} {}", face.style_name())))
    })
    .ok()
    .flatten()?;
    let found = match way {
        Way::Named(own) => own,
        Way::Made(path, made) => {
            let bytes = std::fs::read(path).ok()?;
            let alone: &'static [u8] = Box::leak(face_alone(&bytes, style, &made)?.into_boxed_slice());
            cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(alone)]).ok()?;
            register_with_system(alone);
            MADE.lock().unwrap_or_else(|p| p.into_inner()).push((made.clone(), alone));
            made
        }
    };
    STYLED.lock().unwrap_or_else(|p| p.into_inner()).push((style.to_string(), found.clone()));
    Some(found)
}

#[cfg(not(target_os = "macos"))]
fn style_family(_: &str, _: &str, _: &mut App) -> Option<String> {
    None
}

/// A face held as bytes made known to the system's text layout for as
/// long as the app runs: gpui hands its own faces to Core Text unnamed,
/// and a run's fallbacks are found by name among the faces Core Text
/// knows, so a carried face was never one where it was not also
/// installed.
#[cfg(target_os = "macos")]
fn register_with_system(bytes: &'static [u8]) {
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDataProviderCreateWithData(info: *mut c_void, data: *const c_void, size: usize, release: *const c_void) -> *mut c_void;
        fn CGDataProviderRelease(provider: *mut c_void);
        fn CGFontCreateWithDataProvider(provider: *mut c_void) -> *mut c_void;
        fn CGFontRelease(font: *mut c_void);
    }
    #[link(name = "CoreText", kind = "framework")]
    extern "C" {
        fn CTFontManagerRegisterGraphicsFont(font: *mut c_void, error: *mut *mut c_void) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(obj: *mut c_void);
    }
    // SAFETY: the bytes live as long as the app; each object made here is
    // released once, and an error is released where one is given.
    unsafe {
        let provider = CGDataProviderCreateWithData(std::ptr::null_mut(), bytes.as_ptr().cast(), bytes.len(), std::ptr::null());
        if provider.is_null() {
            return;
        }
        let font = CGFontCreateWithDataProvider(provider);
        CGDataProviderRelease(provider);
        if font.is_null() {
            return;
        }
        let mut error: *mut c_void = std::ptr::null_mut();
        // False where the system already has a face of that name, which
        // is then the one found.
        let _ = CTFontManagerRegisterGraphicsFont(font, &mut error);
        if !error.is_null() {
            CFRelease(error);
        }
        CGFontRelease(font);
    }
}

#[cfg(not(target_os = "macos"))]
fn register_with_system(_: &'static [u8]) {}

/// The smaller cut of a monospace face of the system's (`inline_cut`),
/// from its file, once: a face whose file is not a TrueType font read
/// whole (a collection, a CFF face) has none, and code among words is set
/// in the face itself at the words' size.
fn cut_custom_mono(family: &str, cx: &mut App) {
    let name = format!("{family}{INLINE_SUFFIX}");
    if family.is_empty() || cx.global::<ChatFonts>().inline.contains(&name) {
        return;
    }
    let made = MADE.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|(f, _)| f == family).map(|(_, bytes)| bytes.to_vec());
    let Some(cut) = made.or_else(|| system_font_file(family)).and_then(|bytes| inline_cut(&bytes, INLINE_SCALE, &name)) else { return };
    if cx.text_system().add_fonts(vec![std::borrow::Cow::Owned(cut)]).is_ok() && cx.text_system().all_font_names().contains(&name) {
        cx.global_mut::<ChatFonts>().inline.push(name);
    }
}

/// The file a face of the system's regular cut is in.
#[cfg(target_os = "macos")]
fn system_font_file(family: &str) -> Option<Vec<u8>> {
    use core_text::font_descriptor::TraitAccessors as _;
    let found = std::panic::catch_unwind(|| {
        let faces = core_text::font_collection::create_for_family(family)?.get_descriptors()?;
        let paths: Vec<std::path::PathBuf> = faces.iter().filter_map(|d| d.font_path()).collect();
        // The upright one at the regular weight, where the family says.
        faces.iter().find(|d| d.traits().symbolic_traits() & (core_text::font_descriptor::kCTFontBoldTrait | core_text::font_descriptor::kCTFontItalicTrait) == 0).and_then(|d| d.font_path()).or_else(|| paths.first().cloned())
    });
    std::fs::read(found.ok().flatten()?).ok()
}

#[cfg(not(target_os = "macos"))]
fn system_font_file(_: &str) -> Option<Vec<u8>> {
    None
}

/// The family the chat font `choice` is set in: the face of the system's
/// chosen, for `custom` while there is one.
pub fn chat_name(choice: &str, custom: &Custom) -> String {
    if choice == CUSTOM && !custom.chat.is_empty() {
        return custom.chat_family().to_string();
    }
    chat_face(choice).family.to_string()
}

/// The family the terminal and all code are set in, for a choice in the
/// settings.
pub fn term_family(choice: &str, cx: &App) -> SharedString {
    let custom = &cx.global::<ChatFonts>().custom;
    if choice == CUSTOM && !custom.mono.is_empty() {
        return custom.mono_family().to_string().into();
    }
    mono_face(choice).family.into()
}

/// A place a face of the system's can be chosen for, in Settings, Text:
/// the chat font, the CJK font, the monospace font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Chat,
    Cjk,
    Mono,
}

impl Slot {
    /// The list its faces are chosen from.
    pub fn kinds(self) -> Kinds {
        match self {
            Slot::Cjk => Kinds::Cjk,
            Slot::Mono => Kinds::Mono,
            Slot::Chat => Kinds::All,
        }
    }

    /// The face chosen for it, empty while none was.
    pub fn family(self, custom: &Custom) -> &str {
        match self {
            Slot::Chat => &custom.chat,
            Slot::Cjk => &custom.cjk,
            Slot::Mono => &custom.mono,
        }
    }

    /// The style of it chosen, by its PostScript name; empty for the
    /// family as it is.
    pub fn style(self, custom: &Custom) -> &str {
        match self {
            Slot::Chat => &custom.chat_style,
            Slot::Cjk => &custom.cjk_style,
            Slot::Mono => &custom.mono_style,
        }
    }
}

/// What a list of the system's faces is for: every face, those with
/// CJK characters, or the monospaced ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kinds {
    All,
    Cjk,
    Mono,
}

/// One upright style of a family: its PostScript name, which a choice
/// is kept by, and the name the family gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub ps: String,
    pub name: String,
}

/// What a list says of a family beside its name: its name in Chinese,
/// Japanese or Korean where its file has one, and its upright styles,
/// lightest first.
#[derive(Debug, Clone, Default)]
pub struct About {
    pub native: Option<String>,
    pub styles: Vec<Style>,
}

/// The system's font families, by name, for each kind of list, and what
/// is known of each. Names beginning with a dot are the system's own and
/// left out, and so are the faces made or carried here.
#[derive(Debug, Clone, Default)]
pub struct Families {
    pub all: Vec<String>,
    pub cjk: Vec<String>,
    pub mono: Vec<String>,
    pub about: std::collections::HashMap<String, About>,
}

impl Families {
    pub fn of(&self, kinds: Kinds) -> &[String] {
        match kinds {
            Kinds::All => &self.all,
            Kinds::Cjk => &self.cjk,
            Kinds::Mono => &self.mono,
        }
    }

    /// A family as a list shows it: in its own language and in English
    /// where it has both (`both_names`).
    pub fn shown(&self, family: &str) -> String {
        both_names(self.about.get(family).and_then(|a| a.native.as_deref()), family)
    }

    /// Its upright styles; none where it has only one.
    pub fn styles(&self, family: &str) -> &[Style] {
        self.about.get(family).map(|a| a.styles.as_slice()).filter(|s| s.len() > 1).unwrap_or(&[])
    }
}

/// What is known of one family of the system's (`About`), read once and
/// kept: what the tile of a face chosen says, drawn before any list of
/// the system's faces is read.
pub fn about(family: &str) -> About {
    static KEPT: std::sync::Mutex<Vec<(String, About)>> = std::sync::Mutex::new(Vec::new());
    if family.is_empty() {
        return About::default();
    }
    if let Some((_, found)) = KEPT.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|(f, _)| f == family) {
        return found.clone();
    }
    let found = read_about(family);
    KEPT.lock().unwrap_or_else(|p| p.into_inner()).push((family.to_string(), found.clone()));
    found
}

#[cfg(target_os = "macos")]
fn read_about(family: &str) -> About {
    use core_text::font_descriptor::TraitAccessors as _;
    std::panic::catch_unwind(|| {
        let faces = core_text::font_collection::create_for_family(family)?.get_descriptors()?;
        let first = faces.iter().next()?;
        let native = core_text::font::new_from_descriptor(&first, 12.).get_font_table(u32::from_be_bytes(*b"name")).and_then(|d| native_of(d.bytes()));
        let mut styles: Vec<(f64, Style)> = faces
            .iter()
            .filter(|d| d.traits().symbolic_traits() & core_text::font_descriptor::kCTFontItalicTrait == 0)
            .map(|d| (d.traits().normalized_weight(), Style { ps: d.font_name(), name: d.style_name() }))
            .collect();
        styles.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(About { native, styles: styles.into_iter().map(|(_, s)| s).collect() })
    })
    .ok()
    .flatten()
    .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
fn read_about(_: &str) -> About {
    About::default()
}

/// Whether a family is one this window made or carries, and not the
/// system's to offer.
fn ours(family: &str) -> bool {
    family.ends_with(INLINE_SUFFIX) || CJK.iter().any(|f| f.family == family) || MADE.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|(f, _)| f == family)
}

/// The system's font families, read off the main thread: each family's
/// first face asked whether it is monospaced, and whether it has 汉, あ or
/// 한, and where it has, what it is called in that language; and each of
/// its faces that is upright, a style. A face that cannot be read is left
/// out, not the list.
#[cfg(target_os = "macos")]
pub fn system_families() -> Families {
    use core_text::font_descriptor::SymbolicTraitAccessors as _;
    use core_text::font_descriptor::TraitAccessors as _;
    let Some(faces) = std::panic::catch_unwind(|| core_text::font_collection::create_for_all_families().get_descriptors()).ok().flatten() else { return Families::default() };
    let mut out = Families::default();
    // Each style with its weight, to be put in order.
    let mut weighed: std::collections::HashMap<String, Vec<(f64, Style)>> = std::collections::HashMap::new();
    for face in faces.iter() {
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let family = face.family_name();
            if family.starts_with('.') || ours(&family) {
                return None;
            }
            let traits = face.traits();
            let upright = traits.symbolic_traits() & core_text::font_descriptor::kCTFontItalicTrait == 0;
            let style = upright.then(|| (traits.normalized_weight(), Style { ps: face.font_name(), name: face.style_name() }));
            if out.about.contains_key(&family) {
                return Some((family, None, style));
            }
            let mono = traits.symbolic_traits().is_monospace();
            let font = core_text::font::new_from_descriptor(&face, 12.);
            let cjk = ['汉', 'あ', '한'].iter().any(|c| {
                let mut unit = [0u16; 2];
                let unit = c.encode_utf16(&mut unit);
                let mut glyphs = [0u16; 2];
                unsafe { font.get_glyphs_for_characters(unit.as_ptr(), glyphs.as_mut_ptr(), unit.len() as isize) }
            });
            let native = if cjk { font.get_font_table(u32::from_be_bytes(*b"name")).and_then(|d| native_of(d.bytes())) } else { None };
            Some((family, Some((mono, cjk, native)), style))
        }));
        let Ok(Some((family, first, style))) = read else { continue };
        if let Some((mono, cjk, native)) = first {
            if mono {
                out.mono.push(family.clone());
            }
            if cjk {
                out.cjk.push(family.clone());
            }
            out.all.push(family.clone());
            out.about.insert(family.clone(), About { native, styles: Vec::new() });
        }
        if let Some(style) = style {
            weighed.entry(family).or_default().push(style);
        }
    }
    for (family, mut styles) in weighed {
        styles.sort_by(|a, b| a.0.total_cmp(&b.0));
        styles.dedup_by(|a, b| a.1.ps == b.1.ps);
        if let Some(about) = out.about.get_mut(&family) {
            about.styles = styles.into_iter().map(|(_, s)| s).collect();
        }
    }
    for list in [&mut out.all, &mut out.cjk, &mut out.mono] {
        list.sort_by_cached_key(|f| f.to_lowercase());
    }
    out
}

/// Elsewhere every family the text system knows, in each list: what a
/// face holds is not asked there.
#[cfg(not(target_os = "macos"))]
pub fn system_families_of(names: Vec<String>) -> Families {
    let mut all: Vec<String> = names.into_iter().filter(|f| !f.starts_with('.') && !ours(f)).collect();
    all.sort_by_cached_key(|f| f.to_lowercase());
    all.dedup();
    Families { cjk: all.clone(), mono: all.clone(), all, about: Default::default() }
}

/// The family the app's name is set in, in the sidebar, or None to leave
/// the window's own face.
pub fn wordmark_family(cx: &App) -> Option<SharedString> {
    cx.global::<ChatFonts>().wordmark.clone().map(SharedString::from)
}

/// 絵巻, to stand after the name set at `size`: in a Mincho, at 0.78 of the
/// name, the two a little apart and a little off it, as the site has them.
/// gpui has no letter spacing, so each character is drawn on its own.
pub fn kanji(size: f32, cx: &App) -> gpui::Div {
    let at = size * 0.78;
    div()
        .flex()
        .flex_shrink_0()
        .ml(px(size * 0.18))
        .gap(px(at * 0.06))
        .text_size(px(at))
        .when_some(cx.global::<ChatFonts>().kanji.clone(), |d, f| d.font_family(SharedString::from(f)))
        .child("絵")
        .child("巻")
}

/// The face titles and the greeting are set in: the conversation's, so a
/// page title and the conversation agree.
pub fn display_family(cx: &App) -> SharedString {
    SharedString::from(cx.global::<ChatFonts>().display.clone())
}

/// The family a conversation is drawn in for `choice`.
pub fn chat_family(choice: &str, cx: &App) -> Option<SharedString> {
    Some(chat_name(choice, &cx.global::<ChatFonts>().custom).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table of a font file, by its tag.
    fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
        let word = |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap()) as usize;
        let count = u16::from_be_bytes([font[4], font[5]]) as usize;
        let rec = (0..count).map(|ix| 12 + ix * 16).find(|rec| &font[*rec..*rec + 4] == tag).unwrap();
        &font[word(rec + 8)..word(rec + 8) + word(rec + 12)]
    }

    /// A table of the face whose directory is at `dir`.
    fn table_at<'a>(font: &'a [u8], dir: usize, tag: &[u8; 4]) -> &'a [u8] {
        let word = |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap()) as usize;
        let count = u16::from_be_bytes([font[dir + 4], font[dir + 5]]) as usize;
        let rec = (0..count).map(|ix| dir + 12 + ix * 16).find(|rec| &font[*rec..*rec + 4] == tag).unwrap();
        &font[word(rec + 8)..word(rec + 8) + word(rec + 12)]
    }

    /// Every family name in a `name` table, as text.
    fn families(font: &[u8]) -> Vec<String> {
        let name = table(font, b"name");
        let half = |at: usize| u16::from_be_bytes([name[at], name[at + 1]]) as usize;
        (0..half(2))
            .map(|ix| 6 + ix * 12)
            .filter(|rec| matches!(half(rec + 6), 1 | 16))
            .map(|rec| {
                let raw = &name[half(4) + half(rec + 10)..half(4) + half(rec + 10) + half(rec + 8)];
                // A Mac's names are a byte a letter, the rest UTF-16.
                if half(rec) == 1 { raw.iter().map(|b| *b as char).collect() } else { String::from_utf16_lossy(&raw.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect::<Vec<_>>()) }
            })
            .collect()
    }

    #[test]
    fn a_cut_is_the_face_smaller_and_lighter_under_a_name_of_its_own() {
        let system = std::fs::read("/System/Library/Fonts/Monaco.ttf").ok();
        for (family, font) in MONO_FACES.iter().copied().chain(system.as_deref().map(|f| ("Monaco", f))) {
            let name = format!("{family}{INLINE_SUFFIX}");
            let cut = inline_cut(font, INLINE_SCALE, &name).expect("a TrueType face");
            // More units to the em: everything drawn that much smaller.
            let units = |font: &[u8]| u16::from_be_bytes([table(font, b"head")[18], table(font, b"head")[19]]) as f32;
            assert_eq!(units(&cut), (units(font) / INLINE_SCALE).round());
            // A step heavier by its own word, so asked for at a weight it
            // answers with the face a step under it.
            let weight = |font: &[u8]| u16::from_be_bytes([table(font, b"OS/2")[4], table(font, b"OS/2")[5]]);
            assert_eq!(weight(&cut), weight(font) + INLINE_LIGHTER);
            // Found by the family of its own, never the face's.
            let now = families(&cut);
            assert!(!now.is_empty() && now.iter().all(|n| *n == name), "{now:?}");
            // The outlines are the face's own, and the file sums as a
            // font file must.
            assert_eq!(table(font, b"glyf"), table(&cut, b"glyf"));
            let sum = cut.chunks(4).fold(0u32, |sum, chunk| {
                let mut word = [0u8; 4];
                word[..chunk.len()].copy_from_slice(chunk);
                sum.wrapping_add(u32::from_be_bytes(word))
            });
            assert_eq!(sum, 0xB1B0_AFBA);
        }
    }

    #[test]
    fn a_light_cut_is_the_regular_of_its_cut_family() {
        // Each family's light face, once cut, is at the weight its regular
        // was, so words at the regular weight get it.
        for family in ["JetBrains Mono", "Maple Mono", "Geist Mono", "IBM Plex Mono"] {
            let weights: Vec<u16> = MONO_FACES.iter().filter(|(f, _)| *f == family).map(|(_, font)| inline_cut(font, INLINE_SCALE, "x").unwrap()).map(|cut| u16::from_be_bytes([table(&cut, b"OS/2")[4], table(&cut, b"OS/2")[5]])).collect();
            assert!(weights.contains(&400), "{family}: {weights:?}");
        }
    }

    #[test]
    fn one_face_of_a_collection_stands_alone_as_a_family() {
        let Ok(font) = std::fs::read("/System/Library/Fonts/Supplemental/Songti.ttc") else { return };
        let dirs = faces_in(&font);
        assert!(dirs.len() > 1);
        let ps = name_of(table_at(&font, dirs[1], b"name"), 6, &[0x409]).unwrap();
        let alone = face_alone(&font, &ps, "Songti Test").expect("the face");
        assert_eq!(families(&alone), vec!["Songti Test".to_string(); families(&alone).len()]);
        assert_eq!(u16::from_be_bytes([table(&alone, b"OS/2")[4], table(&alone, b"OS/2")[5]]), 400);
    }

    #[test]
    fn a_family_is_shown_in_its_own_language_and_in_english() {
        assert_eq!(both_names(Some("仓耳今楷02"), "TsangerJinKai02"), "仓耳今楷02 - TsangerJinKai02");
        assert_eq!(both_names(None, "Menlo"), "Menlo");
        let wenkai = table(CJK_FACES[4], b"name");
        assert_eq!(native_of(wenkai).as_deref(), Some("霞鹜文楷"));
        assert_eq!(native_of(table(CJK_FACES[0], b"name")), None);
    }
}
