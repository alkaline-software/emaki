//! Session -> CommonMark. The markdown file is the artifact you keep:
//! greppable, diffable, readable in ten years when this program no longer
//! exists. Nothing parses it back, so it carries no machine scaffolding.

use std::path::Path;

use chrono::{DateTime, Local};
use serde_json::{Map, Value};

use crate::config::Config;
use crate::model::*;
use crate::redact::Redactor;

pub const GAP_MINUTES: i64 = 30;

pub fn fence_for(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

pub fn code_block(text: &str, lang: &str) -> String {
    let fence = fence_for(text);
    format!("{fence}{lang}\n{}\n{fence}", text.trim_end_matches('\n'))
}

/// Truncate on a line boundary. Returns (text, lines_dropped). `limit` is in
/// characters, as it was in the Python renderer, so existing logs do not
/// change under a rebuild.
pub fn clip(text: &str, limit: usize) -> (String, usize) {
    if text.is_empty() || limit == 0 {
        return (text.to_string(), 0);
    }
    let cut = match text.char_indices().nth(limit) {
        Some((byte, _)) => byte,
        None => return (text.to_string(), 0),
    };
    let mut head = &text[..cut];
    if let Some(nl) = head.rfind('\n') {
        if head[..nl].chars().count() > limit / 2 {
            head = &head[..nl];
        }
    }
    let dropped = text[head.len()..].matches('\n').count() + 1;
    (head.trim_end().to_string(), dropped)
}

pub fn human_duration(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    let seconds = ms as f64 / 1000.0;
    if seconds < 1.0 {
        return format!("{ms}ms");
    }
    if seconds < 60.0 {
        let s = format!("{seconds:.1}s");
        return s.replace(".0s", "s");
    }
    let total = seconds as u64;
    let (minutes, secs) = (total / 60, total % 60);
    if minutes < 60 {
        return if secs > 0 { format!("{minutes}m {secs}s") } else { format!("{minutes}m") };
    }
    let (hours, minutes) = (minutes / 60, minutes % 60);
    if minutes > 0 { format!("{hours}h {minutes}m") } else { format!("{hours}h") }
}

pub fn human_tokens(n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    if n < 1000 {
        return n.to_string();
    }
    if n < 1_000_000 {
        return format!("{:.1}k", n as f64 / 1000.0).replace(".0k", "k");
    }
    format!("{:.2}M", n as f64 / 1_000_000.0).replace(".00M", "M")
}

pub fn local_dt(ts: &str) -> Option<DateTime<Local>> {
    crate::build::parse_ts(ts).map(|d| d.with_timezone(&Local))
}

pub fn fmt_time(ts: &str) -> String {
    local_dt(ts).map(|d| d.format("%H:%M:%S").to_string()).unwrap_or_default()
}
pub fn fmt_datetime(ts: &str) -> String {
    local_dt(ts).map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}
pub fn fmt_date(ts: &str) -> String {
    local_dt(ts).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn status_mark(s: CallStatus) -> &'static str {
    match s {
        CallStatus::Ok => "",
        CallStatus::Error => " ⚠︎ error",
        CallStatus::Interrupted => " ⚠︎ interrupted",
        CallStatus::NoResult => " ⊘ not run",
        CallStatus::Pending => " … running",
    }
}

pub fn lang_for_path(path: &str) -> &'static str {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "py" => "python",
        "r" => "r",
        "js" | "mjs" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "json" => "json",
        "sh" | "zsh" | "bash" => "bash",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "md" | "qmd" | "rmd" => "markdown",
        "html" => "html",
        "css" => "css",
        "scss" => "scss",
        "sql" => "sql",
        "go" => "go",
        "rs" => "rust",
        "c" | "h" => "c",
        "cpp" => "cpp",
        "java" => "java",
        "rb" => "ruby",
        "swift" => "swift",
        "lua" => "lua",
        "vim" => "vim",
        _ => "",
    }
}

pub struct MarkdownRenderer<'a> {
    tools: String,
    show_thinking: bool,
    max_output: usize,
    max_input: usize,
    redact: &'a Redactor,
}

