//! Small text helpers for the window.

use chrono::{DateTime, Datelike, Local, Utc};

pub fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// "just now", "4m ago", "3h ago", "yesterday", "Sep 12".
pub fn relative(mtime: f64, now: f64) -> String {
    let delta = (now - mtime).max(0.0);
    if delta < 45.0 {
        return "just now".into();
    }
    if delta < 3600.0 {
        return format!("{}m ago", (delta / 60.0).round() as u64);
    }
    if delta < 86_400.0 {
        return format!("{}h ago", (delta / 3600.0).round() as u64);
    }
    if delta < 172_800.0 {
        return "yesterday".into();
    }
    if delta < 7.0 * 86_400.0 {
        return format!("{}d ago", (delta / 86_400.0).round() as u64);
    }
    let dt = DateTime::<Utc>::from_timestamp(mtime as i64, 0).map(|d| d.with_timezone(&Local));
    dt.map(|d| d.format("%b %-d").to_string()).unwrap_or_default()
}

/// Seconds since an ISO timestamp, as "12s", "4m 03s", "1h 12m".
pub fn elapsed_since(ts: &str, now: f64) -> String {
    let Some(start) = emaki_core::build::parse_ts(ts) else { return String::new() };
    let secs = (now - start.timestamp() as f64).max(0.0) as u64;
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        return format!("{}m {:02}s", secs / 60, secs % 60);
    }
    format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
}

pub fn clock(ts: &str) -> String {
    emaki_core::render_md::fmt_time(ts).chars().take(5).collect()
}

/// When a prompt was sent: "19:39" today, "Oct 2, 19:39" on another day,
/// with the year once it is not this one.
pub fn stamp(ts: &str) -> String {
    let Some(dt) = emaki_core::build::parse_ts(ts).map(|d| d.with_timezone(&Local)) else { return String::new() };
    let today = Local::now().date_naive();
    let form = if dt.date_naive() == today {
        "%H:%M"
    } else if dt.year() == today.year() {
        "%b %-d, %H:%M"
    } else {
        "%b %-d %Y, %H:%M"
    };
    dt.format(form).to_string()
}

pub fn day(ts: &str) -> String {
    emaki_core::render_md::fmt_date(ts)
}

/// Which group of a list by recency a time falls in: the sidebar and the
/// sessions page head their rows with these.
pub fn bucket(mtime: f64) -> &'static str {
    let today = Local::now().date_naive();
    let Some(dt) = DateTime::<Utc>::from_timestamp(mtime as i64, 0).map(|d| d.with_timezone(&Local)) else { return "Earlier" };
    let days = (today - dt.date_naive()).num_days();
    match days {
        i64::MIN..=0 => "Today",
        1 => "Yesterday",
        2..=6 => "This week",
        7..=30 => "This month",
        _ => "Earlier",
    }
}

/// "Friday, 3 October".
pub fn today_line() -> String {
    Local::now().format("%A, %-d %B").to_string()
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("{n} {one}") } else { format!("{n} {many}") }
}
