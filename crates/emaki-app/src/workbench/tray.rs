//! What stands around the composer besides the composer. Over it, the tray:
//! one card that holds what waits on the person, a message that cannot
//! get through and what the agent asks them to decide, and is there only
//! while something does. Under the tray, the agent's own line
//! (`foot_line`). Under the composer, the toggles (`render_toggles`): the
//! rest of what a session has going on, a small button each, shut until
//! pressed. `docs/live-turn.md`, The tray and The toggles.

use super::*;

/// A line of the tray that is one row tall, and the row in it. A button
/// in it has the same air above, below and at the card's edge, and the
/// card is rounded with it.
const LINE_H: f32 = 38.;
const ROW_H: f32 = 26.;
/// How far in the tray's card stands from the composer's edges, and how
/// far in from those the rows' and the composer's buttons all end. A
/// tray as wide as the composer had its buttons further from the side
/// than from the top.
const TRAY_IN: f32 = 7.;
const ROWS_IN: f32 = 15.;
/// The air over the agent's own line.
const FOOT_AIR: f32 = 6.;

/// A toggle's height and its width with nothing but its icon; a number
/// beside the icon adds to that.
const TOGGLE_H: f32 = 22.;
const TOGGLE_W: f32 = 26.;
const TOGGLE_DIGIT: f32 = 7.;
const TOGGLE_GAP: f32 = 3.;
/// In from the row's end the last toggle stands under the middle of the
/// composer's send button.
const TOGGLES_END: f32 = 12.;

/// How long a note stays after Claude Code takes it off its screen: it
/// shows one for moments, and the toggle may not be pressed for a while.
const NOTE_KEPT_SECS: f64 = 3600.0;

/// A line as `render_tray` lays it out.
pub(super) struct Line {
    pub key: String,
    pub on: bool,
    /// Its `presence`, when it is coming or going.
    pub moving: Option<(bool, u32)>,
    pub row: AnyElement,
    /// Its own height, for what may take two lines: it moves by itself
    /// (`fold_with`).
    pub free: bool,
}

/// What Claude Code last said beside a session's prompt, when it last
/// showed it, and whether the person has looked since it changed.
#[derive(Default, Clone)]
pub(super) struct Notes {
    said: Option<(emaki_core::driver::ScreenNote, f64)>,
    read: bool,
}

/// What the decision line answers.
#[derive(Clone)]
pub(super) enum Asked {
    Dialog(emaki_core::driver::Dialog),
    Permission(PermissionRequest),
}

/// The agent's own line, at work or waiting: not in the tray but under
/// it, on its own over the composer, and in from the sides as far as
/// the tray's rows are, so its mark and its button stand in their
/// columns. It opens up from the composer and closes back to it, the air
/// over it part of what moves.
pub(super) fn foot_line(line: Line) -> AnyElement {
    let tall = ROW_H + FOOT_AIR;
    let wrap = v_flex().w_full().max_w(CONTENT_W).px(px(ROWS_IN)).justify_end().overflow_hidden().child(div().w_full().h(px(ROW_H)).flex_shrink_0().child(line.row));
    match line.moving {
        Some((coming, n)) => wrap
            .with_animation(ElementId::Name(format!("{}-{n}", line.key).into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                let t = if coming { t } else { 1. - t };
                d.h(px((tall * t).round())).opacity(t)
            })
            .into_any_element(),
        None => wrap.h(px(tall)).into_any_element(),
    }
}

/// A row of the tray. Every row has this one shape, so what can be
/// pressed stands at one right edge whatever the row is: its mark, what
/// it is, a line on it that gives way first, how long, and its buttons.
pub(super) fn tray_row(id: impl Into<ElementId>, mark: impl IntoElement, title: impl IntoElement, detail: impl Into<SharedString>, time: Option<String>, pills: Vec<AnyElement>, theme: &gpui_component::Theme) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .h(px(ROW_H))
        .gap(px(8.))
        .items_center()
        .text_size(px(12.))
        .text_color(theme.muted_foreground)
        .child(div().size(px(16.)).flex_shrink_0().flex().items_center().justify_center().child(mark))
        .child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(title))
        .child(div().flex_1().min_w_0().truncate().child(detail.into()))
        .when_some(time, |d, time| d.child(div().flex_shrink_0().child(time)))
        .children(pills)
}

