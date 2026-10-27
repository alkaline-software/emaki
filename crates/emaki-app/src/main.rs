//! Emaki: every coding-agent session on this machine, kept for good, in one
//! window you can also talk to. (The crates and `~/.emaki` keep the old
//! name until the repository moves.)

// No console window beside the app on Windows. A debug build keeps one
// for its log; children are started without one (`emaki_core::process`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod a11y;
mod agents_page;
mod ambient;
mod sublime;
mod assets;
mod branches;
mod csd;
mod fetch;
mod file_icons;
mod fonts;
mod format;
mod html_shot;
mod html_web;
mod word;
mod hub;
mod i18n;
mod look;
mod panels;
mod pictures;
mod shortcuts;
mod spelling;
mod sys;
mod term_panel;
mod transcript;
mod ui_state;
mod workbench;

use gpui::*;
use gpui_component::Root;
use i18n::t;
use workbench::{Workbench, COMPOSER_CONTEXT, FIND_CONTEXT, KEY_CONTEXT, SEARCH_CONTEXT, TERMINAL_CONTEXT};

actions!(emaki_app, [Quit, CloseWindow, Hide, HideOthers, ShowAll, Minimize, Zoom, ToggleFullScreen]);

pub use workbench::{ToggleChanges, CloseTab, Escape, FindInPage, FindNext, FindPrev, SaveFile, GoAgents, GoSessions, NewBranch, NewSession, NewSessionHere, ChooseFolder, OpenSettings, Refresh, ReopenTab, ToggleRaw, Send, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, Tab9, TermBackTab, TermClear, TermNewTab, TermTab, ToggleAgent, ToggleFiles, ToggleOutline, ToggleShell, ToggleSearch, ToggleSidebar};

