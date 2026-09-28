//! Reading Claude Code's JSONL transcripts.
//!
//! The transcript is the source of truth. Two access patterns:
//!
//! - `TranscriptTail`: incremental, byte-offset based, for live sessions.
//! - `peek` / `index_claude`: cheap metadata scan across every project.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::build::{turn_state, user_prompt_text, Phase, TurnState};
use crate::json::*;
use crate::model::AgentId;
use crate::paths;

/// A transcript line can legitimately be large, but beyond this is corruption.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

fn parse_row(raw: &[u8]) -> Option<Value> {
    if raw.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let text = String::from_utf8_lossy(raw);
    match serde_json::from_str::<Value>(&text) {
        Ok(v) if v.is_object() => Some(v),
        _ => None,
    }
}

/// Parse an entire transcript. Bad lines are skipped, not fatal.
pub fn read_all(path: &Path) -> Vec<Value> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if let Some(row) = parse_row(&buf) {
                    rows.push(row);
                }
            }
        }
    }
    rows
}

/// Incremental line reader that remembers where it stopped. Handles a partial
/// final line and a file that was truncated or replaced (`--resume` rewrites).
#[derive(Debug)]
pub struct TranscriptTail {
    pub path: PathBuf,
    pub offset: u64,
    pub inode: Option<u64>,
    /// Set when the last `read_new` restarted from byte zero. Callers that
    /// accumulate rows must drop what they have.
    pub restarted: bool,
    carry: Vec<u8>,
}

impl TranscriptTail {
    pub fn new(path: PathBuf) -> Self {
        Self { path, offset: 0, inode: None, restarted: false, carry: Vec::new() }
    }

    pub fn reset(&mut self) {
        self.offset = 0;
        self.inode = None;
        self.carry.clear();
    }

    pub fn read_new(&mut self) -> Vec<Value> {
        self.restarted = false;
        let Ok(st) = fs::metadata(&self.path) else {
            return Vec::new();
        };
        let ino = file_id(&self.path, &st);
        if let Some(prev) = self.inode {
            if prev != ino || st.len() < self.offset {
                self.reset();
                self.restarted = true;
            }
        }
        self.inode = Some(ino);
        if st.len() <= self.offset {
            return Vec::new();
        }
        let Ok(mut fh) = fs::File::open(&self.path) else {
            return Vec::new();
        };
        if fh.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut chunk = Vec::with_capacity((st.len() - self.offset) as usize);
        if fh.take(st.len() - self.offset).read_to_end(&mut chunk).is_err() {
            return Vec::new();
        }
        let consumed = chunk.len() as u64;
        let mut data = std::mem::take(&mut self.carry);
        data.extend_from_slice(&chunk);
        let mut lines: Vec<&[u8]> = data.split(|b| *b == b'\n').collect();
        let tail = lines.pop().unwrap_or(&[]);
        if !tail.is_empty() && tail.len() <= MAX_LINE_BYTES {
            self.carry = tail.to_vec();
        }
        self.offset += consumed;
        lines.into_iter().filter_map(parse_row).collect()
    }
}

/// What identifies a file across rewrites: the inode on Unix, the NTFS
/// file index on Windows. `--resume` and compaction put a new file where the
/// old one was, and this is how the tail reader and the archive tell that
/// from an append. Zero means "unknown", which reads as "a different file"
/// and costs a full copy, never a missed rewrite.
#[cfg(unix)]
pub fn file_id(_path: &Path, st: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    st.ino()
}
#[cfg(windows)]
pub fn file_id(path: &Path, _st: &fs::Metadata) -> u64 {
    fs::File::open(path)
        .ok()
        .and_then(|f| winapi_util::file::information(&f).ok())
        .map(|i| i.file_index() ^ i.volume_serial_number().rotate_left(48))
        .unwrap_or(0)
}
#[cfg(not(any(unix, windows)))]
pub fn file_id(_path: &Path, _st: &fs::Metadata) -> u64 {
    0
}

