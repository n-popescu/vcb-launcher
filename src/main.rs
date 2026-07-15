// VCB Mod Launcher — a small, portable GUI for modding Virtual Circuit Board.
//
// It uses the runtime-modding model: patch vcb.pck once with the Godot Mod Loader (keeping a
// pristine backup), then drop mod .zip files into the game's mods/ folder and launch. The
// launcher also keeps the Mod Loader and your installed mods up to date from GitHub.
//
// See README.md.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod gamemods;
mod icon;
mod icon_render;
mod launch;
mod modloader;
mod modmenu;
mod net;
mod patch;
mod pck;
mod pckbuild;
mod projbin;
mod steam;
mod update;

use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// --- palette --------------------------------------------------------------------------
// A calm, slightly cool dark theme. Layered background tones give the header/status bars a
// gentle separation from the content, and a single mint accent carries the primary actions so
// the eye always knows where "go" is.
const BG: egui::Color32 = egui::Color32::from_rgb(0x0f, 0x13, 0x18); // app background
const BG_CONTENT: egui::Color32 = egui::Color32::from_rgb(0x13, 0x17, 0x1d); // content area
const PANEL: egui::Color32 = egui::Color32::from_rgb(0x19, 0x1f, 0x26); // header / status
const PANEL_2: egui::Color32 = egui::Color32::from_rgb(0x11, 0x16, 0x1b); // insets
const CARD: egui::Color32 = egui::Color32::from_rgb(0x1f, 0x26, 0x2f);
const CARD_HOVER: egui::Color32 = egui::Color32::from_rgb(0x27, 0x30, 0x3a);
const CARD_SEL: egui::Color32 = egui::Color32::from_rgb(0x15, 0x35, 0x2d); // selected list row (accent-tinted)
const CARD_BORDER: egui::Color32 = egui::Color32::from_rgb(0x2b, 0x34, 0x3f); // hairline card edge
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3b, 0xd1, 0x9e);
const ACCENT_DK: egui::Color32 = egui::Color32::from_rgb(0x2b, 0xa5, 0x7c);
const TEXT: egui::Color32 = egui::Color32::from_rgb(0xe9, 0xed, 0xf1);
const DIM: egui::Color32 = egui::Color32::from_rgb(0x97, 0xa2, 0xb0);
const FAINT: egui::Color32 = egui::Color32::from_rgb(0x5c, 0x66, 0x72);
// The game-folder text field's placeholder — deliberately greyer than real input so it reads as
// a hint, not as typed text.
const HINT: egui::Color32 = egui::Color32::from_rgb(0x50, 0x5a, 0x66);
const RED: egui::Color32 = egui::Color32::from_rgb(0xf0, 0x6a, 0x6a);
const YELLOW: egui::Color32 = egui::Color32::from_rgb(0xf2, 0xc1, 0x4e);

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 640.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("VCB Mod Launcher")
            .with_icon(icon::app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "VCB Mod Launcher",
        options,
        Box::new(|cc| Box::new(LauncherApp::new(cc))),
    )
}

struct LauncherApp {
    game_dir: Option<PathBuf>,
    game_dir_input: String,
    modding_on: bool,
    status: String,
    status_error: bool,
    // Launcher self-update state.
    launcher_check: Arc<Mutex<update::LauncherCheck>>, // startup version check (worker-filled)
    apply_phase: Arc<Mutex<update::ApplyPhase>>,       // download/apply progress (worker-filled)
    skip_launcher_version: Option<String>, // persisted "don't remind me about this version"
    update_open: bool,                     // the update prompt is showing
    update_acked: bool,                    // dismissed for this session
    update_info: Option<update::LauncherUpdate>, // the offered update, cached from the check
    dont_show_update_again: bool,          // the checkbox in the update prompt
    update_error: Option<String>,          // last apply failure, shown inside the prompt
    // Godot Mod Loader update state (status shown when modding is enabled).
    modloader_check: Arc<Mutex<modloader::ModLoaderCheck>>, // latest version on GitHub
    ml_update_phase: Arc<Mutex<modloader::UpdatePhase>>,    // "update Mod Loader" download
    // The mods installed in the GAME's mods/ folder, their per-mod update checks, and any
    // in-flight per-mod download (see gamemods.rs).
    game_mods: Vec<gamemods::GameMod>,
    mod_checks: gamemods::ChecksMap,
    mod_update: Arc<Mutex<gamemods::UpdatePhase>>,
    // Launcher self-update: once the new binary is swapped in, we don't relaunch automatically —
    // we hold the path and offer a "Restart now" button (it also applies on the next launch).
    pending_restart: Option<PathBuf>,
    // True while a user-initiated "Check for updates" is in flight, so an "up to date" / error
    // result reports back explicitly (the silent startup check doesn't).
    manual_check: bool,
    // Mod list master/detail: the selected mod (by id) shown in the right-hand details pane.
    selected_mod: Option<String>,
    // Backs "Update all" / single-mod updates: a queue drained one at a time (the per-mod update
    // worker handles one mod at a time), so clicking Update all updates every out-of-date mod.
    update_queue: Vec<(gamemods::GameMod, gamemods::ModLatest)>,
    // Set on the first boot of a freshly-updated launcher, to auto re-apply the patch once.
    boot_reapply: bool,
}

