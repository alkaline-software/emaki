//! The commands whose keys a person may change, in Settings, Shortcuts:
//! the window's, and the file editor's that Sublime Text gave it. Each
//! has its name, the keys it has to begin with and the action they run;
//! the keys chosen in place of those are `config.json`'s
//! `app.shortcuts`; and the key bindings are made from them here. The
//! keys everyone knows (copy, paste, select all, find, undo, save,
//! close, quit), the terminal's, and the editor's other Sublime keys
//! (`sublime::bindings`) are not here and do not change (`TAKEN`).

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::RwLock;

use gpui::{Action, App, KeyBinding, KeyBindingContextPredicate, Keystroke, Modifiers};

use crate::i18n::phrase;
use crate::sublime;
use crate::workbench::*;

/// A command whose keys may change.
pub(crate) struct Command {
    /// What `config.json` names it by.
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// Its keys to begin with, as gpui writes them; a chord's strokes
    /// apart by a space (`cmd-k cmd-u`).
    pub(crate) keys: &'static str,
    /// Other keys it answers to while its own are the ones it began with,
    /// named nowhere (⌘B for the sidebar, as an editor has it).
    aliases: &'static [&'static str],
    /// The file editor's, bound where its text has the keyboard; the
    /// window's otherwise.
    pub(crate) editor: bool,
    action: fn() -> Box<dyn Action>,
}

/// The keys on a Mac, or off one.
const fn os(mac: &'static str, pc: &'static str) -> &'static str {
    if cfg!(target_os = "macos") { mac } else { pc }
}

/// A group of commands as Settings shows it: its name and its commands.
pub(crate) type Group = (&'static str, &'static [Command]);

/// The window's commands, in two groups.
pub(crate) static WINDOW: [Group; 2] = [
    (
        phrase("Sessions"),
        &[
            Command { id: "search", name: phrase("Search"), keys: "secondary-k", aliases: &[], editor: false, action: || Box::new(ToggleSearch) },
            Command { id: "new-session", name: phrase("New Session"), keys: "secondary-n", aliases: &[], editor: false, action: || Box::new(NewSession) },
            Command { id: "new-session-here", name: phrase("New Session Here"), keys: "secondary-t", aliases: &[], editor: false, action: || Box::new(NewSessionHere) },
            Command { id: "choose-folder", name: phrase("Choose Folder…"), keys: "secondary-o", aliases: &[], editor: false, action: || Box::new(ChooseFolder) },
            Command { id: "new-branch", name: phrase("New Branch…"), keys: "secondary-shift-n", aliases: &[], editor: false, action: || Box::new(NewBranch) },
            Command { id: "all-sessions", name: phrase("All Sessions"), keys: "secondary-l", aliases: &[], editor: false, action: || Box::new(GoSessions) },
            Command { id: "agents", name: phrase("Agents"), keys: "secondary-e", aliases: &[], editor: false, action: || Box::new(GoAgents) },
            Command { id: "reopen-tab", name: phrase("Reopen Tab"), keys: "secondary-shift-t", aliases: &[], editor: false, action: || Box::new(ReopenTab) },
            Command { id: "refresh", name: phrase("Refresh"), keys: "secondary-r", aliases: &[], editor: false, action: || Box::new(Refresh) },
        ],
    ),
    (
        phrase("Panels"),
        &[
            Command { id: "sidebar", name: phrase("Sidebar"), keys: "secondary-shift-b", aliases: &["secondary-b"], editor: false, action: || Box::new(ToggleSidebar) },
            Command { id: "files", name: phrase("Files"), keys: "secondary-shift-f", aliases: &[], editor: false, action: || Box::new(ToggleFiles) },
            Command { id: "terminal", name: phrase("Shell Terminal"), keys: "secondary-shift-s", aliases: &[], editor: false, action: || Box::new(ToggleShell) },
            Command { id: "outline", name: phrase("Outline"), keys: "secondary-shift-o", aliases: &[], editor: false, action: || Box::new(ToggleOutline) },
            Command { id: "layout", name: phrase("Layout Switch"), keys: "secondary-shift-l", aliases: &[], editor: false, action: || Box::new(ToggleAgent) },
            Command { id: "changes", name: phrase("Git Changes"), keys: "secondary-shift-g", aliases: &[], editor: false, action: || Box::new(ToggleChanges) },
            Command { id: "raw", name: phrase("Editor Switch"), keys: "secondary-shift-v", aliases: &[], editor: false, action: || Box::new(ToggleRaw) },
        ],
    ),
];

