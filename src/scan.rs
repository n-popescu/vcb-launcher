//! Discover mod `.pck`s in the launcher's `mods/` folder and read their metadata.

use crate::install;
use crate::meta::{self, ModMeta};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ModEntry {
    pub path: PathBuf,
    pub meta: ModMeta,
    pub has_meta: bool,
    pub fingerprint: Option<u64>,
}

impl ModEntry {
    /// A human label even when the `.pck` has no metadata.
    pub fn display_name(&self) -> String {
        if self.has_meta && !self.meta.name.is_empty() {
            self.meta.name.clone()
        } else {
            // Fall back to the parent folder name (mods are often <name>/vcb.pck) or the
            // file stem.
            let parent = self
                .path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if !parent.is_empty() && parent.to_lowercase() != "mods" {
                parent
            } else {
                self.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Unknown mod".to_string())
            }
        }
    }
}

/// The `mods/` directory that lives next to the launcher executable, created if missing.
pub fn mods_dir() -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("mods");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Recursively collect every `*.pck` under `dir` and read its metadata.
pub fn scan(dir: &Path) -> Vec<ModEntry> {
    let mut out = Vec::new();
    collect(dir, 0, &mut out);
    out.sort_by(|a, b| a.display_name().to_lowercase().cmp(&b.display_name().to_lowercase()));
    out
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<ModEntry>) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, depth + 1, out);
        } else if p.extension().map(|x| x.eq_ignore_ascii_case("pck")).unwrap_or(false) {
            let meta = meta::read(&p);
            out.push(ModEntry {
                fingerprint: install::fingerprint(&p),
                has_meta: meta.is_some(),
                meta: meta.unwrap_or_default(),
                path: p,
            });
        }
    }
}
