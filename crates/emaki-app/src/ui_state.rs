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
    /// How wide the files panel was dragged to. Whether it shows, as the
    /// outline, the layout and the terminal panel, is each tab's and is not
    /// kept past a launch (`Workbench::tab_view`).
    pub panel_w: Option<f32>,
    /// How wide the outline's drawer is.
    pub outline_w: Option<f32>,
    /// The list of branches in the order of their names, not by when
    /// each was last committed to.
    pub branches_by_name: Option<bool>,
    /// The shell terminal at the conversation's right: how wide it was
    /// dragged to.
    pub term_w: Option<f32>,
    /// How wide the file shown beside the conversation was dragged to.
    pub file_w: Option<f32>,
    /// The structure at the files panel's foot: open or folded to its
    /// head, and how tall it was dragged to.
    pub file_structure_open: Option<bool>,
    pub file_structure_h: Option<f32>,
    /// What stands beside a PDF's pages: `pages`, `contents` or nothing.
    pub pdf_side: Option<String>,
    /// The agent a new session is begun with, and what a new Codex
    /// session starts in: its mode, model and effort.
    pub new_agent: Option<String>,
    pub codex_next: Option<[String; 3]>,
    /// The modes Claude Code's ⇧Tab goes round, as last seen to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_cycle: Option<Vec<String>>,
    /// `sessions`, `session` or `new`.
    pub page: String,
    /// Session keys (`<agent>:<id>`) with a tab, in tab order.
    pub tabs: Vec<String>,
    /// The tab that was showing, when the page was a session.
    pub active: Option<String>,
    /// The sidebar's folders showing their sessions; absent until one
    /// has been opened or closed.
    pub folders_open: Option<Vec<String>>,
    /// The projects pinned to the head of the sidebar's list, in the
    /// order they stand there.
    pub pinned: Vec<String>,
    /// The projects put out of the sidebar's list, under "Hidden" at
    /// its foot.
    pub hidden: Vec<String>,
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