fn key_bindings() -> Vec<KeyBinding> {
    // The commands whose keys Settings, Shortcuts, changes, first: a
    // binding of the same keys after them is asked first (⌘⇧G's Find
    // Previous, which passes it on to Changes and History while no find
    // is open). The menu names the first binding of an action, so the
    // sidebar's ⌘⇧B stands before its ⌘B.
    let mut keys = shortcuts::bindings();
    keys.extend([
        KeyBinding::new("secondary-1", Tab1, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-2", Tab2, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-3", Tab3, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-4", Tab4, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-5", Tab5, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-6", Tab6, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-7", Tab7, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-8", Tab8, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-9", Tab9, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(SEARCH_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(FIND_CONTEXT)),
        KeyBinding::new("escape", Escape, Some(workbench::SETTINGS_CONTEXT)),
        // Find inside the conversation showing; ⌘G and ⌘⇧G step through
        // the hits from anywhere, Enter and ⇧Enter from the find field.
        KeyBinding::new("secondary-f", FindInPage, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-g", FindNext, Some(KEY_CONTEXT)),
        // The file's pane: what was changed in its editor goes to the file.
        KeyBinding::new("secondary-s", SaveFile, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-g", FindPrev, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-enter", Send, Some(COMPOSER_CONTEXT)),
        // Outside the composer too, for the new-session page with nothing
        // typed (`send_pressed`). A field of its own keeps its own ⌘↩.
        KeyBinding::new("secondary-enter", Send, Some(KEY_CONTEXT)),
        // On the terminal card Tab is Claude Code's, not the toolkit's
        // move to the next field.
        KeyBinding::new("tab", TermTab, Some(TERMINAL_CONTEXT)),
        KeyBinding::new("shift-tab", TermBackTab, Some(TERMINAL_CONTEXT)),
        // The terminal panel's own: where it does not take one (the
        // agent's side has no tabs) the window's binding is next. ⌘W is
        // not here: the panel answers the global `CloseTab` below itself,
        // since a global binding outranks one bound to a context.
        KeyBinding::new("secondary-k", TermClear, Some(TERMINAL_CONTEXT)),
        KeyBinding::new("secondary-t", TermNewTab, Some(TERMINAL_CONTEXT)),
        KeyBinding::new("secondary-n", TermNewTab, Some(TERMINAL_CONTEXT)),
        KeyBinding::new("secondary-q", Quit, None),
        // ⌘, on macOS, Ctrl+, elsewhere; Win+, as well where there is a
        // Win key, though Windows itself may take it first (desktop peek).
        KeyBinding::new("secondary-,", OpenSettings, None),
        // Closes the session tab when one is showing, otherwise the window.
        // One global binding: a context-bound one loses to a global one
        // whenever the focus sits deeper than the workbench, in the composer.
        KeyBinding::new("secondary-w", CloseTab, None),
    ]);
    if !cfg!(target_os = "macos") {
        keys.push(KeyBinding::new("win-,", OpenSettings, None));
        // Off a Mac the window's keys are Ctrl's, and Ctrl with a letter in
        // a terminal is the shell's and the agent's (Ctrl+R, Ctrl+W,
        // Ctrl+L, Ctrl+E): there none of the window's applies, and the
        // panel's own take Ctrl+Shift, as Windows Terminal and GNOME's
        // terminal have them. Bound after the global ones, which these
        // have to outrank at the same depth.
        keys.extend(('a'..='z').map(|c| KeyBinding::new(&format!("ctrl-{c}"), NoAction, Some(TERMINAL_CONTEXT))));
        keys.extend([
            KeyBinding::new("ctrl-enter", NoAction, Some(TERMINAL_CONTEXT)),
            KeyBinding::new("ctrl-shift-k", TermClear, Some(TERMINAL_CONTEXT)),
            KeyBinding::new("ctrl-shift-t", TermNewTab, Some(TERMINAL_CONTEXT)),
            KeyBinding::new("ctrl-shift-w", CloseTab, Some(TERMINAL_CONTEXT)),
        ]);
    }
    if cfg!(target_os = "macos") {
        keys.extend([
            KeyBinding::new("secondary-h", Hide, None),
            KeyBinding::new("secondary-alt-h", HideOthers, None),
            KeyBinding::new("secondary-m", Minimize, None),
            KeyBinding::new("ctrl-secondary-f", ToggleFullScreen, None),
        ]);
    }
    // Sublime Text's, while the keyboard is in the file's editor.
    keys.extend(sublime::bindings());
    keys
}

/// The menus, in the window's language: made again when it changes
/// (`Workbench::set_language`).
pub(crate) fn app_menus() -> Vec<Menu> {
    let mac = cfg!(target_os = "macos");
    let mut app_items = vec![MenuItem::action(t("Settings…"), OpenSettings), MenuItem::separator()];
    if mac {
        app_items.extend([
            MenuItem::action(t("Hide Emaki"), Hide),
            MenuItem::action(t("Hide Others"), HideOthers),
            MenuItem::action(t("Show All"), ShowAll),
            MenuItem::separator(),
        ]);
    }
    app_items.push(MenuItem::action(t("Quit Emaki"), Quit));
    let mut menus = vec![
        Menu { name: "Emaki".into(), items: app_items, disabled: false },
        Menu {
            name: t("File").into(),
            disabled: false,
            items: vec![
                MenuItem::action(t("New Session"), NewSession),
                MenuItem::action(t("New Session in This Project"), NewSessionHere),
                MenuItem::action(t("Choose Folder…"), ChooseFolder),
                MenuItem::action(t("New Branch…"), NewBranch),
                MenuItem::action(t("Refresh"), Refresh),
                MenuItem::separator(),
                MenuItem::action(t("Save File"), SaveFile),
                MenuItem::separator(),
                MenuItem::action(t("Reopen Closed Tab"), ReopenTab),
                MenuItem::action(t("Close Window"), CloseWindow),
            ],
        },
    ];
    if mac {
        menus.push(Menu {
            name: t("Edit").into(),
            disabled: false,
            items: vec![
                MenuItem::os_action(t("Undo"), gpui_component::input::Undo, OsAction::Undo),
                MenuItem::os_action(t("Redo"), gpui_component::input::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action(t("Cut"), gpui_component::input::Cut, OsAction::Cut),
                MenuItem::os_action(t("Copy"), gpui_component::input::Copy, OsAction::Copy),
                MenuItem::os_action(t("Paste"), gpui_component::input::Paste, OsAction::Paste),
                MenuItem::os_action(t("Select All"), gpui_component::input::SelectAll, OsAction::SelectAll),
            ],
        });
    }
    menus.push(Menu {
        name: t("View").into(),
        disabled: false,
        items: vec![
            MenuItem::action(t("All Sessions"), GoSessions),
            MenuItem::action(t("Agents"), GoAgents),
            MenuItem::action(t("Search"), ToggleSearch),
            MenuItem::separator(),
            MenuItem::action(t("Find in Conversation"), FindInPage),
            MenuItem::action(t("Find Next"), FindNext),
            MenuItem::action(t("Find Previous"), FindPrev),
            MenuItem::separator(),
            MenuItem::action(t("Toggle Sidebar"), ToggleSidebar),
            MenuItem::action(t("Toggle Files"), ToggleFiles),
            MenuItem::action(t("Toggle Outline"), ToggleOutline),
            MenuItem::action(t("Toggle Rendered and Raw"), ToggleRaw),
            MenuItem::action(t("Toggle Shell Terminal"), ToggleShell),
            MenuItem::action(t("Toggle CLI Layout"), ToggleAgent),
        ],
    });
    menus.push(Menu {
        name: t("Window").into(),
        disabled: false,
        items: vec![MenuItem::action(t("Minimize"), Minimize), MenuItem::action(t("Zoom"), Zoom), MenuItem::action(t("Toggle Full Screen"), ToggleFullScreen)],
    });
    menus
}

fn with_active_window(cx: &mut App, f: impl FnOnce(&mut Window) + 'static) {
    let Some(w) = cx.active_window() else { return };
    cx.defer(move |cx| {
        w.update(cx, |_, window, _| f(window)).ok();
    });
}

/// The main window and its workbench, for an action that reached no view.
struct MainWorkbench(WeakEntity<Workbench>, AnyWindowHandle);

impl Global for MainWorkbench {}

/// Run `f` on the workbench of the active window, or with none there
/// close that window, which is what the action asked of a window bare.
/// The settings window is one with none: what reaches it bare closes it.
fn with_workbench(cx: &mut App, f: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static) {
    let Some(w) = cx.active_window() else { return };
    let workbench = cx.try_global::<MainWorkbench>().filter(|m| m.1 == w).and_then(|m| m.0.upgrade());
    cx.defer(move |cx| {
        w.update(cx, |_, window, cx| match workbench {
            Some(workbench) => workbench.update(cx, |w, cx| f(w, window, cx)),
            None => window.remove_window(),
        })
        .ok();
    });
}

/// Where the window was last time, if that spot is still on a screen.
/// The least the window is dragged to: the conversation at its least
/// with one pane beside it, the wider of the two there are, so a pane
/// asked for always has room, and the frame at both sides of them.
pub(crate) const MIN_W: Pixels = px(794.);

fn remembered_bounds(cx: &App) -> Option<Bounds<Pixels>> {
    let r = ui_state::UiState::load().window?;
    let b = Bounds { origin: point(px(r.x), px(r.y)), size: size(px(r.w).max(MIN_W), px(r.h.max(480.))) };
    let center = point(b.origin.x + b.size.width / 2., b.origin.y + b.size.height / 2.);
    let display = cx.displays().into_iter().find(|d| d.bounds().contains(&center))?;
    Some(filled(b, display.visible_bounds()))
}

/// A window left a point or two short of an edge of the screen's usable
/// area reaches that edge. Whatever fills the screen with a window (a
/// tiling key, a window manager) can leave it that much short, and the
/// window behind it then shows as a line along that side: a white line
/// under a dark frame.
fn filled(b: Bounds<Pixels>, area: Bounds<Pixels>) -> Bounds<Pixels> {
    let near = |a: Pixels, to: Pixels| a != to && (a - to).abs() <= px(2.);
    let (mut left, mut top, mut right, mut bottom) = (b.left(), b.top(), b.right(), b.bottom());
    if near(left, area.left()) {
        left = area.left();
    }
    if near(top, area.top()) {
        top = area.top();
    }
    if near(right, area.right()) {
        right = area.right();
    }
    if near(bottom, area.bottom()) {
        bottom = area.bottom();
    }
    Bounds::from_corners(point(left, top), point(right, bottom))
}

fn open_main_window(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let bounds = remembered_bounds(cx).unwrap_or_else(|| Bounds::centered(None, size(px(1180.), px(760.)), cx));
    let titlebar = if cfg!(target_os = "macos") {
        TitlebarOptions { title: None, appears_transparent: true, traffic_light_position: Some(point(px(sys::LIGHTS_ROW.0), px(sys::LIGHTS_ROW.1))) }
    } else {
        TitlebarOptions { title: Some("Emaki".into()), appears_transparent: false, traffic_light_position: None }
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(titlebar),
            // Narrow enough for the sidebar to fold away (see NARROW_W) and
            // the conversation to run edge to edge, like the Claude app.
            window_min_size: Some(size(MIN_W, px(480.))),
            // The Linux packages' desktop file is `Emaki.desktop`: the shell
            // finds the window's icon by this name, else a blank one.
            app_id: Some("Emaki".into()),
            // The top strip holds tabs, which are dragged along it, so the
            // app says which presses move the window (`drag_region`).
            app_owns_titlebar_drag: true,
            ..Default::default()
        },
        |window, cx| {
            // The palette follows the system only when the settings say so;
            // a pinned appearance is re-asserted on every system change.
            window
                .observe_window_appearance(|window, cx| {
                    let appearance = emaki_core::config::Config::load().app.appearance;
                    look::apply(&appearance, Some(window), cx);
                })
                .detach();
            look::apply(&emaki_core::config::Config::load().app.appearance, Some(window), cx);
            a11y::install_window_focus_forwarder(window);
            let workbench = cx.new(|cx| Workbench::new(window, cx));
            window.focus(&workbench.read(cx).focus_handle(cx), cx);
            // The window's own close button asks as Quit does when a file
            // has changes not saved.
            let asked = workbench.downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                asked
                    .update(cx, |w, cx| {
                        let go = w.file_guard(crate::panels::FileThen::CloseWindow, cx);
                        if go {
                            w.keep_composers(cx);
                            w.close_settings(cx);
                            w.save_ui_closing(window);
                        }
                        go
                    })
                    .unwrap_or(true)
            });
            cx.set_global(MainWorkbench(workbench.downgrade(), window.window_handle()));
            cx.new(|cx| Root::new(workbench, window, cx))
        },
    )
}

