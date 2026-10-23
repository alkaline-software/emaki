//! The window's own buttons and edges where the system draws neither: a
//! window gpui says decorates itself (`Decorations::Client`), as on
//! GNOME under Wayland, which draws no title bar for an app. Without
//! these the window had no way to be closed, made small or large, or
//! resized by its edge. A window the system decorates (a Mac, Windows,
//! X11, KDE) gets nothing from here.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

/// One button's width, and the gap between two.
const BUTTON: Pixels = px(24.);
const GAP: Pixels = px(8.);
/// How far in from the window's edge a press resizes it.
const EDGE: Pixels = px(6.);

/// Whether this window draws its own buttons and edges.
pub(crate) fn client_side(window: &Window) -> bool {
    matches!(window.window_decorations(), Decorations::Client { .. })
}

/// The buttons the system asks for at each end of the title bar, in its
/// order (GNOME's `button-layout`), less those its window manager does
/// not honor.
fn layout(window: &Window, cx: &App) -> (Vec<WindowButton>, Vec<WindowButton>) {
    let can = window.window_controls();
    let honored = |b: &WindowButton| match b {
        WindowButton::Minimize => can.minimize,
        WindowButton::Maximize => can.maximize,
        WindowButton::Close => true,
    };
    match cx.button_layout() {
        Some(l) => (l.left.iter().flatten().copied().filter(honored).collect(), l.right.iter().flatten().copied().filter(honored).collect()),
        None => (Vec::new(), [WindowButton::Minimize, WindowButton::Maximize, WindowButton::Close].into_iter().filter(honored).collect()),
    }
}

/// The room the buttons take at the strip's left and right ends, none
/// where the window is decorated by the system.
pub(crate) fn room(window: &Window, cx: &App) -> (Pixels, Pixels) {
    if !client_side(window) {
        return (px(0.), px(0.));
    }
    let width = |n: usize| if n == 0 { px(0.) } else { BUTTON * n as f32 + GAP * n as f32 };
    let (left, right) = layout(window, cx);
    (width(left.len()), width(right.len()))
}

/// The buttons at one end, laid over the window's top corner there, and
/// the edges the window is resized by. `close` is the window's own close,
/// which asks about what is not saved.
pub(crate) fn chrome(window: &Window, top: Pixels, close: impl Fn(&mut Window, &mut App) + 'static, cx: &App) -> Option<AnyElement> {
    if !client_side(window) {
        return None;
    }
    let theme = cx.theme().clone();
    let close = std::rc::Rc::new(close);
    let (left, right) = layout(window, cx);
    let maximized = window.is_maximized();
    let button = |b: WindowButton| {
        let icon = match b {
            WindowButton::Minimize => IconName::WindowMinimize,
            WindowButton::Maximize if maximized => IconName::WindowRestore,
            WindowButton::Maximize => IconName::WindowMaximize,
            WindowButton::Close => IconName::WindowClose,
        };
        let close = close.clone();
        div()
            .id(b.id())
            .size(BUTTON)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(theme.muted.opacity(0.6))
            .hover(|s| s.bg(theme.muted))
            .text_color(theme.foreground)
            // The strip under it moves the window at a press: this press
            // is the button's.
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(move |_, window, cx| {
                crate::workbench::swallow_click(window, cx);
                match b {
                    WindowButton::Minimize => window.minimize_window(),
                    WindowButton::Maximize => window.zoom_window(),
                    WindowButton::Close => close(window, cx),
                }
            })
            .child(Icon::new(icon).with_size(px(12.)))
    };
    let end = |buttons: Vec<WindowButton>| h_flex().gap(GAP).items_center().children(buttons.into_iter().map(button));
    let Decorations::Client { tiling } = window.window_decorations() else { return None };
    // A press at an edge or a corner resizes the window, unless that edge
    // stands against a tiled neighbor or the window fills the screen.
    let edge = |e: ResizeEdge, cursor: CursorStyle, at: Div| {
        at.absolute().cursor(cursor).on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            window.start_window_resize(e);
        })
    };
    let free = !maximized;
    let (t, b, l, r) = (free && !tiling.top, free && !tiling.bottom, free && !tiling.left, free && !tiling.right);
    Some(
        div()
            .absolute()
            .inset_0()
            .when(t, |d| d.child(edge(ResizeEdge::Top, CursorStyle::ResizeUpDown, div().top_0().left(EDGE).right(EDGE).h(EDGE))))
            .when(b, |d| d.child(edge(ResizeEdge::Bottom, CursorStyle::ResizeUpDown, div().bottom_0().left(EDGE).right(EDGE).h(EDGE))))
            .when(l, |d| d.child(edge(ResizeEdge::Left, CursorStyle::ResizeLeftRight, div().left_0().top(EDGE).bottom(EDGE).w(EDGE))))
            .when(r, |d| d.child(edge(ResizeEdge::Right, CursorStyle::ResizeLeftRight, div().right_0().top(EDGE).bottom(EDGE).w(EDGE))))
            .when(t && l, |d| d.child(edge(ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight, div().top_0().left_0().size(EDGE * 2.))))
            .when(t && r, |d| d.child(edge(ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft, div().top_0().right_0().size(EDGE * 2.))))
            .when(b && l, |d| d.child(edge(ResizeEdge::BottomLeft, CursorStyle::ResizeUpRightDownLeft, div().bottom_0().left_0().size(EDGE * 2.))))
            .when(b && r, |d| d.child(edge(ResizeEdge::BottomRight, CursorStyle::ResizeUpLeftDownRight, div().bottom_0().right_0().size(EDGE * 2.))))
            .when(!left.is_empty(), |d| d.child(end(left).absolute().top(top).left(px(14.))))
            .when(!right.is_empty(), |d| d.child(end(right).absolute().top(top).right(px(14.))))
            .into_any_element(),
    )
}
