//! The agents page: every coding agent the catalogue knows as a card
//! that says whether it is installed here, and inside one, how to
//! install it and sign in, and the sessions kept of it
//! (`docs/agents.md`).

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use emaki_core::agents::{self, Agent, Found, Os};
use emaki_core::transcript::SessionRef;

use crate::format::plural;
use crate::workbench::{page_in, Page, Press, Scope, Workbench, CONTENT_W};

/// How old what was found may be before a visit to the page looks again.
const AGENTS_FRESH: Duration = Duration::from_secs(20);

/// A command's box of one line, and the shell's button beside it.
const COMMAND_H: f32 = 38.;

/// How long the network's word on the install commands stands.
const COMMANDS_FRESH: Duration = Duration::from_secs(24 * 60 * 60);

/// How long what a choice of the install step shows takes to change.
const SWAP_ANIM: Duration = Duration::from_millis(240);

/// A change of the install step's choices: when, how tall what it
/// showed was, and the choices it showed that under.
#[derive(Clone, Copy)]
pub(crate) struct AgentSwap {
    at: Instant,
    h: f32,
    /// How tall the git step's words were.
    git_h: f32,
    os: Os,
    way: &'static str,
}

/// The way of installing that is a download and not a command.
const DOWNLOAD: &str = "Download";

/// The icon before a choice of the install step: a system's, or a
/// way's, by what does the installing. None for any other control.
pub(crate) fn choice_icon(control: &str, key: &str) -> Option<&'static str> {
    if control == "agent-os" {
        return Some(match key {
            "mac" => "icons/os/mac.svg",
            "windows" => "icons/os/windows.svg",
            _ => "icons/os/linux.svg",
        });
    }
    // Settings, Appearance: what each look is.
    if control == "appearance" || control == "term-theme" {
        return Some(match key {
            "light" => "icons/sun.svg",
            "dark" => "icons/moon.svg",
            _ => "icons/device-desktop.svg",
        });
    }
    if !control.starts_with("agent-way") {
        return None;
    }
    Some(match key.split_whitespace().next().unwrap_or("") {
        DOWNLOAD => "icons/ways/download.svg",
        "Homebrew" => "icons/ways/homebrew.svg",
        "npm" => "icons/ways/npm.svg",
        "winget" => "icons/box.svg",
        _ => "icons/terminal.svg",
    })
}

/// The agents with a mark of their own in `assets/icons/agents`; the
/// rest wear the first letter of their name.
const MARKS: &[&str] = &["codex", "github"];

/// The colour everything of an agent's wears on this page where the
/// accent would stand: its own, or the accent for one with none.
fn agent_ink(a: &Agent, theme: &gpui_component::Theme) -> Hsla {
    match a.reads {
        Some(agent) => crate::workbench::agent_color(agent, theme),
        None if !a.accent.is_empty() => gpui::Rgba::try_from(crate::look::accent_hex(a.accent, theme.mode.is_dark())).map(Hsla::from).unwrap_or(theme.primary),
        None => theme.primary,
    }
}

/// An agent's mark at `size`: in the agent's own colour for one whose
/// sessions are read, else in `color`.
pub fn agent_mark(a: &Agent, size: Pixels, color: Hsla, theme: &gpui_component::Theme) -> AnyElement {
    if let Some(agent) = a.reads {
        return crate::workbench::agent_icon(agent, size, crate::workbench::agent_color(agent, theme)).into_any_element();
    }
    if MARKS.contains(&a.id) {
        // One with a colour of its own wears it, as an agent's mark does.
        let color = if a.accent.is_empty() { color } else { agent_ink(a, theme) };
        return Icon::default().path(SharedString::from(format!("icons/agents/{}.svg", a.id))).with_size(size).text_color(color).into_any_element();
    }
    let letter: String = a.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    div().size(size).flex().items_center().justify_center().text_size(size * 0.82).line_height(size).font_weight(FontWeight::SEMIBOLD).text_color(color).child(letter).into_any_element()
}

impl Workbench {
    /// The agents page: the cards, or inside one agent.
    pub(crate) fn show_agents(&mut self, agent: Option<&'static str>, cx: &mut Context<Self>) {
        if self.agent_open != agent || self.page != Page::Agents {
            self.agents_page_scroll.set_offset(point(px(0.), px(0.)));
        }
        self.agent_open = agent;
        self.page = Page::Agents;
        self.sidebar_peek = false;
        self.save_ui(true);
        self.check_agents(false, cx);
        cx.notify();
    }

    /// Look for every agent again, off the main thread: each is asked
    /// its version, which takes a moment. Not `force`d, what was found a
    /// moment ago stands.
    pub(crate) fn check_agents(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.agents_checking || (!force && self.agents_checked.is_some_and(|at| at.elapsed() < AGENTS_FRESH)) {
            return;
        }
        self.agents_checking = true;
        cx.notify();
        // Whether what each install command fetches is still published:
        // asked of the network at the button, and otherwise once a day,
        // on a visit to the page. Never at launch.
        if self.page == Page::Agents && (force || self.agents_commands_checked.is_none_or(|at| at.elapsed() > COMMANDS_FRESH)) {
            self.agents_commands_checked = Some(Instant::now());
            let asked = cx.background_spawn(async move { agents::stale_commands() });
            cx.spawn(async move |this, cx| {
                let stale = asked.await;
                let _ = this.update(cx, |this, cx| {
                    this.agents_stale = stale.into_iter().collect();
                    cx.notify();
                });
            })
            .detach();
        }
        self.look_for_agents(cx);
    }

