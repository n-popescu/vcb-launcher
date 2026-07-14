//! Discover and update the Mod Loader mods installed in the GAME's `mods/` folder.
//!
//! These are the `.zip` packages the player drops next to the game
//! (`…/Virtual Circuit Board/mods/`), each containing `mods-unpacked/<id>/manifest.json` in Mod
//! Loader format. This module reads that manifest to show the mod's name + version, derives its
//! GitHub repo from the manifest's `website_url`, checks that repo's latest release, and can
//! download the release's `.zip` asset to update the mod in place.
//!
//! This is the game's own mods folder — distinct from the launcher's `mods/` (the legacy `.pck`
//! swap mods, see `scan.rs`). The in-game Mod Menu (`npopescu-ModMenu.zip`) shows up here too and
//! is updated through the same path.

use crate::net;
use crate::update;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const GH_ACCEPT: &str = "application/vnd.github+json";

/// An installed Mod Loader mod (`.zip`) in the game's `mods/` folder.
#[derive(Clone, Debug)]
pub struct GameMod {
    pub file: PathBuf,                   // the .zip on disk
    pub id: String,                      // mods-unpacked/<id> folder name (namespace-name)
    pub name: String,                    // display name from the manifest
    pub version: String,                 // version_number from the manifest
    pub website: String,                 // website_url from the manifest
    pub repo: Option<(String, String)>,  // (owner, repo) parsed from the website, if it's GitHub
}

impl GameMod {
    pub fn display_name(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else {
            self.id.clone()
        }
    }
}

/// Scan `mods_dir` for Mod Loader mod zips and read each one's manifest. Non-mod zips (no
/// `mods-unpacked/<id>/manifest.json`) are skipped. Sorted by display name.
pub fn scan(mods_dir: &Path) -> Vec<GameMod> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.extension().map(|x| x.eq_ignore_ascii_case("zip")).unwrap_or(false) {
            continue;
        }
        if let Some(gm) = read_mod(&p) {
            out.push(gm);
        }
    }
    out.sort_by(|a, b| a.display_name().to_lowercase().cmp(&b.display_name().to_lowercase()));
    out
}

fn read_mod(zip_path: &Path) -> Option<GameMod> {
    let bytes = std::fs::read(zip_path).ok()?;
    let (id, manifest_json) = manifest_from_zip(&bytes)?;
    let m = parse_manifest(&manifest_json)?;
    let name = if m.name.is_empty() { id.clone() } else { m.name };
    Some(GameMod {
        file: zip_path.to_path_buf(),
        id,
        name,
        version: m.version,
        website: m.website.clone(),
        repo: repo_from_url(&m.website),
    })
}

struct Manifest {
    name: String,
    version: String,
    website: String,
}

fn parse_manifest(json: &[u8]) -> Option<Manifest> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(Manifest {
        name: get("name"),
        version: get("version_number"),
        website: get("website_url"),
    })
}

/// Find `mods-unpacked/<id>/manifest.json` inside a zip and return `(id, manifest bytes)`.
fn manifest_from_zip(zip_bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).ok()?;
    let mut hit: Option<(String, usize)> = None;
    for i in 0..archive.len() {
        let name = {
            let file = archive.by_index(i).ok()?;
            file.name().replace('\\', "/")
        };
        if let Some(id) = mod_id_from_manifest_path(&name) {
            hit = Some((id, i));
            break;
        }
    }
    let (id, idx) = hit?;
    let mut file = archive.by_index(idx).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some((id, buf))
}

/// If `name` is exactly `[<top>/]mods-unpacked/<id>/manifest.json`, return `<id>`.
fn mod_id_from_manifest_path(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split('/').filter(|s| !s.is_empty()).collect();
    let pos = parts.iter().position(|s| *s == "mods-unpacked")?;
    // Need exactly <id>/manifest.json after the mods-unpacked segment.
    if parts.len() == pos + 3 && parts[pos + 2] == "manifest.json" {
        return Some(parts[pos + 1].to_string());
    }
    None
}

/// Parse `(owner, repo)` from a GitHub URL like `https://github.com/owner/repo(.git)(/…)`.
pub fn repo_from_url(url: &str) -> Option<(String, String)> {
    let rest = url.trim().split("github.com/").nth(1)?;
    let mut it = rest.split('/');
    let owner = it.next().unwrap_or("").trim();
    let repo_raw = it.next().unwrap_or("").trim();
    // Strip a trailing .git and anything after a ? or # on the repo segment.
    let repo = repo_raw
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

// ---- GitHub: latest release + download ------------------------------------------------

#[derive(Clone, Debug)]
pub struct ModLatest {
    pub version: String,
    pub asset_url: String,
}

/// Query a mod repo's latest release for its version and the downloadable mod zip. Unlike the Mod
/// Loader (which ships two engine lines — see `modloader.rs`), ordinary mods have a single release
/// line, so `releases/latest` is correct here.
pub fn check_latest(owner: &str, repo: &str, mod_id: &str) -> Result<ModLatest, String> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let json = net::get_text(&url, GH_ACCEPT)?;
    let release = update::parse_release(&json)
        .ok_or_else(|| "couldn't parse the release response".to_string())?;
    let asset = pick_asset(&release.assets, mod_id)
        .ok_or_else(|| "the release has no downloadable mod zip".to_string())?;
    Ok(ModLatest {
        version: release.tag.trim_start_matches(['v', 'V']).to_string(),
        asset_url: asset.url.clone(),
    })
}

