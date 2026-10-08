//! What git says about a folder's files, as VS Code's explorer shows it.
//!
//! One `git status` for the repository the folder is in, read into a
//! state per path. A file wears its own state; a folder wears the most
//! pressing state of anything under it. Git names an untracked or ignored
//! folder once and not what it holds, so those are kept as folders and a
//! path under one takes its state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A path's state, in VS Code's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Conflict,
    Untracked,
    Added,
    Renamed,
    Deleted,
    StagedDeleted,
    TypeChanged,
    Modified,
    StagedModified,
    Ignored,
}

impl State {
    /// The letter VS Code sets at a file's right. An ignored file has
    /// none: it is only dimmed.
    pub fn letter(self) -> Option<&'static str> {
        Some(match self {
            State::Conflict => "!",
            State::Untracked => "U",
            State::Added => "A",
            State::Renamed => "R",
            State::Deleted | State::StagedDeleted => "D",
            State::TypeChanged => "T",
            State::Modified | State::StagedModified => "M",
            State::Ignored => return None,
        })
    }

    /// Which of two states a folder holding both wears; the lower wins.
    /// A conflict first, then what is new or gone, then what is changed,
    /// as the explorer has it: a folder with a new file and a changed
    /// one is the new file's colour.
    fn rank(self) -> u8 {
        match self {
            State::Conflict => 0,
            State::Untracked => 1,
            State::Added => 2,
            State::Renamed => 3,
            State::Deleted => 4,
            State::StagedDeleted => 5,
            State::TypeChanged | State::Modified => 6,
            State::StagedModified => 7,
            State::Ignored => 8,
        }
    }
}

/// What a row of the tree wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub state: State,
    /// A folder is marked for what is under it, with a dot and no letter.
    pub folder: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Paths git named one by one.
    files: HashMap<PathBuf, State>,
    /// Folders git named whole, untracked or ignored.
    whole: HashMap<PathBuf, State>,
    /// Every folder with a change somewhere under it.
    holding: HashMap<PathBuf, State>,
    /// Something in the repository is not committed, here or outside
    /// the folder asked about: what a change of branch has to ask of.
    pub dirty: bool,
}

