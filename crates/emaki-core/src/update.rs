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
//!   attributes), and swapped in with two renames. The old process keeps
//!   running from its unlinked files until the window quits, and the new
//!   app is opened once it has: never two at once.
//! - Windows: a copy Velopack installed (every one from v0.3.1) is updated
//!   by Velopack: its feed for this architecture, beside the installers on
//!   the release, says what the newest package is, which is fetched and
//!   applied by its updater once the app has quit, and the app opened
//!   again. A copy from before (NSIS's installer) or a build of one's own
//!   fetches the installer and starts it.
//! - Linux: an AppImage is replaced in place (`APPIMAGE` names the file the
//!   kernel is running) and started again. A `.deb` install fetches the
//!   new `.deb` and has apt install it, through the system's password
//!   prompt (`pkexec`), then starts the new binary.
//!
//! What was learned is kept in `~/.emaki/state/update.json`: when the last
//! check ran and what it found, so an automatic check runs once a day and
//! the settings panel can say when it last looked.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
        Some(if cfg!(target_arch = "aarch64") { "Emaki-windows-arm64-setup.exe" } else { "Emaki-windows-x64-setup.exe" })
    } else if cfg!(target_os = "linux") {
        // What this copy was installed as: an AppImage says so in `APPIMAGE`.
        let image = std::env::var_os("APPIMAGE").is_some();
        Some(match (cfg!(target_arch = "aarch64"), image) {
            (true, true) => "Emaki-linux-arm64.AppImage",
            (false, true) => "Emaki-linux-x64.AppImage",
            (true, false) => "Emaki-linux-arm64.deb",
            (false, false) => "Emaki-linux-x64.deb",
        })
    } else {
        None
    }
}

