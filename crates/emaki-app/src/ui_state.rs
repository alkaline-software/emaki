//! What the window remembers between launches: its bounds, whether the
//! sidebar was open, the page, the open tabs and the open folders.
//! `~/.emaki/state/ui.json`, written when any of that changes and again at
//! quit. An unreadable file means defaults, never a refusal to start.

use emaki_core::paths;
use serde::{Deserialize, Serialize};

/// Window bounds in screen pixels, as gpui reports them.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub window: Option<Rect>,
    pub sidebar_open: Option<bool>,
    /// How wide the sidebar was dragged to.
    pub sidebar_w: Option<f32>,
    /// The files panel and the outline beside a conversation.
    pub files_on: Option<bool>,
    pub outline_on: Option<bool>,
    /// How wide that panel was dragged to.
    pub panel_w: Option<f32>,
    /// The list of branches in the order of their names, not by when
    /// each was last committed to.
    pub branches_by_name: Option<bool>,
    /// The terminal at the conversation's right: how wide it was dragged
    /// to, and whether it showed the agent and not the shell.
    pub term_w: Option<f32>,
    pub term_agent: Option<bool>,
    /// `board`, `sessions`, `session` or `new`.
    pub page: String,
    /// Session keys (`<agent>:<id>`) with a tab, in tab order.
    pub tabs: Vec<String>,
    /// The tab that was showing, when the page was a session.
    pub active: Option<String>,
    /// The sidebar's folders showing their sessions; absent until one
    /// has been opened or closed.
    pub folders_open: Option<Vec<String>>,
}

fn file() -> std::path::PathBuf {
    paths::state_dir().join("ui.json")
}

impl UiState {
    pub fn load() -> Self {
        paths::read_json(&file()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = paths::ensure_dirs();
        if let Ok(v) = serde_json::to_value(self) {
            let _ = paths::write_json(&file(), &v);
        }
    }
}