impl LauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);

        let cfg = config::load();
        // Prefer the folder the user set last time; only fall back to auto-detection.
        let remembered = cfg
            .game_dir
            .clone()
            .map(PathBuf::from)
            .filter(|p| steam::is_game_dir(p));
        let from_config = remembered.is_some();
        let game_dir = remembered.or_else(steam::find_game_dir);

        // A stale <exe>.old can linger after a previous self-update (a running Windows exe
        // can only be renamed, not deleted); clear it now that we're the fresh process.
        update::cleanup_stale();

        let launcher_check = Arc::new(Mutex::new(update::LauncherCheck::Checking));
        let apply_phase = Arc::new(Mutex::new(update::ApplyPhase::Idle));
        // Check GitHub for a newer launcher in the background so the window opens instantly.
        update::spawn_launcher_check(launcher_check.clone(), cc.egui_ctx.clone());

        // Also check the latest Godot Mod Loader version (surfaced when modding is enabled).
        let modloader_check = Arc::new(Mutex::new(modloader::ModLoaderCheck::Checking));
        let ml_update_phase = Arc::new(Mutex::new(modloader::UpdatePhase::Idle));
        modloader::spawn_check(modloader_check.clone(), cc.egui_ctx.clone());

        let mod_checks: gamemods::ChecksMap = Arc::new(Mutex::new(HashMap::new()));
        let mod_update = Arc::new(Mutex::new(gamemods::UpdatePhase::Idle));

        let mut app = LauncherApp {
            game_dir,
            game_dir_input: String::new(),
            modding_on: false,
            status: String::new(),
            status_error: false,
            launcher_check,
            apply_phase,
            skip_launcher_version: cfg.skip_launcher_version.clone(),
            update_open: false,
            update_acked: false,
            update_info: None,
            dont_show_update_again: false,
            update_error: None,
            modloader_check,
            ml_update_phase,
            game_mods: Vec::new(),
            mod_checks,
            mod_update,
            pending_restart: None,
            manual_check: false,
            selected_mod: None,
            update_queue: Vec::new(),
            boot_reapply: false,
        };
        if let Some(d) = &app.game_dir {
            app.game_dir_input = d.display().to_string();
            app.status = if from_config {
                format!("Using saved game folder {}", d.display())
            } else {
                format!("Found the game at {}", d.display())
            };
        } else {
            app.status = "Couldn't auto-detect a Steam install — set the game folder above."
                .to_string();
            app.status_error = true;
        }
        app.refresh_modding();
        // List the mods installed in the game folder and check each for a newer GitHub release.
        app.refresh_game_mods();
        app.spawn_mod_checks(&cc.egui_ctx);

        // First boot of a freshly-updated launcher → auto re-apply the Mod Loader patch once (a new
        // build may carry a newer seed / patch logic). Recorded so it happens once per new version;
        // performed on the first frame (see poll_updates) so the window still opens instantly.
        let is_new_version = cfg.last_launcher_version.as_deref() != Some(update::CURRENT);
        config::save_last_launcher_version(update::CURRENT);
        app.boot_reapply = is_new_version && app.modding_on && app.game_dir.is_some();

        // Keep the in-game Mod Menu current: pull the latest release from the vcb-modmenu repo
        // into the cache, and — if modding is already enabled — drop that fresh copy into the
        // game's mods/ folder now (best-effort, off the UI thread).
        let mm_mods_dir = if app.modding_on {
            app.game_dir.as_deref().map(patch::mods_dir)
        } else {
            None
        };
        modmenu::spawn_refresh(mm_mods_dir);
        app
    }

    // --- state helpers ---------------------------------------------------------------
    fn set_ok(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_error = false;
    }
    fn set_err(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_error = true;
    }

    fn refresh_modding(&mut self) {
        self.modding_on = self
            .game_dir
            .as_ref()
            .map(|d| patch::modding_enabled(d))
            .unwrap_or(false);
    }

    /// Rescan the mods installed in the GAME's mods/ folder (empty when modding is off / no game).
    fn refresh_game_mods(&mut self) {
        self.game_mods = match (&self.game_dir, self.modding_on) {
            (Some(d), true) => gamemods::scan(&patch::mods_dir(d)),
            _ => Vec::new(),
        };
    }

    /// Kick off a background GitHub check for every installed game mod.
    fn spawn_mod_checks(&self, ctx: &egui::Context) {
        gamemods::spawn_check_all(self.game_mods.clone(), self.mod_checks.clone(), ctx.clone());
    }

    fn rescan_and_check_mods(&mut self, ctx: &egui::Context) {
        self.refresh_game_mods();
        self.spawn_mod_checks(ctx);
    }

    /// Per-frame: surface a pending launcher update as a prompt, and act on any finished
    /// download/apply (relaunch, inform, or report failure).
    fn poll_updates(&mut self, ctx: &egui::Context) {
        // First boot after a launcher update: re-apply the Mod Loader patch once, so the new
        // build's seed / patch logic lands without the user pressing Re-apply. Done here (not in
        // new()) so it runs after the window is already showing.
        if self.boot_reapply {
            self.boot_reapply = false;
            if let Some(dir) = self.game_dir.clone() {
                match self.apply_patch(&dir) {
                    Ok(()) => {
                        self.refresh_modding();
                        self.set_ok("New launcher version — re-applied the Mod Loader patch to vcb.pck.");
                    }
                    Err(e) => self.set_err(format!("Couldn't auto re-apply the patch after the update: {}", e)),
                }
            }
        }

        // Open the update prompt once, the first time the background check reports one that
        // the user hasn't chosen to skip.
        if !self.update_acked && !self.update_open {
            let snapshot = self.launcher_check.lock().unwrap().clone();
            match snapshot {
                update::LauncherCheck::Available(u) => {
                    if self.skip_launcher_version.as_deref() == Some(u.latest.as_str())
                        && !self.manual_check
                    {
                        self.update_acked = true; // already declined this exact version
                    } else {
                        self.dont_show_update_again = false;
                        self.update_info = Some(u);
                        self.update_open = true;
                    }
                    self.manual_check = false;
                }
                // A failed check is non-fatal; silent on startup, but reported for a manual check
                // (no network / rate-limited / no release yet).
                update::LauncherCheck::Error(e) => {
                    if self.manual_check {
                        self.set_err(format!("Couldn't check for updates: {e}"));
                        self.manual_check = false;
                    } else {
                        eprintln!("[vcb-launcher] update check failed: {e}");
                    }
                    self.update_acked = true;
                }
                update::LauncherCheck::UpToDate => {
                    if self.manual_check {
                        self.set_ok(format!("The launcher is up to date (v{}).", update::CURRENT));
                        self.manual_check = false;
                    }
                    self.update_acked = true;
                }
                update::LauncherCheck::Checking => {}
            }
        }

        // React to a completed apply.
        let phase = self.apply_phase.lock().unwrap().clone();
        match phase {
            update::ApplyPhase::Relaunch(exe) => {
                // The binary was swapped in place. Rather than yank the app away, hold the path
                // and offer a "Restart now" button in the prompt — it also applies next launch.
                *self.apply_phase.lock().unwrap() = update::ApplyPhase::Idle;
                self.pending_restart = Some(exe);
                self.set_ok("Launcher update downloaded — restart to apply it.");
            }
            update::ApplyPhase::Message(m) => {
                *self.apply_phase.lock().unwrap() = update::ApplyPhase::Idle;
                self.set_ok(m);
                self.close_update_prompt();
            }
            update::ApplyPhase::Failed(e) => {
                *self.apply_phase.lock().unwrap() = update::ApplyPhase::Idle;
                // Show it inside the prompt (the dim overlay hides the status bar) and leave
                // the prompt open so the user can retry or cancel.
                self.update_error = Some(e);
            }
            update::ApplyPhase::Idle | update::ApplyPhase::Working => {}
        }

        // React to a completed Mod Loader download: re-apply the patch from the fresh cache.
        let ml_phase = self.ml_update_phase.lock().unwrap().clone();
        match ml_phase {
            modloader::UpdatePhase::Downloaded(version) => {
                *self.ml_update_phase.lock().unwrap() = modloader::UpdatePhase::Idle;
                if let Some(dir) = self.game_dir.clone() {
                    match self.apply_patch(&dir) {
                        Ok(()) => {
                            self.refresh_modding();
                            self.set_ok(format!(
                                "Mod Loader updated to v{version} and re-applied to vcb.pck."
                            ));
                        }
                        Err(e) => self.set_err(format!(
                            "Downloaded Mod Loader v{version}, but re-applying to vcb.pck failed: {e}"
                        )),
                    }
                } else {
                    self.set_ok(format!("Downloaded Mod Loader v{version} (set the game folder to apply it)."));
                }
            }
            modloader::UpdatePhase::Failed(e) => {
                *self.ml_update_phase.lock().unwrap() = modloader::UpdatePhase::Idle;
                self.set_err(format!("Mod Loader update failed — {e}"));
            }
            modloader::UpdatePhase::Idle | modloader::UpdatePhase::Working => {}
        }

        // React to a completed game-mod update: refresh the list and re-check.
        let mod_phase = self.mod_update.lock().unwrap().clone();
        match mod_phase {
            gamemods::UpdatePhase::Done(id) => {
                *self.mod_update.lock().unwrap() = gamemods::UpdatePhase::Idle;
                let name = self.mod_display_name(&id);
                self.set_ok(format!("Updated {name}."));
                self.rescan_and_check_mods(ctx);
            }
            gamemods::UpdatePhase::Failed(id, e) => {
                *self.mod_update.lock().unwrap() = gamemods::UpdatePhase::Idle;
                let name = self.mod_display_name(&id);
                self.set_err(format!("Couldn't update {name}: {e}"));
            }
            gamemods::UpdatePhase::Idle | gamemods::UpdatePhase::Working(_) => {}
        }

        // Drain the update queue (from "Update all" / single Update) one mod at a time.
        self.maybe_start_next_queued_update(ctx);
    }

    /// If no per-mod update is in flight and the queue has entries, start the next one.
    fn maybe_start_next_queued_update(&mut self, ctx: &egui::Context) {
        let idle = matches!(*self.mod_update.lock().unwrap(), gamemods::UpdatePhase::Idle);
        if !idle {
            return;
        }
        if let Some((gm, latest)) = self.update_queue.pop() {
            gamemods::spawn_update(gm, latest.asset_url, self.mod_update.clone(), ctx.clone());
        }
    }

    /// Queue a single mod for update (deduped; skipped if it's already updating). The queue is
    /// drained by `maybe_start_next_queued_update`.
    fn queue_update(&mut self, gm: gamemods::GameMod, latest: gamemods::ModLatest) {
        if let gamemods::UpdatePhase::Working(id) = &*self.mod_update.lock().unwrap() {
            if *id == gm.id {
                return;
            }
        }
        if self.update_queue.iter().any(|(g, _)| g.id == gm.id) {
            return;
        }
        self.update_queue.push((gm, latest));
    }

    /// Queue every installed mod that has an update available ("Update all").
    fn update_all(&mut self) {
        let checks = self.mod_checks.lock().unwrap().clone();
        let mut queued = 0;
        for gm in self.game_mods.clone() {
            if let Some(gamemods::ModCheck::Available(l)) = checks.get(&gm.id) {
                let before = self.update_queue.len();
                self.queue_update(gm, l.clone());
                if self.update_queue.len() > before {
                    queued += 1;
                }
            }
        }
        if queued == 0 {
            self.set_ok("All mods are up to date.");
        } else {
            self.set_ok(format!("Updating {} mod(s)…", queued));
        }
    }

    fn mod_display_name(&self, id: &str) -> String {
        self.game_mods
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.display_name())
            .unwrap_or_else(|| id.to_string())
    }

    /// User-initiated "Check for updates": re-run the launcher self-update check (reporting the
    /// result even when up to date) and re-check every installed game mod.
    fn check_for_updates(&mut self, ctx: &egui::Context) {
        self.manual_check = true;
        self.update_acked = false; // let the prompt reopen if a launcher update is found
        self.set_ok("Checking for updates…");
        update::spawn_launcher_check(self.launcher_check.clone(), ctx.clone());
        modloader::spawn_check(self.modloader_check.clone(), ctx.clone());
        self.rescan_and_check_mods(ctx);
    }

    /// Close the update prompt, persisting the "don't show again until the next version"
    /// choice: ticked → skip this exact version; unticked → clear any skip (prompt returns
    /// next start, which is when the check runs).
    fn close_update_prompt(&mut self) {
        let skip = if self.dont_show_update_again {
            self.update_info.as_ref().map(|u| u.latest.clone())
        } else {
            None
        };
        if skip != self.skip_launcher_version {
            self.skip_launcher_version = skip.clone();
            config::save_skip_launcher_version(skip);
        }
        self.update_open = false;
        self.update_acked = true;
        self.update_error = None;
    }

    fn start_launcher_update(&mut self, ctx: &egui::Context) {
        if let Some(asset) = self.update_info.as_ref().and_then(|u| u.asset.clone()) {
            self.update_error = None;
            update::spawn_apply(asset, self.apply_phase.clone(), ctx.clone());
        }
    }

    /// The Mod Loader up-to-date / out-of-date indicator shown beside the Disable button
    /// while modding is enabled, with an inline "Update" action when a newer release exists.
    fn modloader_status_inline(&mut self, ui: &mut egui::Ui) {
        if matches!(*self.ml_update_phase.lock().unwrap(), modloader::UpdatePhase::Working) {
            ui.spinner();
            ui.label(egui::RichText::new("Updating Mod Loader…").size(12.0).color(DIM));
            return;
        }
        let (latest, check_err) = match &*self.modloader_check.lock().unwrap() {
            modloader::ModLoaderCheck::Known(l) => (Some(l.clone()), None),
            modloader::ModLoaderCheck::Error(e) => (None, Some(e.clone())),
            modloader::ModLoaderCheck::Checking => (None, None),
        };
        let applied = self
            .game_dir
            .as_ref()
            .and_then(|d| patch::applied_version(&patch::pck_path(d)));

        match (latest, applied) {
            (Some(latest), Some(applied)) if update::is_newer(&latest.version, &applied) => {
                ui.label(egui::RichText::new("⬆").size(13.0).color(YELLOW));
                ui.label(
                    egui::RichText::new(format!("Mod Loader out of date (v{applied} → v{})", latest.version))
                        .size(12.0)
                        .color(YELLOW),
                );
                if ui
                    .add(pill_button("Update"))
                    .on_hover_text("Download the latest Godot Mod Loader and re-apply it to vcb.pck")
                    .clicked()
                {
                    let ctx = ui.ctx().clone();
                    modloader::spawn_download(latest.zipball_url.clone(), self.ml_update_phase.clone(), ctx);
                }
            }
            (Some(_), Some(applied)) => {
                ui.label(egui::RichText::new("●").size(12.0).color(ACCENT));
                ui.label(egui::RichText::new(format!("Mod Loader up to date (v{applied})")).size(12.0).color(DIM));
            }
            (None, Some(applied)) => {
                // Couldn't reach GitHub (offline / rate-limited) or still checking — just show
                // what's installed, with the check error (if any) on hover.
                let resp = ui.label(egui::RichText::new(format!("Mod Loader v{applied}")).size(12.0).color(FAINT));
                if let Some(err) = check_err {
                    resp.on_hover_text(format!("Couldn't check for Mod Loader updates: {err}"));
                }
            }
            _ => {
                ui.label(egui::RichText::new("Mod Loader").size(12.0).color(FAINT));
            }
        }
    }

    /// Patch `vcb.pck` from the best available Mod Loader: the web-downloaded copy in the
    /// `modloader/` cache if present, otherwise the built-in seed. So enable / Re-apply /
    /// "Update Mod Loader" all bake in the newest version the launcher has.
    fn apply_patch(&self, dir: &Path) -> Result<(), patch::PatchError> {
        match modloader::cached_addon_owned() {
            Some(owned) => {
                let refs: Vec<(&str, &[u8])> =
                    owned.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
                patch::enable_modding_with(dir, &refs)
            }
            None => patch::enable_modding(dir),
        }
    }

    /// Patch the game's `vcb.pck` with the Godot Mod Loader (keeping the pristine
    /// original as `vcb.pck.original`). Refuses if the current pck looks like a mod and
    /// there's no clean backup, so we never bake a mod in as the "original".
    fn enable_modding(&mut self) {
        let Some(dir) = self.game_dir.clone() else {
            self.set_err("Set the game folder first.");
            return;
        };
        let pck = patch::pck_path(&dir);
        let has_backup = patch::has_backup(&dir);
        if !has_backup && !patch::is_patched(&pck) && !patch::is_vanilla_pck(&pck) {
            self.set_err("Your current vcb.pck looks like a mod. Revert to vanilla (or Steam → Verify integrity of game files) first, then enable modding.");
            return;
        }
        match self.apply_patch(&dir) {
            Ok(()) => {
                self.refresh_modding();
                self.refresh_game_mods();
                self.set_ok("Modding enabled — vcb.pck patched with the Godot Mod Loader. The in-game mod list (Options ▸ Mods) is installed automatically. Drop more Mod Loader mods (.zip) into the game's mods/ folder, then Launch game.");
            }
            Err(e) => self.set_err(format!("Couldn't enable modding: {}", e)),
        }
    }

    fn disable_modding(&mut self) {
        let Some(dir) = self.game_dir.clone() else {
            self.set_err("Set the game folder first.");
            return;
        };
        match patch::disable_modding(&dir) {
            Ok(()) => {
                self.refresh_modding();
                self.refresh_game_mods();
                self.set_ok("Modding disabled — restored the original vcb.pck.");
            }
            Err(e) => self.set_err(format!("Couldn't disable modding: {}", e)),
        }
    }

    /// Always-available escape hatch: restore the pristine `vcb.pck.original` (undo any patch or
    /// foreign mod). Same restore point the Mod Loader patch keeps.
    fn revert_to_vanilla(&mut self) {
        let Some(dir) = self.game_dir.clone() else {
            self.set_err("Set the game folder first.");
            return;
        };
        match patch::disable_modding(&dir) {
            Ok(()) => {
                self.refresh_modding();
                self.refresh_game_mods();
                self.set_ok("Restored the vanilla game (vcb.pck.original).");
            }
            Err(e) => self.set_err(format!("Couldn't revert: {}", e)),
        }
    }

    fn detect_game(&mut self) {
        match steam::find_game_dir() {
            Some(d) => {
                self.game_dir_input = d.display().to_string();
                self.game_dir = Some(d.clone());
                config::save_game_dir(&d);
                self.refresh_modding();
                self.refresh_game_mods();
                self.set_ok(format!("Found the game at {}", d.display()));
            }
            None => self.set_err("No Steam install found in the usual locations."),
        }
    }

    fn set_game_from_input(&mut self) {
        let p = PathBuf::from(self.game_dir_input.trim());
        if !p.is_dir() {
            self.set_err("That folder doesn't exist.");
            return;
        }
        if !steam::is_game_dir(&p) {
            self.set_err("That folder has no vcb.pck or vcb executable — pick the game folder.");
            return;
        }
        self.game_dir = Some(p.clone());
        config::save_game_dir(&p);
        self.refresh_modding();
        self.refresh_game_mods();
        self.set_ok(format!("Using game folder {} (remembered for next time)", p.display()));
    }

    fn launch_current(&mut self) {
        let Some(dir) = self.game_dir.clone() else {
            self.set_err("Set the game folder first.");
            return;
        };
        match launch::launch_game(&dir) {
            Ok(()) => self.set_ok("Launched the game."),
            Err(e) => self.set_err(format!("Couldn't launch the game: {}", e)),
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_updates(ctx);

        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(20.0, 15.0)))
            .show(ctx, |ui| self.header_ui(ui));

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(20.0, 10.0)))
            .show(ctx, |ui| self.status_ui(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_CONTENT).inner_margin(egui::Margin::same(20.0)))
            .show(ctx, |ui| {
                // The content stacks several cards that can outgrow a short window, so it scrolls
                // (mouse wheel + touchpad — egui maps both to the same scroll events).
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.content_ui(ui));
            });

        // The self-update prompt renders as a centered modal over everything.
        if self.update_open {
            self.update_modal(ctx);
        }
    }
}