/// The file editor's, Sublime Text's keys on a Mac and off one: those of
/// its commands that are used most, in two groups. Its others keep
/// Sublime's keys (`sublime::bindings`).
pub(crate) static EDITOR: [Group; 2] = [
    (
        phrase("Lines"),
        &[
            Command { id: "delete-line", name: phrase("Delete Line"), keys: "ctrl-shift-k", aliases: &[], editor: true, action: || Box::new(sublime::DeleteLine) },
            Command { id: "duplicate-line", name: phrase("Duplicate Line"), keys: os("cmd-shift-d", "ctrl-shift-d"), aliases: &[], editor: true, action: || Box::new(sublime::DuplicateLine) },
            Command { id: "line-up", name: phrase("Move Line Up"), keys: os("ctrl-cmd-up", "ctrl-shift-up"), aliases: &[], editor: true, action: || Box::new(sublime::SwapLineUp) },
            Command { id: "line-down", name: phrase("Move Line Down"), keys: os("ctrl-cmd-down", "ctrl-shift-down"), aliases: &[], editor: true, action: || Box::new(sublime::SwapLineDown) },
            Command { id: "line-after", name: phrase("Insert Line After"), keys: os("cmd-enter", "ctrl-enter"), aliases: &[], editor: true, action: || Box::new(sublime::InsertLineAfter) },
            Command { id: "line-before", name: phrase("Insert Line Before"), keys: os("cmd-shift-enter", "ctrl-shift-enter"), aliases: &[], editor: true, action: || Box::new(sublime::InsertLineBefore) },
            Command { id: "join-lines", name: phrase("Join Lines"), keys: os("cmd-shift-j", "ctrl-shift-j"), aliases: &[], editor: true, action: || Box::new(sublime::JoinLines) },
            Command { id: "sort-lines", name: phrase("Sort Lines"), keys: os("f5", "f9"), aliases: &[], editor: true, action: || Box::new(sublime::SortLines) },
            Command { id: "expand-to-line", name: phrase("Expand to Line"), keys: os("cmd-l", "ctrl-l"), aliases: &[], editor: true, action: || Box::new(sublime::ExpandToLine) },
        ],
    ),
    (
        phrase("Text"),
        &[
            Command { id: "comment", name: phrase("Toggle Comment"), keys: os("cmd-/", "ctrl-/"), aliases: &[], editor: true, action: || Box::new(sublime::ToggleComment) },
            Command { id: "block-comment", name: phrase("Block Comment"), keys: os("cmd-alt-/", "ctrl-shift-/"), aliases: &[], editor: true, action: || Box::new(sublime::ToggleBlockComment) },
            Command { id: "upper-case", name: phrase("Upper Case"), keys: os("cmd-k cmd-u", "ctrl-k ctrl-u"), aliases: &[], editor: true, action: || Box::new(sublime::UpperCase) },
            Command { id: "lower-case", name: phrase("Lower Case"), keys: os("cmd-k cmd-l", "ctrl-k ctrl-l"), aliases: &[], editor: true, action: || Box::new(sublime::LowerCase) },
            Command { id: "next-match", name: phrase("Select Next Match"), keys: os("cmd-d", "ctrl-d"), aliases: &[], editor: true, action: || Box::new(sublime::FindUnderExpand) },
            Command { id: "find-selection", name: phrase("Use Selection for Find"), keys: os("cmd-e", "ctrl-e"), aliases: &[], editor: true, action: || Box::new(sublime::UseSelectionForFind) },
            Command { id: "jump-to-bracket", name: phrase("Jump to Bracket"), keys: "ctrl-m", aliases: &[], editor: true, action: || Box::new(sublime::JumpToBracket) },
            Command { id: "fold", name: phrase("Fold"), keys: os("cmd-alt-[", "ctrl-shift-["), aliases: &[], editor: true, action: || Box::new(sublime::Fold) },
            Command { id: "unfold", name: phrase("Unfold"), keys: os("cmd-alt-]", "ctrl-shift-]"), aliases: &[], editor: true, action: || Box::new(sublime::Unfold) },
        ],
    ),
];

/// The groups of the window's commands, or of the editor's.
pub(crate) fn groups(editor: bool) -> &'static [Group] {
    if editor { &EDITOR } else { &WINDOW }
}

