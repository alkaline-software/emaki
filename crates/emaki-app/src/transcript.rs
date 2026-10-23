//! Drawing one round of a session: the prompt, then every item the agent
//! produced. Tool calls and thoughts fold; a Task call unfolds into the
//! subagent's own rounds.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};
use emaki_core::model::*;
use emaki_core::render_md::{clip, code_block, command_text, human_duration, patch_stat, pretty_args, render_patch};

use crate::format::stamp;
use crate::i18n::{count, t, tf};
use std::path::PathBuf;
use std::time::Duration;

use std::collections::HashSet;

use crate::workbench::{agent_color, agent_glyph, agent_icon, badge_str, file_kind, fit_thumb, human_size, swallow_click, Workbench, CONTENT_W, OUTGOING};

const MAX_BODY: usize = 6000;
/// An opened tool call shows at most this much before it scrolls inside
/// its card, so one long output never takes the whole window.
const MAX_BODY_H: Pixels = px(400.);
/// How far a reply's right edge stays inside the prompts' right edge.
/// A prompt's bubble stops as short of the left edge, so a long
/// prompt, which fills the width it is given, still reads as coming from
/// the right. It is a share of the column, which is as narrow as the
/// panes beside the conversation leave it.
const REPLY_INSET: f32 = 0.1;
/// How long an Explain button takes to change between off and on.
const EXPLAIN_FLIP: Duration = Duration::from_millis(160);
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
fn markdown_is_safe(text: &str, chat: bool) -> bool {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    thread_local! {
        static SEEN: std::cell::RefCell<HashMap<u64, bool>> = std::cell::RefCell::new(HashMap::new());
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (text, chat).hash(&mut h);
    let key = h.finish();
    if let Some(known) = SEEN.with(|s| s.borrow().get(&key).copied()) {
        return known;
    }
    // The options the view parses with, or the check proves nothing.
    let mut opts = markdown::ParseOptions::gfm();
    if chat {
        chat_parse_options(&mut opts);
    }
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

/// What a conversation reads as markdown: GFM, less what chat sets
/// down by accident. A prompt is typed as plain text, and a reply is
/// read in a terminal as well, where none of these are drawn; so a sign
/// that prose uses for something else is left as it is written.
///
/// - A line of `-` or `=` under a line of text is a line of its own, not
///   the line above turned into a heading (setext). `---` after text is
///   then a rule, as a model writing it means.
/// - Text indented four spaces is text, not a block of code. Code comes
///   between fences.
/// - A tag in a sentence (`<Input>`, `<div>`) is its words: raw HTML was
///   parsed as a tag and drawn as nothing.
/// - One tilde is a tilde ("~30% … ~40%"): strikethrough takes two.
pub(crate) fn chat_parse_options(opts: &mut markdown::ParseOptions) {
    opts.constructs.heading_setext = false;
    opts.constructs.code_indented = false;
    opts.constructs.html_flow = false;
    opts.constructs.html_text = false;
    opts.gfm_strikethrough_single_tilde = false;
}

/// The conversation's markdown options, made once. A set made anew
/// takes a new revision, and the view reads its text again at every
/// new revision: made at each draw, every text was parsed again every
/// frame, and a selection was gone the moment the mouse let go.
static CHAT_MARKDOWN: std::sync::LazyLock<gpui_component::text::MarkdownExtensions> = std::sync::LazyLock::new(|| gpui_component::text::MarkdownExtensions::default().parse_options_with(chat_parse_options));

/// Markdown as a conversation draws it: `md_view` read with
/// `chat_parse_options`, and with the code among its words in the
/// smaller cut of the monospace face (`fonts::inline_mono_family`), so
/// it stands no taller than they do.
pub(crate) fn chat_md_view(id: String, text: String, cx: &App) -> impl IntoElement {
    md_view_in(id, text, crate::fonts::inline_mono_family(cx), true, None, cx)
}

/// Markdown as a document draws it, a file's or a page's: all of GFM,
/// as GitHub reads the file.
pub(crate) fn md_view(id: String, text: String, cx: &App) -> impl IntoElement {
    md_view_in(id, text, cx.theme().mono_font_family.clone(), false, None, cx)
}

/// `md_view`, with words found in it painted (`TextFind`): a file as it
/// reads, with its find open.
pub(crate) fn md_view_find(id: String, text: String, find: Option<gpui_component::text::TextFind>, cx: &App) -> impl IntoElement {
    md_view_in(id, text, cx.theme().mono_font_family.clone(), false, find, cx)
}

fn md_view_in(id: String, text: String, inline_mono: SharedString, chat: bool, find: Option<gpui_component::text::TextFind>, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    // A text the parser cannot take is shown as it is, in a code block,
    // rather than taking the window down.
    let text = if markdown_is_safe(&text, chat) { text } else { code_block(&text, "text") };
    let dark = theme.mode.is_dark();
    let code_bg = theme.muted;
    let border = theme.border;
    TextView::markdown(SharedString::from(format!("{id}-{}", if dark { "d" } else { "l" })), text)
        .selectable(true)
        .find(find)
        .when(chat, |v| v.markdown_extensions(CHAT_MARKDOWN.clone()))
        .style(
            TextViewStyle {
                // The headings' sizes go from the text's own where the view
                // stands (the vendored `node.rs`): set from a fixed size,
                // they stayed as they were when the text was made larger.
                heading_base_font_size: Pixels::ZERO,
                // The toolkit's sizes, but for the first level: twice
                // the text's was a poster's headline over a reply.
                heading_font_size: Some(std::sync::Arc::new(|level, base| {
                    base * match level {
                        1 => 1.7,
                        2 => 1.5,
                        3 => 1.25,
                        4 => 1.125,
                        _ => 1.,
                    }
                })),
                paragraph_gap: rems(0.6),
                // The window's own code colors, the same the file's
                // editor has (`look::Inks`).
                highlight_theme: theme.highlight_theme.clone(),
                // As the Claude app sets them: strong text at 600, not the
                // font's true Bold; inline code in the mono face. These and
                // the plate are Emaki's additions to the vendored toolkit.
                strong_font_weight: Some(FontWeight::SEMIBOLD),
                inline_code_font_family: Some(inline_mono),
                // Inline code as the Claude app draws it, in the accent
                // config asks for: the readable shade of it for the
                // letters, a faint wash of it on a rounded plate behind.
                inline_code: HighlightStyle { color: Some(theme.link), ..Default::default() },
                inline_code_chip: Some((theme.primary.opacity(if dark { 0.16 } else { 0.10 }), theme.primary.opacity(if dark { 0.22 } else { 0.18 }))),
                ..Default::default()
            }
            .code_block(StyleRefinement::default().bg(code_bg).border_1().border_color(border).rounded(px(10.)).px(px(12.)).py(px(10.))),
        )
        // A block of code is copied whole with the button at its top
        // right, which shows under the pointer (the vendored `node.rs`).
        .code_block_actions(|block, _, _| {
            gpui_component::clipboard::Clipboard::new("copy").value(block.code()).on_copied(|_, window, cx| gpui_base::TextSelection::end(window, cx))
        })
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
        .when(pending && text.is_empty(), |d| d.child(div().italic().text_size(size - px(1.)).text_color(theme.muted_foreground).child(t("Explaining…"))))
        .when(!(pending && text.is_empty()), |d| d.child(div().text_size(size).text_color(theme.foreground).line_height(relative(1.6)).child(chat_md_view(id, text, cx))))
        .with_animation(ElementId::Name(anim), Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()), |d, t| d.opacity(t).mt(px(-4. * (1. - t))))
}

/// The strip under a picture tile: its name, centerd, in the mono face on
/// a muted ground, wrapping when it is long.
/// The box a prompt's picture is drawn in: `GALLERY_H` tall, as wide as
/// its shape gives within `GALLERY_W`, so pictures of every shape stand
/// in one even row. One whose size is not known gets a 4:3 box.
pub(crate) fn gallery_box(size: Option<(u32, u32)>) -> (f32, f32) {
    let w = match size {
        Some((w, h)) if w > 0 && h > 0 => GALLERY_H * w as f32 / h as f32,
        _ => GALLERY_H * 4. / 3.,
    };
    (w.clamp(GALLERY_W.0, GALLERY_W.1), GALLERY_H)
}

const GALLERY_H: f32 = 132.;
const GALLERY_W: (f32, f32) = (84., 264.);

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
        .truncate()
        .child(name)
}

