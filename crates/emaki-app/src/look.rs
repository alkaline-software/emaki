//! The window's look, as `config.json` asks for it: which appearance to
//! draw (the system's, or light or dark pinned) and which accent colour.
//!
//! The palette itself stays in `themes/emaki.json`, one config per
//! appearance. An accent is a substitution over the handful of keys that
//! carry it there (the primary button, the focus ring, links, the active
//! row's border, the drag border, the selection tint), applied to both
//! configs before they are handed to the toolkit, so `Theme::change`
//! keeps re-applying the chosen accent on every appearance switch with
//! nothing to patch afterwards.

use gpui::{App, Window};
use gpui_component::theme::{Theme, ThemeConfig, ThemeMode, ThemeSet};

/// One accent: its name, and the colour it paints in each appearance
/// (base, hover, active) plus the link colour, which is darker in the light
/// palette and lighter in the dark one so text stays readable.
pub struct Accent {
    pub key: &'static str,
    pub name: &'static str,
    pub light: [&'static str; 3],
    pub dark: [&'static str; 3],
    pub link: [&'static str; 2],
}

/// Seven hues of one lightness and chroma in OKLCH, the first emaki.lol's
/// indigo: 0.52 and 0.13 in the light appearance, 0.74 and 0.105 in the
/// dark, the chroma eased for the hues that are loud at it and amber's
/// light shade a little lighter, where an orange at 0.52 is brown. They
/// go round the wheel in even steps and leave out Claude's red-orange
/// and the other blues (`docs/design.md`, The accents).
pub const ACCENTS: &[Accent] = &[
    Accent { key: "periwinkle", name: "Periwinkle", light: ["#5061B3", "#4A5BAC", "#4050A0"], dark: ["#94A7ED", "#A0B3FB", "#879AE0"], link: ["#4A5BAC", "#94A7ED"] },
    Accent { key: "orchid", name: "Orchid", light: ["#845293", "#7E4C8D", "#724181"], dark: ["#C498D2", "#D1A4DF", "#B78BC4"], link: ["#7E4C8D", "#C498D2"] },
    Accent { key: "rose", name: "Rose", light: ["#9C4A66", "#954360", "#893856"], dark: ["#DA93A9", "#E79FB5", "#CC879C"], link: ["#954360", "#DA93A9"] },
    Accent { key: "amber", name: "Amber", light: ["#A86003", "#A05B00", "#905203"], dark: ["#DD9B5F", "#EAA76B", "#CF8E52"], link: ["#A05B00", "#DD9B5F"] },
    Accent { key: "moss", name: "Moss", light: ["#5A723A", "#556C34", "#4A6129"], dark: ["#9CB67C", "#A7C288", "#8FA86F"], link: ["#556C34", "#9CB67C"] },
    Accent { key: "jade", name: "Jade", light: ["#327860", "#2B725A", "#1E6750"], dark: ["#77BCA1", "#83C8AD", "#6AAE95"], link: ["#2B725A", "#77BCA1"] },
    Accent { key: "teal", name: "Teal", light: ["#277580", "#1F6F79", "#0E646E"], dark: ["#6FB9C4", "#7BC5D0", "#61ACB7"], link: ["#1F6F79", "#6FB9C4"] },
];

/// What is drawn on an accent: white on the light appearance's shade,
/// the ink on the dark one's, each 5 to 1 or more on every accent.
const ON_LIGHT: &str = "#FFFFFF";
const ON_DARK: &str = "#1F1E1D";

/// The accent by its key. A key from before the seven was a colour of
/// its own; the nearest of these stands for it, and the rest (terracotta,
/// graphite, blue) are the first.
pub fn accent(key: &str) -> &'static Accent {
    let key = match key {
        "violet" => "orchid",
        "green" => "jade",
        "brass" => "amber",
        k => k,
    };
    ACCENTS.iter().find(|a| a.key == key).unwrap_or(&ACCENTS[0])
}

/// The accent's base colour in the appearance showing, for a swatch.
pub fn accent_hex(key: &str, dark: bool) -> &'static str {
    let a = accent(key);
    if dark { a.dark[0] } else { a.light[0] }
}

/// The accent installed last, for what stands on the window's frame.
static INSTALLED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The accent as the frame wears it, base and the ink on it. The frame
/// is graphite beside a light page and lighter than a dark one, so it
/// takes the dark appearance's shade in both, as emaki.lol's navbar does:
/// the light one is 2 to 1 on graphite.
pub fn on_frame() -> (&'static str, &'static str) {
    let a = &ACCENTS[INSTALLED.load(std::sync::atomic::Ordering::Relaxed).min(ACCENTS.len() - 1)];
    (a.dark[0], ON_DARK)
}

/// The colours of the agents' own marks, their makers': Claude's
/// terracotta and Codex's green, whatever accent the window wears. Not
/// accents: no window wears them.
pub fn agent_hex(agent: emaki_core::model::AgentId, dark: bool) -> &'static str {
    match agent {
        emaki_core::model::AgentId::ClaudeCode => "#D97757",
        emaki_core::model::AgentId::Codex => {
            if dark {
                "#79B487"
            } else {
                "#5F9E6E"
            }
        }
    }
}

