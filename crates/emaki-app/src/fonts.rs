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
//! the person chose (`CJK`), as Onyx has it, Noto Serif SC to begin with,
//! then the system's faces of the chat face's kind for what it lacks,
//! and the system's emoji last (`cjk_fallbacks`). The window's own words
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
/// conversation's face: Noto Serif SC and Noto Sans SC (Adobe and
/// Google's Source Han faces, the simplified Chinese subset), regular and
/// bold, and LXGW WenKai, a kai hand, regular and medium, carried; and
/// TsangerJinKai, a kai hand of Beijing Tsanger's, the system's where it
/// is installed and never carried: its file asks for the maker's written
/// leave for any use (`ChatFonts::tsanger`). Noto Serif SC is the one to
/// begin with.
pub const CJK: &[Face] = &[
    Face { key: "noto-serif-sc", name: "Noto Serif SC", family: "Noto Serif SC" },
    Face { key: "noto-sans-sc", name: "Noto Sans SC", family: "Noto Sans SC" },
    Face { key: "wenkai", name: "LXGW WenKai", family: "LXGW WenKai" },
    Face { key: TSANGER, name: "TsangerJinKai", family: "" },
];

/// TsangerJinKai's key, whose family is the one installed.
pub const TSANGER: &str = "tsanger";

/// The family TsangerJinKai is installed under, among `names`: its
/// second cut (TsangerJinKai02, the one pingfan.org sets its words in)
/// first, else any of its cuts; empty where none is.
pub fn tsanger_in(names: &[String]) -> String {
    names.iter().find(|n| *n == "TsangerJinKai02").or_else(|| names.iter().find(|n| n.starts_with("TsangerJinKai"))).cloned().unwrap_or_default()
}

/// Whether TsangerJinKai is installed, looked at again: when Settings
/// comes forward, so one installed while the window runs is found.
pub fn find_tsanger(cx: &mut App) {
    let found = tsanger_in(&cx.text_system().all_font_names());
    if cx.global::<ChatFonts>().tsanger != found {
        cx.global_mut::<ChatFonts>().tsanger = found;
    }
}

/// The key a face of the system's the person chose goes by, in the chat
/// font, the CJK font and the monospace font alike.
pub const CUSTOM: &str = "custom";

/// The faces of the system's the person chose, by family; empty where
/// none was. `chat_serif` is whether the chat's is a serif, which says
/// whether the system's serif or sans serif CJK faces stand behind it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Custom {
    pub chat: String,
    pub chat_serif: bool,
    pub cjk: String,
    pub mono: String,
}

