//! Drawing one round of a session: the prompt, then every item the agent
//! produced. Tool calls and thoughts fold; a Task call unfolds into the
//! subagent's own rounds.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::highlighter::HighlightTheme;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};
use scribe_core::model::*;
use scribe_core::render_md::{clip, code_block, command_text, human_duration, human_tokens, patch_stat, pretty_args, render_patch};

use crate::format::clock;
use std::path::PathBuf;

use std::collections::HashSet;

use crate::workbench::{agent_color, agent_glyph, badge, file_icon, file_kind, fit_thumb, human_size, Workbench, CONTENT_W};

const MAX_BODY: usize = 6000;
/// How far a reply's right edge stays inside the prompts' right edge.
const REPLY_INSET: Pixels = px(40.);

pub(crate) fn md_view(id: String, text: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
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
                ..Default::default()
            }
            .code_block(StyleRefinement::default().bg(code_bg).border_1().border_color(border).rounded(px(10.)).px(px(12.)).py(px(10.))),
        )
}

fn mono_block(id: String, text: &str, lang: &str, cx: &App) -> impl IntoElement {
    let (body, dropped) = clip(text, MAX_BODY);
    let mut md = code_block(&body, lang);
    if dropped > 0 {
        md.push_str(&format!("\n\n*… {dropped} more lines*"));
    }
    md_view(id, md, cx)
}

