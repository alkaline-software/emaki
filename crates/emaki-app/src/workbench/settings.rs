//! The settings window (⌘,), Kotoba's settings window in Emaki
//! (`docs/design.md`): a window of its own, the rail of sections at its
//! left under the traffic lights, and at the right the chosen section's
//! name in the window's top over its groups, each a card of rows with a
//! name at the left and a control at the right. The window draws the
//! workbench's own state (`SettingsView`), so every change is drawn at
//! once in both windows and written to `config.json`.

use super::*;

/// The window's size when it opens, and the least it is dragged to.
/// Every section fits the first unscrolled. It is never narrower than
/// it opens: the rows are laid out for that width (the controls'
/// `CONTROL_W` beside a name and its line), and narrower, names broke
/// mid-word and the monospace tiles cut their faces' names. Shorter, the
/// page scrolls.
pub const SETTINGS_W: Pixels = px(820.);
pub const SETTINGS_H: Pixels = px(600.);
const SETTINGS_MIN: Size<Pixels> = Size { width: SETTINGS_W, height: px(460.) };
/// The rail's width, and the page's margin at either side.
pub const SETTINGS_RAIL_W: Pixels = px(190.);
const PAGE_X: Pixels = px(36.);
/// The window's top, clear for the traffic lights and dragged by, where
/// the section's name stands level with them.
const TOP_H: Pixels = px(52.);
/// What the widest controls at a row's right take, so those on a page
/// start and end together.
const CONTROL_W: Pixels = px(336.);
/// Every field, button and choice in a row is this tall.
const CONTROL_H: Pixels = px(28.);
/// The update's button at its least, so its words change in place; a
/// language whose words run longer widens it.
const UPDATE_W: Pixels = px(190.);
/// A card's corner.
const CARD_R: Pixels = px(12.);
/// The language list at Appearance's title: as wide as its longest
/// choice, "ZH-CN" by its flag, or the system's name in any language.
const LANGUAGE_W: Pixels = px(136.);
/// How long a command in Shortcuts says what has the keys pressed.
const REFUSED_FOR: Duration = Duration::from_millis(2200);
/// A row of Shortcuts, two commands side by side.
const SHORTCUT_ROW: f32 = 44.;
/// Panels' two choices, wider than the other controls: each choice is
/// two names with their icons, which in French and German ran to the
/// track's edge at the controls' width.
const SIDES_W: Pixels = px(372.);
/// How long the pieces of Panels' picture take to move to their sides.
const SIDES_ANIM: Duration = Duration::from_millis(320);
/// The key context of the settings window, where Escape puts a list away.
pub const SETTINGS_CONTEXT: &str = "Settings";

/// The settings window's root: the workbench's settings, drawn in a
/// window of their own. Everything it draws is the workbench's, so a
/// listener on it is the workbench's too.
pub struct SettingsView {
    workbench: WeakEntity<Workbench>,
}

impl SettingsView {
    fn new(workbench: WeakEntity<Workbench>, cx: &mut Context<Self>) -> Self {
        if let Some(w) = workbench.upgrade() {
            cx.observe(&w, |_, _, cx| cx.notify()).detach();
        }
        // The window is gone: what belonged to it goes with it. Closing
        // it from code (⌘W) never asks, so this is the one place to hear.
        cx.on_release(|this: &mut Self, cx| {
            if let Some(w) = this.workbench.upgrade() {
                w.update(cx, |w, cx| w.settings_detach(cx));
            }
        })
        .detach();
        Self { workbench }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.workbench.upgrade() {
            Some(w) => w.update(cx, |w, cx| w.render_settings(window, cx)),
            None => div().into_any_element(),
        }
    }
}

/// Open the settings window, centerd on the screen, as a Mac's settings
/// window opens.
fn open_settings_window(workbench: WeakEntity<Workbench>, cx: &mut App) {
    let bounds = Bounds::centered(None, size(SETTINGS_W, SETTINGS_H), cx);
    let titlebar = if cfg!(target_os = "macos") {
        TitlebarOptions { title: Some(t("Settings").into()), appears_transparent: true, traffic_light_position: Some(point(px(17.), px(19.))) }
    } else {
        TitlebarOptions { title: Some(t("Settings").into()), appears_transparent: false, traffic_light_position: None }
    };
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(titlebar),
            window_min_size: Some(SETTINGS_MIN),
            app_id: Some("Emaki".into()),
            // The top is the app's to drag by (`drag_region`): left to
            // AppKit, a press on a control there moved the window.
            app_owns_titlebar_drag: true,
            ..Default::default()
        },
        |window, cx| {
            crate::a11y::install_window_focus_forwarder(window);
            if let Some(w) = workbench.upgrade() {
                w.update(cx, |w, cx| w.settings_attach(window, cx));
            }
            let view = cx.new(|cx| SettingsView::new(workbench.clone(), cx));
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        },
    );
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| {
                // A probe's windows stand over the others, where they draw.
                if std::env::var_os("EMAKI_GO").is_some() {
                    crate::sys::keep_on_top(window);
                }
                window.activate_window();
            });
        }
        Err(e) => eprintln!("emaki: could not open the settings: {e}"),
    }
}

impl Workbench {
    /// Show the settings, on `section` when one is asked for: the window
    /// comes forward, or opens. It is opened once the update asking for
    /// it is over, since its first draw is the workbench's.
    pub(crate) fn open_settings(&mut self, section: Option<&'static str>, cx: &mut Context<Self>) {
        if let Some(name) = section {
            self.settings_go(name, cx);
            if self.pick.as_ref().is_some_and(|p| p.in_settings) {
                self.close_pick(cx);
            }
        }
        cx.notify();
        let this = cx.entity().downgrade();
        let open = self.settings_window;
        cx.defer(move |cx| {
            cx.activate(true);
            if let Some(h) = open {
                if h.update(cx, |_, window, _| window.activate_window()).is_ok() {
                    return;
                }
            }
            open_settings_window(this, cx);
        });
    }

    /// A section, or a page of one, by its name (`settings_place`), with
    /// the page scrolled to its top.
    pub(crate) fn settings_go(&mut self, name: &'static str, cx: &mut Context<Self>) {
        let Some((section, page)) = settings_place(name) else { return };
        let moved = self.settings_section != section || page.is_some_and(|p| self.settings_page(section) != p);
        self.settings_section = section;
        if let Some(page) = page {
            self.settings_pages.insert(section, page);
        }
        if moved {
            self.settings_scroll.set_offset(point(px(0.), px(0.)));
            self.close_settings_lists(cx);
        }
        cx.notify();
    }

