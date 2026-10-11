//! What git says about a folder's files, as VS Code's explorer shows it.
//!
//! One `git status` for the repository the folder is in, read into a
//! state per path. A file wears its own state; a folder wears the most
//! pressing state of anything under it. Git names an untracked or ignored
//! folder once and not what it holds, so those are kept as folders and a
//! path under one takes its state.

use std::collections::HashMap;
use std::ops::Range;
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

/// The git to run: the first on `PATH` that answers `--version`, then
/// where the installers put one when `PATH` does not say. An app opened
/// from the Finder or the Dock has the system's `PATH` and not the
/// shell's, so a git from Homebrew is on no path it knows; and the Mac's
/// own `/usr/bin/git` is a stand-in that only works with Xcode or its
/// command line tools installed and their licence agreed to, and
/// otherwise fails every call, so being there is not enough. That one is
/// not even asked without the tools: asking puts up the system's offer
/// to install them. One found is kept for the process. With none it is
/// plain `git`, which fails as it did (`trouble` says why), and the
/// search is made again `SEARCH_AGAIN` later: a licence agreed to in a
/// terminal is seen without a relaunch.
pub fn binary() -> PathBuf {
    static FOUND: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    static TRIED: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    if let Some(found) = FOUND.get() {
        return found.clone();
    }
    let mut tried = TRIED.lock().unwrap_or_else(|e| e.into_inner());
    if tried.is_some_and(|at| at.elapsed() < SEARCH_AGAIN) {
        return PathBuf::from("git");
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(var) {
                let root = PathBuf::from(root);
                dirs.push(if var == "LOCALAPPDATA" { root.join("Programs").join("Git").join("cmd") } else { root.join("Git").join("cmd") });
            }
        }
    } else {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    }
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let answers = |p: &Path| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success());
    let found = dirs.into_iter().map(|dir| dir.join(name)).find(|p| p.is_file() && (p.as_path() != Path::new(MAC_GIT) || mac_tools()) && answers(p));
    match found {
        Some(found) => FOUND.get_or_init(|| found).clone(),
        None => {
            *tried = Some(std::time::Instant::now());
            PathBuf::from("git")
        }
    }
}

/// How long a search that found no git stands before another is made.
const SEARCH_AGAIN: std::time::Duration = std::time::Duration::from_secs(10);

/// The Mac's stand-in for git.
const MAC_GIT: &str = "/usr/bin/git";

/// Whether the Mac's stand-in has something to stand in for: Xcode or
/// its command line tools. True off the Mac, where there is none.
fn mac_tools() -> bool {
    !cfg!(target_os = "macos") || Command::new("/usr/bin/xcode-select").arg("-p").output().is_ok_and(|o| o.status.success())
}

/// Why git says nothing of a checkout (`trouble`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trouble {
    /// Git's or the system's own words; ours, and short, for the one
    /// trouble that comes with a command, which says the rest.
    pub words: String,
    /// The command that puts it right, when the trouble is one known for
    /// certain and has one.
    pub fix: Option<&'static str>,
}

impl Trouble {
    /// Xcode's licence not agreed to. Apple's message is three lines
    /// long and spells the command out; the command is offered beside
    /// these words, so they only say what is wrong and where to run it.
    pub fn licence() -> Trouble {
        Trouble { words: "The Xcode license has not been accepted. In Terminal, run:".to_string(), fix: Some(XCODE_LICENCE) }
    }
}

/// What Apple's stand-in says to run when Xcode's licence has not been
/// agreed to, as its own message has it.
pub const XCODE_LICENCE: &str = "sudo xcodebuild -license";

/// Whether what a git on a Mac said is the Xcode licence's refusal, and
/// nothing else: Apple's message names the command, and no other
/// failure does.
pub fn xcode_licence(mac: bool, words: &str) -> bool {
    mac && words.contains("xcodebuild -license")
}

