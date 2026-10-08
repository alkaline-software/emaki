//! Writing in the composer: what is misspelt or ungrammatical, and the
//! capital a sentence starts with.
//!
//! English is Harper's (`harper-core`), which runs here and sends nothing
//! anywhere. The other languages have their spelling checked by the
//! system, which is the window's to ask (`sys::spelling`); this file says
//! which words of a message are prose at all, so both leave code, paths
//! and commands alone.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use harper_core::linting::{LintGroup, LintKind, Suggestion};
use harper_core::parsers::PlainEnglish;
use harper_core::spell::FstDictionary;
use harper_core::{Dialect, Document};

/// The languages on offer: the key kept in the settings, and its name.
pub const LANGUAGES: &[(&str, &str)] = &[("en-US", "English (US)"), ("en-GB", "English (UK)"), ("es", "Spanish"), ("fr", "French"), ("de", "German")];

pub fn is_english(language: &str) -> bool {
    language.starts_with("en")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Spelling,
    Grammar,
}

/// One thing marked: where (bytes of the text), what it is, what is
/// wrong in a sentence, and what could stand there instead.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub range: Range<usize>,
    pub kind: Kind,
    pub message: String,
    pub fixes: Vec<String>,
    /// Harper's name for the rule that marked it; empty for the
    /// system's spelling.
    pub rule: String,
}

struct English {
    dictionary: Arc<FstDictionary>,
    american: LintGroup,
    british: LintGroup,
}

/// Harper's rules that are left off: the ones that guess. A mark says
/// "this is wrong", so a rule earns one only when what it marks is wrong
/// however the sentence is read. These are not: they guess which part of
/// speech a word is ("the effect triggers" read as a verb wanted), find
/// a word missing that the sentence does without, or prefer one accepted
/// way of writing over another (a comma, a dash, a title's capitals).
/// With them, `settled` drops what only joins or splits words.
const GUESSES: &[&str] = &[
    "NounVerbConfusion", "VerbToAdjective", "NeedToNoun", "ForNoun", "NominalWants", "ComplainAsNoun", "ThieveNoun", "LayoutVerb", "ShutdownVerb", "ThreatenVerb", "PartsOfSpeech",
    "MissingDeterminer", "DefiniteArticle", "MassNouns", "MissingPreposition", "MissingTo", "SomeWithoutArticle",
    "OxfordComma", "NoOxfordComma", "CommaFixes", "Dashes", "Spaces", "EllipsisLength", "UseEllipsisCharacter", "NoFrenchSpaces", "QuoteSpacing", "UnclosedQuotes", "UseTitleCase", "NumericRangeEnDash", "CurrencyPlacement",
    "Hedging", "FillerWords", "DiscourseMarkers", "BoringWords", "LongSentences", "SpelledNumbers", "AvoidContractions", "AvoidCurses",
];

fn curated(dictionary: &Arc<FstDictionary>, dialect: Dialect) -> LintGroup {
    let mut group = LintGroup::new_curated(dictionary.clone(), dialect);
    for rule in GUESSES {
        group.config.set_rule_enabled(rule, false);
    }
    group
}

fn english_checker() -> &'static Mutex<English> {
    static CHECKER: OnceLock<Mutex<English>> = OnceLock::new();
    CHECKER.get_or_init(|| {
        let dictionary = FstDictionary::curated();
        Mutex::new(English { american: curated(&dictionary, Dialect::American), british: curated(&dictionary, Dialect::British), dictionary })
    })
}

/// Whether a correction only joins or splits the words it replaces:
/// "file system" to "filesystem", "setup" to "set up". Both are written,
/// so it is a preference and no mistake; a word that is wrong joined
/// ("alot") is a misspelling, and marked as one.
fn only_joins(was: &str, fix: &str) -> bool {
    let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace() && *c != '-').flat_map(char::to_lowercase).collect::<String>();
    was != fix && bare(was) == bare(fix)
}

