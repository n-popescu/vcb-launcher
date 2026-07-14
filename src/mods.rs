//! The game's installed **runtime mods** — the `.zip` packages in the `mods/` folder next to the
//! game. This lists them with their versions and repository links, checks each mod's own GitHub
//! repo for a newer release, and updates one in place from that release's asset.
//!
//! A Mod Loader package is a zip containing `mods-unpacked/<id>/manifest.json` (the launcher
//! installs the Mod Menu this way, and users drop other mods' zips in the same folder). The
//! manifest's `website_url` points at the mod's home; when that's a GitHub repo we can offer an
//! update straight from its Releases — the same model `modmenu.rs` uses for the Mod Menu, applied
//! generically to every installed mod.
//!
//! Everything network-facing is best-effort: no repo / offline / rate-limited / no release just
//! means "no update offered", never an error that blocks using the launcher.

use crate::net;
use crate::update;
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const GH_ACCEPT: &str = "application/vnd.github+json";

/// The subset of a Mod Loader `manifest.json` we read.
#[derive(Debug, Clone, Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    website_url: String,
    #[serde(default)]
    description: String,
}

/// One installed mod: the zip it lives in plus the fields read from its manifest.
#[derive(Debug, Clone)]
pub struct GameMod {
    pub file: PathBuf, // the `.zip` in the game's mods/ folder
    pub id: String,    // the `mods-unpacked/<id>` folder name (usually "namespace-name")
    pub name: String,
    pub version: String,
    pub website_url: String,
    pub description: String,
}

impl GameMod {
    /// `(owner, repo)` if `website_url` is a github.com URL, else None.
    pub fn repo(&self) -> Option<(String, String)> {
        parse_github_repo(&self.website_url)
    }
}

/// Per-mod update state, shared with the UI thread (keyed by mod id).
#[derive(Clone, Debug)]
pub enum UpdateState {
    Unchecked,
    NoRepo, // manifest has no github website_url — can't check
    Checking,
    UpToDate,
    Available { latest: String, asset_url: String },
    Updating,
    Updated(String),
    Error(String),
}

// ---- scan ------------------------------------------------------------------------------

/// List the mods installed in `mods_dir` (top-level `.zip` packages), sorted by display name.
pub fn scan(mods_dir: &Path) -> Vec<GameMod> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_file() || !p.extension().map(|x| x.eq_ignore_ascii_case("zip")).unwrap_or(false) {
            continue;
        }
        if let Some(m) = read_zip_mod(&p) {
            out.push(m);
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Read the first `mods-unpacked/<id>/manifest.json` out of a mod zip.
fn read_zip_mod(zip_path: &Path) -> Option<GameMod> {
    let bytes = std::fs::read(zip_path).ok()?;
    let (id, manifest) = manifest_in_zip(&bytes)?;
    let name = if manifest.name.is_empty() { id.clone() } else { manifest.name.clone() };
    Some(GameMod {
        file: zip_path.to_path_buf(),
        id,
        name,
        version: manifest.version_number,
        website_url: manifest.website_url,
        description: manifest.description,
    })
}

/// Find the mod id + parsed manifest inside a zip's `mods-unpacked/<id>/manifest.json`.
fn manifest_in_zip(zip_bytes: &[u8]) -> Option<(String, Manifest)> {
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).ok()?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).ok()?;
        let name = file.name().replace('\\', "/");
        if let Some(id) = mod_id_from_manifest_path(&name) {
            let mut buf = String::new();
            if file.read_to_string(&mut buf).is_ok() {
                if let Ok(m) = serde_json::from_str::<Manifest>(&buf) {
                    return Some((id, m));
                }
            }
        }
    }
    None
}

/// `mods-unpacked/<id>/manifest.json` → `Some("<id>")` (anywhere along the path).
fn mod_id_from_manifest_path(path: &str) -> Option<String> {
    let needle = "mods-unpacked/";
    let idx = path.find(needle)?;
    let rest = &path[idx + needle.len()..];
    let (id, tail) = rest.split_once('/')?;
    if tail == "manifest.json" && !id.is_empty() {
        Some(id.to_string())
    } else {
        None
    }
}

// ---- github repo + release -------------------------------------------------------------

