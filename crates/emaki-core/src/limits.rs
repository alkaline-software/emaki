//! The account's usage windows and the models' context sizes, as the
//! terminal's status line shows them: `Context 37% | 5h: 3% (4h46m) | 7d:
//! 6% (6d7h)`.
//!
//! Claude Code learns both from the API on every request and hands them
//! to a status-line script, but writes neither to the transcript. Emaki
//! sees them two ways. The stream-json wire of its own drivers: a
//! `rate_limit_event` frame after each turn carries the five-hour and
//! seven-day windows (they are per account, so one driver's answer holds
//! for every session), and the `result` frame's `modelUsage` names each
//! model's `contextWindow`. And the terminal's own status line: the
//! script Claude Code runs (`~/.claude/statusline.sh` here) is handed the
//! same windows on stdin and leaves a copy in
//! `~/.emaki/state/rate_limits.json`, which `refresh_from_statusline`
//! reads, so a terminal session keeps the row current too. What was
//! learned is kept in `~/.emaki/state/limits.json` with the time it was
//! seen, and the newer source wins.
//!
//! The context percentage itself needs no wire: the last assistant row's
//! usage (input, cache read, cache creation) is what the next request
//! carries, and `build` records it as `Session::context_tokens`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::json::*;
use crate::paths;

/// One usage window: how much of it is spent (0 to 1) and when it resets
/// (Unix seconds).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub utilization: f64,
    pub resets_at: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    /// When the windows were last read off a wire, Unix seconds; 0 = never.
    pub seen_at: f64,
    /// Context window size per model id, as `modelUsage` reported it.
    pub context_windows: BTreeMap<String, u64>,
}

/// The size assumed for a model nothing has reported on yet. A `[1m]`
/// alias says so itself; `claude-fable-5-1` answered 1,000,000 on the wire
/// checked against Claude Code 2.1.284; everything else is the standard
/// 200k until a driver's `result` says otherwise.
pub fn assumed_context_window(model: &str) -> u64 {
    if model.contains("[1m]") || model.contains("fable") {
        1_000_000
    } else {
        200_000
    }
}

impl Limits {
    fn path() -> std::path::PathBuf {
        paths::state_dir().join("limits.json")
    }

    pub fn load() -> Limits {
        paths::read_json(&Self::path()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        paths::ensure_dirs()?;
        paths::write_json(&Self::path(), &serde_json::to_value(self)?)
    }

    /// The `rate_limit_info` object of a `rate_limit_event` frame. Returns
    /// whether anything was learned.
    pub fn absorb_rate_limit(&mut self, info: &Value, now: f64) -> bool {
        let Some(windows) = info.get("unifiedWindows").and_then(Value::as_object) else { return false };
        let read = |key: &str| -> Option<Window> {
            let w = windows.get(key)?;
            let utilization = w.get("utilization").and_then(Value::as_f64)?;
            let resets_at = w.get("resetsAt").and_then(Value::as_f64).unwrap_or(0.0);
            Some(Window { utilization, resets_at })
        };
        let (five, seven) = (read("five_hour"), read("seven_day"));
        if five.is_none() && seven.is_none() {
            return false;
        }
        if five.is_some() {
            self.five_hour = five;
        }
        if seven.is_some() {
            self.seven_day = seven;
        }
        self.seen_at = now;
        true
    }

    /// Where the terminal's status line leaves the windows it was handed:
    /// `{"rate_limits": {"five_hour": {"used_percentage", "resets_at"},
    /// "seven_day": {...}}, "seen_at": <unix seconds>}`.
    pub fn statusline_path() -> std::path::PathBuf {
        paths::state_dir().join("rate_limits.json")
    }

    /// One status-line stdin, as the script wrote it. Absorbed only when
    /// it is newer than what is held, so a driver's fresher answer is not
    /// overwritten by a stale file. Returns whether anything was learned.
    pub fn absorb_statusline(&mut self, v: &Value) -> bool {
        let seen = v.get("seen_at").and_then(Value::as_f64).unwrap_or(0.0);
        if seen <= self.seen_at {
            return false;
        }
        let Some(rl) = v.get("rate_limits").and_then(Value::as_object) else { return false };
        let read = |key: &str| -> Option<Window> {
            let w = rl.get(key)?;
            let pct = w.get("used_percentage").and_then(Value::as_f64)?;
            let resets_at = w.get("resets_at").and_then(Value::as_f64).unwrap_or(0.0);
            Some(Window { utilization: pct / 100.0, resets_at })
        };
        let (five, seven) = (read("five_hour"), read("seven_day"));
        if five.is_none() && seven.is_none() {
            return false;
        }
        if five.is_some() {
            self.five_hour = five;
        }
        if seven.is_some() {
            self.seven_day = seven;
        }
        self.seen_at = seen;
        true
    }

    /// Read the status line's file if there is one. Cheap enough to call
    /// every second: a few hundred bytes, and nothing when unchanged.
    pub fn refresh_from_statusline(&mut self) -> bool {
        paths::read_json(&Self::statusline_path()).map(|v| self.absorb_statusline(&v)).unwrap_or(false)
    }

    /// The `modelUsage` object of a `result` frame: each model's context
    /// window. Returns whether anything changed.
    pub fn absorb_model_usage(&mut self, model_usage: &Value) -> bool {
        let Some(models) = model_usage.as_object() else { return false };
        let mut changed = false;
        for (model, v) in models {
            let size = u64_of(v, "contextWindow");
            if size > 0 && self.context_windows.get(model) != Some(&size) {
                self.context_windows.insert(model.clone(), size);
                changed = true;
            }
        }
        changed
    }

    /// The context window for `model`: what a driver reported, else the
    /// assumption.
    pub fn context_window(&self, model: &str) -> u64 {
        self.context_windows.get(model).copied().unwrap_or_else(|| assumed_context_window(model))
    }
}

/// `4h46m`, `6d7h`, `12m`: how long until `resets_at`, or empty once past.
pub fn until(resets_at: f64, now: f64) -> String {
    let secs = resets_at - now;
    if secs <= 0.0 {
        return String::new();
    }
    let mins = (secs / 60.0) as u64;
    let hours = mins / 60;
    let days = hours / 24;
    if days >= 1 {
        format!("{days}d{}h", hours % 24)
    } else if hours >= 1 {
        format!("{hours}h{}m", mins % 60)
    } else {
        format!("{mins}m")
    }
}
