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
use crate::workbench::{page_in, pill_button, Page, Workbench, CONTENT_W};

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
        let task = cx.background_spawn(async move { agents::detect_all() });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                this.agents_found = found.into_iter().collect();
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
        self.agent_swap = Some(AgentSwap { at: Instant::now(), h: self.agent_way_h.get(), os: self.agent_os, way: self.agent_way });
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
            .managed_tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(if busy { "Refreshing…" } else { "Refresh status" }).build(window, cx))
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
        let warn = (stale > 0).then(|| div().text_size(px(12.5)).text_color(theme.danger).child(format!("{} no longer {} and may have changed; the agent's page says which.", plural(stale, "install command", "install commands"), if stale == 1 { "works" } else { "work" })));
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
                        .child(div().text_size(px(20.)).font_family(display).child("Beside the agents"))
                        .child(div().text_size(px(13.)).line_height(px(20.)).text_color(theme.muted_foreground).child("Worth having with them: what the agents write ends up in git.")),
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
    pub(crate) fn command_box(&self, key: String, command: &'static str, cx: &mut Context<Self>) -> AnyElement {
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
            .managed_tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(if done { "Copied" } else { "Copy" }).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.copy_text(key.clone(), command.to_string(), cx)))
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
            .child(div().flex_1().min_w_0().font_family(theme.mono_font_family.clone()).text_size(px(12.5)).line_height(px(19.)).child(self.selectable_as(text_id, command, cx)))
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
        let links = h_flex().gap(px(8.)).items_center().child(link("agent-site", "Website", a.site, &tinted)).child(link("agent-docs", "Documentation", a.docs, &tinted)).child(div().flex_1()).child(self.check_button(cx));

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
            let os = self.agent_os;
            let on: Rc<dyn Fn(&mut Self, &'static str, &mut Window, &mut Context<Self>)> = Rc::new(|this, key, _, cx| this.agent_choose(Os::parse(key), None, cx));
            let options = Os::ALL.iter().map(|o| (o.as_str(), o.label().to_string(), None)).collect();
            // Two choices on one row, read as a path: the system, then
            // the way on it. An app is downloaded first, where its
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
            let mut pick = h_flex().gap(px(6.)).items_center().child(self.segmented_sized("agent-os", options, os.as_str(), on, true, cx));
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
                pick = pick.child(Icon::new(IconName::ChevronRight).with_size(px(12.)).text_color(theme.muted_foreground.opacity(0.7))).child(self.segmented_sized(control, options, how, on, true, cx));
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
                        col.child(
                            v_flex()
                                .gap(px(8.))
                                .child(prose("download", format!("Download the installer from {} and open it. When it is installed, press the refresh button above.", a.maker), cx))
                                .child(h_flex().child(pill_button("agent-download", "Download", &tinted, move |_, _, cx| cx.open_url(url)))),
                        )
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
                                .child(self.command_box(format!("agent-way-{}-{}-{i}", a.id, os.as_str()), w.command, cx))
                                .when(gone, |d| d.child(div().text_size(px(12.)).line_height(px(18.)).text_color(theme.danger).child(format!("What this command installs is no longer published, so it has likely changed. See {}'s documentation for the current one.", a.name)))),
                        )
                        .child(quiet("shell", "Open the shell with the button at the top right, paste the command there and press Return. When it has finished, press the refresh button above.".into(), cx))
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
            v_flex().gap(px(10.)).children(here).child(pick).child(shown)
        };

        let signed = found.as_ref().is_some_and(|f| f.signed);
        let mut sign = v_flex().gap(px(10.));
        // What to do stays under the word that it is done, for signing
        // in again or as someone else.
        if signed {
            sign = sign.child(prose("A sign-in is on this machine.".into(), cx));
        }
        if a.sign_in.is_empty() {
            // Signed in to in its own window, with nothing to type.
            sign = sign.child(prose(a.sign_in_how.to_string(), cx));
        } else {
            sign = sign.child(self.command_box("agent-sign-in".into(), a.sign_in, cx)).child(prose(format!("Type it in the shell at the top right. {}", a.sign_in_how), cx));
        }
        sign = sign.child(quiet(a.plans.to_string(), cx));
        if !signed && !a.key_env.is_empty() {
            sign = sign.child(quiet(format!("With a key and no browser: set {} in your shell's profile.", a.key_env.join(" or ")), cx));
        }

        let sessions = self.agent_sessions(a);
        let emaki = match a.reads {
            None if !a.with_emaki.is_empty() => prose(a.with_emaki.to_string(), cx),
            Some(_) if sessions.is_empty() => prose(format!("Emaki keeps every session of {} and lists it under All Projects. There are none yet.", a.name), cx),
            Some(_) => prose(format!("Emaki keeps every session of {} and lists it under All Projects, the ones {} has since deleted too.", a.name, a.name), cx),
            None => prose(format!("Emaki does not read {}'s sessions yet. It says here whether {} is installed; its conversations stay where {} keeps them.", a.name, a.name, a.name), cx),
        };

        let steps = v_flex()
            .w_full()
            .p(px(20.))
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.muted.opacity(0.35) } else { theme.popover })
            .child(self.agent_step(1, installed, false, format!("Install {}", a.name), installed.then(|| "Done".to_string()), install, ink, cx))
            .child(self.agent_step(2, signed, false, "Sign in".into(), signed.then(|| "Done".to_string()), sign, ink, cx))
            .child(self.agent_step(3, (a.reads.is_some() || !a.with_emaki.is_empty()) && installed, true, "Use it with Emaki".into(), None, v_flex().child(emaki), ink, cx));

        let page = v_flex().w_full().max_w(CONTENT_W).pt(px(20.)).pb(px(40.)).gap(px(16.)).child(crumb).child(head).child(prose(a.about.to_string(), cx)).child(links).child(steps);

        page
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
