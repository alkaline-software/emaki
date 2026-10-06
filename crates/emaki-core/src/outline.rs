//! A conversation's outline: one entry a round, what was asked and what
//! came of it, each in a line.
//!
//! A pure function of the model, like everything drawn: no model is asked
//! to summarise anything. The title is the prompt's first line that says
//! something; the gist is the first line of the last thing the agent
//! wrote that round, which is where a reply states its outcome, or, when
//! it wrote nothing, what it did instead.

use crate::model::{Item, NoticeVariant, Round, Session, Source};

/// The most characters a line of the outline keeps.
const LINE_MAX: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The round's place in `Session::rounds`.
    pub round: usize,
    pub ts: String,
    pub title: String,
    pub gist: String,
    pub tools: usize,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Prompt,
    /// A slash command, or anything else Claude Code wrote as a prompt.
    Command,
    /// A message from another session.
    Peer,
    /// The round holds a compaction boundary.
    Compact,
    /// Sent mid-turn and not taken up yet.
    Queued,
}

/// One entry for every round, in order.
pub fn of(session: &Session) -> Vec<Entry> {
    session.rounds.iter().enumerate().map(|(ix, rnd)| entry(ix, rnd)).collect()
}

fn entry(ix: usize, rnd: &Round) -> Entry {
    let compact = rnd.items.iter().any(|i| matches!(i, Item::Notice { variant: NoticeVariant::Compact, .. }));
    let kind = if rnd.queued {
        Kind::Queued
    } else if compact {
        Kind::Compact
    } else if rnd.source == Source::Peer {
        Kind::Peer
    } else if matches!(rnd.source, Source::Command | Source::System) || rnd.prompt.trim_start().starts_with('/') {
        Kind::Command
    } else {
        Kind::Prompt
    };
    let mut title = first_line(&rnd.prompt);
    if title.is_empty() {
        title = match rnd.attachments.first() {
            Some(a) => a.name.clone(),
            None if rnd.images > 0 => "(a picture)".to_string(),
            None => "(no prompt)".to_string(),
        };
    }
    let tools = rnd.tool_count();
    Entry { round: ix, ts: rnd.ts.clone(), title, gist: gist(rnd, tools), tools, kind }
}

/// What came of the round, in a line.
fn gist(rnd: &Round, tools: usize) -> String {
    let said = rnd.items.iter().rev().find_map(|i| match i {
        Item::Text { md, .. } => Some(first_line(md)).filter(|l| !l.is_empty()),
        _ => None,
    });
    if let Some(said) = said {
        return said;
    }
    let noticed = rnd.items.iter().rev().find_map(|i| match i {
        Item::Notice { variant: NoticeVariant::Compact, .. } => Some("Conversation compacted".to_string()),
        Item::Notice { text, variant, .. } => Some(plain(&variant.said(text))).filter(|l| !l.is_empty()),
        _ => None,
    });
    match noticed {
        Some(n) => n,
        None if tools == 1 => "1 tool call".to_string(),
        None if tools > 1 => format!("{tools} tool calls"),
        None => String::new(),
    }
}

/// The first line of `text` that says something, as plain words.
pub fn first_line(text: &str) -> String {
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || t.starts_with("Attached file:") || t.chars().all(|c| matches!(c, '-' | '=' | '*' | '_' | '|' | ' ')) {
            continue;
        }
        let p = plain(t);
        if p.chars().any(char::is_alphanumeric) {
            return p;
        }
    }
    String::new()
}

/// A line of markdown as the words it shows: no heading or list marks, no
/// emphasis, a link as its text, a pasted picture's mark left out.
fn plain(line: &str) -> String {
    let mut t = line.trim();
    t = t.trim_start_matches('#').trim_start();
    t = t.trim_start_matches('>').trim_start();
    for mark in ["- [ ] ", "- [x] ", "- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(mark) {
            t = rest;
            break;
        }
    }
    let mut out = String::with_capacity(t.len());
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '`' => {}
            '*' | '_' if chars.get(i + 1) == Some(&c) => i += 1,
            '*' => {}
            // "[Image #3]" is Claude Code's mark for a pasted picture.
            '[' if chars[i..].iter().collect::<String>().starts_with("[Image #") => {
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
            }
            // "[text](url)" reads as its text.
            '[' => {
                let close = chars[i..].iter().position(|&c| c == ']').map(|n| i + n);
                match close {
                    Some(end) if chars.get(end + 1) == Some(&'(') => {
                        out.extend(&chars[i + 1..end]);
                        i = chars[end..].iter().position(|&c| c == ')').map(|n| end + n).unwrap_or(chars.len());
                    }
                    _ => out.push(c),
                }
            }
            _ => out.push(c),
        }
        i += 1;
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    match out.char_indices().nth(LINE_MAX) {
        Some((cut, _)) => format!("{}…", out[..cut].trim_end()),
        None => out,
    }
}
