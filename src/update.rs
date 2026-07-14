//! Self-update: check the launcher's own GitHub Releases for a newer build and (on the
//! single-file targets) download the right per-platform artifact and swap it in.
//!
//! Flow, all off the UI thread:
//! 1. On startup a worker hits `…/releases/latest`, parses `tag_name` + `assets`, and
//!    compares the tag to the compiled-in `CARGO_PKG_VERSION`.
//! 2. If newer, the UI shows a modal ("update available") with *Update now* / *Cancel* and
//!    a "don't show again until the next version" checkbox (persisted in the launcher config).
//! 3. *Update now* downloads the artifact for this OS. Windows/Linux ship a single binary, so
//!    we swap the running executable and relaunch; macOS ships a `.app` zip, which we save
//!    next to the app and reveal (a bundle can't be safely replaced from inside itself).
//!
//! The GitHub API is unauthenticated (public repo) and every failure is non-fatal: a missing
//! release, no network, or a rate-limit simply means "no update offered this run".

use crate::net;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const OWNER: &str = "n-popescu";
pub const LAUNCHER_REPO: &str = "vcb-launcher";

/// The version baked in at build time (from Cargo.toml).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const GH_ACCEPT: &str = "application/vnd.github+json";

// ---- version compare -----------------------------------------------------------------

/// Parse a `vMAJOR.MINOR.PATCH` (or bare `MAJOR.MINOR.PATCH`) tag into comparable numbers.
/// A missing minor/patch is treated as 0; extra dotted parts are ignored. Returns None if
/// the first three components aren't numeric.
pub fn parse_ver(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim();
    let s = s.strip_prefix('v').or_else(|| s.strip_prefix('V')).unwrap_or(s);
    // Drop any pre-release/build suffix (e.g. "1.2.3-rc1").
    let core = s.split(['-', '+']).next().unwrap_or(s);
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// Is `latest` a strictly newer version than `current`? Unparseable versions → false.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_ver(latest), parse_ver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

// ---- release model -------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub html_url: String,
    pub assets: Vec<Asset>,
}

/// Parse the fields we care about out of a GitHub `releases/latest` JSON payload.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let html_url = v.get("html_url").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: a.get("name")?.as_str()?.to_string(),
                        url: a.get("browser_download_url")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release { tag, html_url, assets })
}

/// The substring identifying this OS's release artifact (matches the CI asset names in
/// `.github/workflows/build.yml`).
pub fn platform_tag() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// Pick this platform's *program* asset. Linux ships aux files (`.png`, `.desktop`) beside
/// the binary in the same release, so those are excluded.
pub fn pick_asset<'a>(release: &'a Release, platform: &str) -> Option<&'a Asset> {
    release.assets.iter().find(|a| {
        let n = a.name.to_ascii_lowercase();
        n.contains(platform) && !n.ends_with(".png") && !n.ends_with(".desktop")
    })
}

// ---- check ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct LauncherUpdate {
    pub current: String,
    pub latest: String,
    pub asset: Option<Asset>, // None if this platform has no matching artifact
    pub html_url: String,
}

/// Result of the startup check, shared with the UI thread.
#[derive(Clone, Debug)]
pub enum LauncherCheck {
    Checking,
    UpToDate,
    Available(LauncherUpdate),
    Error(String),
}

fn latest_release_url(owner: &str, repo: &str) -> String {
    format!("https://api.github.com/repos/{owner}/{repo}/releases/latest")
}

/// Query the latest release and decide whether it's newer than what we're running.
pub fn check(owner: &str, repo: &str, current: &str) -> LauncherCheck {
    let json = match net::get_text(&latest_release_url(owner, repo), GH_ACCEPT) {
        Ok(j) => j,
        // A 404 here just means "no published release yet" — not an error worth surfacing.
        Err(e) if e.contains("HTTP 404") => return LauncherCheck::UpToDate,
        Err(e) => return LauncherCheck::Error(e),
    };
    let Some(release) = parse_release(&json) else {
        return LauncherCheck::Error("couldn't parse the GitHub release response".into());
    };
    if !is_newer(&release.tag, current) {
        return LauncherCheck::UpToDate;
    }
    LauncherCheck::Available(LauncherUpdate {
        current: current.to_string(),
        latest: release.tag.clone(),
        asset: pick_asset(&release, platform_tag()).cloned(),
        html_url: release.html_url,
    })
}

