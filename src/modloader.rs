//! Web-updatable Godot Mod Loader.
//!
//! The launcher still embeds a Mod Loader **seed** (via `build.rs`) so a fresh, offline
//! install can enable modding out of the box — but it no longer has to *stay* on that baked
//! version. This module keeps a **downloaded copy in a `modloader/` folder next to the
//! launcher** and always patches `vcb.pck` from the newest copy available (downloaded > seed).
//! So the Mod Loader can be updated straight from the web, and every enable / Re-apply /
//! update bakes in that version — no launcher rebuild needed.
//!
//! Version discovery is uniform: the Mod Loader records its own version in
//! `addons/mod_loader/mod_loader_store.gd` as `const MODLOADER_VERSION = "x.y.z"`, so we read
//! the applied version straight out of the patched pck, the cached copy, and the seed the
//! same way, and compare against the latest GitHub release.

use crate::net;
use crate::update::{self, Asset, Release};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Upstream repo (public, CC0) that publishes the Mod Loader releases.
pub const OWNER: &str = "GodotModding";
pub const REPO: &str = "godot-mod-loader";

const GH_ACCEPT: &str = "application/vnd.github+json";

/// The store script that carries the version constant (relative to the game's `res://`).
const STORE_REL: &str = "addons/mod_loader/mod_loader_store.gd";
/// The loader autoload — used to sanity-check a downloaded/cached copy is complete.
const LOADER_REL: &str = "addons/mod_loader/mod_loader.gd";

// ---- version parsing -----------------------------------------------------------------

/// Pull the `MODLOADER_VERSION` string out of a `mod_loader_store.gd`.
///
/// Matches `const MODLOADER_VERSION = "6.3.0"` (any whitespace / quote style).
pub fn parse_version(store_gd: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(store_gd);
    for line in text.lines() {
        let l = line.trim();
        if !l.starts_with("const MODLOADER_VERSION") {
            continue;
        }
        let after_eq = l.split_once('=')?.1.trim();
        let inner = after_eq.trim_matches(|c| c == '"' || c == '\'').trim();
        if !inner.is_empty() {
            return Some(inner.to_string());
        }
    }
    None
}

// ---- portable cache dir --------------------------------------------------------------

/// `<launcher dir>/modloader/` — the downloaded Mod Loader, kept next to the launcher like
/// `mods/` and `launcher_config.json`.
pub fn cache_dir() -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("modloader")
}

fn cache_addons_dir() -> PathBuf {
    cache_dir().join("addons")
}

/// Read the cached Mod Loader into the `(res_path, bytes)` shape the patcher wants. Returns
/// `None` if there's no cached copy (or it's missing the two loader scripts, i.e. incomplete).
pub fn cached_addon_owned() -> Option<Vec<(String, Vec<u8>)>> {
    let addons = cache_addons_dir();
    if !addons.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    collect_addon_files(&addons, &cache_dir(), &mut files);
    let has = |rel: &str| files.iter().any(|(p, _)| p == &format!("res://{rel}"));
    if files.is_empty() || !has(STORE_REL) || !has(LOADER_REL) {
        return None;
    }
    Some(files)
}

fn collect_addon_files(dir: &Path, cache_root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_addon_files(&p, cache_root, out);
        } else if let Ok(rel) = p.strip_prefix(cache_root) {
            if let Ok(bytes) = std::fs::read(&p) {
                let res = format!("res://{}", rel.to_string_lossy().replace('\\', "/"));
                out.push((res, bytes));
            }
        }
    }
}

// ---- GitHub: latest release + download -----------------------------------------------

#[derive(Clone, Debug)]
pub struct Latest {
    pub version: String,
    pub zipball_url: String,
}

fn parse_zipball(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("zipball_url")?.as_str().map(|s| s.to_string())
}

/// Query the Mod Loader's latest GitHub release (tag → version, plus the source zipball URL).
pub fn check_latest() -> Result<Latest, String> {
    let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/releases/tag/v6.3.0");
    let json = net::get_text(&url, GH_ACCEPT)?;
    let release: Release = update::parse_release(&json)
        .ok_or_else(|| "couldn't parse the Mod Loader release response".to_string())?;
    // Prefer a packaged asset zip if one is published; else the source zipball (always present).
    let zipball_url = pick_zip_asset(&release)
        .map(|a| a.url.clone())
        .or_else(|| parse_zipball(&json))
        .ok_or_else(|| "release had no downloadable archive".to_string())?;
    Ok(Latest { version: release.tag.trim_start_matches(['v', 'V']).to_string(), zipball_url })
}

/// A published `.zip` asset, if the release attaches one (some Mod Loader releases do).
fn pick_zip_asset(release: &Release) -> Option<&Asset> {
    release.assets.iter().find(|a| a.name.to_ascii_lowercase().ends_with(".zip"))
}

/// Download `zipball_url`, extract its `addons/**`, and replace the cache with it. Returns the
/// version of the installed copy.
pub fn download_and_cache(zipball_url: &str) -> Result<String, String> {
    let bytes = net::get_bytes(zipball_url)?;
    install_from_zip(&bytes)
}