/// Straight-line mix of two colors, for a state change drawn over a moment.
fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    Hsla { h: a.h + (b.h - a.h) * t, s: a.s + (b.s - a.s) * t, l: a.l + (b.l - a.l) * t, a: a.a + (b.a - a.a) * t }
}

fn mono_block(id: String, text: &str, lang: &str, cx: &App) -> impl IntoElement {
    let (body, dropped) = clip(text, MAX_BODY);
    let mut md = code_block(&body, lang);
    if dropped > 0 {
        md.push_str(&format!("\n\n*{}*", count(dropped, "… {n} more line", "… {n} more lines")));
    }
    chat_md_view(id, md, cx)
}

/// The plan an `ExitPlanMode` call asks to go ahead with, as markdown.
fn plan_of(call: &ToolCall) -> Option<&str> {
    (call.name == "ExitPlanMode").then(|| call.input.get("plan").and_then(|v| v.as_str())).flatten().filter(|p| !p.trim().is_empty())
}

/// What folds into a run: tool calls and the thoughts between them. A
/// question is never one of them, so it is never hidden in "n tool
/// calls", and neither is a plan.
fn is_run_item(it: &Item) -> bool {
    match it {
        Item::Tool(c) => c.tool_kind != ToolKind::Ask && plan_of(c).is_none(),
        Item::Thinking { .. } => true,
        _ => false,
    }
}