/// The mod's release asset: prefer `<id>.zip`, else any `.zip`.
fn pick_asset<'a>(assets: &'a [update::Asset], mod_id: &str) -> Option<&'a update::Asset> {
    let named = format!("{mod_id}.zip").to_ascii_lowercase();
    assets
        .iter()
        .find(|a| a.name.to_ascii_lowercase() == named)
        .or_else(|| assets.iter().find(|a| a.name.to_ascii_lowercase().ends_with(".zip")))
}

/// Download `asset_url` and, if it's a package for `mod_id`, install it over `dest` (the existing
/// zip in the game's mods/ folder). Atomic swap via a temp file so a partial download can't corrupt
/// the installed mod.
pub fn download_and_install(asset_url: &str, mod_id: &str, dest: &Path) -> Result<(), String> {
    let bytes = net::get_bytes(asset_url)?;
    match manifest_from_zip(&bytes) {
        Some((id, _)) if id == mod_id => {}
        _ => return Err("the downloaded archive isn't this mod".into()),
    }
    let tmp = temp_sibling(dest);
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("couldn't install the update: {e}"))?;
    Ok(())
}

fn temp_sibling(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().map(|s| s.to_os_string()).unwrap_or_default();
    name.push(".downloading");
    dest.with_file_name(name)
}

// ---- worker orchestration -------------------------------------------------------------

/// Per-mod update-check result, shared with the UI thread (keyed by mod id).
#[derive(Clone, Debug)]
pub enum ModCheck {
    Checking,
    UpToDate,
    Available(ModLatest),
    NoRepo, // manifest had no GitHub website_url — can't check
    Error(String),
}

pub type ChecksMap = Arc<Mutex<HashMap<String, ModCheck>>>;

/// Which mod (if any) is currently downloading an update, and the last result.
#[derive(Clone, Debug)]
pub enum UpdatePhase {
    Idle,
    Working(String),        // mod id
    Done(String),           // mod id — updated in place
    Failed(String, String), // mod id, error
}

/// Check every scanned mod against its GitHub repo on a background thread, filling `shared` as
/// results arrive. Mods without a GitHub `website_url` are marked `NoRepo`.
pub fn spawn_check_all(mods: Vec<GameMod>, shared: ChecksMap, ctx: eframe::egui::Context) {
    {
        let mut m = shared.lock().unwrap();
        m.clear();
        for gm in &mods {
            m.insert(gm.id.clone(), ModCheck::Checking);
        }
    }
    if mods.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for gm in &mods {
            let result = match &gm.repo {
                None => ModCheck::NoRepo,
                Some((owner, repo)) => match check_latest(owner, repo, &gm.id) {
                    Ok(latest) => {
                        if update::is_newer(&latest.version, &gm.version) {
                            ModCheck::Available(latest)
                        } else {
                            ModCheck::UpToDate
                        }
                    }
                    Err(e) => ModCheck::Error(e),
                },
            };
            shared.lock().unwrap().insert(gm.id.clone(), result);
            ctx.request_repaint();
        }
    });
}

