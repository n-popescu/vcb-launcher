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
    /// Set once the user ticks "Don't show again" on the legacy-mode warning, so we stop
    /// popping it every time they open the Legacy tab.
    #[serde(default)]
    pub hide_legacy_warning: bool,
    /// The launcher version the user chose to skip ("Don't show again until the next
    /// version") in the update prompt. The prompt reappears only once a *newer* version than
    /// this shows up; an empty/absent value means "never skipped".
    #[serde(default)]
    pub skip_launcher_version: Option<String>,
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

/// Remember that the user dismissed the legacy-mode warning for good.
pub fn save_hide_legacy_warning(hide: bool) {
    let mut cfg = load();
    cfg.hide_legacy_warning = hide;
    save(&cfg);
}

/// Remember (or clear) the launcher version the user asked not to be reminded about.
pub fn save_skip_launcher_version(version: Option<String>) {
    let mut cfg = load();
    cfg.skip_launcher_version = version;
    save(&cfg);
}
