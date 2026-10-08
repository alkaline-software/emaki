//! The archive: a permanent, byte-for-byte copy of every transcript.
//!
//! This is the reason Emaki exists. Claude Code deletes transcripts older
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
//! While the agent still has a file, the archived copy costs next to no
//! room: where the filesystem can, it is a clone, a second name for the same
//! blocks, made again whenever the source has grown. A clone is a picture of
//! the source as it was, not a link to it: nothing done to the source later
//! reaches it, and when the agent deletes the source the clone is what
//! holds the blocks. Elsewhere the copy is bytes, and what is new is
//! appended. A source that was rewritten or truncated underneath us never
//! overwrites an archived copy; the existing archive is rotated to a
//! generation file first.
//!
//! Once the agent has deleted its file the archived copy is the only one,
//! takes its full size and never changes again, so it is packed, once, by
//! the filesystem's own compression (`pack_stale`). The file keeps its name
//! and reads back the same bytes with any program.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::model::AgentId;
use crate::paths;
use crate::transcript::{file_id, mtime_secs, SessionRef};

pub const SIDECAR_DIRS: &[&str] = &["subagents", "tool-results"];
pub const MAX_COPY_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub sessions: u64,
    pub files: u64,
    pub bytes: u64,
    pub rotated: u64,
    pub errors: u64,
    /// Copies made before there were clones, looked at again: each is
    /// a clone now where the filesystem can.
    pub shared: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    size: u64,
    inode: u64,
    mtime: f64,
    dst: String,
    seen: f64,
    /// The copy has been made a clone, or tried: one made as bytes by an
    /// earlier Emaki is tried once.
    shared: bool,
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

/// Put a clone of `src` where `dst` is: a second name for the same blocks,
/// which takes no room of its own while the source is there and is not
/// touched by anything done to the source afterwards. Made beside `dst`
/// and moved over it, so a reader never sees half of one. `false` where
/// the filesystem cannot (not APFS, another volume) and on every other
/// system, where the caller copies the bytes.
#[cfg(target_os = "macos")]
fn clone_over(src: &Path, dst: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Some(name) = dst.file_name() else { return false };
    let tmp = dst.with_file_name(format!(".{}.cloning", name.to_string_lossy()));
    let _ = fs::remove_file(&tmp);
    let (Ok(from), Ok(to)) = (std::ffi::CString::new(src.as_os_str().as_bytes()), std::ffi::CString::new(tmp.as_os_str().as_bytes())) else { return false };
    // SAFETY: both are NUL-terminated paths that live past the call.
    if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } != 0 {
        return false;
    }
    if fs::rename(&tmp, dst).is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    true
}
#[cfg(not(target_os = "macos"))]
fn clone_over(_src: &Path, _dst: &Path) -> bool {
    false
}

/// Whether what the archive holds is still how the source begins: its
/// last stretch, read from both. A source that grew is taken whole again
/// only then; one whose earlier rows were changed in place is a rewrite,
/// and the archived copy is set aside first.
fn still_begins_with(src: &Path, dst: &Path) -> bool {
    use std::io::Read;
    const SAMPLE: u64 = 64 * 1024;
    let (Ok(have), Ok(now)) = (fs::metadata(dst).map(|m| m.len()), fs::metadata(src).map(|m| m.len())) else { return false };
    if have > now {
        return false;
    }
    let from = have.saturating_sub(SAMPLE);
    let read = |path: &Path| -> Option<Vec<u8>> {
        let mut f = fs::File::open(path).ok()?;
        f.seek(SeekFrom::Start(from)).ok()?;
        let mut buf = Vec::new();
        f.take(have - from).read_to_end(&mut buf).ok()?;
        Some(buf)
    };
    matches!((read(src), read(dst)), (Some(a), Some(b)) if a == b)
}

