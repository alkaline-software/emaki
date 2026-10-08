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

/// How long ago, in whole words: "just now", "15 hours ago", "3 days
/// ago", "2 months ago".
pub fn ago(then: f64, now: f64) -> String {
    let delta = (now - then).max(0.0);
    let count = |n: f64, one: &str| {
        let n = n.round().max(1.0) as u64;
        format!("{n} {one}{} ago", if n == 1 { "" } else { "s" })
    };
    match delta {
        d if d < 60.0 => "just now".into(),
        d if d < 3600.0 => count(d / 60.0, "minute"),
        d if d < 86_400.0 => count(d / 3600.0, "hour"),
        d if d < 60.0 * 86_400.0 => count(d / 86_400.0, "day"),
        d if d < 365.0 * 86_400.0 => count(d / (30.44 * 86_400.0), "month"),
        d => count(d / (365.25 * 86_400.0), "year"),
    }
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

/// A count with its thousands set apart: 1257 is "1,257".
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (ix, c) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
