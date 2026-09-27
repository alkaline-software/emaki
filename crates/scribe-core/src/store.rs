//! Turning a session into an on-disk markdown log. One place decides where a
//! session's log lives and what goes in it.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::model::Session;
use crate::paths;
use crate::redact::Redactor;
use crate::render_md;

/// Fill in the derived fields the renderers want.
pub fn annotate(session: &mut Session) {
    if !session.cwd.is_empty() {
        session.project = paths::project_slug(&session.cwd);
    }
    session.log_path = log_path_for(session).to_string_lossy().to_string();
}

pub fn log_path_for(session: &Session) -> PathBuf {
    let cwd = if session.cwd.is_empty() {
        std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        session.cwd.clone()
    };
    let id = if session.id.is_empty() { "unknown" } else { &session.id };
    let title = if session.title.is_empty() { "session" } else { &session.title };
    let started = if session.started.is_empty() { &session.updated } else { &session.started };
    paths::log_path(&cwd, id, title, started)
}

/// Write the markdown for a session, if markdown output is enabled. Leaves
/// the file (and its mtime) alone when nothing changed.
pub fn write_markdown(session: &Session, cfg: &Config, redactor: &Redactor) -> Option<PathBuf> {
    if !cfg.markdown.enabled {
        return None;
    }
    let text = render_md::render(session, cfg, redactor);
    let target = if session.log_path.is_empty() { log_path_for(session) } else { PathBuf::from(&session.log_path) };
    let _ = paths::ensure_dirs();
    if fs::read_to_string(&target).ok().as_deref() == Some(text.as_str()) {
        return Some(target);
    }
    paths::write_atomic(&target, text.as_bytes()).ok()?;
    Some(target)
}

/// Remove earlier logs for the same session id: the filename embeds the
/// AI-generated title, which the agent refines as a conversation develops.
pub fn prune_stale_logs(session: &Session) {
    let target = if session.log_path.is_empty() { log_path_for(session) } else { PathBuf::from(&session.log_path) };
    let sid8: String = paths::safe_component(&session.id).chars().take(8).collect();
    let suffix = format!("-{sid8}.md");
    let Some(parent) = target.parent() else { return };
    let Ok(rd) = fs::read_dir(parent) else { return };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if p != target && p.file_name().map(|n| n.to_string_lossy().ends_with(&suffix)).unwrap_or(false) {
            let _ = fs::remove_file(&p);
        }
    }
}

pub fn build_one(path: &Path, cfg: &Config, redactor: &Redactor) -> (Session, Option<PathBuf>) {
    let mut session = crate::adapters::load_path(path);
    annotate(&mut session);
    let written = write_markdown(&session, cfg, redactor);
    (session, written)
}
