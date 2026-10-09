//! The files of a folder, for "@" in the composer: what Claude Code's own
//! prompt offers when "@" is typed, a path under the working directory.
//! The list is git's when the folder is in a repository (tracked files and
//! untracked ones git does not ignore), else a walk that leaves out hidden
//! folders and the usual build output. Folders are listed too, with a
//! trailing slash, as the terminal lists them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The most paths a folder's list holds; a larger tree is cut here.
pub const MAX_FILES: usize = 30_000;
/// Folders a walk outside git never goes into.
const SKIPPED: &[&str] = &["node_modules", "target", "dist", "build", "__pycache__", "venv"];

/// One path of the list: as written, relative to the folder, with "/"
/// between parts and after a folder's name.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    lower: String,
    /// Where the last part begins in `path`.
    name_at: usize,
}

impl Entry {
    fn new(path: String) -> Self {
        let body = path.trim_end_matches('/');
        let name_at = body.rfind('/').map(|i| i + 1).unwrap_or(0);
        Entry { lower: path.to_lowercase(), path, name_at }
    }

    pub fn is_dir(&self) -> bool {
        self.path.ends_with('/')
    }

    /// The last part, a folder's with its slash.
    pub fn name(&self) -> &str {
        &self.path[self.name_at..]
    }

    /// The folder it is in, empty at the top.
    pub fn parent(&self) -> &str {
        self.path[..self.name_at].trim_end_matches('/')
    }
}

/// Every file and folder under `cwd`, folders first by depth then by name.
pub fn list(cwd: &str) -> Vec<Entry> {
    let mut files = git_files(cwd).unwrap_or_else(|| walk(Path::new(cwd)));
    files.truncate(MAX_FILES);
    let mut dirs = BTreeSet::new();
    for f in &files {
        let mut at = 0;
        while let Some(i) = f[at..].find('/') {
            at += i + 1;
            dirs.insert(f[..at].to_string());
        }
    }
    let mut all: Vec<String> = dirs.into_iter().chain(files).collect();
    all.sort_by_key(|p| (p.trim_end_matches('/').matches('/').count(), !p.ends_with('/'), p.to_lowercase()));
    all.dedup();
    all.into_iter().map(Entry::new).collect()
}

fn git_files(cwd: &str) -> Option<Vec<String>> {
    let out = crate::git::command().args(["-C", cwd, "ls-files", "-z", "--cached", "--others", "--exclude-standard"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
}

fn walk(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                    stack.push(entry.path());
                }
            } else if let Ok(rel) = entry.path().strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out
}

/// The entries that answer what is typed after "@", best first, at most
/// `limit`. With nothing typed, the top of the folder. A query with a
/// slash is matched against the path, and "dir/" lists what is in it;
/// otherwise a name that starts with the words comes before a name that
/// holds them, then a path that holds them, then one whose letters have
/// them in order.
pub fn matches<'a>(entries: &'a [Entry], typed: &str, limit: usize) -> Vec<&'a Entry> {
    let needle = typed.trim_start_matches("./").to_lowercase();
    if needle.is_empty() {
        return entries.iter().filter(|e| e.parent().is_empty()).take(limit).collect();
    }
    let mut ranked: Vec<(u8, usize, &Entry)> = Vec::new();
    for e in entries {
        let name = &e.lower[e.name_at..];
        let rank = if needle.ends_with('/') {
            // Inside a folder: what it holds directly, then the rest.
            match e.lower.strip_prefix(&needle) {
                Some("") => None,
                Some(rest) if !rest.trim_end_matches('/').contains('/') => Some(0),
                Some(_) => Some(2),
                None => None,
            }
        } else if needle.contains('/') {
            if e.lower.starts_with(&needle) {
                Some(0)
            } else if e.lower.contains(&needle) {
                Some(1)
            } else {
                None
            }
        } else if name.starts_with(&needle) {
            Some(0)
        } else if name.contains(&needle) {
            Some(1)
        } else if e.lower.contains(&needle) {
            Some(2)
        } else if in_order(&e.lower, &needle) {
            Some(3)
        } else {
            None
        };
        if let Some(r) = rank {
            ranked.push((r, e.path.len(), e));
        }
    }
    ranked.sort_by_key(|(r, len, _)| (*r, *len));
    ranked.into_iter().take(limit).map(|(_, _, e)| e).collect()
}

/// Whether `needle`'s characters all appear in `hay`, in order.
fn in_order(hay: &str, needle: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|n| it.any(|h| h == n))
}

/// The path an "@" names, on disk: as it is when absolute, under the home
/// folder after "~/", else under `cwd`.
pub fn resolve(cwd: &str, path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    }
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(cwd).join(p)
    }
}