/// What Harper marks in an English message. Advice on style is left out:
/// a prompt is not an essay. Building the checker takes a moment the
/// first time, so call this off the main thread.
pub fn english(text: &str, british: bool) -> Vec<Issue> {
    let Ok(mut checker) = english_checker().lock() else { return Vec::new() };
    let document = Document::new(text, &PlainEnglish, &checker.dictionary);
    let by_rule = if british { checker.british.organized_lints(&document) } else { checker.american.organized_lints(&document) };
    drop(checker);
    // Harper counts characters; the window counts bytes.
    let mut at: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    at.push(text.len());
    let mut issues = Vec::new();
    for (rule, lint) in by_rule.into_iter().flat_map(|(rule, lints)| lints.into_iter().map(move |lint| (rule.clone(), lint))) {
        let kind = match lint.lint_kind {
            LintKind::Spelling | LintKind::Typo => Kind::Spelling,
            LintKind::Enhancement | LintKind::Formatting | LintKind::Readability | LintKind::Style | LintKind::Regionalism => continue,
            _ => Kind::Grammar,
        };
        let (Some(&start), Some(&end)) = (at.get(lint.span.start), at.get(lint.span.end)) else { continue };
        if start >= end {
            continue;
        }
        let was = &text[start..end];
        let mut fixes: Vec<String> = Vec::new();
        for suggestion in &lint.suggestions {
            let fix = match suggestion {
                Suggestion::ReplaceWith(chars) => chars.iter().collect(),
                Suggestion::InsertAfter(chars) => format!("{was}{}", chars.iter().collect::<String>()),
                Suggestion::Remove => String::new(),
            };
            if fix != was && !fixes.contains(&fix) {
                fixes.push(fix);
            }
        }
        // The window offers corrections and explains nothing, so a mark
        // with nothing to offer would be a mark with no way out.
        if kind == Kind::Grammar && (fixes.is_empty() || fixes.iter().all(|fix| only_joins(was, fix))) {
            continue;
        }
        issues.push(Issue { range: start..end, kind, message: lint.message, fixes, rule });
    }
    issues
}

