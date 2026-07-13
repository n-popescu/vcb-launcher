// VCB Mod Launcher — a small, portable GUI for modding Virtual Circuit Board. It supports
// two models, one per tab:
//   • Runtime modding (recommended) — patch vcb.pck once with the Godot Mod Loader, then
//     drop mod .zip files into the game's mods/ folder and launch.
//   • Legacy (.pck swap) — replace the whole vcb.pck with a modded one, one mod at a time,
//     keeping a backup of the original.
// See README.md.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod archive;
mod bundled;
mod config;
mod icon;
mod icon_render;
mod install;
mod meta;
mod modloader;
mod net;
mod patch;
mod pck;
mod pckbuild;
mod projbin;
mod scan;
mod steam;
mod update;

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// --- palette --------------------------------------------------------------------------
// A calm, slightly cool dark theme. Two background tones give the header/status bars a
// gentle separation from the content, and a single mint accent carries the primary
// actions so the eye always knows where "go" is.
const BG: egui::Color32 = egui::Color32::from_rgb(0x10, 0x14, 0x19); // app background
const BG_CONTENT: egui::Color32 = egui::Color32::from_rgb(0x14, 0x18, 0x1e); // content area
const PANEL: egui::Color32 = egui::Color32::from_rgb(0x1a, 0x20, 0x27); // header / status
const PANEL_2: egui::Color32 = egui::Color32::from_rgb(0x11, 0x16, 0x1b); // insets (tab track)
const CARD: egui::Color32 = egui::Color32::from_rgb(0x20, 0x27, 0x30);
const CARD_HOVER: egui::Color32 = egui::Color32::from_rgb(0x27, 0x30, 0x3a);
const CARD_SEL: egui::Color32 = egui::Color32::from_rgb(0x15, 0x35, 0x2d);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3b, 0xd1, 0x9e);
const ACCENT_DK: egui::Color32 = egui::Color32::from_rgb(0x2b, 0xa5, 0x7c);
const TEXT: egui::Color32 = egui::Color32::from_rgb(0xe9, 0xed, 0xf1);
const DIM: egui::Color32 = egui::Color32::from_rgb(0x97, 0xa2, 0xb0);
const FAINT: egui::Color32 = egui::Color32::from_rgb(0x5c, 0x66, 0x72);
const RED: egui::Color32 = egui::Color32::from_rgb(0xf0, 0x6a, 0x6a);
const YELLOW: egui::Color32 = egui::Color32::from_rgb(0xf2, 0xc1, 0x4e);

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 660.0])
            .with_min_inner_size([780.0, 500.0])
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

#[derive(PartialEq, Clone, Copy)]
enum Sel {
    None,
    Vanilla,
    Mod(usize),
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Runtime,
    Legacy,
}

struct LauncherApp {
    game_dir: Option<PathBuf>,
    game_dir_input: String,
    mods: Vec<scan::ModEntry>,
    selected: Sel,
    active: install::Active,
    modding_on: bool,
    status: String,
    status_error: bool,
    tab: Tab,
    // Legacy-mode "reduced support" warning state.
    hide_legacy_warning: bool,       // persisted preference
    legacy_warning_open: bool,       // currently showing the warning overlay
    legacy_warning_acked: bool,      // acknowledged for this session already
    dont_show_legacy_again: bool,    // the checkbox in the warning
    // Launcher self-update state.
    launcher_check: Arc<Mutex<update::LauncherCheck>>, // startup version check (worker-filled)
    apply_phase: Arc<Mutex<update::ApplyPhase>>,       // download/apply progress (worker-filled)
    skip_launcher_version: Option<String>, // persisted "don't remind me about this version"
    update_open: bool,                     // the update prompt is showing
    update_acked: bool,                    // dismissed for this session
    update_info: Option<update::LauncherUpdate>, // the offered update, cached from the check
    dont_show_update_again: bool,          // the checkbox in the update prompt
    update_error: Option<String>,          // last apply failure, shown inside the prompt
    // Godot Mod Loader update state (status shown by the runtime tab when modding is enabled).
    modloader_check: Arc<Mutex<modloader::ModLoaderCheck>>, // latest version on GitHub
    ml_update_phase: Arc<Mutex<modloader::UpdatePhase>>,    // "update Mod Loader" download
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

