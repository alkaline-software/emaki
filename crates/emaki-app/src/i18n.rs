//! The window's words in the person's language: the one they chose, or
//! the first of the system's preferred languages that Emaki speaks,
//! else English. Kotoba's way (`../kotoba`, `docs/window.md`), so the
//! two apps speak the same languages and are set the same way.
//!
//! English is the key. `locales/words.json` holds, for each English
//! phrase, its every other language; a phrase not there is drawn in
//! English. The languages are a few of the most used (`LANGS`), so the
//! table stays one file a person can read through.
//!
//! Only the window's own words change. What an agent says, what a
//! transcript holds, what git and the system say, and the names an agent
//! gives its modes and models are theirs and stay as they are.

use std::collections::HashMap;
use std::sync::RwLock;

use gpui::{Div, IntoElement as _, ParentElement as _};

/// The languages Emaki speaks besides English: the code in the table,
/// the toolkit's own code for it, and the name the bundle declares
/// (`CFBundleLocalizations`, so System Settings offers each per app).
pub const LANGS: &[(&str, &str, &str)] = &[
    ("zh-Hans", "zh-CN", "zh-Hans"),
    ("zh-Hant", "zh-TW", "zh-Hant"),
    ("ja", "ja", "ja"),
    ("ko", "ko", "ko"),
    ("fr", "fr", "fr"),
    ("de", "de", "de"),
    ("es", "es", "es"),
];

/// What the window's language may be set to, after the system's: each
/// language by its code, its flag (`assets/icons/flags`) and its
/// letters. Traditional Chinese wears Hong Kong's.
pub const CHOICES: &[(&str, &str, &str)] = &[
    ("en", "icons/flags/us.svg", "EN"),
    ("zh-Hans", "icons/flags/cn.svg", "ZH-CN"),
    ("zh-Hant", "icons/flags/hk.svg", "ZH-HK"),
    ("ja", "icons/flags/jp.svg", "JA"),
    ("ko", "icons/flags/kr.svg", "KO"),
    ("fr", "icons/flags/fr.svg", "FR"),
    ("de", "icons/flags/de.svg", "DE"),
    ("es", "icons/flags/es.svg", "ES"),
];

const TABLE: &str = include_str!("../locales/words.json");

/// The language in use and its words. It changes when the person picks
/// another, so it is read at every phrase; the words are `'static`, kept
/// for the process's life.
static WORDS: RwLock<Option<(&'static str, HashMap<&'static str, &'static str>)>> = RwLock::new(None);

/// The language for a system's preferred languages (BCP 47, as macOS
/// gives them: `zh-Hans-CN`, `zh-HK`, `fr-CA`), in their order: the
/// first Emaki speaks, else English. Chinese is told apart by its
/// script, or where none is given, by its region.
pub fn pick<'a>(preferred: impl IntoIterator<Item = &'a str>) -> &'static str {
    for tag in preferred {
        let tag = tag.replace('_', "-").to_ascii_lowercase();
        let mut parts = tag.split('-');
        let lang = parts.next().unwrap_or("");
        let rest: Vec<&str> = parts.collect();
        let code = match lang {
            "en" => "en",
            "zh" if rest.iter().any(|p| *p == "hant" || *p == "tw" || *p == "hk" || *p == "mo") => "zh-Hant",
            "zh" => "zh-Hans",
            other => match LANGS.iter().find(|(c, _, _)| *c == other) {
                Some((c, _, _)) => c,
                None => continue,
            },
        };
        return code;
    }
    "en"
}

/// The language the system would have: its preferred languages, or
/// `EMAKI_LANG` for a probe.
fn system_lang() -> &'static str {
    match std::env::var("EMAKI_LANG") {
        Ok(code) => pick([code.as_str()]),
        Err(_) => pick(crate::sys::system_languages().iter().map(String::as_str)),
    }
}

/// What the language is set to: `system`, or a code of `CHOICES`. On a
/// Mac the app's own `AppleLanguages` says, which System Settings may
/// have changed since; elsewhere the settings do.
pub fn chosen(saved: &str) -> String {
    if cfg!(target_os = "macos") {
        return crate::sys::own_language().map_or("system".into(), |code| pick([code.as_str()]).to_string());
    }
    if saved == "system" || CHOICES.iter().any(|(c, _, _)| *c == saved) { saved.to_string() } else { "system".into() }
}

/// The window's language set to `choice` (`system` or a code), and the
/// toolkit's with it. A probe's `EMAKI_LANG` stands over a choice.
pub fn set(choice: &str) {
    let lang = if choice == "system" || std::env::var_os("EMAKI_LANG").is_some() { system_lang() } else { pick([choice]) };
    let toolkit = LANGS.iter().find(|(c, _, _)| *c == lang).map_or("en", |(_, t, _)| *t);
    gpui_component::set_locale(toolkit);
    if let Ok(mut words) = WORDS.write() {
        *words = Some((lang, words_for(lang)));
    }
}