impl LauncherApp {
    fn header_ui(&mut self, ui: &mut egui::Ui) {
        let can_revert = self.game_dir.as_ref().map(|d| patch::has_backup(d)).unwrap_or(false);
        ui.horizontal(|ui| {
            let (logo_rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
            paint_logo(ui.painter(), logo_rect);
            ui.add_space(8.0);
            ui.label(egui::RichText::new("VCB").size(22.0).strong().color(ACCENT));
            ui.label(egui::RichText::new("Mod Launcher").size(22.0).strong().color(TEXT));

            // Always-available "go back to the unmodded game" action.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = egui::RichText::new("⟲  Revert to vanilla")
                    .color(if can_revert { TEXT } else { FAINT });
                let btn = egui::Button::new(label)
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(1.0, if can_revert { RED } else { FAINT }))
                    .rounding(egui::Rounding::same(999.0));
                let hover = if can_revert {
                    "Restore the original vcb.pck (vcb.pck.original) — undo any patch or mod"
                } else {
                    "No vanilla backup yet — it's created the first time you enable modding over a clean install"
                };
                if ui.add_enabled(can_revert, btn).on_hover_text(hover).clicked() {
                    self.revert_to_vanilla();
                }
            });
        });

        ui.add_space(14.0);

        // Game folder row.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Game folder").size(12.0).color(DIM));
            let found = self.game_dir.as_ref().map(|d| steam::is_game_dir(d)).unwrap_or(false);
            if found {
                ui.label(egui::RichText::new("●").size(12.0).color(ACCENT))
                    .on_hover_text("Game folder looks good");
            } else {
                ui.label(egui::RichText::new("●").size(12.0).color(RED))
                    .on_hover_text("No vcb.pck / vcb executable found here yet");
            }
        });
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            ui.add(
                egui::TextEdit::singleline(&mut self.game_dir_input)
                    // A greyer placeholder so the example path reads as a hint, not as input.
                    .hint_text(
                        egui::RichText::new("…/steamapps/common/Virtual Circuit Board").color(HINT),
                    )
                    .desired_width(avail - 184.0),
            );
            if ui.add(ghost_button("Use")).clicked() {
                self.set_game_from_input();
            }
            if ui.add(ghost_button("Auto-detect")).clicked() {
                self.detect_game();
            }
        });
    }

    fn status_ui(&mut self, ui: &mut egui::Ui) {
        let (dot, color) = if self.status_error { ("⚠", RED) } else { ("●", ACCENT) };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(dot).size(12.0).color(color));
            ui.add_space(2.0);
            ui.label(egui::RichText::new(&self.status).size(12.0).color(if self.status_error { RED } else { DIM }));
        });
    }

    // ================================ Main content ================================
    fn content_ui(&mut self, ui: &mut egui::Ui) {
        let has_game = self.game_dir.is_some();

        section_header(ui, "Runtime modding", "Patch once, then load many mods from the game's mods/ folder");
        ui.add_space(12.0);

        // Status + controls card.
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if self.modding_on {
                    ui.label(egui::RichText::new("●").color(ACCENT).size(15.0));
                    ui.label(egui::RichText::new("Modding is enabled").size(15.0).strong().color(TEXT));
                    ui.label(egui::RichText::new("vcb.pck is patched with the Godot Mod Loader").size(12.0).color(DIM));
                } else {
                    ui.label(egui::RichText::new("●").color(FAINT).size(15.0));
                    ui.label(egui::RichText::new("Modding is disabled").size(15.0).strong().color(TEXT));
                    ui.label(egui::RichText::new("the game is running its stock vcb.pck").size(12.0).color(DIM));
                }
            });

            ui.add_space(14.0);

            ui.horizontal(|ui| {
                // Primary action: launch the (patched) game.
                let launch_hint = if self.modding_on {
                    "Start the game — the Mod Loader loads every .zip in the game's mods/ folder"
                } else {
                    "Start the game (currently unmodded — enable modding first to load mods)"
                };
                if ui
                    .add_enabled(has_game, primary_button("▶  Launch game"))
                    .on_hover_text(launch_hint)
                    .clicked()
                {
                    self.launch_current();
                }

                if self.modding_on {
                    if ui
                        .add(ghost_button("📁  Mods folder"))
                        .on_hover_text("Open the game's mods/ folder — drop Mod Loader mods (.zip) here")
                        .clicked()
                    {
                        if let Some(d) = self.game_dir.clone() {
                            open_path(&patch::mods_dir(&d));
                        }
                    }
                    if ui
                        .add(ghost_button("Re-apply"))
                        .on_hover_text("Re-patch from the pristine original (e.g. after a Steam game update)")
                        .clicked()
                    {
                        self.enable_modding();
                    }
                    if ui
                        .add(danger_button("Disable"))
                        .on_hover_text("Restore the original vcb.pck")
                        .clicked()
                    {
                        self.disable_modding();
                    }
                    ui.add_space(10.0);
                    self.modloader_status_inline(ui);
                } else if ui
                    .add_enabled(has_game, primary_button("Enable modding"))
                    .on_hover_text("Patch vcb.pck with the Godot Mod Loader so it can load mods at runtime (keeps a pristine backup)")
                    .clicked()
                {
                    self.enable_modding();
                }
            });
        });

        ui.add_space(16.0);
        self.updates_card(ui);

        if self.modding_on {
            ui.add_space(16.0);
            self.game_mods_card(ui);
        }

        ui.add_space(16.0);

        // How-it-works helper.
        egui::Frame::none()
            .fill(PANEL_2)
            .rounding(egui::Rounding::same(12.0))
            .stroke(egui::Stroke::new(1.0, CARD_BORDER))
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new("How it works").size(13.0).strong().color(TEXT));
                ui.add_space(8.0);
                for (n, line) in [
                    "Enable modding — the launcher snapshots your pristine vcb.pck and bakes the Mod Loader into a fresh copy. Your original is never lost.",
                    "Open the mods folder and drop in Mod Loader packages (.zip). Several mods can live side by side.",
                    "Launch game — the Mod Loader loads every mod at startup, on the original game engine.",
                ]
                .iter()
                .enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{}.", n + 1)).size(12.0).strong().color(ACCENT));
                        ui.label(egui::RichText::new(*line).size(12.0).color(DIM));
                    });
                    ui.add_space(3.0);
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("After a Steam update replaces vcb.pck, press Re-apply. Full guide: docs/MODDING.md.")
                        .size(11.0)
                        .color(FAINT),
                );
            });

        if !has_game {
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new("Set your game folder at the top before enabling modding or launching.")
                    .size(12.0)
                    .color(YELLOW),
            );
        }
    }

    /// Small "Updates" card: the launcher version, a manual "Check for updates" button (checks the
    /// launcher AND the installed mods), and — after a launcher update has downloaded — a
    /// "Restart to apply" button.
    fn updates_card(&mut self, ui: &mut egui::Ui) {
        let has_restart = self.pending_restart.is_some();
        let mut do_check = false;
        let mut do_restart = false;
        egui::Frame::none()
            .fill(PANEL_2)
            .rounding(egui::Rounding::same(12.0))
            .stroke(egui::Stroke::new(1.0, CARD_BORDER))
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Updates").size(13.0).strong().color(TEXT));
                    ui.label(egui::RichText::new(format!("launcher v{}", update::CURRENT)).size(11.0).color(DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(pill_button("Check for updates"))
                            .on_hover_text("Check GitHub for a newer launcher and newer versions of your installed mods")
                            .clicked()
                        {
                            do_check = true;
                        }
                    });
                });
                if has_restart {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary_button("Restart to apply update")).clicked() {
                            do_restart = true;
                        }
                        ui.label(
                            egui::RichText::new("A launcher update was downloaded — it also applies next time you open the launcher.")
                                .size(11.0)
                                .color(DIM),
                        );
                    });
                }
            });
        let ctx = ui.ctx().clone();
        if do_check {
            self.check_for_updates(&ctx);
        }
        if do_restart {
            self.do_restart();
        }
    }

    fn do_restart(&mut self) {
        if let Some(exe) = self.pending_restart.take() {
            let _ = std::process::Command::new(&exe).spawn();
            std::process::exit(0);
        }
    }

    /// "Mods in your game folder": a master/detail view like the in-game Mod Menu — a clickable
    /// list of installed Mod Loader mods on the left (name, version, update indicator) and the
    /// selected mod's full details on the right (description, repo, and a single-mod Update button
    /// beside its version). "Update all" and "Check for updates" sit at the top. Modding-only.
    fn game_mods_card(&mut self, ui: &mut egui::Ui) {
        let mods = self.game_mods.clone();
        let checks = self.mod_checks.lock().unwrap().clone();
        let working = match &*self.mod_update.lock().unwrap() {
            gamemods::UpdatePhase::Working(id) => Some(id.clone()),
            _ => None,
        };
        let any_updates = mods
            .iter()
            .any(|gm| matches!(checks.get(&gm.id), Some(gamemods::ModCheck::Available(_))));

        // Default the selection to the first mod so the details pane is never empty.
        if self.selected_mod.as_ref().map(|id| !mods.iter().any(|m| &m.id == id)).unwrap_or(true) {
            self.selected_mod = mods.first().map(|m| m.id.clone());
        }
        let selected = self.selected_mod.clone();

        let mut do_check = false;
        let mut do_rescan = false;
        let mut open_folder = false;
        let mut do_update_all = false;
        let mut new_selection: Option<String> = None;
        let mut do_update: Option<(gamemods::GameMod, gamemods::ModLatest)> = None;

        egui::Frame::none()
            .fill(PANEL_2)
            .rounding(egui::Rounding::same(12.0))
            .stroke(egui::Stroke::new(1.0, CARD_BORDER))
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Mods in your game folder").size(13.0).strong().color(TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(any_updates, primary_button("Update all"))
                            .on_hover_text("Update every installed mod that has a newer release")
                            .clicked()
                        {
                            do_update_all = true;
                        }
                        if ui.add(pill_button("Check for updates")).on_hover_text("Check GitHub for newer versions of the launcher and every installed mod").clicked() {
                            do_check = true;
                        }
                        if ui.add(pill_button("⟳")).on_hover_text("Rescan the game's mods folder").clicked() {
                            do_rescan = true;
                        }
                        if ui.add(pill_button("📁 Mods folder")).clicked() {
                            open_folder = true;
                        }
                    });
                });
                ui.add_space(10.0);

                if mods.is_empty() {
                    ui.label(
                        egui::RichText::new("No mods here yet — open the mods folder and drop in Mod Loader .zip mods, then press ⟳.")
                            .size(12.0)
                            .color(DIM),
                    );
                    return;
                }

                // Fixed-height master/detail body (bounded so its inner lists scroll even though the
                // whole tab is inside an outer scroll area).
                let body_h = 300.0_f32;
                let total_w = ui.available_width();
                let list_w = (total_w * 0.42).clamp(170.0, 300.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(total_w, body_h),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        // Left: the mod list.
                        ui.allocate_ui_with_layout(
                            egui::vec2(list_w, body_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(list_w);
                                egui::ScrollArea::vertical()
                                    .id_source("gm_list")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for gm in &mods {
                                            let is_working = working.as_deref() == Some(gm.id.as_str());
                                            let is_sel = selected.as_deref() == Some(gm.id.as_str());
                                            if mod_list_row(ui, gm, checks.get(&gm.id), is_working, is_sel) {
                                                new_selection = Some(gm.id.clone());
                                            }
                                        }
                                    });
                            },
                        );
                        ui.separator();
                        // Right: details for the selected mod.
                        egui::ScrollArea::vertical()
                            .id_source("gm_detail")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if let Some(gm) = mods.iter().find(|m| Some(m.id.as_str()) == selected.as_deref()) {
                                    let is_working = working.as_deref() == Some(gm.id.as_str());
                                    if let Some(latest) = mod_detail_ui(ui, gm, checks.get(&gm.id), is_working) {
                                        do_update = Some((gm.clone(), latest));
                                    }
                                } else {
                                    ui.add_space(20.0);
                                    ui.label(egui::RichText::new("Select a mod on the left.").size(13.0).color(DIM));
                                }
                            });
                    },
                );
            });

        let ctx = ui.ctx().clone();
        if let Some(id) = new_selection {
            self.selected_mod = Some(id);
        }
        if do_update_all {
            self.update_all();
        }
        if let Some((gm, latest)) = do_update {
            self.queue_update(gm, latest);
        }
        if do_check {
            self.check_for_updates(&ctx);
        }
        if do_rescan {
            self.refresh_game_mods();
            self.set_ok("Rescanned the game's mods folder.");
        }
        if open_folder {
            if let Some(d) = self.game_dir.clone() {
                open_path(&patch::mods_dir(&d));
            }
        }
    }

    // ============================ Self-update prompt ============================
    fn update_modal(&mut self, ctx: &egui::Context) {
        let Some(info) = self.update_info.clone() else {
            self.update_open = false;
            return;
        };
        let working = matches!(*self.apply_phase.lock().unwrap(), update::ApplyPhase::Working);
        let has_asset = info.asset.is_some();
        let downloaded = self.pending_restart.is_some();

        // Dim the window behind the dialog and swallow clicks.
        let screen = ctx.screen_rect();
        egui::Area::new("update_dim".into())
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(rect, egui::Rounding::ZERO, egui::Color32::from_black_alpha(150));
                let _ = resp;
            });

        egui::Area::new("update_prompt".into())
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
              egui::Frame::none()
                    .fill(PANEL)
                    .rounding(egui::Rounding::same(14.0))
                    .stroke(egui::Stroke::new(1.0, CARD_BORDER))
                    .inner_margin(egui::Margin::same(22.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 6.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: egui::Color32::from_black_alpha(120),
                    })
              .show(ui, |ui| {
                ui.set_max_width(460.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⬆").size(20.0).color(ACCENT));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Launcher update available").size(17.0).strong().color(TEXT));
                });
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!(
                        "You're running {}. Version {} is available.",
                        info.current, info.latest
                    ))
                    .size(13.0)
                    .color(DIM),
                );
                ui.add_space(6.0);
                if downloaded {
                    ui.label(
                        egui::RichText::new("Downloaded and swapped in. Restart to apply it now — or it applies next time you open the launcher.")
                            .size(12.0)
                            .color(ACCENT),
                    );
                } else if has_asset {
                    ui.label(
                        egui::RichText::new(if cfg!(target_os = "macos") {
                            "Update now downloads the new build and reveals it in Finder — unzip it and replace the app to finish."
                        } else {
                            "Update now downloads the new build and swaps it in; then restart the launcher to apply it."
                        })
                        .size(12.0)
                        .color(FAINT),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("No prebuilt download was found for this platform — open the releases page to grab it.")
                            .size(12.0)
                            .color(YELLOW),
                    );
                }

                if working {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("Downloading…").size(12.0).color(DIM));
                    });
                } else if let Some(err) = &self.update_error {
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("⚠").size(12.0).color(RED));
                        ui.label(egui::RichText::new(err).size(12.0).color(RED));
                    });
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(12.0);

                if downloaded {
                    // The binary is already in place; offer a restart (or defer to next launch).
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(primary_button("Restart now")).clicked() {
                            self.close_update_prompt();
                            self.do_restart();
                        }
                        if ui.add(ghost_button("Later")).clicked() {
                            self.close_update_prompt();
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.add_enabled(!working, egui::Checkbox::without_text(&mut self.dont_show_update_again));
                        ui.label(egui::RichText::new("Don't show again until the next version").size(12.0).color(DIM));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if has_asset {
                                if ui.add_enabled(!working, primary_button("Update now")).clicked() {
                                    self.start_launcher_update(ctx);
                                }
                            } else if ui.add_enabled(!working, primary_button("Open releases page")).clicked() {
                                open_url(&info.html_url);
                            }
                            if ui.add_enabled(!working, ghost_button("Cancel")).clicked() {
                                self.close_update_prompt();
                            }
                        });
                    });
                }
              });
            });
    }
}