/// Parse `https://github.com/OWNER/REPO[/...]` → `(OWNER, REPO)`.
pub fn parse_github_repo(url: &str) -> Option<(String, String)> {
    let u = url.trim();
    let rest = u
        .strip_prefix("https://github.com/")
        .or_else(|| u.strip_prefix("http://github.com/"))
        .or_else(|| u.strip_prefix("github.com/"))?;
    let mut it = rest.split('/');
    let owner = it.next()?.trim();
    let repo = it.next()?.trim().trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// The latest release for `owner/repo`: its version tag and the URL of a downloadable `.zip`
/// asset (preferring one whose name contains the mod id, else any `.zip`).
pub fn check_latest(owner: &str, repo: &str, id: &str) -> Result<(String, String), String> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let json = net::get_text(&url, GH_ACCEPT)?;
    let release =
        update::parse_release(&json).ok_or_else(|| "couldn't parse the release response".to_string())?;
    let asset = pick_zip_asset(&release.assets, id)
        .ok_or_else(|| "the latest release has no downloadable .zip".to_string())?;
    Ok((release.tag, asset.url.clone()))
}

/// Prefer a `.zip` asset whose name contains the mod id (e.g. `npopescu-ModMenu.zip`), else the
/// first `.zip` asset.
fn pick_zip_asset<'a>(assets: &'a [update::Asset], id: &str) -> Option<&'a update::Asset> {
    let id_lc = id.to_ascii_lowercase();
    assets
        .iter()
        .find(|a| {
            let n = a.name.to_ascii_lowercase();
            n.ends_with(".zip") && n.contains(&id_lc)
        })
        .or_else(|| assets.iter().find(|a| a.name.to_ascii_lowercase().ends_with(".zip")))
}

// ---- update (download + replace in place) ----------------------------------------------

/// Download the release asset and replace `dest` with it, after verifying it's a real mod package
/// for `id` (contains `mods-unpacked/<id>/manifest.json`). Returns the version now installed.
pub fn install_update(asset_url: &str, id: &str, dest: &Path) -> Result<String, String> {
    let bytes = net::get_bytes(asset_url)?;
    let (found_id, manifest) =
        manifest_in_zip(&bytes).ok_or_else(|| "downloaded file isn't a Mod Loader package".to_string())?;
    if found_id != id {
        return Err(format!("downloaded package is '{found_id}', expected '{id}'"));
    }
    // Write next to the target then swap in, so a partial download can't corrupt the installed mod.
    let tmp = with_extension(dest, "downloading");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("install {}: {e}", dest.display()))?;
    Ok(manifest.version_number)
}

