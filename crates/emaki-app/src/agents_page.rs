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
use crate::workbench::{key_text, page_in, pill_button, swallow_click, Page, Scope, Workbench, CONTENT_W};

/// How old what was found may be before a visit to the page looks again.
const AGENTS_FRESH: Duration = Duration::from_secs(20);

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
const MARKS: &[&str] = &["codex", "github-desktop"];

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
        let task = cx.background_spawn(async move { (agents::detect_all(), agents::git_here()) });
        cx.spawn(async move |this, cx| {
            let (found, git) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.agents_found = found.into_iter().collect();
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
    fn agent_chip(&self, a: &Agent, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let (color, words, dot) = match self.agent_found(a.id) {
            None => (theme.muted_foreground, "Looking…", false),
            Some(f) if f.installed() => (agent_ink(a, theme), "Installed", true),
            Some(_) => (theme.muted_foreground, "Not installed", false),
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
        let installed = self.agent_found(a.id).is_some_and(|f| f.installed());
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
                        .child(div().text_size(px(20.)).font_family(display).child("GitHub"))
                        .child(div().text_size(px(13.)).line_height(px(20.)).text_color(theme.muted_foreground).child("For continuous development and version control.")),
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
        let found = self.agent_found(a.id);
        let installed = found.is_some_and(|f| f.installed());
        let signed = found.is_some_and(|f| f.signed);
        let ready = installed && signed;
        let facts = v_flex()
            .w(px(200.))
            .flex_shrink_0()
            .pl(px(20.))
            .border_l_1()
            .border_color(theme.border)
            .child(self.agent_fact(installed, if found.is_none() { "Looking…" } else if installed { "Installed" } else { "Not installed" }, String::new(), ink, cx))
            .child(self.agent_fact(signed, if signed { "Signed in" } else { "No sign-in seen" }, String::new(), ink, cx));
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
    pub(crate) fn command_box(&self, key: String, command: SharedString, cx: &mut Context<Self>) -> AnyElement {
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
        h_flex()
            .w_full()
            .min_h(px(38.))
            .pl(px(12.))
            .pr(px(6.))
            .py(px(6.))
            .gap(px(8.))
            .items_center()
            .rounded(px(9.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.background } else { theme.muted.opacity(0.6) })
            .child(div().flex_1().min_w_0().font_family(theme.mono_font_family.clone()).text_size(px(12.5)).line_height(px(19.)).child(self.selectable_as(text_id, &command, cx)))
            .child(copy)
            .into_any_element()
    }

    /// One step of setting an agent up: its number on a disc, a tick
    /// once it is done, and a line down to the next.
    fn agent_step(&self, n: usize, done: bool, last: bool, title: String, note: Option<String>, body: Div, ink: Hsla, cx: &App) -> Div {
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
            .child(div().text_size(px(14.5)).font_weight(FontWeight::MEDIUM).child(title))
            .when_some(note, |d, n| d.child(div().flex_1().min_w_0().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(n)));
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

        let link = |id: &'static str, label: &'static str, url: &'static str, theme: &gpui_component::Theme| pill_button(id, label, theme, move |_, _, cx| cx.open_url(url));
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
        // Which computer the steps are for: this one to begin with. Every
        // step under it follows.
        let os = self.agent_os;
        let os_pick = {
            let on: Rc<dyn Fn(&mut Self, &'static str, &mut Window, &mut Context<Self>)> = Rc::new(|this, key, _, cx| this.agent_choose(Os::parse(key), None, cx));
            let options = Os::ALL.iter().map(|o| (o.as_str(), o.label().to_string(), None)).collect();
            self.segmented_sized("agent-os", options, os.as_str(), on, true, cx)
        };
        let links = h_flex()
            .gap(px(8.))
            .items_center()
            .child(link("agent-site", "Website", a.site, &tinted))
            .child(link("agent-docs", "Documentation", a.docs, &tinted))
            .child(div().flex_1())
            .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("Steps for"))
            .child(os_pick)
            .child(self.check_button(cx));
        // A step's words, a line a thing to do, each with its number.
        // A line that leads with a button is the button's height, and its
        // number stands level with its words.
        let numbered_at = |i: usize, tall: bool, line: AnyElement| h_flex().w_full().items_start().gap(px(8.)).child(div().w(px(16.)).flex_shrink_0().text_size(px(13.)).line_height(px(if tall { 24. } else { 20. })).text_color(theme.muted_foreground).child(format!("{i}."))).child(v_flex().flex_1().min_w_0().gap(px(8.)).child(line));
        let numbered = |i: usize, line: AnyElement| numbered_at(i, false, line);
        // A line with a key in it, its keys drawn.
        let keys = |words: &str| key_text(words).text_size(px(13.)).line_height(px(20.)).text_color(theme.foreground.opacity(0.85)).into_any_element();
        // The terminal, opened from the step that needs it: the shell at
        // the page's right, where the button at the top right opens it.
        let open_term = |id: &'static str, cx: &mut Context<Self>| {
            crate::branches::small_pill(id, "icons/terminal.svg", "Open the Terminal", crate::workbench::Press::Quiet, false, &tinted)
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    if this.term_on().is_none() {
                        this.toggle_term(false, window, cx);
                    } else {
                        window.focus(&this.side_term_focus, cx);
                    }
                }))
                .into_any_element()
        };

        // Installing: where it is once it is there, and the ways to get
        // it for the system chosen, which stay: for another machine, or
        // to install it again.
        let here = found.as_ref().and_then(|f| f.path.as_ref().map(|p| (f, p))).map(|(f, path)| {
            v_flex()
                .gap(px(4.))
                .child(prose(if f.version.is_empty() { format!("{} is on this machine.", a.name) } else { format!("{} {} is on this machine.", a.name, f.version) }, cx))
                .child(div().font_family(theme.mono_font_family.clone()).text_size(px(12.)).text_color(theme.muted_foreground).child(self.selectable(&tilde(&path.to_string_lossy()), cx)))
        });
        let install = {
            // The way on the system chosen above. An app is downloaded first, where its
            // maker has one for the system; each command is another
            // way. One way shows at a time, the maker's first choice to
            // begin with.
            let hows_on = |os: Os| -> Vec<&'static str> {
                let ways = a.ways(os);
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
                            Os::Mac => format!("Open the file you downloaded. If it is a zip, open it, then drag {} into your Applications folder.", a.name),
                            Os::Windows => "Open the file you downloaded and follow what it says.".to_string(),
                            Os::Linux => "Open the file you downloaded with your system's software installer.".to_string(),
                        };
                        col.child(numbered_at(1, true, h_flex().gap(px(10.)).items_center().child(prose("download", format!("Download {} from {}:", a.name, a.maker), cx)).child(crate::branches::small_pill("agent-download", "icons/ways/download.svg", "Download", crate::workbench::Press::Quiet, false, &tinted).on_click(move |_, _, cx| cx.open_url(url))).into_any_element()))
                            .child(numbered(2, prose("download-open", open, cx).into_any_element()))
                            .child(numbered(3, prose("download-done", format!("Come back and press refresh above. This step gets a tick when Emaki finds {}.", a.name), cx).into_any_element()))
                    }
                    Some(by) => {
                        let ways = a.ways(os);
                        let Some((i, w)) = ways.iter().enumerate().find(|(_, w)| w.by == by) else { return col };
                        let label = if hows_on(os).first() == Some(&by) { format!("{} (recommended)", w.by) } else { w.by.to_string() };
                        let gone = self.agents_stale.contains(w.command);
                        col.child(
                            v_flex()
                                .gap(px(5.))
                                .child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground).child(label))
                                .child(self.command_box(format!("agent-way-{}-{}-{i}", a.id, os.as_str()), w.command.into(), cx))
                                .when(gone, |d| d.child(div().text_size(px(12.)).line_height(px(18.)).text_color(theme.danger).child(format!("This command's source is no longer published. See {}'s documentation.", a.name)))),
                        )
                        .child(numbered(1, prose("copy", "Copy the command with the button at its right.".into(), cx).into_any_element()))
                        .child(numbered_at(
                            2,
                            os == Os::here(),
                            if os == Os::here() { h_flex().gap(px(10.)).items_center().child(prose("open", "Open the terminal:".into(), cx)).child(open_term("agent-term-install", cx)).into_any_element() } else { prose("open", format!("Open a terminal on that computer ({}).", if os == Os::Windows { "PowerShell" } else { "Terminal" }), cx).into_any_element() },
                        ))
                        .child(numbered(
                            3,
                            v_flex()
                                .gap(px(4.))
                                .child(keys(if os == Os::Mac { "Paste it there with ⌘V and press ↩." } else { "Paste it there with Ctrl+V and press Enter." }))
                                .child(quiet("password", "If it asks for a password, type the one you log in to the computer with. Nothing shows as you type: that is normal.".into(), cx))
                                .into_any_element(),
                        ))
                        .child(numbered(
                            4,
                            if os == Os::Windows {
                                prose("again", format!("When it has finished, close the terminal and open a new one, so it can find {}.", a.name), cx).into_any_element()
                            } else {
                                v_flex()
                                    .gap(px(8.))
                                    .child(prose("again", format!("When it has finished, start the terminal afresh, so it can find {}:", a.name), cx))
                                    .child(self.command_box(format!("agent-again-{}-{}", a.id, os.as_str()), agents::SHELL_AGAIN.into(), cx))
                                    .child(quiet("path", "If the installer printed a line for you to run, such as one that adds a folder to your PATH, run that line first.".into(), cx))
                                    .into_any_element()
                            },
                        ))
                        .child(numbered(5, prose("done", format!("Press refresh above. This step gets a tick when Emaki finds {}.", a.name), cx).into_any_element()))
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
            v_flex().gap(px(12.)).children(here).child(pick).child(shown)
        };

        // Git, which the agents and Emaki's own panel work with: on a Mac
        // it comes with Apple's command line tools, which a new Mac does
        // not have. Its words follow the system as the install step's do.
        let git = self.agents_git.clone();
        let git_ready = git.as_ref().is_some_and(|g| g.ready);
        let git_step = {
            let git_body = |os: Os, cx: &mut Context<Self>| -> Div {
                let id = |part: &str| format!("agent-git-text-{}-{}-{part}", a.id, os.as_str());
                let prose = |part: &str, words: String, cx: &App| div().text_size(px(13.)).line_height(px(20.)).text_color(theme.foreground.opacity(0.85)).child(self.selectable_as(id(part), &words, cx));
                let quiet = |part: &str, words: String, cx: &App| div().text_size(px(12.)).line_height(px(18.)).text_color(theme.muted_foreground).child(self.selectable_as(id(part), &words, cx));
                let col = v_flex().w_full().gap(px(10.));
                let col = if git_ready && os == Os::here() { col.child(prose("here", if os == Os::Mac { "Apple's command line tools, with git, are on this machine.".into() } else { "Git is on this machine.".into() }, cx)) } else { col };
                match os {
                    Os::Mac => col
                        .child(prose("what", "Git keeps track of the changes to your files, and the agents use it. On a Mac it comes with Apple's command line tools.".into(), cx))
                        .child(self.command_box(format!("agent-git-{}-mac", a.id), agents::GIT_MAC.into(), cx))
                        .child(quiet("how", "Run it in the terminal as the next step shows. A window opens: click Install and wait until it says the software was installed, which can take several minutes. If it says the tools are already installed, there is nothing to do.".into(), cx)),
                    Os::Linux => col
                        .child(prose("what", "Git keeps track of the changes to your files, and the agents use it. Most Linux systems have it. If yours does not, install it with your system's package manager; on Ubuntu or Debian:".into(), cx))
                        .child(self.command_box(format!("agent-git-{}-linux", a.id), agents::GIT_LINUX.into(), cx)),
                    Os::Windows => col
                        .child(prose("what", "Git keeps track of the changes to your files, and the agents use it. Download Git for Windows and open the installer; the choices it offers can be left as they are.".into(), cx))
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
        // What to do stays under the word that it is done, for signing
        // in again or as someone else.
        if signed {
            sign = sign.child(prose("Signed in on this machine.".into(), cx));
        }
        if a.sign_in.is_empty() {
            // Signed in to in its own window, with nothing to type.
            sign = sign.child(prose(a.sign_in_how.to_string(), cx));
        } else {
            sign = sign
                .child(self.command_box("agent-sign-in".into(), a.sign_in.into(), cx))
                .child(numbered_at(1, os == Os::here(), if os == Os::here() { h_flex().gap(px(10.)).items_center().child(prose("Run it in the terminal, as you did to install:".into(), cx)).child(open_term("agent-term-sign-in", cx)).into_any_element() } else { prose("Run it in the terminal, as you did to install.".into(), cx).into_any_element() }))
                .child(numbered(2, prose(a.sign_in_how.to_string(), cx).into_any_element()));
        }
        sign = sign.child(quiet(a.plans.to_string(), cx));
        if !signed && !a.key_env.is_empty() {
            sign = sign.child(quiet(format!("Or set {} in your shell's profile.", a.key_env.join(" or ")), cx));
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
            let mut body = v_flex().gap(px(10.));
            if !known_name.is_empty() && !known_email.is_empty() {
                body = body.child(prose(format!("Your commits are signed as {known_name} <{known_email}>. To change that, type a new name or address and run the line again."), cx));
            } else {
                body = body.child(prose("Git puts a name and an email address on every change you save, so others can see who made it. Use the address of your GitHub account.".into(), cx));
            }
            body = body.child(h_flex().gap(px(10.)).child(field("git-name-field", "Name", &self.git_name_input, cx)).child(field("git-email-field", "Email", &self.git_email_input, cx)));
            if !name.is_empty() && !email.is_empty() {
                let line = agents::git_identity_command(&name, &email, os);
                body = body.child(self.command_box(format!("agent-identity-{}", os.as_str()), line.into(), cx)).child(numbered_at(1, os == Os::here(), if os == Os::here() { h_flex().gap(px(10.)).items_center().child(prose("Copy it and run it in the terminal:".into(), cx)).child(open_term("agent-term-identity", cx)).into_any_element() } else { prose("Copy it and run it in the terminal.".into(), cx).into_any_element() })).child(numbered(2, prose("Press refresh above. This step gets a tick when git knows your name and address.".into(), cx).into_any_element()));
            } else {
                body = body.child(quiet("Type both, and the line to run shows here.".into(), cx));
            }
            (!known_name.is_empty() && !known_email.is_empty(), body)
        });

        let sessions = self.agent_sessions(a);
        let emaki = match a.reads {
            None if !a.with_emaki.is_empty() => prose(a.with_emaki.to_string(), cx),
            Some(_) if sessions.is_empty() => prose(format!("{}'s sessions appear under All Sessions. None yet.", a.name), cx),
            Some(_) => prose(format!("Its sessions are under All Sessions, including ones {} has deleted.", a.name), cx),
            None => prose(format!("Emaki does not read {}'s sessions yet.", a.name), cx),
        };

        let steps = v_flex()
            .w_full()
            .p(px(20.))
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.muted.opacity(0.35) } else { theme.popover })
            .child(self.agent_step(1, git_ready, false, "Get git".into(), git_ready.then(|| "Done".to_string()), git_step, ink, cx))
            .child(self.agent_step(2, installed, false, format!("Install {}", a.name), installed.then(|| "Done".to_string()), install, ink, cx))
            .child(self.agent_step(3, signed, false, "Sign in".into(), signed.then(|| "Done".to_string()), sign, ink, cx))
            .map(|d| match identity {
                Some((known, body)) => d.child(self.agent_step(4, known, false, "Tell git who you are".into(), known.then(|| "Done".to_string()), body, ink, cx)).child(self.agent_step(5, installed, true, "Use it with Emaki".into(), None, v_flex().child(emaki), ink, cx)),
                None => d.child(self.agent_step(4, (a.reads.is_some() || !a.with_emaki.is_empty()) && installed, true, "Use it with Emaki".into(), None, v_flex().child(emaki), ink, cx)),
            });

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
