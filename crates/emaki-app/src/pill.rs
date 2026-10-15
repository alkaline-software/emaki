//! Dictation shown at the top of the screen when its words go somewhere
//! other than the composer: another app, or another field of Emaki's. A
//! black pill hangs from the screen's top edge; on a Mac with a notch it
//! wraps the notch, the wave on the menu bar at its left and the voice at
//! its right. It is a window of its own that never takes the keyboard or
//! the pointer, so the app in front stays in front. A Mac's alone, as
//! dictation is.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, Icon, Sizable as _};

/// How many of the microphone's last moments the bars show.
pub const BARS: usize = 10;
/// Each side of the pill, beside the notch or the middle.
const EAR: f32 = 64.;
/// A side as wide as a few words, for what is said in it.
const EAR_SAID: f32 = 150.;
/// The pill's height on a screen with no notch.
const PLAIN_H: f32 = 32.;
const ENTER: Duration = Duration::from_millis(220);
const LEAVE: Duration = Duration::from_millis(180);
const GROW: Duration = Duration::from_millis(200);

pub enum Phase {
    Listening,
    Writing,
    /// A few words, such as nothing having been heard.
    Said(SharedString),
}

pub struct Pill {
    phase: Phase,
    levels: VecDeque<f32>,
    /// The notch's width, 0 with none, and the pill's height.
    middle: f32,
    height: f32,
    shown: Instant,
    leaving: Option<Instant>,
    /// The right side's width as it grows or shrinks to a new phase.
    ear_from: f32,
    ear_since: Instant,
}

/// The pill's window: as wide as the pill can grow, at the top of the
/// screen under the pointer.
pub fn open(cx: &mut App) -> Option<WindowHandle<Pill>> {
    let place = crate::sys::pill_place()?;
    let (middle, height) = match place.notch {
        Some((left, right, top)) => ((place.width - left - right).max(0.), top),
        None => (8., PLAIN_H),
    };
    let width = middle + 2. * EAR_SAID;
    let bounds = Bounds::new(point(px((place.width - width) / 2.), px(0.)), size(px(width), px(height)));
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                display_id: Some(DisplayId::new(place.display as u64)),
                window_background: WindowBackgroundAppearance::Transparent,
                inactive_frame_interval: None,
                ..Default::default()
            },
            |window, cx| {
                crate::sys::float_pill(window);
                let now = Instant::now();
                cx.new(|_| Pill { phase: Phase::Listening, levels: VecDeque::new(), middle, height, shown: now, leaving: None, ear_from: EAR, ear_since: now })
            },
        )
        .ok()?;
    Some(handle)
}

impl Pill {
    /// The voice's loudness, newest last, 0 to 1.
    pub fn set_levels(&mut self, levels: &VecDeque<f32>, cx: &mut Context<Self>) {
        self.levels = levels.iter().rev().take(BARS).rev().copied().collect();
        cx.notify();
    }

    pub fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        self.ear_from = self.ear_now();
        self.ear_since = Instant::now();
        self.phase = phase;
        cx.notify();
    }

    /// The pill goes back up into the top edge, and its window closes
    /// when it is gone.
    pub fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.leaving.is_some() {
            return;
        }
        self.leaving = Some(Instant::now());
        cx.notify();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(LEAVE + Duration::from_millis(30)).await;
            let _ = cx.update(|window, _| window.remove_window());
        })
        .detach();
    }

    fn ear_to(&self) -> f32 {
        if matches!(self.phase, Phase::Said(_)) {
            EAR_SAID
        } else {
            EAR
        }
    }

    fn ear_now(&self) -> f32 {
        let t = ease((self.ear_since.elapsed().as_secs_f32() / GROW.as_secs_f32()).min(1.));
        self.ear_from + (self.ear_to() - self.ear_from) * t
    }
}

/// Fast at first and slow to rest.
fn ease(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

impl Render for Pill {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // How far down it has come: in from the top edge, and back up.
        let shown = match self.leaving {
            Some(at) => 1. - ease((at.elapsed().as_secs_f32() / LEAVE.as_secs_f32()).min(1.)),
            None => ease((self.shown.elapsed().as_secs_f32() / ENTER.as_secs_f32()).min(1.)),
        };
        let ear = self.ear_now();
        let growing = self.ear_since.elapsed() < GROW;
        let moving = self.leaving.is_some() || self.shown.elapsed() < ENTER;
        if moving || growing || matches!(self.phase, Phase::Writing) {
            window.request_animation_frame();
        }
        let h = self.height;
        let white = gpui::white();
        let phase = match self.phase {
            Phase::Listening => 0usize,
            Phase::Writing => 1,
            Phase::Said(_) => 2,
        };
        let content: AnyElement = match &self.phase {
            Phase::Listening => {
                let bars = (0..BARS).map(|i| {
                    let at = (self.levels.len() + i).checked_sub(BARS).and_then(|j| self.levels.get(j)).copied().unwrap_or(0.0);
                    div().w(px(3.)).h(px(4. + 12. * at)).rounded_full().bg(white.opacity(0.6 + 0.4 * at))
                });
                h_flex().gap(px(2.5)).items_center().children(bars).into_any_element()
            }
            Phase::Writing => {
                // A wave passing through the bars while the words are written.
                let t = self.ear_since.elapsed().as_secs_f32();
                let bars = (0..BARS).map(|i| {
                    let at = 0.5 + 0.5 * (t * 6. - i as f32 * 0.55).sin();
                    div().w(px(3.)).h(px(4. + 6. * at)).rounded_full().bg(white.opacity(0.4 + 0.35 * at))
                });
                h_flex().gap(px(2.5)).items_center().children(bars).into_any_element()
            }
            Phase::Said(words) => div().text_size(px(12.)).text_color(white.opacity(0.9)).whitespace_nowrap().child(words.clone()).into_any_element(),
        };
        div().size_full().flex().justify_center().child(
            h_flex()
                .mt(px(-h * (1. - shown)))
                .h(px(h))
                .w(px(self.middle + 2. * ear))
                .bg(gpui::black())
                .rounded_bl(px(h * 0.42))
                .rounded_br(px(h * 0.42))
                .overflow_hidden()
                .child(h_flex().w(px(ear)).h_full().flex_shrink_0().pl(px(14.)).items_center().child(Icon::default().path("icons/wave-sine.svg").with_size(px(15.)).text_color(white)))
                .child(div().w(px(self.middle)).flex_shrink_0())
                .child(
                    h_flex()
                        .w(px(ear))
                        .h_full()
                        .flex_shrink_0()
                        .pl(px(6.))
                        .pr(px(14.))
                        .items_center()
                        .when(!matches!(self.phase, Phase::Said(_)), |d| d.justify_end())
                        .opacity(shown)
                        .child(div().child(content).with_animation(("pill-phase", phase), Animation::new(Duration::from_millis(180)), |d, t| d.opacity(t))),
                ),
        )
    }
}
