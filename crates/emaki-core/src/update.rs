//! Keeping the app current: what the newest release is, fetching its
//! installer for this machine, and putting it in place.
//!
//! The release lives on GitHub under stable asset names (see WORKFLOW.md),
//! so nothing here needs the API: the newest version is read off the
//! redirect `releases/latest` answers with, and the installer is at
//! `releases/download/v<version>/<asset>`. Installing is per platform:
//!
//! - macOS: the disk image is mounted, `Emaki.app` copied out beside the
//!   running bundle with `ditto` (which keeps the signature and the
//!   attributes), swapped in with two renames, and opened. The old process
//!   keeps running from its unlinked files until the window quits.
//! - Windows: the installer is started; it replaces the files itself once
//!   the running app has quit.
//! - Linux: an AppImage is replaced in place (`APPIMAGE` names the file the
//!   kernel is running) and started again. A `.deb` install is sent to the
//!   release page instead.
//!
//! What was learned is kept in `~/.emaki/state/update.json`: when the last
//! check ran and what it found, so an automatic check runs once a day and
//! the settings panel can say when it last looked.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::driver::version_cmp;
use crate::paths;

pub const REPO: &str = "alkaline-software/emaki";

/// The version this binary is.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The installer for this machine, as the release names it.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some(if cfg!(target_arch = "aarch64") { "Emaki-mac-arm64.dmg" } else { "Emaki-mac-x64.dmg" })
    } else if cfg!(target_os = "windows") {
        Some("Emaki-windows-x64-setup.exe")
    } else if cfg!(target_os = "linux") {
        Some("Emaki-linux-x64.AppImage")
    } else {
        None
    }
}

pub fn release_page(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/v{version}")
}

pub fn asset_url(version: &str, asset: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/v{version}/{asset}")
}

/// The version a `releases/latest` redirect points at: the tag after
/// `/releases/tag/`, without its `v`.
pub fn version_from_location(location: &str) -> Option<String> {
    let tail = location.rsplit("/releases/tag/").next()?;
    let tag = tail.trim_end_matches('/').trim();
    let v = tag.strip_prefix('v').unwrap_or(tag);
    if v.is_empty() || !v.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return None;
    }
    Some(v.to_string())
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    version_cmp(latest, current) == std::cmp::Ordering::Greater
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateState {
    /// When the newest version was last asked for, Unix seconds; 0 = never.
    pub last_check: f64,
    /// What it was then.
    pub latest: String,
}

impl UpdateState {
    fn path() -> PathBuf {
        paths::state_dir().join("update.json")
    }
    pub fn load() -> UpdateState {
        paths::read_json(&Self::path()).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        paths::ensure_dirs()?;
        paths::write_json(&Self::path(), &serde_json::to_value(self)?)
    }
}

fn agent(follow: bool) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(if follow { 8 } else { 0 })
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .user_agent(&format!("emaki/{}", current_version()))
        .build()
}

/// The newest released version, from the network.
pub fn latest() -> Result<String, String> {
    let url = format!("https://github.com/{REPO}/releases/latest");
    let response = match agent(false).get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(describe(e)),
    };
    let status = response.status();
    if (300..400).contains(&status) {
        let location = response.header("Location").unwrap_or("");
        return version_from_location(location).ok_or_else(|| format!("unexpected redirect: {location}"));
    }
    if status == 200 {
        // Redirects off should never land here, but a proxy might.
        return version_from_location(response.get_url()).ok_or_else(|| "no release found".to_string());
    }
    Err(format!("GitHub answered {status}"))
}

fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("GitHub answered {code}"),
        ureq::Error::Transport(t) => match t.kind() {
            ureq::ErrorKind::Dns => "no network (the address could not be resolved)".to_string(),
            ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io => "no network (the connection failed)".to_string(),
            _ => t.to_string(),
        },
    }
}

/// Where installers are kept while they are being fetched.
pub fn downloads_dir() -> PathBuf {
    paths::cache_dir().join("updates")
}