impl<'a> MarkdownRenderer<'a> {
    pub fn new(cfg: &Config, redact: &'a Redactor) -> Self {
        Self {
            tools: cfg.markdown.tools.clone(),
            show_thinking: cfg.markdown.thinking,
            max_output: cfg.markdown.max_output_chars,
            max_input: cfg.markdown.max_input_chars,
            redact,
        }
    }

    fn r(&self, s: &str) -> String {
        self.redact.scrub(s)
    }

    pub fn render(&self, session: &Session) -> String {
        let mut out: Vec<String> = vec![self.header(session)];
        let mut previous_end = String::new();
        for rnd in &session.rounds {
            let gap = self.gap(&previous_end, &rnd.ts);
            if !gap.is_empty() {
                out.push(gap);
            }
            out.push(self.round(rnd, session, 2));
            previous_end = if rnd.end_ts.is_empty() { rnd.ts.clone() } else { rnd.end_ts.clone() };
        }
        out.push(self.footer(session));
        let joined = out.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n");
        format!("{}\n", joined.trim_end())
    }

    fn header(&self, s: &Session) -> String {
        let mut lines = vec![format!("# {}", if s.title.is_empty() { "Session" } else { &s.title }), String::new()];
        let mut facts = Vec::new();
        if !s.cwd.is_empty() {
            facts.push(format!("`{}`", crate::paths::tilde(&s.cwd)));
        }
        if !s.git_branch.is_empty() {
            facts.push(format!("branch `{}`", s.git_branch));
        }
        let mut span = fmt_datetime(&s.started);
        if !s.updated.is_empty() && fmt_date(&s.updated) != fmt_date(&s.started) {
            span.push_str(&format!(" → {}", fmt_datetime(&s.updated)));
        } else if !s.updated.is_empty() {
            let t = fmt_time(&s.updated);
            span.push_str(&format!(" → {}", &t[..t.len().min(5)]));
        }
        if !span.is_empty() {
            facts.push(span);
        }
        if !facts.is_empty() {
            lines.push(facts.join(" · "));
            lines.push(String::new());
        }
        let n = s.rounds.len();
        let t = s.tool_count();
        let mut counts = vec![
            format!("{n} round{}", if n != 1 { "s" } else { "" }),
            format!("{t} tool call{}", if t != 1 { "s" } else { "" }),
        ];
        if s.usage.total() > 0 {
            counts.push(format!("{} tokens", human_tokens(s.usage.total())));
        }
        lines.push(format!("*{}*", counts.join(" · ")));
        lines.push(String::new());
        lines.join("\n")
    }

    fn footer(&self, s: &Session) -> String {
        let mut bits = vec![format!("session `{}`", s.id.chars().take(8).collect::<String>())];
        if !s.version.is_empty() {
            bits.push(format!("{} {}", s.agent.display_name(), s.version));
        }
        if let Some(m) = s.models.last() {
            bits.push(m.clone());
        }
        format!("\n---\n\n<sub>{} · logged by Emaki</sub>\n", esc(&bits.join(" · ")))
    }

    fn gap(&self, previous_end: &str, next_start: &str) -> String {
        let (Some(a), Some(b)) = (local_dt(previous_end), local_dt(next_start)) else {
            return String::new();
        };
        let minutes = (b - a).num_minutes();
        if minutes < GAP_MINUTES {
            return String::new();
        }
        format!("\n<sub>— {} later —</sub>\n", human_duration((minutes * 60_000) as u64))
    }

