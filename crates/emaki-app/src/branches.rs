//! What the list of branches does with a remote: asking it what it has,
//! making a branch on a sheet of its own, publishing one, and saying how
//! to sign in when the remote does not take who is asking
//! (`docs/panels.md`, Branches).

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use emaki_core::git;

use crate::format::plural;
use crate::panels::BranchAsk;
use std::rc::Rc;

use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;

use crate::workbench::{choice_pill, float_shadow, MenuDo, Press, swallow_click, Notice, Workbench};

/// How old the last word from the remote may be before the list of
/// branches asks again when it is opened.
const FETCH_FRESH: Duration = Duration::from_secs(60);

/// What a commit's menu in the history asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommitDo {
    /// Take the last commit back, its changes kept.
    Undo,
    /// A new commit that takes this one's changes back.
    Revert,
    CopySha,
    /// Its page on the remote's host.
    View,
}

/// How many commits the history lists.
const HISTORY_MOST: usize = 300;
/// The most a commit's files take under the commits before they scroll.
const HISTORY_FILES_H: f32 = 190.;
/// The files' own column, and the window's width from which they have one.
const HISTORY_FILES_W: f32 = 270.;
const HISTORY_WIDE: f32 = 980.;

/// The name the history keeps a commit's file's lines under.
pub(crate) fn history_key(c: &crate::panels::Changes) -> Option<std::path::PathBuf> {
    c.commit.as_ref().zip(c.commit_file.as_ref()).map(|(sha, path)| std::path::PathBuf::from(format!("{sha}:{path}")))
}

/// A strip under the files' head: one line and a button.
pub(crate) const STRIP_H: f32 = 38.;

/// The window's button (`choice_pill`) in the size a strip or a foot
/// line takes.
pub(crate) fn small_pill(id: impl Into<ElementId>, icon: &'static str, label: impl Into<SharedString>, kind: Press, off: bool, theme: &gpui_component::Theme) -> Stateful<Div> {
    choice_pill(id, icon, label, kind, off, theme).h(px(24.)).px(px(10.)).gap(px(5.)).text_size(px(12.))
}

/// A branch about to be deleted, on the card that asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BranchDelete {
    pub(crate) name: String,
    /// Whether it is here, a local branch; and whether the remote has
    /// it. One only the remote has is deleted there.
    pub(crate) here: bool,
    pub(crate) there: bool,
    /// The tick: the remote's copy goes with it.
    pub(crate) remote_too: bool,
    /// While git is at it.
    pub(crate) busy: bool,
    /// Counts the times the card came up, so each comes up in motion.
    pub(crate) serial: u32,
}

/// How the default branch here stands against the remote's, for a
/// branch about to be made from it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BaseSync {
    /// The remote is being asked.
    Checking,
    /// Commits here the remote lacks, and commits there not here.
    Known { ahead: usize, behind: usize },
    /// The remote could not be asked, in git's words.
    Failed(String),
    /// There is no remote, or it has no such branch: nothing to say.
    Alone,
}

/// The sheet a branch is made on.
#[derive(Clone, Debug)]
pub(crate) struct BranchNew {
    /// Made from the default branch, not from the one checked out.
    pub(crate) from_default: bool,
    pub(crate) sync: BaseSync,
    /// Bring the default branch up to the remote's before making the
    /// new one from it.
    pub(crate) update: bool,
    /// The default branch is being pushed from the sheet.
    pub(crate) pushing: bool,
}

/// The sheet that says how to sign in to the remote.
#[derive(Clone, Debug)]
pub(crate) struct GitGate {
    /// What was being done: "publish v2".
    pub(crate) what: String,
    /// What git said.
    pub(crate) why: String,
    /// How this machine would sign in; none until it has been asked.
    pub(crate) account: Option<git::Account>,
    pub(crate) checking: bool,
}

/// A branch's name as git will take it from what was typed: GitHub
/// Desktop's rule, a run of spaces becomes one hyphen.
pub(crate) fn branch_name_of(typed: &str) -> String {
    typed.split_whitespace().collect::<Vec<_>>().join("-")
}

