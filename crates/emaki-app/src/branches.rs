//! What the list of branches does with a remote: asking it what it has,
//! making a branch on a sheet of its own, publishing one, and saying how
//! to sign in when the remote does not take who is asking
//! (`docs/panels.md`, Branches).

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _};

use emaki_core::git;

use crate::format::plural;
use crate::panels::BranchAsk;
use crate::workbench::{float_shadow, pill_button, swallow_click, Notice, Workbench};

/// How old the last word from the remote may be before the list of
/// branches asks again when it is opened.
const FETCH_FRESH: Duration = Duration::from_secs(60);

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
        let Some(root) = self.files_root() else { return };
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
    /// last said, or why it said nothing.
    pub(crate) fn branch_fetch_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let b = self.branches()?;
        let origin = b.origin.clone()?;
        let root = self.files_root()?;
        let last = self.branch_fetched.as_ref().filter(|(of, _)| *of == root).map(|(_, r)| r.clone());
        let row = h_flex().flex_shrink_0().min_h(px(30.)).px(px(12.)).py(px(6.)).gap(px(7.)).items_center().border_t_1().border_color(theme.border).text_size(px(11.5)).text_color(theme.muted_foreground);
        let again = |label: &'static str, cx: &mut Context<Self>| {
            let entity = cx.entity().downgrade();
            pill_button("branch-fetch-again", label, &theme, move |_, window, cx| {
                swallow_click(window, cx);
                let _ = entity.update(cx, |this, cx| this.branch_fetch(true, cx));
            })
        };
        if self.branch_fetching {
            return Some(
                row.child(Icon::new(IconName::LoaderCircle).with_size(px(12.)).text_color(theme.muted_foreground).with_animation("branch-fetching", Animation::new(Duration::from_millis(900)).repeat(), |icon, t| icon.rotate(gpui::Radians(t * std::f32::consts::TAU))))
                    .child(format!("Asking {origin} what it has…"))
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
                let entity = cx.entity().downgrade();
                let reason = why.clone();
                row.items_start()
                    .child(Icon::new(IconName::TriangleAlert).with_size(px(12.)).mt(px(3.)).text_color(theme.danger).flex_shrink_0())
                    .child(div().flex_1().min_w_0().line_height(px(17.)).text_color(theme.foreground).child(words))
                    .when(sign_in, |d| {
                        d.child(pill_button("branch-sign-in", "Sign in…", &theme, move |_, window, cx| {
                            swallow_click(window, cx);
                            let _ = entity.update(cx, |this, cx| this.open_gate("fetch".into(), reason.clone(), cx));
                        }))
                    })
                    .when(!sign_in, |d| d.child(again("Try again", cx)))
                    .into_any_element()
            }
            Some(Ok(at)) => row.child(div().flex_1().min_w_0().truncate().child(format!("Fetched from {origin} {}", crate::format::ago(crate::format::now_secs() - at.elapsed().as_secs_f64(), crate::format::now_secs())))).child(again("Fetch", cx)).into_any_element(),
            None => row.child(div().flex_1().min_w_0().truncate().child(format!("Not fetched from {origin} yet"))).child(again("Fetch", cx)).into_any_element(),
        })
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

    // -- publishing -----------------------------------------------------------

    /// Send a branch made here to the remote, where it is not yet.
    pub(crate) fn branch_publish(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
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
                .child(Button::new("branch-publish").primary().xsmall().label(if busy { "Publishing…" } else { "Publish" }).disabled(self.branch_publishing.is_some()).on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.branch_publish(cur.clone(), cx)
                })))
                .into_any_element(),
        )
    }

    // -- making a branch ------------------------------------------------------

    /// Open the sheet a branch is made on, with `name` in its field:
    /// from the list's button, from words that name no branch, and from
    /// ⌘⇧N anywhere.
    pub(crate) fn branch_new_open(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else {
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
        let (Some(root), Some(default)) = (self.files_root(), self.branches().and_then(|b| b.default.clone())) else { return };
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
                    .child(Button::new("branch-new-push").outline().xsmall().label(if new.pushing { "Pushing…".to_string() } else { format!("Push {default}") }).disabled(new.pushing).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.branch_new_push(cx)
                    })))
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
                            d.child(Button::new("branch-new-sign-in").outline().xsmall().label("Sign in…").on_click(cx.listener(move |this, _, window, cx| {
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
                    .child(Button::new("branch-new-cancel").outline().small().label("Cancel").on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.branch_new_close(window, cx);
                    })))
                    .child(Button::new("branch-new-go").primary().small().label(label).disabled(name.is_empty() || taken).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.branch_new_go(window, cx);
                    }))),
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
        let Some(root) = self.files_root() else { return };
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
        let Some(root) = self.files_root() else { return };
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
                steps.push(step(1, "Check your key", vec![self.command_box("gate-ssh".into(), "ssh -T git@github.com", cx), quiet("This repository is reached over ssh, with a key and no password. GitHub answers with your name when it knows the key.".into()).into_any_element()]).into_any_element());
                let link = pill_button("gate-ssh-docs", "How to add a key", &theme, |_, _, cx| cx.open_url("https://docs.github.com/en/authentication/connecting-to-github-with-ssh"));
                steps.push(step(2, "Add a key if it does not", vec![h_flex().child(link).into_any_element()]).into_any_element());
            }
            Some(a) => {
                let mut n = 0;
                if a.gh.is_none() {
                    n += 1;
                    let install: AnyElement = if cfg!(target_os = "macos") {
                        self.command_box("gate-gh-install".into(), "brew install gh", cx)
                    } else if cfg!(windows) {
                        self.command_box("gate-gh-install".into(), "winget install --id GitHub.cli", cx)
                    } else {
                        h_flex().child(pill_button("gate-gh-linux", "How to install it on Linux", &theme, |_, _, cx| cx.open_url("https://github.com/cli/cli/blob/trunk/docs/install_linux.md"))).into_any_element()
                    };
                    steps.push(step(n, "Install GitHub's command line tool", vec![install, quiet("It signs in through the browser and hands git the sign-in, with no token to copy.".into()).into_any_element()]).into_any_element());
                }
                if !a.gh_signed {
                    n += 1;
                    steps.push(step(n, "Sign in", vec![self.command_box("gate-gh-login".into(), "gh auth login", cx), quiet("Choose GitHub.com and HTTPS, answer yes to signing git in with your GitHub credentials, and sign in with a web browser.".into()).into_any_element()]).into_any_element());
                } else {
                    n += 1;
                    steps.push(step(n, "Hand git the sign-in", vec![self.command_box("gate-gh-setup".into(), "gh auth setup-git", cx), quiet("GitHub's tool is signed in on this machine, and git was refused all the same. This has git use that sign-in. If it was refused again after it, the account signed in has no right to this repository.".into()).into_any_element()]).into_any_element());
                }
            }
        }

        let entity = cx.entity().downgrade();
        let card = v_flex()
            .id("git-gate")
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
                        .child(pill_button("gate-signup", "Create one on GitHub", &theme, |_, _, cx| cx.open_url("https://github.com/signup"))),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(px(8.))
                    .child(Button::new("gate-close").outline().small().label("Close").on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.git_gate = None;
                        cx.notify();
                    })))
                    .child(Button::new("gate-check").primary().small().label(if gate.checking { "Checking…" } else { "Check again" }).disabled(gate.checking).on_click(move |_, window, cx| {
                        swallow_click(window, cx);
                        let _ = entity.update(cx, |this, cx| this.gate_check(cx));
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
