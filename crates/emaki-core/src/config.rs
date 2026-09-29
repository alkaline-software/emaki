//! Configuration: defaults, `~/.emaki/config.json`, environment overrides.
//! Loading never fails; a malformed file falls back to the defaults.

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Markdown {
    pub enabled: bool,
    /// full | summary | none
    pub tools: String,
    pub thinking: bool,
    pub max_output_chars: usize,
    pub max_input_chars: usize,
}

impl Default for Markdown {
    fn default() -> Self {
        Self { enabled: true, tools: "full".into(), thinking: true, max_output_chars: 4000, max_input_chars: 4000 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Driver {
    pub enabled: bool,
    pub idle_min: u64,
    /// "" = Claude Code's own `permissions.defaultMode`.
    pub default_mode: String,
    /// "" = the account default.
    pub default_model: String,
    pub allow_bypass: bool,
    /// Where `claude` is, when `PATH` and the installers' folders do not
    /// say. "" = search.
    pub claude_path: String,
}

impl Default for Driver {
    fn default() -> Self {
        Self { enabled: true, idle_min: 30, default_mode: String::new(), default_model: String::new(), allow_bypass: false, claude_path: String::new() }
    }
}

/// What the window looks like. Every field has a spelled-out default so a
/// hand-edited `config.json` with one of them missing still loads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// The conversation's typeface: `serif` (Anthropic Serif) or `sans`
    /// (Anthropic Sans).
    pub chat_font: String,
    /// How large the conversation is set: `small`, `medium` or `large`.
    pub chat_size: String,
    /// `system` follows the OS; `light` and `dark` pin one look.
    pub appearance: String,
    /// The accent colour, by name; see `ACCENTS`.
    pub accent: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { chat_font: "serif".into(), chat_size: "medium".into(), appearance: "system".into(), accent: "terracotta".into() }
    }
}

impl AppConfig {
    /// The conversation's body size in pixels for `chat_size`; the reply
    /// text is set at this, the prompt half a pixel smaller, as before.
    pub fn chat_px(&self) -> f32 {
        match self.chat_size.as_str() {
            "small" => 13.5,
            "large" => 16.0,
            _ => 14.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Redact {
    pub enabled: bool,
    pub extra_patterns: Vec<String>,
}

impl Default for Redact {
    fn default() -> Self {
        Self { enabled: true, extra_patterns: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub markdown: Markdown,
    pub driver: Driver,
    pub redact: Redact,
    pub app: AppConfig,
    /// How often the app rescans the session index, in milliseconds.
    pub scan_interval_ms: u64,
    /// Sessions idle longer than this are not polled (they can still be opened).
    pub active_window_min: u64,
    /// Agents to scan. Empty means every agent with data on this machine.
    pub agents: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            markdown: Markdown::default(),
            driver: Driver::default(),
            redact: Redact::default(),
            app: AppConfig::default(),
            scan_interval_ms: 4000,
            active_window_min: 180,
            agents: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        let mut cfg: Config = paths::read_json(&paths::config_file())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if let Ok(v) = std::env::var("EMAKI_DRIVER_MODE") {
            if !v.is_empty() {
                cfg.driver.default_mode = v;
            }
        }
        if let Ok(v) = std::env::var("EMAKI_DRIVER_MODEL") {
            if !v.is_empty() {
                cfg.driver.default_model = v;
            }
        }
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        paths::ensure_dirs()?;
        paths::write_json(&paths::config_file(), &serde_json::to_value(self)?)
    }

    /// Change the file on disk without writing back the environment
    /// overrides `load` applies to what it returns.
    pub fn edit(change: impl FnOnce(&mut Config)) -> std::io::Result<()> {
        let mut on_disk: Config = paths::read_json(&paths::config_file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        change(&mut on_disk);
        on_disk.save()
    }
}