    fn round(&self, rnd: &Round, session: &Session, level: usize) -> String {
        let hashes = "#".repeat(level);
        let who = match rnd.source {
            Source::Web => "You (Emaki)",
            Source::Peer => "Another session",
            Source::System => "Session",
            _ => "You",
        };
        let mut out = vec![format!("\n{hashes} {} · {who}", rnd.index), String::new()];
        let mut meta = vec![format!("`{}`", fmt_time(&rnd.ts))];
        if rnd.duration_ms > 0 {
            meta.push(human_duration(rnd.duration_ms));
        }
        let tools = rnd.tool_count();
        if tools > 0 {
            meta.push(format!("{tools} tool call{}", if tools != 1 { "s" } else { "" }));
        }
        if rnd.usage.total() > 0 {
            meta.push(format!("{} tokens", human_tokens(rnd.usage.total())));
        }
        out.push(format!("*{}*", meta.join(" · ")));
        out.push(String::new());
        if !rnd.prompt.is_empty() {
            out.push(self.r(&rnd.prompt));
            out.push(String::new());
        }
        if rnd.images > 0 {
            out.push(format!("*+ {} pasted image{}*", rnd.images, if rnd.images != 1 { "s" } else { "" }));
            out.push(String::new());
        }
        let files: Vec<&Attachment> = rnd.attachments.iter().filter(|a| !a.path.is_empty()).collect();
        if !files.is_empty() {
            for a in files {
                out.push(format!("*attached:* [{}]({})", a.name, self.r(&a.path)));
            }
            out.push(String::new());
        }
        let body = self.items(&rnd.items, session, level);
        if !body.is_empty() {
            out.push(format!("{hashes}# {}", session.agent.speaker()));
            out.push(String::new());
            out.push(body);
        }
        out.join("\n")
    }

    fn items(&self, items: &[Item], session: &Session, level: usize) -> String {
        let mut chunks = Vec::new();
        for item in items {
            let rendered = match item {
                Item::Text { md, .. } => self.r(md),
                Item::Thinking { md, seconds, .. } => self.thinking(md, *seconds),
                Item::Tool(call) => self.tool(call, session, level),
                Item::Notice { text, variant, .. } => match variant {
                    NoticeVariant::Command => format!("`{text}`"),
                    NoticeVariant::Compact => format!("> ⓘ {text}"),
                    v => match v.setting() {
                        Some(what) => format!("*{what}: {}*", self.r(text)),
                        None => format!("*{}*", self.r(text)),
                    },
                },
            };
            if !rendered.is_empty() {
                chunks.push(rendered);
            }
        }
        chunks.join("\n\n")
    }

    fn thinking(&self, md: &str, seconds: f64) -> String {
        if !self.show_thinking {
            return String::new();
        }
        let label = if seconds >= 2.0 { format!("Thought for {}", human_duration((seconds * 1000.0) as u64)) } else { "Thinking".into() };
        format!("<details>\n<summary><i>{}</i></summary>\n\n{}\n\n</details>", esc(&label), self.r(md))
    }

    fn tool(&self, call: &ToolCall, session: &Session, level: usize) -> String {
        if self.tools == "none" {
            return String::new();
        }
        let subject = self.r(&call.subject);
        let mark = status_mark(call.status);
        let duration = human_duration(call.duration_ms);
        let tail = if !duration.is_empty() && call.duration_ms > 1500 { format!(" · {duration}") } else { String::new() };
        let mut summary = format!("<b>{}</b>", esc(&call.name));
        if !subject.is_empty() {
            summary.push_str(&format!(" — <code>{}</code>", esc(&subject)));
        }
        if call.tool_kind == ToolKind::Edit {
            let stat = patch_stat(&call.patch);
            if !stat.is_empty() {
                summary.push_str(&format!(" <code>{stat}</code>"));
            }
        }
        summary.push_str(&esc(&format!("{mark}{tail}")));
        if self.tools == "summary" {
            return format!("- {}", strip_tags(&summary));
        }
        let body = self.tool_body(call, session, level);
        if body.is_empty() {
            return format!("- {}", strip_tags(&summary));
        }
        format!("<details>\n<summary>{summary}</summary>\n\n{body}\n\n</details>")
    }

