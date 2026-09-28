//! Filesystem layout for scribe.
//!
//! Everything scribe owns lives under `~/.scribe` (mode 0700), deliberately
//! outside every repository. The layout is the one the Python scribe used, so
//! an existing archive, log tree and project registry are picked up as-is:
//!
//! ```text
//! ~/.scribe/
//!   config.json
//!   archive/<project-slug>/<session>.jsonl        (+ <session>/subagents, tool-results)
//!   archive/_codex/<project-slug>/<file>.jsonl    other agents, under a leading underscore
//!   logs/<project-slug>/<YYYY-MM-DD>-<title-slug>-<sid8>.md
//!   state/projects.json                           cwd -> project-slug registry
//!   index.db                                      full-text search
//!   uploads/<session-id>/<id>-<name>
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

fn env_path(name: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(name)?;
    if raw.is_empty() {
        return None;
    }
    let s = raw.to_string_lossy();
    Some(expand_tilde(&s))
}

pub fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        home().join(rest)
    } else if s == "~" {
        home()
    } else {
        PathBuf::from(s)
    }
}

/// The scribe data directory. `SCRIBE_HOME` overrides (tests use it).
pub fn root() -> PathBuf {
    env_path("SCRIBE_HOME").unwrap_or_else(|| home().join(".scribe"))
}

pub fn claude_home() -> PathBuf {
    env_path("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home().join(".claude"))
}

/// Where Claude Code keeps its JSONL transcripts.
pub fn projects_dir() -> PathBuf {
    claude_home().join("projects")
}

pub fn codex_home() -> PathBuf {
    env_path("CODEX_HOME").unwrap_or_else(|| home().join(".codex"))
}

pub fn config_file() -> PathBuf {
    root().join("config.json")
}
pub fn logs_dir() -> PathBuf {
    root().join("logs")
}
pub fn state_dir() -> PathBuf {
    root().join("state")
}
pub fn cache_dir() -> PathBuf {
    root().join("cache")
}
pub fn run_dir() -> PathBuf {
    root().join("run")
}
pub fn archive_dir() -> PathBuf {
    root().join("archive")
}
pub fn index_db() -> PathBuf {
    root().join("index.db")
}
pub fn uploads_dir(session_id: &str) -> PathBuf {
    let base = root().join("uploads");
    if session_id.is_empty() {
        base
    } else {
        base.join(safe_component(session_id))
    }
}

/// Create the whole tree with private permissions. Idempotent.
pub fn ensure_dirs() -> std::io::Result<()> {
    let r = root();
    fs::create_dir_all(&r)?;
    chmod700(&r);
    for d in [logs_dir(), state_dir(), cache_dir(), run_dir(), archive_dir(), uploads_dir("")] {
        fs::create_dir_all(&d)?;
        chmod700(&d);
    }
    Ok(())
}

#[cfg(unix)]
fn chmod700(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}
#[cfg(not(unix))]
fn chmod700(_path: &Path) {}

// ---------------------------------------------------------------- slugs

/// Lowercase ASCII slug. Empty input yields `untitled`.
pub fn slugify(text: &str, max_len: usize) -> String {
    if text.is_empty() {
        return "untitled".into();
    }
    let ascii: String = text
        .nfkd()
        .filter(|c| c.is_ascii())
        .collect::<String>()
        .to_ascii_lowercase();
    let mut out = String::with_capacity(ascii.len());
    let mut dash = false;
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let mut slug = out.trim_matches('-').to_string();
    if slug.len() > max_len {
        slug.truncate(max_len);
        slug = slug.trim_end_matches('-').to_string();
    }
    if slug.is_empty() {
        "untitled".into()
    } else {
        slug
    }
}

/// A single path component that cannot escape its directory.
pub fn safe_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    let cleaned = out.trim_start_matches('.').to_string();
    let cleaned = if cleaned.is_empty() { "unnamed".to_string() } else { cleaned };
    cleaned.chars().take(120).collect()
}

/// Best-effort inverse of Claude Code's cwd mangling (lossy; display only).
///
/// Claude Code replaces every character outside `[A-Za-z0-9]` with `-`, so
/// `/Users/jp/proj` becomes `-Users-jp-proj` and `C:\Users\jp\proj`
/// becomes `C--Users-jp-proj`. A drive letter followed by two dashes is the
/// Windows shape.
pub fn decode_project_dir(name: &str) -> String {
    let b = name.as_bytes();
    if b.len() > 3 && b[0].is_ascii_alphabetic() && &b[1..3] == b"--" {
        return format!("{}:\\{}", &name[..1], name[3..].replace('-', "\\"));
    }
    if let Some(rest) = name.strip_prefix('-') {
        format!("/{}", rest.replace('-', "/"))
    } else {
        name.to_string()
    }
}