    /// Look for the agents and git on this machine, and nothing on the
    /// network. Also every few seconds while the shell is open beside an
    /// agent's page (`term_panel_watch`), so a step ticks once its
    /// command has run, with nothing to press.
    pub(crate) fn look_for_agents(&mut self, cx: &mut Context<Self>) {
        self.agents_checking = true;
        let task = cx.background_spawn(async move { (agents::detect_all(), agents::git_here(), agents::way_programs_in(&agents::search_dirs())) });
        cx.spawn(async move |this, cx| {
            let (found, git, progs) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.agents_progs = Some(progs);
                this.agents_found = found.into_iter().collect();
                if this.agents_git.as_ref().is_none_or(|g| (&g.name, &g.email) != (&git.name, &git.email)) {
                    this.git_fill = Some((git.name.clone(), git.email.clone()));
                }
                this.agents_git = Some(git);
                this.agents_checking = false;
                this.agents_checked = Some(Instant::now());
                cx.notify();
            });
        })
        .detach();
    }

    /// Choose a system or a way on the install step. What it showed is
    /// kept by its choices, to be drawn going as the new comes.
    pub(crate) fn agent_choose(&mut self, os: Option<Os>, way: Option<&'static str>, cx: &mut Context<Self>) {
        self.agent_swap = Some(AgentSwap { at: Instant::now(), h: self.agent_way_h.get(), git_h: self.agent_git_h.get(), os: self.agent_os, way: self.agent_way });
        self.agent_os = os.unwrap_or(self.agent_os);
        self.agent_way = way.unwrap_or(self.agent_way);
        cx.notify();
    }

    /// What looking found for an agent; none before the first look is in.
    pub(crate) fn agent_found(&self, id: &str) -> Option<&Found> {
        self.agents_found.get(id)
    }

    /// The sessions kept of an agent, newest first.
    fn agent_sessions(&self, a: &Agent) -> Vec<&SessionRef> {
        match a.reads {
            Some(id) => self.refs.iter().filter(|r| r.agent == id).collect(),
            None => Vec::new(),
        }
    }

    pub(crate) fn render_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let open = self.agent_open.and_then(agents::by_id);
        self.selectable_n.set(0);
        // The install step's change is drawn off the clock.
        if self.agent_swap.is_some_and(|swap| swap.at.elapsed() < SWAP_ANIM) {
            window.request_animation_frame();
        }
        let body = match open {
            Some(a) => page_in("page-agent", self.agent_inside(a, cx)),
            None => page_in("page-agents", self.agents_cards(cx)),
        };
        // Inside an agent, a shell in the home folder at the page's
        // right, for the commands the page gives: the conversation's own
        // terminal panel, its shell side alone (`term_owner`).
        let right = if open.is_some() { vec![self.term_shell_button(cx)] } else { Vec::new() };
        let term = match self.term_owner() {
            Some(r) if open.is_some() => self.render_term_panel(&r, window, cx),
            _ => Vec::new(),
        };
        let page = v_flex()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .vertical_scrollbar(&self.agents_page_scroll)
            .child(v_flex().id("agents-page").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.agents_page_scroll).px(px(24.)).items_center().child(body))
            // What is selected on the page is copied as it is in a
            // conversation: a right click on a word selects it first.
            .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, window, cx| this.conversation_menu(ev.position, window, cx)));
        v_flex().flex_1().min_w_0().h_full().bg(theme.background).child(self.render_topbar(String::new(), right, cx)).child(h_flex().flex_1().min_h_0().w_full().items_stretch().child(page).children(term))
    }

    /// Words of the page as text that can be selected and copied: drawn
    /// by the markdown view, as a conversation's are, with every mark of
    /// markdown's taken as the letter it is. The size, ink and face are
    /// the element's it stands in.
    fn selectable(&self, words: &str, cx: &App) -> AnyElement {
        let n = self.selectable_n.get();
        self.selectable_n.set(n + 1);
        self.selectable_as(format!("agents-text-{n}"), words, cx)
    }

    /// `selectable`, under an id of the caller's: for words that come
    /// and go, whose number on the page is not theirs to keep.
    fn selectable_as(&self, id: String, words: &str, cx: &App) -> AnyElement {
        let mut plain = String::with_capacity(words.len() + 8);
        for c in words.chars() {
            if matches!(c, '\\' | '`' | '*' | '_' | '<' | '>' | '[' | ']' | '#' | '|' | '~' | '&' | '!' | '-' | '+' | '.' | '(' | ')' | '{' | '}' | '=' | ':') {
                plain.push('\\');
            }
            plain.push(c);
        }
        crate::transcript::md_view(id, plain, cx).into_any_element()
    }

    /// The button that looks again: the refresh mark, which turns
    /// while the look is under way.
    fn check_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let busy = self.agents_checking;
        let hover_bg = theme.muted;
        let icon = Icon::default().path("icons/refresh.svg").with_size(px(15.)).text_color(theme.muted_foreground);
        div()
            .id("agents-check")
            .size(px(28.))
            .flex_shrink_0()
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .when(!busy, |d| d.cursor_pointer().hover(move |s| s.bg(hover_bg)))
            .managed_tooltip(move |window, cx| crate::workbench::tip(if busy { "Refreshing…" } else { "Refresh status" }, window, cx))
            .on_click(cx.listener(|this, _, _, cx| this.check_agents(true, cx)))
            .child(if busy { icon.with_animation("agents-checking", Animation::new(Duration::from_millis(900)).repeat(), |icon, t| icon.rotate(gpui::Radians(t * std::f32::consts::TAU))).into_any_element() } else { icon.into_any_element() })
            .into_any_element()
    }

    /// The chip that says where an agent stands.
    /// The ways to install `a` that work here: one needing a program this
    /// machine lacks (Homebrew, npm, winget) is left out, while what is
    /// here is known.
    fn usable_ways(&self, a: &Agent, os: Os) -> Vec<&'static agents::Way> {
        a.ways(os).into_iter().filter(|w| agents::way_program(w.by).is_none_or(|p| self.agents_progs.as_ref().is_none_or(|have| have.contains(&p)))).collect()
    }

    /// Whether what a page is about is here, `None` while looking: an
    /// agent's program, and for Git git itself (`agents::git_here`), since
    /// what its entry's `bins` finds is GitHub's tool.
    fn installed_here(&self, a: &Agent) -> Option<bool> {
        if agents::tools().iter().any(|t| t.id == a.id) {
            return self.agents_git.as_ref().map(|g| g.ready);
        }
        self.agent_found(a.id).map(|f| f.installed())
    }

    fn agent_chip(&self, a: &Agent, cx: &App) -> AnyElement {
        let theme = cx.theme();
        // GitHub is not installed: it is ready once git is here and signed in.
        let tool = agents::tools().iter().any(|t| t.id == a.id);
        let signed = self.agent_found(a.id).is_some_and(|f| f.signed);
        let (color, words, dot) = match self.installed_here(a) {
            None => (theme.muted_foreground, "Looking…", false),
            Some(true) if tool && !signed => (theme.muted_foreground, "Not signed in", false),
            Some(true) => (agent_ink(a, theme), if tool { "Ready" } else { "Installed" }, true),
            Some(false) => (theme.muted_foreground, if tool { "No git" } else { "Not installed" }, false),
        };
        h_flex()
            .flex_shrink_0()
            .h(px(22.))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded_full()
            .bg(color.opacity(if theme.mode.is_dark() { 0.16 } else { 0.10 }))
            .when(dot, |d| d.child(div().size(px(6.)).rounded_full().bg(color)))
            .child(div().text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(color).child(words))
            .into_any_element()
    }

    /// The plate an agent's mark stands on.
    fn agent_tile(&self, a: &Agent, side: Pixels, cx: &App) -> Div {
        let theme = cx.theme();
        let installed = self.installed_here(a) == Some(true);
        let ink = if installed { theme.foreground } else { theme.muted_foreground };
        div()
            .size(side)
            .flex_shrink_0()
            .rounded(side * 0.26)
            .bg(if theme.mode.is_dark() { theme.muted } else { theme.foreground.opacity(0.05) })
            .flex()
            .items_center()
            .justify_center()
            .child(agent_mark(a, side * 0.5, ink, theme))
    }

    /// The page's top level: the agents side by side, a tall card each.
    fn agents_cards(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let display = crate::fonts::display_family(cx);
        // Said only when there is something to say: a command whose
        // source is gone.
        let stale = self.agents_stale.len();
        let warn = (stale > 0).then(|| div().text_size(px(12.5)).text_color(theme.danger).child(format!("{} may be out of date; the agent's page says which.", plural(stale, "install command", "install commands"))));
        // One row in the catalogue's order, whatever is installed: a
        // card says so itself, and stays where it is when that changes.
        let cards: Vec<AnyElement> = agents::all().iter().map(|a| self.agent_card(a, cx)).collect();
        // Under the agents, what is worth having beside them: a card as
        // wide as an agent's, so the row is filled out with nothing.
        let tools: Vec<AnyElement> = agents::tools().iter().map(|a| self.tool_card(a, cx)).collect();
        v_flex()
            .w_full()
            .max_w(CONTENT_W)
            .pt(px(20.))
            .pb(px(40.))
            .gap(px(20.))
            .child(h_flex().items_center().gap(px(12.)).child(div().flex_1().min_w_0().text_size(px(30.)).font_family(display.clone()).child("Agents")).child(self.check_button(cx)))
            .children(warn)
            .child(h_flex().w_full().gap(px(16.)).items_stretch().children(cards))
            .when(!tools.is_empty(), |d| {
                d.child(
                    v_flex()
                        .pt(px(12.))
                        .gap(px(4.))
                        .child(div().text_size(px(20.)).font_family(display).child("Version control")),
                )
                .child(v_flex().w_full().gap(px(16.)).children(tools))
            })
    }

    /// One line of a card's standing: a tick on a disc when it is so, a
    /// ring when it is not, and what there is to say of it at the right.
    fn agent_fact(&self, done: bool, label: &'static str, value: String, ink: Hsla, cx: &App) -> Div {
        let theme = cx.theme();
        let disc = div()
            .size(px(18.))
            .flex_shrink_0()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .map(|d| if done { d.bg(ink.opacity(0.16)).child(Icon::new(IconName::Check).with_size(px(11.)).text_color(ink)) } else { d.border_1().border_color(theme.border) });
        h_flex()
            .h(px(26.))
            .gap(px(10.))
            .items_center()
            .child(disc)
            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(if done { theme.foreground } else { theme.muted_foreground }).child(label))
            .child(div().flex_shrink_0().text_size(px(12.)).text_color(theme.muted_foreground).child(value))
    }

    fn agent_card(&self, a: &'static Agent, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let display = crate::fonts::display_family(cx);
        let ink = agent_ink(a, &theme);
        let found = self.agent_found(a.id);
        let installed = found.is_some_and(|f| f.installed());
        let signed = found.is_some_and(|f| f.signed);
        let sessions = self.agent_sessions(a).len();
        let version = found.filter(|f| !f.version.is_empty()).map(|f| format!("v{}", f.version)).unwrap_or_default();
        // What stands: the program, a sign-in when one is seen (one not
        // seen is not "signed out", so the line says only that), and the
        // sessions kept.
        let facts = v_flex()
            .py(px(10.))
            .border_t_1()
            .border_b_1()
            .border_color(theme.border)
            .child(self.agent_fact(installed, if found.is_none() { "Looking…" } else if installed { "Installed" } else { "Not installed" }, version, ink, cx))
            .child(self.agent_fact(signed, if signed { "Signed in" } else { "No sign-in seen" }, String::new(), ink, cx))
            // Sessions are an agent's to have.
            .when(a.with_emaki.is_empty(), |d| d.child(self.agent_fact(sessions > 0, if sessions > 0 { "Sessions kept" } else { "No sessions yet" }, if sessions > 0 { sessions.to_string() } else { String::new() }, ink, cx)));
        let ready = installed && signed;
        let foot = h_flex()
            .gap(px(6.))
            .items_center()
            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(if ready { theme.muted_foreground } else { ink }).child(if ready { "Details" } else { "Set up" }))
            .child(Icon::new(IconName::ChevronRight).with_size(px(14.)).text_color(if ready { theme.muted_foreground.opacity(0.7) } else { ink }));
        let id = a.id;
        v_flex()
            .id(SharedString::from(format!("agent-card-{id}")))
            .flex_1()
            .min_w_0()
            .p(px(24.))
            .gap(px(18.))
            .rounded(px(16.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.muted.opacity(0.35) } else { theme.popover })
            .cursor_pointer()
            .hover(move |s| s.border_color(ink.opacity(0.55)).bg(ink.opacity(0.05)))
            .on_click(cx.listener(move |this, _, _, cx| this.show_agents(Some(id), cx)))
            .child(self.agent_tile(a, px(52.), cx))
            .child(
                v_flex()
                    .gap(px(2.))
                    .child(div().truncate().text_size(px(24.)).line_height(px(30.)).font_family(display).child(a.name))
                    .child(div().truncate().text_size(px(12.5)).text_color(theme.muted_foreground).child(a.maker)),
            )
            .child(div().flex_1().min_h(px(40.)).text_size(px(13.)).line_height(px(20.)).text_color(theme.foreground.opacity(0.82)).child(a.about))
            .child(facts)
            .child(foot)
            .into_any_element()
    }

    /// A tool's card: low and as wide as the row of agents over it, the
    /// mark and what it is at the left, where it stands at the right.
    fn tool_card(&self, a: &'static Agent, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let display = crate::fonts::display_family(cx);
        let ink = agent_ink(a, &theme);
        // Git, and GitHub signed in: the second is recommended, so the card
        // reads as set up with git alone.
        let git = self.installed_here(a);
        let installed = git == Some(true);
        let signed = self.agent_found(a.id).is_some_and(|f| f.signed);
        let ready = installed && signed;
        let facts = v_flex()
            .w(px(200.))
            .flex_shrink_0()
            .pl(px(20.))
            .border_l_1()
            .border_color(theme.border)
            .child(self.agent_fact(installed, match git { None => "Looking…", Some(true) => "Git installed", Some(false) => "No git" }, String::new(), ink, cx))
            .child(self.agent_fact(signed, if signed { "GitHub signed in" } else { "No GitHub sign-in seen" }, String::new(), ink, cx));
        let foot = h_flex()
            .flex_shrink_0()
            .gap(px(6.))
            .items_center()
            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(if ready { theme.muted_foreground } else { ink }).child(if ready { "Details" } else { "Set up" }))
            .child(Icon::new(IconName::ChevronRight).with_size(px(14.)).text_color(if ready { theme.muted_foreground.opacity(0.7) } else { ink }));
        let id = a.id;
        h_flex()
            .id(SharedString::from(format!("agent-card-{id}")))
            .w_full()
            .p(px(24.))
            .gap(px(20.))
            .items_center()
            .rounded(px(16.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.muted.opacity(0.35) } else { theme.popover })
            .cursor_pointer()
            .hover(move |s| s.border_color(ink.opacity(0.55)).bg(ink.opacity(0.05)))
            .on_click(cx.listener(move |this, _, _, cx| this.show_agents(Some(id), cx)))
            .child(self.agent_tile(a, px(52.), cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.))
                    .child(h_flex().gap(px(10.)).items_baseline().child(div().truncate().text_size(px(22.)).line_height(px(28.)).font_family(display).child(a.name)).child(div().truncate().text_size(px(12.5)).text_color(theme.muted_foreground).child(a.maker)))
                    .child(div().text_size(px(13.)).line_height(px(20.)).text_color(theme.foreground.opacity(0.82)).child(a.about)),
            )
            .child(facts)
            .child(foot)
            .into_any_element()
    }

    /// A command on a plate, in the code face, with a button that copies
    /// it.
    /// A command in its box, with a button to copy it and, where it is
    /// one to run on this machine (`term`), the shell's button just
    /// outside the box's right edge: the one at the top right, which
    /// opens or closes the same panel, so the command is run beside it.
    pub(crate) fn command_box(&self, key: String, command: SharedString, term: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        // The command's words are known by its box, not by their
        // number on the page.
        let text_id = format!("command-{key}");
        let key = SharedString::from(key);
        let done = self.copied.as_ref() == Some(&key);
        // Inside an agent's page the tick is in that agent's colour.
        let tick = Some(self.page == Page::Agents).filter(|on| *on).and(self.agent_open).and_then(agents::by_id).map(|a| agent_ink(a, &theme)).unwrap_or(theme.green);
        let hover_bg = theme.foreground.opacity(0.08);
        let copy = div()
            .id(key.clone())
            .size(px(26.))
            .flex_shrink_0()
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .managed_tooltip(move |window, cx| crate::workbench::tip(if done { "Copied" } else { "Copy" }, window, cx))
            .on_click({
                let command = command.clone();
                cx.listener(move |this, _, _, cx| this.copy_text(key.clone(), command.to_string(), cx))
            })
            .child(Icon::new(if done { IconName::Check } else { IconName::Copy }).with_size(px(14.)).text_color(if done { tick } else { theme.muted_foreground }));
        let boxed = h_flex()
            .w_full()
            .min_h(px(COMMAND_H))
            .pl(px(12.))
            .pr(px(6.))
            // Copy (26) and the edge: a box of one line is `COMMAND_H`,
            // the shell button's height.
            .py(px(5.))
            .gap(px(8.))
            .items_center()
            .rounded(px(9.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.background } else { theme.muted.opacity(0.6) })
            .child(div().flex_1().min_w_0().font_family(theme.mono_font_family.clone()).text_size(px(12.5)).line_height(px(19.)).child(self.selectable_as(text_id.clone(), &command, cx)))
            .child(copy);
        if !term {
            return boxed.into_any_element();
        }
        // The shell's button: a square as tall as a box of one line, drawn
        // as the box is (its edge, its corners, no ground of its own), so
        // the two read as a pair. While the shell shows it takes the box's
        // ground, fading in and out with the panel.
        let on = self.term_on().is_some();
        let (was, serial) = self.side_term_anim.map(|(from, _, _, n)| (from.is_some(), n + 1)).unwrap_or((on, 0));
        let ground = if theme.mode.is_dark() { theme.background } else { theme.muted.opacity(0.6) };
        let fill = (on || was).then(|| {
            let (a, b) = (if was { 1. } else { 0. }, if on { 1. } else { 0. });
            div().absolute().inset_0().rounded(px(9.)).bg(ground).with_animation(
                ElementId::Name(format!("{text_id}-term-fill-{serial}").into()),
                Animation::new(Duration::from_millis(220)).with_easing(ease_out_quint()),
                move |d, t| d.opacity(a + (b - a) * t),
            )
        });
        let lit = theme.foreground;
        let hover = theme.foreground.opacity(0.04);
        let shell = div()
            .id(SharedString::from(format!("{text_id}-term")))
            .relative()
            .size(px(COMMAND_H))
            .flex_shrink_0()
            .rounded(px(9.))
            .border_1()
            .border_color(theme.border)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(if on { lit } else { theme.muted_foreground })
            .hover(move |s| s.bg(hover).text_color(lit))
            .managed_tooltip(|window, cx| crate::workbench::tip("Shell Terminal in your home folder (⌘⇧S)", window, cx))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_term(false, window, cx)))
            .children(fill)
            .child(Icon::default().path("icons/terminal.svg").with_size(px(15.)).relative());
        // Centred on the box, however many lines the command takes.
        h_flex().w_full().gap(px(8.)).items_center().child(boxed.flex_1().min_w_0()).child(shell).into_any_element()
    }

    /// One step of setting an agent up: its number on a disc, a tick
    /// once it is done, and a line down to the next.
    /// One step on the rail: its tick or number, its title, and under the
    /// title only what to do. What is so on this machine (the version
    /// found, the name commits are signed with) stands at the title's
    /// right, quiet and slanted, so it is never read as a thing to do;
    /// the tick already says the step is done.
    fn agent_step(&self, n: usize, done: bool, last: bool, title: String, status: Option<String>, body: Div, ink: Hsla, cx: &App) -> Div {
        let theme = cx.theme();
        let disc = div()
            .size(px(24.))
            .flex_shrink_0()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .map(|d| if done { d.bg(ink.opacity(0.16)).child(Icon::new(IconName::Check).with_size(px(13.)).text_color(ink)) } else { d.border_1().border_color(theme.border).text_color(theme.muted_foreground).child(n.to_string()) });
        let rail = v_flex().w(px(24.)).flex_shrink_0().items_center().gap(px(6.)).child(disc).when(!last, |d| d.child(div().w(px(1.)).flex_1().min_h(px(12.)).bg(theme.border)));
        let head = h_flex()
            .min_h(px(24.))
            .gap(px(10.))
            .items_center()
            .child(div().flex_shrink_0().text_size(px(14.5)).font_weight(FontWeight::MEDIUM).child(title))
            .when_some(status, |d, said| {
                // Keyed on its words, so a new status fades in once.
                let id = ElementId::Name(format!("agent-step-status-{n}-{said}").into());
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .justify_end()
                        .child(div().min_w_0().truncate().text_size(px(12.)).italic().text_color(theme.muted_foreground).child(said))
                        .with_animation(id, Animation::new(Duration::from_millis(220)), |d, t| d.opacity(t)),
                )
            });
        h_flex().w_full().gap(px(14.)).items_stretch().child(rail).child(v_flex().flex_1().min_w_0().gap(px(10.)).pb(if last { px(0.) } else { px(22.) }).child(head).child(body))
    }

    /// Inside one agent: what it is, the steps to have it working, and
    /// the sessions kept of it.
    fn agent_inside(&self, a: &'static Agent, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let display = crate::fonts::display_family(cx);
        let ink = agent_ink(a, &theme);
        // The pills here light up in the agent's colour, not the accent.
        let mut tinted = theme.clone();
        tinted.primary = ink;
        let found = self.agent_found(a.id).cloned();
        let installed = found.as_ref().is_some_and(|f| f.installed());
        let prose = |words: String, cx: &App| div().text_size(px(13.)).line_height(px(20.)).text_color(theme.foreground.opacity(0.85)).child(self.selectable(&words, cx));
        let quiet = |words: String, cx: &App| div().text_size(px(12.)).line_height(px(18.)).text_color(theme.muted_foreground).child(self.selectable(&words, cx));

        let hover = theme.foreground;
        let crumb = h_flex()
            .gap(px(6.))
            .items_center()
            .text_size(px(12.5))
            .text_color(theme.muted_foreground)
            .child(div().id("agents-up").cursor_pointer().hover(move |s| s.text_color(hover)).on_click(cx.listener(|this, _, _, cx| this.show_agents(None, cx))).child("Agents"))
            .child(Icon::new(IconName::ChevronRight).with_size(px(11.)))
            .child(div().min_w_0().truncate().child(a.name));

        let link = |id: &'static str, label: &'static str, url: &'static str, theme: &gpui_component::Theme| crate::branches::small_pill(id, "icons/external-link.svg", label, Press::Quiet, false, theme).on_click(move |_, _, cx| cx.open_url(url));
        let head = h_flex()
            .gap(px(16.))
            .items_center()
            .child(self.agent_tile(a, px(56.), cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(h_flex().gap(px(12.)).items_center().child(div().min_w_0().truncate().text_size(px(30.)).font_family(display).child(a.name)).child(self.agent_chip(a, cx)))
                    .child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child(a.maker)),
            );
        // The steps are this machine's: what the page can see and tick.
        let os = Os::here();
        let links = h_flex()
            .gap(px(8.))
            .items_center()
            .child(link("agent-site", "Website", a.site, &tinted))
            .child(link("agent-docs", "Documentation", a.docs, &tinted))
            .child(div().flex_1())
            .child(self.check_button(cx));
        // A step's words, a line a thing to do, each with its number.
        // A line that leads with a button is the button's height, and its
        // number stands level with its words.
        let numbered_at = |i: usize, tall: bool, line: AnyElement| h_flex().w_full().items_start().gap(px(8.)).child(div().w(px(16.)).flex_shrink_0().text_size(px(13.)).line_height(px(if tall { 24. } else { 20. })).text_color(theme.muted_foreground).child(format!("{i}."))).child(v_flex().flex_1().min_w_0().gap(px(8.)).child(line));
        let numbered = |i: usize, line: AnyElement| numbered_at(i, false, line);

        // Installing: where it is once it is there, and the ways to get
        // it for the system chosen, which stay: for another machine, or
        // to install it again.
        // Where it is, beside the step's title: the version and the path.
        let here = found.as_ref().filter(|f| f.installed()).map(|f| {
            let path = f.path.as_ref().map(|p| tilde(&p.to_string_lossy()));
            match (f.version.is_empty(), path) {
                (false, Some(p)) => format!("{} · {p}", f.version),
                (false, None) => f.version.clone(),
                (true, Some(p)) => p,
                (true, None) => "Installed".to_string(),
            }
        });
        let install = {
            // The way on the system chosen above. An app is downloaded first, where its
            // maker has one for the system; each command is another
            // way. One way shows at a time, the maker's first choice to
            // begin with.
            let hows_on = |os: Os| -> Vec<&'static str> {
                let ways = self.usable_ways(a, os);
                let download = !a.download.is_empty() && (!ways.is_empty() || a.install.is_empty());
                download.then_some(DOWNLOAD).into_iter().chain(ways.iter().map(|w| w.by)).collect()
            };
            let how_on = |os: Os, wanted: &'static str| -> Option<&'static str> {
                let hows = hows_on(os);
                hows.iter().copied().find(|key| *key == wanted).or(hows.first().copied())
            };
            let hows = hows_on(os);
            let how = how_on(os, self.agent_way);
            let mut pick = h_flex().gap(px(8.)).items_center();
            if let (Some(how), true) = (how, hows.len() > 1) {
                let on: Rc<dyn Fn(&mut Self, &'static str, &mut Window, &mut Context<Self>)> = Rc::new(|this, key, _, cx| this.agent_choose(None, Some(key), cx));
                // A control a system, so the plate does not slide from
                // one system's ways to another's.
                let control = match os {
                    Os::Mac => "agent-way-mac",
                    Os::Linux => "agent-way-linux",
                    Os::Windows => "agent-way-windows",
                };
                let short = |by: &'static str| by.split(" (").next().unwrap_or(by).to_string();
                let options = hows.iter().map(|by| (*by, short(by), None)).collect();
                pick = pick.child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("Install with")).child(self.segmented_sized(control, options, how, on, true, cx));
            }
            // What a way on a system says to do. Its words are known by
            // the system and the way, never by their place on the page:
            // the markdown view keeps what it has parsed under its id,
            // and parses off the main thread, so words given another id
            // are blank for a frame.
            let body = |os: Os, how: Option<&'static str>, cx: &mut Context<Self>| -> Div {
                let id = |part: &str| format!("agent-way-text-{}-{}-{}-{part}", a.id, os.as_str(), how.unwrap_or("none"));
                let prose = |part: &str, words: String, cx: &App| div().text_size(px(13.)).line_height(px(20.)).text_color(theme.foreground.opacity(0.85)).child(self.selectable_as(id(part), &words, cx));
                let quiet = |part: &str, words: String, cx: &App| div().text_size(px(12.)).line_height(px(18.)).text_color(theme.muted_foreground).child(self.selectable_as(id(part), &words, cx));
                let col = v_flex().w_full().gap(px(10.));
                match how {
                    None => col.child(quiet("none", format!("No installer for {} is listed here. See {}'s documentation.", os.label(), a.name), cx)),
                    Some(DOWNLOAD) => {
                        let url = a.download;
                        let open = match os {
                            Os::Mac => format!("Open it and move {} to Applications.", a.name),
                            Os::Windows => "Open it and install.".to_string(),
                            Os::Linux => "Install it with your software installer.".to_string(),
                        };
                        col.child(numbered_at(1, true, h_flex().gap(px(10.)).items_center().child(prose("download", format!("Download {} from {}:", a.name, a.maker), cx)).child(crate::branches::small_pill("agent-download", "icons/ways/download.svg", "Download", crate::workbench::Press::Quiet, false, &tinted).on_click(move |_, _, cx| cx.open_url(url))).into_any_element()))
                            .child(numbered(2, prose("download-open", open, cx).into_any_element()))
                    }
                    Some(by) => {
                        let ways = self.usable_ways(a, os);
                        let Some((i, w)) = ways.iter().enumerate().find(|(_, w)| w.by == by) else { return col };
                        // What to do, then the command: the way's name is
                        // the choice over it, and says nothing more here.
                        let gone = self.agents_stale.contains(w.command);
                        col.child(numbered(
                            1,
                            v_flex()
                                .gap(px(8.))
                                .child(prose("run", if os == Os::here() { "Run in the terminal:".into() } else { "Run in a terminal on that computer:".into() }, cx))
                                .child(self.command_box(format!("agent-way-{}-{}-{i}", a.id, os.as_str()), w.command.into(), os == Os::here(), cx))
                                .when(gone, |d| d.child(div().text_size(px(12.)).line_height(px(18.)).text_color(theme.danger).child(format!("This command's source is no longer published. See {}'s documentation.", a.name))))
                                .into_any_element(),
                        ))
                        .child(numbered(
                            2,
                            if os == Os::Windows {
                                prose("again", "When it finishes, open a new terminal.".into(), cx).into_any_element()
                            } else {
                                v_flex()
                                    .gap(px(8.))
                                    .child(prose("again", "When it finishes, run any line it asks you to, then restart the terminal:".into(), cx))
                                    .child(self.command_box(format!("agent-again-{}-{}", a.id, os.as_str()), agents::SHELL_AGAIN.into(), os == Os::here(), cx))
                                    .into_any_element()
                            },
                        ))
                    }
                }
            };
            // A change of either choice is in motion: what was there
            // fades out where it stood as the new fades in, and their
            // room runs from the old height to the new, so the steps
            // under it slide. Drawn off the clock in plain boxes: an
            // animation's element has an id, which is part of where
            // the markdown views keep their words, and wrapping them
            // in one and taking it off again blanked them twice.
            let tall = self.agent_way_h.clone();
            let now = body(os, how, cx).relative().child(canvas(move |bounds, _, _| tall.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().inset_0());
            // A choice that shows the same words again changes nothing.
            let shown = match self.agent_swap.filter(|swap| swap.at.elapsed() < SWAP_ANIM && (swap.os, how_on(swap.os, swap.way)) != (os, how)) {
                Some(swap) => {
                    let t = gpui::ease_out_quint()(swap.at.elapsed().as_secs_f32() / SWAP_ANIM.as_secs_f32());
                    let to = self.agent_way_h.get();
                    let was = body(swap.os, how_on(swap.os, swap.way), cx);
                    div().relative().w_full().overflow_hidden().h(px(swap.h + (to - swap.h) * t)).child(now.opacity(t)).child(was.absolute().top_0().left_0().opacity(1. - t))
                }
                None => div().relative().w_full().child(now),
            };
            v_flex().gap(px(12.)).child(pick).child(shown)
        };

        // Git, which the agents and Emaki's own panel work with: on a Mac
        // it comes with Apple's command line tools, which a new Mac does
        // not have. Its words follow the system as the install step's do.
        let git = self.agents_git.clone();
        let git_ready = git.as_ref().is_some_and(|g| g.ready);
        // What this machine has, beside the tick that says so, whichever
        // system's steps are shown.
        let git_status = git_ready.then(|| if Os::here() == Os::Mac { "Command line tools installed".to_string() } else { "Git installed".to_string() });
        let git_step = {
            let git_body = |os: Os, cx: &mut Context<Self>| -> Div {
                let id = |part: &str| format!("agent-git-text-{}-{}-{part}", a.id, os.as_str());
                let prose = |part: &str, words: String, cx: &App| div().text_size(px(13.)).line_height(px(20.)).text_color(theme.foreground.opacity(0.85)).child(self.selectable_as(id(part), &words, cx));
                let col = v_flex().w_full().gap(px(10.));
                match os {
                    Os::Mac => col
                        .child(prose("what", "On a Mac, git comes with Apple's command line tools. Run in the terminal:".into(), cx))
                        .child(self.command_box(format!("agent-git-{}-mac", a.id), agents::GIT_MAC.into(), os == Os::here(), cx)),
                    Os::Linux => col
                        .child(prose("what", "Most Linux systems have git. If not, on Ubuntu or Debian, run:".into(), cx))
                        .child(self.command_box(format!("agent-git-{}-linux", a.id), agents::GIT_LINUX.into(), os == Os::here(), cx)),
                    Os::Windows => col
                        .child(prose("what", "Download Git for Windows and install it with the default choices.".into(), cx))
                        .child(h_flex().child(crate::branches::small_pill("agent-git-download", "icons/ways/download.svg", "Download Git", crate::workbench::Press::Quiet, false, &tinted).on_click(|_, _, cx| cx.open_url(agents::GIT_WINDOWS)))),
                }
            };
            let tall = self.agent_git_h.clone();
            let now = git_body(os, cx).relative().child(canvas(move |bounds, _, _| tall.set(f32::from(bounds.size.height)), |_, _, _, _| {}).absolute().inset_0());
            match self.agent_swap.filter(|swap| swap.at.elapsed() < SWAP_ANIM && swap.os != os) {
                Some(swap) => {
                    let t = gpui::ease_out_quint()(swap.at.elapsed().as_secs_f32() / SWAP_ANIM.as_secs_f32());
                    let to = self.agent_git_h.get();
                    let was = git_body(swap.os, cx);
                    div().relative().w_full().overflow_hidden().h(px(swap.git_h + (to - swap.git_h) * t)).child(now.opacity(t)).child(was.absolute().top_0().left_0().opacity(1. - t))
                }
                None => div().relative().w_full().child(now),
            }
        };

        let signed = found.as_ref().is_some_and(|f| f.signed);
        let mut sign = v_flex().gap(px(10.));
        // What to do stays once it is done, for signing in again or as
        // someone else; that it is done is the tick.
        if a.sign_in.is_empty() {
            // Signed in to in its own window, with nothing to type.
            sign = sign.child(prose(a.sign_in_how.to_string(), cx));
        } else {
            sign = sign
                .child(numbered(1, v_flex().gap(px(8.)).child(prose(if os == Os::here() { "Run in the terminal:".into() } else { "Run in a terminal on that computer:".into() }, cx)).child(self.command_box("agent-sign-in".into(), a.sign_in.into(), os == Os::here(), cx)).into_any_element()))
                .child(numbered(2, prose(a.sign_in_how.to_string(), cx).into_any_element()));
        }
        if !a.plans.is_empty() {
            sign = sign.child(quiet(a.plans.to_string(), cx));
        }

        // Who commits are by: git asks before the first one, and a commit
        // from Emaki's panel is refused until it knows. On GitHub's page,
        // where git is the point. The line is written from the two fields.
        let identity = (!a.with_emaki.is_empty()).then(|| {
            let (known_name, known_email) = git.as_ref().map(|g| (g.name.clone(), g.email.clone())).unwrap_or_default();
            let typed = |input: &Entity<gpui_component::input::InputState>, known: &str| {
                let v = input.read(cx).value().trim().to_string();
                if v.is_empty() { known.to_string() } else { v }
            };
            let name = typed(&self.git_name_input, &known_name);
            let email = typed(&self.git_email_input, &known_email);
            let well = if theme.mode.is_dark() { theme.sidebar } else { theme.muted };
            let field = |id: &'static str, label: &'static str, input: &Entity<gpui_component::input::InputState>, cx: &mut Context<Self>| {
                v_flex().flex_1().min_w_0().gap(px(4.)).child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground).child(label)).child(
                    div()
                        .id(id)
                        .track_focus(&input.read(cx).focus_handle(cx))
                        .h(px(32.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .bg(well)
                        .text_size(px(13.))
                        .child(gpui_component::input::Input::new(input).appearance(false).bordered(false).on_secondary_click(self.input_menu(input, false, cx))),
                )
            };
            // Always the command: run again, it changes the name. The name
            // and email are this machine's own, read from git.
            let mut body = v_flex().gap(px(10.));
            body = body.child(prose("Git needs a name and email to commit. Use an email on your GitHub account, so GitHub credits the commits to you.".into(), cx));
            body = body.child(h_flex().gap(px(10.)).child(field("git-name-field", "Name", &self.git_name_input, cx)).child(field("git-email-field", "Email", &self.git_email_input, cx)));
            if !name.is_empty() && !email.is_empty() {
                let line = agents::git_identity_command(&name, &email, os);
                body = body.child(prose(if os == Os::here() { "Run in the terminal:".into() } else { "Run in a terminal on that computer:".into() }, cx)).child(self.command_box(format!("agent-identity-{}", os.as_str()), line.into(), os == Os::here(), cx));
            } else {
                body = body.child(quiet("Type both to get the line to run.".into(), cx));
            }
            (!known_name.is_empty() && !known_email.is_empty(), body)
        });

        let sessions = self.agent_sessions(a);

        // Git's page: GitHub, recommended and not needed, in one step:
        // GitHub's tool and its sign-in, which git then uses to fetch and
        // publish, and GitHub Desktop offered in a line, needed by nothing.
        let tool = agents::tools().iter().any(|t| t.id == a.id);
        let github = tool.then(|| {
            let here = os == Os::here();
            let mut body = v_flex().gap(px(10.)).child(prose("GitHub keeps your repositories online, and Emaki fetches and publishes there once git is signed in.".into(), cx));
            let install = match self.usable_ways(a, os).first() {
                Some(w) => v_flex().gap(px(8.)).child(prose(if here { "Run in the terminal:".into() } else { "Run in a terminal on that computer:".into() }, cx)).child(self.command_box(format!("agent-gh-{}", os.as_str()), w.command.into(), here, cx)),
                None => v_flex().items_start().gap(px(8.)).child(prose("Install GitHub's tool:".into(), cx)).child(crate::branches::small_pill("agent-gh-get", "icons/external-link.svg", "GitHub CLI", crate::workbench::Press::Quiet, false, &tinted).on_click(|_, _, cx| cx.open_url(agents::GH_INSTALL))),
            };
            body = body
                .child(numbered(1, install.into_any_element()))
                .child(numbered(2, v_flex().gap(px(8.)).child(prose(format!("Then sign in, so git can fetch and publish as you. {}", a.sign_in_how), cx)).child(self.command_box("agent-gh-login".into(), a.sign_in.into(), here, cx)).into_any_element()))
                .child(
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(quiet("Prefer an app? GitHub Desktop works on the same repositories.".into(), cx))
                        .child(crate::branches::small_pill("agent-desktop-get", "icons/ways/download.svg", "GitHub Desktop", crate::workbench::Press::Quiet, false, &tinted).on_click(|_, _, cx| cx.open_url(agents::GITHUB_DESKTOP))),
                );
            body
        });

        let card = v_flex()
            .w_full()
            .p(px(20.))
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.muted.opacity(0.35) } else { theme.popover });
        let steps = match (github, identity) {
            (Some(github), Some((known, who))) => card
                .child(self.agent_step(1, git_ready, false, "Install git".into(), git_status, git_step, ink, cx))
                // Signed in first, so the email the next step asks for is
                // the account's just signed in to.
                .child(self.agent_step(2, signed, false, "Sign in to GitHub".into(), None, github, ink, cx))
                .child(self.agent_step(3, known, true, "Name your commits".into(), None, who, ink, cx)),
            // An agent's own: git is optional for Emaki and has a page of
            // its own, so it is no step here.
            _ => card
                .child(self.agent_step(1, installed, false, format!("Install {}", a.name), here, install, ink, cx))
                .child(self.agent_step(2, signed, true, "Sign in".into(), None, sign, ink, cx)),
        };

        // The way to the sessions themselves: All Sessions, narrowed to
        // this agent. One row and not a list, which said it twice.
        let kept = a.reads.filter(|_| !sessions.is_empty()).map(|id| {
            let count = sessions.len();
            h_flex()
                .id("agent-sessions")
                .w_full()
                .px(px(20.))
                .py(px(14.))
                .gap(px(12.))
                .items_center()
                .rounded(px(14.))
                .border_1()
                .border_color(theme.border)
                .bg(if theme.mode.is_dark() { theme.muted.opacity(0.35) } else { theme.popover })
                .cursor_pointer()
                .hover(move |s| s.border_color(ink.opacity(0.55)).bg(ink.opacity(0.05)))
                .on_click(cx.listener(move |this, _, _, cx| this.show_sessions_in(Scope::Agent(id), None, cx)))
                .child(Icon::default().path("icons/scroll.svg").with_size(px(16.)).text_color(ink))
                .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(plural(count, "session", "sessions")))
                .child(div().flex_shrink_0().text_size(px(12.5)).text_color(theme.muted_foreground).child("See in All Sessions"))
                .child(Icon::new(IconName::ChevronRight).with_size(px(14.)).text_color(theme.muted_foreground.opacity(0.7)))
        });

        v_flex().w_full().max_w(CONTENT_W).pt(px(20.)).pb(px(40.)).gap(px(16.)).child(crumb).child(head).child(prose(a.about.to_string(), cx)).child(links).child(steps).children(kept)
    }
}

/// A path with the home folder as "~".
fn tilde(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix(home.as_str()) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => path.to_string(),
    }
}
