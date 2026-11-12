//! A code block's language as a mark: its icon, its name and its color,
//! the set emaki and pingfan.org share (`assets/icons/lang/`, Tabler's
//! hand with R's, Python's and Quarto's own drawn to match, each in the
//! color its language is known by). A language with no mark of its own
//! gets the plain code mark in gray and its word with a capital.

/// How a language is shown at a code block's head.
pub struct Lang {
    /// The icon's path among the assets.
    pub icon: &'static str,
    pub name: String,
    /// The language's color, as its icon draws it.
    pub rgb: u32,
}

/// The mark for a fence's language word, as written after the fence
/// (`python`, `py`, `{r}`), case folded; a Quarto chunk's braces and
/// options are dropped.
pub fn of(word: &str) -> Lang {
    let inner = word.trim().strip_prefix('{').map(|w| w.trim_end_matches('}')).unwrap_or(word.trim());
    let word = inner.trim_start_matches('=').split(|c: char| c == ',' || c.is_whitespace()).next().unwrap_or("").trim().to_ascii_lowercase();
    let (icon, name, rgb) = match word.as_str() {
        "r" => ("r", "R", 0x276dc3),
        "python" | "py" => ("python", "Python", 0x3776ab),
        "julia" | "jl" => ("julia", "Julia", 0x9558b2),
        "ojs" | "js" | "javascript" | "node" | "jsx" | "ts" | "typescript" | "tsx" => ("javascript", if word.starts_with('t') { "TypeScript" } else { "JavaScript" }, 0xdeb800),
        "mermaid" => ("mermaid", "Mermaid", 0xe64678),
        "dot" | "graphviz" => ("graphviz", "Graphviz", 0x276dc3),
        "bash" | "sh" | "zsh" | "shell" | "fish" | "console" => ("bash", "Bash", 0x4eaa25),
        "sql" | "postgres" | "postgresql" | "sqlite" | "mysql" => ("sql", "SQL", 0x4169e1),
        "html" | "xml" | "svg" => ("html", if word == "html" { "HTML" } else { "XML" }, 0xe34f26),
        "css" | "scss" | "sass" | "less" => ("css", "CSS", 0x1572b6),
        "latex" | "tex" => ("latex", "LaTeX", 0x008080),
        "typst" => ("typst", "Typst", 0x239dad),
        "stata" => ("stata", "Stata", 0x1a5dad),
        "rust" | "rs" => ("rust", "Rust", 0xce422b),
        "c" | "h" => ("c", "C", 0x5569b4),
        "cpp" | "c++" | "cc" | "hpp" | "rcpp" => ("cpp", "C++", 0x00599c),
        "go" | "golang" => ("go", "Go", 0x00add8),
        "java" | "kotlin" | "kt" => ("java", if word == "java" { "Java" } else { "Kotlin" }, 0xb07219),
        "swift" => ("swift", "Swift", 0xf05138),
        "md" | "markdown" | "asis" => ("markdown", "Markdown", 0x787c84),
        "quarto" | "qmd" => ("quarto", "Quarto", 0x75aadb),
        "" => ("code", "", 0x787c84),
        _ => {
            let mut chars = word.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
            return Lang { icon: "icons/lang/code.svg", name: format!("{first}{}", chars.as_str()), rgb: 0x787c84 };
        }
    };
    Lang { icon: icon_path(icon), name: name.to_string(), rgb }
}

/// The asset path of a mark, by its file's stem. Each is named here in
/// full, so the icon test sees every one the window can ask for.
fn icon_path(stem: &str) -> &'static str {
    match stem {
        "r" => "icons/lang/r.svg",
        "python" => "icons/lang/python.svg",
        "julia" => "icons/lang/julia.svg",
        "javascript" => "icons/lang/javascript.svg",
        "mermaid" => "icons/lang/mermaid.svg",
        "graphviz" => "icons/lang/graphviz.svg",
        "bash" => "icons/lang/bash.svg",
        "sql" => "icons/lang/sql.svg",
        "html" => "icons/lang/html.svg",
        "css" => "icons/lang/css.svg",
        "latex" => "icons/lang/latex.svg",
        "typst" => "icons/lang/typst.svg",
        "stata" => "icons/lang/stata.svg",
        "rust" => "icons/lang/rust.svg",
        "c" => "icons/lang/c.svg",
        "cpp" => "icons/lang/cpp.svg",
        "go" => "icons/lang/go.svg",
        "java" => "icons/lang/java.svg",
        "swift" => "icons/lang/swift.svg",
        "markdown" => "icons/lang/markdown.svg",
        "quarto" => "icons/lang/quarto.svg",
        _ => "icons/lang/code.svg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_is_known_by_its_fence_word_or_its_chunk() {
        assert_eq!(of("python").name, "Python");
        assert_eq!(of("py").icon, "icons/lang/python.svg");
        assert_eq!(of("{r setup, echo=FALSE}").name, "R");
        assert_eq!(of("Bash").rgb, 0x4eaa25);
        assert_eq!(of("ts").name, "TypeScript");
        let other = of("haskell");
        assert_eq!((other.name.as_str(), other.icon), ("Haskell", "icons/lang/code.svg"));
        assert_eq!(of("").name, "");
    }
}