fn tool_kind_label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Bash => "run",
        ToolKind::Edit => "edit",
        ToolKind::Write => "write",
        ToolKind::Read => "read",
        ToolKind::Search => "search",
        ToolKind::Web => "web",
        ToolKind::Task => "agent",
        ToolKind::Todo => "todo",
        ToolKind::Ask => "ask",
        ToolKind::Plan => "plan",
        ToolKind::Mcp => "mcp",
        ToolKind::Other => "tool",
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

        let who = match rnd.source {
            Source::Web => "You · scribe",
            Source::Peer => "Another session",
            Source::System => "Session",
            _ => "You",
        };
        let mut meta = vec![clock(&rnd.ts)];
        if rnd.duration_ms > 0 {
            meta.push(human_duration(rnd.duration_ms));
        }
        let tools = rnd.tool_count();
        if tools > 0 {
            meta.push(format!("{tools} tool call{}", if tools == 1 { "" } else { "s" }));
        }
        if rnd.usage.total() > 0 {
            meta.push(format!("{} tokens", human_tokens(rnd.usage.total())));
        }

        let mut column = v_flex().w_full().max_w(CONTENT_W).px(px(24.)).pt(px(20.)).pb(if is_last { px(28.) } else { px(6.) }).gap(px(12.));

        // The prompt, as a bubble on the right: what was attached first
        // (pictures, then files), the words under them, the way a message
        // with a picture reads in the Claude app.
        if !rnd.prompt.is_empty() || rnd.images > 0 || !rnd.attachments.is_empty() {
            let mut bubble = v_flex().max_w(px(600.)).px(px(16.)).py(px(11.)).gap(px(4.)).rounded(px(18.)).bg(theme.muted).text_size(px(14.)).line_height(relative(1.55));
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
            column = column.child(
                v_flex()
                    .w_full()
                    .items_end()
                    .gap(px(3.))
                    .child(bubble)
                    .child(h_flex().gap(px(8.)).pr(px(6.)).text_size(px(11.)).text_color(theme.muted_foreground).child(div().child(who)).child(div().child(meta.join(" · ")))),
            );
        } else {
            column = column.child(h_flex().gap(px(8.)).text_size(px(11.)).text_color(theme.muted_foreground).child(div().child(who)).child(div().child(meta.join(" · "))));
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
            body = body.child(el);
            jx += 1;
        }
        if any {
            let mark_color = if session.agent == scribe_core::model::AgentId::ClaudeCode { theme.primary } else { theme.muted_foreground };
            let working = is_last && self.selected_ref().map(|r| self.is_working(r)).unwrap_or(false);
            column = column.child(
                v_flex()
                    .w_full()
                    .gap(px(8.))
                    .child(h_flex().gap(px(7.)).items_center().child(agent_glyph(session.agent, px(15.), mark_color, working, SharedString::from(format!("round-glyph-{ix}")))).child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).child(speaker)))
                    // The reply stops short of the column's right edge, where
                    // the prompt bubbles end: the two voices sit at different
                    // widths, as in the Claude app, and read apart at a glance.
                    .child(div().w_full().pl(px(22.)).pr(REPLY_INSET).child(body)),
            );
        }
        // The list lays each item out on its own, so an auto margin has
        // nothing to push against; an explicit centring parent keeps the
        // column in the middle of a wide pane.
        h_flex().w_full().justify_center().child(column).into_any_element()
    }

    fn render_item(&mut self, ix: usize, jx: usize, item: &Item, open_tools: &HashSet<(usize, usize)>, open_thoughts: &HashSet<(usize, usize)>, open_subagents: &HashSet<(usize, usize)>, session: &Session, cx: &mut Context<Self>) -> AnyElement {
        match item {
            Item::Text { md, .. } => div().w_full().text_size(px(14.5)).line_height(relative(1.65)).child(md_view(format!("t-{ix}-{jx}"), md.clone(), cx)).into_any_element(),
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
        let mut kinds: Vec<(&'static str, usize)> = Vec::new();
        let mut total_ms = 0u64;
        let mut last_subject = String::new();
        let mut running = false;
        let mut errors = 0;
        let mut count = 0;
        for it in items {
            if let Item::Tool(c) = it {
                count += 1;
                total_ms += c.duration_ms;
                let k = tool_kind_label(c.tool_kind);
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
        let head = h_flex()
            .id(SharedString::from(format!("run-{ix}-{start}")))
            .w_full()
            .px(px(10.))
            .py(px(7.))
            .gap(px(8.))
            .items_center()
            .rounded(px(10.))
            .bg(theme.muted)
            .cursor_pointer()
            .hover(|s| s.bg(theme.list_active))
            .on_click(cx.listener(move |this, _, _, cx| {
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
                inner = inner.child(self.render_item(ix, jx, &item, open_tools, open_thoughts, open_subagents, session, cx));
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
        let frame = |el: AnyElement| {
            div()
                .id(SharedString::from(format!("att-{ix}-{}-{}", a.index, a.name)))
                .rounded(px(10.))
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
                    frame(img(src).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain).into_any_element()).into_any_element()
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
                        .text_size(px(12.5))
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
        let kind_label = tool_kind_label(call.tool_kind);
        let stat = if call.tool_kind == ToolKind::Edit { patch_stat(&call.patch) } else { String::new() };
        let duration = if call.duration_ms > 1500 { human_duration(call.duration_ms) } else { String::new() };
        let has_body = self.tool_has_body(call);
        let subject = call.subject.clone();
        let name = call.name.clone();

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
                    if let Some(d) = this.detail.as_mut() {
                        if !d.open_tools.remove(&(ix, jx)) {
                            d.open_tools.insert((ix, jx));
                        }
                    }
                    cx.notify();
                }))
                .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).with_size(px(12.)).text_color(if has_body { theme.muted_foreground } else { theme.border }))
                .child(badge(kind_label, theme.muted, theme.muted_foreground))
                .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(name))
                .child(div().flex_1().min_w_0().truncate().font_family(theme.mono_font_family.clone()).text_size(px(12.)).text_color(theme.muted_foreground).child(subject))
                .when(!stat.is_empty(), |d| d.child(div().font_family(theme.mono_font_family.clone()).text_size(px(11.5)).text_color(theme.muted_foreground).child(stat)))
                .when(!status_text.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(status_color).child(status_text)))
                .when(!duration.is_empty(), |d| d.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(duration))),
        );
        if open && has_body {
            card = card.child(div().w_full().px(px(10.)).pb(px(10.)).border_t_1().border_color(theme.border).pt(px(8.)).child(self.render_tool_body(ix, jx, call, sub_open, cwd, cx)));
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
        if !call.explanation.is_empty() {
            parts = parts.child(div().text_color(theme.muted_foreground).child(call.explanation.clone()));
        }
        match call.tool_kind {
            ToolKind::Read => {
                if call.result_images > 0 {
                    // The picture the Read returned, read back out of the
                    // transcript like a pasted one; a click opens it large.
                    match self.thumb(ix, &call.result_uuid, call.result_index, cx) {
                        Some(t) => {
                            let (w, h) = fit_thumb(t.size, 320., 240., (176., 118.));
                            let title = call.subject.clone();
                            let image = ImageSource::from(t.image.clone());
                            parts = parts.child(
                                div()
                                    .id(SharedString::from(format!("read-img-{ix}-{jx}")))
                                    .w(px(w))
                                    .rounded(px(10.))
                                    .border_1()
                                    .border_color(theme.border)
                                    .overflow_hidden()
                                    .cursor_pointer()
                                    .hover(|s| s.border_color(theme.primary))
                                    .on_click(cx.listener(move |this, _, window, cx| this.preview_attachment(title.clone(), None, Some(image.clone()), window, cx)))
                                    .child(img(ImageSource::from(t.image)).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain)),
                            );
                        }
                        None => parts = parts.child(div().text_color(theme.muted_foreground).italic().child("loading the picture…")),
                    }
                } else if !call.result_text.trim().is_empty() {
                    let lang = scribe_core::render_md::lang_for_path(data.get("file_path").and_then(|v| v.as_str()).unwrap_or(""));
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
                    parts = parts.child(mono_block(format!("diff-{ix}-{jx}"), &scribe_core::render_md::naive_diff(&call.old_string, &call.new_string, 80), "diff", cx));
                } else if let Some(p) = data.get("patch").and_then(|v| v.as_str()) {
                    parts = parts.child(mono_block(format!("diff-{ix}-{jx}"), p, "diff", cx));
                }
                if call.status == CallStatus::Error && !call.result_text.is_empty() {
                    parts = parts.child(mono_block(format!("res-{ix}-{jx}"), &call.result_text, "text", cx));
                }
            }
            ToolKind::Write => {
                let content = data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let lang = scribe_core::render_md::lang_for_path(data.get("file_path").and_then(|v| v.as_str()).unwrap_or(""));
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
                            .child(badge(match c.tool_kind { ToolKind::Bash => "run", ToolKind::Edit => "edit", ToolKind::Write => "write", ToolKind::Read => "read", ToolKind::Search => "search", ToolKind::Web => "web", ToolKind::Task => "agent", _ => "tool" }, theme.muted, theme.muted_foreground))
                            .child(div().child(c.name.clone()))
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
