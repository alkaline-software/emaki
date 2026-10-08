//! The window's look, as `config.json` asks for it: which appearance to
//! draw (the system's, or light or dark pinned) and which accent colour.
//!
//! The palette itself stays in `themes/emaki.json`, one config per
//! appearance. An accent is a substitution over the handful of keys that
//! carry the terracotta there (the primary button, the focus ring, links,
//! the active row's border, the drag border, the selection tint), applied
//! to both configs before they are handed to the toolkit, so
//! `Theme::change` keeps re-applying the chosen accent on every appearance
//! switch with nothing to patch afterwards.

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

pub const ACCENTS: &[Accent] = &[
    Accent { key: "terracotta", name: "Terracotta", light: ["#D97757", "#C96A4B", "#B85F3F"], dark: ["#D97757", "#E0866A", "#B85F3F"], link: ["#B85F3F", "#E39A7F"] },
    Accent { key: "blue", name: "Blue", light: ["#5B7FB8", "#4F72AA", "#43649A"], dark: ["#7A9BD1", "#8FAADB", "#6386BE"], link: ["#4A6CA8", "#9DB6E0"] },
    Accent { key: "green", name: "Green", light: ["#5F9E6E", "#549162", "#4A8257"], dark: ["#79B487", "#8CC099", "#66A074"], link: ["#4B8A5A", "#9BCBA7"] },
    Accent { key: "violet", name: "Violet", light: ["#9B6FB5", "#8D62A7", "#7E5697"], dark: ["#B08AC8", "#BD9CD2", "#9C77B7"], link: ["#8A5EA6", "#C4A5D8"] },
    Accent { key: "teal", name: "Teal", light: ["#4E9AA6", "#458C97", "#3D7D87"], dark: ["#6FB5BF", "#85C1CA", "#5EA3AD"], link: ["#3F8791", "#93CBD3"] },
    Accent { key: "graphite", name: "Graphite", light: ["#5C5951", "#4E4B44", "#403E38"], dark: ["#A8A59C", "#B6B3AB", "#97948B"], link: ["#4E4B44", "#C2BFB6"] },
];

pub fn accent(key: &str) -> &'static Accent {
    ACCENTS.iter().find(|a| a.key == key).unwrap_or(&ACCENTS[0])
}

/// The accent's base colour in the appearance showing, for a swatch.
pub fn accent_hex(key: &str, dark: bool) -> &'static str {
    let a = accent(key);
    if dark { a.dark[0] } else { a.light[0] }
}

fn paint(cfg: &mut ThemeConfig, a: &Accent) {
    let dark = cfg.mode.is_dark();
    let [base, hover, active] = if dark { a.dark } else { a.light };
    let link = if dark { a.link[1] } else { a.link[0] };
    let c = &mut cfg.colors;
    c.primary = Some(base.into());
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

/// Hand the toolkit both palettes with `accent_key` painted in. Called at
/// start and again whenever the accent changes; `apply` then draws it.
pub fn install(accent_key: &str, cx: &mut App) {
    let set: ThemeSet = match serde_json::from_str(include_str!("../themes/emaki.json")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("emaki: theme file is invalid, using the default look: {e}");
            return;
        }
    };
    let a = accent(accent_key);
    let code = crate::fonts::code_family(cx);
    let theme = Theme::global_mut(cx);
    for mut cfg in set.themes {
        paint(&mut cfg, a);
        // The face for code goes into the config, so `Theme::change` keeps
        // it through every appearance switch, as it does the accent.
        if code.is_some() {
            cfg.mono_font_family = code.clone();
        }
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
}

/// The grammars the toolkit does not carry. R is one: its own crate has
/// the grammar and the queries, and the toolkit's registry takes them
/// under the name `lang_for_path` gives an `.R` file.
pub fn install_languages() {
    use gpui_component::highlighter::{LanguageConfig, LanguageRegistry};
    LanguageRegistry::singleton().register("r", &LanguageConfig::new("r", tree_sitter_r::LANGUAGE.into(), vec![], tree_sitter_r::HIGHLIGHTS_QUERY, "", tree_sitter_r::LOCALS_QUERY));
}