fn git(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_OPTIONAL_LOCKS", "0").output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Whether `dir` is in a repository as far as the window goes: inside
/// one, and not a folder that repository ignores. A build folder under
/// a checkout is on that checkout's disk and none of its business, so
/// it has no branch, no marks and no changes.
pub fn inside(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--git-dir"]).is_some() && git(dir, &["check-ignore", "-q", "."]).is_none()
}

/// The state of what is under `dir` in the repository it is in, or
/// `None` when it is in none (`inside`). A folder part way down a
/// repository is asked about by itself, as VS Code shows one: the
/// changes counted are the ones under it. Paths are spelled as `dir`
/// is: the repository's top is `dir` less its own place in the
/// repository, not git's resolved path, which differs behind a symlink.
pub fn status(dir: &Path) -> Option<Status> {
    if !inside(dir) {
        return None;
    }
    let prefix = String::from_utf8_lossy(&git(dir, &["rev-parse", "--show-prefix"])?).trim_end_matches(['\r', '\n']).to_string();
    let mut top = dir;
    for _ in Path::new(&prefix).components() {
        top = top.parent()?;
    }
    // Every untracked file by itself, as VS Code asks for them, so the
    // count of changes is the count of files; an ignored folder stays
    // one entry, or `target/` would be listed file by file.
    let out = git(dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignored=matching", "--", "."])?;
    let mut status = parse(top, &String::from_utf8_lossy(&out));
    status.dirty = status.changed_count() > 0 || (!prefix.is_empty() && dirty(dir));
    Some(status)
}

/// `git status --porcelain=v1 -z` as states, under the repository's top.
pub fn parse(top: &Path, porcelain: &str) -> Status {
    let mut st = Status::default();
    let mut parts = porcelain.split('\0');
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let (xy, path) = (entry[..2].as_bytes(), &entry[3..]);
        let (x, y) = (xy[0], xy[1]);
        // A rename or a copy is followed by the path it came from.
        if x == b'R' || x == b'C' {
            parts.next();
        }
        let state = match (x, y) {
            (b'?', b'?') => State::Untracked,
            (b'!', b'!') => State::Ignored,
            (b'D', b'D') | (b'A', b'A') | (b'U', _) | (_, b'U') => State::Conflict,
            (_, b'M') => State::Modified,
            (_, b'D') => State::Deleted,
            (_, b'T') => State::TypeChanged,
            (_, b'A') => State::Added,
            (b'M', _) => State::StagedModified,
            (b'T', _) => State::TypeChanged,
            (b'A', _) => State::Added,
            (b'D', _) => State::StagedDeleted,
            (b'R', _) | (b'C', _) => State::Renamed,
            _ => continue,
        };
        let abs = top.join(path.trim_end_matches('/'));
        if path.ends_with('/') {
            st.whole.insert(abs.clone(), state);
        } else {
            st.files.insert(abs.clone(), state);
        }
        if state == State::Ignored {
            continue;
        }
        let mut up = abs.parent();
        while let Some(dir) = up {
            if !dir.starts_with(top) {
                break;
            }
            let held = st.holding.entry(dir.to_path_buf()).or_insert(state);
            if state.rank() < held.rank() {
                *held = state;
            }
            up = dir.parent();
        }
    }
    st
}

impl Status {
    /// What the row for `path` wears, if anything.
    pub fn mark(&self, path: &Path, dir: bool) -> Option<Mark> {
        if !dir {
            if let Some(&state) = self.files.get(path) {
                return Some(Mark { state, folder: false });
            }
        }
        // Inside, or itself, a folder git named whole.
        let mut at = if dir { Some(path) } else { path.parent() };
        while let Some(p) = at {
            if let Some(&state) = self.whole.get(p) {
                return Some(Mark { state, folder: dir });
            }
            at = p.parent();
        }
        if dir {
            return self.holding.get(path).map(|&state| Mark { state, folder: true });
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.whole.is_empty()
    }

    /// Every file with a change, by path: what a commit would be asked
    /// about. Nothing ignored.
    pub fn changed(&self) -> Vec<(PathBuf, State)> {
        let mut out: Vec<(PathBuf, State)> = self.files.iter().filter(|(_, s)| **s != State::Ignored).map(|(p, s)| (p.clone(), *s)).collect();
        out.sort();
        out
    }

    pub fn changed_count(&self) -> usize {
        self.files.values().filter(|s| **s != State::Ignored).count()
    }
}

/// A repository's branches, as a picker lists them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Branches {
    /// The branch checked out; none with the head detached.
    pub current: Option<String>,
    /// The commit checked out, short; empty before the first commit.
    pub head: String,
    /// The branch the remote calls its head, else `main` or `master`.
    pub default: Option<String>,
    /// Local branches: the default first, the rest by name.
    pub local: Vec<String>,
    /// Branches a remote has and this repository does not, by the name
    /// a switch would give them.
    pub remote: Vec<String>,
    /// When each branch's last commit was made, in seconds since 1970.
    pub when: std::collections::HashMap<String, i64>,
    /// How many sets of changes were left behind on the branch checked
    /// out (`Carry::Leave`) and are still waiting to be put back.
    pub stashed: usize,
}

impl Branches {
    /// What a button naming the place says: the branch, else the commit.
    pub fn label(&self) -> String {
        self.current.clone().unwrap_or_else(|| self.head.clone())
    }

    /// Every branch by the time of its last commit, the newest first,
    /// after the default branch, which is first in any order; each with
    /// whether only a remote has it.
    pub fn by_recency(&self) -> Vec<(String, bool)> {
        let mut all: Vec<(String, bool)> = self.local.iter().map(|n| (n.clone(), false)).chain(self.remote.iter().map(|n| (n.clone(), true))).collect();
        all.sort_by_key(|(n, _)| (Some(n) != self.default.as_ref(), std::cmp::Reverse(self.when.get(n).copied().unwrap_or(0)), n.clone()));
        all
    }

    pub fn has(&self, name: &str) -> bool {
        self.local.iter().chain(&self.remote).any(|b| b == name)
    }
}

fn line(dir: &Path, args: &[&str]) -> Option<String> {
    let out = String::from_utf8_lossy(&git(dir, args)?).trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// The branches of the repository `dir` is in, or `None` when it is in
/// none (`inside`).
pub fn branches(dir: &Path) -> Option<Branches> {
    if !inside(dir) {
        return None;
    }
    let refs = String::from_utf8_lossy(&git(dir, &["for-each-ref", "--format=%(refname)%09%(committerdate:unix)", "refs/heads", "refs/remotes"])?).to_string();
    let mut b = Branches { current: line(dir, &["symbolic-ref", "--short", "-q", "HEAD"]), head: line(dir, &["rev-parse", "--short", "HEAD"]).unwrap_or_default(), ..Default::default() };
    for row in refs.lines() {
        let (name, at) = row.split_once('\t').unwrap_or((row, ""));
        let at = at.trim().parse::<i64>().unwrap_or(0);
        if let Some(local) = name.strip_prefix("refs/heads/") {
            b.local.push(local.to_string());
            // The local branch's own time, whatever a remote's copy says.
            b.when.insert(local.to_string(), at);
        } else if let Some((_, branch)) = name.strip_prefix("refs/remotes/").and_then(|r| r.split_once('/')) {
            if branch != "HEAD" {
                b.remote.push(branch.to_string());
                if !refs.contains(&format!("refs/heads/{branch}\t")) {
                    let seen = b.when.entry(branch.to_string()).or_insert(at);
                    *seen = (*seen).max(at);
                }
            }
        }
    }
    // Before the first commit the branch checked out has no ref yet.
    if let Some(cur) = &b.current {
        if !b.local.contains(cur) {
            b.local.push(cur.clone());
        }
    }
    b.remote.retain(|r| !b.local.contains(r));
    b.remote.sort();
    b.remote.dedup();
    b.default = line(dir, &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"])
        .and_then(|h| h.split_once('/').map(|(_, name)| name.to_string()))
        .or_else(|| ["main", "master"].iter().find(|n| b.local.iter().any(|l| l == *n)).map(|n| n.to_string()));
    let default = b.default.clone();
    b.local.sort_by_key(|name| (Some(name) != default.as_ref(), name.clone()));
    b.stashed = b.current.as_ref().map_or(0, |cur| left_on(dir, cur).len());
    Some(b)
}

/// Run a git command that changes the checkout; its own words when it
/// refuses.
fn act(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|e| format!("git did not run: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = said.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    // "error: …" is the reason; the lines after it list files and advice.
    let why = lines.iter().find(|l| l.starts_with("error:") || l.starts_with("fatal:")).or(lines.first()).copied().unwrap_or("git refused");
    Err(why.trim_start_matches("error:").trim_start_matches("fatal:").trim().to_string())
}

/// Check `name` out. A branch only a remote has becomes a local one
/// that follows it, as `git switch` does it. Git refuses when what is
/// changed here would be written over, and nothing is forced.
pub fn switch(dir: &Path, name: &str) -> Result<(), String> {
    act(dir, &["switch", name])
}

/// Make the branch `name` from what is checked out, and check it out.
pub fn create(dir: &Path, name: &str) -> Result<(), String> {
    act(dir, &["switch", "-c", name])
}

/// What becomes of the changes not yet committed when the branch is
/// changed, as GitHub Desktop asks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carry {
    /// They stay with the branch being left, in a stash named for it,
    /// and are put back from there (`restore`).
    Leave,
    /// They come along to the branch gone to.
    Bring,
}

/// The name GitHub Desktop gives the stash of the changes left on a
/// branch, so that app and this one see the same ones.
const LEFT_MARK: &str = "!!GitHub_Desktop";

fn left_name(branch: &str) -> String {
    format!("{LEFT_MARK}<{branch}>")
}

/// The stashes holding changes left on `branch`, newest first, by the
/// name git takes for each (`stash@{0}`).
fn left_on(dir: &Path, branch: &str) -> Vec<String> {
    let Some(out) = git(dir, &["stash", "list", "--format=%gd%x1f%gs"]) else { return Vec::new() };
    let name = left_name(branch);
    String::from_utf8_lossy(&out).lines().filter_map(|l| l.split_once('\u{1f}')).filter(|(_, said)| said.ends_with(&name)).map(|(at, _)| at.to_string()).collect()
}

/// Whether anything here is not committed: changed, staged, or new and
/// not ignored.
pub fn dirty(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain", "--untracked-files=normal"]).is_some_and(|out| !out.iter().all(u8::is_ascii_whitespace))
}

/// The files with a conflict git is still waiting to have resolved.
pub fn unmerged(dir: &Path) -> Vec<String> {
    git(dir, &["diff", "--name-only", "--diff-filter=U"]).map(|out| String::from_utf8_lossy(&out).lines().map(str::to_string).collect()).unwrap_or_default()
}

/// A few of `files` by name, for a sentence: "a, b and 3 more".
pub fn name_some(files: &[String]) -> String {
    let shown: Vec<&str> = files.iter().take(4).map(String::as_str).collect();
    match files.len() - shown.len() {
        0 => shown.join(", "),
        more => format!("{} and {more} more", shown.join(", ")),
    }
}

/// The commit `name` would check out: the local branch, else a remote's.
fn tip(dir: &Path, name: &str) -> Option<String> {
    line(dir, &["rev-parse", "--verify", "-q", &format!("refs/heads/{name}^{{commit}}")]).or_else(|| {
        let refs = git(dir, &["for-each-ref", "--format=%(refname)", "refs/remotes"])?;
        let found = String::from_utf8_lossy(&refs).lines().find(|r| r.strip_prefix("refs/remotes/").and_then(|r| r.split_once('/')).is_some_and(|(_, b)| b == name))?.to_string();
        line(dir, &["rev-parse", "--verify", "-q", &format!("{found}^{{commit}}")])
    })
}

/// The files whose changes here would not go onto the branch `name`:
/// asked of git without touching a file (`stash create` makes a commit
/// of the changes and leaves them where they are, `merge-tree` merges
/// in memory). Empty when they all fit, which is what `Carry::Bring`
/// needs. A new file counts when the branch has a file of its name.
pub fn misfits(dir: &Path, name: &str) -> Vec<String> {
    let Some(theirs) = tip(dir, name) else { return Vec::new() };
    let mut bad = Vec::new();
    if let Some(mine) = line(dir, &["stash", "create"]) {
        let out = Command::new("git").arg("-C").arg(dir).args(["merge-tree", "--write-tree", "--name-only", "--no-messages", "--merge-base=HEAD", &theirs, &mine]).output();
        match out {
            // Exit 1 is "merged, with conflicts": the tree's id, then the files.
            Ok(o) if o.status.code() == Some(1) => bad.extend(String::from_utf8_lossy(&o.stdout).lines().skip(1).filter(|l| !l.is_empty()).map(str::to_string)),
            Ok(o) if o.status.success() => {}
            // A git too old to merge in memory: every changed file the
            // two branches differ in counts, which is what git itself
            // refuses a plain switch over.
            _ => {
                let differ = git(dir, &["diff", "--name-only", "HEAD", &theirs]).map(|o| String::from_utf8_lossy(&o).lines().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
                let changed = git(dir, &["diff", "--name-only", "HEAD"]).map(|o| String::from_utf8_lossy(&o).lines().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
                bad.extend(changed.into_iter().filter(|c| differ.contains(c)));
            }
        }
    }
    let fresh = git(dir, &["ls-files", "--others", "--exclude-standard"]).map(|o| String::from_utf8_lossy(&o).lines().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    if !fresh.is_empty() {
        let there: std::collections::HashSet<String> = git(dir, &["ls-tree", "-r", "--name-only", &theirs]).map(|o| String::from_utf8_lossy(&o).lines().map(str::to_string).collect()).unwrap_or_default();
        bad.extend(fresh.into_iter().filter(|f| there.contains(f)));
    }
    bad.sort();
    bad.dedup();
    bad
}

/// Check `name` out (or make it, with `create`) with changes not yet
/// committed, which go where `carry` says. All or nothing: either the
/// branch is changed and every change is where it was asked to go, or
/// nothing has moved and the reason comes back. No path leaves a
/// conflict behind, and none throws a change away.
pub fn switch_with(dir: &Path, name: &str, create: bool, carry: Carry) -> Result<(), String> {
    let open = unmerged(dir);
    if !open.is_empty() {
        return Err(format!("{} to resolve first: {}", if open.len() == 1 { "a file has a conflict".to_string() } else { format!("{} files have conflicts", open.len()) }, name_some(&open)));
    }
    let go = |dir: &Path| if create { self::create(dir, name) } else { switch(dir, name) };
    if !dirty(dir) {
        return go(dir);
    }
    // Where to come back to if the changes cannot be put down.
    let from = line(dir, &["symbolic-ref", "--short", "-q", "HEAD"]);
    match carry {
        Carry::Leave => {
            let from = from.ok_or("no branch is checked out to leave the changes on")?;
            act(dir, &["stash", "push", "--include-untracked", "-m", &left_name(&from)])?;
            // Not switched after all: the changes go back where they were.
            go(dir).inspect_err(|_| {
                let _ = act(dir, &["stash", "pop"]);
            })
        }
        Carry::Bring => {
            if !create {
                let bad = misfits(dir, name);
                if !bad.is_empty() {
                    return Err(format!("your changes do not fit on {name}: {} would conflict", name_some(&bad)));
                }
            }
            // Git takes changes along by itself when none of them is to
            // a file the two branches differ in.
            if go(dir).is_ok() {
                return Ok(());
            }
            let back = from.clone().or_else(|| line(dir, &["rev-parse", "HEAD"])).ok_or("nothing is checked out")?;
            act(dir, &["stash", "push", "--include-untracked", "-m", &format!("Emaki: changes brought to {name}")])?;
            go(dir).inspect_err(|_| {
                let _ = act(dir, &["stash", "pop"]);
            })?;
            if act(dir, &["stash", "pop"]).is_ok() {
                return Ok(());
            }
            // They did not go on after all (the check above should have
            // said so). Git kept the stash, so the half-applied files are
            // cleared, the branch left is checked out again, and the
            // changes are put back on it.
            let fresh = git(dir, &["ls-tree", "-r", "--name-only", "stash@{0}^3"]).map(|o| String::from_utf8_lossy(&o).lines().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
            act(dir, &["reset", "--hard", "-q"])?;
            for f in fresh {
                let _ = std::fs::remove_file(dir.join(f));
            }
            if create {
                act(dir, &["switch", "-q", if from.is_some() { &back } else { "--detach" }])?;
                let _ = act(dir, &["branch", "-D", name]);
            } else if from.is_some() {
                act(dir, &["switch", "-q", &back])?;
            } else {
                act(dir, &["switch", "-q", "--detach", &back])?;
            }
            act(dir, &["stash", "pop"])?;
            Err(format!("your changes do not fit on {name}, so nothing was switched"))
        }
    }
}

/// Put back the changes last left on the branch checked out. Git
/// refuses, and keeps them, when they would write over a change here.
pub fn restore(dir: &Path) -> Result<(), String> {
    let cur = line(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).ok_or("no branch is checked out")?;
    let at = left_on(dir, &cur).into_iter().next().ok_or("nothing was left on this branch")?;
    act(dir, &["stash", "pop", &at])
}

/// Whether git would take `name` as a branch's name.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && Command::new("git").args(["check-ref-format", "--branch", name]).output().is_ok_and(|o| o.status.success())
}

/// The most lines of a comparison that are kept, and the most of a file
/// with no earlier version that is read.
const DIFF_LINES: usize = 5000;
const DIFF_BYTES: usize = 400_000;

/// A line of a comparison, set side by side: what was on the left, what
/// is on the right, each with its number in its own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// Where a stretch begins: git's "@@ -68,20 +68,24 @@" and the
    /// words after it.
    Hunk(String),
    Same { old: u32, new: u32, text: String },
    /// A line taken out, put in, or one of each side by side.
    Changed { left: Option<(u32, String)>, right: Option<(u32, String)> },
}

/// One file's changes against the last commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub rows: Vec<Row>,
    pub added: usize,
    pub removed: usize,
    /// Not text: there are no lines to set side by side.
    pub binary: bool,
    /// Longer than is kept; the rows are its beginning.
    pub cut: bool,
}

/// What `path` holds now against what the last commit has. A file git
/// does not track yet is all new lines.
pub fn diff(dir: &Path, path: &Path, state: State) -> Diff {
    if state == State::Untracked {
        return whole_file(path);
    }
    let run = |base: &[&str]| Command::new("git").arg("-C").arg(dir).args(["diff", "--no-color", "--no-ext-diff"]).args(base).arg("--").arg(path).env("GIT_OPTIONAL_LOCKS", "0").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    // Before the first commit there is no HEAD to compare with, and
    // what is staged is all there is.
    match run(&["HEAD"]).or_else(|| run(&["--cached"])) {
        Some(text) => parse_diff(&text),
        None => Diff::default(),
    }
}

fn whole_file(path: &Path) -> Diff {
    let Ok(bytes) = std::fs::read(path) else { return Diff::default() };
    let cut = bytes.len() > DIFF_BYTES;
    let head = &bytes[..bytes.len().min(DIFF_BYTES)];
    if head.contains(&0) {
        return Diff { binary: true, ..Default::default() };
    }
    let text = String::from_utf8_lossy(head);
    let mut d = Diff { cut, ..Default::default() };
    for (ix, line) in text.lines().enumerate() {
        if ix >= DIFF_LINES {
            d.cut = true;
            break;
        }
        d.rows.push(Row::Changed { left: None, right: Some((ix as u32 + 1, clean(line))) });
        d.added += 1;
    }
    d
}

/// A line as it is drawn: a tab is four spaces, and no carriage return.
fn clean(line: &str) -> String {
    line.trim_end_matches('\r').replace('\t', "    ")
}

/// `git diff` for one file as rows. Lines taken out and the lines put
/// in after them sit side by side, the first with the first.
pub fn parse_diff(text: &str) -> Diff {
    let mut d = Diff::default();
    let (mut old, mut new) = (0u32, 0u32);
    let mut inside = false;
    // The removed lines of the run being read, and where its rows began.
    let mut pending: Vec<usize> = Vec::new();
    for line in text.lines() {
        if d.rows.len() >= DIFF_LINES {
            d.cut = true;
            break;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            let nums = rest.split(" @@").next().unwrap_or("");
            let start = |sign: char| nums.split(' ').find_map(|p| p.strip_prefix(sign)).and_then(|p| p.split(',').next()).and_then(|n| n.parse::<u32>().ok()).unwrap_or(1);
            (old, new) = (start('-'), start('+'));
            inside = true;
            pending.clear();
            d.rows.push(Row::Hunk(line.to_string()));
            continue;
        }
        if !inside {
            if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
                d.binary = true;
            }
            continue;
        }
        let (mark, body) = line.split_at(line.len().min(1));
        match mark {
            "-" => {
                pending.push(d.rows.len());
                d.rows.push(Row::Changed { left: Some((old, clean(body))), right: None });
                old += 1;
                d.removed += 1;
            }
            "+" => {
                let put = (new, clean(body));
                // Beside the first removed line still alone, else a row of its own.
                match (!pending.is_empty()).then(|| pending.remove(0)) {
                    Some(at) => {
                        if let Row::Changed { right, .. } = &mut d.rows[at] {
                            *right = Some(put);
                        }
                    }
                    None => d.rows.push(Row::Changed { left: None, right: Some(put) }),
                }
                new += 1;
                d.added += 1;
            }
            // "\ No newline at end of file" is about the line before it.
            "\\" => {}
            _ => {
                pending.clear();
                d.rows.push(Row::Same { old, new, text: clean(body) });
                old += 1;
                new += 1;
            }
        }
    }
    d
}