/// Keys no command may take, and what they are: the ones everyone knows,
/// and the window's own that are not offered for a change. ⌘↩ is the
/// composer's Send, and the editor's own in its text.
const TAKEN: &[(&str, &str)] = &[
    ("secondary-c", phrase("Copy")),
    ("secondary-v", phrase("Paste")),
    ("secondary-x", phrase("Cut")),
    ("secondary-a", phrase("Select All")),
    ("secondary-z", phrase("Undo")),
    ("secondary-shift-z", phrase("Redo")),
    ("secondary-f", phrase("Find in Conversation")),
    ("secondary-g", phrase("Find Next")),
    ("secondary-s", phrase("Save File")),
    ("secondary-w", phrase("Close Tab")),
    ("secondary-q", phrase("Quit Emaki")),
    ("secondary-,", phrase("Settings…")),
    ("secondary-enter", phrase("Send")),
    ("secondary-h", phrase("Hide Emaki")),
    ("secondary-alt-h", phrase("Hide Others")),
    ("secondary-m", phrase("Minimize")),
    ("ctrl-secondary-f", phrase("Toggle Full Screen")),
    ("secondary-1", phrase("Tabs")),
    ("secondary-2", phrase("Tabs")),
    ("secondary-3", phrase("Tabs")),
    ("secondary-4", phrase("Tabs")),
    ("secondary-5", phrase("Tabs")),
    ("secondary-6", phrase("Tabs")),
    ("secondary-7", phrase("Tabs")),
    ("secondary-8", phrase("Tabs")),
    ("secondary-9", phrase("Tabs")),
];

/// The keys chosen, as `config.json` has them, for the places that draw
/// a command's keys and have no workbench at hand (a hover's label).
static CHOSEN: RwLock<BTreeMap<String, String>> = RwLock::new(BTreeMap::new());

pub(crate) fn commands() -> impl Iterator<Item = &'static Command> {
    WINDOW.iter().chain(EDITOR.iter()).flat_map(|(_, list)| list.iter())
}

pub(crate) fn command(id: &str) -> Option<&'static Command> {
    commands().find(|c| c.id == id)
}

/// The keys chosen are these from now on (read at launch, and at each
/// change in Settings).
pub(crate) fn set_chosen(chosen: &BTreeMap<String, String>) {
    *CHOSEN.write().unwrap() = chosen.clone();
}

/// A command's keys now, as gpui writes them: those chosen, or its own.
pub(crate) fn keys_of(id: &str) -> String {
    let Some(c) = command(id) else { return String::new() };
    CHOSEN.read().unwrap().get(id).filter(|k| strokes(k).is_some()).cloned().unwrap_or_else(|| c.keys.to_string())
}

/// Whether a command has keys of its own choosing.
pub(crate) fn changed(id: &str) -> bool {
    command(id).is_some_and(|c| !same(&keys_of(id), c.keys))
}

/// The keystrokes of keys as gpui writes them, a chord's one by one;
/// none where one does not read.
fn strokes(keys: &str) -> Option<Vec<Keystroke>> {
    let all: Vec<Keystroke> = keys.split_whitespace().map(Keystroke::parse).collect::<Result<_, _>>().ok()?;
    (!all.is_empty()).then_some(all)
}

/// Two keystrokes are one: the same key and the same modifiers.
fn one(a: &Keystroke, b: &Keystroke) -> bool {
    a.key == b.key && a.modifiers == b.modifiers
}

/// Two keys as gpui writes them are the same: the same strokes, however
/// written (`secondary` or `cmd`).
pub(crate) fn same(a: &str, b: &str) -> bool {
    match (strokes(a), strokes(b)) {
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(&b).all(|(a, b)| one(a, b)),
        _ => false,
    }
}

/// Whether one keystroke pressed would be `keys`, or begin them, as ⌘K
/// begins ⌘K ⌘U.
fn clash(pressed: &str, keys: &str) -> bool {
    match (strokes(pressed), strokes(keys)) {
        (Some(p), Some(k)) => p.len() == 1 && one(&p[0], &k[0]),
        _ => false,
    }
}