    fn tool_body(&self, call: &ToolCall, session: &Session, _level: usize) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !call.explanation.is_empty() {
            parts.push(format!("> {}", self.r(&call.explanation)));
        }
        let data = &call.input;
        match call.tool_kind {
            ToolKind::Bash => {
                let command = self.r(&command_text(data));
                if !command.is_empty() {
                    parts.push(code_block(&command, "bash"));
                }
                parts.push(self.bash_output(call));
            }
            ToolKind::Edit => {
                let diff = render_patch(&call.patch, 12);
                if !diff.is_empty() {
                    parts.push(code_block(&self.r(&diff), "diff"));
                } else if !call.old_string.is_empty() || !call.new_string.is_empty() {
                    parts.push(code_block(&self.r(&naive_diff(&call.old_string, &call.new_string, 60)), "diff"));
                } else if let Some(p) = data.get("patch").and_then(Value::as_str) {
                    // Codex's apply_patch carries the patch as an argument.
                    let (t, dropped) = clip(p, self.max_input);
                    parts.push(code_block(&self.r(&t), "diff"));
                    if dropped > 0 {
                        parts.push(format!("*… {dropped} more lines*"));
                    }
                }
                if call.status == CallStatus::Error {
                    parts.push(self.result_block(call));
                }
            }
            ToolKind::Write => {
                let (content, dropped) = clip(data.get("content").and_then(Value::as_str).unwrap_or(""), self.max_input);
                let lang = lang_for_path(data.get("file_path").and_then(Value::as_str).unwrap_or(""));
                if !content.is_empty() {
                    parts.push(code_block(&self.r(&content), lang));
                }
                if dropped > 0 {
                    parts.push(format!("*… {dropped} more lines*"));
                }
            }
            ToolKind::Read => {
                let note = read_note(call);
                if !note.is_empty() {
                    parts.push(format!("*{note}*"));
                }
                if call.status == CallStatus::Error {
                    parts.push(self.result_block(call));
                }
            }
            ToolKind::Ask => {
                // Each question, its options as a checklist with the
                // chosen ones ticked, and words typed instead as a quote.
                for q in questions_of(data) {
                    let chosen = call.answers.iter().find(|(k, _)| *k == q.question).map(|(_, a)| a.clone()).unwrap_or_default();
                    let mut lines = vec![format!("**{}**", esc(&q.question))];
                    let mut typed = !chosen.is_empty();
                    for (label, detail) in &q.options {
                        // A label may hold a comma, so the answer is matched
                        // whole, and as a part only where several were allowed.
                        let on = chosen == *label || (q.multi && chosen.contains(label.as_str()));
                        typed &= !on;
                        let mark = if on { "x" } else { " " };
                        let tail = if detail.is_empty() { String::new() } else { format!(" — {}", esc(detail)) };
                        lines.push(format!("- [{mark}] {}{tail}", esc(label)));
                    }
                    if typed {
                        lines.push(format!("\n> {}", esc(&chosen)));
                    }
                    parts.push(lines.join("\n"));
                }
                if call.answers.is_empty() && call.status == CallStatus::Ok {
                    parts.push("*Not answered.*".into());
                }
            }
            ToolKind::Task => {
                let prompt = data.get("prompt").or(data.get("description")).and_then(Value::as_str).unwrap_or("");
                let (snippet, _) = clip(prompt, 700);
                if !snippet.is_empty() {
                    parts.push(format!("> {}", self.r(&snippet).replace('\n', "\n> ")));
                }
                if !call.subagent.is_empty() {
                    let nested: Vec<String> = call.subagent.iter().map(|sub| self.round(sub, session, 4)).collect();
                    parts.push(nested.join("\n\n"));
                } else {
                    parts.push(self.result_block(call));
                }
            }
            _ => {
                let args = pretty_args(data, self.max_input);
                if !args.is_empty() {
                    parts.push(code_block(&self.r(&args), "json"));
                }
                parts.push(self.result_block(call));
            }
        }
        parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n")
    }

    fn bash_output(&self, call: &ToolCall) -> String {
        let mut chunks = Vec::new();
        let (stdout, dropped) = clip(&call.stdout, self.max_output);
        if !stdout.trim().is_empty() {
            chunks.push(code_block(&self.r(&stdout), "text"));
            if dropped > 0 {
                chunks.push(format!("*… {dropped} more lines*"));
            }
        }
        let (stderr, err_dropped) = clip(&call.stderr, 1500);
        if !stderr.trim().is_empty() {
            chunks.push("*stderr*".into());
            chunks.push(code_block(&self.r(&stderr), "text"));
            if err_dropped > 0 {
                chunks.push(format!("*… {err_dropped} more lines*"));
            }
        }
        if chunks.is_empty() {
            if !call.result_text.trim().is_empty() {
                let (text, dropped) = clip(&call.result_text, self.max_output);
                chunks.push(code_block(&self.r(&text), "text"));
                if dropped > 0 {
                    chunks.push(format!("*… {dropped} more lines*"));
                }
            } else if call.status == CallStatus::Ok {
                chunks.push("*no output*".into());
            }
        }
        chunks.join("\n\n")
    }

    fn result_block(&self, call: &ToolCall) -> String {
        if call.result_text.trim().is_empty() {
            if call.result_images > 0 {
                return format!("*{} image{} returned*", call.result_images, if call.result_images != 1 { "s" } else { "" });
            }
            return String::new();
        }
        let (clipped, dropped) = clip(&call.result_text, self.max_output);
        let block = code_block(&self.r(&clipped), "text");
        if dropped > 0 { format!("{block}\n\n*… {dropped} more lines*") } else { block }
    }
}