        // Also check the latest Godot Mod Loader version (surfaced on the runtime tab).
        let modloader_check = Arc::new(Mutex::new(modloader::ModLoaderCheck::Checking));
        let ml_update_phase = Arc::new(Mutex::new(modloader::UpdatePhase::Idle));
        modloader::spawn_check(modloader_check.clone(), cc.egui_ctx.clone());

        let mut app = LauncherApp {
            game_dir,
            game_dir_input: String::new(),
            mods: Vec::new(),
            selected: Sel::None,
            active: install::Active::Missing,
            modding_on: false,
            status: String::new(),
            status_error: false,
            tab: Tab::Runtime,
            hide_legacy_warning: cfg.hide_legacy_warning,
            legacy_warning_open: false,
            legacy_warning_acked: false,
            dont_show_legacy_again: cfg.hide_legacy_warning,
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
        app.refresh_mods();
        app.refresh_active();
        app.refresh_modding();
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

    fn refresh_mods(&mut self) {
        self.mods = scan::scan(&scan::mods_dir());
        if let Sel::Mod(i) = self.selected {
            if i >= self.mods.len() {
                self.selected = Sel::None;
            }
        }
    }

    fn refresh_active(&mut self) {
        self.active = match &self.game_dir {
            Some(d) => install::active_state(d),
            None => install::Active::Missing,
        };
    }

    fn refresh_modding(&mut self) {
        self.modding_on = self
            .game_dir
            .as_ref()
            .map(|d| patch::modding_enabled(d))
            .unwrap_or(false);
    }

    /// Switch tabs, popping the legacy warning the first time the Legacy tab is opened
    /// (unless the user asked never to see it again).
    fn switch_tab(&mut self, tab: Tab) {
        self.tab = tab;
        if tab == Tab::Legacy && !self.hide_legacy_warning && !self.legacy_warning_acked {
            self.legacy_warning_open = true;
        }
    }

    fn dismiss_legacy_warning(&mut self) {
        self.legacy_warning_open = false;
        self.legacy_warning_acked = true;
        if self.dont_show_legacy_again != self.hide_legacy_warning {
            self.hide_legacy_warning = self.dont_show_legacy_again;
            config::save_hide_legacy_warning(self.hide_legacy_warning);
        }
    }

    /// Per-frame: surface a pending launcher update as a prompt, and act on any finished
    /// download/apply (relaunch, inform, or report failure).
    fn poll_updates(&mut self, ctx: &egui::Context) {
        // Open the update prompt once, the first time the background check reports one that
        // the user hasn't chosen to skip.
        if !self.update_acked && !self.update_open {
            let snapshot = self.launcher_check.lock().unwrap().clone();
            match snapshot {
                update::LauncherCheck::Available(u) => {
                    if self.skip_launcher_version.as_deref() == Some(u.latest.as_str()) {
                        self.update_acked = true; // already declined this exact version
                    } else {
                        self.dont_show_update_again = false;
                        self.update_info = Some(u);
                        self.update_open = true;
                    }
                }
                // A failed check is non-fatal and silent (no network / rate-limited / no
                // release yet); note it for diagnostics and stop polling this run.
                update::LauncherCheck::Error(e) => {
                    eprintln!("[vcb-launcher] update check failed: {e}");
                    self.update_acked = true;
                }
                update::LauncherCheck::UpToDate => self.update_acked = true,
                update::LauncherCheck::Checking => {}
            }
        }

        // React to a completed apply.
        let phase = self.apply_phase.lock().unwrap().clone();
        match phase {
            update::ApplyPhase::Relaunch(exe) => {
                // The binary was swapped in place — start the new one and quit so it takes over.
                *self.apply_phase.lock().unwrap() = update::ApplyPhase::Idle;
                let _ = std::process::Command::new(&exe).spawn();
                std::process::exit(0);
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
                            self.refresh_active();
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
        let _ = ctx;
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
                    .add(primary_button("Update"))
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
        let has_backup = patch::backup_path(&dir).is_file();
        if !has_backup && !patch::is_patched(&pck) && !install::is_vanilla(&pck) {
            self.set_err("Your current vcb.pck looks like a mod. Revert to vanilla (or Steam → Verify integrity of game files) first, then enable modding.");
            return;
        }
        match self.apply_patch(&dir) {
            Ok(()) => {
                self.refresh_modding();
                self.refresh_active();
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
                self.refresh_active();
                self.set_ok("Modding disabled — restored the original vcb.pck.");
            }
            Err(e) => self.set_err(format!("Couldn't disable modding: {}", e)),
        }
    }

    fn detect_game(&mut self) {
        match steam::find_game_dir() {
            Some(d) => {
                self.game_dir_input = d.display().to_string();
                self.game_dir = Some(d.clone());
                config::save_game_dir(&d);
                self.refresh_active();
                self.refresh_modding();
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
        self.refresh_active();
        self.refresh_modding();
        self.set_ok(format!("Using game folder {} (remembered for next time)", p.display()));
    }

    fn activate(&mut self, idx: usize) {
        let Some(dir) = self.game_dir.clone() else {
            self.set_err("Set the game folder first.");
            return;
        };
        let Some(entry) = self.mods.get(idx) else { return };
        let path = entry.path.clone();
        let is_zip = entry.is_zip;
        let name = entry.display_name();
        let had_backup = install::has_backup(&dir);
        let result = if is_zip {
            install::install_zip(&dir, &path)
        } else {
            install::install(&dir, &path)
        };
        match result {
            Ok(()) => {
                self.refresh_active();
                let note = if !had_backup && !install::has_backup(&dir) {
                    "  (no vanilla backup — the previous vcb.pck was already a mod; verify game files in Steam to get a clean original)"
                } else {
                    ""
                };
                self.set_ok(format!("Activated \"{}\".{}", name, note));
            }
            Err(e) => self.set_err(format!("Couldn't activate \"{}\": {}", name, e)),
        }
    }

    fn restore(&mut self) {
        let Some(dir) = self.game_dir.clone() else {
            self.set_err("Set the game folder first.");
            return;
        };
        match install::restore(&dir) {
            Ok(()) => {
                self.refresh_active();
                self.set_ok("Restored the vanilla game (vcb.pck.original).");
            }
            Err(e) => self.set_err(format!("Couldn't restore: {}", e)),
        }
    }

    fn launch_current(&mut self) {
        let Some(dir) = self.game_dir.clone() else {
            self.set_err("Set the game folder first.");
            return;
        };
        match install::launch_game(&dir) {
            Ok(()) => self.set_ok("Launched the game."),
            Err(e) => self.set_err(format!("Couldn't launch the game: {}", e)),
        }
    }

    fn is_active_mod(&self, idx: usize) -> bool {
        match (&self.active, self.mods.get(idx).and_then(|m| m.fingerprint)) {
            (install::Active::Mod(a), Some(b)) => *a == b,
            _ => false,
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_updates(ctx);

        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(18.0, 14.0)))
            .show(ctx, |ui| self.header_ui(ui));

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(18.0, 9.0)))
            .show(ctx, |ui| self.status_ui(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_CONTENT).inner_margin(egui::Margin::same(18.0)))
            .show(ctx, |ui| {
                match self.tab {
                    Tab::Runtime => self.runtime_tab(ui),
                    Tab::Legacy => self.legacy_tab(ui),
                }
            });

        // The legacy-mode warning renders as a centered modal over everything.
        if self.legacy_warning_open {
            self.legacy_warning_modal(ctx);
        }
        // The self-update prompt (also a centered modal).
        if self.update_open {
            self.update_modal(ctx);
        }
    }
}

impl LauncherApp {
    fn header_ui(&mut self, ui: &mut egui::Ui) {
        let can_revert = self.game_dir.as_ref().map(|d| install::has_backup(d)).unwrap_or(false);
        ui.horizontal(|ui| {
            let (logo_rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
            paint_logo(ui.painter(), logo_rect);
            ui.add_space(6.0);
            ui.label(egui::RichText::new("VCB").size(22.0).strong().color(ACCENT));
            ui.label(egui::RichText::new("Mod Launcher").size(22.0).strong().color(TEXT));

            // Always-available "go back to the unmodded game" action.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = egui::RichText::new("⟲ Revert to vanilla")
                    .color(if can_revert { TEXT } else { FAINT });
                let btn = egui::Button::new(label)
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(1.0, if can_revert { RED } else { FAINT }))
                    .rounding(egui::Rounding::same(7.0));
                let hover = if can_revert {
                    "Restore the original vcb.pck (vcb.pck.original) — undo any active mod"
                } else {
                    "No vanilla backup yet — it's created the first time you activate a legacy mod over a clean install"
                };
                if ui.add_enabled(can_revert, btn).on_hover_text(hover).clicked() {
                    self.restore();
                }
            });
        });

        ui.add_space(12.0);

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
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            ui.add(
                egui::TextEdit::singleline(&mut self.game_dir_input)
                    .hint_text("…/steamapps/common/Virtual Circuit Board")
                    .desired_width(avail - 176.0),
            );
            if ui.button("Use").clicked() {
                self.set_game_from_input();
            }
            if ui.button("Auto-detect").clicked() {
                self.detect_game();
            }
        });

        ui.add_space(12.0);
        self.tab_bar(ui);
    }

    /// The segmented Runtime / Legacy selector.
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(PANEL_2)
            .rounding(egui::Rounding::same(9.0))
            .inner_margin(egui::Margin::same(4.0))
            .show(ui, |ui| {
                let total = ui.available_width();
                let spacing = 4.0;
                let pill_w = ((total - spacing) / 2.0).max(80.0);
                ui.spacing_mut().item_spacing.x = spacing;
                ui.horizontal(|ui| {
                    if tab_pill(ui, pill_w, self.tab == Tab::Runtime, "⚡  Runtime modding", "Mod Loader · recommended") {
                        self.switch_tab(Tab::Runtime);
                    }
                    if tab_pill(ui, pill_w, self.tab == Tab::Legacy, "📦  Legacy", "Whole-.pck swap") {
                        self.switch_tab(Tab::Legacy);
                    }
                });
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

    // ============================ Runtime modding tab ============================
    fn runtime_tab(&mut self, ui: &mut egui::Ui) {
        let has_game = self.game_dir.is_some();

        section_header(ui, "Runtime modding", "Patch once, then load many mods from the game's mods/ folder");
        ui.add_space(10.0);

        // Status + controls card.
        egui::Frame::none()
            .fill(CARD)
            .rounding(egui::Rounding::same(10.0))
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
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

        // How-it-works helper.
        egui::Frame::none()
            .fill(PANEL_2)
            .rounding(egui::Rounding::same(10.0))
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new("How it works").size(13.0).strong().color(TEXT));
                ui.add_space(6.0);
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

    // ================================ Legacy tab ================================
    fn legacy_tab(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Legacy — whole-.pck swap", "Replace the entire vcb.pck with a modded one, one mod at a time");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("⚠").size(12.0).color(YELLOW));
            ui.label(
                egui::RichText::new("Best-effort support. Prefer Runtime modding when a mod offers it.")
                    .size(11.5)
                    .color(YELLOW),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(ghost_button("What's this?")).clicked() {
                    self.legacy_warning_open = true;
                }
            });
        });
        ui.add_space(10.0);

        // Two-column layout: mod list on the left, details on the right.
        ui.horizontal_top(|ui| {
            let list_w = 300.0_f32.min(ui.available_width() * 0.42);
            ui.allocate_ui_with_layout(
                egui::vec2(list_w, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(list_w);
                    self.legacy_mods_list(ui);
                },
            );
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                self.legacy_details(ui);
            });
        });
    }

    fn legacy_mods_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Mods").size(15.0).strong().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⟳").on_hover_text("Rescan the mods folder").clicked() {
                    self.refresh_mods();
                    self.refresh_active();
                    self.set_ok("Rescanned mods.");
                }
                if ui.button("📁").on_hover_text("Open the launcher's mods folder").clicked() {
                    open_path(&scan::mods_dir());
                }
            });
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Vanilla entry.
                let vanilla_active = matches!(self.active, install::Active::Vanilla);
                if card(ui, "Vanilla game", "Original — no mod", matches!(self.selected, Sel::Vanilla), vanilla_active) {
                    self.selected = Sel::Vanilla;
                }

                if self.mods.is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("No mods found.\nDrop mod .pck files — or zipped mods (.zip with a vcb.pck + mod.json) — into the mods folder, then press ⟳.")
                            .size(12.0)
                            .color(DIM),
                    );
                }

                for i in 0..self.mods.len() {
                    let (name, sub) = {
                        let m = &self.mods[i];
                        let ver = if m.meta.version.is_empty() { String::new() } else { format!("v{}", m.meta.version) };
                        let mut tag = if m.has_meta { ver } else { "no metadata".to_string() };
                        if m.is_zip {
                            tag = if tag.is_empty() { "zip".to_string() } else { format!("{}  ·  zip", tag) };
                        }
                        (m.display_name(), tag)
                    };
                    let selected = self.selected == Sel::Mod(i);
                    let is_active = self.is_active_mod(i);
                    if card(ui, &name, &sub, selected, is_active) {
                        self.selected = Sel::Mod(i);
                    }
                }
            });
    }

    fn legacy_details(&mut self, ui: &mut egui::Ui) {
        match self.selected {
            Sel::None => {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Select a mod on the left").size(16.0).color(DIM));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("or pick “Vanilla game” to restore the original.").size(12.0).color(FAINT));
                });
            }
            Sel::Vanilla => self.vanilla_details(ui),
            Sel::Mod(i) => self.mod_details(ui, i),
        }
    }

    fn vanilla_details(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Vanilla game").size(22.0).strong().color(TEXT));
        ui.add_space(4.0);
        ui.label(egui::RichText::new("The unmodified game.").color(DIM));
        ui.add_space(18.0);

        let has_backup = self.game_dir.as_ref().map(|d| install::has_backup(d)).unwrap_or(false);
        let active = matches!(self.active, install::Active::Vanilla);
        ui.horizontal(|ui| {
            if ui.add(primary_button("▶  Launch vanilla")).clicked() {
                if !active {
                    self.restore();
                }
                if !self.status_error {
                    self.launch_current();
                }
            }
            if active {
                ui.add_enabled(false, egui::Button::new("Active ✓"));
            } else if ui.add(ghost_button("Restore vanilla")).clicked() {
                self.restore();
            }
        });
        ui.add_space(8.0);
        if !has_backup {
            ui.label(
                egui::RichText::new("No backup (vcb.pck.original) yet. It's created the first time you activate a mod over a clean install.")
                    .size(12.0)
                    .color(DIM),
            );
        }
    }

    fn mod_details(&mut self, ui: &mut egui::Ui, i: usize) {
        // Copy out what we render so we don't hold a borrow on self.mods across the buttons.
        let (name, ver, author, desc, homepage, has_meta, path, id, game, engine, schema) = {
            let m = &self.mods[i];
            (
                m.display_name(),
                m.meta.version.clone(),
                m.meta.author.clone(),
                m.meta.description.clone(),
                m.meta.homepage.clone(),
                m.has_meta,
                m.path.clone(),
                m.meta.id.clone(),
                m.meta.game.clone(),
                m.meta.engine.clone(),
                m.meta.schema,
            )
        };
        let is_active = self.is_active_mod(i);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&name).size(22.0).strong().color(TEXT));
            if is_active {
                ui.label(egui::RichText::new("  ACTIVE").size(12.0).strong().color(ACCENT));
            }
        });
        ui.add_space(2.0);
        let mut meta_line = Vec::new();
        if !ver.is_empty() {
            meta_line.push(format!("v{}", ver));
        }
        if !author.is_empty() {
            meta_line.push(format!("by {}", author));
        }
        if !meta_line.is_empty() {
            ui.label(egui::RichText::new(meta_line.join("  ·  ")).color(DIM));
        }
        ui.add_space(14.0);

        if !desc.is_empty() {
            ui.label(egui::RichText::new(&desc).size(14.0).color(TEXT));
            ui.add_space(14.0);
        } else if !has_meta {
            ui.label(
                egui::RichText::new("This .pck has no embedded mod.json. Add a res://mod.json (or a sidecar mod.json next to it) to give it a name and description.")
                    .size(12.0)
                    .color(DIM),
            );
            ui.add_space(14.0);
        }

        if !homepage.is_empty() {
            ui.hyperlink_to(egui::RichText::new(&homepage).color(ACCENT), &homepage);
            ui.add_space(14.0);
        }

        // Technical / compatibility line from the metadata.
        let mut tech = Vec::new();
        if !id.is_empty() {
            tech.push(format!("id: {}", id));
        }
        if !game.is_empty() {
            tech.push(game.clone());
        }
        if !engine.is_empty() {
            tech.push(engine.clone());
        }
        if !tech.is_empty() {
            ui.label(egui::RichText::new(tech.join("  ·  ")).size(11.0).color(DIM));
            ui.add_space(6.0);
        }
        if has_meta && schema != 0 && schema != 1 {
            ui.label(
                egui::RichText::new(format!(
                    "⚠ metadata schema v{} is newer than this launcher understands (v1); fields may be missing.",
                    schema
                ))
                .size(11.0)
                .color(RED),
            );
            ui.add_space(6.0);
        }

        ui.separator();
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.add(primary_button("▶  Launch modded")).clicked() {
                self.activate(i);
                if !self.status_error {
                    self.launch_current();
                }
            }
            if is_active {
                ui.add_enabled(false, egui::Button::new("Active ✓"));
            } else if ui.add(ghost_button("Activate only")).clicked() {
                self.activate(i);
            }
            if ui.add(ghost_button("Read metadata")).on_hover_text("Re-parse mod.json from the mod package").clicked() {
                let m = meta::read_any(&path);
                match m {
                    Some(_) => self.set_ok(format!("Read metadata from {}", path.display())),
                    None => self.set_err(format!("No mod.json inside or beside {}", path.display())),
                }
                self.refresh_mods();
            }
        });
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("“Launch modded” installs this mod as vcb.pck and starts the original game exe. “Activate only” just swaps the file (launch from Steam yourself).")
                .size(11.0)
                .color(DIM),
        );
        ui.add_space(6.0);
        ui.label(egui::RichText::new(path.display().to_string()).size(11.0).color(FAINT));
    }

    // ============================ Legacy warning modal ============================
    fn legacy_warning_modal(&mut self, ctx: &egui::Context) {
        // Dim the whole window behind the dialog.
        let screen = ctx.screen_rect();
        egui::Area::new("legacy_warn_dim".into())
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(rect, egui::Rounding::ZERO, egui::Color32::from_black_alpha(150));
                // Swallow clicks so the content behind stays inert.
                let _ = resp;
            });

        egui::Area::new("legacy_warning".into())
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
              egui::Frame::none()
                    .fill(PANEL)
                    .rounding(egui::Rounding::same(12.0))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x2a, 0x33, 0x3d)))
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
                    ui.label(egui::RichText::new("⚠").size(20.0).color(YELLOW));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("You're opening Legacy mode").size(17.0).strong().color(TEXT));
                });
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(
                        "Legacy mode is the launcher's original approach: it swaps the game's entire \
                         vcb.pck for a modded one, one mod at a time, and keeps a backup of your \
                         original so you can always go back.",
                    )
                    .size(13.0)
                    .color(DIM),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "It still works, but it's no longer the recommended path and gets less attention \
                         going forward. Runtime modding (the other tab) is better supported — it patches \
                         the game once with the Godot Mod Loader so you can run several mods as drop-in \
                         .zip files without ever replacing the game's files.",
                    )
                    .size(13.0)
                    .color(DIM),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Use Legacy mode mainly for whole-game mods that ship their own vcb.pck.")
                        .size(12.0)
                        .color(FAINT),
                );

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.dont_show_legacy_again, "");
                    ui.label(egui::RichText::new("Don't show this again").size(12.0).color(DIM));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(primary_button("Continue to Legacy")).clicked() {
                            self.dismiss_legacy_warning();
                        }
                        if ui.add(ghost_button("Back to Runtime")).clicked() {
                            self.legacy_warning_open = false;
                            self.tab = Tab::Runtime;
                        }
                    });
                });
              });
            });
    }

    // ============================ Self-update prompt ============================
    fn update_modal(&mut self, ctx: &egui::Context) {
        let Some(info) = self.update_info.clone() else {
            self.update_open = false;
            return;
        };
        let working = matches!(*self.apply_phase.lock().unwrap(), update::ApplyPhase::Working);
        let has_asset = info.asset.is_some();

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
                    .rounding(egui::Rounding::same(12.0))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x2a, 0x33, 0x3d)))
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
                if has_asset {
                    ui.label(
                        egui::RichText::new(if cfg!(target_os = "macos") {
                            "Update now downloads the new build and reveals it in Finder — unzip it and replace the app to finish."
                        } else {
                            "Update now downloads the new build, swaps it in, and relaunches the launcher."
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
              });
            });
    }
}