/// Extract every `addons/**` file from a zip (a GitHub source zipball wraps everything in a
/// `owner-repo-sha/` top folder; a packaged asset may not) into the cache dir, then verify the
/// result is a usable Mod Loader and report its version.
pub fn install_from_zip(zip_bytes: &[u8]) -> Result<String, String> {
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("bad zip: {e}"))?;

    // Collect (res_relative_path, bytes) for everything under an `addons/` segment.
    let mut collected: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        let Some(rel) = addons_relative(&name) else { continue };
        let mut buf = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut buf).map_err(|e| format!("read {name}: {e}"))?;
        collected.push((rel, buf));
    }

    let has = |rel: &str| collected.iter().any(|(p, _)| p == rel);
    if !has(STORE_REL) || !has(LOADER_REL) {
        return Err("archive didn't contain a complete addons/mod_loader/".into());
    }
    let version = collected
        .iter()
        .find(|(p, _)| p == STORE_REL)
        .and_then(|(_, b)| parse_version(b))
        .ok_or_else(|| "couldn't read MODLOADER_VERSION from the download".to_string())?;

    // Write atomically-ish: build a fresh dir, then swap it in for the old cache.
    let cache = cache_dir();
    let staging = cache.with_extension("downloading");
    let _ = std::fs::remove_dir_all(&staging);
    for (rel, bytes) in &collected {
        let dest = staging.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        std::fs::write(&dest, bytes).map_err(|e| format!("write {}: {e}", dest.display()))?;
    }
    let _ = std::fs::remove_dir_all(&cache);
    std::fs::rename(&staging, &cache)
        .map_err(|e| format!("couldn't install the downloaded Mod Loader: {e}"))?;
    Ok(version)
}

/// If `zip_name` sits under an `addons/` segment, return its path relative to `addons`'s
/// parent (e.g. `addons/mod_loader/mod_loader.gd`); else None.
fn addons_relative(zip_name: &str) -> Option<String> {
    // Find an `addons/` at a path boundary (start, or preceded by `/`).
    let idx = zip_name.match_indices("addons/").find_map(|(i, _)| {
        if i == 0 || zip_name.as_bytes().get(i - 1) == Some(&b'/') {
            Some(i)
        } else {
            None
        }
    })?;
    let rel = &zip_name[idx..];
    if rel.ends_with('/') {
        None
    } else {
        Some(rel.to_string())
    }
}

// ---- worker orchestration ------------------------------------------------------------

/// Result of the startup Mod Loader latest-version check.
#[derive(Clone, Debug)]
pub enum ModLoaderCheck {
    Checking,
    Known(Latest),
    Error(String),
}

/// Progress of a "update the Mod Loader" download.
#[derive(Clone, Debug)]
pub enum UpdatePhase {
    Idle,
    Working,
    /// The cache now holds this version; the UI thread re-applies the patch.
    Downloaded(String),
    Failed(String),
}

pub fn spawn_check(shared: Arc<Mutex<ModLoaderCheck>>, ctx: eframe::egui::Context) {
    *shared.lock().unwrap() = ModLoaderCheck::Checking;
    std::thread::spawn(move || {
        let result = match check_latest() {
            Ok(l) => ModLoaderCheck::Known(l),
            Err(e) => ModLoaderCheck::Error(e),
        };
        *shared.lock().unwrap() = result;
        ctx.request_repaint();
    });
}

pub fn spawn_download(zipball_url: String, phase: Arc<Mutex<UpdatePhase>>, ctx: eframe::egui::Context) {
    *phase.lock().unwrap() = UpdatePhase::Working;
    std::thread::spawn(move || {
        let result = match download_and_cache(&zipball_url) {
            Ok(v) => UpdatePhase::Downloaded(v),
            Err(e) => UpdatePhase::Failed(e),
        };
        *phase.lock().unwrap() = result;
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_constant() {
        assert_eq!(parse_version(b"extends Node\nconst MODLOADER_VERSION = \"6.3.0\"\n").as_deref(), Some("6.3.0"));
        assert_eq!(parse_version(b"const MODLOADER_VERSION='6.4.1'").as_deref(), Some("6.4.1"));
        assert_eq!(parse_version(b"\tconst MODLOADER_VERSION := \"7.0.0\"").as_deref(), Some("7.0.0"));
        assert_eq!(parse_version(b"var x = 1"), None);
    }

    #[test]
    fn addons_relative_finds_the_segment() {
        assert_eq!(
            addons_relative("GodotModding-godot-mod-loader-abc123/addons/mod_loader/mod_loader.gd").as_deref(),
            Some("addons/mod_loader/mod_loader.gd")
        );
        assert_eq!(addons_relative("addons/JSON_Schema_Validator/x.gd").as_deref(), Some("addons/JSON_Schema_Validator/x.gd"));
        // a directory entry
        assert_eq!(addons_relative("repo/addons/mod_loader/"), None);
        // no addons segment
        assert_eq!(addons_relative("repo/README.md"), None);
        // not at a path boundary (must not match a substring like "myaddons/")
        assert_eq!(addons_relative("repo/myaddons/x.gd"), None);
    }

    #[test]
    fn install_rejects_archive_without_mod_loader() {
        // A zip that has an addons/ path but not the loader scripts must be refused *before*
        // touching the cache (so a bad download can't wipe a good cached copy).
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("repo-abc/addons/something_else/foo.gd", opts).unwrap();
        zip.write_all(b"extends Node").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(install_from_zip(&bytes).is_err());
    }

    #[test]
    fn zipball_url_parsed_from_release_json() {
        let json = r#"{"tag_name":"v6.4.0","zipball_url":"https://api.github.com/repos/GodotModding/godot-mod-loader/zipball/v6.4.0","assets":[]}"#;
        assert_eq!(
            parse_zipball(json).as_deref(),
            Some("https://api.github.com/repos/GodotModding/godot-mod-loader/zipball/v6.4.0")
        );
    }
}