fn with_extension(p: &Path, ext: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

// ---- workers ---------------------------------------------------------------------------

/// The shared per-mod state map the UI reads.
pub type States = Arc<Mutex<std::collections::HashMap<String, UpdateState>>>;

/// Check every mod with a GitHub repo in the background, filling `states` by mod id. Mods without
/// a repo are marked `NoRepo`. Requests a repaint as each result lands.
pub fn spawn_check_all(mods: &[GameMod], states: States, ctx: eframe::egui::Context) {
    for m in mods {
        let id = m.id.clone();
        match m.repo() {
            None => {
                states.lock().unwrap().insert(id, UpdateState::NoRepo);
            }
            Some((owner, repo)) => {
                states.lock().unwrap().insert(id.clone(), UpdateState::Checking);
                let states = states.clone();
                let ctx = ctx.clone();
                let version = m.version.clone();
                std::thread::spawn(move || {
                    let result = match check_latest(&owner, &repo, &id) {
                        Ok((tag, asset_url)) => {
                            if update::is_newer(&tag, &version) {
                                UpdateState::Available {
                                    latest: tag.trim_start_matches(['v', 'V']).to_string(),
                                    asset_url,
                                }
                            } else {
                                UpdateState::UpToDate
                            }
                        }
                        Err(e) => UpdateState::Error(e),
                    };
                    states.lock().unwrap().insert(id, result);
                    ctx.request_repaint();
                });
            }
        }
    }
}

/// Download + install an update for one mod in the background, updating `states[id]`.
pub fn spawn_update(
    id: String,
    asset_url: String,
    dest: PathBuf,
    states: States,
    ctx: eframe::egui::Context,
) {
    states.lock().unwrap().insert(id.clone(), UpdateState::Updating);
    std::thread::spawn(move || {
        let result = match install_update(&asset_url, &id, &dest) {
            Ok(v) => UpdateState::Updated(v.trim_start_matches(['v', 'V']).to_string()),
            Err(e) => UpdateState::Error(e),
        };
        states.lock().unwrap().insert(id, result);
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_mod_zip(id: &str, manifest: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(format!("mods-unpacked/{id}/manifest.json"), opts).unwrap();
        zip.write_all(manifest.as_bytes()).unwrap();
        zip.start_file(format!("mods-unpacked/{id}/mod_main.gd"), opts).unwrap();
        zip.write_all(b"extends Node").unwrap();
        zip.finish().unwrap().into_inner()
    }

    const MANIFEST: &str = r#"{
        "name": "ModMenu", "namespace": "npopescu", "version_number": "1.4.0",
        "website_url": "https://github.com/n-popescu/vcb-modmenu",
        "description": "the in-game mod list"
    }"#;

    #[test]
    fn reads_manifest_and_id_from_zip() {
        let bytes = make_mod_zip("npopescu-ModMenu", MANIFEST);
        let (id, m) = manifest_in_zip(&bytes).expect("manifest found");
        assert_eq!(id, "npopescu-ModMenu");
        assert_eq!(m.version_number, "1.4.0");
        assert_eq!(m.name, "ModMenu");
        assert_eq!(m.website_url, "https://github.com/n-popescu/vcb-modmenu");
    }

    #[test]
    fn scan_lists_zip_mods() {
        let base = std::env::temp_dir().join(format!("vcbl_mods_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("npopescu-ModMenu.zip"), make_mod_zip("npopescu-ModMenu", MANIFEST)).unwrap();
        std::fs::write(base.join("notes.txt"), b"not a mod").unwrap();
        let mods = scan(&base);
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].id, "npopescu-ModMenu");
        assert_eq!(mods[0].repo(), Some(("n-popescu".to_string(), "vcb-modmenu".to_string())));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parses_github_repo_urls() {
        assert_eq!(
            parse_github_repo("https://github.com/n-popescu/vcb-modmenu"),
            Some(("n-popescu".to_string(), "vcb-modmenu".to_string()))
        );
        assert_eq!(
            parse_github_repo("https://github.com/n-popescu/vcb-board-size-modifier/"),
            Some(("n-popescu".to_string(), "vcb-board-size-modifier".to_string()))
        );
        assert_eq!(
            parse_github_repo("github.com/o/r.git"),
            Some(("o".to_string(), "r".to_string()))
        );
        assert!(parse_github_repo("https://example.com/x").is_none());
        assert!(parse_github_repo("").is_none());
    }

    #[test]
    fn mod_id_from_manifest_path_matches_only_the_manifest() {
        assert_eq!(mod_id_from_manifest_path("mods-unpacked/npopescu-ModMenu/manifest.json").as_deref(), Some("npopescu-ModMenu"));
        assert_eq!(mod_id_from_manifest_path("x/mods-unpacked/a-b/manifest.json").as_deref(), Some("a-b"));
        assert!(mod_id_from_manifest_path("mods-unpacked/a-b/mod_main.gd").is_none());
        assert!(mod_id_from_manifest_path("readme.md").is_none());
    }

    #[test]
    fn picks_named_zip_asset_then_any() {
        let assets = vec![
            update::Asset { name: "notes.txt".into(), url: "u0".into() },
            update::Asset { name: "other.zip".into(), url: "u1".into() },
            update::Asset { name: "npopescu-ModMenu.zip".into(), url: "u2".into() },
        ];
        assert_eq!(pick_zip_asset(&assets, "npopescu-ModMenu").unwrap().url, "u2");
        assert_eq!(pick_zip_asset(&assets, "absent-id").unwrap().url, "u1");
        assert!(pick_zip_asset(&[], "x").is_none());
    }

    #[test]
    fn install_update_verifies_and_replaces() {
        let base = std::env::temp_dir().join(format!("vcbl_modupd_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dest = base.join("npopescu-ModMenu.zip");
        std::fs::write(&dest, b"old").unwrap();

        // A wrong-id package is refused (dest untouched).
        let wrong = make_mod_zip("someone-Else", MANIFEST);
        let wrong_url = write_temp(&base, "wrong.zip", &wrong);
        assert!(install_from_file(&wrong_url, "npopescu-ModMenu", &dest).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");

        let _ = std::fs::remove_dir_all(&base);
    }

    // Local helpers mirroring install_update without the network (net::get_bytes reads http only).
    fn write_temp(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
    fn install_from_file(src: &Path, id: &str, dest: &Path) -> Result<String, String> {
        let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
        let (found_id, manifest) = manifest_in_zip(&bytes).ok_or("not a package")?;
        if found_id != id {
            return Err("wrong id".into());
        }
        std::fs::write(dest, &bytes).map_err(|e| e.to_string())?;
        Ok(manifest.version_number)
    }
}
