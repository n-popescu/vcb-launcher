//! Tiny persisted launcher settings (the chosen game folder, plus the "skip this launcher
//! version" choice).
//!
//! The file lives in the OS's **per-user config directory** — `%APPDATA%\vcb-launcher\` on
//! Windows, `~/Library/Application Support/vcb-launcher/` on macOS, `~/.config/vcb-launcher/`
//! on Linux (XDG). That location **survives an app update**: on macOS especially, replacing
//! the `.app` bundle wipes anything stored next to the binary, which is where older builds kept
//! this file — so a settings-next-to-the-exe file would be lost on every update. To keep the
//! upgrade seamless, [`load`] transparently **migrates** a legacy `launcher_config.json` found
//! next to the executable into the new location the first time it runs.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// The game folder the user last used, so we don't have to re-detect it every launch.
    #[serde(default)]
    pub game_dir: Option<String>,
    /// The launcher version the user chose to skip ("Don't show again until the next
    /// version") in the update prompt. The prompt reappears only once a *newer* version than
    /// this shows up; an empty/absent value means "never skipped".
    #[serde(default)]
    pub skip_launcher_version: Option<String>,
    /// The launcher version that last ran. Used to detect the first boot of a freshly-updated
    /// launcher, so we can auto re-apply the Mod Loader patch (the new build may carry a newer
    /// seed / patch logic). Absent on a first-ever run.
    #[serde(default)]
    pub last_launcher_version: Option<String>,
    /// The UI theme the user chose: "classic" or "glass" (Liquid Glass). Absent means the default
    /// (classic).
    #[serde(default)]
    pub ui_theme: Option<String>,
}

const FILE_NAME: &str = "launcher_config.json";

/// The per-user config directory for the launcher (created on demand when saving):
/// `<OS config dir>/vcb-launcher`. Falls back to the executable's directory only if the OS
/// doesn't report a config dir (very rare).
fn config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("vcb-launcher"))
        .unwrap_or_else(exe_dir)
}

/// The active config file, in the persistent per-user location.
fn config_path() -> PathBuf {
    config_dir().join(FILE_NAME)
}

/// The directory the launcher executable sits in (used only for the legacy migration path).
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The old, next-to-the-executable config file that pre-persistent-location builds wrote.
/// Read once for migration so upgrading users keep their saved game folder.
fn legacy_config_path() -> PathBuf {
    exe_dir().join(FILE_NAME)
}

fn read_from(path: &Path) -> Option<Config> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

pub fn load() -> Config {
    // Prefer the persistent location. If it isn't there yet, migrate a legacy next-to-the-exe
    // file (if any) into it, so the switch to a persistent location is invisible to the user.
    if let Some(cfg) = read_from(&config_path()) {
        return cfg;
    }
    if let Some(cfg) = read_from(&legacy_config_path()) {
        save(&cfg); // copy it into the persistent location for next time
        return cfg;
    }
    Config::default()
}

pub fn save(cfg: &Config) {
    let _ = std::fs::create_dir_all(config_dir());
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

/// Remember (or clear) the launcher version the user asked not to be reminded about.
pub fn save_skip_launcher_version(version: Option<String>) {
    let mut cfg = load();
    cfg.skip_launcher_version = version;
    save(&cfg);
}

/// Record the launcher version that just ran (for the "first boot of a new version" check).
pub fn save_last_launcher_version(version: &str) {
    let mut cfg = load();
    cfg.last_launcher_version = Some(version.to_string());
    save(&cfg);
}

/// Remember the chosen UI theme ("classic" or "glass").
pub fn save_ui_theme(theme: &str) {
    let mut cfg = load();
    cfg.ui_theme = Some(theme.to_string());
    save(&cfg);
}