/// Where the folded run of tool calls holding item `jx` begins, when there
/// is one: three or more tool calls in a row, thoughts between them
/// included, fold into one row (see `render_round`) while runs fold
/// (`fold`, the setting), and the find bar has to open that row to show
/// a hit inside it.
pub(crate) fn run_start(rnd: &Round, jx: usize, fold: bool) -> Option<usize> {
    let is_run_item = |it: &Item| is_run_item(it);
    if !fold || !rnd.items.get(jx).map(is_run_item).unwrap_or(false) {
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

/// A tool call's badge, in the color of what the call does: running a
/// command, reading, searching, changing a file, writing one, the web, an
/// agent. The inks are code's own (`look::Inks`), each on a wash of
/// itself, so a run of calls reads by color before it is read by word.
/// What is none of those keeps the muted gray.
fn tool_badge(call: &ToolCall, theme: &gpui_component::Theme) -> impl IntoElement {
    let k = if theme.mode.is_dark() { &crate::look::INKS_DARK } else { &crate::look::INKS_LIGHT };
    let ink: Option<Hsla> = match call.tool_kind {
        ToolKind::Bash => Some(rgb(k.gold).into()),
        ToolKind::Read => Some(rgb(k.blue).into()),
        ToolKind::Search => Some(rgb(k.teal).into()),
        ToolKind::Edit => Some(rgb(k.violet).into()),
        ToolKind::Write => Some(rgb(k.green).into()),
        ToolKind::Web => Some(rgb(k.rose).into()),
        ToolKind::Task => Some(theme.primary),
        _ => None,
    };
    match ink {
        Some(ink) => badge_str(tool_label(&call.name), ink.opacity(0.14), ink),
        None => badge_str(tool_label(&call.name), theme.muted, theme.muted_foreground),
    }
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
            Source::Peer => Some(t("Another session")),
            Source::System => Some(t("Session")),
            _ => None,
        };
        let sent = stamp(&rnd.ts);
        // A message sent from here that the transcript does not have yet,
        // and why it did not go when it did not.
        let undelivered = (rnd.uuid == OUTGOING).then(|| self.outgoing.get(&detail.key).map(|o| o.error.clone())).flatten().filter(|e| !e.is_empty());

        let mut column = v_flex().w_full().max_w(CONTENT_W).px(px(24.)).pt(px(14.)).pb(if is_last { px(28.) } else { px(6.) }).gap(px(12.));

        // The prompt, as a bubble on the right: what was attached first
        // (pictures, then files), the words under them, the way a message
        // with a picture reads in the Claude app.
        if !rnd.prompt.is_empty() || rnd.images > 0 || !rnd.attachments.is_empty() {
            let mut bubble = v_flex()
                .relative()
                .min_w_0()
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
                .when(prompt_mark == 2, |d| d.border_2().border_color(theme.primary))
                // The tail at its foot, which says the bubble is from the
                // right: a drawing of the bubble's own color, hanging
                // past its edge.
                .child(svg().path("icons/bubble-tail.svg").absolute().right(px(-7.)).bottom_0().w(px(20.)).h(px(25.)).text_color(theme.muted));
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
                bubble = bubble.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(count(rnd.images as usize, "+ {n} pasted image", "+ {n} pasted images")));
            }
            if has_text {
                // A command the prompt names (`/compact`, or a skill in a
                // sentence) is set as inline code, so it wears the accent.
                let cwd = &session.cwd;
                let prompt = if rnd.prompt.contains('/') { emaki_core::driver::mark_commands(&rnd.prompt, |name| self.hub.knows_command(session.agent, cwd, name)) } else { rnd.prompt.clone() };
                // And an "@" that names a file under the folder.
                let prompt = if prompt.contains('@') { emaki_core::files::mark_mentions(&prompt, cwd) } else { prompt };
                bubble = bubble.child(chat_md_view(format!("p-{ix}"), prompt, cx));
            }
            // The time and the copy button show while the pointer is over
            // the prompt's row, as in the Claude app. Their line is always
            // laid out, so nothing moves when they appear.
            let group = SharedString::from(format!("prompt-{ix}"));
            let copy = has_text.then(|| self.copy_button(ix, false, cx));
            column = column.child(
                // The bubble is in a row of its own, which it shrinks to
                // fit. As an item of this column it took the width of
                // its words up to its most, whatever the column had.
                v_flex().group(group.clone()).w_full().pl(relative(REPLY_INSET)).items_end().gap(px(2.)).child(h_flex().w_full().justify_end().items_start().child(bubble)).child(
                    h_flex()
                        .h(HOVER_ROW_H)
                        .gap(px(6.))
                        .pr(px(2.))
                        .items_center()
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        // A message still in the queue says so, all the
                        // time: it is the one thing under a prompt worth
                        // reading without being asked.
                        .when(!rnd.queued && undelivered.is_none(), |d| d.opacity(0.).group_hover(group, |s| s.opacity(1.)))
                        // So does one that did not go, with the reason
                        // under the pointer. Its words are still here to
                        // be copied.
                        .when_some(undelivered, |d, why| {
                            d.child(
                                h_flex()
                                    .id(SharedString::from(format!("undelivered-{ix}")))
                                    .gap(px(4.))
                                    .items_center()
                                    .text_color(theme.danger)
                                    .child(Icon::default().path("icons/circle-x.svg").with_size(px(14.)).text_color(theme.danger))
                                    .child(t("Not delivered"))
                                    .managed_tooltip(move |window, cx| crate::workbench::tip(why.clone(), window, cx)),
                            )
                        })
                        .when(rnd.queued, |d| d.child(div().child(tf("Queued, {agent} will read it at its next step", &[("agent", &session.agent.speaker())]))))
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
        // A mode changed since the last turn is in no row yet: Claude Code
        // writes it with the next prompt. Said here, in the last round,
        // in the words the transcript's own notice will use once it has
        // one, which then takes this one's place. Each change goes where
        // it happened: before the first line written after it, or at the
        // foot when nothing was, or when its time is not known. With
        // them come the setting lines the transcript folded into a later
        // one of their kind (`Round::superseded`), and the same folding
        // is done again over all of it, so effort, mode, effort reads as
        // three lines and mode, mode, mode as the last.
        let mut extras: Vec<(f64, NoticeVariant, String)> = Vec::new();
        if ix + 1 == session.rounds.len() {
            let modes = self.unwritten_modes();
            if !modes.is_empty() {
                let options = self.options();
                let stamp_of = |ts: &str| chrono::DateTime::parse_from_rfc3339(ts).map(|t| t.timestamp_millis() as f64 / 1000.0).unwrap_or(0.0);
                extras.extend(rnd.superseded.iter().filter_map(|i| if let Item::Notice { ts, text, variant } = i { Some((stamp_of(ts), *variant, text.clone())) } else { None }));
                extras.extend(modes.into_iter().map(|(mode, at)| (at.unwrap_or(f64::INFINITY), NoticeVariant::Mode, crate::workbench::mode_name(&options, &mode))));
                extras.sort_by(|a, b| a.0.total_cmp(&b.0));
            }
        }
        let kind_of = |item: &Item| if let Item::Notice { variant, .. } = item { variant.setting().map(|_| *variant) } else { None };
        let at_of = |item: &Item| chrono::DateTime::parse_from_rfc3339(item.ts()).map(|t| t.timestamp_millis() as f64 / 1000.0).unwrap_or(f64::INFINITY);
        let mut e = 0;
        // Where "New messages" is going to, when that is in this round:
        // a mark of no height before the item, which says where it is
        // drawn at every frame of the move there.
        let mut going = self.unread_go.filter(|(round, _)| *round == ix).map(|(_, item)| item);
        while jx < n {
            any = true;
            if let Some(item) = going {
                // A run of tool calls is one row: the mark goes before it.
                let mut reach = jx + 1;
                while is_run_item(&rnd.items[jx]) && reach < n && is_run_item(&rnd.items[reach]) {
                    reach += 1;
                }
                if item < reach {
                    going = None;
                    let place = self.unread_y.clone();
                    body = body.child(
                        canvas(move |bounds, _, _| place.set(Some(bounds.top())), |_, _, _, _| {})
                        .w_full()
                        .h(px(0.)),
                    );
                }
            }
            // The extra lines from before this item, each unless the line
            // after it is of its own kind.
            let until = at_of(&rnd.items[jx]);
            while e < extras.len() && extras[e].0 < until {
                let next = match extras.get(e + 1).filter(|x| x.0 < until) {
                    Some(x) => Some(x.1),
                    None => kind_of(&rnd.items[jx]),
                };
                if next != Some(extras[e].1) {
                    body = body.child(self.render_notice(&extras[e].2, extras[e].1, cx));
                }
                e += 1;
            }
            if self.cfg.app.fold_runs && is_run_item(&rnd.items[jx]) {
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
            // A written setting line followed by an extra of its kind is
            // folded into that one.
            let folded = kind_of(&rnd.items[jx]).is_some() && extras.get(e).is_some_and(|x| Some(x.1) == kind_of(&rnd.items[jx]) && rnd.items.get(jx + 1).map_or(true, |nx| x.0 < at_of(nx)));
            if !folded {
                let el = self.render_item(ix, jx, &rnd.items[jx], &open_tools, &open_thoughts, &open_subagents, &session, cx);
                body = body.child(self.find_wrap(ix, jx, el, &theme));
            }
            jx += 1;
        }
        while e < extras.len() {
            if extras.get(e + 1).map(|x| x.1) != Some(extras[e].1) {
                any = true;
                body = body.child(self.render_notice(&extras[e].2, extras[e].1, cx));
            }
            e += 1;
        }
        if any {
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
                    .child(h_flex().gap(px(7.)).items_center().child(agent_icon(session.agent, px(15.), agent_color(session.agent, &theme))).child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).child(speaker)))
                    // The reply stops short of the column's right edge, where
                    // the prompt bubbles end: the two voices sit at different
                    // widths, as in the Claude app, and read apart at a glance.
                    .child(div().w_full().pl(px(22.)).pr(relative(REPLY_INSET)).child(body))
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
            .managed_tooltip(move |window, cx| crate::workbench::tip(if done { t("Copied") } else { t("Copy") }, window, cx))
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
            Item::Text { md, .. } => div().w_full().text_size(px(body_px)).line_height(relative(1.5)).child(chat_md_view(format!("t-{ix}-{jx}"), md.clone(), cx)).into_any_element(),
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
        let mut said: Option<String> = None;
        let mut running = false;
        let mut errors = 0;
        let mut calls = 0;
        for it in items {
            if let Item::Tool(c) = it {
                calls += 1;
                total_ms += c.duration_ms;
                let k = tool_label(&c.name);
                match kinds.iter_mut().find(|(n, _)| *n == k) {
                    Some(e) => e.1 += 1,
                    None => kinds.push((k, 1)),
                }
                if c.status == CallStatus::Pending {
                    running = true;
                    // What the agent says of the call that is running,
                    // as its terminal has it over the command.
                    said = c.saying();
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
                        this.reveal_now((1, ix, start));
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
            .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).flex_shrink_0().child(count(calls, "{n} tool call", "{n} tool calls")))
            .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).flex_shrink_0().child(tally))
            .child(match said {
                Some(said) => div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(theme.foreground).child(said),
                None => div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(theme.muted_foreground).child(last_subject),
            })
            .when(errors > 0, |d| d.child(div().text_size(px(11.5)).text_color(theme.danger).flex_shrink_0().child(tf("{n} failed", &[("n", &errors)]))))
            .when(running, |d| d.child(div().text_size(px(11.5)).text_color(agent_color(session.agent, &theme)).flex_shrink_0().child(t("running…"))))
            .when(!running && total_ms > 1500, |d| d.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).flex_shrink_0().child(human_duration(total_ms))));
        let mut v = v_flex().w_full().relative().child(head).children(self.reveal_mark((1, ix, start), cx));
        if let Some(fold) = self.fold_state((1, ix, start), open, cx) {
            let mut inner = v_flex().w_full().gap(px(6.)).pl(px(12.)).border_l_2().border_color(theme.border);
            let session_rc = session.clone();
            for jx in start..end {
                let item = session_rc.rounds[ix].items[jx].clone();
                let el = self.render_item(ix, jx, &item, open_tools, open_thoughts, open_subagents, session, cx);
                inner = inner.child(self.find_wrap(ix, jx, el, &theme));
            }
            v = v.child(self.fold_wrap(fold, 6., inner.into_any_element(), cx));
        }
        v.into_any_element()
    }

    /// A pasted or attached file inside a prompt: a thumbnail for a picture
    /// (from the file it was kept as, or read back out of the transcript), a
    /// typed icon with name and size for anything else.
    fn render_attachment(&mut self, ix: usize, a: &Attachment, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let title = if a.name.is_empty() { t("image").to_string() } else { a.name.clone() };
        let path = (!a.path.is_empty()).then(|| PathBuf::from(&a.path));
        let thumb_key = format!("{}:{}", a.uuid, a.index);
        let is_image = a.kind == "image";
        let (t2, p2, k2) = (title.clone(), path.clone(), thumb_key.clone());
        let dark = theme.mode.is_dark();
        // Resolve the picture, and its size, before the click listener
        // borrows `cx`. The size shapes the tile so the whole picture shows.
        let mut size: Option<(u32, u32)> = None;
        // A kept file is drawn from itself, or from the PNG made of a
        // picture gpui does not read (`pictures::picture_source`).
        let kept = path.as_ref().filter(|p| p.is_file()).and_then(|p| self.picture_source(p, cx));
        let source: Option<ImageSource> = if !is_image {
            None
        } else if let Some(p) = kept.clone() {
            size = self.kept_image_size(&p);
            Some(ImageSource::from(p))
        } else if path.as_ref().is_some_and(|p| p.is_file()) {
            // A picture with no thumbnail to draw yet, or none at all:
            // its tile, as any other file wears.
            None
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
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener({
                        let (p3, k3) = (path.clone(), thumb_key.clone());
                        move |this, ev: &MouseDownEvent, _, cx| {
                            let kept = p3.clone().filter(|p| p.is_file());
                            let pic = if !is_image {
                                None
                            } else if let Some(p) = kept.clone() {
                                this.picture_source(&p, cx).map(crate::workbench::Pic::File)
                            } else {
                                this.detail.as_ref().and_then(|d| d.thumbs.get(&k3).cloned().flatten()).map(|t| crate::workbench::Pic::Bytes(t.image))
                            };
                            this.attachment_menu(ev.position, kept, pic, cx);
                        }
                    }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    let image = if !is_image {
                        None
                    } else if let Some(p) = p2.as_ref().filter(|p| p.is_file()) {
                        this.picture_source(p, cx).map(ImageSource::from)
                    } else {
                        this.detail.as_ref().and_then(|d| d.thumbs.get(&k2).cloned().flatten()).map(|t| ImageSource::from(t.image))
                    };
                    if is_image && image.is_none() && !p2.as_ref().is_some_and(|p| p.is_file()) {
                        return;
                    }
                    this.preview_attachment(t2.clone(), p2.clone(), image, window, cx);
                }))
                .child(el)
        };
        if let Some(src) = source.filter(|_| is_image) {
            // Every picture of a prompt is one height, as wide as its own
            // shape makes it within a range, so a row of them lines up;
            // past the range the picture is letterboxed, never cropped.
            let (w, h) = gallery_box(size);
            let caption = if a.name.is_empty() { t("image").to_string() } else { a.name.clone() };
            return frame(
                v_flex()
                    .child(img(src).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain).bg(theme.muted))
                    .child(tile_caption(w, caption, &theme))
                    .into_any_element(),
            )
            .into_any_element();
        }
        if is_image && path.is_none() {
            // A pasted block not read yet.
            return frame(
                h_flex().h(px(48.)).px(px(12.)).gap(px(8.)).items_center().text_size(px(12.)).text_color(theme.muted_foreground).child(crate::pictures::kind_tile("x.png", 32., dark)).child(title.clone()).into_any_element(),
            )
            .into_any_element();
        }
        frame(
            h_flex()
                .h(px(48.))
                .pl(px(8.))
                .pr(px(12.))
                .gap(px(8.))
                .items_center()
                .child(crate::pictures::kind_tile(&a.name, 32., dark))
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

    /// A line in the conversation that is neither voice: a setting that
    /// changed, a command that ran, what it printed. All of them in the
    /// window's own face, not the reply's, so they read as the app's.
    ///
    /// A change of mode, model or effort is the pill's icon, what was set
    /// in the muted ink and what it was set to in the foreground, on no
    /// plate: "Effort High". It used to be the command's chip and under it
    /// the whole sentence Claude Code printed, on one plate that ran out
    /// of the column. A command is a mono chip, what it printed is quiet
    /// text behind a rule that wraps like any paragraph, and the two
    /// notices that mean something happened to the session (compacted, an
    /// error) keep a tinted plate.
    fn render_notice(&self, text: &str, variant: NoticeVariant, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let face = theme.font_family.clone();
        if let Some(what) = variant.setting() {
            // The value in bold, and in the color its pill has: the
            // agent's own for a mode, the level's place for an effort.
            let options = self.options();
            let color = match variant {
                NoticeVariant::Mode => crate::workbench::mode_color(&options, text, &theme),
                NoticeVariant::Effort => crate::workbench::effort_color(&options, "", text, &theme),
                _ => crate::workbench::Tint::Plain,
            };
            let icon = match variant {
                NoticeVariant::Mode => "icons/shield.svg",
                NoticeVariant::Model => "icons/box.svg",
                _ => "icons/gauge.svg",
            };
            return h_flex()
                .gap(px(7.))
                .items_center()
                .font_family(face)
                .text_size(px(12.5))
                .child(Icon::default().path(icon).with_size(px(13.)).text_color(theme.muted_foreground))
                .child(div().text_color(theme.muted_foreground).child(t(what).to_string()))
                .child(div().text_color(theme.foreground).child(crate::workbench::tinted(text, &color)))
                .into_any_element();
        }
        let plate = |bg: Hsla, fg: Hsla, words: &str| h_flex().child(div().px(px(9.)).py(px(3.)).rounded(px(7.)).bg(bg).text_color(fg).font_family(face.clone()).text_size(px(12.)).child(words.to_string())).into_any_element();
        match variant {
            NoticeVariant::Command => h_flex()
                .child(div().px(px(8.)).py(px(2.)).rounded(px(6.)).bg(theme.muted).text_color(theme.foreground).font_family(theme.mono_font_family.clone()).text_size(px(12.)).child(text.to_string()))
                .into_any_element(),
            // The line of this kind the core writes itself is in the
            // person's language; an agent's own words stay as they are.
            NoticeVariant::Compact => plate(theme.info.opacity(0.13), theme.foreground, t(text)),
            NoticeVariant::Error => plate(theme.danger.opacity(0.13), theme.danger, text),
            NoticeVariant::Interrupted => plate(theme.warning.opacity(0.16), theme.foreground, text),
            _ => div()
                .w_full()
                .min_w_0()
                .pl(px(10.))
                .border_l_2()
                .border_color(theme.border)
                .font_family(face)
                .text_size(px(12.5))
                .line_height(relative(1.45))
                .text_color(theme.muted_foreground)
                .child(text.to_string())
                .into_any_element(),
        }
    }

    fn render_thought(&self, ix: usize, jx: usize, md: &str, seconds: f64, open: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let body_px = self.cfg.app.chat_px();
        let label = if seconds >= 2.0 { tf("Thought for {time}", &[("time", &human_duration((seconds * 1000.0) as u64))]) } else { t("Thinking").to_string() };
        v_flex()
            .w_full()
            .relative()
            .children(self.reveal_mark((2, ix, jx), cx))
            .child(
                h_flex()
                    .id(SharedString::from(format!("th-{ix}-{jx}")))
                    .gap(px(6.))
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pin_scroll();
                        if let Some(d) = this.detail.as_mut() {
                            if !d.open_thoughts.remove(&(ix, jx)) {
                                d.open_thoughts.insert((ix, jx));
                                this.reveal_now((2, ix, jx));
                            }
                        }
                        cx.notify();
                    }))
                    .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(12.)))
                    .child(div().italic().child(label)),
            )
            .when_some(self.fold_state((2, ix, jx), open, cx), |d, fold| {
                let body = div()
                    .pl(px(14.))
                    .border_l_2()
                    .border_color(theme.border)
                    .text_size(px(body_px - 2.))
                    .text_color(theme.muted_foreground)
                    .line_height(relative(1.55))
                    .child(chat_md_view(format!("tk-{ix}-{jx}"), md.to_string(), cx));
                d.child(self.fold_wrap(fold, 4., body.into_any_element(), cx))
            })
            .into_any_element()
    }

    fn render_tool(&mut self, ix: usize, jx: usize, call: &ToolCall, open: bool, sub_open: bool, cwd: &str, cx: &mut Context<Self>) -> AnyElement {
        if call.tool_kind == ToolKind::Ask {
            return self.render_question_call(call, cx);
        }
        if let Some(plan) = plan_of(call) {
            return self.render_plan_call(ix, jx, call, plan, cx);
        }
        let theme = cx.theme().clone();
        let (status_text, status_color) = match call.status {
            CallStatus::Ok => ("", theme.muted_foreground),
            CallStatus::Error => (t("error"), theme.danger),
            CallStatus::Interrupted => (t("interrupted"), theme.warning),
            CallStatus::NoResult => (t("not run"), theme.muted_foreground),
            CallStatus::Pending => (t("running…"), theme.primary),
        };
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
            // keyed on the state so each click plays it once, and only
            // on the button pressed (`explain_flip`): an animation plays
            // whenever its element is first drawn, so every button of a
            // run of calls flashed the accent as the run opened or shut.
            let (off_border, off_bg, off_ink) = (theme.border, theme.transparent, theme.muted_foreground);
            let (on_border, on_bg, on_ink) = (theme.primary.opacity(0.6), theme.primary.opacity(0.14), theme.primary);
            let anim: SharedString = format!("explain-{ix}-{jx}-{}", if ex_open { "on" } else { "off" }).into();
            let flipped = self.explain_flip.as_ref().is_some_and(|(id, at)| *id == call.id && at.elapsed() < EXPLAIN_FLIP);
            let button = h_flex()
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
                .child(if ex_open && explaining && !has_text { t("Explaining…") } else { t("Explain") });
            if flipped {
                button
                    .with_animation(ElementId::Name(anim), Animation::new(EXPLAIN_FLIP).with_easing(ease_out_quint()), move |d, t| {
                        let t = if ex_open { t } else { 1. - t };
                        d.border_color(mix(off_border, on_border, t)).bg(mix(off_bg, on_bg, t)).text_color(mix(off_ink, on_ink, t))
                    })
                    .into_any_element()
            } else if ex_open {
                button.border_color(on_border).bg(on_bg).text_color(on_ink).into_any_element()
            } else {
                button.border_color(off_border).bg(off_bg).text_color(off_ink).into_any_element()
            }
        };

        let mut card = v_flex().w_full().relative().rounded(px(10.)).border_1().border_color(theme.border).bg(theme.popover).overflow_hidden().children(self.reveal_mark((0, ix, jx), cx));
        let fold = self.fold_state((0, ix, jx), open && has_body, cx);
        // A clip is a rectangle: the head's hover is rounded as the card
        // is, at the foot too while nothing is under it.
        let alone = !ex_open && fold.is_none();
        card = card.child(
            h_flex()
                .id(SharedString::from(format!("tool-{ix}-{jx}")))
                .w_full()
                .px(px(10.))
                .py(px(6.))
                .gap(px(8.))
                .items_center()
                .rounded_t(px(9.))
                .when(alone, |d| d.rounded_b(px(9.)))
                .when(has_body, |d| d.cursor_pointer().hover(|s| s.bg(theme.list_hover)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !has_body {
                        return;
                    }
                    this.pin_scroll();
                    if let Some(d) = this.detail.as_mut() {
                        if !d.open_tools.remove(&(ix, jx)) {
                            d.open_tools.insert((ix, jx));
                            this.reveal_now((0, ix, jx));
                        }
                    }
                    cx.notify();
                }))
                .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(12.)).text_color(if has_body { theme.muted_foreground } else { theme.border }))
                .child(div().flex_shrink_0().child(tool_badge(call, &theme)))
                // While it runs, what the agent says it is doing, in
                // the page's own face; then the call itself.
                .child(match call.saying() {
                    Some(said) => div().flex_1().min_w_0().truncate().text_size(px(12.5)).text_color(theme.foreground).child(said),
                    None => div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(12.)).text_color(theme.muted_foreground).child(subject),
                })
                .when(!stat.is_empty(), |d| d.child(div().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(theme.muted_foreground).child(stat)))
                .when(!status_text.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(status_color).child(status_text)))
                .when(!duration.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(duration)))
                .when(can_explain, |d| d.child(pill)),
        );
        if ex_open && can_explain {
            let text = if explanation.is_empty() && !explaining { t("Nothing came back from the model. Click Explain again to retry.").to_string() } else { explanation };
            let size = px(self.cfg.app.chat_px());
            card = card.child(div().w_full().px(px(10.)).pb(px(8.)).child(explain_card(format!("ex-{ix}-{jx}"), text, explaining, size, cx)));
        }
        if let Some(fold) = fold {
            // The body is bounded and scrolls inside the card, so a long
            // output never takes the whole window; the clip's "n more
            // lines" note still says what was left out entirely.
            let body = self.render_tool_body(ix, jx, call, sub_open, cwd, cx);
            // The scroll handle lives in the detail, so the position and the
            // bar survive the redraw every frame brings.
            let scroll = self.detail.as_mut().map(|d| d.body_scrolls.entry((ix, jx)).or_default().clone()).unwrap_or_default();
            let handle = scroll.handle.clone();
            self.inner_scroller(&handle, crate::workbench::Inner::Chained);
            // The wheel over a body that scrolls belongs to the body,
            // but for a stroke begun at its edge (`chain_wheel`).
            let body =
                div()
                    .relative()
                    .w_full()
                    .border_t_1()
                    .border_color(theme.border)
                    .on_scroll_wheel(crate::workbench::chain_wheel(handle.clone(), scroll.stroke.clone()))
                    .child(
                        div()
                            .id(SharedString::from(format!("tool-body-{ix}-{jx}")))
                            .w_full()
                            .max_h(MAX_BODY_H)
                            .overflow_y_scroll()
                            .track_scroll(&handle)
                            .child(div().w_full().px(px(10.)).pt(px(8.)).pb(px(10.)).child(body)),
                    )
                    .vertical_scrollbar(&handle);
            card = card.child(self.fold_wrap(fold, 0., body.into_any_element(), cx));
        }
        card.into_any_element()
    }

    /// A question in the conversation, as it was asked and answered: each
    /// question with its options, the chosen one ticked, words typed instead
    /// quoted under it. While it waits, the card says where the answer
    /// goes: the card under the conversation when a driver of ours holds
    /// it, the terminal otherwise.
    fn render_question_call(&self, call: &ToolCall, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let qs = questions_of(&call.input);
        let pending = call.status == CallStatus::Pending;
        let held_here = self.permissions.iter().any(|(_, p)| p.tool_use_id == call.id);
        let state = if pending {
            if held_here { t("waiting for your answer below") } else { t("waiting for your answer in your terminal") }
        } else if call.answers.is_empty() {
            t("not answered")
        } else {
            ""
        };
        v_flex()
            .w_full()
            .rounded(px(12.))
            .border_1()
            .border_color(if pending { theme.primary.opacity(0.5) } else { theme.border })
            .bg(theme.popover)
            .px(px(14.))
            .py(px(10.))
            .gap(px(10.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(badge_str(t("question").into(), theme.primary.opacity(0.14), theme.primary))
                    .when(!state.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(if pending { theme.primary } else { theme.muted_foreground }).child(state))),
            )
            .children(qs.iter().map(|q| {
                let answer = call.answers.iter().find(|(k, _)| *k == q.question).map(|(_, a)| a.clone()).unwrap_or_default();
                // A label may itself hold a comma, so the answer is matched
                // whole, and as a part only where several were allowed.
                let chose = |label: &str| answer == label || (q.multi && answer.contains(label));
                let typed = !answer.is_empty() && !q.options.iter().any(|(l, _)| chose(l));
                v_flex()
                    .gap(px(5.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .when(!q.header.is_empty(), |d| d.child(badge_str(q.header.clone(), theme.muted, theme.muted_foreground)))
                            .child(div().flex_1().min_w_0().text_size(px(13.5)).child(q.question.clone())),
                    )
                    .children(q.options.iter().map(|(label, detail)| {
                        let on = chose(label);
                        h_flex()
                            .gap(px(8.))
                            .items_start()
                            .pl(px(2.))
                            .text_size(px(12.5))
                            .text_color(if on || pending { theme.foreground } else { theme.muted_foreground })
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size(px(14.))
                                    .mt(px(2.))
                                    .rounded(if q.multi { px(3.) } else { px(7.) })
                                    .border_1()
                                    .border_color(if on { theme.primary } else { theme.border })
                                    .bg(if on { theme.primary } else { theme.transparent })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(on, |d| d.child(Icon::new(IconName::Check).with_size(px(10.)).text_color(theme.primary_foreground))),
                            )
                            .child(div().min_w_0().child(label.clone()).when(!detail.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(detail.clone()))))
                    }))
                    .when(typed, |d| d.child(div().pl(px(24.)).text_size(px(12.5)).child(format!("↳ {answer}"))))
            }))
            .into_any_element()
    }

    /// A plan in the conversation, whole and as markdown, with what
    /// became of it: the terminal prints a plan and then asks, and a
    /// tool card folded shut said only that a tool had run.
    fn render_plan_call(&self, ix: usize, jx: usize, call: &ToolCall, plan: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let pending = call.status == CallStatus::Pending;
        let state = match call.status {
            CallStatus::Pending => t("waiting for your go-ahead"),
            CallStatus::Ok => t("approved"),
            _ => t("not approved"),
        };
        v_flex()
            .w_full()
            .rounded(px(12.))
            .border_1()
            .border_color(if pending { theme.primary.opacity(0.5) } else { theme.border })
            .bg(theme.popover)
            .px(px(14.))
            .py(px(10.))
            .gap(px(10.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(badge_str(t("plan").into(), theme.primary.opacity(0.14), theme.primary))
                    .child(div().text_size(px(11.5)).text_color(if pending { theme.primary } else { theme.muted_foreground }).child(state)),
            )
            .child(div().w_full().text_size(px(self.cfg.app.chat_px())).line_height(relative(1.5)).child(chat_md_view(format!("plan-{ix}-{jx}"), plan.to_string(), cx)))
            .into_any_element()
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
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener({
                                            let (file, pic) = (file.clone(), t.image.clone());
                                            move |this, ev: &MouseDownEvent, _, cx| this.attachment_menu(ev.position, file.clone().filter(|p| p.is_file()), Some(crate::workbench::Pic::Bytes(pic.clone())), cx)
                                        }),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| this.preview_attachment(title.clone(), file.clone(), Some(image.clone()), window, cx)))
                                    .child(img(ImageSource::from(t.image)).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain))
                                    .child(tile_caption(w, caption, &theme)),
                            );
                        }
                        None => parts = parts.child(div().text_color(theme.muted_foreground).italic().child(t("loading the picture…"))),
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
                    parts = parts.child(div().text_color(theme.muted_foreground).italic().child(t("no output")));
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
                    parts = parts.child(div().pl(px(10.)).border_l_2().border_color(theme.border).text_color(theme.muted_foreground).child(chat_md_view(format!("tp-{ix}-{jx}"), snippet, cx)));
                }
                if !call.subagent.is_empty() {
                    let n = call.subagent.len();
                    // The head and what folds under it in a column of
                    // their own with no gap: the gap is in the fold.
                    let mut sub = v_flex().w_full().child(
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
                            .child(div().child(format!("{} · {}", if call.agent_name.is_empty() { t("subagent") } else { &call.agent_name }, count(n, "{n} round", "{n} rounds")))),
                    );
                    if let Some(fold) = self.fold_state((3, ix, jx), sub_open, cx) {
                        let mut nested = v_flex().w_full().gap(px(10.)).pl(px(10.)).border_l_2().border_color(theme.border);
                        for (k, sub) in call.subagent.iter().enumerate() {
                            nested = nested.child(self.render_subround(ix, jx, k, sub, cx));
                        }
                        sub = sub.child(self.fold_wrap(fold, 8., nested.into_any_element(), cx));
                    }
                    parts = parts.child(sub);
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
                    parts = parts.child(div().text_color(theme.muted_foreground).italic().child(count(call.result_images as usize, "{n} image returned", "{n} images returned")));
                }
            }
        }
        parts.into_any_element()
    }

    /// A subagent's round, drawn flat: prompt, then text and a one-line
    /// summary per tool call. Deeper nesting is summarized, not expanded.
    fn render_subround(&self, ix: usize, jx: usize, k: usize, sub: &Round, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut v = v_flex().w_full().gap(px(6.));
        if !sub.prompt.is_empty() {
            let (snippet, _) = clip(&sub.prompt, 1200);
            v = v.child(div().px(px(10.)).py(px(6.)).rounded(px(8.)).bg(theme.muted).text_size(px(12.)).child(chat_md_view(format!("sp-{ix}-{jx}-{k}"), snippet, cx)));
        }
        for (m, item) in sub.items.iter().enumerate() {
            match item {
                Item::Text { md, .. } => {
                    v = v.child(div().text_size(px(12.5)).line_height(relative(1.55)).child(chat_md_view(format!("st-{ix}-{jx}-{k}-{m}"), md.clone(), cx)));
                }
                Item::Tool(c) => {
                    v = v.child(
                        h_flex()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(theme.muted_foreground)
                            .child(div().flex_shrink_0().child(tool_badge(c, &theme)))
                            .child(div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).child(c.subject.clone())),
                    );
                }
                Item::Thinking { seconds, .. } => {
                    v = v.child(div().text_size(px(11.5)).italic().text_color(theme.muted_foreground).child(if *seconds >= 2.0 { tf("thought for {time}", &[("time", &human_duration((seconds * 1000.0) as u64))]) } else { t("thinking").to_string() }));
                }
                Item::Notice { text, variant, .. } => {
                    v = v.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(match variant.setting() {
                        Some(what) => format!("{} {text}", t(what)),
                        None if *variant == NoticeVariant::Compact => t(text).to_string(),
                        None => text.clone(),
                    }));
                }
            }
        }
        v.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::chat_parse_options;
    use markdown::mdast::Node;

    fn chat(text: &str) -> Node {
        let mut opts = markdown::ParseOptions::gfm();
        chat_parse_options(&mut opts);
        markdown::to_mdast(text, &opts).unwrap()
    }

    fn has(node: &Node, want: &dyn Fn(&Node) -> bool) -> bool {
        want(node) || node.children().is_some_and(|kids| kids.iter().any(|k| has(k, want)))
    }

    fn heading(n: &Node) -> bool {
        matches!(n, Node::Heading(_))
    }

    #[test]
    fn lines_split_by_a_dash_are_not_headings() {
        // A prompt that split a list into parts with a dash on a line.
        let prompt = "Also, I'd like to have division lines to split them into 3 parts:\n\nGet Started\n-\nProjects\nConversation\n-\nFile Panel\nOutline Panel\n-\nSettings\nKeys\n";
        assert!(!has(&chat(prompt), &heading));
        assert!(!has(&chat("Title\n====\n"), &heading));
        // A model's rule under a paragraph stays a rule.
        assert!(has(&chat("Done.\n---\nNext part.\n"), &|n| matches!(n, Node::ThematicBreak(_))));
    }

    #[test]
    fn prose_signs_stay_text() {
        assert!(!has(&chat("    indented, as pasted\n"), &|n| matches!(n, Node::Code(_))));
        assert!(!has(&chat("Use the <Input> field here.\n"), &|n| matches!(n, Node::Html(_))));
        assert!(!has(&chat("It took ~30% then ~40% of the time.\n"), &|n| matches!(n, Node::Delete(_))));
    }

    #[test]
    fn markdown_meant_as_markdown_still_reads() {
        assert!(has(&chat("## A heading\n"), &heading));
        assert!(has(&chat("Some **bold** words.\n"), &|n| matches!(n, Node::Strong(_))));
        assert!(has(&chat("```rust\nfn x() {}\n```\n"), &|n| matches!(n, Node::Code(_))));
        assert!(has(&chat("~~gone~~\n"), &|n| matches!(n, Node::Delete(_))));
        assert!(has(&chat("| a | b |\n|---|---|\n| 1 | 2 |\n"), &|n| matches!(n, Node::Table(_))));
        assert!(has(&chat("- one\n- two\n"), &|n| matches!(n, Node::List(_))));
    }
}
