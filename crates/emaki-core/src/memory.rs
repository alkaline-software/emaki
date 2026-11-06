//! What this process holds in memory, and a report of it written when
//! that grows past what Emaki should ever need.
//!
//! A leak seen on one machine and nowhere else (issue 8, 14 GB while
//! typing) cannot be found from the code: what grew has to be measured
//! where it grew. So the window reads its own footprint on its clock and,
//! past `FIRST_REPORT` and at each doubling after it, writes what the
//! system and the app say it holds to `~/.emaki/logs`. Two reports of one
//! run show what grew between them.

use std::path::PathBuf;

/// The first footprint a report is written at. Emaki runs in well under
/// a gigabyte with a long session open.
pub const FIRST_REPORT: u64 = 3 << 30;

/// How many reports are kept; the oldest go first.
const KEPT: usize = 20;

/// The process's physical footprint in bytes: on a Mac the number
/// Activity Monitor shows as its memory (`proc_pid_rusage`), on Linux its
/// resident set. One system call, cheap enough for the window's clock.
/// None on Windows, which writes no report.
pub fn footprint() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let got = unsafe { libc::proc_pid_rusage(std::process::id() as libc::c_int, libc::RUSAGE_INFO_V4, (&mut info as *mut libc::rusage_info_v4).cast()) };
        (got == 0).then_some(info.ri_phys_footprint)
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let kb: u64 = status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
        Some(kb * 1024)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// The footprint the first report is written at: `FIRST_REPORT`, or
/// `EMAKI_MEMORY_REPORT_MB` for a probe that wants one sooner.
pub fn first_report() -> u64 {
    std::env::var("EMAKI_MEMORY_REPORT_MB").ok().and_then(|v| v.parse::<u64>().ok()).filter(|mb| *mb > 0).map(|mb| mb << 20).unwrap_or(FIRST_REPORT)
}

/// The step a report is due at for `now`, when one is: the largest of
/// `first`, twice it, four times and so on that `now` has reached and
/// `reported` (the last step written, 0 for none) has not.
pub fn report_due(now: u64, reported: u64, first: u64) -> Option<u64> {
    if first == 0 || now < first {
        return None;
    }
    let mut step = first;
    while step.checked_mul(2).is_some_and(|next| next <= now) {
        step *= 2;
    }
    (step > reported).then_some(step)
}

/// Write a report: when and on what, the footprint, what the app says it
/// holds (`held`, its own counts), and the system's view of the process.
/// The system's tools take seconds on a large process, so call this off
/// the window's thread. Returns where it was written.
pub fn write_report(footprint: u64, held: &str) -> std::io::Result<PathBuf> {
    let dir = crate::paths::logs_dir();
    std::fs::create_dir_all(&dir)?;
    let now = chrono::Local::now();
    let path = dir.join(format!("memory-{}.txt", now.format("%Y%m%d-%H%M%S")));
    let mut out = String::new();
    out.push_str(&format!("Emaki {} memory report, {}\n", crate::update::current_version(), now.to_rfc3339()));
    out.push_str(&format!("Footprint: {} MB\n\n", footprint >> 20));
    out.push_str("== What Emaki holds ==\n");
    out.push_str(held.trim_end());
    out.push_str("\n\n");
    for (title, program, args) in system_views() {
        out.push_str(&format!("== {title} ==\n"));
        let pid = std::process::id().to_string();
        let args: Vec<String> = args.iter().map(|a| if *a == "{pid}" { pid.clone() } else { a.to_string() }).collect();
        match crate::process::command(program).args(&args).output() {
            Ok(o) => {
                out.push_str(&String::from_utf8_lossy(&o.stdout));
                out.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            Err(e) => out.push_str(&format!("could not run {program}: {e}\n")),
        }
        out.push('\n');
    }
    crate::paths::write_atomic(&path, out.as_bytes())?;
    prune(&dir);
    Ok(path)
}

/// What the system says of the machine and of this process, as (title,
/// program, arguments): `{pid}` is this process.
fn system_views() -> Vec<(&'static str, &'static str, Vec<&'static str>)> {
    #[cfg(target_os = "macos")]
    {
        vec![
            ("System", "sw_vers", vec![]),
            ("Machine", "sysctl", vec!["hw.model", "hw.memsize", "machdep.cpu.brand_string"]),
            ("Displays", "system_profiler", vec!["SPDisplaysDataType"]),
            ("Footprint", "footprint", vec!["{pid}"]),
            ("Regions", "vmmap", vec!["--summary", "{pid}"]),
        ]
    }
    #[cfg(target_os = "linux")]
    {
        vec![("System", "uname", vec!["-a"]), ("Memory", "cat", vec!["/proc/{pid}/status"]), ("Regions", "cat", vec!["/proc/{pid}/smaps_rollup"])]
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        vec![]
    }
}

/// The newest `KEPT` reports stay; the rest go.
fn prune(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut reports: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("memory-") && n.ends_with(".txt"))).collect();
    // The names sort by when they were written.
    reports.sort();
    let extra = reports.len().saturating_sub(KEPT);
    for old in reports.into_iter().take(extra) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_due_once_a_step() {
        let g = 1u64 << 30;
        assert_eq!(report_due(2 * g, 0, 3 * g), None);
        assert_eq!(report_due(3 * g, 0, 3 * g), Some(3 * g));
        assert_eq!(report_due(5 * g, 3 * g, 3 * g), None);
        assert_eq!(report_due(6 * g, 3 * g, 3 * g), Some(6 * g));
        // A jump past several steps writes one report, at the highest.
        assert_eq!(report_due(13 * g, 3 * g, 3 * g), Some(12 * g));
        assert_eq!(report_due(13 * g, 12 * g, 3 * g), None);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_footprint_is_read() {
        let now = footprint().unwrap();
        assert!(now > 1 << 20 && now < 1 << 40, "{now}");
    }
}