/// Why git says nothing of `dir` though it is a checkout by its own
/// files (a `.git` in it or above it), in git's or the system's own
/// words; `None` when it is no checkout, or git answers for it. No git
/// that runs: none installed, or the Mac's with Xcode's licence not
/// agreed to. Or a git that runs and refuses the folder: one owned by
/// someone else ("dubious ownership"), a damaged repository. Only the
/// licence comes with a command to run.
pub fn trouble(dir: &Path) -> Option<Trouble> {
    if !dir.ancestors().any(|a| a.join(".git").exists()) {
        return None;
    }
    let said = |err: &[u8]| {
        let words = String::from_utf8_lossy(err).split_whitespace().collect::<Vec<_>>().join(" ");
        let words = words.strip_prefix("fatal: ").unwrap_or(&words).to_string();
        let words = if words.is_empty() { "Git did not run.".to_string() } else { words };
        if xcode_licence(cfg!(target_os = "macos"), &words) {
            return Trouble::licence();
        }
        Trouble { words, fix: None }
    };
    let none = || Trouble { words: "Git is not installed.".to_string(), fix: None };
    let bin = binary();
    if bin.is_absolute() {
        let out = Command::new(&bin).arg("-C").arg(dir).args(["rev-parse", "--git-dir"]).env("GIT_OPTIONAL_LOCKS", "0").output().ok()?;
        return (!out.status.success()).then(|| said(&out.stderr));
    }
    if !mac_tools() {
        return Some(none());
    }
    match Command::new(&bin).arg("--version").output() {
        Err(_) => Some(none()),
        Ok(out) if out.status.success() => None,
        Ok(out) => Some(said(&out.stderr)),
    }
}

/// A call of that git.
pub fn command() -> Command {
    Command::new(binary())
}

fn git(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = command().arg("-C").arg(dir).args(args).env("GIT_OPTIONAL_LOCKS", "0").output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// What `discard` did with a file's changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discarded {
    /// The file is as the last commit has it again.
    Restored,
    /// The last commit has no such file: it is no longer staged, and is
    /// still on disk for the caller to put in the trash.
    New,
}

/// Take back what has changed in one file since the last commit, staged
/// or not. A file the commit has is put back as it was there (`git
/// restore`), after what it held is written into git's object store
/// (`hash-object -w`), where it stays until git next prunes: nothing
/// shows it, and it can still be had back by hand. A file the commit
/// does not have is only unstaged, and left for the caller to trash.
pub fn discard(dir: &Path, path: &Path) -> Result<Discarded, String> {
    let rel = path.strip_prefix(dir).unwrap_or(path).to_string_lossy().replace('\\', "/");
    let at = path.to_string_lossy().to_string();
    let run = |args: &[&str]| -> Result<(), String> {
        let out = command().arg("-C").arg(dir).args(args).output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
    };
    if git(dir, &["cat-file", "-e", &format!("HEAD:./{rel}")]).is_none() {
        run(&["rm", "--cached", "-q", "--ignore-unmatch", "--", &at])?;
        return Ok(Discarded::New);
    }
    if path.is_file() {
        let _ = run(&["hash-object", "-w", "--", &at]);
    }
    run(&["restore", "--source=HEAD", "--staged", "--worktree", "--", &at])?;
    Ok(Discarded::Restored)
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
    /// When each local branch was made here, in seconds: the first line
    /// of its reflog, which is all git keeps of that. A branch only a
    /// remote has, and one whose reflog has been pruned to nothing, has
    /// none.
    pub made: std::collections::HashMap<String, i64>,
    /// How many sets of changes were left behind on the branch checked
    /// out (`Carry::Leave`) and are still waiting to be put back.
    pub stashed: usize,
    /// The remote a branch is published to and fetched from: `origin`,
    /// else the only one there is. None in a repository with no remote.
    pub origin: Option<String>,
    /// Local branches no remote has: made here and not yet published.
    /// Empty with no remote, where there is nowhere to publish to.
    pub unpublished: Vec<String>,
}

/// A name in pieces that order as a person reads it: a run of digits
/// is its number, anything else its letters.
fn natural(name: &str) -> Vec<(u64, String)> {
    let mut out: Vec<(u64, String)> = Vec::new();
    let mut was_digit = None;
    for c in name.chars() {
        let digit = c.is_ascii_digit();
        if was_digit != Some(digit) {
            out.push((0, String::new()));
            was_digit = Some(digit);
        }
        let last = out.last_mut().unwrap();
        match c.to_digit(10).filter(|_| digit) {
            Some(d) => last.0 = last.0.saturating_mul(10).saturating_add(d as u64),
            None => last.1.push(c),
        }
    }
    out
}