/// Whether this copy came from a store that keeps it current itself, and
/// nothing here updates it: the Microsoft Store (Windows runs a Store
/// package from its own folder, `WindowsApps`) or the Snap Store (snapd
/// starts a snap with `SNAP_NAME` set, `in_snap`).
pub fn from_store() -> bool {
    (cfg!(windows) && std::env::current_exe().is_ok_and(|p| p.components().any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps")))) || in_snap()
}

/// Whether this copy is the snap, started by snapd.
pub fn in_snap() -> bool {
    cfg!(target_os = "linux") && std::env::var("SNAP_NAME").is_ok_and(|n| n == "emaki")
}

/// The project's own page.
pub fn repo_page() -> String {
    format!("https://github.com/{REPO}")
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
    #[cfg(windows)]
    if let Some(um) = velopack() {
        return velopack_download(&um, progress);
    }
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
/// the caller quits: the new one starts once it has (macOS), is already
/// running (Linux), or the installer is up (Windows).
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
        if installer.extension().is_some_and(|e| e == "nupkg") {
            return velopack_apply();
        }
        crate::process::command(installer).spawn().map(|_| ()).map_err(|e| format!("could not start the installer: {e}"))
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
    let _ = crate::process::command("hdiutil").args(["detach", "-quiet"]).arg(&mount).output();
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    let out = crate::process::command("hdiutil").args(["attach", "-nobrowse", "-readonly", "-quiet", "-mountpoint"]).arg(&mount).arg(dmg).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("the disk image would not mount: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let detach = || {
        let _ = crate::process::command("hdiutil").args(["detach", "-quiet"]).arg(&mount).output();
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
    let out = crate::process::command("ditto").arg(&fresh).arg(&staged).output().map_err(|e| e.to_string())?;
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
    open_once_gone(std::process::id(), &bundle).map_err(|e| format!("the new app is in place but did not start: {e}"))
}

/// Open `bundle` once the process `pid` has ended: a shell of its own
/// that outlives this one and waits on the pid, not on a length of time.
/// The new app must not start while the old one runs. Opened beside it
/// (`open -n`), it was a second Emaki to the Dock, which gave it an icon
/// of its own at the Dock's end and left the one the person keeps there
/// without a running app.
#[cfg(target_os = "macos")]
fn open_once_gone(pid: u32, bundle: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    crate::process::command("/bin/sh")
        .args(["-c", r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$2""#, "sh"])
        .arg(pid.to_string())
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn install_linux(image: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if image.extension().is_some_and(|e| e == "deb") {
        return install_deb(image);
    }
    let running = std::env::var_os("APPIMAGE").map(PathBuf::from).ok_or("Emaki is not running as an AppImage here; get the new build from the release page")?;
    let staged = running.with_extension("AppImage.new");
    std::fs::copy(image, &staged).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    std::fs::rename(&staged, &running).map_err(|e| format!("could not replace the AppImage: {e}"))?;
    crate::process::command(&running).spawn().map(|_| ()).map_err(|e| format!("the new AppImage is in place but did not start: {e}"))
}

/// A `.deb` installed over the one running, by apt (which also brings
/// any library the new one needs), as root through `pkexec`: the
/// system's own password prompt. The running binary keeps its unlinked
/// file; the new one is started from the same path.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn install_deb(deb: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let deb = std::fs::canonicalize(deb).map_err(|e| e.to_string())?;
    let out = crate::process::command("pkexec").args(["apt-get", "install", "-y"]).arg(&deb).output().map_err(|e| format!("could not ask for the password to install: {e}"))?;
    if !out.status.success() {
        // pkexec answers 126 when the prompt was dismissed.
        if out.status.code() == Some(126) {
            return Err("the install was not allowed".into());
        }
        return Err(format!("apt could not install the new version: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let exe = exe.to_string_lossy().trim_end_matches(" (deleted)").to_string();
    crate::process::command(&exe).spawn().map(|_| ()).map_err(|e| format!("the new version is installed but did not start: {e}"))
}

/// Velopack's view of this copy, when Velopack installed it: its feeds
/// are read from the latest release's files, as the installers are, or
/// from the folder or address `EMAKI_UPDATE_FEED` names, for a probe.
#[cfg(windows)]
fn velopack() -> Option<velopack::UpdateManager> {
    // An address on github.com would be read through GitHub's API by
    // `AutoSource`, sixty times an hour at most; the files themselves are not.
    match std::env::var("EMAKI_UPDATE_FEED") {
        Ok(feed) => velopack::UpdateManager::new(velopack::sources::AutoSource::new(&feed), None, None),
        Err(_) => velopack::UpdateManager::new(velopack::sources::HttpSource::new(format!("https://github.com/{REPO}/releases/latest/download")), None, None),
    }
    .ok()
}

/// The newest package fetched into Velopack's own folder, reporting its
/// bytes as the installer's download does. Answers the package.
#[cfg(windows)]
fn velopack_download(um: &velopack::UpdateManager, progress: &dyn Fn(u64, Option<u64>)) -> Result<PathBuf, String> {
    let info = match um.check_for_updates().map_err(|e| format!("the release feed could not be read: {e}"))? {
        velopack::UpdateCheck::UpdateAvailable(info) => info,
        _ => return Err("the release has no newer package for this computer".into()),
    };
    let total = info.TargetFullRelease.Size;
    let (tx, rx) = std::sync::mpsc::channel::<i16>();
    let fetch = {
        let (um, info) = (um.clone(), info.clone());
        std::thread::spawn(move || um.download_updates(&info, Some(tx)))
    };
    progress(0, Some(total));
    for percent in rx {
        progress(total * percent.clamp(0, 100) as u64 / 100, Some(total));
    }
    fetch.join().map_err(|_| "the download stopped".to_string())?.map_err(|e| format!("the download stopped: {e}"))?;
    let here = velopack::locator::auto_locate_app_manifest(velopack::locator::LocationContext::FromCurrentExe).map_err(|e| e.to_string())?;
    Ok(here.get_packages_dir().join(&info.TargetFullRelease.FileName))
}

/// Velopack's updater is started and waits for this app to quit, then
/// puts the fetched package in place and opens the new version.
#[cfg(windows)]
fn velopack_apply() -> Result<(), String> {
    let um = velopack().ok_or("Velopack did not install this copy")?;
    let asset = um.get_update_pending_restart().ok_or("the new version was not fetched")?;
    um.wait_exit_then_apply_updates(&asset, false, true, Vec::<String>::new()).map_err(|e| format!("could not start the updater: {e}"))
}

/// Where Velopack puts the app: `%LOCALAPPDATA%\<pack id>`. Not `Emaki`,
/// NSIS's folder until v0.3.0, where Velopack's installer found an app
/// already installed and asked whether to repair it. release.yml packs
/// under it.
pub const WINDOWS_PACK_ID: &str = "AlkalineSoftware.Emaki";

/// What NSIS's installer left (v0.3.0 and before), taken away once this
/// copy is Velopack's: its program and uninstaller, their folder, its
/// uninstall entry and Start menu folder. The desktop's shortcut has the
/// same name as Velopack's, which replaced it. NSIS's uninstaller is not
/// run: it ends every `Emaki.exe`, this one too. Looked for at each
/// start, so a program still running at the first is taken at the next.
#[cfg(windows)]
pub fn drop_nsis_install() {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return };
    let old = PathBuf::from(local).join("Emaki");
    if !old.join("uninstall.exe").is_file() || velopack::locator::auto_locate_app_manifest(velopack::locator::LocationContext::FromCurrentExe).is_err() {
        return;
    }
    if std::fs::remove_file(old.join("Emaki.exe")).is_err() && old.join("Emaki.exe").exists() {
        return;
    }
    let _ = std::fs::remove_file(old.join("uninstall.exe"));
    let _ = std::fs::remove_dir(&old);
    let _ = windows_registry::CURRENT_USER.remove_tree(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Emaki");
    let _ = windows_registry::CURRENT_USER.remove_tree(r"Software\Alkaline Software\Emaki");
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let folder = PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Emaki");
        let _ = std::fs::remove_file(folder.join("Emaki.lnk"));
        let _ = std::fs::remove_dir(&folder);
    }
}
