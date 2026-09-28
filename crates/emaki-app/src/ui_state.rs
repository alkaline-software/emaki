//! What the window remembers between launches: its bounds, whether the
//! sidebar was open, the page, the open tabs and where each was scrolled.
//! `~/.emaki/state/ui.json`, written when any of that changes and again at
//! quit. An unreadable file means defaults, never a refusal to start.

use std::collections::HashMap;

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

/// A position in a conversation: the round at the top of the view and how
/// far into it the view starts.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Scroll {
    pub item: usize,
    pub offset: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub window: Option<Rect>,
    pub sidebar_open: Option<bool>,
    /// `board`, `sessions`, `session` or `new`.
    pub page: String,
    /// Session keys (`<agent>:<id>`) with a tab, in tab order.
    pub tabs: Vec<String>,
    /// The tab that was showing, when the page was a session.
    pub active: Option<String>,
    pub scroll: HashMap<String, Scroll>,
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
