//! One adapter per coding agent. This is the shape borrowed from Wake: each
//! agent knows where its sessions live, how to list them cheaply, and how to
//! parse one into the shared `Session` model. Everything above this layer (the
//! archive, the search index, the app) is agent-agnostic.
//!
//! The archive is the part Wake does not have. `index_all` unions what each
//! agent still has on disk with what emaki archived, so a session the agent
//! deleted stays listed, searchable and readable.

pub mod claude;
pub mod codex;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::archive;
use crate::build::{Phase, TurnState};
use crate::model::{AgentId, CallStatus, Session};
use crate::options::Catalogue;
use crate::paths;
use crate::transcript::SessionRef;

pub trait Adapter: Send + Sync {
    fn id(&self) -> AgentId;

    /// Where this agent keeps its sessions, whether or not the paths exist.
    /// The watcher and the archive sweep derive from this.
    fn data_roots(&self) -> Vec<PathBuf>;

    fn detect(&self) -> bool {
        self.data_roots().iter().any(|p| p.exists())
    }

    /// Whether a path (from the watcher or the archive) is one of this agent's
    /// main session files.
    fn owns(&self, path: &Path) -> bool;

    /// Every live session, cheap: a stat and a peek per file.
    fn list(&self, min_size: u64) -> Vec<SessionRef>;

    /// Cheap metadata for one file, live or archived.
    fn peek(&self, path: &Path) -> SessionRef;

    /// The full model for one file.
    fn load_path(&self, path: &Path, cwd_hint: &str) -> Session;

    /// What a session of this agent in `cwd` can be told and set to: its
    /// slash commands, and the modes, models and effort levels it offers,
    /// asked of the agent itself. Blocking, so off the main thread. An
    /// agent the app cannot drive has nothing to offer here.
    fn catalogue(&self, _cwd: &str) -> Catalogue {
        Catalogue::default()
    }

    fn load(&self, r: &SessionRef) -> Session {
        let mut s = self.load_path(&r.path, &r.cwd);
        if s.title.is_empty() {
            s.title = r.title.clone();
        }
        s
    }
}

pub fn all() -> Vec<Box<dyn Adapter>> {
    vec![Box::new(claude::ClaudeAdapter::new()), Box::new(codex::CodexAdapter::new())]
}

pub fn for_agent(agent: AgentId) -> Box<dyn Adapter> {
    match agent {
        AgentId::ClaudeCode => Box::new(claude::ClaudeAdapter::new()),
        AgentId::Codex => Box::new(codex::CodexAdapter::new()),
    }
}

/// Which agent a file belongs to, by its path shape. Archived copies keep the
/// original layout, so this answers for both.
pub fn agent_for_path(path: &Path) -> AgentId {
    for a in all() {
        if a.owns(path) {
            return a.id();
        }
    }
    AgentId::ClaudeCode
}

pub fn load_path(path: &Path) -> Session {
    for_agent(agent_for_path(path)).load_path(path, "")
}

/// Metadata for every session of every agent, newest first: what is still on
/// disk plus what only the archive has. Archived entries are flagged.
pub fn index_all(min_size: u64, enabled: &[String]) -> Vec<SessionRef> {
    // Sessions that ran under our own directory (the explainer's) are not
    // conversations; the directory's old name counts too.
    let own = paths::root().to_string_lossy().to_string();
    let legacy = paths::legacy_root().to_string_lossy().to_string();
    let mut refs: Vec<SessionRef> = Vec::new();
    let mut seen: HashSet<(AgentId, String)> = HashSet::new();
    for adapter in all() {
        if !enabled.is_empty() && !enabled.iter().any(|e| e == adapter.id().as_str()) {
            continue;
        }
        for r in adapter.list(min_size) {
            if seen.insert((r.agent, r.session_id.clone())) {
                refs.push(r);
            }
        }
        for (_project, path) in archive::iter_archived(adapter.id()) {
            let Ok(st) = std::fs::metadata(&path) else { continue };
            if st.len() < min_size {
                continue;
            }
            let mut r = adapter.peek(&path);
            if !r.cwd.is_empty() && (paths::is_explainer_cwd(&r.cwd, &own) || r.cwd.starts_with(&legacy)) {
                continue;
            }
            if seen.contains(&(r.agent, r.session_id.clone())) {
                continue;
            }
            r.archived = true;
            seen.insert((r.agent, r.session_id.clone()));
            refs.push(r);
        }
    }
    refs.sort_by(|a, b| b.mtime.partial_cmp(&a.mtime).unwrap_or(std::cmp::Ordering::Equal));
    refs
}