pub fn tilde(path: &str) -> String {
    let h = home().to_string_lossy().to_string();
    if let Some(rest) = path.strip_prefix(&h) {
        format!("~{rest}")
    } else {
        path.to_string()
    }
}

// ---------------------------------------------------------------- project registry

static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

/// Stable, friendly directory name for a project.
///
/// The basename is what a human recognises. Two checkouts sharing a basename
/// would collide, so the first cwd to claim a slug keeps it and later ones get
/// a short hash suffix. The claim is recorded in `state/projects.json` so the
/// answer never changes underneath an existing log directory.
pub fn project_slug(cwd: &str) -> String {
    let cwd = absolute(cwd);
    let base_name = cwd
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "root".into());
    let base = slugify(&base_name, 48);
    let key = cwd.to_string_lossy().to_string();

    let _guard = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let registry_path = state_dir().join("projects.json");
    let mut registry: BTreeMap<String, String> = read_json(&registry_path)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    if let Some(found) = registry.get(&key) {
        return found.clone();
    }
    let taken: std::collections::HashSet<&String> = registry.values().collect();
    let slug = if taken.contains(&base) {
        let digest = sha1_smol::Sha1::from(key.as_bytes()).digest().to_string();
        format!("{base}-{}", &digest[..6])
    } else {
        base
    };
    registry.insert(key, slug.clone());
    let _ = ensure_dirs();
    let _ = write_json(&registry_path, &serde_json::to_value(&registry).unwrap_or(Value::Null));
    slug
}

pub fn absolute(p: &str) -> PathBuf {
    let p = if p.is_empty() { "." } else { p };
    let path = expand_tilde(p);
    if path.is_absolute() {
        normalise(&path)
    } else {
        std::env::current_dir().map(|c| normalise(&c.join(&path))).unwrap_or(path)
    }
}

fn normalise(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Where a session's markdown lives:
/// `<logs>/<project-slug>/<YYYY-MM-DD>-<title-slug>-<sid8>.md`.
pub fn log_path(cwd: &str, session_id: &str, title: &str, started: &str) -> PathBuf {
    let day = if started.len() >= 10 { &started[..10] } else { "0000-00-00" };
    let sid = safe_component(session_id);
    let sid8: String = sid.chars().take(8).collect();
    let name = format!("{day}-{}-{sid8}.md", slugify(title, 48));
    logs_dir().join(project_slug(cwd)).join(name)
}

// ---------------------------------------------------------------- json helpers

pub fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_json(path: &Path, data: &Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(data)?;
    text.push('\n');
    write_atomic(path, text.as_bytes())
}

/// Write via a sibling temp file and rename, so a reader sees either the old
/// file or the new one and never a half-written one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!("{name}.tmp{}", std::process::id()));
    let result = (|| {
        let mut fh = fs::File::create(&tmp)?;
        fh.write_all(bytes)?;
        fh.sync_all().ok();
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// A process-wide "this is the explainer's scratch cwd" check. Sessions that
/// ran there are scribe's own helper children and are dropped from the index.
pub fn is_explainer_cwd(cwd: &str, own_root: &str) -> bool {
    let path = absolute(cwd);
    let s = path.to_string_lossy();
    let s = s.trim_end_matches('/');
    if !own_root.is_empty() && s.starts_with(own_root) {
        return true;
    }
    s.ends_with("/run/explain")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_dirs_decode_on_both_path_shapes() {
        assert_eq!(decode_project_dir("-Users-jp-proj"), "/Users/jp/proj");
        assert_eq!(decode_project_dir("C--Users-jp-proj"), "C:\\Users\\jp\\proj");
        assert_eq!(decode_project_dir("D--"), "D--");
        assert_eq!(decode_project_dir("plain"), "plain");
    }

    #[test]
    fn slugs_and_components_are_portable() {
        assert_eq!(slugify("My Project (v2)", 48), "my-project-v2");
        assert_eq!(slugify("", 48), "untitled");
        assert_eq!(safe_component("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_component("C:\\Users\\jp"), "C_Users_jp");
    }

    #[test]
    fn normalise_drops_dot_and_dotdot_on_any_separator() {
        let p = normalise(Path::new("/a/b/../c/./d"));
        assert_eq!(p, PathBuf::from("/a/c/d"));
    }
}
