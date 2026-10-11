//! The window's typefaces, every one built in (`assets/fonts`): four
//! serifs and four sans serifs for a conversation, four monospace faces
//! for code and the terminal (three built in, and the Mac's Monaco from
//! the system). Each built in is under the SIL Open Font License,
//! which lets an app carry it, with its licence beside its files. Nothing
//! is loaded from another app or fetched: the Claude desktop app's own
//! faces were used when that app was installed, and are not, being
//! Anthropic's and not ours to use.
//!
//! A face is a file a weight (regular, semibold, bold and an italic; the
//! monospace ones a medium too), which the text system takes as bytes on
//! every platform. A variable font handed over as bytes is held as its
//! regular alone, and every bold drawn as regular.

use gpui::{App, Global, SharedString};

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

/// The monospace faces this system offers.
pub fn monos() -> &'static [Face] {
    MONOS
}

/// The faces the name in the sidebar is tried in, most wanted first: a
/// light, elegant sans (Optima's flared strokes, else Avenir Next; both
/// come with macOS and are the system's, not carried here). With none of
/// them, the window's face.
const WORDMARK_FACES: &[&str] = &["Optima", "Avenir Next", "Avenir", WORDMARK_OWN];

/// The face of that kind the window carries, for a system with none of
/// the Mac's: Belleza (etunni/belleza), a sans with Optima's flared
/// strokes, under the SIL Open Font License.
const WORDMARK_OWN: &str = "Belleza";

/// What the terminal falls back on for the letters its face lacks:
/// Kaku's own list. The symbols are built in.
pub const TERM_FALLBACKS: &[&str] = &["PingFang SC", "Apple Symbols", "Apple Color Emoji", "Symbols Nerd Font Mono"];

const FACES: &[&[u8]] = &[
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
    /// The family titles and the greeting are set in: the chat's.
    pub display: String,
}

impl Global for ChatFonts {}

/// Hand the built-in faces to the text system. `chat` is the chat font
/// `config.json` asks for, which titles follow.
pub fn install(chat: &str, cx: &mut App) {
    let _ = cx.text_system().add_fonts(FACES.iter().map(|b| std::borrow::Cow::Borrowed(*b)).collect());
    let names = cx.text_system().all_font_names();
    // `EMAKI_WORDMARK=<family>` draws the name in that face, for a look
    // at the one Windows and Linux get.
    let asked = std::env::var("EMAKI_WORDMARK").ok();
    let wordmark = asked.or_else(|| WORDMARK_FACES.iter().find(|f| names.iter().any(|n| n == *f)).map(|f| f.to_string()));
    cx.set_global(ChatFonts { wordmark, display: chat_face(chat).family.to_string() });
}

/// The chat font changed: titles go with it.
pub fn set_chat(chat: &str, cx: &mut App) {
    cx.global_mut::<ChatFonts>().display = chat_face(chat).family.to_string();
}

/// The family the terminal and all code are set in, for a choice in the
/// settings.
pub fn term_family(choice: &str, _: &App) -> SharedString {
    mono_face(choice).family.into()
}

/// The family the app's name is set in, in the sidebar, or None to leave
/// the window's own face.
pub fn wordmark_family(cx: &App) -> Option<SharedString> {
    cx.global::<ChatFonts>().wordmark.clone().map(SharedString::from)
}

/// The face titles and the greeting are set in: the conversation's, so a
/// page title and the conversation agree.
pub fn display_family(cx: &App) -> SharedString {
    SharedString::from(cx.global::<ChatFonts>().display.clone())
}

/// The family a conversation is drawn in for `choice`.
pub fn chat_family(choice: &str, _: &App) -> Option<SharedString> {
    Some(chat_face(choice).family.into())
}