/// The language in use, by its code in the table.
pub fn lang() -> &'static str {
    WORDS.read().ok().and_then(|w| w.as_ref().map(|(l, _)| *l)).unwrap_or("en")
}

fn table() -> HashMap<&'static str, HashMap<&'static str, &'static str>> {
    serde_json::from_str(TABLE).expect("locales/words.json is the table")
}

fn words_for(lang: &str) -> HashMap<&'static str, &'static str> {
    if lang == "en" {
        return HashMap::new();
    }
    table().into_iter().filter_map(|(en, all)| all.get(lang).map(|w| (en, *w))).collect()
}

/// A phrase in the language in use: a literal in the code, or a phrase
/// the core hands the window (each such list is named in the test).
pub fn t(en: &str) -> &str {
    WORDS.read().ok().and_then(|w| w.as_ref().and_then(|(_, words)| words.get(en).copied())).unwrap_or(en)
}

/// A phrase with its blanks filled: `{name}` in it, in either language,
/// takes its value.
pub fn tf(en: &str, values: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut s = t(en).to_string();
    for (name, value) in values {
        s = s.replace(&format!("{{{name}}}"), &value.to_string());
    }
    s
}

/// A count with its noun: `one` or `many` in English (`"{n} prompt"`,
/// `"{n} prompts"`), by the language's own rule for which, then put in
/// that language with `{n}` filled.
pub fn count(n: usize, one: &str, many: &str) -> String {
    // French takes the singular for none and one; the others here use
    // one for one alone, or have no plural.
    let single = if lang() == "fr" { n <= 1 } else { n == 1 };
    tf(if single { one } else { many }, &[("n", &n)])
}