/// What has these keys already, as its name: another command, in either
/// list, since a window's key the editor's text also has is dead there
/// and the other way round; one of the window's keys that do not change;
/// or, in the editor, a key Sublime Text has there. A command may always
/// go back to its own keys. None when they are free.
pub(crate) fn taken_by(keys: &str, but: &str) -> Option<&'static str> {
    let me = command(but)?;
    if same(keys, me.keys) {
        return None;
    }
    let editor = me.editor;
    let theirs = commands().filter(|c| c.id != but).find(|c| clash(keys, &keys_of(c.id)) || !changed(c.id) && c.aliases.iter().any(|a| clash(keys, a)));
    theirs
        .map(|c| c.name)
        .or_else(|| TAKEN.iter().filter(|(k, _)| !(editor && *k == "secondary-enter")).find(|(k, _)| same(k, keys)).map(|(_, n)| *n))
        .or_else(|| {
            let k = strokes(keys)?;
            // One key is taken by a chord it begins; a chord by itself.
            let hit = |b: &KeyBinding| match b.match_keystrokes(&k) {
                Some(pending) => k.len() == 1 || !pending,
                None => false,
            };
            (editor && sublime::bindings().iter().any(hit)).then_some(phrase("Sublime Text"))
        })
        .or_else(|| (!editor && strokes(keys).is_some_and(|k| k.len() == 1 && terminal_reads(&k[0]))).then_some(phrase("Terminal")))
}

/// `taken_by`, and any other binding in the keymap the keys are, or
/// begin: the text fields', the terminal's, a list's or a menu's. A
/// command's key that one of those has is dead where it is heard first.
pub(crate) fn conflict(keys: &str, but: &str, cx: &App) -> Option<&'static str> {
    if command(but).is_some_and(|c| same(keys, c.keys)) {
        return None;
    }
    taken_by(keys, but).or_else(|| {
        let k = strokes(keys).filter(|k| k.len() == 1)?;
        let ours = |b: &KeyBinding| commands().any(|c| b.action().partial_eq(&*(c.action)()));
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let b = keymap.bindings().find(|b| !ours(b) && b.match_keystrokes(&k).is_some())?;
        let context = b.predicate().map(|p| p.to_string()).unwrap_or_default();
        Some(if context.contains(sublime::CONTEXT.split(' ').next().unwrap_or("FileEditor")) {
            phrase("Sublime Text")
        } else if context.contains(TERMINAL_CONTEXT) {
            phrase("Terminal")
        } else if context.contains("Input") {
            phrase("Text fields")
        } else {
            phrase("Emaki")
        })
    })
}

/// Keys the terminal reads for itself, not through a binding
/// (`term_panel_key`): a window's command on one would take it from the
/// terminal. On a Mac, ⌃ and a key (the shell's ^C, ^R), ⌘ with an arrow,
/// Backspace, Delete, Return or a page key, ⌘⇧[ and ⌘⇧], and a function
/// key without ⌘; off one, Alt and a key, Ctrl or Shift with those keys,
/// and a function key without Ctrl.
fn terminal_reads(k: &Keystroke) -> bool {
    let m = &k.modifiers;
    let moves = matches!(k.key.as_str(), "left" | "right" | "up" | "down" | "backspace" | "delete" | "enter" | "home" | "end" | "pageup" | "pagedown" | "tab" | "insert");
    let function_key = k.key.len() > 1 && k.key.starts_with('f') && k.key[1..].chars().all(|c| c.is_ascii_digit());
    let tabs = m.shift && matches!(k.key.as_str(), "[" | "]" | "{" | "}");
    if cfg!(target_os = "macos") {
        m.control && !m.platform || m.platform && (moves || tabs) || function_key && !m.platform
    } else {
        m.alt && !m.control || (m.control || m.shift) && moves || function_key && !m.control
    }
}

/// Where a command's keys are heard: the window, or the editor's text.
fn context_of(c: &Command) -> Option<Rc<KeyBindingContextPredicate>> {
    KeyBindingContextPredicate::parse(if c.editor { sublime::CONTEXT } else { KEY_CONTEXT }).ok().map(Rc::new)
}

/// The window's bindings of the commands, with the keys now chosen. They
/// stand first among the window's, so a binding of the same keys bound
/// after them (⌘⇧G's Find Previous over Changes and History) is asked
/// first and may pass the key on.
pub(crate) fn bindings() -> Vec<KeyBinding> {
    let bind = |keys: &str, c: &Command| KeyBinding::load(keys, (c.action)(), context_of(c), false, None, &gpui::DummyKeyboardMapper).ok();
    let mut out = Vec::new();
    for c in commands() {
        out.extend(bind(&keys_of(c.id), c).or_else(|| bind(c.keys, c)));
        if !changed(c.id) {
            out.extend(c.aliases.iter().filter_map(|a| bind(a, c)));
        }
    }
    out
}