/// Every "@path" in `text` that stands as a word of its own, as byte
/// ranges with the "@" and the path each names: at the start or after a
/// space or an opening bracket, so an address (`a@b.com`) is not one. A
/// path with spaces is written `@"my file.txt"`, as Claude Code writes it.
pub fn at_tokens(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap();
        let opens = prev.map(|p| p.is_whitespace() || "([{".contains(p)).unwrap_or(true);
        prev = Some(c);
        if c != '@' || !opens {
            i += c.len_utf8();
            continue;
        }
        let rest = &text[i + 1..];
        if let Some(quoted) = rest.strip_prefix('"') {
            if let Some(close) = quoted.find(['"', '\n']).filter(|j| quoted.as_bytes()[*j] == b'"') {
                let end = i + 2 + close + 1;
                out.push((i, end, quoted[..close].to_string()));
                prev = Some('"');
                i = end;
                continue;
            }
        }
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if len > 0 {
            out.push((i, i + 1 + len, rest[..len].to_string()));
            prev = rest[..len].chars().next_back();
        }
        i += 1 + len;
    }
    out
}

/// The part of an "@" token that names something on disk under `cwd`:
/// what is left once the sentence's own punctuation is taken off its end
/// ("see @src/main.rs."), or the whole of it. The shorter is asked first:
/// Windows answers for "main.rs." with "main.rs", so the whole would take
/// the sentence's full stop with it. None when neither is there.
pub fn named<'a>(cwd: &str, path: &'a str) -> Option<&'a str> {
    if cwd.is_empty() || path.is_empty() {
        return None;
    }
    let bare = path.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}']);
    [bare, path].into_iter().find(|p| !p.is_empty() && resolve(cwd, p).exists())
}

/// The "@" token the caret is in or just after, while one is being typed:
/// where it starts, where it ends, and what of the path lies before the
/// caret. None inside a quoted path, which is not completed.
pub fn at_token_at(text: &str, caret: usize) -> Option<(usize, usize, String)> {
    let caret = caret.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let back = text[..caret].chars().rev().take_while(|c| !c.is_whitespace()).map(char::len_utf8).sum::<usize>();
    let word = caret - back;
    // The "@" is the word's first character, or follows a bracket in it.
    let start = text[word..caret].char_indices().find(|(i, c)| *c == '@' && (*i == 0 || "([{".contains(text[word..word + i].chars().next_back().unwrap_or(' ')))).map(|(i, _)| word + i)?;
    let typed = &text[start + 1..caret];
    if typed.starts_with('"') {
        return None;
    }
    let end = caret + text[caret..].chars().take_while(|c| !c.is_whitespace()).map(char::len_utf8).sum::<usize>();
    Some((start, end, typed.to_string()))
}

/// The path as it is written after "@": between quotes when it has a
/// space in it.
pub fn written(path: &str) -> String {
    if path.contains(' ') {
        format!("@\"{path}\"")
    } else {
        format!("@{path}")
    }
}

/// `text` as markdown with each "@path" that names something under `cwd`
/// set as inline code, which is how the conversation colours it. Code is
/// left alone, as in `driver::mark_commands`.
pub fn mark_mentions(text: &str, cwd: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push_str(line);
            continue;
        }
        if fenced || line.starts_with("    ") || line.starts_with('\t') {
            out.push_str(line);
            continue;
        }
        let mut at = 0;
        for (a, b, path) in at_tokens(line) {
            if line[..a].matches('`').count() % 2 == 1 {
                continue;
            }
            // A quoted path is the whole token; a bare one may end in the
            // sentence's punctuation.
            let end = if line[a + 1..].starts_with('"') {
                if !resolve(cwd, &path).exists() || cwd.is_empty() {
                    continue;
                }
                b
            } else {
                match named(cwd, &path) {
                    Some(p) => a + 1 + p.len(),
                    None => continue,
                }
            };
            out.push_str(&line[at..a]);
            out.push('`');
            out.push_str(&line[a..end]);
            out.push('`');
            at = end;
        }
        out.push_str(&line[at..]);
    }
    out
}

/// A comma- or tab-separated file as rows of cells, for showing: quoted
/// cells may hold the separator, a doubled quote and line breaks. No
/// more than `most` rows are read; the second answer says whether the
/// text went on.
pub fn table(text: &str, sep: char, most: usize) -> (Vec<Vec<String>>, bool) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut cell, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
    let mut chars = text.strip_prefix('\u{feff}').unwrap_or(text).chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                c => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => {
                quoted = true;
                any = true;
            }
            c if c == sep => {
                row.push(std::mem::take(&mut cell));
                any = true;
            }
            '\r' => {}
            '\n' => {
                if any || !cell.is_empty() {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                }
                any = false;
                if rows.len() >= most {
                    return (rows, chars.any(|c| !c.is_whitespace()));
                }
            }
            c => cell.push(c),
        }
    }
    if any || !cell.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    (rows, false)
}