/// A phrase kept somewhere other than where it is drawn (a constant, a
/// list of choices), marked for the table and for the test that reads
/// the sources. It is drawn through `t` where it is shown.
pub const fn phrase(en: &'static str) -> &'static str {
    en
}

/// A sentence with things in it that are not words (links), woven from
/// its translation: `{name}` in the sentence takes the thing of that
/// name, so each language puts them where its order wants them.
pub fn weave(sentence: &str, mut things: Vec<(&str, gpui::AnyElement)>, row: Div) -> Div {
    let mut row = row;
    let mut rest = sentence;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|c| open + c) else { break };
        if open > 0 {
            row = row.child(rest[..open].to_string());
        }
        let name = &rest[open + 1..close];
        if let Some(i) = things.iter().position(|(n, _)| *n == name) {
            row = row.child(things.remove(i).1);
        }
        rest = &rest[close + 1..];
    }
    if !rest.is_empty() {
        row = row.child(rest.to_string().into_any_element());
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language's flag is a picture the app carries: a flag not in
    /// `assets::OWN` draws as nothing.
    #[test]
    fn every_flag_is_carried() {
        for (code, flag, _) in CHOICES {
            assert!(crate::assets::OWN.iter().any(|(path, _)| path == flag), "{code}'s flag {flag} is not in assets::OWN");
        }
    }

    #[test]
    fn languages_are_picked_from_the_systems_order() {
        assert_eq!(pick(["zh-Hans-CN"]), "zh-Hans");
        assert_eq!(pick(["zh-Hant-TW"]), "zh-Hant");
        assert_eq!(pick(["zh-HK"]), "zh-Hant");
        assert_eq!(pick(["zh-CN"]), "zh-Hans");
        assert_eq!(pick(["fr-CA"]), "fr");
        assert_eq!(pick(["it-IT", "de-AT"]), "de");
        assert_eq!(pick(["it-IT", "pt-BR"]), "en");
        assert_eq!(pick(["en-GB", "ja-JP"]), "en");
        assert_eq!(pick(["ja_JP"]), "ja");
    }

    /// The blanks of a phrase, `{name}` each, in order of name.
    fn blanks(s: &str) -> Vec<String> {
        let mut b: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string())).collect();
        b.sort();
        b
    }

    /// Every phrase in the table is there in every language, with the
    /// same blanks as its English.
    #[test]
    fn every_phrase_is_in_every_language() {
        for (en, all) in table() {
            for (lang, _, _) in LANGS {
                let w = all.get(lang).unwrap_or_else(|| panic!("{en:?} has no {lang}"));
                assert_eq!(blanks(w), blanks(en), "{en:?} in {lang}: {w:?}");
            }
            assert_eq!(all.len(), LANGS.len(), "{en:?} has a language not in LANGS");
        }
    }

    /// The phrases a source asks for: the literal first argument of each
    /// `t(`, `tf(` and `phrase(`, and both of `count(`'s.
    fn asked_in(src: &str, asked: &mut Vec<String>) {
        for call in ["t", "tf", "count", "phrase"] {
            let opener = format!("{call}(");
            let mut from = 0;
            while let Some(at) = src[from..].find(&opener).map(|i| i + from) {
                from = at + opener.len();
                // A call of its own, not the end of another name.
                if src[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                let rest = &src[from..];
                if call == "count" {
                    // count(n, "one", "many"): the two literals after the
                    // number, which may itself hold calls (`x.len()`).
                    let mut depth = 0;
                    let Some(comma) = rest.char_indices().find_map(|(i, c)| {
                        match c {
                            '(' | '[' => depth += 1,
                            ')' | ']' if depth == 0 => return Some(None),
                            ')' | ']' => depth -= 1,
                            ',' if depth == 0 => return Some(Some(i)),
                            _ => {}
                        }
                        None
                    }).flatten() else { continue };
                    let mut after = rest[comma + 1..].trim_start();
                    for _ in 0..2 {
                        let Some(lit) = after.strip_prefix('"') else { break };
                        let Some(end) = literal_end(lit) else { break };
                        asked.push(unescape(&lit[..end]));
                        after = lit[end + 1..].trim_start().trim_start_matches(',').trim_start();
                    }
                } else if let Some(lit) = rest.strip_prefix('"') {
                    if let Some(end) = literal_end(lit) {
                        asked.push(unescape(&lit[..end]));
                    }
                }
            }
        }
    }

    /// Where a string literal's body ends: the first quote not escaped.
    fn literal_end(s: &str) -> Option<usize> {
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => return Some(i),
                _ => i += 1,
            }
        }
        None
    }

    fn unescape(s: &str) -> String {
        s.replace("\\\"", "\"").replace("\\n", "\n").replace("\\\\", "\\")
    }

    /// Every phrase the app asks for is in the table, and every phrase in
    /// the table is asked for: each `t("…")`, `tf("…"`, `phrase("…")` and
    /// `count(…, "…", "…")` in the window's sources, and the lists of
    /// words the core hands the window.
    #[test]
    fn every_phrase_asked_for_is_in_the_table() {
        let table = table();
        let sources = [
            include_str!("main.rs"),
            include_str!("workbench.rs"),
            include_str!("workbench/settings.rs"),
            include_str!("panels.rs"),
            include_str!("transcript.rs"),
            include_str!("agents_page.rs"),
            include_str!("branches.rs"),
            include_str!("term_panel.rs"),
            include_str!("word.rs"),
            include_str!("pictures.rs"),
            include_str!("spelling.rs"),
            include_str!("html_web.rs"),
            include_str!("html_shot.rs"),
            include_str!("format.rs"),
            include_str!("fonts.rs"),
            include_str!("look.rs"),
            include_str!("sys.rs"),
            include_str!("file_icons.rs"),
            include_str!("hub.rs"),
        ];
        let mut asked: Vec<String> = Vec::new();
        for src in sources {
            asked_in(src, &mut asked);
        }
        asked.extend(core_phrases());
        let missing: Vec<&String> = asked.iter().filter(|p| !p.is_empty() && !table.contains_key(p.as_str())).collect();
        assert!(missing.is_empty(), "not in locales/words.json: {missing:#?}");
        let unused: Vec<&&str> = table.keys().filter(|k| !asked.iter().any(|a| a == *k)).collect();
        assert!(unused.is_empty(), "in locales/words.json but never asked for: {unused:#?}");
    }

    /// The core's words the window draws through `t`, by the lists that
    /// hold them.
    fn core_phrases() -> Vec<String> {
        use emaki_core::{agents, format, git, model};
        let mut all: Vec<String> = Vec::new();
        // A setting's notice ("Effort High"), and the compaction's.
        all.extend([model::NoticeVariant::Mode, model::NoticeVariant::Model, model::NoticeVariant::Effort].iter().filter_map(|v| v.setting()).map(str::to_string));
        all.push(model::COMPACTED.to_string());
        all.push(emaki_core::driver::DEFAULT_MODEL.to_string());
        // The files strip's two troubles that are ours, not git's.
        all.push(git::Trouble::license().words);
        all.push(git::NO_GIT.to_string());
        // The agents page: what each agent and tool is, how one signs in,
        // its plans, its maker, and each way to install it by its name.
        for a in agents::all().iter().chain(agents::tools()) {
            all.extend([a.about, a.sign_in_how, a.plans, a.maker].iter().filter(|s| !s.is_empty()).map(|s| s.to_string()));
            all.extend(a.install.iter().map(|w| w.by.split(" (").next().unwrap_or(w.by).to_string()));
        }
        // The spelling languages' names, on the hover of each.
        all.extend(emaki_core::check::LANGUAGES.iter().map(|(_, name)| name.to_string()));
        // The formatters' languages, in Settings, Files.
        all.extend(format::LANGS.iter().map(|l| l.name.to_string()));
        all
    }
}