// ---- apply (download + swap) ---------------------------------------------------------

/// Where the running executable lives (and, on macOS, used to locate the enclosing `.app`).
fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("can't find our own executable: {e}"))
}

/// Best-effort cleanup of the leftover `<exe>.old` from a previous self-update. Called at
/// startup; a running Windows exe can be renamed but only deleted once it's no longer live.
pub fn cleanup_stale() {
    if let Ok(exe) = current_exe() {
        let old = with_suffix(&exe, ".old");
        let _ = std::fs::remove_file(old);
    }
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Outcome of an apply attempt, consumed by the UI thread.
#[derive(Clone, Debug)]
pub enum ApplyPhase {
    Idle,
    Working,
    /// Windows/Linux: the binary was swapped; the UI thread should launch `PathBuf` and exit.
    Relaunch(PathBuf),
    /// macOS (or a fallback): informational message; the app keeps running.
    Message(String),
    Failed(String),
}

/// Download `asset` and apply it. On Windows/Linux this swaps the running binary and returns
/// `Relaunch(new_exe)`; on macOS it saves the `.app` zip beside the app and reveals it.
pub fn apply(asset: &Asset) -> ApplyPhase {
    let bytes = match net::get_bytes(&asset.url) {
        Ok(b) if !b.is_empty() => b,
        Ok(_) => return ApplyPhase::Failed("downloaded an empty file".into()),
        Err(e) => return ApplyPhase::Failed(format!("download failed: {e}")),
    };

    if cfg!(target_os = "macos") {
        return apply_macos(asset, &bytes);
    }
    apply_binary_swap(&bytes)
}

/// Windows/Linux: write the new binary next to the current one, move the running exe aside,
/// and put the new one in its place so relaunching picks it up.
fn apply_binary_swap(bytes: &[u8]) -> ApplyPhase {
    let exe = match current_exe() {
        Ok(p) => p,
        Err(e) => return ApplyPhase::Failed(e),
    };
    let new = with_suffix(&exe, ".new");
    let old = with_suffix(&exe, ".old");

    if let Err(e) = std::fs::write(&new, bytes) {
        return ApplyPhase::Failed(format!("couldn't write the new binary: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755)) {
            let _ = std::fs::remove_file(&new);
            return ApplyPhase::Failed(format!("couldn't mark the new binary executable: {e}"));
        }
    }
    // Move the live exe aside (allowed while running on both Windows and Linux), then swap
    // the fresh one in. If the final rename fails, restore the original so we never brick it.
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old) {
        let _ = std::fs::remove_file(&new);
        return ApplyPhase::Failed(format!("couldn't move the current binary aside: {e}"));
    }
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe); // put the original back
        let _ = std::fs::remove_file(&new);
        return ApplyPhase::Failed(format!("couldn't install the new binary: {e}"));
    }
    ApplyPhase::Relaunch(exe)
}

/// macOS: the artifact is a zipped `.app`; save it beside the current app and reveal it in
/// Finder. Replacing a bundle from inside itself (possibly in /Applications, quarantined) is
/// unsafe, so we hand off to the user with a clear message.
fn apply_macos(asset: &Asset, bytes: &[u8]) -> ApplyPhase {
    let dest_dir = current_exe()
        .ok()
        .and_then(|p| enclosing_app_dir(&p))
        .and_then(|app| app.parent().map(|d| d.to_path_buf()))
        .or_else(|| dirs::download_dir())
        .unwrap_or_else(|| PathBuf::from("."));
    let dest = dest_dir.join(&asset.name);
    if let Err(e) = std::fs::write(&dest, bytes) {
        return ApplyPhase::Failed(format!("couldn't save the download: {e}"));
    }
    let _ = std::process::Command::new("open").arg("-R").arg(&dest).spawn();
    ApplyPhase::Message(format!(
        "Downloaded {} — unzip it and replace the app to finish updating.",
        dest.display()
    ))
}

