//! Find inside one conversation: the ⌘F of the window.
//!
//! `search` answers "which sessions mention this" from the FTS index; this
//! answers "where in the one I am reading". It is a plain case-insensitive
//! substring match over every item the transcript has (the prompt, each
//! reply, each thought, each tool call with its arguments and output, the
//! rounds of a subagent counted against the Task call that spawned it), so
//! the answer is exactly what the page shows and needs no index on disk.
//!
//! The text is lowered once per session, when the bar opens, so typing a
//! query costs one scan of a `Vec<String>`, not a re-render of the model.

use crate::model::{Item, Round, Session, ToolCall};

/// One place a query matches: a round, and the item in it, or the prompt
/// when `item` is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub round: usize,
    pub item: Option<usize>,
}

struct Doc {
    round: usize,
    item: Option<usize>,
    /// The searchable text, lower-cased once.
    text: String,
}

/// Every searchable piece of one session, ready to be scanned.
pub struct FindIndex {
    docs: Vec<Doc>,
}

impl FindIndex {
    pub fn build(session: &Session) -> Self {
        let mut docs = Vec::new();
        for (ix, rnd) in session.rounds.iter().enumerate() {
            if !rnd.prompt.trim().is_empty() {
                docs.push(Doc { round: ix, item: None, text: rnd.prompt.to_lowercase() });
            }
            for (jx, item) in rnd.items.iter().enumerate() {
                let text = match item {
                    Item::Text { md, .. } | Item::Thinking { md, .. } => md.clone(),
                    Item::Notice { text, .. } => text.clone(),
                    Item::Tool(call) => tool_text(call),
                };
                if text.trim().is_empty() {
                    continue;
                }
                docs.push(Doc { round: ix, item: Some(jx), text: text.to_lowercase() });
            }
        }
        Self { docs }
    }

    /// Every place `query` occurs, in reading order. An empty or blank
    /// query matches nothing.
    pub fn find(&self, query: &str) -> Vec<Hit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        self.docs.iter().filter(|d| d.text.contains(&q)).map(|d| Hit { round: d.round, item: d.item }).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

/// What a tool call is searchable by: its name and subject, the arguments a
/// person would recognise, what came back, and the subagent it ran.
fn tool_text(call: &ToolCall) -> String {
    let mut parts = vec![call.name.clone(), call.subject.clone()];
    for key in ["command", "pattern", "query", "url", "description", "prompt", "file_path", "content", "new_string", "old_string"] {
        if let Some(v) = call.input.get(key).and_then(serde_json::Value::as_str) {
            if !v.trim().is_empty() {
                parts.push(v.to_string());
            }
        }
    }
    if !call.explanation.is_empty() {
        parts.push(call.explanation.clone());
    }
    for chunk in [&call.stdout, &call.stderr, &call.result_text] {
        if !chunk.trim().is_empty() {
            parts.push(chunk.clone());
        }
    }
    subagent_text(&call.subagent, &mut parts);
    parts.join("\n")
}

fn subagent_text(rounds: &[Round], out: &mut Vec<String>) {
    for rnd in rounds {
        if !rnd.prompt.trim().is_empty() {
            out.push(rnd.prompt.clone());
        }
        for item in &rnd.items {
            match item {
                Item::Text { md, .. } | Item::Thinking { md, .. } => out.push(md.clone()),
                Item::Notice { text, .. } => out.push(text.clone()),
                Item::Tool(c) => {
                    out.push(format!("{} {}", c.name, c.subject));
                    if !c.result_text.trim().is_empty() {
                        out.push(c.result_text.clone());
                    }
                    subagent_text(&c.subagent, out);
                }
            }
        }
    }
}