impl Branches {
    /// What a button naming the place says: the branch, else the commit.
    pub fn label(&self) -> String {
        self.current.clone().unwrap_or_else(|| self.head.clone())
    }

    /// Every branch by the time of its last commit, the newest first,
    /// after the default branch, which is first in any order; each with
    /// whether only a remote has it. Branches at one commit have one
    /// time (a branch just made from the default one, and the branch
    /// last merged into it): the one made later is first then (`made`).
    /// Where that does not say, the later name is, a number in a name
    /// read as a number, so "v0.2.10" is above "v0.2.9" and that above
    /// "v0.2.1".
    pub fn by_recency(&self) -> Vec<(String, bool)> {
        let mut all: Vec<(String, bool)> = self.local.iter().map(|n| (n.clone(), false)).chain(self.remote.iter().map(|n| (n.clone(), true))).collect();
        all.sort_by_key(|(n, _)| (Some(n) != self.default.as_ref(), std::cmp::Reverse(self.when.get(n).copied().unwrap_or(0)), std::cmp::Reverse(self.made.get(n).copied().unwrap_or(0)), std::cmp::Reverse(natural(n))));
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

/// The time on a reflog's first line: "<old> <new> <who> <seconds>
/// <zone>", then a tab and what was done.
fn first_logged(log: &Path) -> Option<i64> {
    use std::io::BufRead;
    let mut first = String::new();
    std::io::BufReader::new(std::fs::File::open(log).ok()?).read_line(&mut first).ok()?;
    first.split('\t').next()?.split_whitespace().rev().nth(1)?.parse().ok()
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
    if let Some(logs) = line(dir, &["rev-parse", "--git-common-dir"]).map(|d| dir.join(d).join("logs").join("refs").join("heads")) {
        for name in &b.local {
            if let Some(at) = first_logged(&logs.join(name)) {
                b.made.insert(name.clone(), at);
            }
        }
    }
    b.origin = remote(dir);
    if b.origin.is_some() {
        b.unpublished = b.local.iter().filter(|l| !b.remote.contains(l)).cloned().collect();
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
    let out = command().arg("-C").arg(dir).args(args).output().map_err(|e| format!("git did not run: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = said.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    // "error: …" is the reason; the lines after it list files and advice.
    let why = lines.iter().find(|l| l.starts_with("error:") || l.starts_with("fatal:")).or(lines.first()).copied().unwrap_or("git refused");
    Err(why.trim_start_matches("error:").trim_start_matches("fatal:").trim().to_string())
}

/// The remote this repository publishes to: `origin` when there is one,
/// else the first git lists.
pub fn remote(dir: &Path) -> Option<String> {
    let all = String::from_utf8_lossy(&git(dir, &["remote"])?).lines().map(str::to_string).collect::<Vec<_>>();
    all.iter().find(|r| *r == "origin").or(all.first()).cloned()
}

/// How long a call that goes to a remote may take before it is given
/// up: a host that does not answer would otherwise hold the list of
/// branches for as long as the system's own timeout.
const NET_WAIT: std::time::Duration = std::time::Duration::from_secs(40);

/// Run a git command that talks to a remote; its own words when it
/// fails. Git may not ask for a name or a password here, in a terminal
/// or in a window of a helper's: there is nobody at one, and the call
/// would wait for ever. A failure to sign in comes back as one
/// (`why_net`), and the window says how to sign in.
fn net(dir: &Path, args: &[&str]) -> Result<(), String> {
    let mut child = command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes -o ConnectTimeout=15")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("git did not run: {e}"))?;
    let mut err = child.stderr.take().ok_or("git did not run")?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut err, &mut text);
        text
    });
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if started.elapsed() < NET_WAIT => std::thread::sleep(std::time::Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the remote did not answer in time".to_string());
            }
        }
    };
    if status.success() {
        return Ok(());
    }
    let said = reader.join().unwrap_or_default();
    let lines: Vec<&str> = said.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    // The last "fatal:" is git's own verdict; a "remote:" line before it
    // is the host's reason, which says more ("Permission to x denied").
    let host = lines.iter().find(|l| l.starts_with("remote:")).map(|l| l.trim_start_matches("remote:").trim());
    let why = lines.iter().rev().find(|l| l.starts_with("fatal:") || l.starts_with("error:") || l.starts_with("ssh:")).or(lines.last()).copied().unwrap_or("git refused");
    let why = why.trim_start_matches("fatal:").trim_start_matches("error:").trim();
    Err(match host {
        Some(h) if !h.is_empty() && !why.contains(h) => format!("{why} ({h})"),
        _ => why.to_string(),
    })
}