/// From `…/VCB Mod Launcher.app/Contents/MacOS/vcb-launcher`, walk up to the `.app` dir.
fn enclosing_app_dir(exe: &Path) -> Option<PathBuf> {
    let mut cur = exe;
    while let Some(parent) = cur.parent() {
        if parent.extension().map(|e| e == "app").unwrap_or(false) {
            return Some(parent.to_path_buf());
        }
        cur = parent;
    }
    None
}

// ---- worker orchestration ------------------------------------------------------------

/// Run the startup launcher-version check on a background thread, publishing the result into
/// `shared` and asking egui to repaint when it lands.
pub fn spawn_launcher_check(shared: Arc<Mutex<LauncherCheck>>, ctx: eframe::egui::Context) {
    *shared.lock().unwrap() = LauncherCheck::Checking;
    std::thread::spawn(move || {
        let result = check(OWNER, LAUNCHER_REPO, CURRENT);
        *shared.lock().unwrap() = result;
        ctx.request_repaint();
    });
}

/// Run a download+apply on a background thread, publishing progress into `phase`.
pub fn spawn_apply(asset: Asset, phase: Arc<Mutex<ApplyPhase>>, ctx: eframe::egui::Context) {
    *phase.lock().unwrap() = ApplyPhase::Working;
    std::thread::spawn(move || {
        let result = apply(&asset);
        *phase.lock().unwrap() = result;
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse_and_compare() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        // pre-release suffix on the tag is ignored for the core compare
        assert!(is_newer("v0.2.0-rc1", "0.1.0"));
        // shorter tags treated as .0
        assert!(is_newer("v2", "1.9.9"));
        // garbage never triggers an update
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("v0.2.0", "garbage"));
    }

    const SAMPLE: &str = r#"{
        "tag_name": "v0.2.0",
        "html_url": "https://github.com/n-popescu/vcb-launcher/releases/tag/v0.2.0",
        "assets": [
            {"name": "vcb-launcher-windows-x86_64.exe", "browser_download_url": "https://x/win.exe"},
            {"name": "vcb-launcher-linux-x86_64", "browser_download_url": "https://x/linux"},
            {"name": "vcb-launcher.png", "browser_download_url": "https://x/icon.png"},
            {"name": "vcb-launcher.desktop", "browser_download_url": "https://x/app.desktop"},
            {"name": "vcb-launcher-macos-universal.zip", "browser_download_url": "https://x/mac.zip"}
        ]
    }"#;

    #[test]
    fn parses_release_and_assets() {
        let r = parse_release(SAMPLE).expect("parse");
        assert_eq!(r.tag, "v0.2.0");
        assert_eq!(r.assets.len(), 5);
        assert!(r.html_url.ends_with("v0.2.0"));
    }

    #[test]
    fn picks_the_program_asset_not_aux_files() {
        let r = parse_release(SAMPLE).unwrap();
        assert_eq!(pick_asset(&r, "windows").unwrap().name, "vcb-launcher-windows-x86_64.exe");
        // linux must resolve to the binary, never the .png/.desktop shipped alongside it
        assert_eq!(pick_asset(&r, "linux").unwrap().name, "vcb-launcher-linux-x86_64");
        assert_eq!(pick_asset(&r, "macos").unwrap().name, "vcb-launcher-macos-universal.zip");
        assert!(pick_asset(&r, "solaris").is_none());
    }

    #[test]
    fn handles_release_with_no_assets() {
        let r = parse_release(r#"{"tag_name":"v9.9.9","html_url":"","assets":[]}"#).unwrap();
        assert!(pick_asset(&r, "linux").is_none());
        assert!(is_newer(&r.tag, CURRENT));
    }
}
