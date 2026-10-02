//! Drawing one round of a session: the prompt, then every item the agent
//! produced. Tool calls and thoughts fold; a Task call unfolds into the
//! subagent's own rounds.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::highlighter::HighlightTheme;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};
use emaki_core::model::*;
use emaki_core::render_md::{clip, code_block, command_text, human_duration, patch_stat, pretty_args, render_patch};

use crate::format::stamp;
use std::path::PathBuf;
use std::time::Duration;

use std::collections::HashSet;

use crate::workbench::{agent_color, agent_glyph, agent_icon, badge_str, file_icon, file_kind, fit_thumb, human_size, swallow_click, Workbench, CONTENT_W};

const MAX_BODY: usize = 6000;
/// An opened tool call shows at most this much before it scrolls inside
/// its card, so one long output never takes the whole window.
const MAX_BODY_H: Pixels = px(400.);
/// How far a reply's right edge stays inside the prompts' right edge.
const REPLY_INSET: Pixels = px(40.);
/// The line under a prompt or a reply that shows on hover: the time and
/// the copy button. It is always laid out at this height.
const HOVER_ROW_H: Pixels = px(24.);

/// Whether the markdown crate can parse `text` without panicking. Version
/// 1.0.0, the one the toolkit's text view uses, aborts on some inputs
/// ("Cannot push to non-parent" in `to_mdast`); the smallest one found is a
/// paragraph followed by two `---` lines, which YAML front matter quoted in
/// a Codex tool result produces. A panic inside an element's layout takes
/// the app down, so every text is tried here first, under `catch_unwind`,
/// once per distinct text (the answer is kept by hash).
fn markdown_is_safe(text: &str) -> bool {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    thread_local! {
        static SEEN: std::cell::RefCell<HashMap<u64, bool>> = std::cell::RefCell::new(HashMap::new());
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    let key = h.finish();
    if let Some(known) = SEEN.with(|s| s.borrow().get(&key).copied()) {
        return known;
    }
    let opts = markdown::ParseOptions::gfm();
    let safe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| markdown::to_mdast(text, &opts).is_ok())).unwrap_or(false);
    SEEN.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() > 4096 {
            s.clear();
        }
        s.insert(key, safe);
    });
    safe
}

pub(crate) fn md_view(id: String, text: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    // A text the parser cannot take is shown as it is, in a code block,
    // rather than taking the window down.
    let text = if markdown_is_safe(&text) { text } else { code_block(&text, "text") };
    let dark = theme.mode.is_dark();
    let code_bg = theme.muted;
    let border = theme.border;
    TextView::markdown(SharedString::from(format!("{id}-{}", if dark { "d" } else { "l" })), text)
        .selectable(true)
        .style(
            TextViewStyle {
                heading_base_font_size: px(15.),
                paragraph_gap: rems(0.6),
                highlight_theme: if dark { HighlightTheme::default_dark() } else { HighlightTheme::default_light() },
                // As the Claude app sets them: strong text at 600, not the
                // font's true Bold; inline code in the mono face. These and
                // the plate are Emaki's additions to the vendored toolkit.
                strong_font_weight: Some(FontWeight::SEMIBOLD),
                inline_code_font_family: Some(crate::fonts::inline_code_family(cx).unwrap_or_else(|| theme.mono_font_family.clone())),
                // Inline code as the Claude app draws it, in the accent
                // config asks for: the readable shade of it for the
                // letters, a faint wash of it on a rounded plate behind.
                inline_code: HighlightStyle { color: Some(theme.link), ..Default::default() },
                inline_code_chip: Some((theme.primary.opacity(if dark { 0.16 } else { 0.10 }), theme.primary.opacity(if dark { 0.22 } else { 0.18 }))),
                ..Default::default()
            }
            .code_block(StyleRefinement::default().bg(code_bg).border_1().border_color(border).rounded(px(10.)).px(px(12.)).py(px(10.))),
        )
}

/// The plain-words line on a tool call or a permission card: a box on a
/// faint accent tint, the text drawn through the markdown view so it can
/// be selected and copied. With no text yet it says so. It fades and
/// settles in over a moment when it appears, keyed on its own id, so each
/// opening plays once.
pub(crate) fn explain_card(id: String, text: String, pending: bool, size: Pixels, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    let anim: SharedString = format!("{id}-in").into();
    v_flex()
        .w_full()
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .bg(theme.primary.opacity(if theme.mode.is_dark() { 0.10 } else { 0.07 }))
        .when(pending && text.is_empty(), |d| d.child(div().italic().text_size(size - px(1.)).text_color(theme.muted_foreground).child("Explaining…")))
        .when(!(pending && text.is_empty()), |d| d.child(div().text_size(size).text_color(theme.foreground).line_height(relative(1.6)).child(md_view(id, text, cx))))
        .with_animation(ElementId::Name(anim), Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t).mt(px(-4. * (1. - t))))
}

/// The strip under a picture tile: its name, centred, in the mono face on
/// a muted ground, wrapping when it is long.
pub(crate) fn tile_caption(w: f32, name: String, theme: &gpui_component::Theme) -> impl IntoElement {
    div()
        .w(px(w))
        .px(px(8.))
        .py(px(5.))
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.muted)
        .font_family(theme.mono_font_family.clone())
        .text_size(px(11.))
        .text_color(theme.muted_foreground)
        .text_center()
        .whitespace_normal()
        .child(name)
}