/// Why a call to a remote failed, as far as its words say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetTrouble {
    /// The host could not be reached: no network, no such host, no
    /// answer.
    Offline,
    /// The host was reached and did not take who was asking: not signed
    /// in, a sign-in that has run out, or an account with no right to
    /// the repository.
    SignIn,
    /// Something else, in git's own words.
    Other,
}

/// What kind of failure git's words name.
pub fn why_net(words: &str) -> NetTrouble {
    let w = words.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| w.contains(n));
    if has(&["could not resolve host", "could not resolve hostname", "unable to access", "network is unreachable", "connection timed out", "operation timed out", "failed to connect", "connection refused", "did not answer in time", "no route to host", "couldn't connect"]) && !has(&["403", "401"]) {
        NetTrouble::Offline
    } else if has(&["authentication failed", "could not read username", "could not read password", "terminal prompts disabled", "permission denied", "invalid credentials", "403", "401", "repository not found", "logon failed", "host key verification failed", "invalid username or", "access denied", "permission to "]) {
        NetTrouble::SignIn
    } else {
        NetTrouble::Other
    }
}

/// Ask the remote what it has now, and forget the branches it no longer
/// has. Nothing to do, and no failure, in a repository with no remote.
pub fn fetch(dir: &Path) -> Result<(), String> {
    match remote(dir) {
        Some(r) => net(dir, &["fetch", "--prune", "--quiet", &r]),
        None => Ok(()),
    }
}

/// Whether the remote takes who is asking: the smallest question there
/// is, which moves nothing.
pub fn reach(dir: &Path) -> Result<(), String> {
    let r = remote(dir).ok_or("this repository has no remote")?;
    net(dir, &["ls-remote", "--quiet", "--heads", &r, "HEAD"])
}

/// How far the local branch `name` and the remote's copy of it are
/// apart, as last fetched: commits here the remote lacks, and commits
/// there that are not here. None when either does not exist.
pub fn ahead_behind(dir: &Path, name: &str) -> Option<(usize, usize)> {
    let r = remote(dir)?;
    let out = line(dir, &["rev-list", "--left-right", "--count", &format!("refs/heads/{name}...refs/remotes/{r}/{name}")])?;
    let mut n = out.split_whitespace().filter_map(|x| x.parse::<usize>().ok());
    Some((n.next()?, n.next()?))
}

/// Bring the local branch `name` up to the remote's copy as last
/// fetched, when that only adds commits. Git refuses when the branch
/// has commits of its own the remote lacks: that takes a merge, which
/// is not made here. The branch need not be the one checked out.
pub fn fast_forward(dir: &Path, name: &str) -> Result<(), String> {
    let r = remote(dir).ok_or("this repository has no remote")?;
    if line(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).as_deref() == Some(name) {
        act(dir, &["merge", "--ff-only", "--quiet", &format!("refs/remotes/{r}/{name}")])
    } else {
        act(dir, &["fetch", "--quiet", ".", &format!("refs/remotes/{r}/{name}:refs/heads/{name}")])
    }
}

/// Send the branch `name` to the remote, and have it follow its copy
/// there from now on: what publishes a branch made here, and what
/// pushes one already there.
pub fn publish(dir: &Path, name: &str) -> Result<(), String> {
    let r = remote(dir).ok_or("this repository has no remote to publish to")?;
    net(dir, &["push", "--quiet", "--set-upstream", &r, &format!("refs/heads/{name}:refs/heads/{name}")])
}

