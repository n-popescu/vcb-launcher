//! Mod metadata (`mod.json`) — see `MOD_METADATA.md` in the vcb-mp repo for the schema.

use crate::pck;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// The embedded metadata path the launcher looks for inside a `.pck`.
pub const EMBED_PATH: &str = "res://mod.json";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModMeta {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub homepage: String,
}

/// Read a mod's metadata: first from `res://mod.json` inside the `.pck`, then from a
/// sidecar `mod.json` / `<stem>.json` next to it. `None` if neither is present/valid.
pub fn read(pck_path: &Path) -> Option<ModMeta> {
    if let Ok(Some(bytes)) = pck::extract_file(pck_path, EMBED_PATH) {
        if let Ok(m) = serde_json::from_slice::<ModMeta>(&bytes) {
            return Some(m);
        }
    }
    for cand in sidecars(pck_path) {
        if let Ok(bytes) = fs::read(&cand) {
            if let Ok(m) = serde_json::from_slice::<ModMeta>(&bytes) {
                return Some(m);
            }
        }
    }
    None
}

fn sidecars(pck_path: &Path) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = pck_path.parent() {
        v.push(dir.join("mod.json"));
        if let Some(stem) = pck_path.file_stem() {
            v.push(dir.join(format!("{}.json", stem.to_string_lossy())));
        }
    }
    v
}
