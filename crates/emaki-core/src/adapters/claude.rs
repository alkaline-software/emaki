//! Claude Code: `~/.claude/projects/<mangled-cwd>/<session>.jsonl` plus the
//! subagent and tool-result sidecars beside it.

use std::path::{Path, PathBuf};

use super::Adapter;
use crate::build::build_from_path;
use crate::model::{AgentId, Session};
use crate::paths;
use crate::transcript::{index_claude, peek, SessionRef};

pub struct ClaudeAdapter {
    projects: PathBuf,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self { projects: paths::projects_dir() }
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl Adapter for ClaudeAdapter {
    fn id(&self) -> AgentId {
        AgentId::ClaudeCode
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.projects.clone()]
    }

    fn owns(&self, path: &Path) -> bool {
        if path.extension().map(|e| e != "jsonl").unwrap_or(true) {
            return false;
        }
        let s = path.to_string_lossy();
        // Sidecars are not sessions; the archive's `_agent` subtrees are not ours.
        if s.contains("/subagents/") || s.contains("/tool-results/") {
            return false;
        }
        let in_projects = path.starts_with(&self.projects);
        let in_archive = path.starts_with(paths::archive_dir())
            && path
                .strip_prefix(paths::archive_dir())
                .ok()
                .and_then(|r| r.components().next())
                .map(|c| !c.as_os_str().to_string_lossy().starts_with('_'))
                .unwrap_or(false);
        in_projects || in_archive
    }

    fn list(&self, min_size: u64) -> Vec<SessionRef> {
        index_claude(&self.projects, min_size)
    }

    fn peek(&self, path: &Path) -> SessionRef {
        peek(path)
    }

    fn load_path(&self, path: &Path, cwd_hint: &str) -> Session {
        build_from_path(path, cwd_hint)
    }
}
