//! Web-updatable Mod Menu (the in-game Options ▸ Mods list).
//!
//! The Mod Menu used to be embedded in the launcher (`vendor/mod-menu/` → `build.rs`). It now
//! lives in its own repo, [`n-popescu/vcb-modmenu`](https://github.com/n-popescu/vcb-modmenu),
//! which auto-publishes a release asset (`npopescu-ModMenu.zip`) on every version bump. The
//! launcher keeps the latest download in a `modmenu/` folder next to the executable (like
//! `modloader/`) and copies that zip into the game's `mods/` folder on enable / Re-apply. So the
//! Mod Menu updates straight from the web — a new upstream release is all it takes, no launcher
//! rebuild.
//!
//! Best-effort throughout: on a fresh, offline install with no cached copy we simply skip the
//! Mod Menu (the in-game list is a convenience, not required for modding to work).

use crate::net;
use crate::update::{self, Asset};
use std::path::{Path, PathBuf};

/// Upstream repo that publishes the Mod Menu releases.
pub const OWNER: &str = "n-popescu";
pub const REPO: &str = "vcb-modmenu";

const GH_ACCEPT: &str = "application/vnd.github+json";

/// The release asset name AND the file name written into the game's `mods/` folder.
pub const MOD_MENU_ZIP: &str = "npopescu-ModMenu.zip";

/// A file that must be inside the zip for it to be a real Mod Menu package.
const MANIFEST_IN_ZIP: &str = "mods-unpacked/npopescu-ModMenu/manifest.json";

// ---- portable cache dir (next to the launcher, like modloader/) -----------------------

/// `<launcher dir>/modmenu/` — the downloaded Mod Menu, kept next to the launcher like `mods/`,
/// `modloader/`, and `launcher_config.json`.
pub fn cache_dir() -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("modmenu")
}

fn cached_zip_path() -> PathBuf {
    cache_dir().join(MOD_MENU_ZIP)
}

fn version_path() -> PathBuf {
    cache_dir().join("version.txt")
}

/// The cached zip, if a non-empty one exists.
pub fn cached_zip() -> Option<PathBuf> {
    let p = cached_zip_path();
    match std::fs::metadata(&p) {
        Ok(m) if m.len() > 0 => Some(p),
        _ => None,
    }
}

/// The version string of the cached copy (from `version.txt`), if any.
pub fn cached_version() -> Option<String> {
    std::fs::read_to_string(version_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---- GitHub: latest release + download ------------------------------------------------

#[derive(Clone, Debug)]
pub struct Latest {
    pub version: String,
    pub zip_url: String,
}

fn latest_release_url() -> String {
    format!("https://api.github.com/repos/{OWNER}/{REPO}/releases/latest")
}

/// Query the Mod Menu's latest GitHub release: its version (tag without a leading `v`) and the
/// download URL of the `npopescu-ModMenu.zip` asset.
pub fn check_latest() -> Result<Latest, String> {
    let json = net::get_text(&latest_release_url(), GH_ACCEPT)?;
    let release = update::parse_release(&json)
        .ok_or_else(|| "couldn't parse the Mod Menu release response".to_string())?;
    let asset = pick_zip_asset(&release.assets)
        .ok_or_else(|| "the Mod Menu release has no downloadable zip".to_string())?;
    Ok(Latest {
        version: release.tag.trim_start_matches(['v', 'V']).to_string(),
        zip_url: asset.url.clone(),
    })
}

/// The Mod Menu zip asset: prefer the exact `npopescu-ModMenu.zip`, else any `.zip`.
fn pick_zip_asset(assets: &[Asset]) -> Option<&Asset> {
    assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(MOD_MENU_ZIP))
        .or_else(|| assets.iter().find(|a| a.name.to_ascii_lowercase().ends_with(".zip")))
}

/// Download the zip, verify it's a real Mod Menu package, and replace the cache with it. Records
/// `version` alongside so a later run can skip a redundant re-download. Returns the version.
pub fn download_and_cache(zip_url: &str, version: &str) -> Result<String, String> {
    let bytes = net::get_bytes(zip_url)?;
    if !zip_has_manifest(&bytes) {
        return Err("the downloaded archive isn't a Mod Menu package".into());
    }
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    // Write to a temp file then swap in, so a partial download can't leave a corrupt cache.
    let tmp = cache_dir().join("npopescu-ModMenu.zip.downloading");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, cached_zip_path())
        .map_err(|e| format!("couldn't install the downloaded Mod Menu: {e}"))?;
    let _ = std::fs::write(version_path(), version);
    Ok(version.to_string())
}

