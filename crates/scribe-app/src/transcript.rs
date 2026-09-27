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
use crate::workbench::{badge, Workbench};

const MAX_BODY: usize = 6000;

fn md_view(id: String, text: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    let dark = theme.mode.is_dark();
    let code_bg = theme.muted;
    let border = theme.border;
    TextView::markdown(SharedString::from(format!("{id}-{}", if dark { "d" } else { "l" })), text)
        .selectable(true)
        .style(
            TextViewStyle {
                heading_base_font_size: px(14.),
                paragraph_gap: rems(0.6),
                highlight_theme: if dark { HighlightTheme::default_dark() } else { HighlightTheme::default_light() },
                ..Default::default()
            }
            .code_block(StyleRefinement::default().bg(code_bg).border_1().border_color(border).rounded(px(6.)).px(px(10.)).py(px(8.))),
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

impl Workbench {
    pub fn render_round(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(detail) = &self.detail else { return div().into_any_element() };
        let session = detail.session.clone();
        let Some(rnd) = session.rounds.get(ix) else { return div().into_any_element() };
        let open_tools = detail.open_tools.clone();
        let open_thoughts = detail.open_thoughts.clone();
        let open_subagents = detail.open_subagents.clone();
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

        let mut column = v_flex().w_full().max_w(px(760.)).mx_auto().px(px(24.)).pt(px(18.)).pb(if is_last { px(28.) } else { px(6.) }).gap(px(10.));

        // The prompt, as a bubble on the right.
        if !rnd.prompt.is_empty() || rnd.images > 0 || !rnd.attachments.is_empty() {
            let mut bubble = v_flex().max_w(px(560.)).px(px(14.)).py(px(9.)).gap(px(4.)).rounded(px(12.)).bg(theme.muted);
            if !rnd.prompt.is_empty() {
                bubble = bubble.child(md_view(format!("p-{ix}"), rnd.prompt.clone(), cx));
            }
            if rnd.images > 0 {
                bubble = bubble.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(format!("+ {} pasted image{}", rnd.images, if rnd.images == 1 { "" } else { "s" })));
            }
            for a in &rnd.attachments {
                if !a.path.is_empty() {
                    bubble = bubble.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(format!("attached: {}", a.name)));
                }
            }
            column = column.child(
                v_flex()
                    .w_full()
                    .items_end()
                    .gap(px(3.))
                    .child(h_flex().gap(px(8.)).text_size(px(11.)).text_color(theme.muted_foreground).child(div().child(who)).child(div().child(meta.join(" · "))))
                    .child(bubble),
            );
        } else {
            column = column.child(h_flex().gap(px(8.)).text_size(px(11.)).text_color(theme.muted_foreground).child(div().child(who)).child(div().child(meta.join(" · "))));
        }

        // The agent's items, on the left.
        let mut body = v_flex().w_full().gap(px(8.));
        let mut any = false;
        for (jx, item) in rnd.items.iter().enumerate() {
            any = true;
            let el = match item {
                Item::Text { md, .. } => div().w_full().text_size(px(13.5)).line_height(relative(1.6)).child(md_view(format!("t-{ix}-{jx}"), md.clone(), cx)).into_any_element(),
                Item::Thinking { md, seconds, .. } => self.render_thought(ix, jx, md, *seconds, open_thoughts.contains(&(ix, jx)), cx),
                Item::Notice { text, variant, .. } => self.render_notice(text, *variant, cx),
                Item::Tool(call) => self.render_tool(ix, jx, call, open_tools.contains(&(ix, jx)), open_subagents.contains(&(ix, jx)), &session.cwd, cx),
            };
            body = body.child(el);
        }
        if any {
            column = column.child(
                v_flex()
                    .w_full()
                    .gap(px(4.))
                    .child(div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(speaker))
                    .child(body),
            );
        }
        column.into_any_element()
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
                            d.list.splice(ix..ix + 1, 1);
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

    fn render_tool(&self, ix: usize, jx: usize, call: &ToolCall, open: bool, sub_open: bool, cwd: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (status_text, status_color) = match call.status {
            CallStatus::Ok => ("", theme.muted_foreground),
            CallStatus::Error => ("error", theme.danger),
            CallStatus::Interrupted => ("interrupted", theme.warning),
            CallStatus::NoResult => ("not run", theme.muted_foreground),
            CallStatus::Pending => ("running…", theme.primary),
        };
        let kind_label = match call.tool_kind {
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
        };
        let stat = if call.tool_kind == ToolKind::Edit { patch_stat(&call.patch) } else { String::new() };
        let duration = if call.duration_ms > 1500 { human_duration(call.duration_ms) } else { String::new() };
        let has_body = self.tool_has_body(call);
        let subject = call.subject.clone();
        let name = call.name.clone();

        let mut card = v_flex().w_full().rounded(px(8.)).border_1().border_color(theme.border).bg(theme.popover).overflow_hidden();
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
                        d.list.splice(ix..ix + 1, 1);
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
            ToolKind::Read => call.status == CallStatus::Error && !call.result_text.is_empty(),
            ToolKind::Task => !call.subagent.is_empty() || !call.result_text.is_empty() || call.input.get("prompt").is_some(),
            _ => true,
        }
    }

    fn render_tool_body(&self, ix: usize, jx: usize, call: &ToolCall, sub_open: bool, _cwd: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let data = &call.input;
        let mut parts = v_flex().w_full().gap(px(8.)).text_size(px(12.5));
        if !call.explanation.is_empty() {
            parts = parts.child(div().text_color(theme.muted_foreground).child(call.explanation.clone()));
        }
        match call.tool_kind {
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
                                    d.list.splice(ix..ix + 1, 1);
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