/// Where the remote is, and how the machine would sign in to it: what
/// the window goes by to say how.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    /// The remote's address as git has it.
    pub url: String,
    /// Its host: "github.com".
    pub host: String,
    /// Reached over ssh, with a key, and not over https, with a token.
    pub ssh: bool,
    /// The helpers git asks for a name and a password, as configured.
    pub helpers: Vec<String>,
    /// GitHub's own command line tool, when it is installed.
    pub gh: Option<PathBuf>,
    /// Whether that tool says someone is signed in to the host.
    pub gh_signed: bool,
}

impl Account {
    pub fn github(&self) -> bool {
        self.host == "github.com" || self.host.is_empty()
    }
}

/// The host of a remote's address: `https://host/x`, `ssh://git@host/x`
/// and `git@host:x` all name one.
pub fn host_of(url: &str) -> (String, bool) {
    if let Some(rest) = url.split_once("://").map(|(_, r)| r) {
        let host = rest.split('/').next().unwrap_or("");
        let host = host.rsplit('@').next().unwrap_or(host).split(':').next().unwrap_or("");
        return (host.to_string(), url.starts_with("ssh://"));
    }
    match url.split_once(':') {
        Some((before, _)) if !before.contains('/') && !before.is_empty() => (before.rsplit('@').next().unwrap_or(before).to_string(), true),
        _ => (String::new(), false),
    }
}

/// How this machine would sign in to the repository's remote. It asks
/// the tools and opens no credential.
pub fn account(dir: &Path) -> Account {
    let url = remote(dir).and_then(|r| line(dir, &["remote", "get-url", &r])).unwrap_or_default();
    let (host, ssh) = host_of(&url);
    let helpers = git(dir, &["config", "--get-all", "credential.helper"]).map(|o| String::from_utf8_lossy(&o).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()).unwrap_or_default();
    let gh = gh_binary();
    let gh_signed = gh.as_ref().is_some_and(|g| {
        let mut c = Command::new(g);
        c.args(["auth", "status"]);
        if !host.is_empty() {
            c.args(["--hostname", &host]);
        }
        c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
    });
    Account { url, host, ssh, helpers, gh, gh_signed }
}

/// GitHub's command line tool, on `PATH` or where Homebrew puts it.
fn gh_binary() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["gh.exe"] } else { &["gh"] };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if !cfg!(windows) {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    dirs.into_iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
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
        let out = command().arg("-C").arg(dir).args(["merge-tree", "--write-tree", "--name-only", "--no-messages", "--merge-base=HEAD", &theirs, &mine]).output();
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
    switch_how(dir, name, create, None, carry)
}

/// Make the branch `name` from the branch `base`, which need not be the
/// one checked out, and check it out, with the changes not yet
/// committed going where `carry` says: all or nothing, as `switch_with`
/// is. Brought along, they have to fit on `base`.
pub fn create_from(dir: &Path, name: &str, base: &str, carry: Carry) -> Result<(), String> {
    let here = line(dir, &["symbolic-ref", "--short", "-q", "HEAD"]);
    switch_how(dir, name, true, Some(base).filter(|b| here.as_deref() != Some(*b)), carry)
}

fn switch_how(dir: &Path, name: &str, create: bool, base: Option<&str>, carry: Carry) -> Result<(), String> {
    let open = unmerged(dir);
    if !open.is_empty() {
        return Err(format!("{} to resolve first: {}", if open.len() == 1 { "a file has a conflict".to_string() } else { format!("{} files have conflicts", open.len()) }, name_some(&open)));
    }
    let go = |dir: &Path| match (create, base) {
        (true, Some(base)) => act(dir, &["switch", "-c", name, base]),
        (true, None) => self::create(dir, name),
        _ => switch(dir, name),
    };
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
            // A branch made from here starts with what is here, so the
            // changes always fit; made from another branch, they have to
            // fit on that one.
            if let Some(onto) = if create { base } else { Some(name) } {
                let bad = misfits(dir, onto);
                if !bad.is_empty() {
                    return Err(format!("your changes do not fit on {onto}: {} would conflict", name_some(&bad)));
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
    !name.is_empty() && !name.starts_with('-') && command().args(["check-ref-format", "--branch", name]).output().is_ok_and(|o| o.status.success())
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
    let run = |base: &[&str]| command().arg("-C").arg(dir).args(["diff", "--no-color", "--no-ext-diff"]).args(base).arg("--").arg(path).env("GIT_OPTIONAL_LOCKS", "0").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).to_string());
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

/// How a run of lines in a file differs from what git has of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineChange {
    /// Lines git does not have.
    Added,
    /// Lines that stand where others stood.
    Modified,
    /// Lines git has that are gone from here: the mark is a place, the
    /// top of `line`, and takes no lines up.
    Deleted,
}

