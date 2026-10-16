//! The settings panel (⌘,), laid out as Kotoba's settings window is
//! (`docs/design.md`): a rail of sections at the left, and at the right
//! the chosen section's name over its groups, each a card of rows with a
//! name at the left and a control at the right. Every change is drawn at
//! once and written to `config.json`.

use super::*;

/// The panel's size: wide enough for a rail beside the rows, and a fixed
/// height so it reads as a sheet, not a second window (capped by the
/// window when that is smaller). Every section fits in it unscrolled.
pub const SETTINGS_W: Pixels = px(820.);
pub const SETTINGS_H: Pixels = px(600.);
/// The rail's width, and the page's margin at either side.
pub const SETTINGS_RAIL_W: Pixels = px(190.);
const PAGE_X: Pixels = px(36.);
/// The band at the top of the page that holds the section's name, and
/// of the rail, where "Settings" stands level with it.
const TOP_H: Pixels = px(52.);
/// What the widest controls at a row's right take, so those on a page
/// start and end together.
const CONTROL_W: Pixels = px(336.);
/// Every field, button and choice in a row is this tall.
const CONTROL_H: Pixels = px(28.);
/// The update's button, set once so its words can change in place.
const UPDATE_W: Pixels = px(190.);
/// A card's corner.
const CARD_R: Pixels = px(12.);

impl Workbench {
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The page's scroller is named before anything on it.
        self.inner_scroller(&self.settings_scroll, Inner::Over);
        let theme = cx.theme().clone();
        let section = self.settings_section;
        let rows = match section {
            "fonts" => self.fonts_rows(cx),
            "composer" => self.composer_rows(cx),
            "sessions" => self.sessions_rows(cx),
            "terminal" => self.terminal_rows(cx),
            "files" => self.files_rows(cx),
            "updates" => self.updates_rows(cx),
            "about" => self.about_rows(cx),
            _ => self.appearance_rows(cx),
        };
        let title = SETTINGS_SECTIONS.iter().find(|(k, _, _)| *k == section).map(|(_, name, _)| *name).unwrap_or("Appearance");
        let (title_anim, page_anim): (&'static str, &'static str) = match section {
            "fonts" => ("title-fonts", "page-fonts"),
            "composer" => ("title-composer", "page-composer"),
            "sessions" => ("title-sessions", "page-sessions"),
            "terminal" => ("title-terminal", "page-terminal"),
            "files" => ("title-files", "page-files"),
            "updates" => ("title-updates", "page-updates"),
            "about" => ("title-about", "page-about"),
            _ => ("title-appearance", "page-appearance"),
        };