/// The window's key bindings made again with the keys now chosen: every
/// binding of a command's action where it is heard is taken out (the
/// toolkit binds some of the editor's actions in every field too), and the commands' own are
/// put where the first of them stood, so every other binding keeps its
/// place and its rank. The menus are made again for the keys they show.
pub(crate) fn rebind(cx: &mut App) {
    let ours = |b: &KeyBinding| commands().any(|c| b.action().partial_eq(&*(c.action)()) && b.predicate().as_deref() == context_of(c).as_deref());
    let old: Vec<KeyBinding> = cx.key_bindings().borrow().bindings().cloned().collect();
    let mut new = Vec::with_capacity(old.len());
    let mut placed = false;
    for b in old {
        if !ours(&b) {
            new.push(b);
        } else if !placed {
            new.extend(bindings());
            placed = true;
        }
    }
    cx.clear_key_bindings();
    cx.bind_keys(new);
    cx.set_menus(crate::app_menus());
}

/// Keys as the signs and the key a person reads, in the order the
/// window writes them everywhere: ⌘, then ⌃, ⌥ and ⇧, and the key. Off a
/// Mac ⌘ is Ctrl's sign, as `key_cap` draws it, and ⌥ is Alt's. A
/// chord's strokes are parted by an empty one.
pub(crate) fn caps(keys: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, k) in strokes(keys).unwrap_or_default().iter().enumerate() {
        if i > 0 {
            out.push(String::new());
        }
        out.extend(stroke_caps(k));
    }
    out
}

fn stroke_caps(k: &Keystroke) -> Vec<String> {
    let m = &k.modifiers;
    let mac = cfg!(target_os = "macos");
    let mut out: Vec<String> = Vec::new();
    if mac && m.platform || !mac && m.control {
        out.push("⌘".into());
    }
    if !mac && m.platform {
        out.push("Win".into());
    }
    if mac && m.control {
        out.push("⌃".into());
    }
    if m.alt {
        out.push("⌥".into());
    }
    if m.shift {
        out.push("⇧".into());
    }
    out.push(key_name(&k.key));
    out
}

/// The modifiers held, as their signs in the window's order (`caps`).
pub(crate) fn held(m: &Modifiers) -> Vec<String> {
    if !m.modified() {
        return Vec::new();
    }
    // A key no modifier names, to drop again.
    let mut out = caps(&Keystroke { modifiers: Modifiers { function: false, ..*m }, key: "a".into(), key_char: None }.unparse());
    out.pop();
    out
}