impl Custom {
    pub fn of(app: &emaki_core::config::AppConfig, terminal: &emaki_core::config::Terminal) -> Self {
        Custom { chat_serif: is_serif_face(&app.chat_custom), chat: app.chat_custom.clone(), cjk: app.cjk_custom.clone(), mono: terminal.font_custom.clone() }
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

/// The family a CJK face is known by. TsangerJinKai's is the one
/// installed (`tsanger`), and `custom` the face chosen. Where that is
/// none, and for a key from before (`auto`, which paired a serif with a
/// serif), it is the first.
pub fn cjk_family(key: &str, custom: &Custom, tsanger: &str) -> String {
    match CJK.iter().find(|f| f.key == key) {
        Some(f) if !f.family.is_empty() => f.family.to_string(),
        _ if key == TSANGER && !tsanger.is_empty() => tsanger.to_string(),
        _ if key == CUSTOM && !custom.cjk.is_empty() => custom.cjk.clone(),
        _ => CJK[0].family.to_string(),
    }
}

/// Whether a family is a serif: one of the serifs, or the chat's own
/// face of the system's when that is one.
pub fn is_serif(family: &str, custom: &Custom) -> bool {
    SERIFS.iter().any(|f| f.family == family) || (custom.chat_serif && custom.chat == family)
}

/// The faces Chinese, Japanese and Korean fall back on behind `family`,
/// which has none of their characters: the CJK face chosen, then the
/// system's faces of the chat face's kind (serif or sans serif) for
/// whatever it lacks, and the system's emoji last. Auto, which put the
/// system's faces first in a window in Japanese or traditional Chinese,
/// was taken out at the person's word.
pub fn cjk_fallbacks(family: &str, fonts: &ChatFonts) -> gpui::FontFallbacks {
    let system = system_fallbacks(is_serif(family, &fonts.custom));
    let mut faces = vec![cjk_family(&fonts.cjk, &fonts.custom, &fonts.tsanger)];
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
    include_bytes!("../assets/fonts/NotoSerifSC-Regular.otf"),
    include_bytes!("../assets/fonts/NotoSerifSC-Bold.otf"),
    include_bytes!("../assets/fonts/NotoSansSC-Regular.otf"),
    include_bytes!("../assets/fonts/NotoSansSC-Bold.otf"),
    include_bytes!("../assets/fonts/LXGWWenKai-Regular.ttf"),
    include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf"),
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

/// The monospace faces among `FACES`, which a conversation also has a
/// smaller cut of (`inline_cut`).
const MONO_FACES: &[&[u8]] = &[
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Italic.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Bold.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Italic.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Bold.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Italic.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Medium.ttf"),
    include_bytes!("../assets/fonts/MapleMono-Regular.ttf"),
    include_bytes!("../assets/fonts/MapleMono-SemiBold.ttf"),
];

/// How large code set among a conversation's words is beside them. A
/// monospace face is drawn large on its body: at the words' own size it
/// stood over them.
pub const INLINE_SCALE: f32 = 0.875;

/// What the smaller cut of a monospace family is called after its name.
const INLINE_SUFFIX: &str = " Inline";

/// The monospace family code among a conversation's words is set in:
/// the smaller cut of the one in use, or that one where no cut of it
/// could be made.
pub fn inline_mono_family(cx: &App) -> SharedString {
    use gpui_component::ActiveTheme as _;
    let family = cx.theme().mono_font_family.clone();
    let cut = format!("{family}{INLINE_SUFFIX}");
    if cx.global::<ChatFonts>().inline.iter().any(|name| *name == cut) { cut.into() } else { family }
}

/// A font file again as a family of its own, `suffix` after its name,
/// drawn `scale` times as large at any size: what `size-adjust` does to
/// a face in a style sheet. A paragraph is laid out at one size, so
/// code among words can be smaller only by being another face.
///
/// The size is the file's own word: its glyphs are in units, and
/// `unitsPerEm` in `head` says how many make the size asked for. More
/// units to the em and everything in the file is that much smaller,
/// advances and line metrics with the outlines. The names are in
/// `name`. None for a file that is not a TrueType font read whole.
fn inline_cut(font: &[u8], scale: f32, suffix: &str) -> Option<Vec<u8>> {
    let u16_at = |data: &[u8], at: usize| data.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let u32_at = |data: &[u8], at: usize| data.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    if !matches!(u32_at(font, 0)?, 0x0001_0000 | 0x7472_7565) {
        return None;
    }
    let count = u16_at(font, 4)? as usize;
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(count);
    for ix in 0..count {
        let rec = 12 + ix * 16;
        let tag: [u8; 4] = font.get(rec..rec + 4)?.try_into().ok()?;
        let (at, len) = (u32_at(font, rec + 8)? as usize, u32_at(font, rec + 12)? as usize);
        tables.push((tag, font.get(at..at.checked_add(len)?)?.to_vec()));
    }
    for (tag, data) in tables.iter_mut() {
        match &*tag {
            b"head" => {
                let units = (u16_at(data, 18)? as f32 / scale).round().clamp(16., 16384.) as u16;
                data.get_mut(18..20)?.copy_from_slice(&units.to_be_bytes());
                // The file's own sum, set once the file is whole.
                data.get_mut(8..12)?.copy_from_slice(&[0; 4]);
            }
            b"name" => *data = renamed(data, suffix)?,
            _ => {}
        }
    }
    let sum = |data: &[u8]| data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    });
    let mut out = font.get(..12)?.to_vec();
    let mut body = Vec::new();
    let mut head_at = None;
    for (tag, data) in &tables {
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

/// A `name` table with `suffix` in every name a family is found by: the
/// family's after it, and in the full, PostScript and unique names
/// where the family's ends, so no two faces of the window share one.
fn renamed(name: &[u8], suffix: &str) -> Option<Vec<u8>> {
    let u16_at = |at: usize| name.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let (count, strings) = (u16_at(2)? as usize, u16_at(4)? as usize);
    let mut records = Vec::with_capacity(count);
    let mut storage: Vec<u8> = Vec::new();
    for ix in 0..count {
        let rec = 6 + ix * 12;
        let (platform, id) = (u16_at(rec)?, u16_at(rec + 6)?);
        let (len, at) = (u16_at(rec + 8)? as usize, strings + u16_at(rec + 10)? as usize);
        let raw = name.get(at..at + len)?;
        // Unicode and Windows names are UTF-16, a Mac's one byte a letter.
        let wide = platform != 1;
        let text: String = if wide { String::from_utf16_lossy(&raw.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect::<Vec<_>>()) } else { raw.iter().map(|b| *b as char).collect() };
        let text = match id {
            // The family, the typographic family, the WWS family.
            1 | 16 | 21 => format!("{text}{suffix}"),
            // The full, PostScript and unique names, and a variable
            // font's prefix: before the style's part where there is one.
            3 | 4 | 6 | 25 => {
                let tight = suffix.replace(' ', "");
                match text.find('-') {
                    Some(at) => format!("{}{tight}{}", &text[..at], &text[at..]),
                    None => format!("{text}{tight}"),
                }
            }
            _ => text,
        };
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
    /// The faces of the system's chosen.
    pub custom: Custom,
    /// The family TsangerJinKai is installed under, empty where it is
    /// not (`tsanger_in`).
    pub tsanger: String,
}

impl Global for ChatFonts {}

/// Hand the built-in faces to the text system. `chat` is the chat font
/// `config.json` asks for, which titles follow.
pub fn install(chat: &str, cjk: &str, custom: Custom, cx: &mut App) {
    let _ = cx.text_system().add_fonts(FACES.iter().map(|b| std::borrow::Cow::Borrowed(*b)).collect());
    emaki_core::math::use_font(MATH_FACE);
    // The smaller cut of each monospace face, for code among a
    // conversation's words: the built-in ones, and the system's own
    // where that is one of the choices, read from the system and never
    // carried.
    let system = if cfg!(target_os = "macos") { std::fs::read("/System/Library/Fonts/Monaco.ttf").ok() } else { None };
    let cuts: Vec<std::borrow::Cow<'static, [u8]>> = MONO_FACES.iter().copied().chain(system.as_deref()).filter_map(|font| inline_cut(font, INLINE_SCALE, INLINE_SUFFIX)).map(std::borrow::Cow::Owned).collect();
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
    let display = chat_name(chat, &custom);
    let mono = custom.mono.clone();
    let tsanger = tsanger_in(&names);
    cx.set_global(ChatFonts { wordmark, kanji, display, inline, cjk: cjk.to_string(), custom, tsanger });
    cut_custom_mono(&mono, cx);
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

/// A face of the system's chosen for a custom place; titles follow the
/// chat font, and a monospace face gets its smaller cut for code among
/// words where one can be made.
pub fn set_custom(custom: Custom, chat: &str, cx: &mut App) {
    let mono = (cx.global::<ChatFonts>().custom.mono != custom.mono).then(|| custom.mono.clone());
    let fonts = cx.global_mut::<ChatFonts>();
    fonts.display = chat_name(chat, &custom);
    fonts.custom = custom;
    if let Some(mono) = mono {
        cut_custom_mono(&mono, cx);
    }
}

/// The smaller cut of a monospace face of the system's (`inline_cut`),
/// from its file, once: a face whose file is not a TrueType font read
/// whole (a collection, a CFF face) has none, and code among words is set
/// in the face itself at the words' size.
fn cut_custom_mono(family: &str, cx: &mut App) {
    let name = format!("{family}{INLINE_SUFFIX}");
    if family.is_empty() || cx.global::<ChatFonts>().inline.contains(&name) {
        return;
    }
    let Some(cut) = system_font_file(family).and_then(|bytes| inline_cut(&bytes, INLINE_SCALE, INLINE_SUFFIX)) else { return };
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
        return custom.chat.clone();
    }
    chat_face(choice).family.to_string()
}

/// The family the terminal and all code are set in, for a choice in the
/// settings.
pub fn term_family(choice: &str, cx: &App) -> SharedString {
    let custom = &cx.global::<ChatFonts>().custom.mono;
    if choice == CUSTOM && !custom.is_empty() {
        return custom.clone().into();
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
}

/// What a list of the system's faces is for: every face, those with
/// CJK characters, or the monospaced ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kinds {
    All,
    Cjk,
    Mono,
}

/// The system's font families, by name, for each kind of list. Names
/// beginning with a dot are the system's own and left out, and so are
/// the cuts made here.
#[derive(Debug, Clone, Default)]
pub struct Families {
    pub all: Vec<String>,
    pub cjk: Vec<String>,
    pub mono: Vec<String>,
}

impl Families {
    pub fn of(&self, kinds: Kinds) -> &[String] {
        match kinds {
            Kinds::All => &self.all,
            Kinds::Cjk => &self.cjk,
            Kinds::Mono => &self.mono,
        }
    }
}

/// The system's font families, read off the main thread: each family's
/// first face asked whether it is monospaced, and whether it has 汉, あ or
/// 한. A face that cannot be read is left out, not the list.
#[cfg(target_os = "macos")]
pub fn system_families() -> Families {
    use core_text::font_descriptor::SymbolicTraitAccessors as _;
    use core_text::font_descriptor::TraitAccessors as _;
    let Some(faces) = std::panic::catch_unwind(|| core_text::font_collection::create_for_all_families().get_descriptors()).ok().flatten() else { return Families::default() };
    let mut seen = std::collections::HashSet::new();
    let mut out = Families::default();
    for face in faces.iter() {
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let family = face.family_name();
            if family.starts_with('.') || family.ends_with(INLINE_SUFFIX) || seen.contains(&family) {
                return None;
            }
            let mono = face.traits().symbolic_traits().is_monospace();
            let font = core_text::font::new_from_descriptor(&face, 12.);
            let cjk = ['汉', 'あ', '한'].iter().any(|c| {
                let mut unit = [0u16; 2];
                let unit = c.encode_utf16(&mut unit);
                let mut glyphs = [0u16; 2];
                unsafe { font.get_glyphs_for_characters(unit.as_ptr(), glyphs.as_mut_ptr(), unit.len() as isize) }
            });
            Some((family, mono, cjk))
        }));
        if let Ok(Some((family, mono, cjk))) = read {
            seen.insert(family.clone());
            if mono {
                out.mono.push(family.clone());
            }
            if cjk {
                out.cjk.push(family.clone());
            }
            out.all.push(family);
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
    let mut all: Vec<String> = names.into_iter().filter(|f| !f.starts_with('.') && !f.ends_with(INLINE_SUFFIX)).collect();
    all.sort_by_cached_key(|f| f.to_lowercase());
    all.dedup();
    Families { cjk: all.clone(), mono: all.clone(), all }
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
    fn a_cut_is_the_face_smaller_under_a_name_of_its_own() {
        let system = std::fs::read("/System/Library/Fonts/Monaco.ttf").ok();
        for font in MONO_FACES.iter().copied().chain(system.as_deref()) {
            let cut = inline_cut(font, INLINE_SCALE, INLINE_SUFFIX).expect("a TrueType face");
            // More units to the em: everything drawn that much smaller.
            let units = |font: &[u8]| u16::from_be_bytes([table(font, b"head")[18], table(font, b"head")[19]]) as f32;
            assert_eq!(units(&cut), (units(font) / INLINE_SCALE).round());
            // Found by a family of its own, never the face's.
            let (was, now) = (families(font), families(&cut));
            assert!(!now.is_empty() && now.iter().all(|name| name.ends_with(INLINE_SUFFIX)), "{now:?}");
            assert!(was.iter().all(|name| !now.contains(name)));
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
}