// --- widgets --------------------------------------------------------------------------

/// One row in the mod list (left pane): the mod name, its version, and a compact update
/// indicator. Clickable + selectable; returns true when clicked this frame. Free-standing so
/// `game_mods_card` doesn't borrow `self` inside the layout closures.
fn mod_list_row(
    ui: &mut egui::Ui,
    gm: &gamemods::GameMod,
    check: Option<&gamemods::ModCheck>,
    is_working: bool,
    selected: bool,
) -> bool {
    let fill = if selected { CARD_SEL } else { CARD };
    let stroke = if selected {
        egui::Stroke::new(1.0, ACCENT)
    } else {
        egui::Stroke::new(1.0, CARD_BORDER)
    };
    let mut resp = egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(10.0))
        .stroke(stroke)
        .inner_margin(egui::Margin::symmetric(12.0, 9.0))
        .outer_margin(egui::Margin { top: 0.0, bottom: 6.0, left: 0.0, right: 0.0 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(gm.display_name()).size(13.5).strong().color(TEXT));
                ui.horizontal(|ui| {
                    let ver = if gm.version.is_empty() { "—".to_string() } else { format!("v{}", gm.version) };
                    ui.label(egui::RichText::new(ver).size(11.0).color(DIM));
                    if is_working {
                        ui.label(egui::RichText::new("· updating…").size(11.0).color(DIM));
                    } else {
                        match check {
                            Some(gamemods::ModCheck::Available(l)) => {
                                ui.label(egui::RichText::new(format!("⬆ v{}", l.version)).size(11.0).color(YELLOW))
                                    .on_hover_text("Update available");
                            }
                            Some(gamemods::ModCheck::UpToDate) => {
                                ui.label(egui::RichText::new("●").size(10.0).color(ACCENT))
                                    .on_hover_text("Up to date");
                            }
                            Some(gamemods::ModCheck::Checking) => {
                                ui.label(egui::RichText::new("· checking…").size(11.0).color(FAINT));
                            }
                            _ => {}
                        }
                    }
                });
            });
        })
        .response
        .interact(egui::Sense::click());
    resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() && !selected {
        ui.painter().rect_filled(
            resp.rect,
            egui::Rounding::same(10.0),
            egui::Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 8),
        );
    }
    resp.clicked()
}