/// A key's name on its cap.
fn key_name(key: &str) -> String {
    match key {
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "enter" => "↩".into(),
        "tab" => "⇥".into(),
        "space" => "Space".into(),
        "escape" => "Esc".into(),
        "backspace" => "⌫".into(),
        "delete" => "⌦".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "PgUp".into(),
        "pagedown" => "PgDn".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        // F1 and the like.
        k => {
            let mut c = k.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    }
}

/// A command's keys as words that name keys (`key_text`): "⌘⇧S".
pub(crate) fn signs(id: &str) -> String {
    caps(&keys_of(id)).concat()
}

/// Words that name a command's keys as it began with them ("Files
/// (⌘⇧F)") naming the keys it has now. The words are each language's;
/// only the keys in them change.
pub(crate) fn keyed(text: &str, id: &str) -> String {
    match command(id).filter(|_| changed(id)) {
        Some(c) => text.replacen(&caps(c.keys).concat(), &signs(id), 1),
        None => text.to_string(),
    }
}

/// What a shortcut needs that a key pressed without it lacks: ⌘ or ⌃ on
/// a Mac, Ctrl or Alt off one (`key_text` draws ⌘ and ⌥ so there).
const NEEDS: &str = if cfg!(target_os = "macos") { phrase("Add ⌘ or ⌃") } else { phrase("Add ⌘ or ⌥") };

/// What a keystroke pressed in Settings, Shortcuts, makes of a command's
/// keys, as gpui writes them: one modifier or several (⌘ or ⌃ among them,
/// Ctrl or Alt off a Mac) with a letter, a digit or a mark, or with an
/// arrow, Return, Tab, Space, Backspace, Delete or a page key; or a
/// function key, alone or not. A key that would be one but lacks ⌘ or ⌃
/// is what it needs (`NEEDS`). None for a modifier alone, which only
/// shows while it is held.
pub(crate) fn recorded(k: &Keystroke) -> Option<Result<String, &'static str>> {
    let m = k.modifiers;
    let held = if cfg!(target_os = "macos") { m.platform || m.control } else { m.control || m.alt };
    let function_key = k.key.len() > 1 && k.key.starts_with('f') && k.key[1..].chars().all(|c| c.is_ascii_digit());
    let named = matches!(k.key.as_str(), "up" | "down" | "left" | "right" | "enter" | "tab" | "space" | "backspace" | "delete" | "home" | "end" | "pageup" | "pagedown");
    if !(k.key.chars().count() == 1 || named || function_key) {
        return None;
    }
    if !(held || function_key) {
        return Some(Err(NEEDS));
    }
    let modifiers = Modifiers { function: false, ..m };
    Some(Ok(Keystroke { modifiers, key: k.key.clone(), key_char: None }.unparse()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_as_their_signs() {
        assert_eq!(caps("secondary-shift-s").concat(), "⌘⇧S");
        assert!(same("secondary-shift-s", if cfg!(target_os = "macos") { "cmd-shift-s" } else { "ctrl-shift-s" }));
        assert!(!same("secondary-s", "secondary-shift-s"));
        assert_eq!(caps("cmd-k cmd-u").len(), 5);
        assert!(clash("cmd-k", "cmd-k cmd-u") && !clash("cmd-u", "cmd-k cmd-u"));
    }

    #[test]
    fn every_command_has_keys_of_its_own() {
        for c in commands() {
            assert!(strokes(c.keys).is_some(), "{}", c.id);
            // In its own list no other has them, or begins with them.
            let other = commands().filter(|o| o.id != c.id && o.editor == c.editor).find(|o| clash(c.keys, o.keys) || clash(o.keys, c.keys));
            assert!(other.is_none(), "{} begins with keys {} has", c.id, other.map(|o| o.id).unwrap_or(""));
        }
    }

    #[test]
    fn a_shortcut_is_modifiers_and_a_key() {
        let press = |s: &str| recorded(&Keystroke::parse(s).unwrap());
        let cmd = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
        // One modifier or several, with a letter or a mark.
        assert_eq!(press(&format!("{cmd}-k")), Some(Ok(format!("{cmd}-k"))));
        assert!(matches!(press(&format!("{cmd}-alt-shift-k")), Some(Ok(k)) if same(&k, &format!("{cmd}-alt-shift-k"))));
        assert!(matches!(press(&format!("{cmd}-shift-/")), Some(Ok(_))));
        assert!(matches!(press(&format!("{cmd}-;")), Some(Ok(_))));
        // Without ⌘ or ⌃ it says what it needs; a modifier alone waits.
        assert_eq!(press("shift-k"), Some(Err(NEEDS)));
        assert_eq!(press("k"), Some(Err(NEEDS)));
        assert_eq!(press("shift"), None);
        assert!(matches!(press("f6"), Some(Ok(_))));
    }

    #[test]
    fn keys_that_are_not_free_are_refused() {
        let cmd = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
        // Another command's, in this list or the other.
        assert_eq!(taken_by(&format!("{cmd}-k"), "files"), Some(phrase("Search")));
        assert_eq!(taken_by(&format!("{cmd}-shift-d"), "files"), Some(phrase("Duplicate Line")));
        // One everyone knows.
        assert_eq!(taken_by(&format!("{cmd}-c"), "files"), Some(phrase("Copy")));
        // A command's own keys, though another shares them.
        assert_eq!(taken_by(&format!("{cmd}-l"), "all-sessions"), None);
        assert_eq!(taken_by(&format!("{cmd}-shift-y"), "files"), None);
        if cfg!(target_os = "macos") {
            // The terminal's own.
            assert_eq!(taken_by("ctrl-r", "files"), Some(phrase("Terminal")));
            assert_eq!(taken_by("cmd-left", "files"), Some(phrase("Terminal")));
            // A chord's first key, in the editor (and Search's in the window).
            assert!(clash("cmd-k", &keys_of("upper-case")));
            assert!(taken_by("cmd-k", "fold").is_some());
        }
    }
}
