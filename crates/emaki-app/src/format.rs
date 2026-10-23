//! Small text helpers for the window.

use chrono::{DateTime, Datelike, Local, Utc};

use crate::i18n::{count, phrase, t, tf};

pub fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// How long ago, in whole words: "just now", "15 hours ago", "3 days
/// ago", "2 months ago", in the person's language.
pub fn ago(then: f64, now: f64) -> String {
    let delta = (now - then).max(0.0);
    let n = |d: f64| d.round().max(1.0) as usize;
    match delta {
        d if d < 60.0 => t("just now").to_string(),
        d if d < 3600.0 => {
            let k = n(d / 60.0);
            count(k, "{n} minute ago", "{n} minutes ago")
        }
        d if d < 86_400.0 => {
            let k = n(d / 3600.0);
            count(k, "{n} hour ago", "{n} hours ago")
        }
        d if d < 60.0 * 86_400.0 => {
            let k = n(d / 86_400.0);
            count(k, "{n} day ago", "{n} days ago")
        }
        d if d < 365.0 * 86_400.0 => {
            let k = n(d / (30.44 * 86_400.0));
            count(k, "{n} month ago", "{n} months ago")
        }
        d => {
            let k = n(d / (365.25 * 86_400.0));
            count(k, "{n} year ago", "{n} years ago")
        }
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

/// The months' short names, drawn through `t`.
const MONTHS: [&str; 12] = [
    phrase("Jan"),
    phrase("Feb"),
    phrase("Mar"),
    phrase("Apr"),
    phrase("May"),
    phrase("Jun"),
    phrase("Jul"),
    phrase("Aug"),
    phrase("Sep"),
    phrase("Oct"),
    phrase("Nov"),
    phrase("Dec"),
];

fn month(dt: &impl Datelike) -> &'static str {
    t(MONTHS[dt.month0() as usize])
}

/// A day as the person's language writes it: "Oct 2", or "Oct 2, 2025"
/// with its year.
pub fn date(d: &impl Datelike, year: bool) -> String {
    if year {
        tf("{month} {day}, {year}", &[("month", &month(d)), ("day", &d.day()), ("year", &d.year())])
    } else {
        tf("{month} {day}", &[("month", &month(d)), ("day", &d.day())])
    }
}

/// When a prompt was sent: "19:39" today, "Oct 2, 19:39" on another day,
/// with the year once it is not this one, in the language's own order.
pub fn stamp(ts: &str) -> String {
    let Some(dt) = emaki_core::build::parse_ts(ts).map(|d| d.with_timezone(&Local)) else { return String::new() };
    let today = Local::now().date_naive();
    let time = dt.format("%H:%M").to_string();
    if dt.date_naive() == today {
        time
    } else if dt.year() == today.year() {
        tf("{month} {day}, {time}", &[("month", &month(&dt)), ("day", &dt.day()), ("time", &time)])
    } else {
        tf("{month} {day} {year}, {time}", &[("month", &month(&dt)), ("day", &dt.day()), ("year", &dt.year()), ("time", &time)])
    }
}

/// The day something was last touched, for a row's right end: the time
/// today, "Oct 2" on another day, with the year once it is not this one.
pub fn day(mtime: f64) -> String {
    let Some(dt) = DateTime::<Utc>::from_timestamp(mtime as i64, 0).map(|d| d.with_timezone(&Local)) else { return String::new() };
    let today = Local::now().date_naive();
    if dt.date_naive() == today {
        dt.format("%H:%M").to_string()
    } else {
        date(&dt, dt.year() != today.year())
    }
}

/// A count with its thousands set apart as the person's language sets
/// them: 1257 is "1,257", "1.257" in German and Spanish, "1 257" in French.
pub fn thousands(n: usize) -> String {
    let mark = match crate::i18n::lang() {
        "de" | "es" => '.',
        "fr" => '\u{202f}',
        _ => ',',
    };
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (ix, c) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix) % 3 == 0 {
            out.push(mark);
        }
        out.push(c);
    }
    out
}
