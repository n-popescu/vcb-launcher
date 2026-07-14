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
//! same way, and compare against the latest **Godot 3.x** GitHub release (see `check_latest`:
//! the game is Godot 3.5.1, so the Mod Loader's Godot 4.x line — 7.x+ — is deliberately ignored).

use crate::net;
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

// ---- GitHub: latest Godot-3.x release + download -------------------------------------

/// The Godot Mod Loader publishes two lines: **6.x for Godot 3.x** and **7.x+ for Godot 4.x**
/// (its 7.x release notes literally say "This is the Godot 4.x Version!"). VCB runs on Godot
/// 3.5.1, so a 7.x build is the wrong engine — yet GitHub's `releases/latest` points at it. So
/// we never take the bare "latest"; we list the releases and pick the newest one whose major
/// version is still in the Godot 3.x line.
const GODOT3_MAX_MAJOR: u64 = 6;

#[derive(Clone, Debug)]
pub struct Latest {
    pub version: String,
    pub zipball_url: String,
}

/// Query the Mod Loader's releases and return the newest **Godot 3.x** one (tag → version, plus
/// a download URL: a packaged `.zip` asset if the release attaches one, else the source zipball).
pub fn check_latest() -> Result<Latest, String> {
    let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/releases?per_page=100");
    let json = net::get_text(&url, GH_ACCEPT)?;
    pick_latest_godot3(&json)
        .ok_or_else(|| "couldn't find a Godot 3.x Mod Loader release".to_string())
}

/// Choose the newest non-draft, non-prerelease release with a Godot-3.x-line version (major
/// `<= GODOT3_MAX_MAJOR`) out of a GitHub `/releases` JSON array.
fn pick_latest_godot3(list_json: &str) -> Option<Latest> {
    let arr = serde_json::from_str::<serde_json::Value>(list_json).ok()?;
    let mut best: Option<((u64, u64, u64), Latest)> = None;
    for rel in arr.as_array()? {
        if rel.get("draft").and_then(|v| v.as_bool()).unwrap_or(false)
            || rel.get("prerelease").and_then(|v| v.as_bool()).unwrap_or(false)
        {
            continue;
        }
        let Some(tag) = rel.get("tag_name").and_then(|v| v.as_str()) else { continue };
        let Some(ver) = parse_semver(tag) else { continue };
        if ver.0 > GODOT3_MAX_MAJOR {
            continue; // 7.x+ is the Godot 4.x line
        }
        if let Some((best_ver, _)) = best.as_ref() {
            if ver <= *best_ver {
                continue;
            }
        }
        // Prefer a packaged `.zip` asset if published; else the source zipball (always present).
        let Some(zipball_url) = pick_zip_asset_url(rel)
            .or_else(|| rel.get("zipball_url").and_then(|v| v.as_str()).map(String::from))
        else {
            continue;
        };
        let version = tag.trim_start_matches(['v', 'V']).to_string();
        best = Some((ver, Latest { version, zipball_url }));
    }
    best.map(|(_, latest)| latest)
}

/// The `browser_download_url` of the first `.zip` asset attached to a release JSON object.
fn pick_zip_asset_url(rel: &serde_json::Value) -> Option<String> {
    for a in rel.get("assets")?.as_array()? {
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.to_ascii_lowercase().ends_with(".zip") {
            if let Some(url) = a.get("browser_download_url").and_then(|v| v.as_str()) {
                return Some(url.to_string());
            }
        }
    }
    None
}

/// Parse a `vMAJOR.MINOR.PATCH` (or bare) tag into comparable numbers; a missing minor/patch is
/// treated as 0 and a pre-release/build suffix is dropped. None if the major isn't numeric.
fn parse_semver(tag: &str) -> Option<(u64, u64, u64)> {
    let s = tag.trim();
    let s = s.strip_prefix('v').or_else(|| s.strip_prefix('V')).unwrap_or(s);
    let core = s.split(['-', '+']).next().unwrap_or(s);
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
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
    fn parses_semver_tags() {
        assert_eq!(parse_semver("v6.3.0"), Some((6, 3, 0)));
        assert_eq!(parse_semver("7.0.1"), Some((7, 0, 1)));
        assert_eq!(parse_semver("v6"), Some((6, 0, 0)));
        assert_eq!(parse_semver("v6.4.0-rc1"), Some((6, 4, 0)));
        assert_eq!(parse_semver("nightly"), None);
    }

    #[test]
    fn picks_latest_godot3_release_ignoring_godot4_and_drafts() {
        // Newest-first, like the GitHub API: the "latest" release is a Godot 4.x (7.x) build and
        // must be skipped, a draft 6.x is skipped, and the newest real 6.x wins.
        let json = r#"[
            {"tag_name":"v7.0.1","draft":false,"prerelease":false,"zipball_url":"https://z/7.0.1","assets":[]},
            {"tag_name":"v6.4.0","draft":true,"prerelease":false,"zipball_url":"https://z/6.4.0d","assets":[]},
            {"tag_name":"v6.3.0","draft":false,"prerelease":false,"zipball_url":"https://z/6.3.0","assets":[]},
            {"tag_name":"v6.2.0","draft":false,"prerelease":false,"zipball_url":"https://z/6.2.0","assets":[]}
        ]"#;
        let latest = pick_latest_godot3(json).expect("a Godot 3.x release");
        assert_eq!(latest.version, "6.3.0");
        assert_eq!(latest.zipball_url, "https://z/6.3.0");
    }

    #[test]
    fn newer_godot3_version_is_picked_regardless_of_list_order() {
        let json = r#"[
            {"tag_name":"v6.3.0","draft":false,"prerelease":false,"zipball_url":"https://z/6.3.0","assets":[]},
            {"tag_name":"v6.10.0","draft":false,"prerelease":false,"zipball_url":"https://z/6.10.0","assets":[]}
        ]"#;
        assert_eq!(pick_latest_godot3(json).unwrap().version, "6.10.0");
    }

    #[test]
    fn prefers_a_packaged_zip_asset_over_the_source_zipball() {
        let json = r#"[
            {"tag_name":"v6.3.0","draft":false,"prerelease":false,"zipball_url":"https://z/src",
             "assets":[{"name":"notes.txt","browser_download_url":"https://a/notes"},
                       {"name":"ModLoader.zip","browser_download_url":"https://a/ml.zip"}]}
        ]"#;
        assert_eq!(pick_latest_godot3(json).unwrap().zipball_url, "https://a/ml.zip");
    }

    #[test]
    fn a_list_of_only_godot4_releases_yields_none() {
        let json = r#"[{"tag_name":"v7.0.1","draft":false,"prerelease":false,"zipball_url":"https://z/7","assets":[]}]"#;
        assert!(pick_latest_godot3(json).is_none());
    }
}