/// A button of a row, a little lower than the row. It takes the press
/// for itself, not the row's.
pub(super) fn tray_pill(id: impl Into<ElementId>, icon: &'static str, label: impl Into<SharedString>, kind: Press, off: bool, theme: &gpui_component::Theme) -> Stateful<Div> {
    crate::branches::small_pill(id, icon, label, kind, off, theme).h(px(24.)).occlude()
}

impl Workbench {
    /// The tray, its lines in the order they are given. A line comes and
    /// goes in motion; the first in and the last out take the card with
    /// them, and it opens up from the composer as a card does.
    pub(super) fn render_tray(&self, r: &SessionRef, lines: Vec<Line>, deciding: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let on = lines.iter().any(|l| l.on);
        let key = format!("tray-{}", r.session_id);
        let whole = self.arrival(&key, on, cx).is_some();
        if !on && !whole {
            return None;
        }
        let body = v_flex().w_full().children(lines.into_iter().enumerate().map(|(i, line)| {
            let free = line.free;
            let row = div().w_full().flex_shrink_0().px(px(ROWS_IN - TRAY_IN)).py(px(6.)).when(i > 0, |d| d.border_t_1().border_color(theme.border.opacity(0.6))).child(line.row);
            if free {
                // It says for itself how tall it is (`decision_line`).
                return row.into_any_element();
            }
            let tall = LINE_H + if i > 0 { 1. } else { 0. };
            let wrap = v_flex().w_full().justify_end().overflow_hidden().child(row.h(px(tall)));
            match line.moving.filter(|_| !whole) {
                Some((coming, n)) => wrap
                    .with_animation(ElementId::Name(format!("{}-{n}", line.key).into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                        let t = if coming { t } else { 1. - t };
                        d.h(px((tall * t).round())).opacity(t)
                    })
                    .into_any_element(),
                None => wrap.h(px(tall)).into_any_element(),
            }
        }));
        let card = div().w_full().pt(px(8.)).px(px(TRAY_IN)).child(div().w_full().rounded(px(LINE_H / 2.)).border_1().border_color(if deciding { theme.primary } else { theme.border }).bg(theme.popover).child(body));
        self.fold_with(key, on, 0., card.into_any_element(), cx)
    }