/// The command of a shell call, whichever agent wrote it.
pub fn command_text(data: &Map<String, Value>) -> String {
    if let Some(c) = data.get("command").and_then(Value::as_str) {
        return c.to_string();
    }
    if let Some(arr) = data.get("command").and_then(Value::as_array) {
        return arr.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
    }
    data.get("cmd").and_then(Value::as_str).unwrap_or("").to_string()
}

fn strip_tags(html: &str) -> String {
    let mut text = html.to_string();
    for (open, close, mark) in [("<b>", "</b>", "**"), ("<i>", "</i>", "*"), ("<code>", "</code>", "`")] {
        text = text.replace(open, mark).replace(close, mark);
    }
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = re.replace_all(&text, "").into_owned();
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

fn read_note(call: &ToolCall) -> String {
    if call.status == CallStatus::Error {
        return String::new();
    }
    let lines = call.result_text.matches('\n').count();
    if lines > 0 {
        return format!("read {lines} line{}", if lines != 1 { "s" } else { "" });
    }
    if call.result_images > 0 {
        return format!("read {} image{}", call.result_images, if call.result_images != 1 { "s" } else { "" });
    }
    "read".into()
}

pub fn patch_counts(patch: &[Value]) -> (usize, usize) {
    let (mut added, mut removed) = (0, 0);
    for hunk in patch {
        for line in hunk.get("lines").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let s = line.as_str().unwrap_or("");
            if s.starts_with('+') {
                added += 1;
            } else if s.starts_with('-') {
                removed += 1;
            }
        }
    }
    (added, removed)
}

pub fn patch_stat(patch: &[Value]) -> String {
    let (a, r) = patch_counts(patch);
    if a == 0 && r == 0 { String::new() } else { format!("+{a} −{r}") }
}

/// `structuredPatch` -> unified diff text.
pub fn render_patch(patch: &[Value], max_hunks: usize) -> String {
    if patch.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    for hunk in patch.iter().take(max_hunks) {
        let n = |k: &str| hunk.get(k).and_then(Value::as_u64).unwrap_or(0);
        out.push(format!("@@ -{},{} +{},{} @@", n("oldStart"), n("oldLines"), n("newStart"), n("newLines")));
        for line in hunk.get("lines").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            out.push(line.as_str().unwrap_or("").to_string());
        }
    }
    if patch.len() > max_hunks {
        out.push(format!("… {} more hunks", patch.len() - max_hunks));
    }
    out.join("\n")
}

/// Fallback when no structured patch is available: old lines as `-`, new as `+`.
pub fn naive_diff(old: &str, new: &str, limit: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    for l in old.lines() {
        out.push(format!("-{l}"));
    }
    for l in new.lines() {
        out.push(format!("+{l}"));
    }
    if out.len() > limit {
        let extra = out.len() - limit;
        out.truncate(limit);
        out.push(format!("… {extra} more lines"));
    }
    out.join("\n")
}

pub fn pretty_args(data: &Map<String, Value>, limit: usize) -> String {
    if data.is_empty() {
        return String::new();
    }
    let text = serde_json::to_string_pretty(data).unwrap_or_default();
    let (clipped, dropped) = clip(&text, limit);
    if dropped > 0 { format!("{clipped}\n… {dropped} more lines") } else { clipped }
}

pub fn render(session: &Session, cfg: &Config, redactor: &Redactor) -> String {
    MarkdownRenderer::new(cfg, redactor).render(session)
}
