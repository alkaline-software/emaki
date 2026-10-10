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
//! status-line command Claude Code runs is handed the same windows on
//! stdin, and Emaki's script (`statusline.rs`, installed from the
//! settings panel) leaves a copy in `~/.emaki/state/rate_limits.json`,
//! which `refresh_from_statusline` reads, so a terminal session keeps the
//! row current too. What was learned is kept in `~/.emaki/state/limits.json`
//! with the time it was seen, and the newer source wins.
//!
//! The context percentage is two numbers. The tokens in use need no wire:
//! the last assistant row's usage (input, cache read, cache creation) is
//! what the next request carries, and `build` records it as
//! `Session::context_tokens`. The size of the window does: one model id
//! comes in two sizes (`claude-opus-5-5` is 200k or 1M by the session's
//! choice), and the transcript never says which. The status line is
//! handed it, per session, and leaves it in
//! `state/context/<session>.json` (`session_context`); `window_for`
//! takes that, else what was learned for the model, else the assumption,
//! and never a window smaller than what is in it.

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

impl Window {
    /// The window as it stands at `now`. Past its reset nothing is spent
    /// and no next reset is known: Claude Code leaves a window that has
    /// run out off what it hands over until a request starts the next
    /// one, so what is held is still the old window's.
    pub fn at(self, now: f64) -> Window {
        if self.resets_at > 0.0 && self.resets_at <= now { Window::default() } else { self }
    }
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
    /// Codex's windows for the account, the shorter first, and when a
    /// rollout last recorded them.
    pub codex: Vec<crate::model::UsageWindow>,
    pub codex_seen_at: f64,
}

/// What the status line was last handed about one session:
/// `{"model", "effort", "window", "used", "seen_at"}`. Claude Code reruns
/// the status line when the model or the effort changes (its own refresh
/// list names `mainLoopModel` and `effortValue`), so this is the
/// terminal's word on both, sooner than the transcript. The permission
/// mode is on that refresh list too and is not in what the script is
/// handed (read from a captured input, 2.1.288), so the mode a terminal
/// session is in stays what its transcript last recorded.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct SessionContext {
    pub model: String,
    /// The effort level; empty when the model takes none.
    pub effort: String,
    /// The window's size in tokens.
    pub window: u64,
    pub used: u64,
    pub seen_at: f64,
}

/// Where the status line leaves each session's context, one file a session.
pub fn context_dir() -> std::path::PathBuf {
    paths::state_dir().join("context")
}

/// What the terminal's status line last said of `session`, if it ever
/// ran for it and named a window.
pub fn session_context(session: &str) -> Option<SessionContext> {
    if session.is_empty() {
        return None;
    }
    let v = paths::read_json(&context_dir().join(format!("{session}.json")))?;
    serde_json::from_value::<SessionContext>(v).ok().filter(|c| c.window > 0)
}

/// Drop the context files of sessions the status line has not run for in
/// `max_age` seconds. Returns how many went.
pub fn prune_contexts(now: f64, max_age: f64) -> usize {
    let Ok(dir) = std::fs::read_dir(context_dir()) else { return 0 };
    let mut gone = 0;
    for e in dir.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .is_some_and(|t| now - t.as_secs_f64() > max_age);
        if old && std::fs::remove_file(e.path()).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// A model id without the `[1m]` the status line may put after it.
fn bare_model(model: &str) -> &str {
    model.split('[').next().unwrap_or(model)
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

    /// What a Codex session's rollout last recorded of the account's
    /// windows. They are the account's, so the newest of any session's
    /// stands for all. Returns whether anything was learned.
    pub fn absorb_codex(&mut self, windows: &[crate::model::UsageWindow], at: f64) -> bool {
        if windows.is_empty() || at <= self.codex_seen_at {
            return false;
        }
        self.codex = windows.to_vec();
        self.codex_seen_at = at;
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

    /// The context window for `model`: what a driver or a status line
    /// reported, else the assumption.
    pub fn context_window(&self, model: &str) -> u64 {
        self.context_windows.get(model).copied().unwrap_or_else(|| assumed_context_window(model))
    }

    /// Remember the window a status line named for a session's model, so
    /// a session no status line has run for starts from the size last
    /// seen for that model. Returns whether anything changed.
    pub fn learn_window(&mut self, ctx: &SessionContext) -> bool {
        let model = bare_model(&ctx.model);
        if model.is_empty() || ctx.window == 0 || self.context_windows.get(model) == Some(&ctx.window) {
            return false;
        }
        self.context_windows.insert(model.to_string(), ctx.window);
        true
    }

    /// The window a session's percentage is taken of. The session's own,
    /// as its status line named it, while that is still about the model
    /// the transcript is on; else the model's. `tokens` in use is the
    /// floor: a context larger than the window it is supposed to be in
    /// means the window is the large one, whatever was assumed.
    pub fn window_for(&self, model: &str, tokens: u64, ctx: Option<&SessionContext>) -> u64 {
        let own = ctx.filter(|c| c.window > 0 && (model.is_empty() || c.model.is_empty() || bare_model(&c.model) == bare_model(model)));
        let window = own.map(|c| c.window).unwrap_or_else(|| self.context_window(model));
        if tokens > window { window.max(1_000_000) } else { window }
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