/// True if `zip_bytes` is a zip that contains the Mod Menu manifest at the expected path.
fn zip_has_manifest(zip_bytes: &[u8]) -> bool {
    let reader = std::io::Cursor::new(zip_bytes);
    let Ok(mut archive) = zip::ZipArchive::new(reader) else {
        return false;
    };
    for i in 0..archive.len() {
        let Ok(file) = archive.by_index(i) else { continue };
        if file.name().replace('\\', "/") == MANIFEST_IN_ZIP {
            return true;
        }
    }
    false
}

/// Refresh the cache to the latest release if it differs (or there's no cache). Returns the
/// version now in the cache. Callers treat errors (offline, rate-limit, no release) as non-fatal.
pub fn refresh() -> Result<String, String> {
    let latest = check_latest()?;
    if cached_zip().is_some() && cached_version().as_deref() == Some(latest.version.as_str()) {
        return Ok(latest.version); // cache already current
    }
    download_and_cache(&latest.zip_url, &latest.version)
}

// ---- install into the game's mods/ folder ---------------------------------------------

/// Copy the cached Mod Menu zip into `mods_dir` as `npopescu-ModMenu.zip`. Returns `Ok(false)`
/// if nothing is cached yet (offline first run) — the caller treats that as non-fatal.
pub fn install(mods_dir: &Path) -> std::io::Result<bool> {
    match cached_zip() {
        Some(src) => {
            install_from(&src, mods_dir)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Copy a specific Mod Menu zip into `mods_dir` as `npopescu-ModMenu.zip`.
pub fn install_from(zip: &Path, mods_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(mods_dir)?;
    std::fs::copy(zip, mods_dir.join(MOD_MENU_ZIP))?;
    Ok(())
}

// ---- worker ---------------------------------------------------------------------------

/// Best-effort background refresh at startup: pull the latest Mod Menu into the cache and, if
/// modding is already set up, drop the fresh copy into the game's `mods/` folder so the next
/// launch has it. Fire-and-forget; failures (offline, rate-limit) are ignored.
pub fn spawn_refresh(mods_dir: Option<PathBuf>) {
    std::thread::spawn(move || {
        let _ = refresh();
        if let Some(dir) = mods_dir {
            let _ = install(&dir);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(names: &[&str]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for n in names {
            zip.start_file(*n, opts).unwrap();
            zip.write_all(b"{}").unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn picks_the_named_zip_asset() {
        let assets = vec![
            Asset { name: "notes.txt".into(), url: "u1".into() },
            Asset { name: "npopescu-ModMenu.zip".into(), url: "u2".into() },
        ];
        assert_eq!(pick_zip_asset(&assets).unwrap().url, "u2");
    }

    #[test]
    fn falls_back_to_any_zip_asset() {
        let assets = vec![Asset { name: "ModMenu-1.3.0.zip".into(), url: "z".into() }];
        assert_eq!(pick_zip_asset(&assets).unwrap().url, "z");
    }

    #[test]
    fn no_zip_asset_is_none() {
        let assets = vec![Asset { name: "readme.md".into(), url: "x".into() }];
        assert!(pick_zip_asset(&assets).is_none());
    }

    #[test]
    fn recognises_a_mod_menu_zip() {
        let good = make_zip(&[
            "mods-unpacked/npopescu-ModMenu/manifest.json",
            "mods-unpacked/npopescu-ModMenu/mod_main.gd",
        ]);
        assert!(zip_has_manifest(&good));
        let bad = make_zip(&["mods-unpacked/somethingelse/manifest.json"]);
        assert!(!zip_has_manifest(&bad));
        assert!(!zip_has_manifest(b"not a zip"));
    }

    #[test]
    fn install_from_copies_the_zip_into_mods_dir() {
        let base = std::env::temp_dir().join(format!("vcbl_modmenu_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let src = base.join("src.zip");
        std::fs::write(&src, make_zip(&[MANIFEST_IN_ZIP])).unwrap();
        let mods = base.join("mods");
        assert!(install_from(&src, &mods).is_ok());
        assert!(mods.join(MOD_MENU_ZIP).is_file(), "zip copied into mods dir");
        let _ = std::fs::remove_dir_all(&base);
    }
}