    /// A message that cannot get to the prompt: what stands in its way,
    /// and with one press the way past it. Sent into a silence, it
    /// looked sent and nothing came back.
    pub(super) fn held_line(&self, r: &SessionRef, cx: &mut Context<Self>) -> Option<Line> {
        use emaki_core::driver::Held;
        let key = format!("line-held-{}", r.session_id);
        let live = self.held.get(&r.session_id).cloned();
        let on = live.is_some();
        if let Some(h) = &live {
            *self.held_kept.borrow_mut() = Some(h.clone());
        }
        let moving = self.presence(&key, on, cx);
        if !on && moving.is_none() {
            return None;
        }
        let held = self.held_kept.borrow().clone()?;
        let theme = cx.theme().clone();
        let agent = r.agent.display_name();
        let (detail, pill) = match &held {
            Held::Covered(name) if name.is_empty() => (tf("something is open over {agent}'s input", &[("agent", &agent)]), Some(("icons/arrow-left.svg", t("Back to the input")))),
            Held::Covered(name) => (tf("“{what}” is open over {agent}'s input", &[("what", name), ("agent", &agent)]), Some(("icons/arrow-left.svg", t("Back to the input")))),
            Held::Asked => (tf("it goes once {agent}'s question is answered", &[("agent", &agent)]), None),
            Held::Other => (tf("{agent} is not at its input", &[("agent", &agent)]), Some(("icons/terminal.svg", t("Show the terminal")))),
        };
        let sid = r.session_id.clone();
        let covered = matches!(held, Held::Covered(_));
        let pills = pill.map(|(icon, word)| {
            tray_pill("held-fix", icon, word, Press::Primary, !on, &theme)
                .when(on, |b| {
                    b.on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        if covered {
                            this.hub.back_to_prompt(&sid);
                        } else {
                            this.go_to_terminal(cx);
                        }
                    }))
                })
                .into_any_element()
        });
        let row = tray_row("held-row", Icon::default().path("icons/triangle-alert.svg").with_size(px(14.)).text_color(theme.warning), t("Your message is waiting"), detail, None, pills.into_iter().collect(), &theme);
        Some(Line { key, on, moving, row: row.into_any_element(), free: false })
    }

    /// Claude Code's own notes for the session showing, off its screen.
    /// Only a hidden terminal of ours has a screen to read. One read at
    /// a time, off the main thread.
    pub(super) fn read_notes(&mut self, cx: &mut Context<Self>) {
        let target = self.selected_ref().filter(|_| self.page == Page::Session).and_then(|r| self.hub.terminal_for(&r.session_id).map(|p| (r.session_id.clone(), p.pid)));
        let Some((sid, pid)) = target else { return };
        if self.notes_reading {
            return;
        }
        self.notes_reading = true;
        cx.spawn(async move |this, cx| {
            let seen = cx.background_spawn(async move { crate::sys::terminal_styled(pid).map(|t| emaki_core::driver::notes_on_screen(&t)).unwrap_or_default() }).await;
            let _ = this.update(cx, |this, cx| {
                this.notes_reading = false;
                let now = this.now;
                let gone = this.notes_gone.clone();
                let notes = this.notes.entry(sid.clone()).or_default();
                let before = notes.said.as_ref().map(|n| n.0.text.clone());
                if let Some(note) = seen.into_iter().rfind(|n| !gone.contains(&n.text)) {
                    notes.read &= before.as_deref() == Some(note.text.as_str());
                    notes.said = Some((note, now));
                }
                notes.said = notes.said.take().filter(|(_, at)| now - at < NOTE_KEPT_SECS);
                let after = notes.said.as_ref().map(|n| n.0.text.clone());
                if after.is_none() && this.notes_open.as_deref() == Some(sid.as_str()) {
                    this.notes_open = None;
                }
                if before != after {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A note as if it had been read, for a probe (`note:`, a word for
    /// the space that a probe's value cannot hold).
    pub(super) fn note_probe(&mut self, sid: String, text: &str, now: f64) {
        let note = emaki_core::driver::ScreenNote { text: text.replace('_', " "), color: Some(0xFFC107) };
        self.notes.insert(sid, Notes { said: Some((note, now + 600.)), read: false });
    }

    /// Opens or shuts the note's card, which reads the note.
    pub(super) fn notes_toggle(&mut self, sid: &str) {
        self.notes_open = if self.notes_open.as_deref() == Some(sid) { None } else { Some(sid.to_string()) };
        if self.notes_open.is_some() {
            if let Some(n) = self.notes.get_mut(sid) {
                n.read = true;
            }
            self.cards_show();
        }
    }

    /// One toggle under the composer: an icon, how many when there is
    /// more than one, and what it is when pointed at. It comes and goes
    /// sideways, what stands beside it moving over, and being on or off
    /// is in the accent, over a moment. `press` is what it does.
    #[allow(clippy::too_many_arguments)]
    fn foot_toggle(&self, name: &str, r: &SessionRef, there: bool, open: bool, icon: &'static str, n: usize, ink: Option<Hsla>, dot: Option<(Hsla, f32)>, words: String, press: impl Fn(&mut Self) + 'static, cx: &mut Context<Self>) -> Option<AnyElement> {
        let key = format!("toggle-{name}-{}", r.session_id);
        let moving = self.presence(&key, there, cx);
        if !there && moving.is_none() {
            return None;
        }
        let flip = self.presence(&format!("{key}-open"), open, cx);
        let theme = cx.theme().clone();
        let (rest, lit, plate) = (ink.unwrap_or(theme.muted_foreground), theme.primary, theme.primary.opacity(0.14));
        let number = (n > 1).then(|| n.to_string());
        let wide = TOGGLE_W + number.as_ref().map_or(0., |s| TOGGLE_GAP + TOGGLE_DIGIT * s.len() as f32);
        let mark = div()
            .relative()
            .size(px(14.))
            .flex_shrink_0()
            .child(Icon::default().path(icon).with_size(px(14.)))
            .when_some(dot, |d, (color, strength)| {
                let spot = div().size(px(5.)).rounded_full().bg(color).opacity(strength);
                d.child(div().absolute().top(px(-2.)).right(px(-2.)).size(px(7.)).rounded_full().bg(theme.background).flex().items_center().justify_center().child(spot))
            });
        let face = h_flex()
            .id(SharedString::from(key.clone()))
            .w(px(wide))
            .h(px(TOGGLE_H))
            .flex_shrink_0()
            .gap(px(TOGGLE_GAP))
            .items_center()
            .justify_center()
            .rounded_full()
            .text_size(px(11.5))
            .text_color(if open { lit } else { rest })
            .when(open, |d| d.bg(plate))
            .cursor_pointer()
            .hover(|s| if open { s.bg(theme.primary.opacity(0.2)) } else { s.bg(theme.muted) })
            .child(mark)
            .children(number)
            .managed_tooltip(move |window, cx| crate::workbench::tip(words.clone(), window, cx))
            .when(there, |d| {
                d.on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    press(this);
                    cx.notify();
                }))
            });
        let face = match flip {
            Some((coming, step)) => face
                .with_animation(ElementId::Name(format!("{key}-open-{step}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                    let k = if coming { t } else { 1. - t };
                    d.bg(Hsla { a: plate.a * k, ..plate }).text_color(crate::transcript::mix(rest, lit, k))
                })
                .into_any_element(),
            None => face.into_any_element(),
        };
        Some(match moving {
            Some((coming, step)) => div()
                .h(px(TOGGLE_H))
                .flex_shrink_0()
                .overflow_hidden()
                .child(face)
                .with_animation(ElementId::Name(format!("{key}-{step}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                    let t = if coming { t } else { 1. - t };
                    d.w(px((wide * t).round())).opacity(t)
                })
                .into_any_element(),
            None => face,
        })
    }

    /// The toggles: what a session has going on that does not wait on
    /// the person, each shut until pressed, and its card at the
    /// conversation's end when it is on. A dot breathes on what is
    /// running, and a dot in the note's own color stands on a note not
    /// yet read.
    pub(super) fn render_toggles(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let r = self.selected_ref().filter(|_| self.page == Page::Session)?.clone();
        let session = &self.detail.as_ref().filter(|d| d.session.id == r.session_id)?.session;
        let sid = r.session_id.clone();
        let theme = cx.theme().clone();
        let mut toggles: Vec<AnyElement> = Vec::new();

        let pages = &session.artifacts;
        let id = sid.clone();
        toggles.extend(self.foot_toggle("artifacts", &r, !pages.is_empty(), self.artifacts_open.as_deref() == Some(sid.as_str()), "icons/app-window.svg", pages.len(), None, None, count(pages.len(), "{n} artifact", "{n} artifacts"), move |this| {
            this.artifacts_open = if this.artifacts_open.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
            if this.artifacts_open.is_some() {
                this.cards_show();
            }
        }, cx));

        let running = self.shells_running(&r, session);
        let breath = crate::ambient::pulse(0.35, 1., crate::ambient::phase(Duration::from_millis(1400)));
        for (kind, icon) in [(TaskKind::Agents, "icons/bot.svg"), (TaskKind::Monitors, "icons/eye.svg"), (TaskKind::Commands, "icons/terminal.svg")] {
            let list: Vec<_> = running.iter().filter(|sh| task_of(sh) == kind).cloned().collect();
            if !list.is_empty() {
                self.shells_kept.borrow_mut()[kind as usize] = list.clone();
            }
            let n = list.len();
            let words = match kind {
                TaskKind::Agents => count(n, "{n} background agent", "{n} background agents"),
                TaskKind::Commands => count(n, "{n} background command", "{n} background commands"),
                TaskKind::Monitors => count(n, "{n} background monitor", "{n} background monitors"),
            };
            let id = sid.clone();
            toggles.extend(self.foot_toggle(kind.word(), &r, n > 0, self.task_open(kind).as_deref() == Some(sid.as_str()), icon, n, None, Some((theme.blue, breath)), words, move |this| {
                this.task_toggle(kind, &id);
                if this.task_open(kind).is_some() {
                    this.cards_show();
                }
            }, cx));
        }

        let notes = self.notes.get(&sid).cloned().unwrap_or_default();
        let note = notes.said.map(|n| n.0);
        if let Some(n) = &note {
            *self.notes_kept.borrow_mut() = Some(n.clone());
        }
        let ink = note.as_ref().filter(|_| !notes.read).map(|n| screen_ink(n.color, &theme));
        let id = sid.clone();
        toggles.extend(self.foot_toggle("notes", &r, note.is_some(), self.notes_open.as_deref() == Some(sid.as_str()), "icons/bell.svg", 1, ink, ink.map(|c| (c, 1.)), count(1, "{n} notice", "{n} notices"), move |this| this.notes_toggle(&id), cx));

        (!toggles.is_empty()).then(|| h_flex().flex_shrink_0().mr(px(TOGGLES_END)).items_center().children(toggles).into_any_element())
    }

    /// What the bell opens: Claude Code's note in its own color, at the
    /// conversation's end, and its ×.
    pub(super) fn render_notes_card(&self, r: &SessionRef, cx: &mut Context<Self>) -> Option<AnyElement> {
        let live = self.notes.get(&r.session_id).is_some_and(|n| n.said.is_some());
        let on = live && self.notes_open.as_deref() == Some(r.session_id.as_str());
        let key = format!("fold-notes-{}", r.session_id);
        if !on && !self.lingering(&key) {
            return None;
        }
        let note = self.notes_kept.borrow().clone()?;
        let theme = cx.theme().clone();
        let ink = screen_ink(note.color, &theme);
        let (sid, text) = (r.session_id.clone(), note.text.clone());
        let close = div()
            .id("note-close")
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .when(on, |d| d.cursor_pointer().hover(|s| s.bg(theme.muted)))
            .child(Icon::new(IconName::Close).with_size(px(12.)).text_color(theme.muted_foreground))
            .when(on, |d| {
                d.on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.notes_gone.insert(text.clone());
                    this.notes_open = None;
                    if let Some(n) = this.notes.get_mut(&sid) {
                        n.said = None;
                    }
                    cx.notify();
                }))
            });
        let row = h_flex().w_full().px(px(8.)).py(px(2.)).gap(px(10.)).items_center().child(Icon::default().path("icons/bell.svg").with_size(px(15.)).text_color(ink).flex_shrink_0()).child(div().flex_1().min_w_0().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_color(ink).child(note.text)).child(close);
        // The card's own words are the window's face, not the chat's.
        let card = v_flex().w_full().p(px(6.)).rounded(px(10.)).border_1().border_color(theme.border).bg(theme.muted.opacity(0.25)).font_family(theme.font_family.clone()).child(row);
        self.card_fold(key, on, card.into_any_element(), cx)
    }

    /// What the agent waits for the person to decide, under everything
    /// else and always in view: its card at the conversation's end says
    /// what is asked and scrolls, and may be left behind, and the
    /// choices stand here in a row. A card that held them was cut off by
    /// the composer, and scrolled away from under the pointer.
    pub(super) fn decision_line(&self, r: &SessionRef, left_behind: bool, others: bool, cx: &mut Context<Self>) -> Option<Line> {
        let live = self.dialog_seen.as_ref().filter(|(sid, _)| *sid == r.session_id).map(|(_, d)| Asked::Dialog(d.clone())).or_else(|| self.permissions.iter().find(|(sid, _)| *sid == r.session_id).map(|(_, p)| Asked::Permission(p.clone())));
        let key = format!("bar-{}", r.session_id);
        let on = live.is_some();
        if let Some(a) = live {
            *self.bar_kept.borrow_mut() = Some(a);
        }
        let moving = self.arrival(&key, on, cx);
        if !on && moving.is_none() {
            return None;
        }
        let asked = self.bar_kept.borrow().clone()?;
        let row = match &asked {
            Asked::Dialog(d) => self.dialog_bar(r, d, !on, left_behind, cx),
            Asked::Permission(p) => self.permission_bar(r, p, !on, left_behind, cx),
        };
        // Alone it moves with the tray; among others, by itself.
        let row = if others { self.fold_with(key.clone(), on, 0., row, cx)? } else { row };
        Some(Line { key, on, moving, row, free: true })
    }

    /// "Show", for a card the reader has gone up the conversation from.
    fn show_pill(&self, show: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        show.then(|| {
            tray_pill("bar-show", "icons/arrow-down.svg", t("Show"), Press::Quiet, false, &theme)
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.cards_show();
                    cx.notify();
                }))
                .into_any_element()
        })
    }

    /// A choice as a button of the decision line: one line, cut with its
    /// whole words under the pointer. A choice's words are as long as
    /// the agent makes them ("Yes, and don't ask again for cargo…").
    fn bar_pill(&self, id: impl Into<ElementId>, icon: &'static str, label: String, kind: Press, off: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let whole = label.clone();
        tray_pill(id, icon, "", kind, off, &theme).max_w(px(300.)).min_w_0().child(div().min_w_0().truncate().child(label)).managed_tooltip(move |window, cx| crate::workbench::tip(whole.clone(), window, cx))
    }

    /// The decision line's frame: what is asked, or the tabs of what is
    /// asked, and the buttons, at the right edge the rows' are at. Too
    /// wide for one line, the buttons go under it.
    fn bar_frame(&self, r: &SessionRef, said: Option<String>, tabs: Option<AnyElement>, hint: &'static str, pills: Vec<AnyElement>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        h_flex()
            .w_full()
            .min_h(px(ROW_H))
            .flex_wrap()
            .gap_x(px(8.))
            .gap_y(px(6.))
            .items_center()
            .text_size(px(12.))
            .text_color(theme.muted_foreground)
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(150.))
                    .gap(px(8.))
                    .items_center()
                    .child(div().size(px(16.)).flex_shrink_0().flex().items_center().justify_center().child(agent_glyph(r.agent, px(14.), agent_color(r.agent, &theme), false)))
                    .when_some(said, |d, said| d.child(div().min_w_0().truncate().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(said)))
                    .children(tabs)
                    .when(!hint.is_empty(), |d| d.child(div().min_w_0().truncate().child(hint))),
            )
            .child(h_flex().ml_auto().flex_wrap().min_w_0().gap(px(6.)).items_center().justify_end().children(pills))
            .into_any_element()
    }

    fn dialog_bar(&self, r: &SessionRef, d: &emaki_core::driver::Dialog, leaving: bool, left_behind: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let off = self.dialog_sending || leaving;
        let approval = d.body.last().is_some_and(|l| l.starts_with("Do you want"));
        let review = d.body.first().is_some_and(|l| l.starts_with("Review your answers"));
        let plan = d.body.iter().any(|l| l.contains(PLAN_ASKS));
        let mut pills: Vec<AnyElement> = self.show_pill(left_behind, cx).into_iter().collect();
        let said = if approval {
            d.body.last().cloned()
        } else if plan {
            Some(tf("{agent} has a plan", &[("agent", &r.agent.speaker())]))
        } else if review {
            // Claude Code's own words, as what an approval asks is.
            d.body.first().cloned()
        } else {
            None
        };
        if approval || review || plan {
            // The choices are the buttons, in the order the terminal
            // has them: the first is what is asked for, the last of an
            // approval a no.
            for (i, o) in d.options.iter().enumerate() {
                let own = o.label.starts_with("Type something");
                let n = o.n;
                let no = o.label.starts_with("No") || (review && i > 0);
                let (icon, kind) = if i == 0 { ("icons/check.svg", Press::Primary) } else if no { ("icons/close.svg", Press::Quiet) } else { ("icons/circle-check.svg", Press::Quiet) };
                pills.push(
                    self.bar_pill(SharedString::from(format!("bar-opt-{n}")), icon, o.label.clone(), kind, off, cx)
                        .when(!off, |b| {
                            b.on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                if own {
                                    let handle = this.composer.read(cx).focus_handle(cx);
                                    window.focus(&handle, cx);
                                    this.notice = Some(Notice::said(t("type your answer in the box and send it")));
                                    cx.notify();
                                } else {
                                    this.dialog_send(vec![DialogStep { keys: n.to_string(), until: None }], cx);
                                }
                            }))
                        })
                        .into_any_element(),
                );
            }
        } else {
            if d.preview.is_some() {
                pills.push(
                    tray_pill("dialog-choose", "icons/check.svg", t("Choose"), Press::Primary, off, &theme)
                        .when(!off, |b| {
                            b.on_click(cx.listener(|this, _, window, cx| {
                                swallow_click(window, cx);
                                this.dialog_send(vec![DialogStep { keys: "\r".into(), until: None }], cx)
                            }))
                        })
                        .into_any_element(),
                );
            }
            if d.multi {
                pills.push(
                    tray_pill("dialog-next", "icons/arrow-right.svg", t("Next"), Press::Primary, off, &theme)
                        .when(!off, |b| {
                            b.on_click(cx.listener(|this, _, window, cx| {
                                swallow_click(window, cx);
                                this.dialog_send(vec![DialogStep { keys: "\t".into(), until: None }], cx)
                            }))
                        })
                        .into_any_element(),
                );
            }
        }
        if !review {
            pills.push(
                tray_pill("dialog-cancel", "icons/close.svg", t("Cancel"), Press::Quiet, off, &theme)
                    .child(key_text("Esc"))
                    .when(!off, |b| {
                        b.on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.dialog_send(vec![DialogStep { keys: "\x1b".into(), until: None }], cx)
                        }))
                    })
                    .into_any_element(),
            );
        }
        let tabs = (!d.tabs.is_empty() && said.is_none()).then(|| self.dialog_tabs(d, off, cx));
        let said = said.or_else(|| tabs.is_none().then(|| tf("{agent} asks", &[("agent", &r.agent.speaker())])));
        let typed = !d.multi && d.options.iter().any(|o| o.label.starts_with("Type something"));
        self.bar_frame(r, said, tabs, if typed && !approval { t("or type an answer below") } else { "" }, pills, cx)
    }

    fn permission_bar(&self, r: &SessionRef, p: &PermissionRequest, leaving: bool, left_behind: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut pills: Vec<AnyElement> = self.show_pill(left_behind, cx).into_iter().collect();
        let who = r.agent.speaker();
        if p.is_question() {
            let qs = questions_of(&p.input);
            let picks = self.picks.get(&p.request_id).cloned().unwrap_or_default();
            let by_button = qs.len() > 1 || qs.iter().any(|q| q.multi);
            let ready = qs.iter().all(|q| picks.get(&q.question).map(|l| !l.is_empty()).unwrap_or(false));
            if by_button {
                let rid = p.request_id.clone();
                pills.push(
                    tray_pill(SharedString::from(format!("answer-{}", p.request_id)), "icons/check.svg", t("Answer"), Press::Primary, !ready || leaving, &theme)
                        .when(ready && !leaving, |b| {
                            b.on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.submit_question(rid.clone(), cx)
                            }))
                        })
                        .into_any_element(),
                );
            }
            return self.bar_frame(r, Some(tf("{agent} asks", &[("agent", &who)])), None, t("or type an answer below"), pills, cx);
        }
        let plan = p.tool_name == "ExitPlanMode" && p.input.get("plan").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
        let said = if plan { t("Implement this plan?").to_string() } else { crate::workbench::permission_heading(p, who) };
        let (yes, no) = (p.request_id.clone(), p.request_id.clone());
        // What only the agent's terminal can take: the way there, and a
        // way past it.
        if p.in_terminal() {
            pills.push(
                tray_pill(SharedString::from(format!("terminal-{}", p.request_id)), "icons/terminal.svg", t("Show the terminal"), Press::Primary, leaving, &theme)
                    .when(!leaving, |b| {
                        b.on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.show_agent_terminal(window, cx)
                        }))
                    })
                    .into_any_element(),
            );
            pills.push(
                tray_pill(SharedString::from(format!("skip-{}", p.request_id)), "icons/close.svg", t("Skip"), Press::Quiet, leaving, &theme)
                    .when(!leaving, |b| {
                        b.on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.answer_permission(no.clone(), false, cx)
                        }))
                    })
                    .into_any_element(),
            );
            return self.bar_frame(r, Some(said), None, "", pills, cx);
        }
        let open_page = p.tool_name == emaki_core::driver::OPEN_URL_ASK;
        let several = self.permissions.iter().filter(|(sid, p)| *sid == r.session_id && !p.is_question() && !p.in_terminal()).count() > 1;
        pills.push(
            tray_pill(SharedString::from(format!("allow-{}", p.request_id)), "icons/check.svg", if plan { t("Yes, implement") } else if open_page { t("Open") } else { t("Allow") }, Press::Primary, leaving, &theme)
                .child(key_text("↩"))
                .when(!leaving, |b| {
                    b.on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.answer_permission(yes.clone(), true, cx)
                    }))
                })
                .into_any_element(),
        );
        pills.push(
            tray_pill(SharedString::from(format!("deny-{}", p.request_id)), "icons/close.svg", if plan { t("No, keep planning") } else { t("Deny") }, Press::Quiet, leaving, &theme)
                .child(key_text("⇧↩"))
                .when(!leaving, |b| {
                    b.on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.answer_permission(no.clone(), false, cx)
                    }))
                })
                .into_any_element(),
        );
        if several {
            pills.push(
                tray_pill("allow-all", "icons/circle-check.svg", t("Allow all"), Press::Quiet, leaving, &theme)
                    .when(!leaving, |b| {
                        b.on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.allow_all_pending(cx)
                        }))
                    })
                    .into_any_element(),
            );
        }
        self.bar_frame(r, Some(said), None, "", pills, cx)
    }
}