/// The parts of a message that are not prose: code between backticks,
/// fenced blocks, and every word that is a path, a command, a link, a
/// name out of code or a number. Sorted, not overlapping.
pub fn not_prose(text: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    // Fenced blocks, then spans on one line.
    let mut fence: Option<usize> = None;
    let mut line_at = 0;
    for line in text.split_inclusive('\n') {
        let is_fence = line.trim_start().starts_with("```");
        match (fence, is_fence) {
            (None, true) => fence = Some(line_at),
            (Some(start), true) => {
                out.push(start..line_at + line.len());
                fence = None;
            }
            (None, false) => {
                let mut open: Option<usize> = None;
                for (i, c) in line.char_indices() {
                    if c == '`' {
                        match open.take() {
                            Some(start) => out.push(line_at + start..line_at + i + 1),
                            None => open = Some(i),
                        }
                    }
                }
            }
            (Some(_), false) => {}
        }
        line_at += line.len();
    }
    if let Some(start) = fence {
        out.push(start..text.len());
    }
    // Words that are not words.
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let c = rest.chars().next().unwrap_or(' ');
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let end = i + rest.find(char::is_whitespace).unwrap_or(rest.len());
        if !is_word(&text[i..end]) {
            out.push(i..end);
        }
        i = end;
    }
    out.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for r in out {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

/// Whether a run of non-space characters is a word of a sentence, with
/// whatever punctuation stands round it.
fn is_word(token: &str) -> bool {
    let core = token.trim_matches(|c: char| !c.is_alphanumeric());
    if core.is_empty() {
        return true;
    }
    // A path, a command, a mention, a variable, a flag.
    let lead = &token[..token.find(char::is_alphanumeric).unwrap_or(0)];
    if lead.contains(['/', '@', '#', '$', '~', '<', '\\']) || lead.ends_with('-') {
        return false;
    }
    if core.contains(['/', '\\', '_', '@', '=', ':', '<', '>', '(', '[', '{', '|', '&', '*', '+', '#']) || core.contains("..") {
        return false;
    }
    if core.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    // "main.rs", "foo.bar()": a dot inside, with no space after it.
    if core.contains('.') {
        return false;
    }
    // "HashMap", "camelCase", "API": a capital after the first letter.
    let mut letters = core.chars();
    letters.next();
    !letters.any(|c| c.is_uppercase())
}

/// The issues that are about prose, less the words the person has
/// taught, one to a place.
pub fn keep(text: &str, mut issues: Vec<Issue>, learned: &HashSet<String>, ignored: &HashSet<String>) -> Vec<Issue> {
    let skip = not_prose(text);
    issues.retain(|issue| {
        if skip.iter().any(|r| issue.range.start < r.end && r.start < issue.range.end) {
            return false;
        }
        let words = &text[issue.range.clone()];
        match issue.kind {
            Kind::Spelling => !learned.contains(&words.to_lowercase()),
            Kind::Grammar => !ignored.contains(&ignore_key(&issue.rule, words)),
        }
    });
    // Grammar first where two start together: it knows more of the
    // sentence than the dictionary does.
    issues.sort_by_key(|issue| (issue.range.start, issue.kind == Kind::Spelling, issue.range.end));
    let mut out: Vec<Issue> = Vec::new();
    for issue in issues {
        match out.last_mut() {
            Some(last) if issue.range.start < last.range.end => {
                if issue.range == last.range {
                    for fix in issue.fixes {
                        if !last.fixes.contains(&fix) {
                            last.fixes.push(fix);
                        }
                    }
                }
            }
            _ => out.push(issue),
        }
    }
    out
}

/// The issues carried over a change of the text, until it is checked
/// again: those before the change stay, those after it move with the
/// text, and those it touched go.
pub fn carry(old: &str, new: &str, issues: &[Issue]) -> Vec<Issue> {
    let (start, old_end, new_end) = changed(old, new);
    issues
        .iter()
        .filter_map(|issue| {
            if issue.range.end < start {
                Some(issue.clone())
            } else if issue.range.start > old_end {
                let mut moved = issue.clone();
                moved.range = issue.range.start + new_end - old_end..issue.range.end + new_end - old_end;
                Some(moved)
            } else {
                None
            }
        })
        .collect()
}

/// Where two texts differ: the start, and the end in each.
fn changed(old: &str, new: &str) -> (usize, usize, usize) {
    let mut start = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let mut tail = old[start..].bytes().rev().zip(new[start..].bytes().rev()).take_while(|(a, b)| a == b).count();
    while !old.is_char_boundary(old.len() - tail) || !new.is_char_boundary(new.len() - tail) {
        tail -= 1;
    }
    (start, old.len() - tail, new.len() - tail)
}

/// What one keystroke did to the text, as far as a capital cares.
#[derive(Debug, PartialEq)]
pub enum Typed {
    /// One character went in, ending at this byte.
    In(char, usize),
    /// One character came out, at this byte.
    Out(usize),
    Other,
}

pub fn typed(old: &str, new: &str, caret: usize) -> Typed {
    let (start, old_end, new_end) = changed(old, new);
    let (gone, came) = (&old[start..old_end], &new[start..new_end]);
    // A letter typed beside its twin ("l" into "hello") differs further
    // on than where it went; the caret says where that was.
    let twins = |text: &str, c: char| text.get(caret..).is_some_and(|rest| rest.len() >= start.saturating_sub(caret) && caret <= start && text[caret..start].chars().all(|x| x == c));
    let (mut c_in, mut c_out) = (came.chars(), gone.chars());
    match (c_out.next(), c_in.next()) {
        (None, Some(c)) if c_in.next().is_none() && caret >= c.len_utf8() && new.is_char_boundary(caret) && new[caret..new_end].chars().all(|x| x == c) && new[..caret].ends_with(c) => Typed::In(c, caret),
        (Some(c), None) if c_out.next().is_none() && twins(new, c) => Typed::Out(caret),
        _ => Typed::Other,
    }
}

/// The capital a keystroke calls for: the character just typed at
/// `caret` is the first letter of a sentence, or, in English, ends a
/// lone "i". The answer is the bytes to replace and with what.
pub fn capital(text: &str, caret: usize, english: bool) -> Option<(Range<usize>, String)> {
    let before = text.get(..caret)?;
    let c = before.chars().next_back()?;
    let at = caret - c.len_utf8();
    if in_code(&text[..at]) {
        return None;
    }
    if c.is_lowercase() {
        let upper: String = c.to_uppercase().collect();
        // A letter with no capital of its own, or one that is two.
        if upper.chars().count() != 1 || upper.starts_with(c) {
            return None;
        }
        return starts_sentence(&text[..at]).then_some((at..caret, upper));
    }
    if english && matches!(c, ' ' | '\n' | '\'' | '’' | ',' | '!' | '?') {
        let head = &text[..at];
        let lone = head.ends_with('i') && head[..head.len() - 1].chars().next_back().is_none_or(|p| p.is_whitespace() || matches!(p, '(' | '"' | '“'));
        return lone.then(|| (at - 1..at, "I".to_string()));
    }
    None
}

/// Whether the place after `before` is inside code: a fenced block left
/// open, or a backtick left open on this line.
fn in_code(before: &str) -> bool {
    if before.lines().filter(|l| l.trim_start().starts_with("```")).count() % 2 == 1 {
        return true;
    }
    before.rsplit('\n').next().unwrap_or("").matches('`').count() % 2 == 1
}

/// Whether a letter typed after `before` starts a sentence: the start of
/// the message or of a line, or after a full stop, "!" or "?" and a
/// space. Not after an abbreviation or an ellipsis, whose sentence goes
/// on.
fn starts_sentence(before: &str) -> bool {
    let head = before.trim_end_matches([' ', '\t']);
    if head.is_empty() || head.ends_with('\n') {
        return true;
    }
    if head.len() == before.len() {
        return false;
    }
    let closed = head.trim_end_matches(['"', '”', '\'', '’', ')']);
    let Some(mark) = closed.chars().next_back() else { return false };
    if !matches!(mark, '.' | '!' | '?') {
        return false;
    }
    if mark != '.' {
        return true;
    }
    let word = closed.rsplit(char::is_whitespace).next().unwrap_or("");
    if word.ends_with("..") {
        return false;
    }
    let stem = word.trim_end_matches('.').to_lowercase();
    // "e.g.", "i.e.", "a.m.": letters with stops between them.
    if stem.contains('.') && stem.split('.').all(|part| part.chars().count() <= 1) {
        return false;
    }
    !matches!(stem.as_str(), "etc" | "vs" | "cf" | "eg" | "ie" | "mr" | "mrs" | "ms" | "dr" | "st" | "no" | "approx" | "fig")
}

/// How a grammar mark the person has ignored is kept: the rule and the
/// words it marked, so the same words are still marked for another
/// reason.
pub fn ignore_key(rule: &str, words: &str) -> String {
    format!("{rule}\t{}", words.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn ignored_file() -> PathBuf {
    crate::paths::root().join("ignored.txt")
}

pub fn ignored() -> HashSet<String> {
    std::fs::read_to_string(ignored_file()).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()
}

/// Never mark these words for this rule again.
pub fn ignore(rule: &str, words: &str) -> std::io::Result<()> {
    let mut all = ignored();
    if !all.insert(ignore_key(rule, words)) {
        return Ok(());
    }
    let mut list: Vec<String> = all.into_iter().collect();
    list.sort();
    let file = ignored_file();
    let tmp = file.with_extension("txt.tmp");
    std::fs::write(&tmp, list.join("\n") + "\n")?;
    std::fs::rename(tmp, file)
}

/// Where the words the person has taught are kept, one to a line.
pub fn learned_file() -> PathBuf {
    crate::paths::root().join("words.txt")
}

pub fn learned() -> HashSet<String> {
    std::fs::read_to_string(learned_file()).unwrap_or_default().lines().map(|l| l.trim().to_lowercase()).filter(|l| !l.is_empty()).collect()
}

/// Adds a word to those never marked as misspelt.
pub fn learn(word: &str) -> std::io::Result<()> {
    let mut words = learned();
    if !words.insert(word.trim().to_lowercase()) {
        return Ok(());
    }
    let mut list: Vec<String> = words.into_iter().collect();
    list.sort();
    let file = learned_file();
    let tmp = file.with_extension("txt.tmp");
    std::fs::write(&tmp, list.join("\n") + "\n")?;
    std::fs::rename(tmp, file)
}