/// Straight-line mix of two colours, for a state change drawn over a moment.
fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    Hsla { h: a.h + (b.h - a.h) * t, s: a.s + (b.s - a.s) * t, l: a.l + (b.l - a.l) * t, a: a.a + (b.a - a.a) * t }
}

fn mono_block(id: String, text: &str, lang: &str, cx: &App) -> impl IntoElement {
    let (body, dropped) = clip(text, MAX_BODY);
    let mut md = code_block(&body, lang);
    if dropped > 0 {
        md.push_str(&format!("\n\n*… {dropped} more lines*"));
    }
    md_view(id, md, cx)
}

/// Where the folded run of tool calls holding item `jx` begins, when there
/// is one: three or more tool calls in a row, thoughts between them
/// included, fold into one row (see `render_round`), and the find bar has
/// to open that row to show a hit inside it.
pub(crate) fn run_start(rnd: &Round, jx: usize) -> Option<usize> {
    let is_run_item = |it: &Item| matches!(it, Item::Tool(_) | Item::Thinking { .. });
    if !rnd.items.get(jx).map(is_run_item).unwrap_or(false) {
        return None;
    }
    let mut start = jx;
    while start > 0 && is_run_item(&rnd.items[start - 1]) {
        start -= 1;
    }
    let mut end = jx + 1;
    while end < rnd.items.len() && is_run_item(&rnd.items[end]) {
        end += 1;
    }
    let tools = rnd.items[start..end].iter().filter(|it| matches!(it, Item::Tool(_))).count();
    (tools >= 3).then_some(start)
}

/// What a tool call's badge says: the tool's own name in lower case
/// ("bash", "read", "write"), and for an MCP tool the last part of it
/// ("mcp__server__navigate" is "navigate"). The badge is the only place
/// the card names the tool; it used to say the kind ("run") beside the
/// name ("Bash"), which for most tools said the same word twice.
fn tool_label(name: &str) -> String {
    name.rsplit("__").next().unwrap_or(name).to_lowercase()
}

impl Workbench {
    pub fn render_round(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(detail) = &self.detail else { return div().into_any_element() };
        let session = detail.session.clone();
        let Some(rnd) = session.rounds.get(ix) else { return div().into_any_element() };
        let open_tools = detail.open_tools.clone();
        let open_thoughts = detail.open_thoughts.clone();
        let open_subagents = detail.open_subagents.clone();
        let open_runs = detail.open_runs.clone();
        let speaker = session.agent.speaker();
        let theme = cx.theme().clone();
        let is_last = ix + 1 == session.rounds.len();
        // The conversation's size from the settings: replies at it, the
        // prompt half a pixel under, thoughts two under.
        let body_px = self.cfg.app.chat_px();
        let prompt_mark = self.find_mark(ix, None);

        // Under a prompt: when it was sent, and whose it was when it was
        // not the person's own. The duration, the tool calls and the tokens
        // used to follow; nobody read them there.
        let who = match rnd.source {
            Source::Peer => Some("Another session"),
            Source::System => Some("Session"),
            _ => None,
        };
        let sent = stamp(&rnd.ts);

        let mut column = v_flex().w_full().max_w(CONTENT_W).px(px(24.)).pt(px(14.)).pb(if is_last { px(28.) } else { px(6.) }).gap(px(12.));

        // The prompt, as a bubble on the right: what was attached first
        // (pictures, then files), the words under them, the way a message
        // with a picture reads in the Claude app.
        if !rnd.prompt.is_empty() || rnd.images > 0 || !rnd.attachments.is_empty() {
            let mut bubble = v_flex()
                .max_w(px(600.))
                .px(px(16.))
                .py(px(11.))
                .gap(px(4.))
                .rounded(px(18.))
                .bg(theme.muted)
                .text_size(px(body_px - 0.5))
                .line_height(relative(1.55))
                // A prompt the find bar matched wears the accent on its
                // edge; the one the bar is on now, a full ring.
                .when(prompt_mark == 1, |d| d.border_1().border_color(theme.primary.opacity(0.45)))
                .when(prompt_mark == 2, |d| d.border_2().border_color(theme.primary));
            let has_text = !rnd.prompt.is_empty();
            if !rnd.attachments.is_empty() {
                let (pics, files): (Vec<&Attachment>, Vec<&Attachment>) = rnd.attachments.iter().partition(|a| a.kind == "image");
                if !pics.is_empty() {
                    let mut row = h_flex().flex_wrap().gap(px(8.)).pt(px(2.)).when(has_text || !files.is_empty(), |d| d.pb(px(4.)));
                    for a in pics {
                        row = row.child(self.render_attachment(ix, a, cx));
                    }
                    bubble = bubble.child(row);
                }
                if !files.is_empty() {
                    let mut row = h_flex().flex_wrap().gap(px(8.)).when(has_text, |d| d.pb(px(4.)));
                    for a in files {
                        row = row.child(self.render_attachment(ix, a, cx));
                    }
                    bubble = bubble.child(row);
                }
            } else if rnd.images > 0 {
                bubble = bubble.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(format!("+ {} pasted image{}", rnd.images, if rnd.images == 1 { "" } else { "s" })));
            }
            if has_text {
                bubble = bubble.child(md_view(format!("p-{ix}"), rnd.prompt.clone(), cx));
            }
            // The time and the copy button show while the pointer is over
            // the prompt's row, as in the Claude app. Their line is always
            // laid out, so nothing moves when they appear.
            let group = SharedString::from(format!("prompt-{ix}"));
            let copy = has_text.then(|| self.copy_button(ix, false, cx));
            column = column.child(
                v_flex().group(group.clone()).w_full().items_end().gap(px(2.)).child(bubble).child(
                    h_flex()
                        .h(HOVER_ROW_H)
                        .gap(px(6.))
                        .pr(px(2.))
                        .items_center()
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        .opacity(0.)
                        .group_hover(group, |s| s.opacity(1.))
                        .children(who.map(|w| div().child(w)))
                        .child(div().child(sent))
                        .children(copy),
                ),
            );
        } else {
            column = column.child(h_flex().gap(px(8.)).text_size(px(11.)).text_color(theme.muted_foreground).children(who.map(|w| div().child(w))).child(div().child(sent)));
        }