/// The details pane (right) for the selected mod: name, version + a single-mod Update button when
/// an update is available, repo link, and the full description. Returns the available `ModLatest`
/// when its Update button is clicked this frame.
fn mod_detail_ui(
    ui: &mut egui::Ui,
    gm: &gamemods::GameMod,
    check: Option<&gamemods::ModCheck>,
    is_working: bool,
) -> Option<gamemods::ModLatest> {
    let mut clicked_latest: Option<gamemods::ModLatest> = None;

    ui.label(egui::RichText::new(gm.display_name()).size(20.0).strong().color(TEXT));
    ui.add_space(4.0);

    // Version + update status / action, side by side (the "Update" button sits by the version).
    ui.horizontal(|ui| {
        let ver = if gm.version.is_empty() { "no version".to_string() } else { format!("v{}", gm.version) };
        ui.label(egui::RichText::new(ver).size(13.0).color(DIM));
        if is_working {
            ui.spinner();
            ui.label(egui::RichText::new("Updating…").size(12.0).color(DIM));
        } else {
            match check {
                Some(gamemods::ModCheck::Available(l)) => {
                    if ui
                        .add(pill_button("Update"))
                        .on_hover_text(format!("Download v{} and replace this mod", l.version))
                        .clicked()
                    {
                        clicked_latest = Some(l.clone());
                    }
                    ui.label(egui::RichText::new(format!("⬆ v{} available", l.version)).size(12.0).color(YELLOW));
                }
                Some(gamemods::ModCheck::UpToDate) => {
                    ui.label(egui::RichText::new("●").size(12.0).color(ACCENT));
                    ui.label(egui::RichText::new("up to date").size(12.0).color(DIM));
                }
                Some(gamemods::ModCheck::Checking) => {
                    ui.spinner();
                    ui.label(egui::RichText::new("checking…").size(12.0).color(FAINT));
                }
                Some(gamemods::ModCheck::NoRepo) => {
                    ui.label(egui::RichText::new("no update source").size(12.0).color(FAINT))
                        .on_hover_text("This mod's manifest has no GitHub website_url, so the launcher can't check for updates");
                }
                Some(gamemods::ModCheck::Error(e)) => {
                    ui.label(egui::RichText::new("check failed").size(12.0).color(FAINT)).on_hover_text(e.clone());
                }
                None => {
                    ui.label(egui::RichText::new("not checked").size(12.0).color(FAINT));
                }
            }
        }
    });

    ui.add_space(6.0);
    if !gm.website.is_empty() {
        let label = match &gm.repo {
            Some((o, r)) => format!("{o}/{r}"),
            None => gm.website.clone(),
        };
        ui.hyperlink_to(egui::RichText::new(label).size(12.0).color(ACCENT), &gm.website);
    }
    ui.label(egui::RichText::new(format!("id: {}", gm.id)).size(11.0).color(FAINT));

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);

    if gm.description.trim().is_empty() {
        ui.label(egui::RichText::new("This mod's manifest has no description.").size(12.0).color(DIM));
    } else {
        ui.label(egui::RichText::new(&gm.description).size(13.0).color(TEXT));
    }

    clicked_latest
}