// --- widgets --------------------------------------------------------------------------

/// One tab in the segmented control. Fixed width so both tabs are equal.
fn tab_pill(ui: &mut egui::Ui, width: f32, selected: bool, title: &str, subtitle: &str) -> bool {
    let fill = if selected { CARD } else { egui::Color32::TRANSPARENT };
    let resp = egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(7.0))
        .inner_margin(egui::Margin::symmetric(12.0, 7.0))
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(13.5)
                        .strong()
                        .color(if selected { ACCENT } else { DIM }),
                );
                ui.label(
                    egui::RichText::new(subtitle)
                        .size(10.5)
                        .color(if selected { DIM } else { FAINT }),
                );
            });
        })
        .response
        .interact(egui::Sense::click());
    if resp.hovered() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// A clickable mod card. Returns true when clicked.
fn card(ui: &mut egui::Ui, title: &str, subtitle: &str, selected: bool, active: bool) -> bool {
    let fill = if selected { CARD_SEL } else { CARD };
    let stroke = if selected {
        egui::Stroke::new(1.0, ACCENT)
    } else {
        egui::Stroke::NONE
    };
    let mut resp = egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(8.0))
        .stroke(stroke)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .outer_margin(egui::Margin { top: 0.0, bottom: 6.0, left: 0.0, right: 0.0 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).size(15.0).strong().color(TEXT));
                    ui.label(egui::RichText::new(subtitle).size(11.0).color(DIM));
                });
                if active {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("●").color(ACCENT).size(14.0))
                            .on_hover_text("Currently installed");
                    });
                }
            });
        })
        .response
        .interact(egui::Sense::click());
    resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    // Subtle hover highlight for unselected cards.
    if resp.hovered() && !selected {
        ui.painter().rect_filled(
            resp.rect,
            egui::Rounding::same(8.0),
            egui::Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 8),
        );
    }
    resp.clicked()
}

/// Accent-filled primary button.
fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(egui::Color32::BLACK).strong())
        .fill(ACCENT)
        .stroke(egui::Stroke::new(1.0, ACCENT_DK))
        .rounding(egui::Rounding::same(7.0))
}

/// Neutral outlined secondary button.
fn ghost_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(TEXT))
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0x2c, 0x35, 0x40)))
        .rounding(egui::Rounding::same(7.0))
}

/// Outlined danger button.
fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(RED))
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, RED))
        .rounding(egui::Rounding::same(7.0))
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
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x0c, 0x0f, 0x13);
    visuals.faint_bg_color = CARD;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.inactive.rounding = egui::Rounding::same(7.0);
    visuals.widgets.hovered.rounding = egui::Rounding::same(7.0);
    visuals.widgets.active.rounding = egui::Rounding::same(7.0);
    visuals.widgets.inactive.bg_fill = CARD;
    visuals.widgets.hovered.bg_fill = CARD_HOVER;
    visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
    visuals.widgets.inactive.weak_bg_fill = CARD;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(13.0, 8.0);
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
