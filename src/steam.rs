//! Locate a Steam install of Virtual Circuit Board.
//!
//! We enumerate Steam library folders and look under each `steamapps/common/*` for the
//! game directory (the one holding `vcb.pck` and/or the `vcb` executable). The exact
//! folder name isn't assumed.

use std::fs;
use std::path::{Path, PathBuf};

/// Candidate Steam root install folders for this OS (only those that exist).
pub fn steam_roots() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();

    #[cfg(windows)]
    {
        if let Some(p) = registry_steam_path() {
            v.push(p);
        }
        v.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
        v.push(PathBuf::from(r"C:\Program Files\Steam"));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            v.push(home.join(".steam/steam"));
            v.push(home.join(".steam/root"));
            v.push(home.join(".local/share/Steam"));
            // Flatpak Steam
            v.push(home.join(".var/app/com.valvesoftware.Steam/data/Steam"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            v.push(home.join("Library/Application Support/Steam"));
        }
    }

    v.retain(|p| p.exists());
    dedup(&mut v);
    v
}

/// All Steam library folders reachable from a root (the root itself plus every `path`
/// listed in `steamapps/libraryfolders.vdf`).
pub fn library_dirs(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_path_buf()];
    let vdf = root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(txt) = fs::read_to_string(&vdf) {
        for line in txt.lines() {
            let l = line.trim();
            if l.starts_with("\"path\"") {
                if let Some(p) = last_quoted(l) {
                    libs.push(PathBuf::from(p));
                }
            }
        }
    }
    libs.retain(|p| p.exists());
    dedup(&mut libs);
    libs
}

/// Best-effort auto-detection of the game directory across all Steam libraries.
pub fn find_game_dir() -> Option<PathBuf> {
    for root in steam_roots() {
        for lib in library_dirs(&root) {
            let common = lib.join("steamapps").join("common");
            if let Ok(entries) = fs::read_dir(&common) {
                for e in entries.flatten() {
                    let dir = e.path();
                    if dir.is_dir() && is_game_dir(&dir) {
                        return Some(dir);
                    }
                }
            }
        }
    }
    None
}

/// A directory is the game if it holds `vcb.pck` (active or a mod) or a `vcb` executable.
pub fn is_game_dir(dir: &Path) -> bool {
    if dir.join("vcb.pck").is_file() {
        return true;
    }
    for exe in ["vcb.exe", "vcb.x86_64", "vcb"] {
        if dir.join(exe).is_file() {
            return true;
        }
    }
    false
}

fn dedup(v: &mut Vec<PathBuf>) {
    let mut seen: Vec<PathBuf> = Vec::new();
    v.retain(|p| {
        let c = p.canonicalize().unwrap_or_else(|_| p.clone());
        if seen.contains(&c) {
            false
        } else {
            seen.push(c);
            true
        }
    });
}

/// Pull the last `"..."` token off a VDF line and unescape `\\` and `\"`.
fn last_quoted(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let end = line.rfind('"')?;
    // find the matching opening quote before `end`, honouring escapes
    let mut i = end;
    while i > 0 {
        i -= 1;
        if bytes[i] == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            let raw = &line[i + 1..end];
            return Some(raw.replace("\\\\", "\\").replace("\\\"", "\""));
        }
    }
    None
}

#[cfg(windows)]
fn registry_steam_path() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey(r"Software\Valve\Steam").ok()?;
    let path: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(path))
}
