//! The pairs a language's editor closes as an open is typed, and puts
//! around a selection, as VS Code's language configurations have them
//! (`docs/panels.md`, the file's editor).

fn pairs(s: &str) -> Vec<(char, char)> {
    s.chars().collect::<Vec<_>>().chunks(2).map(|p| (p[0], p[1])).collect()
}

/// For `lang` as `render_md::lang_for_path` names it: the pairs an open
/// typed closes, and the pairs an open typed over a selection goes
/// around it. Markdown closes no quote, where an apostrophe is most of
/// them, and surrounds with its own marks; the rest as VS Code's
/// configurations for each, and plain text's for a language with none.
pub fn for_lang(lang: &str) -> (Vec<(char, char)>, Vec<(char, char)>) {
    match lang {
        "markdown" | "md" | "qmd" | "rmd" => (pairs("{}[]()<>"), pairs("()[]``__**{}''\"\"<>~~$$")),
        "rust" | "rs" => (pairs("{}[]()\"\""), pairs("{}[]()\"\"<>")),
        "json" => (pairs("{}[]\"\""), pairs("{}[]\"\"")),
        "javascript" | "js" | "mjs" | "jsx" | "typescript" | "ts" | "tsx" | "astro" | "svelte" => (pairs("{}[]()\"\"''``"), pairs("{}[]()\"\"''``<>")),
        "html" => (pairs("{}[]()\"\"''"), pairs("{}[]()\"\"''<>")),
        "bash" | "sh" | "zsh" | "go" | "r" | "sql" | "elixir" | "ex" | "exs" | "ruby" | "rb" | "php" => (pairs("{}[]()\"\"''``"), pairs("{}[]()\"\"''``")),
        _ => (pairs("{}[]()\"\"''"), pairs("{}[]()\"\"''``<>")),
    }
}