/// The inks code is written in, one set an appearance: every place that
/// colours code or a terminal takes them from here. The editor in the
/// file's pane and a conversation's code blocks name them in
/// `themes/emaki.json` ("@l-blue", "@d-rose"), and the terminals' named
/// colours are these same ones (`term_panel::scheme`).
///
/// The hues are Rosé Pine's (pine, iris, gold, rose, foam, love), with a
/// sage green beside them for a terminal's "added", each taken darker or
/// lighter until it reads on the window's own grounds: 4.5 to 1 or more
/// against the page, and the grey, for comments, a step quieter.
pub struct Inks {
    pub blue: u32,
    pub violet: u32,
    pub gold: u32,
    pub rose: u32,
    pub teal: u32,
    pub green: u32,
    pub red: u32,
    pub grey: u32,
}

pub const INKS_LIGHT: Inks = Inks { blue: 0x286983, violet: 0x7A6398, gold: 0x8A6A1E, rose: 0xA3566F, teal: 0x357783, green: 0x4C7B58, red: 0xB0505F, grey: 0x85818F };
pub const INKS_DARK: Inks = Inks { blue: 0x7FB4D0, violet: 0xC4A7E7, gold: 0xE9C083, rose: 0xE8B0AE, teal: 0x9CCFD8, green: 0xA3CCA4, red: 0xEB8FA6, grey: 0x8F8BA0 };

/// The theme file with each ink's name replaced by its colour.
fn inked(json: &str) -> String {
    let mut out = json.to_string();
    for (p, inks) in [("l", &INKS_LIGHT), ("d", &INKS_DARK)] {
        for (name, c) in [("blue", inks.blue), ("violet", inks.violet), ("gold", inks.gold), ("rose", inks.rose), ("teal", inks.teal), ("green", inks.green), ("red", inks.red), ("grey", inks.grey)] {
            out = out.replace(&format!("\"@{p}-{name}\""), &format!("\"#{c:06X}\""));
        }
    }
    out
}

fn paint(cfg: &mut ThemeConfig, a: &Accent) {
    let dark = cfg.mode.is_dark();
    let [base, hover, active] = if dark { a.dark } else { a.light };
    let link = if dark { a.link[1] } else { a.link[0] };
    let on = if dark { ON_DARK } else { ON_LIGHT };
    let c = &mut cfg.colors;
    c.primary = Some(base.into());
    c.primary_foreground = Some(on.into());
    c.sidebar_primary_foreground = Some(on.into());
    c.primary_hover = Some(hover.into());
    c.primary_active = Some(active.into());
    c.ring = Some(base.into());
    c.link = Some(link.into());
    c.link_hover = Some(active.into());
    c.link_active = Some(active.into());
    c.sidebar_primary = Some(base.into());
    c.list_active_border = Some(base.into());
    c.drag_border = Some(base.into());
    c.progress_bar = Some(base.into());
    c.slider_bar = Some(base.into());
    c.drop_target = Some(format!("{base}33").into());
    c.selection = Some(format!("{base}{}", if dark { "66" } else { "59" }).into());
}

/// Hand the toolkit both palettes with `accent_key` painted in and
/// `mono` (the choice `fonts::term_family` reads) as every monospace
/// face. Called at start and again whenever either changes; `apply` then
/// draws it.
pub fn install(accent_key: &str, mono: &str, cx: &mut App) {
    let set: ThemeSet = match serde_json::from_str(&inked(include_str!("../themes/emaki.json"))) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("emaki: theme file is invalid, using the default look: {e}");
            return;
        }
    };
    let a = accent(accent_key);
    INSTALLED.store(ACCENTS.iter().position(|x| std::ptr::eq(x, a)).unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    let code = crate::fonts::term_family(mono, cx);
    let theme = Theme::global_mut(cx);
    for mut cfg in set.themes {
        paint(&mut cfg, a);
        // The face for code goes into the config, so `Theme::change` keeps
        // it through every appearance switch, as it does the accent.
        cfg.mono_font_family = Some(code.clone());
        let cfg = std::rc::Rc::new(cfg);
        if cfg.mode.is_dark() {
            theme.dark_theme = cfg;
        } else {
            theme.light_theme = cfg;
        }
    }
}

/// Draw the appearance `config.json` asks for: the system's when
/// `appearance` is `system`, otherwise the one named. Called at start, on
/// every system appearance change, and from the settings panel.
pub fn apply(appearance: &str, window: Option<&mut Window>, cx: &mut App) {
    match appearance {
        "light" => Theme::change(ThemeMode::Light, window, cx),
        "dark" => Theme::change(ThemeMode::Dark, window, cx),
        _ => Theme::sync_system_appearance(window, cx),
    }
    // The toolkit draws again only the window it is handed; the main
    // window and the settings window both wear the palette.
    cx.refresh_windows();
}

/// The grammars the toolkit does not carry. R is one: its own crate has
/// the grammar and the queries, and the toolkit's registry takes them
/// under the name `lang_for_path` gives an `.R` file.
pub fn install_languages() {
    use gpui_component::highlighter::{LanguageConfig, LanguageRegistry};
    LanguageRegistry::singleton().register("r", &LanguageConfig::new("r", tree_sitter_r::LANGUAGE.into(), vec![], tree_sitter_r::HIGHLIGHTS_QUERY, "", tree_sitter_r::LOCALS_QUERY));
}