/// One run of changed lines: where it starts (from 0), how many lines
/// it is (none for `Deleted`), and how it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineMark {
    pub line: usize,
    pub lines: usize,
    pub kind: LineChange,
}

/// What git has of a file to compare it with: the copy in the index,
/// which is the last commit's until something is staged. VS Code's
/// editor marks a file against the same thing. `None` for a file git
/// does not track, or one in no repository: nothing to compare with,
/// and no marks.
pub fn base_text(file: &Path) -> Option<String> {
    let (dir, name) = (file.parent()?, file.file_name()?.to_str()?);
    let out = git(dir, &["show", &format!(":./{name}")])?;
    String::from_utf8(out).ok()
}

/// One place a file differs from git's copy: the lines git has there
/// and the lines that stand there now (either may be none), by line
/// from 0. Lines are counted as `str::split_inclusive('\n')` cuts them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

/// Where `now` differs from `base`, line by line (Myers', by `similar`).
pub fn line_hunks(base: &str, now: &str) -> Vec<Hunk> {
    use similar::DiffOp;
    similar::TextDiff::from_lines(base, now)
        .ops()
        .iter()
        .filter_map(|op| match *op {
            DiffOp::Equal { .. } => None,
            DiffOp::Insert { old_index, new_index, new_len } => Some(Hunk { old: old_index..old_index, new: new_index..new_index + new_len }),
            DiffOp::Delete { old_index, old_len, new_index } => Some(Hunk { old: old_index..old_index + old_len, new: new_index..new_index }),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => Some(Hunk { old: old_index..old_index + old_len, new: new_index..new_index + new_len }),
        })
        .collect()
}

/// The marks an editor's margin shows for those places, each with the
/// place it belongs to: added, modified, and where lines were taken
/// from.
pub fn hunk_marks(hunks: &[Hunk]) -> Vec<(usize, LineMark)> {
    let mut marks = Vec::new();
    for (ix, h) in hunks.iter().enumerate() {
        let (old_len, new_len) = (h.old.len(), h.new.len());
        // Lines in the place of others, and not as many: as many as
        // there were are changed, and what is over is new, or what is
        // short was taken out. One run marked "changed" whole called
        // three lines changed where one was and two were put in under
        // it.
        let both = old_len.min(new_len);
        if both > 0 {
            marks.push((ix, LineMark { line: h.new.start, lines: both, kind: LineChange::Modified }));
        }
        if new_len > both {
            marks.push((ix, LineMark { line: h.new.start + both, lines: new_len - both, kind: LineChange::Added }));
        } else if old_len > both {
            marks.push((ix, LineMark { line: h.new.start + both, lines: 0, kind: LineChange::Deleted }));
        }
    }
    marks
}

/// The lines of `now` that differ from `base`, as an editor marks them
/// in its margin.
pub fn line_marks(base: &str, now: &str) -> Vec<LineMark> {
    hunk_marks(&line_hunks(base, now)).into_iter().map(|(_, mark)| mark).collect()
}

/// The lines `range` of a text, whole, with their line breaks, and
/// where they are in it by byte.
pub fn lines_of(text: &str, range: &Range<usize>) -> (Range<usize>, String) {
    let mut at = 0;
    let (mut from, mut to) = (text.len(), text.len());
    for (ix, line) in text.split_inclusive('\n').enumerate() {
        if ix == range.start {
            from = at;
        }
        if ix == range.end {
            to = at;
            break;
        }
        at += line.len();
    }
    let from = from.min(to);
    (from..to, text[from..to].to_string())
}