/// Where a session stands, for agents whose transcripts carry no stop reason:
/// derived from the built model's tail. Where the transcript says when a
/// turn starts and ends (`Session::turn_open`), that decides between
/// working and not: words said in an open turn are commentary on the way,
/// and a call left pending in a turn that is over is not running.
pub fn turn_state_from_session(s: &Session) -> TurnState {
    use crate::model::{Item, NoticeVariant, ToolKind};
    let mut st = TurnState::default();
    let Some(last) = s.rounds.last() else { return st };
    st.turn_started = last.ts.clone();
    st.since = if last.end_ts.is_empty() { last.ts.clone() } else { last.end_ts.clone() };
    let pending = last.tool_calls().filter(|c| c.status == CallStatus::Pending).last();
    if let Some(call) = pending.filter(|_| s.turn_open != Some(false)) {
        st.phase = if call.tool_kind == ToolKind::Ask { Phase::NeedsYou } else { Phase::Working };
        st.activity = if call.subject.is_empty() { call.name.clone() } else { call.subject.clone() };
        st.activity_kind = if call.tool_kind == ToolKind::Ask { "ask".into() } else { call.tool_kind.as_str().into() };
        st.tool = call.name.clone();
        return st;
    }
    if s.turn_open == Some(true) {
        st.phase = Phase::Working;
        st.activity = if last.items.is_empty() { "reading the prompt" } else { "thinking" }.into();
        st.activity_kind = "wait".into();
        return st;
    }
    // Stopped by the person, as the transcript says at the round's foot:
    // a prompt with no reply under it would read as working for ever. A
    // setting changed since is said under the stop and is not what the
    // round ended on.
    let foot = last.items.iter().rev().find(|i| !matches!(i, Item::Notice { variant, .. } if variant.setting().is_some()));
    match foot {
        Some(Item::Notice { variant: NoticeVariant::Interrupted, .. }) => {
            st.phase = Phase::YourTurn;
            st.activity = "interrupted".into();
            st.activity_kind = "stop".into();
            return st;
        }
        Some(Item::Notice { variant: NoticeVariant::Error, .. }) if s.turn_open == Some(false) => {
            st.phase = Phase::YourTurn;
            st.activity = "failed".into();
            st.activity_kind = "stop".into();
            return st;
        }
        Some(Item::Tool(call)) if s.turn_open == Some(false) && call.tool_kind == ToolKind::Plan && call.status == CallStatus::Pending => {
            st.phase = Phase::YourTurn;
            st.activity = "plan ready".into();
            st.activity_kind = "plan".into();
            return st;
        }
        _ => {}
    }
    if let Some(text) = last.items.iter().rev().find_map(|i| match i {
        Item::Text { md, .. } => Some(md),
        _ => None,
    }) {
        st.phase = Phase::YourTurn;
        st.activity = "replied".into();
        st.activity_kind = "reply".into();
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        st.reply = crate::build::one_line(&first.replace("**", "").replace('`', ""), 160);
        return st;
    }
    if s.turn_open == Some(false) {
        st.phase = Phase::YourTurn;
        st.activity = "done".into();
        st.activity_kind = "reply".into();
    } else if !last.prompt.is_empty() {
        st.phase = Phase::Working;
        st.activity = "reading the prompt".into();
        st.activity_kind = "wait".into();
    }
    st
}