impl Workbench {
    /// Ask the remote what it has, off the main thread, when the last
    /// answer is old. The list is read again when it is in.
    pub(crate) fn branch_fetch(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(root) = self.git_dir() else { return };
        if self.branches().is_none_or(|b| b.origin.is_none()) || self.branch_fetching {
            return;
        }
        if !force && self.branch_fetched.as_ref().is_some_and(|(of, at)| *of == root && at.as_ref().is_ok_and(|at| at.elapsed() < FETCH_FRESH)) {
            return;
        }
        self.branch_fetching = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let dir = root.clone();
            let done = cx.background_executor().spawn(async move { git::fetch(&dir) }).await;
            this.update(cx, |this, cx| {
                this.branch_fetching = false;
                this.branch_fetched = Some((root.clone(), done.map(|_| Instant::now())));
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The line at the foot of the list of branches: what the remote
    /// last said, or why it said nothing, then the buttons: Pull when
    /// the branch checked out is behind the remote's, and Fetch. The
    /// buttons keep their place while the remote is being asked, so
    /// nothing in the line moves.
    pub(crate) fn branch_fetch_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let b = self.branches()?;
        let origin = b.origin.clone()?;
        let root = self.git_dir()?;
        let last = self.branch_fetched.as_ref().filter(|(of, _)| *of == root).map(|(_, r)| r.clone());
        let row = h_flex().flex_shrink_0().min_h(px(44.)).px(px(12.)).py(px(8.)).gap(px(8.)).items_center().border_t_1().border_color(theme.border).text_size(px(11.5)).text_color(theme.muted_foreground);
        let fetch = small_pill("branch-fetch-again", "icons/refresh.svg", if matches!(last, Some(Err(_))) { "Try Again" } else { "Fetch" }, Press::Quiet, self.branch_fetching, &theme).when(!self.branch_fetching, |d| {
            d.on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.branch_fetch(true, cx);
            }))
        });
        let pull = self.sync_pill(true, cx);
        let web = self.repo_web_pill(cx);
        if self.branch_fetching {
            return Some(
                row.child(Icon::new(IconName::LoaderCircle).with_size(px(12.)).text_color(theme.muted_foreground).flex_shrink_0().with_animation("branch-fetching", Animation::new(Duration::from_millis(900)).repeat(), |icon, t| icon.rotate(gpui::Radians(t * std::f32::consts::TAU))))
                    .child(div().flex_1().min_w_0().truncate().child(format!("Asking {origin}…")))
                    .children(web)
                    .children(pull)
                    .child(fetch)
                    .into_any_element(),
            );
        }
        Some(match last {
            Some(Err(why)) => {
                let sign_in = git::why_net(&why) == git::NetTrouble::SignIn;
                let words = match git::why_net(&why) {
                    git::NetTrouble::Offline => format!("Could not reach {origin}. The list is as it was last fetched."),
                    git::NetTrouble::SignIn => format!("{origin} did not take your sign-in. The list is as it was last fetched."),
                    git::NetTrouble::Other => format!("Could not fetch from {origin}: {why}"),
                };
                let reason = why.clone();
                row.items_start()
                    .child(Icon::new(IconName::TriangleAlert).with_size(px(12.)).mt(px(3.)).text_color(theme.danger).flex_shrink_0())
                    .child(div().flex_1().min_w_0().line_height(px(17.)).text_color(theme.foreground).child(words))
                    .children(web)
                    .when(sign_in, |d| {
                        d.child(small_pill("branch-sign-in", "icons/circle-user.svg", "Sign In…", Press::Quiet, false, &theme).on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.open_gate("fetch".into(), reason.clone(), cx);
                        })))
                    })
                    .when(!sign_in, |d| d.child(fetch))
                    .into_any_element()
            }
            Some(Ok(at)) => {
                let when = crate::format::ago(crate::format::now_secs() - at.elapsed().as_secs_f64(), crate::format::now_secs());
                let words = match self.sync_words() {
                    Some((said, _)) => format!("{said}. Fetched {when}"),
                    None => format!("Fetched from {origin} {when}"),
                };
                row.child(div().flex_1().min_w_0().truncate().child(words)).children(web).children(pull).child(fetch).into_any_element()
            }
            None => row.child(div().flex_1().min_w_0().truncate().child(format!("Not fetched from {origin} yet"))).children(web).children(pull).child(fetch).into_any_element(),
        })
    }

    // -- pushing and pulling --------------------------------------------------

    /// How the branch checked out stands against the remote's as last
    /// fetched, in words, and what there is to do about it: pull (true),
    /// push (false), or neither when the two have gone separate ways,
    /// which takes a merge and is not done here. Nothing when they are
    /// level or the remote has no such branch.
    pub(crate) fn sync_words(&self) -> Option<(String, Option<bool>)> {
        let b = self.branches()?;
        b.origin.as_ref()?;
        let commits = |n: usize| plural(n, "commit", "commits");
        match (b.ahead, b.behind) {
            (0, 0) => None,
            (ahead, 0) => Some((format!("{} to push", commits(ahead)), Some(false))),
            (0, behind) => Some((format!("{} to pull", commits(behind)), Some(true))),
            (ahead, behind) => Some((format!("{} to push and {} to pull: merge in a terminal", commits(ahead), behind), None)),
        }
    }

    /// The button that pulls, or the one that pushes, when there is
    /// that to do; nothing otherwise.
    pub(crate) fn sync_pill(&self, pull: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        self.sync_words().filter(|(_, what)| *what == Some(pull))?;
        let busy = self.syncing == Some(pull);
        let label = match (pull, busy) {
            (true, true) => "Pulling…",
            (true, false) => "Pull",
            (false, true) => "Pushing…",
            (false, false) => "Push",
        };
        let off = self.syncing.is_some() || self.branch_busy;
        Some(
            small_pill(if pull { "sync-pull" } else { "sync-push" }, if pull { "icons/arrow-down.svg" } else { "icons/arrow-up.svg" }, label, Press::Primary, off, &theme)
                // ⌘P is the comparison's key for it.
                .when(!pull, |d| d.managed_tooltip(|window, cx| crate::workbench::tip("Push (⌘P in Changes)", window, cx)))
                .when(!off, |d| {
                    d.on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.sync_go(pull, cx);
                    }))
                })
                .into_any_element(),
        )
    }

    /// Undo, beside Push: the last commit taken back while the remote
    /// does not have it (`CommitDo::Undo`), its changes kept and its
    /// words put back in the fields. There while there is a commit to
    /// push, which is when the last one is still only here.
    pub(crate) fn undo_pill(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        self.branches().filter(|b| b.ahead > 0)?;
        let off = self.committing || self.syncing.is_some() || self.branch_busy;
        Some(
            small_pill("sync-undo", "icons/undo-2.svg", "Undo", Press::Quiet, off, &theme)
                .managed_tooltip(|window, cx| crate::workbench::tip("Take the last commit back, keeping its changes", window, cx))
                .when(!off, |d| {
                    d.on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.commit_do(CommitDo::Undo, String::new(), window, cx);
                    }))
                })
                .into_any_element(),
        )
    }

    /// Pull: bring the branch checked out up to the remote's as last
    /// fetched, which only adds commits (`git::fast_forward`). Push:
    /// send its commits to the remote. Neither is done unasked. Either
    /// may be done while an agent works: git is the person's to run.
    pub(crate) fn sync_go(&mut self, pull: bool, cx: &mut Context<Self>) {
        let (Some(root), Some(b)) = (self.git_dir(), self.branches()) else { return };
        let Some(name) = b.current.clone() else { return };
        if self.syncing.is_some() {
            return;
        }
        let origin = b.origin.clone().unwrap_or_else(|| "origin".into());
        let said = self.git_where();
        self.syncing = Some(pull);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (dir, branch) = (root.clone(), name.clone());
            let done = cx.background_executor().spawn(async move { if pull { git::fast_forward(&dir, &branch) } else { git::publish(&dir, &branch) } }).await;
            this.update(cx, |this, cx| {
                this.syncing = None;
                match (done, pull) {
                    (Ok(()), true) => this.notice = Some(Notice::said(format!("pulled {said} from {origin}"))),
                    (Ok(()), false) => {
                        this.notice = Some(Notice::said(format!("pushed {said} to {origin}")));
                        // With nothing left to commit, the comparison has
                        // done what it was opened for.
                        if this.changed_files().is_empty() && this.changes.as_ref().is_some_and(|c| !c.history) {
                            this.close_changes(cx);
                        }
                    }
                    (Err(why), true) => this.branch_ask = Some(BranchAsk::stop("Not pulled", format!("Nothing was changed. Git said: {why}."))),
                    (Err(why), false) => this.net_failed("Not pushed", format!("push {name}"), why, cx),
                }
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Under the files' head, with commits to push or to pull: one line
    /// saying how many, and the button. It opens and closes in motion,
    /// drawn as it goes from what it last said.
    pub(crate) fn sync_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let now = self.sync_words();
        let on = now.is_some();
        if on {
            *self.sync_kept.borrow_mut() = now;
        }
        let moving = self.presence("strip-sync", on, cx);
        let (said, what) = self.sync_kept.borrow().clone().filter(|_| on || moving.is_some())?;
        let row = h_flex()
            .w_full()
            .h(px(STRIP_H))
            .flex_shrink_0()
            .px(px(12.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.primary.opacity(0.07))
            .text_size(px(11.5))
            .child(Icon::default().path(if what == Some(true) { "icons/arrow-down.svg" } else { "icons/arrow-up.svg" }).with_size(px(12.)).text_color(theme.link).flex_shrink_0())
            .child(div().flex_1().min_w_0().truncate().text_color(theme.foreground).child(said))
            .children(self.undo_pill(cx).filter(|_| on))
            .children(what.filter(|_| on).and_then(|pull| self.sync_pill(pull, cx)));
        let wrap = v_flex().w_full().flex_shrink_0().justify_end().overflow_hidden().child(row);
        Some(match moving {
            Some((coming, n)) => wrap
                .with_animation(ElementId::Name(format!("strip-sync-{n}").into()), Animation::new(crate::workbench::FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                    let t = if coming { t } else { 1. - t };
                    d.h(px((STRIP_H * t).round())).opacity(t)
                })
                .into_any_element(),
            None => wrap.into_any_element(),
        })
    }

    // -- the history ----------------------------------------------------------

    /// The sheet's other side: the branch's commits, or back to what is
    /// not committed yet.
    pub(crate) fn history_show(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        if c.history == on {
            return;
        }
        c.history = on;
        c.diff = None;
        c.reading = false;
        c.list.reset(0);
        if on {
            self.history_load(cx);
        } else {
            self.load_diff(cx);
        }
        cx.notify();
    }

    /// Read the commits, off the main thread, and show the newest when
    /// the one picked is not among them any more.
    pub(crate) fn history_load(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.changes.as_ref().map(|c| c.root.clone()) else { return };
        cx.spawn(async move |this, cx| {
            let dir = root.clone();
            let log = cx.background_executor().spawn(async move { git::log(&dir, HISTORY_MOST) }).await;
            this.update(cx, |this, cx| {
                let Some(c) = this.changes.as_mut().filter(|c| c.root == root) else { return };
                let pick = c.commit.clone().filter(|sha| log.iter().any(|m| m.sha == *sha)).or_else(|| log.first().map(|m| m.sha.clone()));
                let moved = pick != c.commit || c.commit_files.is_none();
                c.commits = Some(Rc::new(log));
                if let (Some(sha), true) = (pick, moved) {
                    this.history_pick(sha, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Show a commit: its files, and the first one's lines.
    pub(crate) fn history_pick(&mut self, sha: String, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        c.commit = Some(sha.clone());
        c.commit_file = None;
        c.cfiles_scroll.set_offset(point(px(0.), px(0.)));
        let root = c.root.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (dir, of) = (root, sha.clone());
            let (files, top) = cx.background_executor().spawn(async move { (git::commit_files(&dir, &of), git::top(&dir)) }).await;
            this.update(cx, |this, cx| {
                let Some(c) = this.changes.as_mut().filter(|c| c.commit.as_ref() == Some(&sha)) else { return };
                c.top = top;
                let first = files.first().map(|(p, _)| p.clone());
                c.commit_files = Some((sha, Rc::new(files)));
                match first {
                    Some(path) => this.history_file(path, cx),
                    None => cx.notify(),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Show one of the picked commit's files.
    pub(crate) fn history_file(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        let Some(sha) = c.commit.clone() else { return };
        c.commit_file = Some(path.clone());
        let (root, key) = (c.root.clone(), history_key(c));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let of = path.clone();
            let diff = cx.background_executor().spawn(async move { git::commit_diff(&root, &sha, &of) }).await;
            this.update(cx, |this, cx| {
                let Some(c) = this.changes.as_mut().filter(|c| c.history && history_key(c) == key) else { return };
                c.list.reset(diff.rows.len());
                c.pictures = crate::panels::picture_sides(std::path::Path::new(&path), &diff);
                c.diff = key.map(|k| (k, Rc::new(diff)));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// ↑ and ↓ in the history: the commit over or under the one picked.
    pub(crate) fn history_step(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(c) = &self.changes else { return };
        let Some(list) = c.commits.clone() else { return };
        let at = c.commit.as_ref().and_then(|sha| list.iter().position(|m| m.sha == *sha));
        let to = match (at, down) {
            (None, _) => 0,
            (Some(at), true) => (at + 1).min(list.len().saturating_sub(1)),
            (Some(at), false) => at.saturating_sub(1),
        };
        if let Some(m) = list.get(to).filter(|_| at != Some(to)) {
            c.commits_scroll.scroll_to_item(to);
            self.history_pick(m.sha.clone(), cx);
        }
    }

    /// A right click on a commit: what can be asked of it. The newest
    /// one can be taken back while the remote does not have it; any can
    /// be reverted by a commit of its own.
    pub(crate) fn history_menu(&mut self, sha: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(c) = &self.changes else { return };
        let newest = c.commits.as_ref().and_then(|l| l.first()).filter(|m| m.sha == sha && m.local).is_some();
        let page = git::commit_url(&c.root, &sha).is_some();
        let item = |label: &'static str, what: CommitDo| (label, MenuDo::Commit(what, sha.clone()));
        let mut items = Vec::new();
        if newest {
            items.push(item("Undo Commit", CommitDo::Undo));
        }
        items.extend([item("Revert Changes in Commit", CommitDo::Revert), ("", MenuDo::Rule), item("Copy Commit Hash", CommitDo::CopySha)]);
        if page {
            items.push(item("View on the Web", CommitDo::View));
        }
        if self.changes.as_ref().is_some_and(|c| c.commit.as_ref() != Some(&sha)) {
            self.history_pick(sha.clone(), cx);
        }
        self.open_menu(at, items, cx);
    }

    /// A right click on one of a commit's files: the file as it is on
    /// disk now, when it is still there, its paths, and its page on the
    /// remote's host as that commit has it.
    pub(crate) fn history_file_menu(&mut self, path: String, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(c) = &self.changes else { return };
        let Some(sha) = c.commit.clone() else { return };
        let here = c.top.as_ref().map(|top| top.join(&path));
        let page = git::file_url(&c.root, &sha, &path);
        let mut items = Vec::new();
        if let Some(file) = &here {
            let item = |label: &'static str, what: crate::panels::FileDo| (label, MenuDo::File(what, file.clone()));
            // A file since removed has nothing to open or to show.
            if file.exists() {
                items.extend([item("Open", crate::panels::FileDo::Open), item(crate::sys::REVEAL_LABEL, crate::panels::FileDo::Reveal), ("", MenuDo::Rule)]);
            }
            items.extend([item("Copy Path", crate::panels::FileDo::CopyPath), item("Copy Relative Path", crate::panels::FileDo::CopyRel)]);
        }
        if let Some(url) = page {
            items.extend([("", MenuDo::Rule), ("View on the Web", MenuDo::Web(url))]);
        }
        if self.changes.as_ref().is_some_and(|c| c.commit_file.as_ref() != Some(&path)) {
            self.history_file(path, cx);
        }
        self.open_menu(at, items, cx);
    }

    /// Do what the commit's menu was asked. Undo and Revert are git's
    /// own and may be refused; nothing is forced.
    pub(crate) fn commit_do(&mut self, what: CommitDo, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        // From the history's menu, or for Undo from the strip under the
        // files' head, where no sheet is up.
        let Some(root) = self.changes.as_ref().map(|c| c.root.clone()).or_else(|| self.files_root()) else { return };
        match what {
            CommitDo::CopySha => return cx.write_to_clipboard(ClipboardItem::new_string(sha)),
            CommitDo::View => {
                if let Some(url) = git::commit_url(&root, &sha) {
                    cx.open_url(&url);
                }
                return;
            }
            _ => {}
        }
        if std::mem::replace(&mut self.committing, true) {
            return;
        }
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let dir = root.clone();
            let done = cx.background_executor().spawn(async move { if what == CommitDo::Undo { git::undo_last(&dir).map(Some) } else { git::revert(&dir, &sha).map(|_| None) } }).await;
            this.update_in(cx, |this, window, cx| {
                this.committing = false;
                match done {
                    // The commit's words are back in the fields it was
                    // typed in, over what it changed, to commit again.
                    Ok(Some((summary, body))) => {
                        this.commit_summary.update(cx, |s, cx| s.set_value(summary, window, cx));
                        this.commit_body.update(cx, |s, cx| s.set_value(body, window, cx));
                        this.notice = Some(Notice::said("took the last commit back; its changes are here to commit again"));
                        this.history_show(false, cx);
                    }
                    Ok(None) => {
                        this.notice = Some(Notice::said("reverted the commit with a new one"));
                        this.history_load(cx);
                    }
                    Err(why) => this.branch_ask = Some(BranchAsk::stop(if what == CommitDo::Undo { "Not undone" } else { "Not reverted" }, format!("Nothing was changed. Git said: {why}."))),
                }
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The history's side of the sheet: the commits at the left with the
    /// picked one's files under them, and at the right that commit's
    /// words over the lines of the file picked.
    pub(crate) fn render_history(&self, body: AnyElement, counts: Option<(usize, usize)>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(c) = &self.changes else { return div().into_any_element() };
        let dark = theme.mode.is_dark();
        let (hover, chosen_bg, quiet) = (crate::panels::row_hover(&theme, dark), theme.primary.opacity(if dark { 0.18 } else { 0.12 }), theme.muted_foreground);
        let now = crate::format::now_secs();
        let commits = c.commits.clone();
        let rows = commits.iter().flat_map(|l| l.iter()).enumerate().map(|(ix, m)| {
            let chosen = c.commit.as_ref() == Some(&m.sha);
            let (sha, menu_sha) = (m.sha.clone(), m.sha.clone());
            h_flex()
                .id(("commit-row", ix))
                .w_full()
                .flex_shrink_0()
                .h(px(46.))
                .px(px(10.))
                .gap(px(8.))
                .items_center()
                .rounded(px(7.))
                .cursor_pointer()
                .map(|d| if chosen { d.bg(chosen_bg) } else { d.hover(move |s| s.bg(hover)) })
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.history_pick(sha.clone(), cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.history_menu(menu_sha.clone(), e.position, cx);
                    }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(div().w_full().truncate().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(m.summary.clone()))
                        .child(div().w_full().truncate().text_size(px(11.)).text_color(quiet).child(format!("{} \u{b7} {}", m.author, crate::format::ago(m.when as f64, now)))),
                )
                // A release's tag, and a mark on a commit still to push.
                .children(m.tags.first().map(|t| div().flex_shrink_0().px(px(6.)).py(px(1.)).rounded(px(5.)).bg(theme.muted).text_size(px(10.5)).text_color(quiet).child(t.clone())))
                .when(m.local, |d| d.child(Icon::default().path("icons/arrow-up.svg").with_size(px(12.)).text_color(theme.primary).flex_shrink_0()))
        });
        let rows: Vec<_> = rows.collect();
        let files = c.commit_files.as_ref().filter(|(of, _)| Some(of) == c.commit.as_ref()).map(|(_, f)| f.clone());
        let file_rows = files.iter().flat_map(|f| f.iter()).enumerate().map(|(ix, (path, state))| {
            let (folder, name) = match path.rfind('/') {
                Some(cut) => (path[..=cut].to_string(), path[cut + 1..].to_string()),
                None => (String::new(), path.clone()),
            };
            let chosen = c.commit_file.as_ref() == Some(path);
            let (to, menu_path) = (path.clone(), path.clone());
            h_flex()
                .id(("commit-file", ix))
                .w_full()
                .flex_shrink_0()
                .h(px(26.))
                .px(px(8.))
                .gap(px(6.))
                .items_center()
                .rounded(px(6.))
                .cursor_pointer()
                .text_size(px(12.5))
                .map(|d| if chosen { d.bg(chosen_bg) } else { d.hover(move |s| s.bg(hover)) })
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.history_file(to.clone(), cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.history_file_menu(menu_path.clone(), e.position, cx);
                    }),
                )
                .child(img(crate::file_icons::path(&name, false, false, dark)).size(px(16.)).flex_shrink_0())
                .child(h_flex().flex_1().min_w_0().child(div().min_w_0().truncate().text_color(quiet).child(folder)).child(div().flex_shrink_0().text_color(theme.foreground).child(name)))
                .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().text_size(px(11.)).text_color(crate::panels::git_color(*state, dark)).children(state.letter()))
        });
        let file_rows: Vec<_> = file_rows.collect();
        let n_files = file_rows.len();
        let picked = commits.as_ref().and_then(|l| l.iter().find(|m| Some(&m.sha) == c.commit.as_ref())).cloned();
        let empty = commits.as_ref().is_some_and(|l| l.is_empty());
        // Three columns, as GitHub Desktop sets them: the commits, the
        // picked one's files, the picked file's lines, with the commit's
        // words over the last two. In a window too narrow to leave the
        // lines their room the files go under the commits instead.
        let wide = self.view_w >= px(HISTORY_WIDE);
        let files_head = div().flex_shrink_0().px(px(14.)).pt(px(9.)).pb(px(4.)).text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(quiet).child(if wide { plural(n_files, "changed file", "changed files") } else { format!("{} in this commit", plural(n_files, "file", "files")) });
        // Both lists keep a little room under what scrolls, so a row cut
        // at the foot stays inside the sheet's corner.
        let files_list = div()
            .relative()
            .pb(px(4.))
            .when(wide, |d| d.flex_1().min_h_0())
            .child(v_flex().id("history-files").map(|d| if wide { d.size_full() } else { d.max_h(px(HISTORY_FILES_H)) }).overflow_y_scroll().track_scroll(&c.cfiles_scroll).px(px(6.)).pb(px(6.)).children(file_rows))
            .vertical_scrollbar(&c.cfiles_scroll);
        let commits_list = div()
            .relative()
            .pb(px(4.))
            .flex_1()
            .min_h_0()
            .child(v_flex().id("history-commits").size_full().overflow_y_scroll().track_scroll(&c.commits_scroll).p(px(6.)).gap(px(1.)).children(rows).when(empty, |d| d.child(div().p(px(12.)).text_size(px(12.5)).text_color(quiet).child("No commits yet."))))
            .vertical_scrollbar(&c.commits_scroll);
        let head = picked.map(|m| {
            v_flex()
                .flex_shrink_0()
                .px(px(16.))
                .py(px(11.))
                .gap(px(4.))
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).line_height(px(20.)).child(m.summary.clone()))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .text_size(px(11.5))
                        .text_color(quiet)
                        .child(div().flex_shrink_0().child(m.author.clone()))
                        .child(div().flex_shrink_0().font_family(theme.mono_font_family.clone()).child(m.short.clone()))
                        .child(div().min_w_0().truncate().child(crate::format::ago(m.when as f64, now)))
                        .when(m.local, |d| d.child(div().flex_shrink_0().text_color(theme.primary).child("not pushed yet"))),
                )
                .when(!m.body.is_empty(), |d| d.child(div().id("history-body").mt(px(4.)).max_h(px(72.)).overflow_y_scroll().text_size(px(12.5)).line_height(px(18.)).text_color(theme.foreground.opacity(0.85)).child(m.body.clone())))
        });
        let file_head = c.commit_file.clone().map(|rel| {
            h_flex()
                .h(px(38.))
                .flex_shrink_0()
                .px(px(14.))
                .gap(px(10.))
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(12.)).child(rel))
                .when_some(counts, |el, (a, r)| {
                    el.child(div().flex_shrink_0().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(crate::panels::git_color(git::State::Added, dark)).child(format!("+{a}")))
                        .child(div().flex_shrink_0().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(crate::panels::git_color(git::State::Deleted, dark)).child(format!("\u{2212}{r}")))
                })
        });
        let lines = v_flex().flex_1().min_w_0().children(file_head).child(div().flex_1().min_h_0().child(body));
        let left = v_flex().w(crate::panels::CHANGES_LIST_W).flex_shrink_0().border_r_1().border_color(theme.border).bg(theme.sidebar).rounded_bl(px(15.)).child(commits_list);
        if wide {
            let files = v_flex().w(px(HISTORY_FILES_W)).flex_shrink_0().border_r_1().border_color(theme.border).bg(theme.sidebar.opacity(0.5)).child(files_head).child(files_list);
            return h_flex()
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(left)
                .child(v_flex().flex_1().min_w_0().children(head).child(h_flex().flex_1().min_h_0().items_stretch().when(n_files > 0, |d| d.child(files)).child(lines)))
                .into_any_element();
        }
        let left = left.when(n_files > 0, |d| d.child(v_flex().flex_shrink_0().border_t_1().border_color(theme.border).child(files_head).child(files_list)));
        h_flex().flex_1().min_h_0().items_stretch().child(left).child(v_flex().flex_1().min_w_0().children(head).child(lines)).into_any_element()
    }

    // -- committing -----------------------------------------------------------

    /// The files a commit takes: every changed one not left out, and
    /// whether that is all of them.
    pub(crate) fn commit_files(&self) -> (Vec<std::path::PathBuf>, bool) {
        // The repository's own: of a folder of checkouts, the one the
        // comparison is on.
        let root = self.changes.as_ref().map(|c| c.root.clone());
        let changed: Vec<std::path::PathBuf> = self.changed_files().iter().map(|(p, _)| p.clone()).filter(|p| root.as_ref().is_none_or(|r| p.starts_with(r))).collect();
        let out = self.changes.as_ref().map(|c| &c.left_out);
        let taken: Vec<std::path::PathBuf> = changed.iter().filter(|p| out.is_none_or(|o| !o.contains(*p))).cloned().collect();
        let all = taken.len() == changed.len();
        (taken, all)
    }

    /// A click on a file's box in the comparison: in or out of the
    /// commit, and with it every file chosen together with it.
    pub(crate) fn commit_tick(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let Some(c) = &mut self.changes else { return };
        let out = !c.left_out.contains(&path);
        let together: Vec<std::path::PathBuf> = if c.marked.len() > 1 && c.marked.contains(&path) { c.marked.clone() } else { vec![path] };
        for p in together {
            if out {
                c.left_out.insert(p);
            } else {
                c.left_out.remove(&p);
            }
        }
        cx.notify();
    }

    /// Commit, from the button or ↩ in the summary: the files ticked,
    /// under the words typed. Git does it and may refuse (no name to
    /// commit under, a hook, a merge half done); its words are shown
    /// and nothing is forced.
    pub(crate) fn commit_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.changes.as_ref().map(|c| c.root.clone()) else { return };
        let (files, all) = self.commit_files();
        let summary = self.commit_summary.read(cx).value().trim().to_string();
        let body = self.commit_body.read(cx).value().trim().to_string();
        if self.committing || files.is_empty() || summary.is_empty() {
            return;
        }
        // Said with the checkout's name in a folder of them.
        let branch = self.git_where();
        self.committing = true;
        cx.notify();
        let n = files.len();
        cx.spawn_in(window, async move |this, cx| {
            let dir = root.clone();
            let done = cx.background_executor().spawn(async move { git::commit(&dir, &summary, &body, (!all).then_some(&files[..])) }).await;
            this.update_in(cx, |this, window, cx| {
                this.committing = false;
                match done {
                    Ok(()) => {
                        this.commit_summary.update(cx, |s, cx| s.set_value("", window, cx));
                        this.commit_body.update(cx, |s, cx| s.set_value("", window, cx));
                        if let Some(c) = &mut this.changes {
                            c.left_out.clear();
                        }
                        this.notice = Some(Notice::said(format!("committed {} to {branch}", plural(n, "file", "files"))));
                    }
                    Err(why) => this.branch_ask = Some(BranchAsk::stop("Not committed", format!("Nothing was committed. Git said: {why}."))),
                }
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// At the foot of the comparison's files: the commit's summary and
    /// description, the button that commits the files ticked, and under
    /// it how the branch stands against the remote's, with Push or Pull.
    pub(crate) fn render_commit_box(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (files, _) = self.commit_files();
        let summary = self.commit_summary.read(cx).value().trim().is_empty();
        let off = self.committing || files.is_empty() || summary;
        // "Commit 7 files to" and the branch's name, as GitHub Desktop
        // words it: the name in a heavier face, and the part that gives
        // way when the two do not fit.
        let branch = self.branches().map(|b| b.label()).filter(|_| !self.committing && !files.is_empty());
        let label = if self.committing { "Committing…".to_string() } else if files.is_empty() { "No files to commit".to_string() } else if branch.is_some() { format!("Commit {} to", plural(files.len(), "file", "files")) } else { format!("Commit {}", plural(files.len(), "file", "files")) };
        // A field is a hollow in the panel, as the list of branches' is.
        // In the light appearance the hollow's grey is the panel's own,
        // so there it is the page's ground inside a line.
        let dark = theme.mode.is_dark();
        let field = |id: &'static str, focus: FocusHandle| h_flex().id(id).track_focus(&focus).w_full().px(px(8.)).rounded(px(8.)).text_size(px(13.)).border_1().map(|d| if dark { d.bg(theme.muted).border_color(theme.transparent) } else { d.bg(theme.background).border_color(theme.input) });
        // The comment is the one thing a commit needs: a star at the
        // field's end says so until there is one, drawn
        // as an icon in the accent, five solid petals: the letter sat
        // high on its line, and a star of thin lines read as a scribble.
        let need = self.presence("commit-need", summary, cx);
        let star = div().flex_shrink_0().size(px(16.)).mr(px(2.)).flex().items_center().justify_center().child(Icon::default().path("icons/required.svg").with_size(px(12.)).text_color(theme.primary));
        // With a comment the star's place is the key that commits. The
        // two fade across each other in one slot, as wide as the key,
        // so the field's words keep their room.
        let key = crate::workbench::kbd_hint("⌘↩", &theme);
        let (star, key) = match need {
            Some((coming, n)) => (
                star.with_animation(ElementId::Name(format!("commit-need-{n}").into()), Animation::new(crate::workbench::FOLD_ANIM), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
                div().child(key).with_animation(ElementId::Name(format!("commit-key-{n}").into()), Animation::new(crate::workbench::FOLD_ANIM), move |d, t| d.opacity(if coming { 1. - t } else { t })).into_any_element(),
            ),
            None => (star.opacity(if summary { 1. } else { 0. }).into_any_element(), div().child(key).opacity(if summary { 0. } else { 1. }).into_any_element()),
        };
        let star = h_flex().relative().flex_shrink_0().pr(px(4.)).items_center().child(key).child(div().absolute().top_0().bottom_0().right(px(2.)).flex().items_center().child(star)).into_any_element();
        let (summary_focus, body_focus) = (self.commit_summary.read(cx).focus_handle(cx), self.commit_body.read(cx).focus_handle(cx));
        let sync = self.sync_words().map(|(said, what)| {
            h_flex()
                .w_full()
                .gap(px(8.))
                .items_center()
                .text_size(px(11.5))
                .text_color(theme.muted_foreground)
                .child(div().flex_1().min_w_0().line_height(px(16.)).child(said))
                .children(self.undo_pill(cx))
                .children(what.and_then(|pull| self.sync_pill(pull, cx)))
        });
        v_flex()
            .id("commit-box")
            .flex_shrink_0()
            .w_full()
            .p(px(10.))
            .gap(px(8.))
            .border_t_1()
            .border_color(theme.border)
            // A press here is not one on the list of files over it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, window, cx| swallow_click(window, cx))
            .child(field("commit-summary", summary_focus).h(px(32.)).items_center().child(div().flex_1().min_w_0().child(gpui_component::input::Input::new(&self.commit_summary).appearance(false).bordered(false).on_secondary_click(self.input_menu(&self.commit_summary, false, cx)))).child(star))
            .child(field("commit-body", body_focus).py(px(4.)).items_start().child(gpui_component::input::Textarea::new(&self.commit_body).appearance(false).bordered(false).on_secondary_click(self.input_menu(&self.commit_body, false, cx))))
            .child(choice_pill("commit-go", "icons/check.svg", label, Press::Primary, off, &theme).w_full().min_w_0().justify_center().children(branch.map(|b| div().min_w_0().truncate().ml(px(-2.)).font_weight(FontWeight::SEMIBOLD).child(b))).managed_tooltip(|window, cx| crate::workbench::tip("Commit (⌘↩)", window, cx)).when(!off, |d| {
                d.on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.commit_go(window, cx);
                }))
            }))
            .children(sync)
            .into_any_element()
    }

    /// A call to the remote failed while `what` was being done: the
    /// sheet that says how to sign in when that is why, else the
    /// refusal in git's words.
    pub(crate) fn net_failed(&mut self, title: &str, what: String, why: String, cx: &mut Context<Self>) {
        match git::why_net(&why) {
            git::NetTrouble::SignIn => self.open_gate(what, why, cx),
            git::NetTrouble::Offline => self.branch_ask = Some(BranchAsk::stop(title, format!("The remote could not be reached, so nothing was changed. Check the network and try again. Git said: {why}."))),
            git::NetTrouble::Other => self.branch_ask = Some(BranchAsk::stop(title, format!("Nothing was changed. Git said: {why}."))),
        }
        cx.notify();
    }

    // -- deleting a branch ----------------------------------------------------

    /// A right click on a branch, in the list or on the pill: its name
    /// to copy, and Delete for any branch but the default one, which is
    /// never deleted from here.
    pub(crate) fn branch_menu_items(&self, name: &str) -> Vec<(&'static str, MenuDo)> {
        let mut items = vec![("Copy Branch Name", MenuDo::Copy(name.to_string()))];
        if self.branches().is_some_and(|b| b.default.as_deref() != Some(name) && b.has(name)) {
            items.extend([("", MenuDo::Rule), ("Delete Branch", MenuDo::DeleteBranch(name.to_string()))]);
        }
        items
    }

    /// Delete, chosen for a branch: the question, over the list, which
    /// goes. The tick for the remote's copy starts off, as in GitHub
    /// Desktop.
    pub(crate) fn branch_delete_ask(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.branches() else { return };
        if b.default.as_deref() == Some(name.as_str()) {
            return;
        }
        let here = b.local.contains(&name);
        let there = b.origin.is_some() && if here { !b.unpublished.contains(&name) } else { b.remote.contains(&name) };
        if !here && !there {
            return;
        }
        let serial = self.branch_delete.as_ref().map_or(0, |d| d.serial + 1);
        self.branch_delete = Some(BranchDelete { name, here, there, remote_too: false, busy: false, serial });
        if self.branch_menu.is_some() {
            self.close_branch_menu(window, cx);
        }
        cx.notify();
    }

    /// Delete pressed: the branch here, then on the remote when ticked,
    /// or there alone for one only the remote has. The branch checked
    /// out is left for the default one first (`git::delete_branch`).
    /// Git may refuse either, and says so; the local half done before
    /// the remote refused is said as it is.
    pub(crate) fn branch_delete_go(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.git_dir() else { return };
        let Some(d) = self.branch_delete.as_mut() else { return };
        if d.busy {
            return;
        }
        d.busy = true;
        let (name, here, there) = (d.name.clone(), d.here, d.there && (d.remote_too || !d.here));
        let origin = self.branches().and_then(|b| b.origin.clone()).unwrap_or_else(|| "origin".into());
        let leaves = self.branches().and_then(|b| b.current.clone().filter(|c| *c == name).and(b.default.clone()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (dir, branch) = (root.clone(), name.clone());
            let done = cx
                .background_executor()
                .spawn(async move {
                    if here {
                        git::delete_branch(&dir, &branch).map_err(|why| (false, why))?;
                    }
                    if there {
                        git::delete_remote_branch(&dir, &branch).map_err(|why| (here, why))?;
                    }
                    Ok::<(), (bool, String)>(())
                })
                .await;
            this.update(cx, |this, cx| {
                this.branch_delete = None;
                match done {
                    Ok(()) => {
                        let what = match (here, there) {
                            (true, true) => format!("deleted {name} here and on {origin}"),
                            (true, false) => format!("deleted {name}"),
                            _ => format!("deleted {name} on {origin}"),
                        };
                        this.notice = Some(Notice::said(match &leaves {
                            Some(default) => format!("switched to {default} and {what}"),
                            None => what,
                        }));
                    }
                    // Here it is gone; the remote kept its copy, and
                    // the list still shows it as the remote's.
                    Err((true, why)) => {
                        this.notice = Some(Notice::error(format!("deleted {name} here, but not on {origin}: {why}")));
                        match git::why_net(&why) {
                            git::NetTrouble::SignIn => this.open_gate(format!("delete {name} on {origin}"), why, cx),
                            _ => this.branch_ask = Some(BranchAsk::stop("Not deleted on the remote", format!("{name} is deleted here, but {origin} still has it. Git said: {why}."))),
                        }
                    }
                    Err((false, why)) if here => {
                        this.notice = Some(Notice::error(format!("{name} was not deleted: {why}")));
                        this.branch_ask = Some(BranchAsk::stop("Not deleted", format!("Nothing was changed. {}{}.", why[..1].to_uppercase(), &why[1..])));
                    }
                    Err((false, why)) => {
                        this.notice = Some(Notice::error(format!("{name} was not deleted on {origin}: {why}")));
                        this.net_failed("Not deleted", format!("delete {name} on {origin}"), why, cx);
                    }
                }
                if leaves.is_some() {
                    // The files are the default branch's now.
                    this.tree.refresh(&root);
                }
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The question Delete asks: the branch by name, that it cannot be
    /// undone, and, for a branch the remote has too, the tick that
    /// deletes it there as well.
    pub(crate) fn render_branch_delete(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(d) = self.branch_delete.clone() else { return div().into_any_element() };
        let origin = self.branches().and_then(|b| b.origin.clone()).unwrap_or_else(|| "origin".into());
        let title = if d.here { format!("Delete branch {}?", d.name) } else { format!("Delete branch {} on {origin}?", d.name) };
        let tick = (d.here && d.there).then(|| {
            Checkbox::new("branch-delete-remote")
                .small()
                .checked(d.remote_too)
                .label(format!("Also delete it on {origin}"))
                .on_click(cx.listener(|this, on: &bool, window, cx| {
                    swallow_click(window, cx);
                    if let Some(d) = this.branch_delete.as_mut().filter(|d| !d.busy) {
                        d.remote_too = *on;
                    }
                    cx.notify();
                }))
                .into_any_element()
        });
        let no = choice_pill("branch-delete-cancel", "icons/close.svg", "Cancel", Press::Quiet, d.busy, &theme).when(!d.busy, |p| {
            p.on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.branch_delete = None;
                cx.notify();
            }))
        });
        let yes = choice_pill("branch-delete-yes", "icons/trash.svg", if d.busy { "Deleting…" } else { "Delete" }, Press::Danger, d.busy, &theme).when(!d.busy, |p| {
            p.on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.branch_delete_go(cx);
            }))
        });
        self.ask_card("branch-delete", d.serial, title, "This can't be undone.".into(), ["Cancel", "Delete"], tick, no, yes, cx)
    }

    /// At the list's foot: the repository's page on its host, named
    /// for the host, in the accent. None where the remote is no host.
    pub(crate) fn repo_web_pill(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let url = self.branches().and_then(|b| b.web.clone())?;
        let label = format!("View on {}", git::host_name(&git::host_of(&url).0));
        Some(small_pill("branch-web", "icons/globe.svg", label, Press::Primary, false, cx.theme()).on_click(cx.listener(move |_, _, window, cx| {
            swallow_click(window, cx);
            cx.open_url(&url);
        })))
    }

    // -- publishing -----------------------------------------------------------

    /// Send a branch made here to the remote, where it is not yet.
    pub(crate) fn branch_publish(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(root) = self.git_dir() else { return };
        if self.branch_publishing.is_some() {
            return;
        }
        let origin = self.branches().and_then(|b| b.origin.clone()).unwrap_or_else(|| "origin".into());
        self.branch_publishing = Some(name.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (dir, branch) = (root.clone(), name.clone());
            let done = cx.background_executor().spawn(async move { git::publish(&dir, &branch) }).await;
            this.update(cx, |this, cx| {
                this.branch_publishing = None;
                match done {
                    Ok(()) => this.notice = Some(Notice::said(format!("published {name} to {origin}"))),
                    Err(why) => {
                        this.notice = Some(Notice::error(format!("{name} was not published: {why}")));
                        this.net_failed("Not published", format!("publish {name}"), why, cx);
                    }
                }
                this.read_git(root, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Under the files' head, on a branch no remote has: one line
    /// saying so, and the button that publishes it.
    pub(crate) fn publish_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let b = self.branches()?;
        let cur = b.current.clone().filter(|c| b.unpublished.contains(c))?;
        let busy = self.branch_publishing.as_deref() == Some(cur.as_str());
        Some(
            h_flex()
                .w_full()
                .flex_shrink_0()
                .px(px(12.))
                .py(px(7.))
                .gap(px(8.))
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.primary.opacity(0.07))
                .text_size(px(11.5))
                .child(Icon::default().path("icons/git-branch.svg").with_size(px(12.)).text_color(theme.link).flex_shrink_0())
                .child(div().flex_1().min_w_0().truncate().text_color(theme.foreground).child("Not published yet"))
                .child(small_pill("branch-publish", "icons/arrow-up.svg", if busy { "Publishing…" } else { "Publish" }, Press::Primary, self.branch_publishing.is_some(), &theme).when(self.branch_publishing.is_none(), |d| {
                    d.on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.branch_publish(cur.clone(), cx)
                    }))
                }))
                .into_any_element(),
        )
    }

    // -- making a branch ------------------------------------------------------

    /// Open the sheet a branch is made on, with `name` in its field:
    /// from the list's button, from words that name no branch, and from
    /// ⌘⇧N anywhere.
    pub(crate) fn branch_new_open(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.git_dir() else {
            self.notice = Some(Notice::error("open a session in a git repository to make a branch"));
            cx.notify();
            return;
        };
        let Some(b) = self.branches() else {
            // Git has not been asked about this folder yet (the files
            // panel was never shown): ask, and open when it answers.
            if self.branch_new_wait.is_none() && git::inside(&root) {
                self.branch_new_wait = Some(name.unwrap_or_default());
                self.read_git(root, cx);
            } else if self.branch_new_wait.is_none() {
                self.notice = Some(Notice::error("this folder is not in a git repository"));
                cx.notify();
            }
            return;
        };
        self.branch_menu = None;
        // Made from the default branch unless that is where we are, or
        // there is none to name.
        let other_default = b.default.as_ref().filter(|d| b.current.as_ref() != Some(*d) && b.local.contains(d)).cloned();
        self.branch_new = Some(BranchNew { from_default: other_default.is_some(), sync: if other_default.is_some() && b.origin.is_some() { BaseSync::Checking } else { BaseSync::Alone }, update: false, pushing: false });
        self.branch_name_input.update(cx, |s, cx| {
            s.set_value(name.unwrap_or_default(), window, cx);
            s.focus(window, cx);
        });
        if let (Some(default), true) = (other_default, b.origin.is_some()) {
            self.branch_new_sync(root, default, true, cx);
        }
        cx.notify();
    }

    /// How the default branch stands against the remote's, asked of the
    /// remote first when `fetch`.
    fn branch_new_sync(&mut self, root: std::path::PathBuf, default: String, fetch: bool, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let dir = root.clone();
            let sync = cx
                .background_executor()
                .spawn(async move {
                    let fetched = if fetch { git::fetch(&dir) } else { Ok(()) };
                    match (fetched, git::ahead_behind(&dir, &default)) {
                        (Err(why), _) => BaseSync::Failed(why),
                        (Ok(()), Some((ahead, behind))) => BaseSync::Known { ahead, behind },
                        (Ok(()), None) => BaseSync::Alone,
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                if fetch {
                    this.branch_fetched = Some((root.clone(), match &sync {
                        BaseSync::Failed(why) => Err(why.clone()),
                        _ => Ok(Instant::now()),
                    }));
                }
                if let Some(n) = this.branch_new.as_mut() {
                    // Updating is the choice to begin with when it can be
                    // done: a branch made from an old main meets a
                    // conflict when it is merged.
                    n.update = matches!(sync, BaseSync::Known { ahead: 0, behind } if behind > 0);
                    n.sync = sync;
                    n.pushing = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn branch_new_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.branch_new = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Push the default branch's own commits from the sheet, so that the
    /// new branch starts from what the remote has too.
    fn branch_new_push(&mut self, cx: &mut Context<Self>) {
        let (Some(root), Some(default)) = (self.git_dir(), self.branches().and_then(|b| b.default.clone())) else { return };
        if let Some(n) = self.branch_new.as_mut() {
            n.pushing = true;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (dir, branch) = (root.clone(), default.clone());
            let done = cx.background_executor().spawn(async move { git::publish(&dir, &branch) }).await;
            this.update(cx, |this, cx| match done {
                Ok(()) => {
                    this.notice = Some(Notice::said(format!("pushed {default}")));
                    this.branch_new_sync(root, default, false, cx);
                }
                Err(why) => {
                    if let Some(n) = this.branch_new.as_mut() {
                        n.pushing = false;
                    }
                    this.net_failed("Not pushed", format!("push {default}"), why, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Create Branch on the sheet, or ↩ in its field.
    pub(crate) fn branch_new_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(new) = self.branch_new.clone() else { return };
        let Some(b) = self.branches() else { return };
        let name = branch_name_of(&self.branch_name_input.read(cx).value());
        if name.is_empty() || b.has(&name) {
            return;
        }
        let base = b.default.clone().filter(|_| new.from_default);
        let update = new.update && base.is_some();
        self.branch_new = None;
        self.branch_begin(name, true, base, update, window, cx);
    }

    pub(crate) fn render_branch_new(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (Some(new), Some(b)) = (self.branch_new.clone(), self.branches()) else { return div().into_any_element() };
        let focus = self.branch_name_input.read(cx).focus_handle(cx);
        let typed = self.branch_name_input.read(cx).value().to_string();
        let name = branch_name_of(&typed);
        let taken = b.has(&name);
        let default = b.default.clone().unwrap_or_default();
        let here = b.label();
        let origin = b.origin.clone().unwrap_or_else(|| "origin".into());
        let two = b.default.as_ref().is_some_and(|d| b.current.as_ref() != Some(d) && b.local.contains(d));
        let quiet = |words: String| div().text_size(px(12.)).line_height(px(18.)).text_color(theme.muted_foreground).child(words);

        let option = |id: &'static str, on: bool, title: String, line: &'static str, from_default: bool, cx: &mut Context<Self>| {
            h_flex()
                .id(id)
                .w_full()
                .px(px(12.))
                .py(px(10.))
                .gap(px(10.))
                .items_start()
                .when(on, |d| d.bg(theme.primary.opacity(0.07)))
                .when(!on, |d| d.cursor_pointer().hover(|s| s.bg(theme.muted.opacity(0.6))))
                // Rounded where the box is, the first at its top and the
                // second at its foot: a clip is a rectangle.
                .map(|d| if from_default { d.rounded_t(px(9.)) } else { d.rounded_b(px(9.)) })
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    if let Some(n) = this.branch_new.as_mut() {
                        n.from_default = from_default;
                    }
                    cx.notify();
                }))
                .child(div().mt(px(2.)).size(px(14.)).flex_shrink_0().rounded_full().border_1().border_color(if on { theme.primary } else { theme.muted_foreground.opacity(0.6) }).flex().items_center().justify_center().when(on, |d| d.child(div().size(px(8.)).rounded_full().bg(theme.primary))))
                .child(v_flex().flex_1().min_w_0().gap(px(2.)).child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title)).child(div().text_size(px(12.)).line_height(px(17.)).text_color(theme.muted_foreground).child(line)))
        };

        // How the default branch stands against the remote's, and what
        // can be done about it before the branch is made.
        let sync: Option<Option<AnyElement>> = (two && new.from_default).then(|| match &new.sync {
            BaseSync::Alone => None,
            BaseSync::Checking => Some(
                h_flex()
                    .gap(px(7.))
                    .items_center()
                    .child(Icon::new(IconName::LoaderCircle).with_size(px(12.)).text_color(theme.muted_foreground).with_animation("branch-new-checking", Animation::new(Duration::from_millis(900)).repeat(), |icon, t| icon.rotate(gpui::Radians(t * std::f32::consts::TAU))))
                    .child(quiet(format!("Asking {origin} whether {default} is up to date…")))
                    .into_any_element(),
            ),
            BaseSync::Known { ahead: 0, behind: 0 } => Some(h_flex().gap(px(7.)).items_center().child(Icon::new(IconName::Check).with_size(px(12.)).text_color(theme.green)).child(quiet(format!("{default} is up to date with {origin}."))).into_any_element()),
            BaseSync::Known { ahead: 0, behind } => Some(
                v_flex()
                    .gap(px(6.))
                    .child(
                        Checkbox::new("branch-new-update").small().checked(new.update).label(format!("Update {default} from {origin} first")).on_click(cx.listener(|this, on: &bool, window, cx| {
                            swallow_click(window, cx);
                            if let Some(n) = this.branch_new.as_mut() {
                                n.update = *on;
                            }
                            cx.notify();
                        })),
                    )
                    .child(quiet(format!("{origin} has {} that {default} here lacks. A branch made without {} can conflict when it is merged.", plural(*behind, "commit", "commits"), if *behind == 1 { "it" } else { "them" })))
                    .into_any_element(),
            ),
            BaseSync::Known { ahead, behind: 0 } => Some(
                h_flex()
                    .gap(px(8.))
                    .items_start()
                    .child(div().flex_1().min_w_0().child(quiet(format!("{default} has {} not pushed to {origin}. The new branch will have {}.", plural(*ahead, "commit", "commits"), if *ahead == 1 { "it" } else { "them" }))))
                    .child(small_pill("branch-new-push", "icons/arrow-up.svg", if new.pushing { "Pushing…".to_string() } else { format!("Push {default}") }, Press::Quiet, new.pushing, &theme).when(!new.pushing, |d| {
                        d.on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.branch_new_push(cx)
                        }))
                    }))
                    .into_any_element(),
            ),
            BaseSync::Known { ahead, behind } => Some(quiet(format!("{default} here and on {origin} have gone separate ways: {} here, {} there. Bringing them together takes a merge, which is yours to make, so the branch is made from {default} as it is here.", plural(*ahead, "commit", "commits"), plural(*behind, "commit", "commits"))).into_any_element()),
            BaseSync::Failed(why) => {
                let sign_in = git::why_net(why) == git::NetTrouble::SignIn;
                let words = match git::why_net(why) {
                    git::NetTrouble::Offline => format!("Could not reach {origin} to see whether {default} is up to date. The branch will be made from {default} as it is here."),
                    git::NetTrouble::SignIn => format!("{origin} did not take your sign-in, so it is not known whether {default} is up to date. The branch will be made from {default} as it is here."),
                    git::NetTrouble::Other => format!("Could not ask {origin} whether {default} is up to date: {why}."),
                };
                let reason = why.clone();
                Some(
                    h_flex()
                        .gap(px(8.))
                        .items_start()
                        .child(Icon::new(IconName::TriangleAlert).with_size(px(12.)).mt(px(3.)).text_color(theme.danger).flex_shrink_0())
                        .child(div().flex_1().min_w_0().text_size(px(12.)).line_height(px(18.)).child(words))
                        .when(sign_in, |d| {
                            d.child(small_pill("branch-new-sign-in", "icons/circle-user.svg", "Sign in…", Press::Quiet, false, &theme).on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.open_gate("fetch".into(), reason.clone(), cx)
                            })))
                        })
                        .into_any_element(),
                )
            }
        });
        let sync = sync.flatten();

        let based = if two {
            v_flex()
                .gap(px(8.))
                .child(div().text_size(px(12.5)).child("Create branch based on…"))
                .child(
                    v_flex()
                        .w_full()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(option("new-from-default", new.from_default, default.clone(), "The default branch of the repository. Pick this to start on something new that does not depend on your current branch.", true, cx))
                        .child(div().h(px(1.)).w_full().bg(theme.border))
                        .child(option("new-from-here", !new.from_default, here.clone(), "The branch checked out. Pick this to build on the work done on it.", false, cx)),
                )
                .children(sync)
        } else if b.current.is_some() && b.current == b.default {
            v_flex().child(quiet(format!("The new branch will be based on {here}, the branch checked out, which is the default branch of the repository.")))
        } else if b.current.is_some() {
            v_flex().child(quiet(format!("The new branch will be based on {here}, the branch checked out.")))
        } else {
            v_flex().child(quiet(format!("The new branch will be based on the commit checked out ({here}).")))
        };

        let under_name = if taken {
            Some(div().text_size(px(12.)).text_color(theme.danger).child(format!("A branch named {name} already exists.")).into_any_element())
        } else if !name.is_empty() && name != typed.trim() {
            Some(quiet(format!("Will be created as {name}.")).into_any_element())
        } else {
            None
        };
        let label = if new.update && new.from_default && two { "Update and Create Branch" } else { "Create Branch" };

        let card = v_flex()
            .id("branch-new")
            .occlude()
            .on_click(|_, window, cx| swallow_click(window, cx))
            .w(px(440.))
            .max_w(gpui::relative(0.94))
            .p(px(16.))
            .gap(px(12.))
            .rounded(px(16.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow(float_shadow(&theme))
            .child(
                h_flex()
                    .items_center()
                    .child(div().flex_1().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Create a branch"))
                    .child(Button::new("branch-new-close").ghost().xsmall().icon(IconName::Close).on_click(cx.listener(|this, _, window, cx| this.branch_new_close(window, cx)))),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(div().text_size(px(12.5)).child("Name"))
                    .child(
                        h_flex()
                            .id("branch-name-field")
                            .track_focus(&focus)
                            .role(Role::TextInput)
                            .aria_label("The new branch's name")
                            .aria_value(typed.clone())
                            .px(px(8.))
                            .h(px(32.))
                            .items_center()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if taken { theme.danger.opacity(0.7) } else { theme.border })
                            .bg(theme.background)
                            .text_size(px(13.))
                            .child(div().flex_1().min_w_0().child(gpui_component::input::Input::new(&self.branch_name_input).appearance(false).bordered(false).on_secondary_click(self.input_menu(&self.branch_name_input, false, cx)))),
                    )
                    .children(under_name),
            )
            .child(based)
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(px(8.))
                    .child(choice_pill("branch-new-cancel", "icons/close.svg", "Cancel", Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.branch_new_close(window, cx);
                    })))
                    .child(choice_pill("branch-new-go", "icons/git-branch.svg", label, Press::Primary, name.is_empty() || taken, &theme).when(!(name.is_empty() || taken), |d| {
                        d.on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.branch_new_go(window, cx);
                        }))
                    })),
            );
        div()
            .id("branch-new-overlay")
            .key_context(crate::workbench::SEARCH_CONTEXT)
            .on_action(cx.listener(|this, _: &crate::workbench::Escape, window, cx| this.branch_new_close(window, cx)))
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .flex_col()
            .items_center()
            .pt(px(120.))
            .on_click(cx.listener(|this, _, window, cx| this.branch_new_close(window, cx)))
            .child(card)
            .into_any_element()
    }

    // -- signing in -----------------------------------------------------------

    /// The sheet that says how to sign in, after the remote refused
    /// `what` with `why`.
    pub(crate) fn open_gate(&mut self, what: String, why: String, cx: &mut Context<Self>) {
        self.branch_ask = None;
        self.branch_menu = None;
        self.git_gate = Some(GitGate { what, why, account: None, checking: false });
        cx.notify();
        let Some(root) = self.git_dir() else { return };
        cx.spawn(async move |this, cx| {
            let account = cx.background_executor().spawn(async move { git::account(&root) }).await;
            this.update(cx, |this, cx| {
                if let Some(g) = this.git_gate.as_mut() {
                    g.account = Some(account);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// "Check again" on the sheet: ask the remote the smallest question
    /// there is. An answer puts the sheet away.
    fn gate_check(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.git_dir() else { return };
        let Some(g) = self.git_gate.as_mut().filter(|g| !g.checking) else { return };
        g.checking = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let dir = root.clone();
            let (reached, account) = cx.background_executor().spawn(async move { (git::reach(&dir), git::account(&dir)) }).await;
            this.update(cx, |this, cx| {
                match reached {
                    Ok(()) => {
                        this.git_gate = None;
                        this.notice = Some(Notice::said("signed in: the remote answers"));
                        this.branch_fetch(true, cx);
                    }
                    Err(why) => {
                        if let Some(g) = this.git_gate.as_mut() {
                            g.checking = false;
                            g.why = why;
                            g.account = Some(account);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn render_git_gate(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(gate) = self.git_gate.clone() else { return div().into_any_element() };
        let prose = |words: String| div().text_size(px(12.5)).line_height(px(19.)).child(words);
        let quiet = |words: String| div().text_size(px(12.)).line_height(px(18.)).text_color(theme.muted_foreground).child(words);
        let step = |n: usize, title: &'static str, body: Vec<AnyElement>| {
            h_flex()
                .w_full()
                .gap(px(10.))
                .items_start()
                .child(div().size(px(20.)).flex_shrink_0().rounded_full().border_1().border_color(theme.border).flex().items_center().justify_center().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(n.to_string()))
                .child(v_flex().flex_1().min_w_0().gap(px(6.)).child(div().min_h(px(20.)).flex().items_center().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title)).children(body))
        };
        let host = gate.account.as_ref().map(|a| a.host.clone()).filter(|h| !h.is_empty()).unwrap_or_else(|| "github.com".into());
        let github = gate.account.as_ref().is_none_or(|a| a.github());
        let name = if github { "GitHub".to_string() } else { host.clone() };

        let mut steps: Vec<AnyElement> = Vec::new();
        match &gate.account {
            None => steps.push(quiet("Looking at how this machine signs in…".into()).into_any_element()),
            Some(a) if !a.github() => {
                steps.push(prose(format!("This repository is on {host}. Sign git in to it the way {host} documents, then check again.")).into_any_element());
            }
            Some(a) if a.ssh => {
                steps.push(step(1, "Check your key", vec![self.command_box("gate-ssh".into(), "ssh -T git@github.com".into(), false, cx), quiet("This repository is reached over ssh, with a key and no password. GitHub answers with your name when it knows the key.".into()).into_any_element()]).into_any_element());
                let link = small_pill("gate-ssh-docs", "icons/external-link.svg", "How to add a key", Press::Quiet, false, &theme).on_click(|_, _, cx| cx.open_url("https://docs.github.com/en/authentication/connecting-to-github-with-ssh"));
                steps.push(step(2, "Add a key if it does not", vec![h_flex().child(link).into_any_element()]).into_any_element());
            }
            Some(a) => {
                let mut n = 0;
                if a.gh.is_none() {
                    n += 1;
                    let install: AnyElement = if cfg!(target_os = "macos") {
                        self.command_box("gate-gh-install".into(), "brew install gh".into(), false, cx)
                    } else if cfg!(windows) {
                        self.command_box("gate-gh-install".into(), "winget install --id GitHub.cli".into(), false, cx)
                    } else {
                        h_flex().child(small_pill("gate-gh-linux", "icons/external-link.svg", "How to install it on Linux", Press::Quiet, false, &theme).on_click(|_, _, cx| cx.open_url("https://github.com/cli/cli/blob/trunk/docs/install_linux.md"))).into_any_element()
                    };
                    steps.push(step(n, "Install GitHub's command line tool", vec![install, quiet("It signs in through the browser and hands git the sign-in, with no token to copy.".into()).into_any_element()]).into_any_element());
                }
                if !a.gh_signed {
                    n += 1;
                    steps.push(step(n, "Sign in", vec![self.command_box("gate-gh-login".into(), "gh auth login".into(), false, cx), quiet("Choose GitHub.com and HTTPS, answer yes to signing git in with your GitHub credentials, and sign in with a web browser.".into()).into_any_element()]).into_any_element());
                } else {
                    n += 1;
                    steps.push(step(n, "Hand git the sign-in", vec![self.command_box("gate-gh-setup".into(), "gh auth setup-git".into(), false, cx), quiet("GitHub's tool is signed in on this machine, and git was refused all the same. This has git use that sign-in. If it was refused again after it, the account signed in has no right to this repository.".into()).into_any_element()]).into_any_element());
                }
            }
        }

        let entity = cx.entity().downgrade();
        let card = v_flex()
            .id("git-gate")
            .occlude()
            .on_click(|_, window, cx| swallow_click(window, cx))
            .w(px(480.))
            .max_w(gpui::relative(0.94))
            .p(px(16.))
            .gap(px(12.))
            .rounded(px(16.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow(float_shadow(&theme))
            .child(h_flex().gap(px(8.)).items_center().child(Icon::default().path("icons/github.svg").with_size(px(16.)).text_color(theme.foreground)).child(div().flex_1().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(format!("Sign in to {name}"))))
            .child(prose(format!("{name} did not take this machine's sign-in, so Emaki could not {}. Nothing was changed.", gate.what)))
            .child(div().w_full().px(px(10.)).py(px(7.)).rounded(px(8.)).bg(theme.muted.opacity(0.6)).font_family(theme.mono_font_family.clone()).text_size(px(11.5)).line_height(px(17.)).text_color(theme.muted_foreground).child(gate.why.clone()))
            .children(steps)
            .when(github, |d| {
                d.child(quiet("Paste a command into a terminal. GitHub Desktop signs in for itself alone: being signed in there does not sign git in for other apps.".into())).child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(quiet("No account yet?".into()))
                        .child(small_pill("gate-signup", "icons/external-link.svg", "Create one on GitHub", Press::Quiet, false, &theme).on_click(|_, _, cx| cx.open_url("https://github.com/signup"))),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(px(8.))
                    .child(choice_pill("gate-close", "icons/close.svg", "Close", Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.git_gate = None;
                        cx.notify();
                    })))
                    .child(choice_pill("gate-check", "icons/refresh.svg", if gate.checking { "Checking…" } else { "Check again" }, Press::Primary, gate.checking, &theme).when(!gate.checking, |d| {
                        d.on_click(move |_, window, cx| {
                            swallow_click(window, cx);
                            let _ = entity.update(cx, |this, cx| this.gate_check(cx));
                        })
                    })),
            );
        div()
            .id("git-gate-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .flex_col()
            .items_center()
            .pt(px(100.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.git_gate = None;
                cx.notify();
            }))
            .child(card)
            .into_any_element()
    }
}