pub fn mtime_secs(st: &fs::Metadata) -> f64 {
    st.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn mtime_ns(st: &fs::Metadata) -> u128 {
    st.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- subagents

/// Where a session's subagent transcripts live: `<project>/<session>/subagents`.
/// The layout is identical inside the archive, so this resolves for both.
pub fn subagent_dir(transcript_path: &Path) -> PathBuf {
    transcript_path.with_extension("").join("subagents")
}

#[derive(Debug, Clone, Default)]
pub struct SubagentRecord {
    pub rows: Vec<Value>,
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    pub path: PathBuf,
}

/// Subagent conversations, keyed by the `toolUseId` that spawned each. Records
/// with no id are collected under the empty key.
pub fn load_subagents(transcript_path: &Path) -> HashMap<String, Vec<SubagentRecord>> {
    let mut out: HashMap<String, Vec<SubagentRecord>> = HashMap::new();
    let folder = subagent_dir(transcript_path);
    let Ok(entries) = fs::read_dir(&folder) else {
        return out;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|e| e == "jsonl").unwrap_or(false)
                && p.file_name().map(|n| n.to_string_lossy().starts_with("agent-")).unwrap_or(false)
        })
        .collect();
    files.sort();
    for jsonl in files {
        let meta = paths::read_json(&jsonl.with_extension("meta.json")).unwrap_or(Value::Null);
        let rows = read_all(&jsonl);
        if rows.is_empty() {
            continue;
        }
        let stem = jsonl.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let record = SubagentRecord {
            rows,
            agent_id: stem.trim_start_matches("agent-").to_string(),
            agent_type: str_of(&meta, "agentType").to_string(),
            description: str_of(&meta, "description").to_string(),
            path: jsonl.clone(),
        };
        let key = str_of(&meta, "toolUseId").to_string();
        out.entry(key).or_default().push(record);
    }
    out
}

// ---------------------------------------------------------------- discovery

/// What we know about a session without parsing all of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRef {
    pub agent: AgentId,
    pub session_id: String,
    pub path: PathBuf,
    pub cwd: String,
    pub project_dir: String,
    pub title: String,
    pub started: String,
    pub updated: String,
    pub size: u64,
    pub mtime: f64,
    pub git_branch: String,
    pub version: String,
    pub slug: String,
    /// True when the agent has already deleted the original and this session
    /// exists only because emaki archived it.
    pub archived: bool,
    pub state: TurnState,
}

impl SessionRef {
    pub fn project(&self) -> String {
        if self.cwd.is_empty() {
            self.project_dir.clone()
        } else {
            paths::project_slug(&self.cwd)
        }
    }
}

pub fn iter_transcripts(projects_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(projects) = fs::read_dir(projects_root) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = projects.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for project in dirs {
        let Ok(files) = fs::read_dir(&project) else { continue };
        let mut jsonls: Vec<PathBuf> = files
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map(|e| e == "jsonl").unwrap_or(false))
            .collect();
        jsonls.sort();
        out.extend(jsonls);
    }
    out
}

/// `peek` results by path, keyed on (size, mtime). The app re-indexes every
/// few seconds; without this every rescan re-reads the tail of every
/// transcript on the machine to learn nothing has changed.
static PEEK_CACHE: LazyLock<Mutex<HashMap<PathBuf, (u64, u128, SessionRef)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// How far back to look for a content row when the normal tail slice has none.
pub const WIDE_TAIL_BYTES: u64 = 8 * 1024 * 1024;

const HEAD_LINES: usize = 40;
const TAIL_BYTES: u64 = 262_144;

/// Cheap metadata read: a few lines from the front, a slice from the back.
pub fn peek(path: &Path) -> SessionRef {
    let mut r = SessionRef {
        agent: AgentId::ClaudeCode,
        session_id: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        path: path.to_path_buf(),
        project_dir: path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        ..Default::default()
    };
    let Ok(st) = fs::metadata(path) else {
        return r;
    };
    let key = (st.len(), mtime_ns(&st));
    if let Ok(cache) = PEEK_CACHE.lock() {
        if let Some((size, mt, hit)) = cache.get(path) {
            if *size == key.0 && *mt == key.1 {
                return hit.clone();
            }
        }
    }
    peek_into(&mut r, &st);
    if let Ok(mut cache) = PEEK_CACHE.lock() {
        cache.insert(path.to_path_buf(), (key.0, key.1, r.clone()));
    }
    r
}

fn tail_rows(fh: &mut fs::File, size: u64, tail_bytes: u64) -> Vec<Value> {
    if size == 0 {
        return Vec::new();
    }
    let start = size.saturating_sub(tail_bytes);
    if fh.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut blob = Vec::new();
    if fh.read_to_end(&mut blob).is_err() {
        return Vec::new();
    }
    let mut pieces: Vec<&[u8]> = blob.split(|b| *b == b'\n').collect();
    if start > 0 && !pieces.is_empty() {
        pieces.remove(0);
    }
    pieces.into_iter().filter_map(parse_row).collect()
}