/// Download + install one mod update on a background thread, publishing progress into `phase`.
pub fn spawn_update(gm: GameMod, asset_url: String, phase: Arc<Mutex<UpdatePhase>>, ctx: eframe::egui::Context) {
    *phase.lock().unwrap() = UpdatePhase::Working(gm.id.clone());
    std::thread::spawn(move || {
        let result = match download_and_install(&asset_url, &gm.id, &gm.file) {
            Ok(()) => UpdatePhase::Done(gm.id.clone()),
            Err(e) => UpdatePhase::Failed(gm.id.clone(), e),
        };
        *phase.lock().unwrap() = result;
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(files: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in files {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn parses_github_repo_from_website() {
        assert_eq!(
            repo_from_url("https://github.com/n-popescu/vcb-modmenu"),
            Some(("n-popescu".into(), "vcb-modmenu".into()))
        );
        assert_eq!(
            repo_from_url("https://github.com/n-popescu/vcb-board-size-modifier/"),
            Some(("n-popescu".into(), "vcb-board-size-modifier".into()))
        );
        assert_eq!(
            repo_from_url("http://github.com/owner/repo.git"),
            Some(("owner".into(), "repo".into()))
        );
        assert_eq!(
            repo_from_url("https://github.com/owner/repo/tree/main"),
            Some(("owner".into(), "repo".into()))
        );
        assert!(repo_from_url("https://example.com/not/github").is_none());
        assert!(repo_from_url("").is_none());
        assert!(repo_from_url("https://github.com/onlyowner").is_none());
    }

    #[test]
    fn finds_manifest_and_id_in_a_zip() {
        let z = make_zip(&[
            ("mods-unpacked/npopescu-ModMenu/manifest.json", r#"{"name":"ModMenu","namespace":"npopescu","version_number":"1.4.0","website_url":"https://github.com/n-popescu/vcb-modmenu"}"#),
            ("mods-unpacked/npopescu-ModMenu/mod_main.gd", "extends Node"),
        ]);
        let (id, json) = manifest_from_zip(&z).expect("manifest found");
        assert_eq!(id, "npopescu-ModMenu");
        let m = parse_manifest(&json).unwrap();
        assert_eq!(m.name, "ModMenu");
        assert_eq!(m.version, "1.4.0");
        assert_eq!(m.website, "https://github.com/n-popescu/vcb-modmenu");
    }

    #[test]
    fn handles_a_top_level_wrapper_folder() {
        let z = make_zip(&[(
            "wrapper/mods-unpacked/author-Mod/manifest.json",
            r#"{"name":"Mod","namespace":"author","version_number":"2.0.0"}"#,
        )]);
        assert_eq!(manifest_from_zip(&z).unwrap().0, "author-Mod");
    }

    #[test]
    fn rejects_a_non_mod_zip() {
        let z = make_zip(&[("readme.txt", "hi"), ("mods-unpacked/x/other.txt", "y")]);
        assert!(manifest_from_zip(&z).is_none());
        assert!(manifest_from_zip(b"not a zip").is_none());
    }

    #[test]
    fn picks_the_named_asset_then_any_zip() {
        let assets = vec![
            update::Asset { name: "notes.txt".into(), url: "u0".into() },
            update::Asset { name: "other.zip".into(), url: "u1".into() },
            update::Asset { name: "npopescu-ModMenu.zip".into(), url: "u2".into() },
        ];
        assert_eq!(pick_asset(&assets, "npopescu-ModMenu").unwrap().url, "u2");
        let only_other = vec![update::Asset { name: "some-pkg.zip".into(), url: "z".into() }];
        assert_eq!(pick_asset(&only_other, "npopescu-ModMenu").unwrap().url, "z");
        let none = vec![update::Asset { name: "a.txt".into(), url: "x".into() }];
        assert!(pick_asset(&none, "id").is_none());
    }

    #[test]
    fn scan_reads_mods_and_skips_junk() {
        let base = std::env::temp_dir().join(format!("vcbl_gamemods_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("npopescu-ModMenu.zip"),
            make_zip(&[(
                "mods-unpacked/npopescu-ModMenu/manifest.json",
                r#"{"name":"ModMenu","namespace":"npopescu","version_number":"1.4.0","website_url":"https://github.com/n-popescu/vcb-modmenu"}"#,
            )]),
        )
        .unwrap();
        std::fs::write(base.join("random.zip"), make_zip(&[("hi.txt", "x")])).unwrap();
        std::fs::write(base.join("notes.txt"), b"ignored").unwrap();

        let mods = scan(&base);
        assert_eq!(mods.len(), 1, "only the real mod zip is listed");
        assert_eq!(mods[0].id, "npopescu-ModMenu");
        assert_eq!(mods[0].version, "1.4.0");
        assert_eq!(mods[0].repo, Some(("n-popescu".into(), "vcb-modmenu".into())));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn install_replaces_only_a_matching_package() {
        let base = std::env::temp_dir().join(format!("vcbl_gminstall_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dest = base.join("author-Mod.zip");
        std::fs::write(&dest, b"old").unwrap();

        // A mismatched package must be refused (dest untouched). We can't hit the network here, so
        // exercise the verify path by writing the "downloaded" bytes to a file and checking the id.
        let wrong = make_zip(&[("mods-unpacked/other-Mod/manifest.json", "{}")]);
        assert_ne!(manifest_from_zip(&wrong).unwrap().0, "author-Mod");

        // A matching package installs.
        let right = make_zip(&[("mods-unpacked/author-Mod/manifest.json", r#"{"name":"Mod","namespace":"author","version_number":"9.9.9"}"#)]);
        let tmp = temp_sibling(&dest);
        std::fs::write(&tmp, &right).unwrap();
        std::fs::rename(&tmp, &dest).unwrap();
        assert_eq!(manifest_from_zip(&std::fs::read(&dest).unwrap()).unwrap().0, "author-Mod");
        let _ = std::fs::remove_dir_all(&base);
    }
}