        // The agent's items, on the left. Consecutive tool calls (thoughts
        // between them included) fold into one row when there are three or
        // more: the trace is kept, the page is not buried in it.
        let mut body = v_flex().w_full().gap(px(8.));
        let mut any = false;
        let mut jx = 0;
        let n = rnd.items.len();
        while jx < n {
            any = true;
            let is_run_item = |it: &Item| matches!(it, Item::Tool(_) | Item::Thinking { .. });
            if is_run_item(&rnd.items[jx]) {
                let mut end = jx;
                while end < n && is_run_item(&rnd.items[end]) {
                    end += 1;
                }
                let tools = rnd.items[jx..end].iter().filter(|it| matches!(it, Item::Tool(_))).count();
                if tools >= 3 {
                    body = body.child(self.render_run(ix, jx, end, open_runs.contains(&(ix, jx)), &open_tools, &open_thoughts, &open_subagents, &session, cx));
                    jx = end;
                    continue;
                }
            }
            let el = self.render_item(ix, jx, &rnd.items[jx], &open_tools, &open_thoughts, &open_subagents, &session, cx);
            body = body.child(self.find_wrap(ix, jx, el, &theme));
            jx += 1;
        }
        if any {
            let mark_color = if session.agent == emaki_core::model::AgentId::ClaudeCode { theme.primary } else { theme.muted_foreground };
            // The reply's copy button and the time of its last words, under
            // its last line and shown while the pointer is over the reply.
            // The button's icon lines up with the text.
            let group = SharedString::from(format!("reply-{ix}"));
            let said = rnd.items.iter().rev().find_map(|i| if let Item::Text { ts, .. } = i { Some(stamp(ts)) } else { None }).unwrap_or_default();
            let copy = rnd.has_text().then(|| {
                h_flex()
                    .h(HOVER_ROW_H)
                    .mt(px(-4.))
                    .pl(px(17.))
                    .gap(px(6.))
                    .items_center()
                    .text_size(px(11.5))
                    .text_color(theme.muted_foreground)
                    .opacity(0.)
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .child(self.copy_button(ix, true, cx))
                    .child(div().child(said))
            });
            column = column.child(
                v_flex()
                    .group(group)
                    .w_full()
                    .gap(px(8.))
                    .child(h_flex().gap(px(7.)).items_center().child(agent_icon(session.agent, px(15.), mark_color)).child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).child(speaker)))
                    // The reply stops short of the column's right edge, where
                    // the prompt bubbles end: the two voices sit at different
                    // widths, as in the Claude app, and read apart at a glance.
                    .child(div().w_full().pl(px(22.)).pr(REPLY_INSET).child(body))
                    .children(copy),
            );
        }
        // The list lays each item out on its own, so an auto margin has
        // nothing to push against; an explicit centring parent keeps the
        // column in the middle of a wide pane.
        h_flex().w_full().justify_center().child(column).into_any_element()
    }

    /// The copy button under a prompt or a reply. It copies the markdown as
    /// it was written, not the rendered text: the prompt's words, or every
    /// text item of the reply (`Round::reply_markdown`), read out of the
    /// session at the click. The icon is a tick for a moment afterwards.
    fn copy_button(&self, ix: usize, reply: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let key = SharedString::from(format!("copy-{}-{ix}", if reply { "reply" } else { "prompt" }));
        let done = self.copied.as_ref() == Some(&key);
        let hover_bg = theme.muted;
        div()
            .id(key.clone())
            .size(px(24.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(if done { "Copied" } else { "Copy" }).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                let text = this.detail.as_ref().and_then(|d| d.session.rounds.get(ix)).map(|r| if reply { r.reply_markdown() } else { r.prompt.clone() }).unwrap_or_default();
                this.copy_text(key.clone(), text, cx);
            }))
            .child(Icon::new(if done { IconName::Check } else { IconName::Copy }).with_size(px(14.)).text_color(theme.muted_foreground))
    }

    /// An item the find bar matched wears the same accent ring a matched
    /// prompt does: a faint one for a hit, a full one for the hit the bar
    /// is on. One look for every kind of hit; no tints, no edge bars.
    fn find_wrap(&self, ix: usize, jx: usize, el: AnyElement, theme: &gpui_component::Theme) -> AnyElement {
        match self.find_mark(ix, Some(jx)) {
            0 => el,
            1 => div().w_full().rounded(px(10.)).px(px(6.)).py(px(3.)).ml(px(-6.)).border_1().border_color(theme.primary.opacity(0.45)).child(el).into_any_element(),
            _ => div().w_full().rounded(px(10.)).px(px(6.)).py(px(3.)).ml(px(-6.)).border_2().border_color(theme.primary).child(el).into_any_element(),
        }
    }

    fn render_item(&mut self, ix: usize, jx: usize, item: &Item, open_tools: &HashSet<(usize, usize)>, open_thoughts: &HashSet<(usize, usize)>, open_subagents: &HashSet<(usize, usize)>, session: &Session, cx: &mut Context<Self>) -> AnyElement {
        let body_px = self.cfg.app.chat_px();
        match item {
            Item::Text { md, .. } => div().w_full().text_size(px(body_px)).line_height(relative(1.5)).child(md_view(format!("t-{ix}-{jx}"), md.clone(), cx)).into_any_element(),
            Item::Thinking { md, seconds, .. } => self.render_thought(ix, jx, md, *seconds, open_thoughts.contains(&(ix, jx)), cx),
            Item::Notice { text, variant, .. } => self.render_notice(text, *variant, cx),
            Item::Tool(call) => self.render_tool(ix, jx, call, open_tools.contains(&(ix, jx)), open_subagents.contains(&(ix, jx)), &session.cwd, cx),
        }
    }

    /// A run of tool calls as one row: how many, of what kinds, the last
    /// subject, the time they took, and a turning mark while one is still
    /// running. Opening it shows every call, each folding on its own.
    fn render_run(&mut self, ix: usize, start: usize, end: usize, open: bool, open_tools: &HashSet<(usize, usize)>, open_thoughts: &HashSet<(usize, usize)>, open_subagents: &HashSet<(usize, usize)>, session: &Session, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let rnd = &session.rounds[ix];
        let items = &rnd.items[start..end];
        let mut kinds: Vec<(String, usize)> = Vec::new();
        let mut total_ms = 0u64;
        let mut last_subject = String::new();
        let mut running = false;
        let mut errors = 0;
        let mut count = 0;
        for it in items {
            if let Item::Tool(c) = it {
                count += 1;
                total_ms += c.duration_ms;
                let k = tool_label(&c.name);
                match kinds.iter_mut().find(|(n, _)| *n == k) {
                    Some(e) => e.1 += 1,
                    None => kinds.push((k, 1)),
                }
                if c.status == CallStatus::Pending {
                    running = true;
                    last_subject = format!("{} {}", c.name, c.subject);
                } else if !running {
                    last_subject = format!("{} {}", c.name, c.subject);
                }
                if c.status == CallStatus::Error {
                    errors += 1;
                }
            }
        }
        kinds.sort_by(|a, b| b.1.cmp(&a.1));
        let tally = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(" · ");
        // A folded run with a find hit inside says so on its edge, so the
        // hit is not invisible until the run is opened.
        let run_mark = (start..end).map(|jx| self.find_mark(ix, Some(jx))).max().unwrap_or(0);
        let head = h_flex()
            .id(SharedString::from(format!("run-{ix}-{start}")))
            .w_full()
            .px(px(10.))
            .py(px(7.))
            .gap(px(8.))
            .items_center()
            .rounded(px(10.))
            .bg(theme.muted)
            .when(run_mark == 1 && !open, |d| d.border_1().border_color(theme.primary.opacity(0.45)))
            .when(run_mark == 2 && !open, |d| d.border_2().border_color(theme.primary))
            .cursor_pointer()
            .hover(|s| s.bg(theme.list_active))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pin_scroll();
                if let Some(d) = this.detail.as_mut() {
                    if !d.open_runs.remove(&(ix, start)) {
                        d.open_runs.insert((ix, start));
                    }
                }
                cx.notify();
            }))
            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(12.)).text_color(theme.muted_foreground))
            .child(if running {
                agent_glyph(session.agent, px(13.), agent_color(session.agent, &theme), true, SharedString::from(format!("run-glyph-{ix}-{start}")))
            } else {
                Icon::new(IconName::SquareTerminal).with_size(px(13.)).text_color(theme.muted_foreground).into_any_element()
            })
            .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).flex_shrink_0().child(format!("{count} tool calls")))
            .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).flex_shrink_0().child(tally))
            .child(div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(theme.muted_foreground).child(last_subject))
            .when(errors > 0, |d| d.child(div().text_size(px(11.5)).text_color(theme.danger).flex_shrink_0().child(format!("{errors} failed"))))
            .when(running, |d| d.child(div().text_size(px(11.5)).text_color(agent_color(session.agent, &theme)).flex_shrink_0().child("running…")))
            .when(!running && total_ms > 1500, |d| d.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).flex_shrink_0().child(human_duration(total_ms))));
        let mut v = v_flex().w_full().gap(px(6.)).child(head);
        if open {
            let mut inner = v_flex().w_full().gap(px(6.)).pl(px(12.)).border_l_2().border_color(theme.border);
            let session_rc = session.clone();
            for jx in start..end {
                let item = session_rc.rounds[ix].items[jx].clone();
                let el = self.render_item(ix, jx, &item, open_tools, open_thoughts, open_subagents, session, cx);
                inner = inner.child(self.find_wrap(ix, jx, el, &theme));
            }
            v = v.child(inner);
        }
        v.into_any_element()
    }

    /// A pasted or attached file inside a prompt: a thumbnail for a picture
    /// (from the file it was kept as, or read back out of the transcript), a
    /// typed icon with name and size for anything else.
    fn render_attachment(&mut self, ix: usize, a: &Attachment, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let title = if a.name.is_empty() { "image".to_string() } else { a.name.clone() };
        let path = (!a.path.is_empty()).then(|| PathBuf::from(&a.path));
        let thumb_key = format!("{}:{}", a.uuid, a.index);
        let is_image = a.kind == "image";
        let (t2, p2, k2) = (title.clone(), path.clone(), thumb_key.clone());
        // Resolve the picture, and its size, before the click listener
        // borrows `cx`. The size shapes the tile so the whole picture shows.
        let mut size: Option<(u32, u32)> = None;
        let source: Option<ImageSource> = if !is_image {
            None
        } else if let Some(p) = path.as_ref().filter(|p| p.is_file()) {
            size = self.kept_image_size(p);
            Some(ImageSource::from(p.clone()))
        } else if !a.uuid.is_empty() {
            self.thumb(ix, &a.uuid, a.index, cx).map(|t| {
                size = t.size;
                ImageSource::from(t.image)
            })
        } else {
            None
        };
        // A picture's tile is square-cornered: the picture itself has sharp
        // corners, and a rounded frame around a sharp picture reads as a
        // mismatch. File chips keep their rounding.
        let frame = |el: AnyElement| {
            div()
                .id(SharedString::from(format!("att-{ix}-{}-{}", a.index, a.name)))
                .when(!is_image, |d| d.rounded(px(10.)))
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .overflow_hidden()
                .cursor_pointer()
                .hover(|s| s.border_color(theme.primary))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let image = if !is_image {
                        None
                    } else if let Some(p) = p2.as_ref().filter(|p| p.is_file()) {
                        Some(ImageSource::from(p.clone()))
                    } else {
                        this.detail.as_ref().and_then(|d| d.thumbs.get(&k2).cloned().flatten()).map(|t| ImageSource::from(t.image))
                    };
                    if is_image && image.is_none() {
                        return;
                    }
                    this.preview_attachment(t2.clone(), p2.clone(), image, window, cx);
                }))
                .child(el)
        };
        if a.kind == "image" {
            return match source {
                Some(src) => {
                    // The tile takes the picture's own shape, up to 240 by
                    // 180, so nothing is cropped away; a picture whose size
                    // the header did not give gets the old box, letterboxed.
                    let (w, h) = fit_thumb(size, 240., 180., (176., 118.));
                    let caption = if a.name.is_empty() { "image".to_string() } else { a.name.clone() };
                    frame(
                        v_flex()
                            .child(img(src).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain))
                            .child(tile_caption(w, caption, &theme))
                            .into_any_element(),
                    )
                    .into_any_element()
                }
                None => frame(
                    h_flex().h(px(48.)).px(px(12.)).gap(px(8.)).items_center().text_size(px(12.)).text_color(theme.muted_foreground).child(file_icon("x.png", px(18.), theme.muted_foreground)).child(if a.name.is_empty() { "image".to_string() } else { a.name.clone() }).into_any_element(),
                )
                .into_any_element(),
            };
        }
        frame(
            h_flex()
                .h(px(48.))
                .pl(px(8.))
                .pr(px(12.))
                .gap(px(8.))
                .items_center()
                .child(div().size(px(32.)).rounded(px(8.)).bg(theme.muted).flex().items_center().justify_center().child(file_icon(&a.name, px(18.), theme.muted_foreground)))
                .child(
                    v_flex()
                        .gap(px(1.))
                        .text_size(px(12.))
                        .child(div().max_w(px(220.)).truncate().font_weight(FontWeight::MEDIUM).child(a.name.clone()))
                        .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(format!("{} · {}", file_kind(&a.name), human_size(a.size)))),
                )
                .into_any_element(),
        )
        .into_any_element()
    }

    fn render_notice(&self, text: &str, variant: NoticeVariant, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (bg, fg) = match variant {
            NoticeVariant::Command => (theme.muted, theme.foreground),
            NoticeVariant::Compact => (theme.info.opacity(0.15), theme.foreground),
            NoticeVariant::Error => (theme.danger.opacity(0.15), theme.danger),
            _ => (theme.muted, theme.muted_foreground),
        };
        h_flex()
            .child(div().px(px(8.)).py(px(2.)).rounded(px(6.)).bg(bg).text_color(fg).text_size(px(11.5)).when(variant == NoticeVariant::Command, |d| d.font_family(theme.mono_font_family.clone())).child(text.to_string()))
            .into_any_element()
    }

    fn render_thought(&self, ix: usize, jx: usize, md: &str, seconds: f64, open: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let body_px = self.cfg.app.chat_px();
        let label = if seconds >= 2.0 { format!("Thought for {}", human_duration((seconds * 1000.0) as u64)) } else { "Thinking".to_string() };
        v_flex()
            .w_full()
            .gap(px(4.))
            .child(
                h_flex()
                    .id(SharedString::from(format!("th-{ix}-{jx}")))
                    .gap(px(6.))
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(d) = this.detail.as_mut() {
                            if !d.open_thoughts.remove(&(ix, jx)) {
                                d.open_thoughts.insert((ix, jx));
                            }
                        }
                        cx.notify();
                    }))
                    .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(12.)))
                    .child(div().italic().child(label)),
            )
            .when(open, |d| {
                d.child(
                    div()
                        .pl(px(14.))
                        .border_l_2()
                        .border_color(theme.border)
                        .text_size(px(body_px - 2.))
                        .text_color(theme.muted_foreground)
                        .line_height(relative(1.55))
                        .child(md_view(format!("tk-{ix}-{jx}"), md.to_string(), cx)),
                )
            })
            .into_any_element()
    }

    fn render_tool(&mut self, ix: usize, jx: usize, call: &ToolCall, open: bool, sub_open: bool, cwd: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (status_text, status_color) = match call.status {
            CallStatus::Ok => ("", theme.muted_foreground),
            CallStatus::Error => ("error", theme.danger),
            CallStatus::Interrupted => ("interrupted", theme.warning),
            CallStatus::NoResult => ("not run", theme.muted_foreground),
            CallStatus::Pending => ("running…", theme.primary),
        };
        let label = tool_label(&call.name);
        let stat = if call.tool_kind == ToolKind::Edit { patch_stat(&call.patch) } else { String::new() };
        let duration = if call.duration_ms > 1500 { human_duration(call.duration_ms) } else { String::new() };
        let has_body = self.tool_has_body(call);
        let subject = call.subject.clone();
        let explanation = self.explanation_for(call);
        let explaining = self.explaining.contains(&call.id);
        let ex_open = self.detail.as_ref().map(|d| d.open_explanations.contains(&call.id)).unwrap_or(false);
        let can_explain = self.hub.explainer.cfg().enabled;
        let pill = {
            let (id, cname, input, has_text) = (call.id.clone(), call.name.clone(), call.input.clone(), !explanation.is_empty());
            // Off is an outline in the muted ink; on is filled with the
            // accent tint. The switch between them is drawn over a moment,
            // keyed on the state so each click plays it once.
            let (off_border, off_bg, off_ink) = (theme.border, theme.transparent, theme.muted_foreground);
            let (on_border, on_bg, on_ink) = (theme.primary.opacity(0.6), theme.primary.opacity(0.14), theme.primary);
            let anim: SharedString = format!("explain-{ix}-{jx}-{}", if ex_open { "on" } else { "off" }).into();
            h_flex()
                .id(SharedString::from(format!("explain-{ix}-{jx}")))
                .flex_shrink_0()
                .h(px(20.))
                .px(px(8.))
                .items_center()
                .rounded_full()
                .border_1()
                .cursor_pointer()
                .text_size(px(11.))
                .when(!ex_open, |d| d.hover(|s| s.border_color(theme.primary.opacity(0.6)).bg(theme.primary.opacity(0.10)).text_color(theme.foreground)))
                .active(|s| s.bg(theme.primary.opacity(0.22)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.toggle_explain(&id, &cname, &input, has_text, cx);
                }))
                .child(if ex_open && explaining && !has_text { "Explaining…" } else { "Explain" })
                .with_animation(ElementId::Name(anim), Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()), move |d, t| {
                    let t = if ex_open { t } else { 1. - t };
                    d.border_color(mix(off_border, on_border, t)).bg(mix(off_bg, on_bg, t)).text_color(mix(off_ink, on_ink, t))
                })
        };

        let mut card = v_flex().w_full().rounded(px(10.)).border_1().border_color(theme.border).bg(theme.popover).overflow_hidden();
        card = card.child(
            h_flex()
                .id(SharedString::from(format!("tool-{ix}-{jx}")))
                .w_full()
                .px(px(10.))
                .py(px(6.))
                .gap(px(8.))
                .items_center()
                .when(has_body, |d| d.cursor_pointer().hover(|s| s.bg(theme.list_hover)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !has_body {
                        return;
                    }
                    this.pin_scroll();
                    if let Some(d) = this.detail.as_mut() {
                        if !d.open_tools.remove(&(ix, jx)) {
                            d.open_tools.insert((ix, jx));
                        }
                    }
                    cx.notify();
                }))
                .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(12.)).text_color(if has_body { theme.muted_foreground } else { theme.border }))
                .child(div().flex_shrink_0().child(badge_str(label, theme.muted, theme.muted_foreground)))
                .child(div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(12.)).text_color(theme.muted_foreground).child(subject))
                .when(!stat.is_empty(), |d| d.child(div().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(theme.muted_foreground).child(stat)))
                .when(!status_text.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(status_color).child(status_text)))
                .when(!duration.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(duration)))
                .when(can_explain, |d| d.child(pill)),
        );
        if ex_open {
            let text = if explanation.is_empty() && !explaining { "Nothing came back from the model. Click Explain again to retry.".to_string() } else { explanation };
            let size = px(self.cfg.app.chat_px());
            card = card.child(div().w_full().px(px(10.)).pb(px(8.)).child(explain_card(format!("ex-{ix}-{jx}"), text, explaining, size, cx)));
        }
        if open && has_body {
            // The body is bounded and scrolls inside the card, so a long
            // output never takes the whole window; the clip's "n more
            // lines" note still says what was left out entirely.
            let body = self.render_tool_body(ix, jx, call, sub_open, cwd, cx);
            // The scroll handle lives in the detail, so the position and the
            // bar survive the redraw every frame brings.
            let scroll = self.detail.as_mut().map(|d| d.body_scrolls.entry((ix, jx)).or_default().clone()).unwrap_or_default();
            let handle = scroll.handle.clone();
            // The wheel over a body that scrolls belongs to the body: the
            // list would otherwise take the same event (gpui hands a scroll
            // to every scrollable under the mouse) and both would move.
            // Whose a stroke is gets decided as it starts (see `Stroke`):
            // the body's own handler has run by the time this one does, so
            // the offset before it is `at - dy`, and an edge there in the
            // wheel's direction means the body had nothing to give and the
            // conversation takes the stroke. A body that fits passes
            // everything on.
            let wheel_handle = handle.clone();
            let stroke = scroll.stroke.clone();
            card = card.child(
                div()
                    .relative()
                    .w_full()
                    .border_t_1()
                    .border_color(theme.border)
                    .on_scroll_wheel(move |event, window, cx| {
                        let max = wheel_handle.max_offset().y;
                        if max <= px(0.) {
                            return;
                        }
                        let now = std::time::Instant::now();
                        let mut s = stroke.borrow_mut();
                        let touched = matches!(event.touch_phase, gpui::TouchPhase::Started);
                        let paused = s.last.map(|t| now.duration_since(t) > crate::workbench::STROKE_GAP).unwrap_or(true);
                        s.last = Some(now);
                        if touched || paused {
                            s.decided = false;
                            s.handed_over = false;
                        }
                        if !s.decided {
                            let dy = event.delta.pixel_delta(window.line_height()).y;
                            if dy != px(0.) {
                                let before = wheel_handle.offset().y - dy;
                                let at_bottom = before <= -max;
                                let at_top = before >= px(0.);
                                s.handed_over = (dy < px(0.) && at_bottom) || (dy > px(0.) && at_top);
                                s.decided = true;
                            }
                        }
                        if !s.handed_over {
                            cx.stop_propagation();
                        }
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("tool-body-{ix}-{jx}")))
                            .w_full()
                            .max_h(MAX_BODY_H)
                            .overflow_y_scroll()
                            .track_scroll(&handle)
                            .child(div().w_full().px(px(10.)).pt(px(8.)).pb(px(10.)).child(body)),
                    )
                    .vertical_scrollbar(&handle),
            );
        }
        card.into_any_element()
    }

    fn tool_has_body(&self, call: &ToolCall) -> bool {
        match call.tool_kind {
            // A Read opens on what came back: the text, or the picture.
            ToolKind::Read => call.result_images > 0 || !call.result_text.trim().is_empty(),
            ToolKind::Task => !call.subagent.is_empty() || !call.result_text.is_empty() || call.input.get("prompt").is_some(),
            _ => true,
        }
    }

    fn render_tool_body(&mut self, ix: usize, jx: usize, call: &ToolCall, sub_open: bool, _cwd: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let data = &call.input;
        let mut parts = v_flex().w_full().gap(px(8.)).text_size(px(12.5));
        match call.tool_kind {
            ToolKind::Read => {
                if call.result_images > 0 {
                    // The picture the Read returned, read back out of the
                    // transcript like a pasted one; a click opens it large.
                    match self.thumb(ix, &call.result_uuid, call.result_index, cx) {
                        Some(t) => {
                            let (w, h) = fit_thumb(t.size, 320., 240., (176., 118.));
                            let image = ImageSource::from(t.image.clone());
                            // The same square tile as a pasted picture, the
                            // file's name under it. The lightbox gets the
                            // file's full path, so it can open and reveal it.
                            let file = data.get("file_path").and_then(|v| v.as_str()).filter(|p| !p.is_empty()).map(PathBuf::from);
                            let title = file.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| call.subject.clone());
                            let caption = std::path::Path::new(&call.subject).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| call.subject.clone());
                            parts = parts.child(
                                div()
                                    .id(SharedString::from(format!("read-img-{ix}-{jx}")))
                                    .w(px(w))
                                    .border_1()
                                    .border_color(theme.border)
                                    .overflow_hidden()
                                    .cursor_pointer()
                                    .hover(|s| s.border_color(theme.primary))
                                    .on_click(cx.listener(move |this, _, window, cx| this.preview_attachment(title.clone(), file.clone(), Some(image.clone()), window, cx)))
                                    .child(img(ImageSource::from(t.image)).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain))
                                    .child(tile_caption(w, caption, &theme)),
                            );
                        }
                        None => parts = parts.child(div().text_color(theme.muted_foreground).italic().child("loading the picture…")),
                    }
                } else if !call.result_text.trim().is_empty() {
                    let lang = emaki_core::render_md::lang_for_path(data.get("file_path").and_then(|v| v.as_str()).unwrap_or(""));
                    parts = parts.child(mono_block(format!("res-{ix}-{jx}"), &call.result_text, lang, cx));
                }
            }
            ToolKind::Bash => {
                let command = command_text(data);
                if !command.is_empty() {
                    parts = parts.child(mono_block(format!("cmd-{ix}-{jx}"), &command, "bash", cx));
                }
                let out = if !call.stdout.trim().is_empty() { call.stdout.clone() } else { call.result_text.clone() };
                if !out.trim().is_empty() {
                    parts = parts.child(mono_block(format!("out-{ix}-{jx}"), &out, "text", cx));
                } else if call.status == CallStatus::Ok {
                    parts = parts.child(div().text_color(theme.muted_foreground).italic().child("no output"));
                }
                if !call.stderr.trim().is_empty() {
                    parts = parts.child(div().text_size(px(11.)).text_color(theme.muted_foreground).child("stderr"));
                    parts = parts.child(mono_block(format!("err-{ix}-{jx}"), &call.stderr, "text", cx));
                }
            }
            ToolKind::Edit => {
                let diff = render_patch(&call.patch, 12);
                if !diff.is_empty() {
                    parts = parts.child(mono_block(format!("diff-{ix}-{jx}"), &diff, "diff", cx));
                } else if !call.old_string.is_empty() || !call.new_string.is_empty() {
                    parts = parts.child(mono_block(format!("diff-{ix}-{jx}"), &emaki_core::render_md::naive_diff(&call.old_string, &call.new_string, 80), "diff", cx));
                } else if let Some(p) = data.get("patch").and_then(|v| v.as_str()) {
                    parts = parts.child(mono_block(format!("diff-{ix}-{jx}"), p, "diff", cx));
                }
                if call.status == CallStatus::Error && !call.result_text.is_empty() {
                    parts = parts.child(mono_block(format!("res-{ix}-{jx}"), &call.result_text, "text", cx));
                }
            }
            ToolKind::Write => {
                let content = data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let lang = emaki_core::render_md::lang_for_path(data.get("file_path").and_then(|v| v.as_str()).unwrap_or(""));
                parts = parts.child(mono_block(format!("w-{ix}-{jx}"), content, lang, cx));
            }
            ToolKind::Task => {
                if let Some(p) = data.get("prompt").or(data.get("description")).and_then(|v| v.as_str()) {
                    let (snippet, _) = clip(p, 700);
                    parts = parts.child(div().pl(px(10.)).border_l_2().border_color(theme.border).text_color(theme.muted_foreground).child(md_view(format!("tp-{ix}-{jx}"), snippet, cx)));
                }
                if !call.subagent.is_empty() {
                    let n = call.subagent.len();
                    parts = parts.child(
                        h_flex()
                            .id(SharedString::from(format!("sub-{ix}-{jx}")))
                            .gap(px(6.))
                            .items_center()
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pin_scroll();
                                if let Some(d) = this.detail.as_mut() {
                                    if !d.open_subagents.remove(&(ix, jx)) {
                                        d.open_subagents.insert((ix, jx));
                                    }
                                }
                                cx.notify();
                            }))
                            .child(Icon::new(if sub_open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(12.)))
                            .child(div().child(format!("{}{} · {} round{}", if call.agent_name.is_empty() { "subagent" } else { &call.agent_name }, "", n, if n == 1 { "" } else { "s" }))),
                    );
                    if sub_open {
                        let mut nested = v_flex().w_full().gap(px(10.)).pl(px(10.)).border_l_2().border_color(theme.border);
                        for (k, sub) in call.subagent.iter().enumerate() {
                            nested = nested.child(self.render_subround(ix, jx, k, sub, cx));
                        }
                        parts = parts.child(nested);
                    }
                } else if !call.result_text.is_empty() {
                    parts = parts.child(mono_block(format!("res-{ix}-{jx}"), &call.result_text, "text", cx));
                }
            }
            _ => {
                let args = pretty_args(data, 3000);
                if !args.is_empty() {
                    parts = parts.child(mono_block(format!("args-{ix}-{jx}"), &args, "json", cx));
                }
                if !call.result_text.trim().is_empty() {
                    parts = parts.child(mono_block(format!("res-{ix}-{jx}"), &call.result_text, "text", cx));
                } else if call.result_images > 0 {
                    parts = parts.child(div().text_color(theme.muted_foreground).italic().child(format!("{} image{} returned", call.result_images, if call.result_images == 1 { "" } else { "s" })));
                }
            }
        }
        parts.into_any_element()
    }

    /// A subagent's round, drawn flat: prompt, then text and a one-line
    /// summary per tool call. Deeper nesting is summarised, not expanded.
    fn render_subround(&self, ix: usize, jx: usize, k: usize, sub: &Round, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut v = v_flex().w_full().gap(px(6.));
        if !sub.prompt.is_empty() {
            let (snippet, _) = clip(&sub.prompt, 1200);
            v = v.child(div().px(px(10.)).py(px(6.)).rounded(px(8.)).bg(theme.muted).text_size(px(12.)).child(md_view(format!("sp-{ix}-{jx}-{k}"), snippet, cx)));
        }
        for (m, item) in sub.items.iter().enumerate() {
            match item {
                Item::Text { md, .. } => {
                    v = v.child(div().text_size(px(12.5)).line_height(relative(1.55)).child(md_view(format!("st-{ix}-{jx}-{k}-{m}"), md.clone(), cx)));
                }
                Item::Tool(c) => {
                    v = v.child(
                        h_flex()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(theme.muted_foreground)
                            .child(div().flex_shrink_0().child(badge_str(tool_label(&c.name), theme.muted, theme.muted_foreground)))
                            .child(div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).child(c.subject.clone())),
                    );
                }
                Item::Thinking { seconds, .. } => {
                    v = v.child(div().text_size(px(11.5)).italic().text_color(theme.muted_foreground).child(if *seconds >= 2.0 { format!("thought for {}", human_duration((seconds * 1000.0) as u64)) } else { "thinking".into() }));
                }
                Item::Notice { text, .. } => {
                    v = v.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(text.clone()));
                }
            }
        }
        v.into_any_element()
    }
}
