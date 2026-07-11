//! Tiny persisted launcher settings (currently just the chosen game folder), stored as
//! `launcher_config.json` next to the launcher executable — same portable, next-to-the-exe
//! convention the `mods/` folder uses.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// The game folder the user last used, so we don't have to re-detect it every launch.
    #[serde(default)]
    pub game_dir: Option<String>,
}

/// `launcher_config.json` next to the launcher executable.
fn config_path() -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("launcher_config.json")
}

pub fn load() -> Config {
    std::fs::read(config_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &Config) {
    if let Ok(txt) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(config_path(), txt);
    }
}

/// Remember `dir` as the game folder for next time.
pub fn save_game_dir(dir: &Path) {
    let mut cfg = load();
    cfg.game_dir = Some(dir.display().to_string());
    save(&cfg);
}