/// A framed content card (rounded, hairline-bordered).
fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .rounding(egui::Rounding::same(12.0))
        .stroke(egui::Stroke::new(1.0, CARD_BORDER))
        .inner_margin(egui::Margin::same(16.0))
}

/// Accent-filled primary button.
fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(egui::Color32::BLACK).strong())
        .fill(ACCENT)
        .stroke(egui::Stroke::new(1.0, ACCENT_DK))
        .rounding(egui::Rounding::same(10.0))
}

/// Neutral outlined secondary button.
fn ghost_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(TEXT))
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0, CARD_BORDER))
        .rounding(egui::Rounding::same(10.0))
}

/// A fully-rounded "pill" button — the round, vanilla-like style used for the mod actions
/// (mirrors the in-game Mod Menu's rounded buttons).
fn pill_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(TEXT))
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0, CARD_BORDER))
        .rounding(egui::Rounding::same(999.0))
}

/// Outlined danger button.
fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(RED))
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, RED))
        .rounding(egui::Rounding::same(10.0))
}

/// A section title + one-line description.
fn section_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(egui::RichText::new(title).size(18.0).strong().color(TEXT));
    ui.add_space(2.0);
    ui.label(egui::RichText::new(subtitle).size(12.0).color(DIM));
}

/// The app's little logo — a circuit chip with pins and a lit central via — drawn as
/// vectors so it stays crisp at any DPI. Matches the window icon in `icon.rs`.
fn paint_logo(p: &egui::Painter, rect: egui::Rect) {
    let c = rect.center();
    let s = rect.width();
    let body = egui::Rect::from_center_size(c, rect.size() * 0.72);
    let rounding = egui::Rounding::same(4.0);
    let pin = egui::Stroke::new(1.6, ACCENT.linear_multiply(0.75));

    // Chip pins (three per side), behind the body.
    for k in [-1.0_f32, 0.0, 1.0] {
        let o = k * s * 0.2;
        p.line_segment([egui::pos2(body.left() - s * 0.12, c.y + o), egui::pos2(body.left(), c.y + o)], pin);
        p.line_segment([egui::pos2(body.right(), c.y + o), egui::pos2(body.right() + s * 0.12, c.y + o)], pin);
        p.line_segment([egui::pos2(c.x + o, body.top() - s * 0.12), egui::pos2(c.x + o, body.top())], pin);
        p.line_segment([egui::pos2(c.x + o, body.bottom()), egui::pos2(c.x + o, body.bottom() + s * 0.12)], pin);
    }

    // Chip body.
    p.rect_filled(body, rounding, CARD);
    p.rect_stroke(body, rounding, egui::Stroke::new(1.8, ACCENT));

    // Traces out of the via.
    let tr = egui::Stroke::new(1.6, ACCENT);
    p.line_segment([c, egui::pos2(body.right() - 2.0, c.y)], tr);
    p.line_segment([c, egui::pos2(c.x, body.bottom() - 2.0)], tr);

    // Lit via.
    p.circle_filled(c, 3.0, ACCENT);
    p.circle_filled(c, 1.2, BG);
}

// --- style / os ------------------------------------------------------------------------

fn setup_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x0b, 0x0e, 0x12);
    visuals.faint_bg_color = CARD;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.inactive.rounding = egui::Rounding::same(10.0);
    visuals.widgets.hovered.rounding = egui::Rounding::same(10.0);
    visuals.widgets.active.rounding = egui::Rounding::same(10.0);
    visuals.widgets.inactive.bg_fill = CARD;
    visuals.widgets.hovered.bg_fill = CARD_HOVER;
    visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
    visuals.widgets.inactive.weak_bg_fill = CARD;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    ctx.set_style(style);
}

/// Open a folder in the OS file manager.
fn open_path(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Open a URL in the default browser.
fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
