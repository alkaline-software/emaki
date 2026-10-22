//! Emaki: every coding-agent session on this machine, kept for good, in one
//! window you can also talk to. (The crates and `~/.emaki` keep the old
//! name until the repository moves.)

mod a11y;
mod agents_page;
mod assets;
mod branches;
mod file_icons;
mod fonts;
mod format;
mod html_shot;
mod html_web;
mod word;
mod hub;
mod look;
mod panels;
mod pictures;
mod sys;
mod term_panel;
mod transcript;
mod ui_state;
mod workbench;

use gpui::*;
use gpui_component::Root;
use workbench::{Workbench, COMPOSER_CONTEXT, FIND_CONTEXT, KEY_CONTEXT, SEARCH_CONTEXT, TERMINAL_CONTEXT};

actions!(emaki_app, [Quit, CloseWindow, Hide, HideOthers, ShowAll, Minimize, Zoom, ToggleFullScreen]);

pub use workbench::{CloseTab, Escape, FindInPage, FindNext, FindPrev, SaveFile, GoAgents, GoSessions, NewBranch, NewSession, NewSessionHere, ChooseFolder, OpenSettings, Refresh, ReopenTab, Send, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, Tab9, TermBackTab, TermClear, TermNewTab, TermTab, ToggleAgent, ToggleFiles, ToggleOutline, ToggleShell, ToggleSearch, ToggleSidebar};

fn key_bindings() -> Vec<KeyBinding> {
    let mut keys = vec![
        KeyBinding::new("secondary-k", ToggleSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-r", Refresh, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-n", NewSession, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-t", NewSessionHere, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-o", ChooseFolder, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-n", NewBranch, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-l", GoSessions, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-e", GoAgents, Some(KEY_CONTEXT)),
        // ⌘⇧B, with the panels' keys (⌘⇧S is the shell terminal's), and
        // ⌘B, as an editor has it, named nowhere. The menu names the
        // first binding of an action, so ⌘⇧B stays first.
        KeyBinding::new("secondary-shift-b", ToggleSidebar, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-b", ToggleSidebar, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-f", ToggleFiles, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-o", ToggleOutline, Some(KEY_CONTEXT)),
        // The two sides of the terminal at a conversation's right.
        KeyBinding::new("secondary-shift-s", ToggleShell, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-a", ToggleAgent, Some(KEY_CONTEXT)),
        // The tab closed last comes back, and at each press the one before it.
        KeyBinding::new("secondary-shift-t", ReopenTab, Some(KEY_CONTEXT)),
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
                MenuItem::action("New Session in This Project", NewSessionHere),
                MenuItem::action("Choose Folder…", ChooseFolder),
                MenuItem::action("New Branch…", NewBranch),
                MenuItem::action("Refresh", Refresh),
                MenuItem::separator(),
                MenuItem::action("Save File", SaveFile),
                MenuItem::separator(),
                MenuItem::action("Reopen Closed Tab", ReopenTab),
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
            MenuItem::action("All Sessions", GoSessions),
            MenuItem::action("Agents", GoAgents),
            MenuItem::action("Search", ToggleSearch),
            MenuItem::separator(),
            MenuItem::action("Find in Conversation", FindInPage),
            MenuItem::action("Find Next", FindNext),
            MenuItem::action("Find Previous", FindPrev),
            MenuItem::separator(),
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Files", ToggleFiles),
            MenuItem::action("Toggle Outline", ToggleOutline),
            MenuItem::action("Toggle Shell Terminal", ToggleShell),
            MenuItem::action("Toggle Agent Terminal", ToggleAgent),
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
    let centre = point(b.origin.x + b.size.width / 2., b.origin.y + b.size.height / 2.);
    let display = cx.displays().into_iter().find(|d| d.bounds().contains(&centre))?;
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
        TitlebarOptions { title: None, appears_transparent: true, traffic_light_position: Some(point(px(17.), px(20.))) }
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
            // The window's own close button asks as Quit does when a file
            // has changes not saved.
            let asked = workbench.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                asked
                    .update(cx, |w, cx| {
                        let go = w.file_guard(crate::panels::FileThen::CloseWindow, cx);
                        if go {
                            w.keep_composers(cx);
                            w.close_settings(cx);
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
        // The toolkit's own hover labels (the editor's find) draw their
        // keys as every label here does.
        gpui_component::tooltip::set_label(|text, window, cx| workbench::tip(text, window, cx));
        sys::install_dock_icon();
        // The palette (cream and charcoal, one config per appearance) with
        // the accent from config painted in, then the appearance it asks for.
        let cfg = emaki_core::config::Config::load();
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