/// Fetch this machine's installer for `version` into the cache, reporting
/// bytes so far and the total when known. Answers the file.
pub fn download(version: &str, progress: &dyn Fn(u64, Option<u64>)) -> Result<PathBuf, String> {
    let asset = asset_name().ok_or("no installer is built for this platform")?;
    let dir = downloads_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(asset);
    let tmp = dir.join(format!("{asset}.part"));
    let response = agent(true).get(&asset_url(version, asset)).call().map_err(describe)?;
    let total = response.header("Content-Length").and_then(|s| s.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    progress(0, total);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("the download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress(done, total);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if let Some(t) = total {
        if done != t {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("the download ended early ({done} of {t} bytes)"));
        }
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Put the fetched installer in place and start the new app. On success
/// the caller quits; the new one is already running (macOS, Linux) or the
/// installer is up (Windows).
pub fn install(installer: &Path) -> Result<(), String> {
    install_into(installer, None)
}

/// `install`, with the bundle to replace named (macOS only; the CLI uses
/// it to try the swap on a throwaway bundle). `None` is the running one.
pub fn install_into(installer: &Path, bundle: Option<&Path>) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    let _ = bundle;
    #[cfg(target_os = "macos")]
    {
        install_macos(installer, bundle)
    }
    #[cfg(target_os = "windows")]
    {
        Command::new(installer).spawn().map(|_| ()).map_err(|e| format!("could not start the installer: {e}"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        install_linux(installer)
    }
}

/// The `.app` the running binary lives in, if it is in one.
pub fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().map(|e| e == "app").unwrap_or(false)).map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn install_macos(dmg: &Path, bundle: Option<&Path>) -> Result<(), String> {
    let bundle = match bundle {
        Some(b) => b.to_path_buf(),
        None => bundle_path().ok_or("Emaki is not running from an app bundle here; open the disk image and drag Emaki to Applications instead")?,
    };
    let parent = bundle.parent().ok_or("the bundle has no parent folder")?;
    let mount = downloads_dir().join("mnt");
    let _ = Command::new("hdiutil").args(["detach", "-quiet"]).arg(&mount).output();
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    let out = Command::new("hdiutil").args(["attach", "-nobrowse", "-readonly", "-quiet", "-mountpoint"]).arg(&mount).arg(dmg).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("the disk image would not mount: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let detach = || {
        let _ = Command::new("hdiutil").args(["detach", "-quiet"]).arg(&mount).output();
    };
    let fresh = mount.join("Emaki.app");
    if !fresh.is_dir() {
        detach();
        return Err("the disk image holds no Emaki.app".into());
    }
    let staged = parent.join("Emaki.app.update");
    let old = parent.join("Emaki.app.old");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&old);
    let out = Command::new("ditto").arg(&fresh).arg(&staged).output().map_err(|e| e.to_string())?;
    detach();
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&staged);
        return Err(format!("could not copy the new app next to this one: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    std::fs::rename(&bundle, &old).map_err(|e| format!("could not move the current app aside ({}): {e}", bundle.display()))?;
    if let Err(e) = std::fs::rename(&staged, &bundle) {
        let _ = std::fs::rename(&old, &bundle);
        let _ = std::fs::remove_dir_all(&staged);
        return Err(format!("could not put the new app in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Command::new("open").arg("-n").arg(&bundle).spawn().map(|_| ()).map_err(|e| format!("the new app is in place but did not start: {e}"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn install_linux(image: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let running = std::env::var_os("APPIMAGE").map(PathBuf::from).ok_or("Emaki is not running as an AppImage here; get the new build from the release page")?;
    let staged = running.with_extension("AppImage.new");
    std::fs::copy(image, &staged).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    std::fs::rename(&staged, &running).map_err(|e| format!("could not replace the AppImage: {e}"))?;
    Command::new(&running).spawn().map(|_| ()).map_err(|e| format!("the new AppImage is in place but did not start: {e}"))
}