/// Whether two files hold the same bytes, all of them read.
fn same_bytes(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    let (Ok(fa), Ok(fb)) = (fs::File::open(a), fs::File::open(b)) else { return false };
    if !matches!((fa.metadata(), fb.metadata()), (Ok(x), Ok(y)) if x.len() == y.len()) {
        return false;
    }
    let (mut ra, mut rb) = (std::io::BufReader::with_capacity(1 << 20, fa), std::io::BufReader::with_capacity(1 << 20, fb));
    let (mut ba, mut bb) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    loop {
        let Ok(n) = ra.read(&mut ba) else { return false };
        if n == 0 {
            return true;
        }
        if rb.read_exact(&mut bb[..n]).is_err() || ba[..n] != bb[..n] {
            return false;
        }
    }
}

/// Whether every row the archived copy holds is in the source too, so
/// that putting the source in its place loses nothing. A last row cut
/// short is not counted: half a row is no row. This is what tells a copy
/// that is merely behind, or was put together wrongly, from one that holds
/// history the source no longer has, which is kept as a generation file.
fn nothing_lost(src: &Path, dst: &Path) -> bool {
    use std::collections::HashSet;
    use std::hash::{Hash, Hasher};
    use std::io::BufRead;
    let key = |line: &[u8]| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        line.hash(&mut h);
        (h.finish(), line.len())
    };
    let lines = |path: &Path, each: &mut dyn FnMut(&[u8], bool) -> bool| -> bool {
        let Ok(f) = fs::File::open(path) else { return false };
        let mut reader = std::io::BufReader::with_capacity(1 << 20, f);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => return true,
                Ok(_) => {
                    let whole = buf.ends_with(b"\n");
                    if !each(buf.strip_suffix(b"\n").unwrap_or(&buf), whole) {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
    };
    let mut have: HashSet<(u64, usize)> = HashSet::new();
    if !lines(src, &mut |line, _| {
        have.insert(key(line));
        true
    }) {
        return false;
    }
    lines(dst, &mut |line, whole| !whole || line.is_empty() || have.contains(&key(line)))
}

/// Take whatever of `src` we do not already have in `dst`. Keyed on
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
    let ino = file_id(src, &st);
    let same_file = have && prev.inode == ino && state.contains_key(&key);

    let result: std::io::Result<u64> = (|| {
        if same_file && st.len() == prev.size {
            // A copy an earlier Emaki made as bytes takes its full room
            // beside the source. Once, it is made a clone of the source it
            // still matches.
            // It is also where a copy an earlier Emaki put together
            // wrongly is found: a source rewritten in place and longer
            // was taken as one that had grown, and its new end was put
            // after the old copy, which then had rows twice or lacked
            // some. Such a copy is not the source and is taken again.
            if !prev.shared {
                stats.shared += 1;
                if let Some(e) = state.get_mut(&key) {
                    e.shared = true;
                }
                if same_bytes(src, dst) {
                    let _ = clone_over(src, dst);
                    return Ok(0);
                }
                if !nothing_lost(src, dst) && rotate(dst).is_some() {
                    stats.rotated += 1;
                }
                if !clone_over(src, dst) {
                    fs::copy(src, dst)?;
                }
                return Ok(st.len());
            }
            return Ok(0);
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if same_file && st.len() > prev.size && still_begins_with(src, dst) {
            let grown = st.len() - prev.size;
            // The source as it is now, in the place of the source as it was.
            if clone_over(src, dst) {
                return Ok(grown);
            }
            let at = fs::metadata(dst)?.len();
            let mut fin = fs::File::open(src)?;
            fin.seek(SeekFrom::Start(at))?;
            let mut fout = fs::OpenOptions::new().append(true).open(dst)?;
            let copied = std::io::copy(&mut fin, &mut fout)?;
            fout.flush()?;
            return Ok(copied);
        }
        // What is there is set aside when it holds a row the source does
        // not. One that is only behind the source is not history.
        if have && !nothing_lost(src, dst) && rotate(dst).is_some() {
            stats.rotated += 1;
        }
        if !clone_over(src, dst) {
            fs::copy(src, dst)?;
        }
        Ok(st.len())
    })();

    match result {
        Ok(0) if same_file => {}
        Ok(copied) => {
            stats.files += 1;
            stats.bytes += copied;
            state.insert(key, Entry { size: st.len(), inode: ino, mtime: mtime_secs(&st), dst: dst.to_string_lossy().to_string(), seen: now(), shared: true });
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
    if stats.files > 0 || stats.rotated > 0 || stats.shared > 0 {
        save_state(&state);
    }
    stats
}

pub fn archive_one(r: &SessionRef) -> Stats {
    sweep(std::slice::from_ref(r))
}

/// How long a copy the agent no longer has is left as it is before it is
/// packed: a source that is only away for a moment comes back to a file
/// nothing was done to.
const PACK_AFTER_SECS: f64 = 24.0 * 3600.0;
/// Under this a file is not worth packing: it fits a block or two.
const PACK_MIN_BYTES: u64 = 16 * 1024;

/// Pack the sessions only the archive has: each one's transcript and the
/// files beside it, once, by the filesystem's own compression, so the
/// file keeps its name and any program reads the same bytes out of it.
/// How many files were packed. Nothing is done on a filesystem without
/// such compression, and a file is never left half done: on a Mac the
/// packed copy is made beside it, read back, compared byte for byte, and
/// only then moved over it.
pub fn pack_stale(refs: &[SessionRef]) -> u64 {
    let mut packed = 0;
    for r in refs.iter().filter(|r| r.archived && now() - r.mtime > PACK_AFTER_SECS && is_inside_archive(&r.path)) {
        packed += u64::from(pack_file(&r.path));
        let beside = r.path.with_extension("");
        for name in SIDECAR_DIRS {
            pack_tree(&beside.join(name), &mut packed);
        }
    }
    packed
}

fn pack_tree(dir: &Path, packed: &mut u64) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok).map(|e| e.path()) {
        if entry.is_dir() {
            pack_tree(&entry, packed);
        } else if entry.is_file() {
            *packed += u64::from(pack_file(&entry));
        }
    }
}

/// Whether the filesystem already keeps the file packed.
#[cfg(target_os = "macos")]
fn is_packed(st: &fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const UF_COMPRESSED: u32 = 0x20;
    st.st_flags() & UF_COMPRESSED != 0
}
#[cfg(windows)]
fn is_packed(st: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
    st.file_attributes() & FILE_ATTRIBUTE_COMPRESSED != 0
}
#[cfg(not(any(target_os = "macos", windows)))]
fn is_packed(_st: &fs::Metadata) -> bool {
    true
}

/// Pack one file where it is. On a Mac `ditto` writes a copy the
/// filesystem keeps compressed (the same thing the system does to its own
/// files); on Windows `compact` asks NTFS to. Linux has no one way to ask,
/// so a file there stays as it is.
fn pack_file(path: &Path) -> bool {
    let Ok(st) = fs::metadata(path) else { return false };
    if st.len() < PACK_MIN_BYTES || is_packed(&st) {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        let Some(name) = path.file_name() else { return false };
        let tmp = path.with_file_name(format!(".{}.packing", name.to_string_lossy()));
        let _ = fs::remove_file(&tmp);
        // Without `--noclone` the copy is a clone and nothing is packed.
        let made = std::process::Command::new("/usr/bin/ditto").arg("--hfsCompression").arg("--noclone").arg(path).arg(&tmp).output().is_ok_and(|o| o.status.success());
        let good = made && fs::metadata(&tmp).is_ok_and(|t| is_packed(&t)) && same_bytes(path, &tmp);
        if !good || fs::rename(&tmp, path).is_err() {
            let _ = fs::remove_file(&tmp);
            return false;
        }
        true
    }
    #[cfg(windows)]
    {
        std::process::Command::new("compact").arg("/c").arg("/q").arg(path).output().is_ok_and(|o| o.status.success())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        false
    }
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
