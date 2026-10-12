//! What the files tree does to the disk, kept so it can be taken back
//! and done again: making, renaming, moving, copying in, removing.
//!
//! Every one of them is a list of moves, a path before and a path after,
//! where one of the two may be in the holding folder: a file removed is
//! moved there and not thrown away, and a file made or copied in has a
//! place there to go back to. Taking a thing back is its moves the other
//! way round, last first; doing it again is the moves as they were. So
//! nothing here deletes anything. What is in the holding folder and can
//! no longer be reached by either key is handed back to the caller, who
//! puts it in the system's trash.

use std::path::{Path, PathBuf};

/// What was done, for the words that say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Made,
    Renamed,
    Moved,
    Copied,
    Removed,
}

/// One thing done: its kind and its moves, each from where a path was
/// before to where it was after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    pub kind: Kind,
    pub moves: Vec<(PathBuf, PathBuf)>,
}

/// The things done and the things taken back, newest last, and the
/// folder what is out of the tree is held in.
#[derive(Debug)]
pub struct Journal {
    hold: PathBuf,
    done: Vec<Op>,
    undone: Vec<Op>,
    next: u64,
}

/// How many things done are kept to take back.
const MOST: usize = 100;

/// Move a file or a folder to a path nothing is at. Its folder is made
/// when it is not there. Across two volumes it is not moved: a rename is
/// all this does, so nothing is ever half copied.
pub fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::symlink_metadata(from).is_err() {
        return Err(format!("{} is no longer there", name(from)));
    }
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(format!("{} is already there", name(to)));
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string())
}

impl Journal {
    pub fn new(hold: PathBuf) -> Self {
        Journal { hold, done: Vec::new(), undone: Vec::new(), next: 0 }
    }

    /// A free place in the holding folder for something of this name.
    /// Nothing is made there until something is moved to it.
    pub fn slot(&mut self, of: &Path) -> PathBuf {
        loop {
            self.next += 1;
            let at = self.hold.join(format!("{}-{}", std::process::id(), self.next));
            if !at.exists() {
                return at.join(of.file_name().unwrap_or_default());
            }
        }
    }

    fn held(&self, path: &Path) -> bool {
        path.starts_with(&self.hold)
    }

    /// What of an op is in the holding folder now.
    fn holds(&self, op: &Op) -> Vec<PathBuf> {
        op.moves.iter().flat_map(|(a, b)| [a, b]).filter(|p| self.held(p) && std::fs::symlink_metadata(p).is_ok()).cloned().collect()
    }

    /// A thing has been done. What was taken back before it cannot be
    /// done again, and the oldest thing done falls off a full list: what
    /// either holds is returned, for the trash.
    pub fn record(&mut self, op: Op) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::mem::take(&mut self.undone).iter().flat_map(|old| self.holds(old)).collect();
        self.done.push(op);
        while self.done.len() > MOST {
            let old = self.done.remove(0);
            out.extend(self.holds(&old));
        }
        out
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Run moves one way. One that fails puts back those already run,
    /// so a thing is taken back whole or not at all.
    fn run(moves: &[(PathBuf, PathBuf)]) -> Result<(), String> {
        for (ix, (from, to)) in moves.iter().enumerate() {
            if let Err(why) = move_path(from, to) {
                for (from, to) in moves[..ix].iter().rev() {
                    let _ = move_path(to, from);
                }
                return Err(why);
            }
        }
        Ok(())
    }

    /// Take the last thing done back. None with nothing to take back.
    /// One that cannot be (a file since changed places, a name since
    /// taken) is left as it is, forgotten, and the reason said, with
    /// what it holds returned for the trash.
    pub fn undo(&mut self) -> Option<Result<Op, (String, Vec<PathBuf>)>> {
        let op = self.done.pop()?;
        let back: Vec<(PathBuf, PathBuf)> = op.moves.iter().rev().map(|(before, after)| (after.clone(), before.clone())).collect();
        Some(match Self::run(&back) {
            Ok(()) => {
                self.undone.push(op.clone());
                Ok(op)
            }
            Err(why) => Err((why, self.holds(&op))),
        })
    }

    /// Do the last thing taken back again.
    pub fn redo(&mut self) -> Option<Result<Op, (String, Vec<PathBuf>)>> {
        let op = self.undone.pop()?;
        Some(match Self::run(&op.moves) {
            Ok(()) => {
                self.done.push(op.clone());
                Ok(op)
            }
            Err(why) => Err((why, self.holds(&op))),
        })
    }

    /// Everything in a holding folder, with nothing kept of what it was:
    /// what a launch finds left by the one before, and what a quit
    /// leaves. For the trash. The folders they stood in are removed.
    pub fn leftovers(hold: &Path) -> Vec<PathBuf> {
        let Ok(slots) = std::fs::read_dir(hold) else { return Vec::new() };
        slots.flatten().flat_map(|slot| std::fs::read_dir(slot.path()).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>()).collect()
    }

    /// Forget everything, as at a quit: what is held is returned.
    pub fn close(&mut self) -> Vec<PathBuf> {
        self.done.clear();
        self.undone.clear();
        Self::leftovers(&self.hold)
    }

    /// The holding folder's empty places removed, once what they held
    /// has gone to the trash.
    pub fn tidy(hold: &Path) {
        let Ok(slots) = std::fs::read_dir(hold) else { return };
        for slot in slots.flatten() {
            let _ = std::fs::remove_dir(slot.path());
        }
    }
}
