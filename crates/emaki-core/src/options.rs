//! What an agent lets a session choose: its modes, its models and their
//! effort levels, as the agent itself lists them.
//!
//! Nothing here names a mode, a model or a level. An adapter asks its agent
//! (`Adapter::catalogue`) and fills these in; the window draws whatever came
//! back, so a release that adds a model or a mode shows it with no change
//! here, and another agent brings its own. What was last heard is kept in
//! `cache/options.json`, so the pills have their lists at launch, before any
//! agent has been asked.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::driver::CommandInfo;
use crate::model::AgentId;
use crate::paths;

/// One choice, in the agent's own words: the name it takes on the wire,
/// what to call it, and a line on what it does when the agent gave one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub detail: String,
    /// The colour the agent itself shows this choice in, when it has one:
    /// `0xRRGGBB` on a light ground, then on a dark one.
    #[serde(default)]
    pub color: Option<[u32; 2]>,
    /// The colours the agent runs through this choice's name, letter by
    /// letter, when it shows it in more than one.
    #[serde(default)]
    pub spectrum: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChoice {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub detail: String,
    /// The id the key runs as (`opus` is `claude-opus-5-5`), which is the
    /// name a session reports its model by.
    #[serde(default)]
    pub resolved: String,
    /// The effort levels this model takes; none when it takes none.
    #[serde(default)]
    pub efforts: Vec<Choice>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    pub modes: Vec<Choice>,
    pub models: Vec<ModelChoice>,
    /// The agent's effort levels when it lists them apart from a model:
    /// what a model that is not on the list is offered.
    #[serde(default)]
    pub efforts: Vec<Choice>,
    /// The key that stands for "whatever the agent's default model is",
    /// when the list has one. It resolves to the same id as a named model,
    /// and a session running that id is called by the named one.
    #[serde(default)]
    pub default_model: String,
}

/// What a folder's sessions know: the slash commands, and the choices.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    pub commands: Vec<CommandInfo>,
    pub options: Options,
}

impl Options {
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty() && self.models.is_empty() && self.efforts.is_empty()
    }

    pub fn mode(&self, key: &str) -> Option<&Choice> {
        self.modes.iter().find(|m| m.key == key)
    }

    /// The model `name` means: a key off the list, or the id one resolves
    /// to, a named model before the default's stand-in.
    pub fn model(&self, name: &str) -> Option<&ModelChoice> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let named = |m: &&ModelChoice| m.key != self.default_model;
        self.models
            .iter()
            .find(|m| m.key == name)
            .or_else(|| self.models.iter().filter(named).find(|m| m.resolved == name))
            .or_else(|| self.models.iter().find(|m| m.resolved == name))
    }

    /// The effort levels a session on `model` can be set to.
    pub fn efforts_for(&self, model: &str) -> &[Choice] {
        match self.model(model) {
            Some(m) => &m.efforts,
            None => &self.efforts,
        }
    }

    /// What to call an effort level: the agent's word for it on `model`,
    /// else on any model, else the key made readable.
    pub fn effort_label(&self, model: &str, key: &str) -> String {
        self.efforts_for(model)
            .iter()
            .chain(&self.efforts)
            .chain(self.models.iter().flat_map(|m| &m.efforts))
            .find(|e| e.key == key)
            .map(|e| e.label.clone())
            .unwrap_or_else(|| humanize(key))
    }

    /// What `agent` last said it offers, from `cache/options.json`.
    pub fn cached(agent: AgentId) -> Options {
        let all: BTreeMap<String, Options> =
            paths::read_json(&cache_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        all.get(agent.as_str()).cloned().unwrap_or_default()
    }

    /// Keep what `agent` offers for the next launch. An empty answer is a
    /// failed ask, not an agent with nothing to choose, and is not kept.
    pub fn remember(&self, agent: AgentId) {
        if self.is_empty() {
            return;
        }
        let file = cache_file();
        let mut all: BTreeMap<String, Options> = paths::read_json(&file).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        if all.get(agent.as_str()) == Some(self) {
            return;
        }
        all.insert(agent.as_str().to_string(), self.clone());
        if let Ok(v) = serde_json::to_value(&all) {
            let _ = paths::write_json(&file, &v);
        }
    }
}

fn cache_file() -> std::path::PathBuf {
    paths::cache_dir().join("options.json")
}

/// A key as words, for a choice the agent names and does not describe:
/// `acceptEdits` is "Accept edits", `read-only` is "Read only".
pub fn humanize(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    let mut prev_lower = false;
    for c in key.chars() {
        if c == '-' || c == '_' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        out.extend(c.to_lowercase());
    }
    let mut c = out.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => out,
    }
}