    /// The page a section shows: the one chosen, else its first; none
    /// for a section of one page.
    pub(crate) fn settings_page(&self, section: &str) -> &'static str {
        let Some((_, pages)) = SETTINGS_PAGES.iter().find(|(s, _)| *s == section) else { return "" };
        self.settings_pages.get(section).copied().filter(|p| pages.iter().any(|(k, _)| k == p)).unwrap_or(pages[0].0)
    }

    /// Whether `window` is the settings window: a list or a menu opened
    /// in it is drawn there, not in the main window.
    pub(crate) fn is_settings_window(&self, window: &Window) -> bool {
        self.settings_window.is_some_and(|h| h.window_id() == window.window_handle().window_id())
    }

    /// Close the settings window, with the main window or at ⌘W.
    pub(crate) fn close_settings(&mut self, cx: &mut App) {
        if let Some(h) = self.settings_window {
            let _ = h.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// The window just opened: the name's field, which belongs to a
    /// window, made in it, and the keys given to the window.
    fn settings_attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_window = Some(window.window_handle());
        let machine = crate::sys::user_first_name();
        let saved = self.cfg.app.user_name.clone();
        let name_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(if machine.is_empty() { t("Your name").to_string() } else { machine });
            s.set_value(saved, window, cx);
            s
        });
        cx.subscribe_in(&name_input, window, |this, input, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                let name = input.read(cx).value().trim().to_string();
                if name != this.cfg.app.user_name {
                    this.user_name = if name.is_empty() { crate::sys::user_first_name() } else { name.clone() };
                    this.cfg.app.user_name = name;
                    this.save_app_config();
                    cx.notify();
                }
            }
        })
        .detach();
        self.name_input = Some(name_input);
        // Text Size's fields: a number with a tenth at most, set at ↩ or
        // when the field is left; anything else put back as it was.
        self.size_inputs = SIZE_PLACES
            .iter()
            .map(|(key, _)| {
                let key: &'static str = key;
                let input = cx.new(|cx| InputState::new(window, cx).validate(|text, _| size_typable(text)));
                cx.subscribe_in(&input, window, move |this, input, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.size_commit(key, input, window, cx);
                    }
                })
                .detach();
                (key, input)
            })
            .collect();
        // A list or a menu open in the window goes when another window is
        // used, as the Mac's own do.
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.close_settings_lists(cx);
            }
        })
        .detach();
        window.focus(&self.settings_focus, cx);
    }

    /// What was typed in a place's field is its size, to a tenth and in
    /// range; anything that is no number leaves it, and the field says
    /// the size it has.
    fn size_commit(&mut self, key: &'static str, input: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
        let typed = input.read(cx).value().trim().parse::<f32>().ok().filter(|v| *v > 0.);
        if let Some(v) = typed {
            self.set_text_size(|s| *size_field(s, key) = emaki_core::config::TextSizes::typed(v), cx);
        }
        let now = size_words(*size_field(&mut self.cfg.app.text_sizes(), key));
        input.update(cx, |s, cx| s.set_value(now, window, cx));
    }

    /// The window is gone: its field, and a list or a menu it had open.
    fn settings_detach(&mut self, cx: &mut Context<Self>) {
        self.settings_window = None;
        self.shortcut_rec = None;
        self.name_input = None;
        self.size_inputs.clear();
        if self.pick.as_ref().is_some_and(|p| p.in_settings) {
            self.pick = None;
        }
        if self.pick_gone.as_ref().is_some_and(|(p, _)| p.in_settings) {
            self.pick_gone = None;
        }
        if self.menu.as_ref().is_some_and(|m| m.in_settings) {
            self.menu = None;
        }
        if self.menu_gone.as_ref().is_some_and(|(m, _)| m.in_settings) {
            self.menu_gone = None;
        }
        cx.notify();
    }

    /// Put away a list or a menu open in the settings window, and stop a
    /// tile's wait for its keys.
    fn close_settings_lists(&mut self, cx: &mut Context<Self>) {
        if self.shortcut_rec.take().is_some() {
            cx.notify();
        }
        if self.pick.as_ref().is_some_and(|p| p.in_settings) {
            self.close_pick(cx);
        }
        if self.menu.as_ref().is_some_and(|m| m.in_settings) {
            self.close_menu(cx);
        }
    }

    /// The settings window's content: the rail, and the section with its
    /// name in the window's top, and over them a list or a menu opened
    /// here.
    pub(super) fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let section = self.settings_section;
        // Text Size's fields say the sizes as they are, but for the one
        // being typed in.
        let sizes = self.cfg.app.text_sizes();
        for (key, input) in self.size_inputs.clone() {
            let now = size_words(*size_field(&mut sizes.clone(), key));
            if !input.read(cx).focus_handle(cx).is_focused(window) && input.read(cx).value() != now.as_str() {
                input.update(cx, |s, cx| s.set_value(now, window, cx));
            }
        }
        if let Some((place, words)) = self.size_probe.take() {
            if let Some((_, input)) = self.size_inputs.iter().find(|(k, _)| *k == place) {
                let input = input.clone();
                input.update(cx, |s, cx| s.set_value(words, window, cx));
                let key = SIZE_PLACES.iter().map(|(k, _)| *k).find(|k| *k == place).unwrap_or("conversation");
                self.size_commit(key, &input, window, cx);
            }
        }
        let page = self.settings_page(section);
        let rows = match (section, page) {
            ("text", "size") => self.text_rows(cx),
            ("text", _) => self.fonts_rows(cx),
            ("conversation", _) => self.conversation_rows(cx),
            ("panels", _) => self.panels_rows(window, cx),
            ("files", _) => self.files_rows(cx),
            ("terminal", _) => self.terminal_rows(cx),
            ("sessions", _) => self.sessions_rows(cx),
            ("shortcuts", _) => self.shortcut_rows(cx),
            ("about", _) => self.about_rows(cx),
            _ => self.appearance_rows(cx),
        };
        let title = SETTINGS_SECTIONS.iter().find(|(k, _, _)| *k == section).map(|(_, name, _)| t(name)).unwrap_or(t("Appearance"));
        // The window's language, at the right of Appearance's name in the
        // window's top, as Kotoba has it at General's: the system's, or a
        // language by its flag and letters, in a list that drops down. A
        // press on it is not a drag of the top.
        let (lang_icon, lang_label) = match crate::i18n::CHOICES.iter().find(|(c, _, _)| *c == self.cfg.app.language) {
            Some((_, flag, letters)) => (*flag, letters.to_string()),
            None => ("icons/globe.svg", t("System").to_string()),
        };
        let language = page_in(
            "language-pick",
            div().w(LANGUAGE_W).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(self.settings_pick_h(PickFor::Language, "language", lang_icon, PillText::plain(lang_label), CONTROL_H, cx)),
        );
        // A section of more than one page has its pages at the right of
        // its name, as Appearance has its language.
        let pages = SETTINGS_PAGES.iter().find(|(s, _)| *s == section).map(|(s, pages)| {
            let control: &'static str = if *s == "text" { "settings-pages-text" } else { "settings-pages-shortcuts" };
            let section: &'static str = s;
            page_in(
                control,
                div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(self.segmented(
                    control,
                    pages.iter().map(|(k, name)| (*k, t(name).to_string(), None)).collect(),
                    page,
                    Rc::new(move |this, key, _, cx| {
                        this.settings_pages.insert(section, key);
                        this.settings_scroll.set_offset(point(px(0.), px(0.)));
                        this.close_settings_lists(cx);
                        cx.notify();
                    }),
                    cx,
                )),
            )
        });
        // New sessions' agents, by their marks, where Text has its pages.
        let agents = page_in(
            "settings-agents",
            div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(self.segmented(
                "new-sessions-agent",
                AgentId::ALL.iter().map(|a| (a.as_str(), String::new(), None)).collect(),
                self.settings_agent.as_str(),
                Rc::new(|this, key, _, cx| {
                    if let Some(agent) = AgentId::parse(key) {
                        this.settings_agent = agent;
                        this.close_pick(cx);
                        cx.notify();
                    }
                }),
                cx,
            )),
        );
        let (title_anim, page_anim): (&'static str, &'static str) = match (section, page) {
            ("text", "size") => ("title-text", "page-text-size"),
            ("text", _) => ("title-text", "page-text-fonts"),
            ("conversation", _) => ("title-conversation", "page-conversation"),
            ("panels", _) => ("title-panels", "page-panels"),
            ("files", _) => ("title-files", "page-files"),
            ("terminal", _) => ("title-terminal", "page-terminal"),
            ("sessions", _) => ("title-sessions", "page-sessions"),
            ("shortcuts", "editor") => ("title-shortcuts", "page-shortcuts-editor"),
            ("shortcuts", _) => ("title-shortcuts", "page-shortcuts"),
            ("about", _) => ("title-about", "page-about"),
            _ => ("title-appearance", "page-appearance"),
        };
        div()
            .id("settings-window")
            .key_context(SETTINGS_CONTEXT)
            .track_focus(&self.settings_focus)
            .size_full()
            .relative()
            .bg(theme.popover)
            .text_color(theme.foreground)
            .text_size(px(13.))
            // Escape puts away a list or a menu opened here, and with
            // none open closes the window, as ⌘W does.
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                let open = this.pick.as_ref().is_some_and(|p| p.in_settings) || this.menu.as_ref().is_some_and(|m| m.in_settings);
                if open {
                    this.close_settings_lists(cx);
                } else {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|_, _: &CloseTab, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &crate::CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &OpenSettings, window, _| window.activate_window()))
            // A tile waiting for its keys shows the modifiers held, and a
            // press anywhere else stops the wait.
            .on_modifiers_changed(cx.listener(|this, e: &ModifiersChangedEvent, _, cx| {
                if this.shortcut_rec.is_some() {
                    this.shortcut_held = e.modifiers;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.shortcut_rec.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(
                h_flex()
                    .size_full()
                    .items_stretch()
                    // A new language is faded into, rail and page alike.
                    .with_animation(ElementId::NamedInteger("settings-lang".into(), self.lang_serial as u64), Animation::new(Duration::from_millis(240)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
                    .child(self.settings_rail(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            // The window's top is dragged by, and holds the
                            // section's name level with the traffic lights,
                            // as the Mac's own settings do. It fades in with
                            // the page.
                            .child(
                                // The window's own buttons stand at its right
                                // where the system draws none (`csd.rs`).
                                Self::drag_region(h_flex().h(TOP_H).flex_shrink_0().w_full().pl(PAGE_X).pr(PAGE_X + crate::csd::room(window, cx).1).items_center(), cx)
                                    .child(page_in(title_anim, div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title)))
                                    .when(section == "appearance", |d| d.child(div().flex_1()).child(language))
                                    .when_some(pages, |d, pages| d.child(div().flex_1()).child(pages))
                                    .when(section == "sessions", |d| d.child(div().flex_1()).child(agents)),
                            )
                            // The rows scroll under the top with the
                            // toolkit's scrollbar at their edge (it fades a
                            // second after the last scroll, as every
                            // scrollbar in the app does). Every section
                            // fits the window as it opens; the scroll is
                            // for a window made smaller.
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
            .when_some(self.menu_gone.clone().filter(|(m, at)| m.in_settings && at.elapsed() < MENU_OUT), |d, (m, at)| d.child(self.render_menu(m, Some(at.elapsed()), window, cx)))
            .when_some(self.menu.clone().filter(|m| m.in_settings), |d, m| d.child(self.render_menu(m, None, window, cx)))
            .when_some(self.pick_gone.clone().filter(|(m, at)| m.in_settings && at.elapsed() < MENU_OUT), |d, (m, _)| d.child(self.render_pick(m, true, window, cx)))
            // Where its field is now, not where it was when the list opened.
            .when_some(self.pick.clone().filter(|m| m.in_settings), |d, mut m| {
                m.by = self.pill_at.borrow().get(&(m.what, true)).copied().unwrap_or(m.by);
                d.child(self.render_pick(m, false, window, cx))
            })
            .children(crate::csd::chrome(window, TOP_H / 2. - px(12.), |window, _| window.remove_window(), cx))
            .into_any_element()
    }

    /// The rail of sections at the window's left, under its traffic
    /// lights: an icon and a name per section, the chosen one on a plate.
    /// Updates wears a dot while a newer Emaki waits. Its empty room drags
    /// the window, as the top does.
    fn settings_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let current = self.settings_section;
        let update_waiting = self.update.available.is_some();
        Self::drag_region(
            v_flex()
                .w(SETTINGS_RAIL_W)
                .h_full()
                .flex_shrink_0()
                .pt(TOP_H)
                .px(px(10.))
                .gap(px(2.))
                .bg(if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.5) })
                .border_r_1()
                .border_color(theme.border),
            cx,
        )
        .children(SETTINGS_SECTIONS.iter().map(|(key, label, icon)| {
            let active = *key == current;
            let key: &'static str = key;
            let dot = key == "about" && update_waiting;
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
                // A press on a section is not a drag of the window.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.settings_section != key {
                        this.settings_section = key;
                        this.settings_scroll.set_offset(point(px(0.), px(0.)));
                        this.close_settings_lists(cx);
                    }
                    cx.notify();
                }))
                .child(Icon::default().path(*icon).with_size(px(15.)).text_color(if active { theme.foreground } else { theme.muted_foreground }))
                .child(div().flex_1().text_size(px(13.)).when(active, |d| d.font_weight(FontWeight::MEDIUM)).text_color(if active { theme.foreground } else { theme.sidebar_foreground }).child(t(label)))
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
    /// field (`name_input`, made with the window), in the focus wrapper
    /// every own input has.
    fn your_name_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(input) = self.name_input.clone() else { return div().w(CONTROL_W).h(CONTROL_H).into_any_element() };
        div()
            .id("name-field")
            .track_focus(&input.read(cx).focus_handle(cx))
            .role(Role::TextInput)
            .aria_label(t("Your name"))
            .aria_value(input.read(cx).value().to_string())
            .w(CONTROL_W)
            .h(CONTROL_H)
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .bg(theme.muted)
            .text_size(px(12.5))
            .child(Input::new(&input).appearance(false).bordered(false).on_secondary_click(self.input_menu(&input, false, cx)))
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
        let appearance = div().w(CONTROL_W).child(self.segmented("appearance", vec![("system", t("System").into(), None), ("light", t("Light").into(), None), ("dark", t("Dark").into(), None)], appearance_key, Rc::new(|this, key, window, cx| this.set_appearance(key, window, cx)), cx)).into_any_element();

        // The picture at the start of the controls, what is done with it
        // after: a click on the picture chooses one too.
        let avatar = h_flex()
            .gap(px(8.))
            .items_center()
            .child(self.avatar(28., cx).id("settings-avatar").cursor_pointer().hover(|s| s.opacity(0.85)).on_click(cx.listener(|this, _, _, cx| this.pick_avatar(cx))))
            .child(crate::branches::small_pill("avatar-choose", "icons/image.svg", t("Choose…"), Press::Quiet, false, &theme).h(CONTROL_H).on_click(cx.listener(|this, _, _, cx| this.pick_avatar(cx))))
            .when(!app.avatar.is_empty(), |d| d.child(crate::branches::small_pill("avatar-remove", "icons/trash.svg", t("Remove"), Press::Danger, false, &theme).h(CONTROL_H).on_click(cx.listener(|this, _, _, cx| this.set_avatar(String::new(), cx)))))
            .into_any_element();

        // Accents are swatches: a disc of the color, the chosen one with
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
                let name = t(a.name);
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
        let accent_name = t(crate::look::accent(&app.accent).name);
        let display_on = self.switch("keep-display-on", app.keep_display_on, Rc::new(|this, on, _, cx| this.set_keep_display_on(on, cx)), cx);

        vec![
            group(Some(t("You")), None, vec![line(t("Avatar"), None, avatar, &theme), line(t("Name"), None, self.your_name_field(cx), &theme)], &theme),
            group(Some(t("Window")), None, vec![line(t("Theme"), None, appearance, &theme), line(t("Accent"), Some(div().child(accent_name).into_any_element()), accents, &theme)], &theme),
            group(Some(t("Power")), None, vec![line(t("Keep the display on while a session works"), None, display_on, &theme)], &theme),
        ]
    }

    /// Fonts: the conversation's face and the monospace face, each a
    /// tile drawn in itself. Their sizes are Text Size's.
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
                    // One line, at a size that fits the narrowest tile:
                    // wrapped in a narrow tile, or in a wider face drawn
                    // while its own loads, the sample pushed the face's
                    // name out of the tile.
                    .child(div().whitespace_nowrap().overflow_hidden().text_size(px(if mono { 14. } else { 20. })).line_height(px(25.)).font_family(face.family).child(sample))
                    .child(div().truncate().text_size(px(11.5)).text_color(if active { theme.foreground } else { theme.muted_foreground }).child(face.name))
            }))
        };
        // A kind of face, small and quiet, over its tiles.
        let kind = |name: &'static str| div().text_size(px(11.5)).text_color(theme.muted_foreground).child(name);
        let chat = v_flex()
            .p(px(14.))
            .gap(px(8.))
            .child(kind(t("Serif")))
            .child(tiles(crate::fonts::SERIFS, chat_now, "Ag", false, cx))
            .child(div().h(px(4.)))
            .child(kind(t("Sans serif")))
            .child(tiles(crate::fonts::SANS, chat_now, "Ag", false, cx))
            .into_any_element();
        let mono = div().p(px(14.)).child(tiles(crate::fonts::monos(), mono_now, "fn(0) -> 1", true, cx)).into_any_element();
        vec![group(Some(t("Chat font")), None, vec![chat], &theme), group(Some(t("Monospace font")), None, vec![mono], &theme)]
    }

    /// Text Size: a preset for every place at once, and each place's
    /// size under it, which a preset sets. Custom is the person's own,
    /// set here by a step or by what is typed, and kept while a preset is
    /// chosen; under a preset the sizes are its and are only shown.
    fn text_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let mut sizes = self.cfg.app.text_sizes();
        let now = match self.cfg.app.chat_size.as_str() {
            "small" => "small",
            "large" => "large",
            "custom" => "custom",
            _ => "medium",
        };
        let presets = div()
            .w(CONTROL_W)
            .child(self.segmented("size", vec![("small", t("Small").into(), None), ("medium", t("Medium").into(), None), ("large", t("Large").into(), None), ("custom", t("Custom").into(), None)], now, Rc::new(|this, key, _, cx| this.set_chat_size(key, cx)), cx))
            .into_any_element();
        let custom = now == "custom";
        let rows = SIZE_PLACES
            .iter()
            .map(|(key, name)| {
                let value = *size_field(&mut sizes, key);
                let control = h_flex().gap(px(8.)).items_center().children(self.size_reset(key, value, custom, cx)).child(self.stepper(key, value, custom, cx)).into_any_element();
                line(t(name), None, control, &theme)
            })
            .collect();
        vec![group(None, None, vec![line(t("Size"), None, presets, &theme)], &theme), group(None, None, rows, &theme)]
    }

    /// Under Custom, beside a size not Medium's: a button that puts it
    /// back to Medium's, which is Custom's own until it is changed. It
    /// comes and goes in motion (`presence`).
    fn size_reset(&self, key: &'static str, value: f32, custom: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let medium = *size_field(&mut emaki_core::config::TextSizes::MEDIUM.clone(), key);
        let shown = custom && value != medium;
        let id = format!("size-reset-{key}");
        let moving = self.presence(&id, shown, cx);
        if !shown && moving.is_none() {
            return None;
        }
        let theme = cx.theme().clone();
        let hover = theme.foreground.opacity(0.07);
        let tip: SharedString = tf("Reset to {size} pt", &[("size", &size_words(medium))]).into();
        let button = div()
            .id(SharedString::from(id.clone()))
            .size(px(26.))
            .flex_shrink_0()
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .when(shown, |d| {
                d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.set_text_size(|s| *size_field(s, key) = medium, cx);
                }))
            })
            .managed_tooltip(move |window, cx| crate::workbench::tip(tip.clone(), window, cx))
            .child(Icon::default().path("icons/rotate.svg").with_size(px(14.)).text_color(theme.muted_foreground));
        Some(match moving {
            Some((coming, n)) => button.with_animation(ElementId::Name(format!("{id}-{n}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
            None => button.into_any_element(),
        })
    }

    /// A size in points: a step down, the number in a field of its own
    /// with "pt" after it, and a step up, on the track a choice has
    /// (`segmented`). A step is a whole point; a tenth is only typed.
    /// Not `live`, it shows a preset's size and nothing in it moves it.
    fn stepper(&self, key: &'static str, value: f32, live: bool, cx: &mut Context<Self>) -> AnyElement {
        use emaki_core::config::TextSizes;
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let track = if dark { theme.tab_bar_segmented } else { theme.muted };
        let plate = if dark { theme.secondary_active } else { theme.popover };
        let step = |up: bool, icon: &'static str| {
            let to = TextSizes::step(value, up);
            let able = live && to != value;
            div()
                .id(SharedString::from(format!("size-{key}-{}", if up { "up" } else { "down" })))
                .size(px(22.))
                .flex_shrink_0()
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .when(able, |d| {
                    d.cursor_pointer().hover(move |s| s.bg(plate).shadow_sm()).on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.set_text_size(|s| *size_field(s, key) = to, cx);
                    }))
                })
                .child(Icon::default().path(icon).with_size(px(13.)).text_color(if able { theme.foreground } else { theme.muted_foreground.opacity(0.5) }))
        };
        let down = step(false, "icons/minus.svg");
        let up = step(true, "icons/plus.svg");
        // The number in the window's face, a weight over the words, in
        // the middle of its box and in full ink: under a preset only the
        // steps are quiet, and the field has no plate to type on.
        let face = theme.font_family.clone();
        let number = match self.size_inputs.iter().find(|(k, _)| *k == key).filter(|_| live) {
            Some((_, input)) => div()
                .id(SharedString::from(format!("size-{key}-field")))
                .track_focus(&input.read(cx).focus_handle(cx))
                .role(Role::TextInput)
                .aria_label(SIZE_PLACES.iter().find(|(k, _)| *k == key).map(|(_, n)| t(n)).unwrap_or(""))
                .aria_value(input.read(cx).value().to_string())
                .w(px(42.))
                .h(px(22.))
                .px(px(5.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .bg(plate)
                .cursor_text()
                .font_family(face.clone())
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(13.))
                .text_color(theme.foreground)
                // The field's own room at its sides is none here: in this
                // width it cut the number.
                .child(Input::new(input).appearance(false).bordered(false).px_0().font_family(face).text_center())
                .into_any_element(),
            None => div().w(px(42.)).text_center().font_family(face).font_weight(FontWeight::MEDIUM).text_size(px(13.)).text_color(theme.foreground).child(size_words(value)).into_any_element(),
        };
        h_flex()
            .w(px(140.))
            .h(CONTROL_H)
            .p(px(3.))
            .gap(px(4.))
            .rounded(px(8.))
            .bg(track)
            .items_center()
            .child(down)
            .child(h_flex().flex_1().justify_center().items_center().gap(px(3.)).child(number).child(div().text_size(px(12.5)).text_color(theme.muted_foreground).child("pt")))
            .child(up)
            .into_any_element()
    }

    /// Conversation: how its tool calls are drawn, what is typed (the
    /// composer's rows), and the two helpers a small model writes for,
    /// each a call on the person's account, so each can be turned off.
    fn conversation_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let fold = self.switch("fold-runs", self.cfg.app.fold_runs, Rc::new(|this, on, _, cx| this.set_fold_runs(on, cx)), cx);
        let mut groups = vec![group(Some(t("Agent")), None, vec![line(t("Fold runs of tool calls"), Some(div().child(t("Three or more in a row show as one row that opens.")).into_any_element()), fold, &theme)], &theme)];
        groups.extend(self.composer_rows(cx));
        let explain = self.switch("explain-on", self.cfg.explain.enabled, Rc::new(|this, on, _, cx| this.set_explain(on, cx)), cx);
        let outline = self.switch("outline-on", self.cfg.app.outline, Rc::new(|this, on, _, cx| this.set_outline(on, cx)), cx);
        groups.push(group(
            Some(t("On-demand model calls")),
            None,
            vec![line(t("Explain tool calls"), None, explain, &theme), line(t("Outline panel"), None, outline, &theme)],
            &theme,
        ));
        groups
    }

    /// Panels: the side of the conversation each side panel stands at,
    /// and each margin's control, over a picture of the window whose
    /// pieces move to their places as a choice is made. Each pair is one
    /// choice, its two in the order they stand from left to right.
    fn panels_rows(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let (files_right, outline_right) = (self.files_right(), self.outline_right());
        // How far each pair has gone toward its right-hand order.
        let mut moving = false;
        let mut toward = |at: Option<Instant>, right: bool| {
            let to = if right { 1. } else { 0. };
            match at.map(|at| at.elapsed().as_secs_f32() / SIDES_ANIM.as_secs_f32()) {
                Some(p) if p < 1. => {
                    moving = true;
                    (1. - to) + (2. * to - 1.) * ease_out_quint()(p)
                }
                _ => to,
            }
        };
        let kf = toward(self.sides_anim[0], files_right);
        let ko = toward(self.sides_anim[1], outline_right);
        if moving {
            window.request_animation_frame();
        }
        let lerp = |a: f32, b: f32, k: f32| a + (b - a) * k;

        // The picture: the top strip, a side panel at either side of the
        // conversation, and in the conversation's margins the two seams.
        // No wider than the room its card has, measured as it is drawn:
        // on GNOME the page came out narrower than the window's width
        // says, and a picture of a set width ran out of its card.
        let room = self.panels_room.get();
        let w: f32 = if room > 0. { room.clamp(440., 528.) } else { 528. };
        const H: f32 = 176.;
        const STRIP: f32 = 20.;
        const SIDE: f32 = 96.;
        const COL: f32 = 168.;
        let col_l = SIDE + (w - 2. * SIDE - COL) / 2.;
        let col_r = col_l + COL;
        // The seams stand out in the margins, as the window's do, so the
        // names under their knobs clear what the column holds.
        const SEAM_OUT: f32 = 26.;
        let (seam_l, seam_r) = (col_l - SEAM_OUT, col_r + SEAM_OUT);
        let ink = theme.muted_foreground;
        let bar = |x: f32, y: f32, w: f32| div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(4.)).rounded_full().bg(ink.opacity(0.22));
        let named = |x: f32, ground: Hsla, icon: &'static str, words: &'static str| {
            v_flex()
                .absolute()
                .top(px(STRIP))
                .bottom_0()
                .left(px(x))
                .w(px(SIDE))
                .items_center()
                .justify_center()
                .gap(px(5.))
                .bg(ground)
                .text_color(ink)
                .child(Icon::default().path(icon).with_size(px(15.)))
                .child(div().text_size(px(10.5)).child(words))
        };
        let strip_icon = |x: f32, icon: &'static str| div().absolute().top(px(5.)).left(px(x)).text_color(ink).child(Icon::default().path(icon).with_size(px(10.)));
        let seam = |x: f32, icon: &'static str, words: &'static str| {
            // The line stops for the knob and the name under it.
            let (top, foot) = (STRIP + 12., H - 42.);
            let mid = (top + foot) / 2. - 8.;
            let line = |from: f32, to: f32| div().absolute().left(px(x - 0.75)).top(px(from)).h(px(to - from)).w(px(1.5)).bg(theme.primary.opacity(0.5));
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(line(top, mid - 9.))
                .child(line(mid + 30., foot))
                .child(
                    div()
                        .absolute()
                        .left(px(x - 9.))
                        .top(px(mid - 9.))
                        .size(px(18.))
                        .rounded_full()
                        .border_1()
                        .border_color(theme.primary.opacity(0.7))
                        .bg(theme.background)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(theme.primary)
                        .child(Icon::default().path(icon).with_size(px(10.))),
                )
                .child(div().absolute().left(px(x - 40.)).w(px(80.)).top(px(mid + 13.)).text_center().text_size(px(10.)).text_color(ink).child(words))
        };
        let picture = div()
            .relative()
            .w(px(w))
            .h(px(H))
            .mx_auto()
            .overflow_hidden()
            .rounded(px(10.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(div().absolute().top_0().left_0().right_0().h(px(STRIP)).border_b_1().border_color(theme.border).bg(theme.muted.opacity(0.3)))
            .child(div().absolute().top(px(5.)).left(px(w / 2. - 40.)).w(px(80.)).h(px(10.)).rounded(px(4.)).bg(theme.muted))
            .child(strip_icon(lerp(8., w - 20., kf), "icons/tree-view.svg"))
            .child(strip_icon(lerp(w - 20., 8., kf), "icons/terminal.svg"))
            .child(named(lerp(0., w - SIDE, kf), crate::look::files_ground(&theme), "icons/tree-view.svg", t("Files")))
            .child(named(lerp(w - SIDE, 0., kf), self.term_ground(cx), "icons/terminal.svg", t("Terminal")))
            // The conversation: a prompt, a reply, a prompt, the composer.
            .child(div().absolute().left(px(col_r - 14. - 64.)).top(px(STRIP + 14.)).w(px(64.)).h(px(12.)).rounded(px(6.)).bg(theme.muted))
            .child(bar(col_l + 14., STRIP + 36., 126.))
            .child(bar(col_l + 14., STRIP + 46., 110.))
            .child(bar(col_l + 14., STRIP + 56., 80.))
            .child(div().absolute().left(px(col_r - 14. - 48.)).top(px(STRIP + 72.)).w(px(48.)).h(px(12.)).rounded(px(6.)).bg(theme.muted))
            .child(div().absolute().left(px(col_l)).top(px(H - 32.)).w(px(COL)).h(px(22.)).rounded(px(8.)).border_1().border_color(theme.border))
            .child(seam(lerp(seam_l, seam_r, ko), "icons/list-bullets.svg", t("Outline")))
            .child(seam(lerp(seam_r, seam_l, ko), "icons/square-terminal.svg", t("Layout")));
        let held = self.panels_room.clone();
        let measure = canvas(
            move |bounds, window, _| {
                let room = f32::from(bounds.size.width);
                if (held.get() - room).abs() > 0.5 {
                    held.set(room);
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        // Each choice is drawn as its two by their icons (`side_pair`).
        let files = div()
            .w(SIDES_W)
            .child(self.segmented("files-side", vec![("left", String::new(), None), ("right", String::new(), None)], if files_right { "right" } else { "left" }, Rc::new(|this, side, _, cx| this.set_side(true, side, cx)), cx))
            .into_any_element();
        let margins = div()
            .w(SIDES_W)
            .child(self.segmented("outline-side", vec![("left", String::new(), None), ("right", String::new(), None)], if outline_right { "right" } else { "left" }, Rc::new(|this, side, _, cx| this.set_side(false, side, cx)), cx))
            .into_any_element();
        vec![
            group(None, None, vec![div().p(px(14.)).child(div().relative().w_full().child(measure).child(picture)).into_any_element()], &theme),
            group(None, None, vec![line(t("Side panels"), None, files, &theme), line(t("Margins"), None, margins, &theme)], &theme),
        ]
    }

    /// The composer's groups, at the top of Conversation: the key that
    /// sends, capitals, and spelling.
    fn composer_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let app = self.cfg.app.clone();
        // The key that sends, drawn as the keyboard has it (`key_text`:
        // the Mac's signs on a Mac, Ctrl in words elsewhere), with what
        // makes a new line under its name.
        let enter_sends = app.send_key == "enter";
        let send = div().w(CONTROL_W).child(self.segmented("send-key", vec![("cmd-enter", "⌘↩".into(), None), ("enter", "↩".into(), None)], if enter_sends { "enter" } else { "cmd-enter" }, Rc::new(|this, key, _, cx| this.set_send_key(key, cx)), cx)).into_any_element();
        let send_note = key_text_wrap(if enter_sends { t("⇧↩ for a new line") } else { t("↩ or ⇧↩ for a new line") }).into_any_element();
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
        let language_note = (!crate::sys::SYSTEM_SPELLING).then(|| div().child(t("Spanish, French and German are checked on a Mac only.")).into_any_element());
        vec![
            group(Some(t("Composer")), None, vec![line(t("Send with"), Some(send_note), send, &theme), line(t("Auto-capitalization"), None, auto_cap, &theme)], &theme),
            group(Some(t("Spelling and grammar")), None, vec![line(t("Check as you type"), None, check_writing, &theme), line(t("Language"), language_note, writing_language, &theme)], &theme),
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
        // Whose they are; the agent is chosen at the right of the
        // section's name.
        let head = h_flex().gap(px(7.)).items_center().child(agent_icon(agent, px(13.), agent_color(agent, &theme))).child(agent.display_name()).into_any_element();
        vec![group_with(Some(head), None, vec![line(t("Permission mode"), None, mode, &theme), line(t("Model"), None, model, &theme), line(t("Effort"), None, effort, &theme)], &theme)]
    }

    /// Terminal: how it looks, then how it acts, and whose design it is.
    fn terminal_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let term = self.cfg.terminal.clone();
        let pick = |list: &[&'static str], now: &str| list.iter().copied().find(|k| *k == now).unwrap_or(list[0]);
        let term_theme = div().w(CONTROL_W).child(self.segmented("term-theme", vec![("match", t("Match window").into(), None), ("dark", t("Dark").into(), None), ("light", t("Light").into(), None)], pick(&["match", "dark", "light"], &term.theme), Rc::new(|this, key, _, cx| this.set_terminal(|t| t.theme = key.into(), cx)), cx)).into_any_element();
        let term_cursor = div().w(CONTROL_W).child(self.segmented("term-cursor", vec![("bar", t("Bar").into(), None), ("block", t("Block").into(), None), ("underline", t("Underline").into(), None)], pick(&["bar", "block", "underline"], &term.cursor), Rc::new(|this, key, _, cx| this.set_terminal(|t| t.cursor = key.into(), cx)), cx)).into_any_element();
        let term_blink = self.switch("term-blink", term.cursor_blink, Rc::new(|this, on, _, cx| this.set_terminal(|t| t.cursor_blink = on, cx)), cx);
        // The size is Text Size's terminal: shown here, set there.
        let term_size = crate::branches::small_pill("term-size", "icons/a-large-small.svg", format!("{} pt", size_words(self.cfg.app.text_sizes().terminal)), Press::Quiet, false, &theme)
            .h(CONTROL_H)
            .on_click(cx.listener(|this, _, _, cx| this.settings_go("size", cx)))
            .into_any_element();
        let term_ligatures = self.switch("term-ligatures", term.ligatures, Rc::new(|this, on, _, cx| this.set_terminal(|t| t.ligatures = on, cx)), cx);
        let term_copy = self.switch("term-copy", term.copy_on_select, Rc::new(|this, on, _, cx| this.set_terminal(|t| t.copy_on_select = on, cx)), cx);
        // The face is the window's, set in Fonts.
        let fonts = crate::branches::small_pill("term-fonts", "icons/typography.svg", t("Open Fonts"), Press::Quiet, false, &theme)
            .h(CONTROL_H)
            .on_click(cx.listener(|this, _, _, cx| this.settings_go("fonts", cx)))
            .into_any_element();
        // The marks it joins, with a zero-width mark between the two
        // letters of each, or the window's own face would join them here.
        // They are blanks of the sentence, so its words are kept whole.
        let joins = div().child(tf("Joins {arrow} and {unequal} into one mark.", &[("arrow", &"-\u{200C}>"), ("unequal", &"!\u{200C}=")])).into_any_element();
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
        // The two names are links, set where each language's order puts
        // them.
        let credit = crate::i18n::weave(
            t("The styling and UI design are from {kaku}, made by {tw93}."),
            vec![("kaku", credit_link("term-credit", "Kaku", "https://kaku.fun").into_any_element()), ("tw93", credit_link("term-credit-author", "tw93", "https://github.com/tw93").into_any_element())],
            h_flex().flex_wrap().px(px(14.)).text_size(px(11.5)).text_color(theme.muted_foreground),
        )
        .into_any_element();
        vec![
            group(Some(t("Look")), None, vec![line(t("Theme"), None, term_theme, &theme), line(t("Cursor"), None, term_cursor, &theme), line(t("Blinking cursor"), None, term_blink, &theme), line(t("Font"), None, fonts, &theme), line(t("Size"), None, term_size, &theme)], &theme),
            group(Some(t("Behavior")), None, vec![line(t("Ligatures"), Some(joins), term_ligatures, &theme), line(t("Copy on select"), None, term_copy, &theme)], &theme),
            credit,
        ]
    }

    /// Shortcuts: the window's commands whose keys may change, or the
    /// file editor's, a group a card of rows as every section has them,
    /// two commands to a row: a name at the left, its keys at the right.
    /// A press on a command waits for its new keys, and one whose keys
    /// were changed has a button before them that puts its own back.
    fn shortcut_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        crate::shortcuts::groups(self.settings_page("shortcuts") == "editor")
            .iter()
            .map(|(name, list)| {
                let rows = list
                    .chunks(2)
                    .map(|pair| {
                        let mut row = h_flex().w_full();
                        for (i, c) in pair.iter().enumerate() {
                            row = row.child(self.shortcut_cell(c, i > 0, cx));
                        }
                        // A short last row keeps its one command a half,
                        // the empty half as a command's, room and edge alike.
                        if pair.len() == 1 {
                            row = row.child(div().flex_1().min_w_0().h(px(SHORTCUT_ROW)).px(px(14.)).border_l_1().border_color(theme.border));
                        }
                        row.into_any_element()
                    })
                    .collect();
                group(Some(t(name)), None, rows, &theme)
            })
            .collect()
    }

    /// One command: its name, or for a moment what has the keys just
    /// pressed; and its keys, or while it waits for new ones the
    /// modifiers held, in a field that takes the accent.
    fn shortcut_cell(&self, c: &'static crate::shortcuts::Command, right: bool, cx: &mut Context<Self>) -> AnyElement {
        use crate::shortcuts;
        let theme = cx.theme().clone();
        let id = c.id;
        let waiting = self.shortcut_rec == Some(id);
        let refused = self.shortcut_refused.as_ref().filter(|(on, _, at)| *on == id && at.elapsed() < REFUSED_FOR).map(|(_, said, _)| said.clone());
        let keys = shortcuts::keys_of(id);
        let caps = if waiting { shortcuts::held(&self.shortcut_held) } else { shortcuts::caps(&keys) };
        let keys_shown = if caps.is_empty() {
            div().text_size(px(12.)).text_color(theme.primary).child(t("Press a shortcut")).into_any_element()
        } else {
            // A chord's strokes stand apart.
            h_flex().gap(px(3.)).text_color(if waiting { theme.primary } else { theme.foreground }).children(caps.iter().map(|k| if k.is_empty() { div().w(px(4.)) } else { crate::workbench::key_cap_sized(k, 20., 11.) })).into_any_element()
        };
        // New keys, or the wait begun or ended, fade in.
        let keys_shown = div().flex().items_center().child(keys_shown).with_animation(
            ElementId::Name(format!("shortcut-{id}-{keys}-{waiting}-{}", caps.concat()).into()),
            Animation::new(FOLD_ANIM).with_easing(ease_out_quint()),
            |d, t| d.opacity(t),
        );
        let hover = theme.foreground.opacity(0.06);
        let field = div()
            .flex_shrink_0()
            .h(px(28.))
            .px(px(4.))
            .flex()
            .items_center()
            .rounded(px(7.))
            .border_1()
            .border_color(if waiting { theme.primary } else { transparent_black() })
            .when(waiting, |d| d.bg(theme.primary.opacity(0.08)))
            .when(!waiting, |d| d.group_hover("shortcut-cell", move |s| s.bg(hover)))
            .child(keys_shown);
        let name = match refused {
            Some(said) => div().flex_1().min_w_0().overflow_hidden().text_size(px(13.)).text_color(theme.danger).child(crate::workbench::key_text(&said)),
            None => div().flex_1().min_w_0().truncate().text_size(px(13.)).child(t(c.name)),
        };
        h_flex()
            .id(SharedString::from(format!("shortcut-{id}")))
            .group("shortcut-cell")
            .flex_1()
            .min_w_0()
            .h(px(SHORTCUT_ROW))
            .px(px(14.))
            .gap(px(8.))
            .items_center()
            .cursor_pointer()
            .when(right, |d| d.border_l_1().border_color(theme.border))
            // Its own press is not the press elsewhere that stops a wait.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                this.shortcut_rec = if this.shortcut_rec == Some(id) { None } else { Some(id) };
                this.shortcut_held = window.modifiers();
                this.shortcut_refused = None;
                window.focus(&this.settings_focus, cx);
                cx.notify();
            }))
            .child(name)
            .children(self.shortcut_reset(c, cx))
            .child(field)
            .into_any_element()
    }

    /// Before the keys of a command whose keys were changed: a button
    /// that puts its own back, coming and going in motion as Text Size's
    /// does.
    fn shortcut_reset(&self, c: &'static crate::shortcuts::Command, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = c.id;
        let shown = crate::shortcuts::changed(id) && self.shortcut_rec != Some(id);
        let key = format!("shortcut-reset-{id}");
        let moving = self.presence(&key, shown, cx);
        if !shown && moving.is_none() {
            return None;
        }
        let theme = cx.theme().clone();
        let hover = theme.foreground.opacity(0.07);
        let tip: SharedString = tf("Reset to {keys}", &[("keys", &crate::shortcuts::caps(c.keys).concat())]).into();
        let button = div()
            .id(SharedString::from(key.clone()))
            .flex_shrink_0()
            .size(px(22.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .when(shown, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        cx.stop_propagation();
                        this.set_shortcut(id, None, cx);
                    }))
            })
            .managed_tooltip(move |window, cx| crate::workbench::tip(tip.clone(), window, cx))
            .child(Icon::default().path("icons/rotate.svg").with_size(px(13.)).text_color(theme.muted_foreground));
        Some(match moving {
            Some((coming, n)) => button.with_animation(ElementId::Name(format!("{key}-{n}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
            None => button.into_any_element(),
        })
    }

    /// A key pressed anywhere while a tile waits for its keys, before any
    /// binding hears it (`Workbench::new`): Escape stops the wait, keys
    /// with ⌘ or Ctrl become the command's unless something has them
    /// already, which the tile says for a moment, and any other key is
    /// passed over. Whether the key was the tile's.
    pub(crate) fn shortcut_key(&mut self, k: &Keystroke, window: &Window, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.shortcut_rec else { return false };
        if !self.is_settings_window(window) {
            return false;
        }
        if k.key == "escape" && !k.modifiers.modified() {
            self.shortcut_rec = None;
            cx.notify();
            return true;
        }
        let said = match crate::shortcuts::recorded(k) {
            None => return true,
            Some(Err(needs)) => t(needs).to_string(),
            Some(Ok(keys)) => match crate::shortcuts::conflict(&keys, id, cx) {
                Some(by) => tf("Taken by {name}", &[("name", &t(by))]),
                None => {
                    self.set_shortcut(id, Some(keys), cx);
                    return true;
                }
            },
        };
        self.shortcut_refused = Some((id, said, Instant::now()));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REFUSED_FOR + Duration::from_millis(30)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
        true
    }

    /// A command's keys are these from now on, or its own with none: in
    /// `config.json`, in the window's bindings and menus, and in every
    /// label that names them.
    pub(crate) fn set_shortcut(&mut self, id: &'static str, keys: Option<String>, cx: &mut Context<Self>) {
        let Some(c) = crate::shortcuts::command(id) else { return };
        match keys.filter(|k| !crate::shortcuts::same(k, c.keys)) {
            Some(k) => self.cfg.app.shortcuts.insert(id.to_string(), k),
            None => self.cfg.app.shortcuts.remove(id),
        };
        self.shortcut_rec = None;
        self.shortcut_refused = None;
        crate::shortcuts::set_chosen(&self.cfg.app.shortcuts);
        let chosen = self.cfg.app.shortcuts.clone();
        if let Err(e) = Config::edit(move |c| c.app.shortcuts = chosen) {
            self.notice = Some(Notice::error(tf("could not save settings: {e}", &[("e", &e)])));
        }
        crate::shortcuts::rebind(cx);
        cx.notify();
    }

    /// Files: how a file that shows two ways opens; one switch, and the
    /// languages it formats, in two columns
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
                    .child(div().w(px(84.)).flex_shrink_0().text_size(px(13.)).child(t(l.name)))
                    .child(div().min_w_0().truncate().text_size(px(11.5)).text_color(theme.muted_foreground).child(match needs {
                        Some(_) => tf("by {tool}, not installed", &[("tool", &l.by)]),
                        None => tf("by {tool}", &[("tool", &l.by)]),
                    }))
            });
            v_flex().flex_1().min_w_0().child(div().pb(px(4.)).text_size(px(11.5)).text_color(theme.muted_foreground).child(title)).children(rows.collect::<Vec<_>>())
        };
        let lists = h_flex()
            .p(px(14.))
            .items_start()
            .gap(px(16.))
            .when(!formatting, |d| d.opacity(0.45))
            .child(column(t("Code"), true, cx))
            .child(column(t("Documents and data"), false, cx))
            .into_any_element();
        let raw = self.cfg.app.file_view == "raw";
        let view = div().w(CONTROL_W).child(self.segmented("file-view", vec![("rendered", t("Rendered").into(), None), ("raw", t("Raw").into(), None)], if raw { "raw" } else { "rendered" }, Rc::new(|this, key, _, cx| this.set_file_view(key == "raw", cx)), cx)).into_any_element();
        vec![
            group(None, None, vec![line(t("Open files as"), Some(div().child(t("For supported files.")).into_any_element()), view, &theme)], &theme),
            group(None, None, vec![line(t("Format on save"), Some(div().child(t("A file saved from its pane is put in its language's form.")).into_any_element()), format_on_save, &theme)], &theme),
            group(Some(t("Languages")), None, vec![lists], &theme),
        ]
    }

    /// Updates, at About's foot, as little as possible:
    /// what a check found, and one button that is "Check for updates"
    /// until a check finds something and "Update to x" after, in the
    /// same place, with the new release's notes under the name then.
    fn updates_group(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let u = &self.update;
        let update_line = if u.installing {
            match u.progress {
                Some((done, Some(total))) if total > 0 => tf("Downloading… {percent}%", &[("percent", &(done * 100 / total))]),
                Some(_) => t("Downloading…").to_string(),
                None => t("Installing… Emaki restarts by itself.").to_string(),
            }
        } else if !u.install_error.is_empty() {
            tf("The update did not go through: {e}", &[("e", &u.install_error)])
        } else if !u.error.is_empty() {
            t("Could not reach GitHub.").to_string()
        } else if u.available.is_none() && u.answered && !u.automatic {
            t("Up to date.").to_string()
        } else {
            String::new()
        };
        let can_install = u.available.is_some() && !u.installing && update::asset_name().is_some();
        let (checking, installing) = (u.checking, u.installing);
        let notes_url = u.available.as_deref().map(update::release_page);
        // At least `UPDATE_W`, so the words change in place; a language
        // whose words run longer widens it to them, its right edge where
        // the other controls end.
        let press = |id: &'static str, label: SharedString, kind: Press, off: bool| crate::branches::small_pill(id, "icons/redo-2.svg", label, kind, off, &theme).h(CONTROL_H).min_w(UPDATE_W).justify_center();
        let button = if can_install {
            press("update-now", tf("Update to {version}", &[("version", &u.available.as_deref().unwrap_or(""))]).into(), Press::Primary, false).on_click(cx.listener(|this, _, _, cx| this.install_update_now(cx))).into_any_element()
        } else if installing {
            press("update-busy", t("Updating…").into(), Press::Quiet, true).into_any_element()
        } else {
            press("update-check", if checking { t("Checking…") } else { t("Check for updates") }.into(), Press::Quiet, checking).on_click(cx.listener(|this, _, _, cx| this.check_updates_now(cx))).into_any_element()
        };
        let (link_color, link_hover) = (theme.muted_foreground, theme.primary);
        // The version is on the title card above; the line says only what
        // a check found, and links the new release's notes.
        let has_note = !update_line.is_empty() || notes_url.is_some();
        let version_note = v_flex().gap(px(1.)).when(!update_line.is_empty(), |d| d.child(div().child(update_line))).when_some(notes_url, |d, url| {
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
                    .child(t("Release notes")),
            )
        });
        let auto_check = self.switch("update-auto", self.cfg.app.check_updates, Rc::new(|this, on, _, cx| this.set_check_updates(on, cx)), cx);
        group(None, None, vec![line(t("Updates"), has_note.then(|| version_note.into_any_element()), button, &theme), line(t("Check automatically"), Some(div().child(t("Once a day. Nothing is installed without you.")).into_any_element()), auto_check, &theme)], &theme)
    }

    /// Who makes Emaki: the app on a title card, a sentence naming the
    /// studio and its two owners, each name a link to where they are
    /// found, and the updates last, so the credits stay with the card.
    fn about_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        // A copy from the Microsoft Store is updated by the Store.
        let updates = (!update::from_store()).then(|| self.updates_group(cx));
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
        let mut rows = vec![
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
                .child(crate::i18n::weave(t("{studio} presents"), vec![("studio", link("about-studio", "Alkaline Software", "https://github.com/alkaline-software").into_any_element())], prose().text_size(px(12.5))))
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
                .child(div().mt(px(4.)).text_size(px(12.5)).text_color(theme.muted_foreground).child(tf("Version {version}", &[("version", &current)])))
                .into_any_element(),
            v_flex()
                .w_full()
                .items_center()
                .gap(px(16.))
                .pt(px(4.))
                .child(crate::i18n::weave(t("A studio by {jp} and {pingfan}."), vec![("jp", link("about-jp", "Dr. John Paul Helveston", "https://www.jhelvy.com").into_any_element()), ("pingfan", link("about-pingfan", "Dr. Pingfan Hu", "https://pingfan.org").into_any_element())], prose()))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(go("about-site", "icons/globe.svg", t("Website"), "https://emaki.lol".into()))
                        .child(go("about-source", "icons/github.svg", "GitHub", update::repo_page()))
                        .child(go("about-notes", "icons/file-text.svg", t("Release Notes"), update::release_page(&current))),
                )
                .into_any_element(),
        ];
        // More room over it than the page's gap, so the pills close the
        // credits and the card starts something else.
        rows.extend(updates.map(|u| div().w_full().pt(px(12.)).child(u).into_any_element()));
        rows
    }
}

/// The places text is set in, as Text Size names them.
const SIZE_PLACES: [(&str, &str); 5] = [("conversation", phrase("Conversation")), ("composer", phrase("Composer")), ("preview", phrase("File preview")), ("editor", phrase("File editor")), ("terminal", phrase("Terminal"))];

/// A place's size in the sizes.
fn size_field<'a>(s: &'a mut emaki_core::config::TextSizes, key: &str) -> &'a mut f32 {
    match key {
        "conversation" => &mut s.conversation,
        "composer" => &mut s.composer,
        "preview" => &mut s.preview,
        "editor" => &mut s.editor,
        _ => &mut s.terminal,
    }
}

/// What a size's field takes as it is typed: two digits at most, and a
/// tenth after a point.
fn size_typable(text: &str) -> bool {
    let (whole, tenth) = text.split_once('.').unwrap_or((text, ""));
    whole.len() <= 2 && tenth.len() <= 1 && whole.chars().chain(tenth.chars()).all(|c| c.is_ascii_digit())
}

/// A size as its field says it: a whole point bare, a tenth after it.
fn size_words(v: f32) -> String {
    if v.fract() == 0. { format!("{v:.0}") } else { format!("{v:.1}") }
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
