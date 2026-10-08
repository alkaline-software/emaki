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
    /// A session the window starts or continues runs as an interactive
    /// Claude Code on a terminal of Emaki's own, with no window (`pty`).
    /// Off, it is the headless `claude -p` child it was before.
    pub hidden_terminal: bool,
}

impl Default for Driver {
    fn default() -> Self {
        Self { enabled: true, idle_min: 30, default_mode: String::new(), default_model: String::new(), allow_bypass: false, claude_path: String::new(), hidden_terminal: true }
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
    /// The key that sends a message: `cmd-enter` (Ctrl off a Mac) or
    /// `enter`. Shift with Enter is a new line either way.
    pub send_key: String,
    /// `system` follows the OS; `light` and `dark` pin one look.
    pub appearance: String,
    /// The accent colour, by name; see `ACCENTS`.
    pub accent: String,
    /// Ask GitHub for the newest release once a day and say so in the
    /// settings panel when there is one. Nothing is installed unasked.
    pub check_updates: bool,
    /// What the window calls the person; empty for the machine's own
    /// account name.
    pub user_name: String,
    /// The person's picture, a file the app keeps a copy of; empty for
    /// the first letter of the name on a disc.
    pub avatar: String,
    /// A sentence typed in the composer starts with a capital.
    pub auto_capitalize: bool,
    /// Spelling and grammar are marked in the composer.
    pub check_writing: bool,
    /// The language they are checked in; see `check::LANGUAGES`.
    pub writing_language: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { chat_font: "serif".into(), chat_size: "medium".into(), send_key: "cmd-enter".into(), appearance: "system".into(), accent: "terracotta".into(), check_updates: true, user_name: String::new(), avatar: String::new(), auto_capitalize: true, check_writing: true, writing_language: "en-US".into() }
    }
}

impl AppConfig {
    /// The reply's pixel size for `chat_size`, the prompt half a pixel
    /// under. Medium is the Claude
    /// desktop app's own body size (its stylesheet's
    /// `font-claude-response-body`: 16px, line height 1.5); the others
    /// step a point and a half either side.
    pub fn chat_px(&self) -> f32 {
        match self.chat_size.as_str() {
            "small" => 14.5,
            "large" => 17.5,
            _ => 16.0,
        }
    }
}

/// The terminal beside a conversation: how it looks and what its pointer
/// does. The look is Kaku's, with the middle step of each size.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminal {
    /// `match` follows the window's appearance; `dark` and `light` keep
    /// Kaku Dark or Kaku Light whatever the window is.
    pub theme: String,
    /// `jetbrains` (JetBrains Mono, built in), `code` (the face the
    /// window sets code in) or `system` (the system's monospaced face).
    pub font: String,
    /// The letters' size: the settings offer 12, 13 and 15.
    pub size: f32,
    /// A row's height over the font's own line: `tight` (1), `normal`
    /// (1.15) or `roomy` (Kaku's 1.28).
    pub spacing: String,
    /// The room around the screen: `compact`, `medium` or `roomy` (Kaku's).
    pub padding: String,
    /// `bar`, `block` or `underline`.
    pub cursor: String,
    pub cursor_blink: bool,
    /// Whether the face may join letters ("->" as an arrow).
    pub ligatures: bool,
    /// A selection is copied when the button is let go.
    pub copy_on_select: bool,
}

impl Default for Terminal {
    fn default() -> Self {
        Self { theme: "match".into(), font: "jetbrains".into(), size: 13., spacing: "normal".into(), padding: "medium".into(), cursor: "bar".into(), cursor_blink: true, ligatures: false, copy_on_select: true }
    }
}

impl Terminal {
    /// What `spacing` multiplies the font's own line by.
    pub fn line_scale(&self) -> f32 {
        match self.spacing.as_str() {
            "tight" => 1.0,
            "roomy" => 1.28,
            _ => 1.15,
        }
    }
}

/// Margin explanations of opaque tool calls, asked of a small model
/// through the `claude` binary. See `explain.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Explain {
    pub enabled: bool,
    pub model: String,
    /// `off`, `permission` (calls waiting on a card) or `all` (every new
    /// call in the conversation showing).
    pub scope: String,
    /// A call shorter than this, with no opaque shape in it, is left alone.
    pub min_chars: usize,
    pub timeout_s: u64,
    /// Tools whose canned line is always enough.
    pub canned_tools: Vec<String>,
}

impl Default for Explain {
    fn default() -> Self {
        Self {
            enabled: true,
            model: "claude-haiku-4-5".into(),
            scope: "permission".into(),
            min_chars: 60,
            timeout_s: 25,
            canned_tools: ["Read", "Glob", "Grep", "NotebookRead", "TodoWrite", "TaskCreate", "TaskUpdate", "TaskList", "TaskGet"].iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Explain {
    /// Whether any call may reach a model at all.
    pub fn active(&self) -> bool {
        self.enabled && self.scope != "off"
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
    pub explain: Explain,
    pub app: AppConfig,
    pub terminal: Terminal,
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
            explain: Explain::default(),
            app: AppConfig::default(),
            terminal: Terminal::default(),
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
