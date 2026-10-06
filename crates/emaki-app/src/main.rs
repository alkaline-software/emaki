//! Emaki: every coding-agent session on this machine, kept for good, in one
//! window you can also talk to. (The crates and `~/.emaki` keep the old
//! name until the repository moves.)

mod a11y;
mod assets;
mod file_icons;
mod fonts;
mod format;
mod hub;
mod look;
mod panels;
mod sys;
mod transcript;
mod ui_state;
mod workbench;

use gpui::*;
use gpui_component::Root;
use workbench::{Workbench, COMPOSER_CONTEXT, FIND_CONTEXT, KEY_CONTEXT, SEARCH_CONTEXT, TERMINAL_CONTEXT};

actions!(emaki_app, [Quit, CloseWindow, Hide, HideOthers, ShowAll, Minimize, Zoom, ToggleFullScreen]);

pub use workbench::{CloseTab, Escape, FindInPage, FindNext, FindPrev, GoBoard, GoSessions, NewSession, OpenSettings, Refresh, Send, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, Tab9, TermBackTab, TermTab, ToggleFiles, ToggleOutline, ToggleSearch, ToggleSidebar};

fn key_bindings() -> Vec<KeyBinding> {
    let mut keys = vec![
        KeyBinding::new("secondary-k", ToggleSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-r", Refresh, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-n", NewSession, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-b", GoBoard, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-l", GoSessions, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-s", ToggleSidebar, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-e", ToggleFiles, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-o", ToggleOutline, Some(KEY_CONTEXT)),
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
        // Find inside the conversation showing; ⌘G and ⌘⇧G step through
        // the hits from anywhere, Enter and ⇧Enter from the find field.
        KeyBinding::new("secondary-f", FindInPage, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-g", FindNext, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-g", FindPrev, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-enter", Send, Some(COMPOSER_CONTEXT)),
        // On the terminal card Tab is Claude Code's, not the toolkit's
        // move to the next field.
        KeyBinding::new("tab", TermTab, Some(TERMINAL_CONTEXT)),
        KeyBinding::new("shift-tab", TermBackTab, Some(TERMINAL_CONTEXT)),
        KeyBinding::new("secondary-q", Quit, None),
        // ⌘, on macOS, Ctrl+, elsewhere; Win+, as well where there is a
        // Win key, though Windows itself may take it first (desktop peek).
        KeyBinding::new("secondary-,", OpenSettings, None),
        // Closes the session tab when one is showing, otherwise the window.
        // One global binding: a context-bound one loses to a global one
        // whenever the focus sits deeper than the workbench, in the composer.
        KeyBinding::new("secondary-w", CloseTab, None),
    ];
    if !cfg!(target_os = "macos") {
        keys.push(KeyBinding::new("win-,", OpenSettings, None));
    }
    if cfg!(target_os = "macos") {
        keys.extend([
            KeyBinding::new("secondary-h", Hide, None),
            KeyBinding::new("secondary-alt-h", HideOthers, None),
            KeyBinding::new("secondary-m", Minimize, None),
            KeyBinding::new("ctrl-secondary-f", ToggleFullScreen, None),
        ]);
    }
    keys
}

fn app_menus() -> Vec<Menu> {
    let mac = cfg!(target_os = "macos");
    let mut app_items = vec![MenuItem::action("Settings…", OpenSettings), MenuItem::separator()];
    if mac {
        app_items.extend([
            MenuItem::action("Hide Emaki", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
        ]);
    }
    app_items.push(MenuItem::action("Quit Emaki", Quit));
    let mut menus = vec![
        Menu { name: "Emaki".into(), items: app_items, disabled: false },
        Menu {
            name: "File".into(),
            disabled: false,
            items: vec![
                MenuItem::action("New Session", NewSession),
                MenuItem::action("Refresh", Refresh),
                MenuItem::separator(),
                MenuItem::action("Close Window", CloseWindow),
            ],
        },
    ];
    if mac {
        menus.push(Menu {
            name: "Edit".into(),
            disabled: false,
            items: vec![
                MenuItem::os_action("Undo", gpui_component::input::Undo, OsAction::Undo),
                MenuItem::os_action("Redo", gpui_component::input::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", gpui_component::input::Cut, OsAction::Cut),
                MenuItem::os_action("Copy", gpui_component::input::Copy, OsAction::Copy),
                MenuItem::os_action("Paste", gpui_component::input::Paste, OsAction::Paste),
                MenuItem::os_action("Select All", gpui_component::input::SelectAll, OsAction::SelectAll),
            ],
        });
    }
    menus.push(Menu {
        name: "View".into(),
        disabled: false,
        items: vec![
            MenuItem::action("Board", GoBoard),
            MenuItem::action("Projects", GoSessions),
            MenuItem::action("Search", ToggleSearch),
            MenuItem::separator(),
            MenuItem::action("Find in Conversation", FindInPage),
            MenuItem::action("Find Next", FindNext),
            MenuItem::action("Find Previous", FindPrev),
            MenuItem::separator(),
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Files", ToggleFiles),
            MenuItem::action("Toggle Outline", ToggleOutline),
        ],
    });
    menus.push(Menu {
        name: "Window".into(),
        disabled: false,
        items: vec![MenuItem::action("Minimize", Minimize), MenuItem::action("Zoom", Zoom), MenuItem::action("Toggle Full Screen", ToggleFullScreen)],
    });
    menus
}

fn with_active_window(cx: &mut App, f: impl FnOnce(&mut Window) + 'static) {
    let Some(w) = cx.active_window() else { return };
    cx.defer(move |cx| {
        w.update(cx, |_, window, _| f(window)).ok();
    });
}

/// Where the window was last time, if that spot is still on a screen.
fn remembered_bounds(cx: &App) -> Option<Bounds<Pixels>> {
    let r = ui_state::UiState::load().window?;
    let b = Bounds { origin: point(px(r.x), px(r.y)), size: size(px(r.w.max(560.)), px(r.h.max(480.))) };
    let centre = point(b.origin.x + b.size.width / 2., b.origin.y + b.size.height / 2.);
    cx.displays().iter().any(|d| d.bounds().contains(&centre)).then_some(b)
}

fn open_main_window(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let bounds = remembered_bounds(cx).unwrap_or_else(|| Bounds::centered(None, size(px(1180.), px(760.)), cx));
    let titlebar = if cfg!(target_os = "macos") {
        TitlebarOptions { title: None, appears_transparent: true, traffic_light_position: Some(point(px(17.), px(18.))) }
    } else {
        TitlebarOptions { title: Some("Emaki".into()), appears_transparent: false, traffic_light_position: None }
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(titlebar),
            // Narrow enough for the sidebar to fold away (see NARROW_W) and
            // the conversation to run edge to edge, like the Claude app.
            window_min_size: Some(size(px(560.), px(480.))),
            app_id: Some("emaki".into()),
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
            cx.new(|cx| Root::new(workbench, window, cx))
        },
    )
}

fn main() {
    let app = gpui_platform::application().with_assets(assets::Assets);
    // A click on the Dock icon, or a second launch, after the window was
    // closed (⌘W on the last tab): open it again, or bring it forward.
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            if let Err(e) = open_main_window(cx) {
                eprintln!("emaki: could not open a window: {e}");
            }
        }
        cx.activate(true);
    });
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        sys::install_dock_icon();
        fonts::install(cx);
        // The palette (cream and charcoal, one config per appearance) with
        // the accent from config painted in, then the appearance it asks for.
        let app_cfg = emaki_core::config::Config::load().app;
        look::install(&app_cfg.accent, cx);
        look::apply(&app_cfg.appearance, None, cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &CloseWindow, cx| with_active_window(cx, |w| w.remove_window()));
        // ⌘W that no view claimed (nothing focused, or nothing showing that
        // ⌘W could close) closes the window, as it does in any app.
        cx.on_action(|_: &CloseTab, cx| with_active_window(cx, |w| w.remove_window()));
        cx.on_action(|_: &Minimize, cx| with_active_window(cx, |w| w.minimize_window()));
        cx.on_action(|_: &Zoom, cx| with_active_window(cx, |w| w.zoom_window()));
        cx.on_action(|_: &ToggleFullScreen, cx| with_active_window(cx, |w| w.toggle_fullscreen()));
        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());

        cx.bind_keys(key_bindings());
        cx.set_menus(app_menus());

        if let Err(e) = open_main_window(cx) {
            eprintln!("emaki: could not open a window: {e}");
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
