//! The clock that everything moving while something is at work runs on:
//! the agent's mark turning, the dots of commands and subagents breathing, an
//! outline row waiting for its label, a spinner. Each is drawn where the
//! clock stands (`phase`), and while one was drawn the window is drawn
//! again a step later (`Ambient`, `after_frame`): about 30 steps a second,
//! 20 while another app is in use.
//!
//! An animation that repeats (`Animation::repeat`) asks for every frame the
//! display has, and the window is drawn whole at each: a mark of 14 points
//! turning held the window at 60 frames a second for as long as an agent
//! worked, a fifth of a core and as much again of the window server's.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use emaki_core::model::AgentId;
use gpui::{AnyElement, App, Bounds, ContentMask, Context, Entity, Hsla, IntoElement, ParentElement as _, Pixels, Render, Styled as _, Window, canvas, div};
use gpui_component::text::KeptViews;

thread_local! {
    static EPOCH: Instant = Instant::now();
    static WANTED: Cell<bool> = const { Cell::new(false) };
}

/// The wait for the next step, while the window is in use and while it
/// is not: the frame lands on the display's next refresh after it, about
/// 33 ms and 50 ms apart at 60 Hz.
const STEP: Duration = Duration::from_millis(28);
const STEP_AWAY: Duration = Duration::from_millis(45);

/// Where a motion `period` long stands now, from 0 to 1, and a note that
/// something on the clock was drawn in this frame.
pub fn phase(period: Duration) -> f32 {
    WANTED.with(|w| w.set(true));
    let p = period.as_secs_f64();
    (EPOCH.with(|e| e.elapsed().as_secs_f64()) % p / p) as f32
}

/// gpui's `pulsating_between`, a breath from `min` to `max` and back, at
/// the clock's phase `t`.
pub fn pulse(min: f32, max: f32, t: f32) -> f32 {
    let s = (t * std::f32::consts::TAU).sin();
    let breath = (s * s * s + s) / 2.;
    min + (breath + 1.) / 2. * (max - min)
}

/// The window's step on the clock. A view of no size, so its notify draws
/// the window again without telling the workbench: the views the window
/// keeps between frames are drawn afresh when the workbench changes, and a
/// step changes nothing in them.
#[derive(Default)]
pub struct Ambient {
    due: bool,
}

impl Render for Ambient {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A frame, for what the workbench draws afresh at every frame anyway
/// (the terminal panel's screen): asked of the clock's view, so what the
/// window keeps between frames is not drawn again with it.
pub fn redraw(ambient: &Entity<Ambient>, cx: &mut App) {
    ambient.update(cx, |_, cx| cx.notify());
}

/// At the end of a frame, from the last thing it paints: when something
/// on the clock was drawn in it, the next step is asked for.
pub fn after_frame(ambient: &Entity<Ambient>, window: &Window, cx: &mut App) {
    if !WANTED.with(|w| w.replace(false)) {
        return;
    }
    let step = if window.is_window_active() { STEP } else { STEP_AWAY };
    ambient.update(cx, |a, cx| {
        if a.due {
            return;
        }
        a.due = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(step).await;
            let _ = this.update(cx, |a, cx| {
                a.due = false;
                cx.notify();
            });
        })
        .detach();
    });
}

/// The marks at work in a view the window keeps between frames, where its
/// last drawing put them, each with the clip it was drawn under: painted
/// after the view at every frame, on the clock (`paint`), so a step of the
/// clock does not draw the view again. A view drawn afresh at every step
/// for one mark that turns in it saved nothing by being kept.
#[derive(Clone, Default)]
pub struct Marks(Rc<RefCell<Vec<(AgentId, Hsla, Bounds<Pixels>, Bounds<Pixels>)>>>);

impl Marks {
    /// Before the view is drawn again: its marks are where that drawing
    /// puts them.
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub fn paint(&self, window: &mut Window, cx: &mut App) {
        for (agent, color, bounds, clip) in self.0.borrow().iter() {
            window.with_content_mask(Some(ContentMask { bounds: *clip }), |window| crate::workbench::paint_agent_glyph(*agent, *color, *bounds, window, cx));
        }
    }
}

thread_local! {
    static WITHIN: RefCell<Option<(Option<KeptViews>, Marks)>> = const { RefCell::new(None) };
}

/// While a kept view's content is made: where its text views are kept
/// and its marks at work go.
pub struct Within(Option<(Option<KeptViews>, Marks)>);

impl Within {
    pub fn enter(kept: Option<KeptViews>, marks: Marks) -> Self {
        Self(WITHIN.with(|w| w.borrow_mut().replace((kept, marks))))
    }
}

impl Drop for Within {
    fn drop(&mut self) {
        WITHIN.with(|w| *w.borrow_mut() = self.0.take());
    }
}

/// Where a text view made now is kept, in a kept view.
pub fn kept_here() -> Option<KeptViews> {
    WITHIN.with(|w| w.borrow().as_ref().and_then(|(k, _)| k.clone()))
}

/// In a kept view, a place for a mark at work of `size`, which the view's
/// marks paint; elsewhere None.
pub fn mark_place(agent: AgentId, size: Pixels, color: Hsla) -> Option<AnyElement> {
    let marks = WITHIN.with(|w| w.borrow().as_ref().map(|(_, m)| m.clone()))?;
    Some(
        div()
            .size(size)
            .flex_shrink_0()
            .child(canvas(move |bounds, window, _| marks.0.borrow_mut().push((agent, color, bounds, window.content_mask().bounds)), |_, _, _, _| {}).size_full())
            .into_any_element(),
    )
}