        // The scrim is a flex box, so the panel sits in the middle of the
        // window both ways (an absolute panel with auto margins did not).
        // The panel occludes, as every sheet on a scrim that closes at a
        // click does: the scrim's bounds take the panel in, so a press in
        // the panel let go outside it, or the other way, was a click on
        // the scrim and closed it.
        div()
            .id("settings-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_open = false;
                cx.notify();
            }))
            .child(
                h_flex()
                    .id("settings-panel")
                    .occlude()
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(SETTINGS_W)
                    .h(SETTINGS_H)
                    .max_w(gpui::relative(0.94))
                    .max_h(gpui::relative(0.9))
                    .items_stretch()
                    .rounded(px(18.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .overflow_hidden()
                    .child(self.settings_rail(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            // The section's name in the top band, fading
                            // in with its page, and the way out at its end.
                            .child(
                                h_flex()
                                    .h(TOP_H)
                                    .flex_shrink_0()
                                    .pl(PAGE_X)
                                    .pr(px(14.))
                                    .items_center()
                                    .child(page_in(title_anim, div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title)))
                                    .child(div().flex_1())
                                    .child(Button::new("settings-close").ghost().small().icon(Icon::new(IconName::Close)).tooltip("Close (esc)").on_click(cx.listener(|this, _, _, cx| {
                                        this.settings_open = false;
                                        cx.notify();
                                    }))),
                            )
                            // The rows scroll under the band with the
                            // toolkit's scrollbar at their edge (it fades a
                            // second after the last scroll, as every
                            // scrollbar in the window does). Every section
                            // fits unscrolled; the scroll is for a window
                            // smaller than the panel.
                            .child(
                                v_flex()
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .child(
                                        v_flex()
                                            .id("settings-body")
                                            .flex_1()
                                            .min_h_0()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.settings_scroll)
                                            .px(PAGE_X)
                                            .pt(px(2.))
                                            .pb(px(24.))
                                            .child(page_in(page_anim, v_flex().gap(px(18.)).children(rows))),
                                    )
                                    .vertical_scrollbar(&self.settings_scroll),
                            ),
                    ),
            )
    }

    /// The rail of sections at the panel's left: "Settings" in the top
    /// band, level with the section's name, then an icon and a name per
    /// section, the chosen one on a plate. Updates wears a dot while a
    /// newer Emaki waits.
    fn settings_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let current = self.settings_section;
        let update_waiting = self.update.available.is_some();
        v_flex()
            .w(SETTINGS_RAIL_W)
            .flex_shrink_0()
            .px(px(10.))
            .pb(px(10.))
            .gap(px(2.))
            .bg(if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.5) })
            .border_r_1()
            .border_color(theme.border)
            // A clip is a rectangle: the rail's ground is rounded where
            // it meets the panel's corners, the panel's less its border.
            .rounded_tl(px(17.))
            .rounded_bl(px(17.))
            .child(h_flex().h(TOP_H).flex_shrink_0().px(px(10.)).items_center().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child("Settings"))
            .children(SETTINGS_SECTIONS.iter().map(|(key, label, icon)| {
                let active = *key == current;
                let key: &'static str = key;
                let dot = key == "updates" && update_waiting;
                h_flex()
                    .id(SharedString::from(format!("settings-nav-{key}")))
                    .h(px(30.))
                    .px(px(10.))
                    .gap(px(9.))
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.sidebar_accent))
                    .when(!active, |d| d.hover(|s| s.bg(theme.sidebar_accent.opacity(0.6))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.settings_section != key {
                            this.settings_section = key;
                            this.settings_scroll.set_offset(point(px(0.), px(0.)));
                            this.close_pick(cx);
                        }
                        cx.notify();
                    }))
                    .child(Icon::default().path(*icon).with_size(px(15.)).text_color(if active { theme.foreground } else { theme.muted_foreground }))
                    .child(div().flex_1().text_size(px(13.)).when(active, |d| d.font_weight(FontWeight::MEDIUM)).text_color(if active { theme.foreground } else { theme.sidebar_foreground }).child(*label))
                    .when(dot, |d| d.child(div().size(px(7.)).rounded_full().bg(theme.primary)))
            }))
            .into_any_element()
    }

    /// A switch, for what is on or off: a track that fills with the
    /// accent and a knob that slides to its end, both over a moment as it
    /// is turned (`presence`). A press anywhere on it turns it.
    #[allow(clippy::type_complexity)]
    fn switch(&self, id: &'static str, on: bool, turn: Rc<dyn Fn(&mut Self, bool, &mut Window, &mut Context<Self>)>, cx: &mut Context<Self>) -> AnyElement {
        const W: f32 = 36.;
        const H: f32 = 20.;
        const KNOB: f32 = 16.;
        const INSET: f32 = 2.;
        let travel = W - KNOB - INSET * 2.;
        let theme = cx.theme().clone();
        let moving = self.presence(&format!("switch-{id}"), on, cx);
        let at = move |t: f32| px(INSET + travel * t);
        let fill = div().absolute().inset_0().rounded_full().bg(theme.primary);
        let knob = div().absolute().top(px(INSET)).size(px(KNOB)).rounded_full().bg(gpui::white()).shadow_sm();
        let (fill, knob) = match moving {
            Some((coming, n)) => (
                fill.with_animation(ElementId::Name(format!("switch-{id}-fill-{n}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
                knob.with_animation(ElementId::Name(format!("switch-{id}-knob-{n}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.left(at(if coming { t } else { 1. - t }))).into_any_element(),
            ),
            None => (fill.opacity(if on { 1. } else { 0. }).into_any_element(), knob.left(at(if on { 1. } else { 0. })).into_any_element()),
        };
        div()
            .id(SharedString::from(format!("switch-{id}")))
            .relative()
            .flex_shrink_0()
            .w(px(W))
            .h(px(H))
            .rounded_full()
            .bg(theme.switch)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                turn(this, !on, window, cx)
            }))
            .child(fill)
            .child(knob)
            .into_any_element()
    }

    /// A field in the settings, the controls' width: the name's text
    /// field (`name_input`), in the focus wrapper every own input has.
    fn your_name_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        div()
            .id("name-field")
            .track_focus(&self.name_input.read(cx).focus_handle(cx))
            .role(Role::TextInput)
            .aria_label("Your name")
            .aria_value(self.name_input.read(cx).value().to_string())
            .w(CONTROL_W)
            .h(CONTROL_H)
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .bg(theme.muted)
            .text_size(px(12.5))
            .child(Input::new(&self.name_input).appearance(false).bordered(false).on_secondary_click(self.input_menu(&self.name_input, false, cx)))
            .into_any_element()
    }

    /// A choice from a list, the controls' width (`settings_pick_h`).
    fn pick_field(&self, what: PickFor, id: &'static str, icon: &'static str, label: PillText, cx: &Context<Self>) -> AnyElement {
        div().w(CONTROL_W).child(self.settings_pick_h(what, id, icon, label, CONTROL_H, cx)).into_any_element()
    }

    /// Appearance: who you are, then how the window looks.
    fn appearance_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let app = self.cfg.app.clone();
        let dark = theme.mode.is_dark();
        let appearance_key = match app.appearance.as_str() {
            "light" => "light",
            "dark" => "dark",
            _ => "system",
        };
        let appearance = div().w(CONTROL_W).child(self.segmented("appearance", vec![("system", "System".into(), None), ("light", "Light".into(), None), ("dark", "Dark".into(), None)], appearance_key, Rc::new(|this, key, window, cx| this.set_appearance(key, window, cx)), cx)).into_any_element();

        // The picture at the start of the controls, what is done with it
        // after: a click on the picture chooses one too.
        let avatar = h_flex()
            .gap(px(8.))
            .items_center()
            .child(self.avatar(28., cx).id("settings-avatar").cursor_pointer().hover(|s| s.opacity(0.85)).on_click(cx.listener(|this, _, _, cx| this.pick_avatar(cx))))
            .child(crate::branches::small_pill("avatar-choose", "icons/image.svg", "Choose…", Press::Quiet, false, &theme).h(CONTROL_H).on_click(cx.listener(|this, _, _, cx| this.pick_avatar(cx))))
            .when(!app.avatar.is_empty(), |d| d.child(crate::branches::small_pill("avatar-remove", "icons/trash.svg", "Remove", Press::Danger, false, &theme).h(CONTROL_H).on_click(cx.listener(|this, _, _, cx| this.set_avatar(String::new(), cx)))))
            .into_any_element();

        // Accents are swatches: a disc of the colour, the chosen one with
        // a ring inside it in the card's ground. The ring is inside so
        // every disc is one size, and the first and the last end where the
        // controls do. The chosen one's name is the row's line.
        let ring = if dark { theme.sidebar } else { theme.popover };
        let accents = h_flex()
            .w(CONTROL_W)
            .justify_between()
            .children(crate::look::ACCENTS.iter().map(|a| {
                let active = crate::look::accent(&app.accent).key == a.key;
                let color: Hsla = gpui::Rgba::try_from(crate::look::accent_hex(a.key, dark)).map(Hsla::from).unwrap_or(theme.primary);
                let key = a.key;
                let name = a.name;
                div()
                    .id(SharedString::from(format!("accent-{key}")))
                    .group("accent")
                    .size(px(24.))
                    .flex_shrink_0()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(color)
                    .managed_tooltip(move |window, cx| crate::workbench::tip(name, window, cx))
                    .on_click(cx.listener(move |this, _, window, cx| this.set_accent(key, window, cx)))
                    .child(div().size(px(16.)).rounded_full().border_2().border_color(if active { ring } else { ring.opacity(0.) }).when(!active, |d| d.group_hover("accent", |s| s.border_color(ring.opacity(0.55)))))
            }))
            .into_any_element();
        let accent_name = crate::look::accent(&app.accent).name;

        vec![
            group(Some("You"), None, vec![line("Avatar", None, avatar, &theme), line("Name", None, self.your_name_field(cx), &theme)], &theme),
            group(Some("Window"), None, vec![line("Theme", None, appearance, &theme), line("Accent", Some(div().child(accent_name).into_any_element()), accents, &theme)], &theme),
        ]
    }

    /// Fonts: the conversation's face, the monospace face, and the size
    /// of both. Each face is a tile drawn in itself.
    fn fonts_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let app = self.cfg.app.clone();
        let chat_now = crate::fonts::chat_face(&app.chat_font).key;
        let mono_now = crate::fonts::mono_face(&self.cfg.terminal.font).key;
        let tile_ground = if theme.mode.is_dark() { theme.muted } else { theme.popover };
        let tiles = |faces: &'static [crate::fonts::Face], now: &'static str, sample: &'static str, mono: bool, cx: &mut Context<Self>| {
            h_flex().gap(px(8.)).children(faces.iter().map(|face| {
                let (key, active) = (face.key, face.key == now);
                v_flex()
                    .id(SharedString::from(format!("face-{key}")))
                    .flex_1()
                    .min_w_0()
                    .h(px(64.))
                    .px(px(12.))
                    .justify_center()
                    .gap(px(2.))
                    .rounded(px(10.))
                    .border_1()
                    .cursor_pointer()
                    .bg(tile_ground)
                    .border_color(if active { theme.primary } else { theme.border })
                    .when(active, |d| d.bg(theme.primary.opacity(0.08)))
                    .when(!active, |d| d.hover(|s| s.border_color(theme.muted_foreground)))
                    .on_click(cx.listener(move |this, _, window, cx| if mono { this.set_mono_font(key, window, cx) } else { this.set_chat_font(key, cx) }))
                    .child(div().text_size(px(if mono { 16. } else { 20. })).line_height(px(25.)).font_family(face.family).child(sample))
                    .child(div().truncate().text_size(px(11.5)).text_color(if active { theme.foreground } else { theme.muted_foreground }).child(face.name))
            }))
        };
        // A kind of face, small and quiet, over its tiles.
        let kind = |name: &'static str| div().text_size(px(11.5)).text_color(theme.muted_foreground).child(name);
        let chat = v_flex()
            .p(px(14.))
            .gap(px(8.))
            .child(kind("Serif"))
            .child(tiles(crate::fonts::SERIFS, chat_now, "Ag", false, cx))
            .child(div().h(px(4.)))
            .child(kind("Sans serif"))
            .child(tiles(crate::fonts::SANS, chat_now, "Ag", false, cx))
            .into_any_element();
        let mono = div().p(px(14.)).child(tiles(crate::fonts::monos(), mono_now, "fn(0) -> 1", true, cx)).into_any_element();
        let size_key = match app.chat_size.as_str() {
            "small" => "small",
            "large" => "large",
            _ => "medium",
        };
        let size = div().w(CONTROL_W).child(self.segmented("size", vec![("small", "Small".into(), None), ("medium", "Medium".into(), None), ("large", "Large".into(), None)], size_key, Rc::new(|this, key, _, cx| this.set_chat_size(key, cx)), cx)).into_any_element();
        vec![
            group(Some("Chat font"), None, vec![chat], &theme),
            group(Some("Monospace font"), None, vec![mono], &theme),
            group(None, None, vec![line("Font size", None, size, &theme)], &theme),
        ]
    }

    /// Composer: the key that sends, capitals, and spelling.
    fn composer_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let app = self.cfg.app.clone();
        // The key that sends, drawn as the keyboard has it (`key_text`:
        // the Mac's signs on a Mac, Ctrl in words elsewhere), with what
        // makes a new line under its name.
        let enter_sends = app.send_key == "enter";
        let send = div().w(CONTROL_W).child(self.segmented("send-key", vec![("cmd-enter", "⌘↩".into(), None), ("enter", "↩".into(), None)], if enter_sends { "enter" } else { "cmd-enter" }, Rc::new(|this, key, _, cx| this.set_send_key(key, cx)), cx)).into_any_element();
        let send_note = key_text_wrap(if enter_sends { "⇧↩ for a new line" } else { "↩ or ⇧↩ for a new line" }).into_any_element();
        let auto_cap = self.switch("auto-capitalize", app.auto_capitalize, Rc::new(|this, on, _, cx| this.set_writing(|a| a.auto_capitalize = on, cx)), cx);
        let spelling = app.check_writing;
        let check_writing = self.switch("check-writing", spelling, Rc::new(|this, on, _, cx| this.set_writing(|a| a.check_writing = on, cx)), cx);
        // The languages are the check's choices: dimmed and out of the
        // pointer's reach while it is off, as the formatters' are. A flag
        // and two letters each; the name is the hover's.
        let languages: Vec<&'static str> = emaki_core::check::LANGUAGES.iter().map(|l| l.0).collect();
        let pick = |list: &[&'static str], now: &str| list.iter().copied().find(|k| *k == now).unwrap_or(list[0]);
        let writing_language = self.segmented("writing-language", emaki_core::check::LANGUAGES.iter().map(|(key, _)| (*key, language_code(key).to_string(), None)).collect(), pick(&languages, &app.writing_language), Rc::new(|this, key, _, cx| this.set_writing(|a| a.writing_language = key.into(), cx)), cx);
        let writing_language = div().relative().w(CONTROL_W).when(!spelling, |d| d.opacity(0.45)).child(writing_language).when(!spelling, |d| d.child(div().absolute().inset_0().occlude())).into_any_element();
        // Said only where a choice does nothing: off a Mac.
        let language_note = (!crate::sys::SYSTEM_SPELLING).then(|| div().child("Spanish, French and German are checked on a Mac only.").into_any_element());
        vec![
            group(None, None, vec![line("Send with", Some(send_note), send, &theme), line("Auto-capitalization", None, auto_cap, &theme)], &theme),
            group(Some("Spelling and grammar"), None, vec![line("Check as you type", None, check_writing, &theme), line("Language", language_note, writing_language, &theme)], &theme),
        ]
    }

    /// New sessions: what each agent's begin with. Whose three are
    /// showing is chosen at the group's head, by the agents' marks.
    fn sessions_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let agent = self.settings_agent;
        // The same lists the pills under the composer open, since both
        // are whatever the agent offers: each agent's own three.
        let (mode, model, effort) = if agent == AgentId::Codex {
            let codex = self.new_options(AgentId::Codex);
            let model = if self.codex_next[1].is_empty() { codex.models.first().map(|m| m.key.clone()).unwrap_or_default() } else { self.codex_next[1].clone() };
            (
                self.pick_field(PickFor::CodexMode, "codex-mode", "icons/shield.svg", PillText { tail: "", ..mode_pill(&codex, &self.codex_next[0], &theme) }, cx),
                self.pick_field(PickFor::CodexModel, "codex-model", "icons/box.svg", PillText::plain(model_name(&codex, &model)), cx),
                self.pick_field(PickFor::CodexEffort, "codex-effort", "icons/gauge.svg", PillText { tail: "", ..effort_pill(&codex, &model, &self.codex_next[2], &theme) }, cx),
            )
        } else {
            let options = self.new_options(AgentId::ClaudeCode);
            let mode = if self.cfg.driver.default_mode.is_empty() { "default".to_string() } else { self.cfg.driver.default_mode.clone() };
            let model = if self.cfg.driver.default_model.is_empty() { options.default_model.clone() } else { self.cfg.driver.default_model.clone() };
            (
                self.pick_field(PickFor::DefaultMode, "default-mode", "icons/shield.svg", PillText { tail: "", ..mode_pill(&options, &mode, &theme) }, cx),
                self.pick_field(PickFor::DefaultModel, "default-model", "icons/box.svg", PillText::plain(model_name(&options, &model)), cx),
                self.pick_field(PickFor::DefaultEffort, "default-effort", "icons/gauge.svg", PillText { tail: "", ..effort_pill(&options, &model, &self.next_effort, &theme) }, cx),
            )
        };
        let head = h_flex().gap(px(7.)).items_center().child(agent_icon(agent, px(13.), agent_color(agent, &theme))).child(agent.display_name()).into_any_element();
        let pick = self.segmented_sized(
            "new-sessions-agent",
            AgentId::ALL.iter().map(|a| (a.as_str(), String::new(), None)).collect(),
            agent.as_str(),
            Rc::new(|this, key, _, cx| {
                if let Some(agent) = AgentId::parse(key) {
                    this.settings_agent = agent;
                    this.close_pick(cx);
                    cx.notify();
                }
            }),
            true,
            cx,
        );
        vec![group_with(Some(head), Some(pick), vec![line("Permission mode", None, mode, &theme), line("Model", None, model, &theme), line("Effort", None, effort, &theme)], &theme)]
    }

    /// Terminal: how it looks, then how it acts, and whose design it is.
    fn terminal_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let term = self.cfg.terminal.clone();
        let pick = |list: &[&'static str], now: &str| list.iter().copied().find(|k| *k == now).unwrap_or(list[0]);
        let term_theme = div().w(CONTROL_W).child(self.segmented("term-theme", vec![("match", "Match window".into(), None), ("dark", "Dark".into(), None), ("light", "Light".into(), None)], pick(&["match", "dark", "light"], &term.theme), Rc::new(|this, key, _, cx| this.set_terminal(|t| t.theme = key.into(), cx)), cx)).into_any_element();
        let term_cursor = div().w(CONTROL_W).child(self.segmented("term-cursor", vec![("bar", "Bar".into(), None), ("block", "Block".into(), None), ("underline", "Underline".into(), None)], pick(&["bar", "block", "underline"], &term.cursor), Rc::new(|this, key, _, cx| this.set_terminal(|t| t.cursor = key.into(), cx)), cx)).into_any_element();
        let term_blink = self.switch("term-blink", term.cursor_blink, Rc::new(|this, on, _, cx| this.set_terminal(|t| t.cursor_blink = on, cx)), cx);
        let term_ligatures = self.switch("term-ligatures", term.ligatures, Rc::new(|this, on, _, cx| this.set_terminal(|t| t.ligatures = on, cx)), cx);
        let term_copy = self.switch("term-copy", term.copy_on_select, Rc::new(|this, on, _, cx| this.set_terminal(|t| t.copy_on_select = on, cx)), cx);
        // The face and the size are the window's, set in Fonts.
        let fonts = crate::branches::small_pill("term-fonts", "icons/typography.svg", "Open Fonts", Press::Quiet, false, &theme)
            .h(CONTROL_H)
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_section = "fonts";
                this.settings_scroll.set_offset(point(px(0.), px(0.)));
                cx.notify();
            }))
            .into_any_element();
        // The marks it joins, with a zero-width mark between the two
        // letters of each, or the window's own face would join them here.
        let joins = div().child("Joins -\u{200C}> and !\u{200C}= into one mark.").into_any_element();
        let (link_color, link_hover) = (theme.muted_foreground, theme.primary);
        let credit_link = |id: &'static str, name: &'static str, url: &'static str| {
            div()
                .id(id)
                .cursor_pointer()
                .text_decoration_1()
                .text_decoration_color(link_color.opacity(0.6))
                .hover(move |s| s.text_color(link_hover).text_decoration_color(link_hover))
                .on_click(move |_, _, cx| cx.open_url(url))
                .child(name)
        };
        let credit = h_flex()
            .px(px(14.))
            .text_size(px(11.5))
            .text_color(theme.muted_foreground)
            .child("The styling and UI design are from ")
            .child(credit_link("term-credit", "Kaku", "https://kaku.fun"))
            .child(", made by ")
            .child(credit_link("term-credit-author", "tw93", "https://github.com/tw93"))
            .child(".")
            .into_any_element();
        vec![
            group(Some("Look"), None, vec![line("Theme", None, term_theme, &theme), line("Cursor", None, term_cursor, &theme), line("Blinking cursor", None, term_blink, &theme), line("Font", Some(div().child("The window's monospace font and size.").into_any_element()), fonts, &theme)], &theme),
            group(Some("Behavior"), None, vec![line("Ligatures", Some(joins), term_ligatures, &theme), line("Copy on select", None, term_copy, &theme)], &theme),
            credit,
        ]
    }

    /// Files: one switch, and the languages it formats, in two columns
    /// on one card, there whether it is on or not and dimmed while off.
    fn files_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let formatting = self.cfg.app.format_on_save;
        let format_on_save = self.switch(
            "format-on-save",
            formatting,
            Rc::new(|this, on, _, cx| {
                this.cfg.app.format_on_save = on;
                this.save_app_config();
                cx.notify();
            }),
            cx,
        );
        let column = |title: &'static str, code: bool, cx: &mut Context<Self>| {
            let rows = emaki_core::format::LANGS.iter().filter(|l| l.code == code).map(|l| {
                let (key, on) = (l.key, self.cfg.app.format_langs.iter().any(|k| k == l.key));
                // A language whose formatter is a program this machine
                // lacks says which, since ticking it does nothing here.
                let needs = self.format_missing.get_or_init(|| emaki_core::format::LANGS.iter().filter_map(|l| emaki_core::format::missing_program(l.key).map(|tool| (l.key, tool))).collect()).iter().find(|(k, _)| *k == key).map(|(_, tool)| *tool);
                h_flex()
                    .h(px(30.))
                    .gap(px(9.))
                    .items_center()
                    // The tick first, beside the language it is for.
                    .child(Checkbox::new(SharedString::from(format!("format-{key}"))).checked(on).disabled(!formatting).on_click(cx.listener(move |this, on: &bool, _, cx| {
                        this.cfg.app.format_langs.retain(|k| k != key);
                        if *on {
                            this.cfg.app.format_langs.push(key.to_string());
                        }
                        this.save_app_config();
                        cx.notify();
                    })))
                    .child(img(crate::file_icons::path(l.file, false, false, dark)).size(px(16.)).flex_shrink_0())
                    // The names in a column of one width, so what follows
                    // them starts at one place down the list.
                    .child(div().w(px(84.)).flex_shrink_0().text_size(px(13.)).child(l.name))
                    .child(div().min_w_0().truncate().text_size(px(11.5)).text_color(theme.muted_foreground).child(match needs {
                        Some(_) => format!("by {}, not installed", l.by),
                        None => format!("by {}", l.by),
                    }))
            });
            v_flex().flex_1().min_w_0().child(div().pb(px(4.)).text_size(px(11.5)).text_color(theme.muted_foreground).child(title)).children(rows.collect::<Vec<_>>())
        };
        let lists = h_flex()
            .p(px(14.))
            .items_start()
            .gap(px(16.))
            .when(!formatting, |d| d.opacity(0.45))
            .child(column("Code", true, cx))
            .child(column("Documents and data", false, cx))
            .into_any_element();
        vec![group(None, None, vec![line("Format on save", Some(div().child("A file saved from its pane is put in its language's form.").into_any_element()), format_on_save, &theme)], &theme), group(Some("Languages"), None, vec![lists], &theme)]
    }

    /// Updates, as little as possible: the version, with what a check
    /// found, and one button that is "Check for updates" until a check
    /// finds something and "Update to x" after, in the same place, with
    /// "Release notes" under the version then.
    fn updates_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let u = &self.update;
        let current = update::current_version();
        let update_line = if u.installing {
            match u.progress {
                Some((done, Some(total))) if total > 0 => format!("Downloading… {}%", done * 100 / total),
                Some(_) => "Downloading…".to_string(),
                None => "Installing… Emaki restarts by itself.".to_string(),
            }
        } else if !u.install_error.is_empty() {
            format!("The update did not go through: {}", u.install_error)
        } else if !u.error.is_empty() {
            "Could not reach GitHub.".to_string()
        } else if u.available.is_none() && u.answered && !u.automatic {
            "Up to date.".to_string()
        } else {
            String::new()
        };
        let can_install = u.available.is_some() && !u.installing && update::asset_name().is_some();
        let (checking, installing) = (u.checking, u.installing);
        let notes_url = u.available.as_deref().map(update::release_page);
        let press = |id: &'static str, label: SharedString, kind: Press, off: bool| crate::branches::small_pill(id, "icons/redo-2.svg", label, kind, off, &theme).h(CONTROL_H).w(UPDATE_W).justify_center();
        let button = if can_install {
            press("update-now", format!("Update to {}", u.available.as_deref().unwrap_or("")).into(), Press::Primary, false).on_click(cx.listener(|this, _, _, cx| this.install_update_now(cx))).into_any_element()
        } else if installing {
            press("update-busy", "Updating…".into(), Press::Quiet, true).into_any_element()
        } else {
            press("update-check", if checking { "Checking…" } else { "Check for updates" }.into(), Press::Quiet, checking).on_click(cx.listener(|this, _, _, cx| this.check_updates_now(cx))).into_any_element()
        };
        let (link_color, link_hover) = (theme.muted_foreground, theme.primary);
        let version_detail: SharedString = if update_line.is_empty() { format!("Emaki {current}") } else { format!("Emaki {current} · {update_line}") }.into();
        let version_note = v_flex().gap(px(1.)).child(div().child(version_detail)).when_some(notes_url, |d, url| {
            d.child(
                div()
                    .id("update-notes")
                    .self_start()
                    .cursor_pointer()
                    .text_color(link_color)
                    .text_decoration_1()
                    .text_decoration_color(link_color.opacity(0.6))
                    .hover(move |s| s.text_color(link_hover).text_decoration_color(link_hover))
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .child("Release notes"),
            )
        });
        let auto_check = self.switch("update-auto", self.cfg.app.check_updates, Rc::new(|this, on, _, cx| this.set_check_updates(on, cx)), cx);
        vec![group(None, None, vec![line("Version", Some(version_note.into_any_element()), button, &theme), line("Check automatically", None, auto_check, &theme)], &theme)]
    }

    /// Who makes Emaki: the app on a title card, and a sentence naming
    /// the studio and its two owners, each name a link to where they are
    /// found.
    fn about_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let current = update::current_version();
        let link_hover = theme.primary;
        let link = |id: &'static str, name: &'static str, url: &'static str| {
            div()
                .id(id)
                .cursor_pointer()
                .text_color(theme.foreground)
                .text_decoration_1()
                .text_decoration_color(theme.foreground.opacity(0.35))
                .hover(move |s| s.text_color(link_hover).text_decoration_color(link_hover))
                .on_click(move |_, _, cx| cx.open_url(url))
                .child(name)
        };
        let prose = || h_flex().flex_wrap().justify_center().text_size(px(13.5)).line_height(px(22.)).text_color(theme.muted_foreground);
        let go = |id: &'static str, icon: &'static str, label: &'static str, url: String| crate::branches::small_pill(id, icon, label, Press::Quiet, false, &theme).h(CONTROL_H).on_click(move |_, _, cx| cx.open_url(&url));
        vec![
            // A title card, set in the middle as a film's first one is:
            // who presents, the app as the sidebar names it, and its
            // version. The icon and the name are each a link to the
            // app's own page: two things to press, and the room between
            // them is not one.
            v_flex()
                .w_full()
                .items_center()
                .pt(px(30.))
                .pb(px(28.))
                .rounded(CARD_R)
                .border_1()
                .border_color(theme.border)
                .bg(card_ground(&theme))
                .child(prose().text_size(px(12.5)).child(link("about-studio", "Alkaline Software", "https://github.com/alkaline-software")).child(" presents"))
                .child(div().id("about-icon").mt(px(18.)).cursor_pointer().hover(|s| s.opacity(0.82)).on_click(|_, _, cx| cx.open_url("https://emaki.lol")).child(img("icon/app-large.png").size(px(88.)).flex_shrink_0()))
                .child(
                    div()
                        .id("about-name")
                        .mt(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.82))
                        .on_click(|_, _, cx| cx.open_url("https://emaki.lol"))
                        .text_size(px(42.))
                        .line_height(px(48.))
                        .text_color(theme.primary)
                        .when_some(crate::fonts::wordmark_family(cx), |d, f| d.font_family(f))
                        .child("Emaki"),
                )
                .child(div().mt(px(4.)).text_size(px(12.5)).text_color(theme.muted_foreground).child(format!("Version {current}")))
                .into_any_element(),
            v_flex()
                .w_full()
                .items_center()
                .gap(px(16.))
                .pt(px(4.))
                .child(prose().child("A studio by ").child(link("about-jp", "Dr. John Paul Helveston", "https://www.jhelvy.com")).child(" and ").child(link("about-pingfan", "Dr. Pingfan Hu", "https://pingfan.org")).child("."))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(go("about-site", "icons/globe.svg", "Website", "https://emaki.lol".into()))
                        .child(go("about-source", "icons/github.svg", "GitHub", update::repo_page()))
                        .child(go("about-notes", "icons/file-text.svg", "Release Notes", update::release_page(&current))),
                )
                .into_any_element(),
        ]
    }
}

