//! The files panel's icons: Catppuccin's, as its VS Code icon theme
//! chooses them.
//!
//! `assets/catppuccin/` holds the theme's icons in two flavors (Latte
//! for a light window, Mocha for a dark one) and `theme.json`, its table
//! of which icon a name gets, both copied from the extension
//! (`catppuccin.catppuccin-vsc-icons` 1.26.0, MIT, `LICENSE` beside
//! them). Some are ours, drawn in the theme's hand for kinds it has no
//! icon for, and their extensions added to the table: `quarto.svg`,
//! Quarto's circle of four quarters with a light fill, in Markdown's
//! sapphire since a `.qmd` is markdown and more (the theme's issue 579
//! asks for one); `parquet`, `arrow` (Arrow and Feather), `calendar`,
//! `email`, `ebook`, `subtitles`, `lock`, `package` (a Linux or Android
//! package), `disc` (a disk image) and `contact`; the coding agents'
//! marks, for their files and folders, in the colors the window gives
//! the agents (`look::agent_hex`): `claude`, Anthropic's A and backslash
//! in lines, for `CLAUDE.md`; `codex`, OpenAI's knot; `gemini`; and
//! `agents`, the Agents page's sparkles, for `AGENTS.md`;
//! and folders with a mark at their lower right as the theme's have:
//! `folder_quarto` (`.quarto`, `_freeze`, `_extensions`), `folder_claude`,
//! `folder_codex`, `folder_gemini`, `folder_agents` and `folder_r` (`R`,
//! `renv`). Kinds one of the theme's own icons fits are added under that
//! icon (`.rds` under R's data, a DuckDB file under the database, `target`
//! under the folder of what is built). One of the theme's is changed:
//! `folder_github`'s mark is GitHub's orchid, as the Agents page wears
//! it, where the theme's is gray.
//! The icons carry their own colors, so they are drawn as
//! pictures (`img`), not as the one-color masks the window's other
//! icons are.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

use gpui::SharedString;

#[derive(rust_embed::RustEmbed)]
#[folder = "assets/catppuccin"]
struct Files;

/// Where the icons are served from, by `assets::Assets`.
pub const PREFIX: &str = "catppuccin/";

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Table {
    file: String,
    folder: String,
    folder_expanded: String,
    file_extensions: HashMap<String, String>,
    file_names: HashMap<String, String>,
    folder_names: HashMap<String, String>,
    folder_names_expanded: HashMap<String, String>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| {
    let bytes = Files::get("theme.json").expect("the icon table is built in");
    serde_json::from_slice(&bytes.data).expect("the icon table reads")
});

/// A file under `catppuccin/`, for the asset source.
pub fn load(path: &str) -> Option<Cow<'static, [u8]>> {
    Files::get(path.strip_prefix(PREFIX)?).map(|f| f.data)
}

fn named<'a>(table: &'a HashMap<String, String>, name: &str) -> Option<&'a String> {
    table.get(name).or_else(|| table.get(&name.to_lowercase()))
}

/// The icon's id for a file of this name: the whole name first, then
/// its extensions from the longest ("d.ts" before "ts").
fn file_id(name: &str) -> &'static str {
    let t = &*TABLE;
    if let Some(id) = named(&t.file_names, name) {
        return id;
    }
    let mut rest = name;
    while let Some((_, ext)) = rest.split_once('.') {
        if let Some(id) = named(&t.file_extensions, ext) {
            return id;
        }
        rest = ext;
    }
    &t.file
}

fn folder_id(name: &str, open: bool) -> &'static str {
    let t = &*TABLE;
    let (names, plain) = if open { (&t.folder_names_expanded, &t.folder_expanded) } else { (&t.folder_names, &t.folder) };
    named(names, name).unwrap_or(plain)
}

/// The asset path of the icon a row of the tree wears.
pub fn path(name: &str, dir: bool, open: bool, dark: bool) -> SharedString {
    let id = if dir { folder_id(name, open) } else { file_id(name) };
    format!("{PREFIX}{}/{id}.svg", if dark { "mocha" } else { "latte" }).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_get_the_icons_the_theme_gives_them() {
        assert_eq!(file_id("main.rs"), "rust");
        assert_eq!(file_id("Cargo.toml"), "cargo");
        assert_eq!(file_id("Cargo.lock"), "cargo-lock");
        assert_eq!(file_id("README.md"), "readme");
        assert_eq!(file_id("notes.md"), "markdown");
        assert_eq!(file_id("LICENSE"), "license");
        assert_eq!(file_id("no-such.zzzz"), "_file");
        // Ours, for what the theme has no icon for, and the theme's own
        // for kinds it does not name.
        assert_eq!(file_id("works.qmd"), "quarto");
        assert_eq!(file_id("trips.parquet"), "parquet");
        assert_eq!(file_id("Emaki.AppImage"), "package");
        assert_eq!(file_id("renv.lock"), "lock");
        assert_eq!(file_id("model.rds"), "rdata");
        assert_eq!(file_id("CLAUDE.md"), "claude");
        assert_eq!(file_id("AGENTS.md"), "agents");
        // YAML is YAML's whatever it is for, Quarto's own config too.
        assert_eq!(file_id("_quarto.yml"), "yaml");
        assert_eq!(folder_id(".claude", false), "folder_claude");
        assert_eq!(folder_id(".codex", true), "folder_codex_open");
        assert_eq!(folder_id(".quarto", false), "folder_quarto");
        assert_eq!(folder_id("R", false), "folder_r");
        assert_eq!(folder_id("target", false), "folder_dist");
        assert_eq!(folder_id("scripts", false), "folder_scripts");
        assert_eq!(folder_id("scripts", true), "folder_scripts_open");
        assert_eq!(folder_id("whatever", false), "_folder");
        // Every icon the table names is there, in both flavors.
        let t = &*TABLE;
        let ids = t.file_extensions.values().chain(t.file_names.values()).chain(t.folder_names.values()).chain(t.folder_names_expanded.values()).chain([&t.file, &t.folder, &t.folder_expanded]);
        for id in ids {
            for flavor in ["latte", "mocha"] {
                assert!(load(&format!("{PREFIX}{flavor}/{id}.svg")).is_some(), "{flavor}/{id}");
            }
        }
    }
}
