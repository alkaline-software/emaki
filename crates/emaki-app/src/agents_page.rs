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

use crate::format::{plural, relative};
use crate::workbench::{page_in, pill_button, Page, Scope, Workbench, CONTENT_W};

/// How old what was found may be before a visit to the page looks again.
const AGENTS_FRESH: Duration = Duration::from_secs(20);

/// How long the network's word on the install commands stands.
const COMMANDS_FRESH: Duration = Duration::from_secs(24 * 60 * 60);

/// How many of an agent's projects its page lists.
const AGENT_PROJECTS: usize = 5;

/// The agents with a mark of their own in `assets/icons/agents`; the
/// rest wear the first letter of their name.
const MARKS: &[&str] = &["codex", "gemini"];

/// An agent's mark at `size` in `color`.
pub fn agent_mark(a: &Agent, size: Pixels, color: Hsla) -> AnyElement {
    if a.id == "claude-code" {
        return crate::workbench::claude_icon(size, color).into_any_element();
    }
    if MARKS.contains(&a.id) {
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
        let mut plain = String::with_capacity(words.len() + 8);
        for c in words.chars() {
            if matches!(c, '\\' | '`' | '*' | '_' | '<' | '>' | '[' | ']' | '#' | '|' | '~' | '&' | '!' | '-' | '+' | '.' | '(' | ')' | '{' | '}' | '=') {
                plain.push('\\');
            }
            plain.push(c);
        }
        crate::transcript::md_view(format!("agents-text-{n}"), plain, cx).into_any_element()
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
            Some(f) if f.installed() => (theme.green, "Installed", true),
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
        let ink = match (installed, a.id) {
            (true, "claude-code") => theme.primary,
            (true, _) => theme.foreground,
            _ => theme.muted_foreground,
        };
        div()
            .size(side)
            .flex_shrink_0()
            .rounded(side * 0.26)
            .bg(if theme.mode.is_dark() { theme.muted } else { theme.foreground.opacity(0.05) })
            .flex()
            .items_center()
            .justify_center()
            .child(agent_mark(a, side * 0.5, ink))
    }

    /// The page's top level: a card an agent, the installed first.
    fn agents_cards(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let display = crate::fonts::display_family(cx);
        let all = agents::all();
        // Said only when there is something to say: a command whose
        // source is gone.
        let stale = self.agents_stale.len();
        let warn = (stale > 0).then(|| div().text_size(px(12.5)).text_color(theme.danger).child(format!("{} no longer {} and may have changed; the agent's page says which.", plural(stale, "install command", "install commands"), if stale == 1 { "works" } else { "work" })));
        // After the agents there are, a card that is no agent's: the
        // catalogue is short on purpose, and says more are on the way.
        let coming = || {
            v_flex()
                .flex_1()
                .min_w_0()
                .p(px(16.))
                .gap(px(12.))
                .rounded(px(14.))
                .border_1()
                .border_dashed()
                .border_color(theme.border)
                .child(
                    h_flex()
                        .gap(px(12.))
                        .items_center()
                        .child(div().size(px(40.)).flex_shrink_0().rounded(px(10.4)).border_1().border_dashed().border_color(theme.border).flex().items_center().justify_center().child(Icon::new(IconName::Plus).with_size(px(18.)).text_color(theme.muted_foreground)))
                        .child(v_flex().flex_1().min_w_0().gap(px(1.)).child(div().truncate().text_size(px(14.5)).font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground).child("More are coming")).child(div().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child("In a later release"))),
                )
                .child(div().flex_1().min_h(px(38.)).text_size(px(12.5)).line_height(px(19.)).text_color(theme.muted_foreground).child("Emaki starts with the three most used. Other coding agents will be added here as it learns to read their sessions."))
        };
        let grid = |list: Vec<&'static Agent>, last: bool, this: &Self, cx: &mut Context<Self>| {
            let mut cells: Vec<AnyElement> = list.into_iter().map(|a| this.agent_card(a, cx)).collect();
            if last {
                cells.push(coming().into_any_element());
            }
            let mut rows = v_flex().w_full().gap(px(12.));
            let mut cells = cells.into_iter();
            while let Some(first) = cells.next() {
                let second = cells.next().unwrap_or_else(|| div().flex_1().min_w_0().into_any_element());
                rows = rows.child(h_flex().w_full().gap(px(12.)).items_stretch().child(first).child(second));
            }
            rows
        };
        // One grid in the catalogue's order, whatever is installed: a
        // card says so itself, and stays where it is when that changes.
        v_flex()
            .w_full()
            .max_w(CONTENT_W)
            .pt(px(20.))
            .pb(px(40.))
            .gap(px(16.))
            .child(h_flex().items_center().gap(px(12.)).child(div().flex_1().min_w_0().text_size(px(30.)).font_family(display).child("Agents")).child(self.check_button(cx)))
            .children(warn)
            .child(grid(all.iter().collect(), true, self, cx))
    }

    fn agent_card(&self, a: &'static Agent, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let found = self.agent_found(a.id);
        let installed = found.is_some_and(|f| f.installed());
        let sessions = self.agent_sessions(a).len();
        // The card's foot: what is known of an installed one, or the way
        // in for one that is not.
        let mut facts: Vec<String> = Vec::new();
        if let Some(f) = found.filter(|f| f.installed()) {
            if !f.version.is_empty() {
                facts.push(format!("v{}", f.version));
            }
            if f.signed {
                facts.push("signed in".into());
            }
        }
        if sessions > 0 {
            facts.push(plural(sessions, "session", "sessions"));
        }
        let foot = if installed || sessions > 0 {
            div().flex_1().min_w_0().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(facts.join(" · "))
        } else {
            div().flex_1().min_w_0().truncate().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(theme.primary).child(if found.is_some() { "Set up" } else { "" })
        };
        let id = a.id;
        v_flex()
            .id(SharedString::from(format!("agent-card-{id}")))
            .flex_1()
            .min_w_0()
            .p(px(16.))
            .gap(px(12.))
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.muted.opacity(0.35) } else { theme.popover })
            .cursor_pointer()
            .hover(|s| s.border_color(theme.primary.opacity(0.55)).bg(theme.primary.opacity(0.05)))
            .on_click(cx.listener(move |this, _, _, cx| this.show_agents(Some(id), cx)))
            .child(
                h_flex()
                    .gap(px(12.))
                    .items_center()
                    .child(self.agent_tile(a, px(40.), cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(1.))
                            .child(div().truncate().text_size(px(14.5)).font_weight(FontWeight::MEDIUM).child(a.name))
                            .child(div().truncate().text_size(px(12.)).text_color(theme.muted_foreground).child(a.maker)),
                    )
                    .child(self.agent_chip(a, cx)),
            )
            .child(div().flex_1().min_h(px(38.)).text_size(px(12.5)).line_height(px(19.)).text_color(theme.foreground.opacity(0.82)).child(a.about))
            .child(h_flex().gap(px(8.)).items_center().child(foot).child(Icon::new(IconName::ChevronRight).with_size(px(14.)).text_color(theme.muted_foreground.opacity(0.7))))
            .into_any_element()
    }

    /// A command on a plate, in the code face, with a button that copies
    /// it.
    pub(crate) fn command_box(&self, key: String, command: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let key = SharedString::from(key);
        let done = self.copied.as_ref() == Some(&key);
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
            .child(Icon::new(if done { IconName::Check } else { IconName::Copy }).with_size(px(14.)).text_color(if done { theme.green } else { theme.muted_foreground }));
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
            .child(div().flex_1().min_w_0().font_family(theme.mono_font_family.clone()).text_size(px(12.5)).line_height(px(19.)).child(self.selectable(command, cx)))
            .child(copy)
            .into_any_element()
    }

    /// One step of setting an agent up: its number on a disc, a tick
    /// once it is done, and a line down to the next.
    fn agent_step(&self, n: usize, done: bool, last: bool, title: String, note: Option<String>, body: Div, cx: &App) -> Div {
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
            .map(|d| if done { d.bg(theme.green.opacity(0.16)).child(Icon::new(IconName::Check).with_size(px(13.)).text_color(theme.green)) } else { d.border_1().border_color(theme.border).text_color(theme.muted_foreground).child(n.to_string()) });
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
        let links = h_flex().gap(px(8.)).items_center().child(link("agent-site", "Website", a.site, &theme)).child(link("agent-docs", "Documentation", a.docs, &theme)).child(div().flex_1()).child(self.check_button(cx));

        // Installing: where it is once it is there; the ways to get it
        // until then, for the system chosen.
        let install = match found.as_ref().and_then(|f| f.path.as_ref().map(|p| (f, p))) {
            Some((f, path)) => {
                let mut lines = v_flex().gap(px(4.)).child(prose(if f.version.is_empty() { format!("{} is on this machine.", a.name) } else { format!("{} {} is on this machine.", a.name, f.version) }, cx));
                lines = lines.child(div().font_family(theme.mono_font_family.clone()).text_size(px(12.)).text_color(theme.muted_foreground).child(self.selectable(&tilde(&path.to_string_lossy()), cx)));
                lines
            }
            None => {
                let os = self.agent_os;
                let on: Rc<dyn Fn(&mut Self, &'static str, &mut Window, &mut Context<Self>)> = Rc::new(|this, key, _, cx| {
                    if let Some(os) = Os::parse(key) {
                        this.agent_os = os;
                        cx.notify();
                    }
                });
                let options = Os::ALL.iter().map(|o| (o.as_str(), o.label().to_string(), None)).collect();
                let mut col = v_flex().gap(px(10.)).child(h_flex().child(self.segmented_sized("agent-os", options, os.as_str(), on, true, cx)));
                let ways = a.ways(os);
                let any = !ways.is_empty();
                if !any {
                    col = col.child(quiet(format!("No installer for {} is listed here. See {}'s documentation.", os.label(), a.name), cx));
                }
                for (i, w) in ways.into_iter().enumerate() {
                    let label = if i == 0 { format!("{} (recommended)", w.by) } else { w.by.to_string() };
                    let gone = self.agents_stale.contains(w.command);
                    col = col.child(
                        v_flex()
                            .gap(px(5.))
                            .child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground).child(label))
                            .child(self.command_box(format!("agent-way-{}-{i}", os.as_str()), w.command, cx))
                            .when(gone, |d| d.child(div().text_size(px(12.)).line_height(px(18.)).text_color(theme.danger).child(format!("What this command installs is no longer published, so it has likely changed. See {}'s documentation for the current one.", a.name)))),
                    );
                }
                col.when(any, |c| c.child(quiet("Open the shell with the button at the top right, paste the command there and press Return. When it has finished, press the refresh button above.".into(), cx)))
            }
        };

        let signed = found.as_ref().is_some_and(|f| f.signed);
        let mut sign = v_flex().gap(px(10.));
        if signed {
            sign = sign.child(prose("A sign-in is on this machine.".into(), cx));
        } else {
            sign = sign.child(self.command_box("agent-sign-in".into(), a.sign_in, cx)).child(prose(format!("Type it in the shell at the top right. {}", a.sign_in_how), cx));
        }
        sign = sign.child(quiet(a.plans.to_string(), cx));
        if !signed && !a.key_env.is_empty() {
            sign = sign.child(quiet(format!("With a key and no browser: set {} in your shell's profile.", a.key_env.join(" or ")), cx));
        }

        let sessions = self.agent_sessions(a);
        let emaki = match a.reads {
            Some(_) if sessions.is_empty() => prose(format!("Emaki keeps every session of {} and shows it here. There are none yet.", a.name), cx),
            Some(_) => prose(format!("Emaki keeps every session of {} and shows it here, the ones {} has since deleted too.", a.name, a.name), cx),
            None => prose(format!("Emaki does not read {}'s sessions yet. It says here whether {} is installed; its conversations stay where {} keeps them.", a.name, a.name, a.name), cx),
        };

        let steps = v_flex()
            .w_full()
            .p(px(20.))
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .bg(if theme.mode.is_dark() { theme.muted.opacity(0.35) } else { theme.popover })
            .child(self.agent_step(1, installed, false, format!("Install {}", a.name), installed.then(|| "Done".to_string()), install, cx))
            .child(self.agent_step(2, signed, false, "Sign in".into(), signed.then(|| "Done".to_string()), sign, cx))
            .child(self.agent_step(3, a.reads.is_some() && installed, true, "Use it with Emaki".into(), None, v_flex().child(emaki), cx));

        let mut page = v_flex().w_full().max_w(CONTENT_W).pt(px(20.)).pb(px(40.)).gap(px(16.)).child(crumb).child(head).child(prose(a.about.to_string(), cx)).child(links).child(steps);

        // The sessions kept of it, by project, as the sessions page has
        // them: a click goes there, inside that project.
        if let Some(agent) = a.reads.filter(|_| !sessions.is_empty()) {
            let mut folders: Vec<(String, Vec<&SessionRef>)> = Vec::new();
            for r in &sessions {
                let p = r.project();
                match folders.iter_mut().find(|(name, _)| *name == p) {
                    Some((_, list)) => list.push(r),
                    None => folders.push((p, vec![r])),
                }
            }
            let total = folders.len();
            let mut rows = v_flex().w_full().gap(px(2.));
            for (p, list) in folders.into_iter().take(AGENT_PROJECTS) {
                let newest = list.iter().map(|r| r.mtime).fold(0., f64::max);
                let live = list.iter().find_map(|r| self.live_color(r, cx));
                let name = p.clone();
                rows = rows.child(
                    h_flex()
                        .id(SharedString::from(format!("agent-folder-{p}")))
                        .w_full()
                        .px(px(12.))
                        .py(px(8.))
                        .gap(px(12.))
                        .items_center()
                        .rounded(px(10.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .on_click(cx.listener(move |this, _, _, cx| this.show_sessions_in(Scope::Agent(agent), Some(name.clone()), cx)))
                        .child(div().w(px(24.)).flex().justify_center().child(Icon::new(IconName::Folder).with_size(px(16.)).text_color(theme.muted_foreground)))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(13.5)).child(p))
                        .when_some(live, |d, c| d.child(div().size(px(7.)).rounded_full().bg(c).flex_shrink_0()))
                        .child(div().flex_shrink_0().text_size(px(12.)).text_color(theme.muted_foreground).child(format!("{} · {}", plural(list.len(), "session", "sessions"), relative(newest, self.now))))
                        .child(Icon::new(IconName::ChevronRight).with_size(px(14.)).text_color(theme.muted_foreground.opacity(0.7))),
                );
            }
            let all = h_flex().child(pill_button("agent-all-sessions", format!("All {} in {}", plural(sessions.len(), "session", "sessions"), plural(total, "project", "projects")), &theme, {
                let entity = cx.entity().downgrade();
                move |_, _, cx| {
                    let _ = entity.update(cx, |this, cx| this.show_sessions_in(Scope::Agent(agent), None, cx));
                }
            }));
            page = page
                .child(div().pt(px(8.)).px(px(2.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Sessions"))
                .child(rows)
                .child(all.px(px(12.)));
        }
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