/// A group of the settings: its name over it and its rows on one card.
fn group(title: Option<&'static str>, aside: Option<AnyElement>, rows: Vec<AnyElement>, theme: &gpui_component::Theme) -> AnyElement {
    group_with(title.map(|t| div().child(t).into_any_element()), aside, rows, theme)
}

/// `group` whose name is more than words (an agent's mark and name), and
/// with what belongs to the whole group at the name's right (`aside`).
fn group_with(title: Option<AnyElement>, aside: Option<AnyElement>, rows: Vec<AnyElement>, theme: &gpui_component::Theme) -> AnyElement {
    let head = (title.is_some() || aside.is_some()).then(|| {
        // In line with the rows' names and controls under it.
        h_flex()
            .min_h(px(20.))
            .px(px(14.))
            .gap(px(12.))
            .items_center()
            .justify_between()
            .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).children(title))
            .children(aside)
    });
    v_flex().gap(px(6.)).children(head).child(card(rows, theme)).into_any_element()
}

/// The ground of a group: a card a shade off the page, with a hairline
/// between its rows. A row that paints its own ground rounds it where it
/// meets the card's corners: a clip in gpui is a rectangle.
fn card(rows: Vec<AnyElement>, theme: &gpui_component::Theme) -> Div {
    v_flex()
        .w_full()
        .rounded(CARD_R)
        .border_1()
        .border_color(theme.border)
        .bg(card_ground(theme))
        .children(rows.into_iter().enumerate().map(|(i, row)| div().when(i > 0, |d| d.border_t_1().border_color(theme.border)).child(row)))
}

/// A card's ground: the muted shade thinned over the panel in the light
/// appearance, the sidebar's, darker than the panel, in the dark one.
fn card_ground(theme: &gpui_component::Theme) -> Hsla {
    if theme.mode.is_dark() {
        theme.sidebar
    } else {
        theme.muted.opacity(0.4)
    }
}

/// A row of a card: its name, and under it a line where the name does
/// not say it all; its control at the right.
fn line(title: &'static str, detail: Option<AnyElement>, control: AnyElement, theme: &gpui_component::Theme) -> AnyElement {
    h_flex()
        .min_h(px(46.))
        .px(px(14.))
        .py(px(8.))
        .gap(px(16.))
        .items_center()
        .child(v_flex().flex_1().min_w_0().gap(px(1.)).child(div().text_size(px(13.)).child(title)).children(detail.map(|d| div().text_size(px(11.5)).line_height(px(15.)).text_color(theme.muted_foreground).child(d))))
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}