fn main() {
    // Velopack's installer and updater start the app with a hook to answer,
    // and it leaves at once. What NSIS's installer left goes once
    // Velopack's copy runs.
    #[cfg(windows)]
    {
        velopack::VelopackApp::build().run();
        emaki_core::update::drop_nsis_install();
    }
    // Started from the Dock there is no shell above us, and the PATH is
    // the system's: the person's own is asked of their shell, before any
    // thread or child is started, so that everything Claude Code runs
    // finds what their terminal finds.
    if std::env::var_os("SHLVL").is_none() {
        if let Some(path) = emaki_core::driver::login_shell_path() {
            std::env::set_var("PATH", path);
        }
    }
    let app = gpui_platform::application().with_assets(assets::Assets);
    // A click on the Dock icon, or a second launch, after the window was
    // closed (⌘W on the last tab): open it again, or bring it forward.
    // The settings window alone is no main window.
    app.on_reopen(|cx| {
        let main = cx.try_global::<MainWorkbench>().map(|m| m.1);
        if !main.is_some_and(|h| cx.windows().contains(&h)) {
            if let Err(e) = open_main_window(cx) {
                eprintln!("emaki: could not open a window: {e}");
            }
        }
        cx.activate(true);
    });
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.set_http_client(std::sync::Arc::new(fetch::Fetcher));
        // The toolkit's own hover labels (the editor's find) draw their
        // keys as every label here does.
        gpui_component::tooltip::set_label(|text, window, cx| workbench::tip(text, window, cx));
        sys::install_dock_icon();
        // The palette (cream and charcoal, one config per appearance) with
        // the accent from config painted in, then the appearance it asks for.
        let cfg = emaki_core::config::Config::load();
        // The words first: the menus and the window are named in them.
        i18n::set(&i18n::chosen(&cfg.app.language));
        shortcuts::set_chosen(&cfg.app.shortcuts);
        let app_cfg = cfg.app;
        fonts::install(&app_cfg.chat_font, cx);
        look::install(&app_cfg.accent, &cfg.terminal.font, cx);
        look::install_languages();
        look::apply(&app_cfg.appearance, None, cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        // Close Window and ⌘W that no view heard: the focus is on nothing
        // the page draws, so the key came from the window's root, above
        // the workbench. They are handed to it all the same. Closing the
        // window here closed it with tabs open, whatever was not saved.
        cx.on_action(|_: &CloseWindow, cx| with_workbench(cx, |w, window, cx| w.close_window(window, cx)));
        cx.on_action(|_: &CloseTab, cx| with_workbench(cx, |w, window, cx| w.close_nearest(window, cx)));
        cx.on_action(|_: &Minimize, cx| with_active_window(cx, |w| w.minimize_window()));
        cx.on_action(|_: &Zoom, cx| with_active_window(cx, |w| w.zoom_window()));
        cx.on_action(|_: &ToggleFullScreen, cx| with_active_window(cx, |w| w.toggle_fullscreen()));
        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());

        cx.bind_keys(key_bindings());
        word::bind_keys(cx);
        cx.set_menus(app_menus());

        if let Err(e) = open_main_window(cx) {
            eprintln!("emaki: could not open a window: {e}");
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
