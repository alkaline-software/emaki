//! The archive: a permanent, byte-for-byte copy of every transcript.
//!
//! This is the reason scribe exists. Claude Code deletes transcripts older
//! than `cleanupPeriodDays` (30 by default) and does so silently. Every other
//! tool in this space reads `~/.claude/projects` and stops there, so they all
//! inherit that expiry. The archive is not a backup you hope never to need: it
//! is a peer source, and a session that has aged out of the agent's own
//! directory stays listed, searchable, renderable and exportable.
//!
//! A Claude Code session is four things on disk, not one:
//!
//! ```text
//! <project>/<session>.jsonl                        the conversation
//! <project>/<session>/subagents/agent-*.jsonl      subagent conversations
//! <project>/<session>/subagents/agent-*.meta.json  which Task spawned each
//! <project>/<session>/tool-results/<id>.txt        outputs too large to inline
//! ```
//!
//! The copy is incremental and append-only, which matches how agents write:
//! we remember how many bytes we have taken and append whatever is new. A
//! source that was rewritten or truncated underneath us never overwrites an
//! archived copy; the existing archive is rotated to a generation file first.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::model::AgentId;
use crate::paths;
use crate::transcript::{inode_of, mtime_secs, SessionRef};

pub const SIDECAR_DIRS: &[&str] = &["subagents", "tool-results"];
pub const MAX_COPY_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub sessions: u64,
    pub files: u64,
    pub bytes: u64,
    pub rotated: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    size: u64,
    inode: u64,
    mtime: f64,
    dst: String,
    seen: f64,
}

pub type State = BTreeMap<String, Entry>;

pub fn archive_dir() -> PathBuf {
    paths::archive_dir()
}

fn state_file() -> PathBuf {
    archive_dir().join("state.json")
}

fn load_state() -> State {
    paths::read_json(&state_file())
        .and_then(|v| serde_json::from_value::<State>(v).ok())
        .unwrap_or_default()
}