fn peek_into(r: &mut SessionRef, st: &fs::Metadata) {
    r.size = st.len();
    r.mtime = mtime_secs(st);
    let Ok(mut fh) = fs::File::open(&r.path) else {
        return;
    };
    let mut head: Vec<Value> = Vec::new();
    {
        let mut reader = BufReader::new(&mut fh);
        let mut buf = Vec::new();
        for _ in 0..HEAD_LINES {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if let Some(row) = parse_row(&buf) {
                        head.push(row);
                    }
                }
            }
        }
    }
    let tail = tail_rows(&mut fh, st.len(), TAIL_BYTES);
    let mut state_rows: &[Value] = if tail.is_empty() { &head } else { &tail };
    let wide;
    if turn_state(state_rows, "").phase == Phase::Idle && st.len() > TAIL_BYTES {
        wide = tail_rows(&mut fh, st.len(), st.len().min(WIDE_TAIL_BYTES));
        if !wide.is_empty() {
            state_rows = &wide;
        }
    }

    for row in head.iter().chain(tail.iter()) {
        if r.cwd.is_empty() && !str_of(row, "cwd").is_empty() {
            r.cwd = str_of(row, "cwd").to_string();
        }
        if r.git_branch.is_empty() && !str_of(row, "gitBranch").is_empty() {
            r.git_branch = str_of(row, "gitBranch").to_string();
        }
        if r.version.is_empty() && !str_of(row, "version").is_empty() {
            r.version = str_of(row, "version").to_string();
        }
        if !str_of(row, "sessionId").is_empty() {
            r.session_id = str_of(row, "sessionId").to_string();
        }
        if r.slug.is_empty() && !str_of(row, "slug").is_empty() {
            r.slug = str_of(row, "slug").to_string();
        }
    }
    if let Some(min) = head.iter().map(|x| str_of(x, "timestamp")).filter(|s| !s.is_empty()).min() {
        r.started = min.to_string();
    }
    if let Some(max) = tail.iter().map(|x| str_of(x, "timestamp")).filter(|s| !s.is_empty()).max() {
        r.updated = max.to_string();
    }
    let all: Vec<Value> = head.iter().chain(tail.iter()).cloned().collect();
    r.title = pick_title(&all);
    if r.title.is_empty() {
        r.title = first_prompt_title(&head, 72);
    }
    if r.title.is_empty() {
        r.title = if r.slug.is_empty() { "Untitled session".into() } else { r.slug.clone() };
    }
    r.state = turn_state(state_rows, &r.cwd);
}

static SLUG_SHAPED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)+$").unwrap());

/// The best AI-written title in a set of rows: the newest `ai-title` that is
/// neither a known agent name nor slug-shaped (when a session runs under a
/// named agent, Claude Code writes the agent's name into that field).
pub fn pick_title(rows: &[Value]) -> String {
    let agent_names: std::collections::HashSet<String> =
        rows.iter().map(|r| str_of(r, "agentName").trim().to_string()).filter(|s| !s.is_empty()).collect();
    let titles: Vec<String> = rows
        .iter()
        .filter(|r| str_of(r, "type") == "ai-title")
        .map(|r| str_of(r, "aiTitle").trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    for title in titles.iter().rev() {
        if agent_names.contains(title) || SLUG_SHAPED.is_match(title) {
            continue;
        }
        return title.clone();
    }
    titles.first().cloned().unwrap_or_default()
}

pub fn first_prompt_title(rows: &[Value], limit: usize) -> String {
    for row in rows {
        if str_of(row, "type") != "user" || bool_of(row, "isSidechain") {
            continue;
        }
        let text = user_prompt_text(row);
        if text.is_empty() {
            continue;
        }
        return crate::build::one_line(&text, limit);
    }
    String::new()
}

/// Every Claude Code transcript under `projects_root`, newest first. Stubs
/// smaller than `min_size` are dropped.
pub fn index_claude(projects_root: &Path, min_size: u64) -> Vec<SessionRef> {
    let own = paths::root().to_string_lossy().to_string();
    let mut refs = Vec::new();
    for jsonl in iter_transcripts(projects_root) {
        let Ok(st) = fs::metadata(&jsonl) else { continue };
        if st.len() < min_size {
            continue;
        }
        let r = peek(&jsonl);
        if !r.cwd.is_empty() && paths::is_explainer_cwd(&r.cwd, &own) {
            continue;
        }
        refs.push(r);
    }
    refs
}

/// The bytes of an image block a user row carries, for a thumbnail. Rows are
/// found by their `uuid`; the block is the `index`-th entry of the message's
/// content. When that block is a `tool_result` (a Read of a picture), the
/// picture is the first image inside it. This reads the file, so call it
/// off the main thread.
pub fn image_block_bytes(path: &Path, uuid: &str, index: usize) -> Option<(String, Vec<u8>)> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        if !line.contains(uuid) {
            continue;
        }
        let Ok(row) = serde_json::from_str::<Value>(&line) else { continue };
        if row.get("uuid").and_then(|v| v.as_str()) != Some(uuid) {
            continue;
        }
        let block = row.get("message")?.get("content")?.as_array()?.get(index)?;
        let block = if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
            block.get("content")?.as_array()?.iter().find(|b| b.get("type").and_then(|v| v.as_str()) == Some("image"))?
        } else {
            block
        };
        let src = block.get("source")?;
        let media = src.get("media_type").and_then(|v| v.as_str()).unwrap_or("image/png").to_string();
        let data = src.get("data")?.as_str()?;
        return Some((media, crate::driver::base64_decode(data)?));
    }
    None
}