fn save_state(state: &State) {
    let _ = fs::create_dir_all(archive_dir());
    if let Ok(v) = serde_json::to_value(state) {
        let _ = paths::write_json(&state_file(), &v);
    }
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Move an archived file aside so a rewritten source cannot destroy it.
fn rotate(dst: &Path) -> Option<PathBuf> {
    let stem = dst.file_stem()?.to_string_lossy().to_string();
    let ext = dst.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1..1000 {
        let candidate = dst.with_file_name(format!("{stem}.gen{n}{ext}"));
        if !candidate.exists() {
            return fs::rename(dst, &candidate).ok().map(|_| candidate);
        }
    }
    None
}

pub fn is_inside_archive(path: &Path) -> bool {
    match (path.canonicalize(), archive_dir().canonicalize()) {
        (Ok(p), Ok(a)) => p.starts_with(a),
        _ => false,
    }
}

/// Copy whatever of `src` we do not already have in `dst`. Keyed on
/// (inode, size) so an unchanged file costs a single `stat`.
pub fn mirror_file(src: &Path, dst: &Path, state: &mut State, stats: &mut Stats) {
    // Never mirror the archive onto itself. Once a session outlives its
    // original it re-enters the index pointing at the archived copy, and
    // without this the rotate-then-copy path renames the destination away and
    // then fails to read the source it just moved.
    match (src.canonicalize(), dst.canonicalize()) {
        (Ok(a), Ok(b)) if a == b => return,
        (Err(_), _) => return,
        _ => {}
    }
    let Ok(st) = fs::metadata(src) else { return };
    if st.len() > MAX_COPY_BYTES {
        return;
    }
    let key = src.to_string_lossy().to_string();
    let prev = state.get(&key).cloned().unwrap_or_default();
    let have = dst.exists();
    let ino = inode_of(&st);
    let same_file = have && prev.inode == ino && state.contains_key(&key);

    let result: std::io::Result<u64> = (|| {
        if same_file && st.len() == prev.size {
            return Ok(0);
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if same_file && st.len() > prev.size {
            let mut fin = fs::File::open(src)?;
            fin.seek(SeekFrom::Start(prev.size))?;
            let mut fout = fs::OpenOptions::new().append(true).open(dst)?;
            let copied = std::io::copy(&mut fin, &mut fout)?;
            fout.flush()?;
            return Ok(copied);
        }
        if have && rotate(dst).is_some() {
            stats.rotated += 1;
        }
        fs::copy(src, dst)?;
        Ok(st.len())
    })();

    match result {
        Ok(0) if same_file => {}
        Ok(copied) => {
            stats.files += 1;
            stats.bytes += copied;
            state.insert(key, Entry { size: st.len(), inode: ino, mtime: mtime_secs(&st), dst: dst.to_string_lossy().to_string(), seen: now() });
        }
        Err(_) => stats.errors += 1,
    }
}

pub fn mirror_tree(src_dir: &Path, dst_dir: &Path, state: &mut State, stats: &mut Stats) {
    let Ok(entries) = fs::read_dir(src_dir) else { return };
    let mut items: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    items.sort();
    for entry in items {
        let Some(name) = entry.file_name() else { continue };
        if entry.is_dir() {
            mirror_tree(&entry, &dst_dir.join(name), state, stats);
        } else if entry.is_file() {
            mirror_file(&entry, &dst_dir.join(name), state, stats);
        }
    }
}

/// Where one session's main file lives in the archive.
pub fn session_archive_path(agent: AgentId, project_slug: &str, file_name: &str) -> PathBuf {
    let mut base = archive_dir();
    if !agent.archive_subdir().is_empty() {
        base = base.join(agent.archive_subdir());
    }
    base.join(paths::safe_component(project_slug)).join(paths::safe_component(file_name))
}

/// Mirror one session: its main file and every sidecar it owns.
pub fn archive_ref(r: &SessionRef, state: &mut State, stats: &mut Stats) {
    let own = paths::root().to_string_lossy().to_string();
    if !r.cwd.is_empty() && paths::is_explainer_cwd(&r.cwd, &own) {
        return;
    }
    if r.archived || is_inside_archive(&r.path) {
        return;
    }
    let slug = if r.cwd.is_empty() { paths::safe_component(&r.project_dir) } else { paths::project_slug(&r.cwd) };
    let file_name = match r.agent {
        AgentId::ClaudeCode => format!("{}.jsonl", r.session_id),
        _ => r.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| format!("{}.jsonl", r.session_id)),
    };
    let target = session_archive_path(r.agent, &slug, &file_name);
    mirror_file(&r.path, &target, state, stats);

    if r.agent == AgentId::ClaudeCode {
        let sidecar_src = r.path.with_extension("");
        if sidecar_src.is_dir() {
            let sidecar_dst = target.with_extension("");
            for name in SIDECAR_DIRS {
                mirror_tree(&sidecar_src.join(name), &sidecar_dst.join(name), state, stats);
            }
        }
    }
    stats.sessions += 1;
}

/// Archive every session given. Cheap when nothing has changed.
pub fn sweep(refs: &[SessionRef]) -> Stats {
    let _ = paths::ensure_dirs();
    let mut state = load_state();
    let mut stats = Stats::default();
    for r in refs {
        archive_ref(r, &mut state, &mut stats);
    }
    if stats.files > 0 || stats.rotated > 0 {
        save_state(&state);
    }
    stats
}

pub fn archive_one(r: &SessionRef) -> Stats {
    sweep(std::slice::from_ref(r))
}

/// Every archived main file for `agent`, as `(project_slug, path)`. Generation
/// files (`*.gen1.jsonl`) are surfaced too: they are earlier incarnations of a
/// rewritten session and still real history.
pub fn iter_archived(agent: AgentId) -> Vec<(String, PathBuf)> {
    let mut root = archive_dir();
    if !agent.archive_subdir().is_empty() {
        root = root.join(agent.archive_subdir());
    }
    let mut out = Vec::new();
    let Ok(projects) = fs::read_dir(&root) else { return out };
    let mut dirs: Vec<PathBuf> = projects.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for project in dirs {
        let name = project.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // Other agents' subtrees sit beside Claude's project folders.
        if agent == AgentId::ClaudeCode && name.starts_with('_') {
            continue;
        }
        let Ok(files) = fs::read_dir(&project) else { continue };
        let mut jsonls: Vec<PathBuf> = files
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|e| e == "jsonl").unwrap_or(false))
            .collect();
        jsonls.sort();
        for j in jsonls {
            out.push((name.clone(), j));
        }
    }
    out
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub sessions: u64,
    pub files: u64,
    pub bytes: u64,
    pub path: String,
}

pub fn summary() -> Summary {
    let root = archive_dir();
    let mut s = Summary { path: root.to_string_lossy().to_string(), ..Default::default() };
    fn walk(dir: &Path, s: &mut Summary) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                walk(&p, s);
            } else if p.is_file() && p.file_name().map(|n| n != "state.json").unwrap_or(true) {
                s.files += 1;
                s.bytes += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    walk(&root, &mut s);
    for agent in AgentId::ALL {
        s.sessions += iter_archived(agent).len() as u64;
    }
    s
}

pub fn human_bytes(n: u64) -> String {
    let mut v = n as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if v < 1024.0 || unit == "GB" {
            return if unit == "B" { format!("{v:.0}{unit}") } else { format!("{v:.1}{unit}") };
        }
        v /= 1024.0;
    }
    format!("{v:.1}GB")
}
