// VCB Mod Launcher — a small, portable GUI that swaps mod `.pck` files in and out of a
// Steam install of Virtual Circuit Board. See README.md.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod archive;
mod config;
mod icon;
mod install;
mod meta;
mod patch;
mod pck;
mod pckbuild;
mod projbin;
mod scan;
mod steam;

use eframe::egui;
use std::path::{Path, PathBuf};

// --- palette --------------------------------------------------------------------------
const BG: egui::Color32 = egui::Color32::from_rgb(0x14, 0x18, 0x1d);
const PANEL: egui::Color32 = egui::Color32::from_rgb(0x1b, 0x21, 0x27);
const CARD: egui::Color32 = egui::Color32::from_rgb(0x22, 0x29, 0x31);
const CARD_SEL: egui::Color32 = egui::Color32::from_rgb(0x14, 0x33, 0x2c);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3b, 0xd1, 0x9e);
const TEXT: egui::Color32 = egui::Color32::from_rgb(0xe6, 0xe9, 0xee);
const DIM: egui::Color32 = egui::Color32::from_rgb(0x93, 0x9e, 0xab);
const RED: egui::Color32 = egui::Color32::from_rgb(0xf0, 0x6a, 0x6a);

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 640.0])
            .with_min_inner_size([760.0, 480.0])
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

struct LauncherApp {
    game_dir: Option<PathBuf>,
    game_dir_input: String,
    mods: Vec<scan::ModEntry>,
    selected: Sel,
    active: install::Active,
    modding_on: bool,
    status: String,
    status_error: bool,
}

impl LauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);

        // Prefer the folder the user set last time; only fall back to auto-detection.
        let remembered = config::load()
            .game_dir
            .map(PathBuf::from)
            .filter(|p| steam::is_game_dir(p));
        let from_config = remembered.is_some();
        let game_dir = remembered.or_else(steam::find_game_dir);

        let mut app = LauncherApp {
            game_dir,
            game_dir_input: String::new(),
            mods: Vec::new(),
            selected: Sel::None,
            active: install::Active::Missing,
            modding_on: false,
            status: String::new(),
            status_error: false,
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
        match patch::enable_modding(&dir) {
            Ok(()) => {
                self.refresh_modding();
                self.refresh_active();
                self.set_ok("Modding enabled — vcb.pck patched with the Godot Mod Loader. Drop Mod Loader mods (.zip) into the game's mods/ folder.");
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
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(18.0, 14.0)))
            .show(ctx, |ui| self.header_ui(ui));

        egui::TopBottomPanel::top("modding")
            .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(18.0, 10.0)))
            .show(ctx, |ui| self.modding_ui(ui));

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(18.0, 8.0)))
            .show(ctx, |ui| self.status_ui(ui));

        egui::SidePanel::left("mods")
            .resizable(false)
            .exact_width(320.0)
            .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(12.0)))
            .show(ctx, |ui| self.mods_ui(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::same(18.0)))
            .show(ctx, |ui| self.details_ui(ui));
    }
}

impl LauncherApp {
    fn header_ui(&mut self, ui: &mut egui::Ui) {
        let can_revert = self.game_dir.as_ref().map(|d| install::has_backup(d)).unwrap_or(false);
        ui.horizontal(|ui| {
            let (logo_rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
            paint_logo(ui.painter(), logo_rect);
            ui.add_space(4.0);
            ui.label(egui::RichText::new("VCB").size(22.0).strong().color(ACCENT));
            ui.label(egui::RichText::new("Mod Launcher").size(22.0).strong().color(TEXT));

            // Always-available "go back to the unmodded game" action.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = egui::RichText::new("⟲ Revert to vanilla")
                    .color(if can_revert { TEXT } else { DIM });
                let btn = egui::Button::new(label)
                    .stroke(egui::Stroke::new(1.0, if can_revert { RED } else { DIM }))
                    .rounding(egui::Rounding::same(6.0));
                let hover = if can_revert {
                    "Restore the original vcb.pck (vcb.pck.original) — undo any active mod"
                } else {
                    "No vanilla backup yet — it's created the first time you activate a mod over a clean install"
                };
                if ui.add_enabled(can_revert, btn).on_hover_text(hover).clicked() {
                    self.restore();
                }
            });
        });
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("Swap a mod's vcb.pck into your game install and launch it. Your original is backed up automatically. One mod at a time (combining mods is planned).")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Game folder").color(DIM));
            let found = self.game_dir.as_ref().map(|d| steam::is_game_dir(d)).unwrap_or(false);
            if found {
                ui.label(egui::RichText::new("●").color(ACCENT));
            } else {
                ui.label(egui::RichText::new("●").color(RED));
            }
        });
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            ui.add(
                egui::TextEdit::singleline(&mut self.game_dir_input)
                    .hint_text("…/steamapps/common/Virtual Circuit Board")
                    .desired_width(avail - 170.0),
            );
            if ui.button("Use").clicked() {
                self.set_game_from_input();
            }
            if ui.button("Auto-detect").clicked() {
                self.detect_game();
            }
        });
    }

    fn modding_ui(&mut self, ui: &mut egui::Ui) {
        let has_game = self.game_dir.is_some();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Runtime modding").size(14.0).strong().color(TEXT));
            if self.modding_on {
                ui.label(egui::RichText::new("● enabled").size(12.0).color(ACCENT))
                    .on_hover_text("vcb.pck is patched with the Godot Mod Loader");
            } else {
                ui.label(egui::RichText::new("● disabled").size(12.0).color(DIM));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.modding_on {
                    if ui
                        .button("📁 Mods folder")
                        .on_hover_text("Open the game's mods/ folder — drop Mod Loader mods (.zip) here")
                        .clicked()
                    {
                        if let Some(d) = self.game_dir.clone() {
                            open_path(&patch::mods_dir(&d));
                        }
                    }
                    if ui
                        .button("Re-apply")
                        .on_hover_text("Re-patch from the pristine original (e.g. after a Steam game update)")
                        .clicked()
                    {
                        self.enable_modding();
                    }
                    if ui
                        .button(egui::RichText::new("Disable").color(RED))
                        .on_hover_text("Restore the original vcb.pck")
                        .clicked()
                    {
                        self.disable_modding();
                    }
                } else if ui
                    .add_enabled(has_game, accent_widget("Enable modding"))
                    .on_hover_text("Patch vcb.pck with the Godot Mod Loader so it can load mods at runtime (keeps a pristine backup)")
                    .clicked()
                {
                    self.enable_modding();
                }
            });
        });
        ui.label(
            egui::RichText::new("Patches vcb.pck once with the Godot Mod Loader (the original is kept safe). Mods are Mod Loader packages (.zip) dropped into the game's mods/ folder — see docs/MODDING.md.")
                .size(11.0)
                .color(DIM),
        );
    }

    fn status_ui(&mut self, ui: &mut egui::Ui) {
        let color = if self.status_error { RED } else { DIM };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&self.status).size(12.0).color(color));
        });
    }

    fn mods_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Mods").size(16.0).strong().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⟳").on_hover_text("Rescan the mods folder").clicked() {
                    self.refresh_mods();
                    self.refresh_active();
                    self.set_ok("Rescanned mods.");
                }
                if ui.button("📁").on_hover_text("Open the mods folder").clicked() {
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

    fn details_ui(&mut self, ui: &mut egui::Ui) {
        match self.selected {
            Sel::None => {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Select a mod on the left").size(16.0).color(DIM));
                });
            }
            Sel::Vanilla => self.vanilla_details(ui),
            Sel::Mod(i) => self.mod_details(ui, i),
        }
    }

    fn vanilla_details(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Vanilla game").size(24.0).strong().color(TEXT));
        ui.add_space(4.0);
        ui.label(egui::RichText::new("The unmodified game.").color(DIM));
        ui.add_space(18.0);

        let has_backup = self.game_dir.as_ref().map(|d| install::has_backup(d)).unwrap_or(false);
        let active = matches!(self.active, install::Active::Vanilla);
        ui.horizontal(|ui| {
            if accent_button(ui, "▶ Launch vanilla").clicked() {
                if !active {
                    self.restore();
                }
                if !self.status_error {
                    self.launch_current();
                }
            }
            if active {
                ui.add_enabled(false, egui::Button::new("Active ✓"));
            } else if ui.button("Restore vanilla").clicked() {
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
            ui.label(egui::RichText::new(&name).size(24.0).strong().color(TEXT));
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
            if accent_button(ui, "▶ Launch modded").clicked() {
                self.activate(i);
                if !self.status_error {
                    self.launch_current();
                }
            }
            if is_active {
                ui.add_enabled(false, egui::Button::new("Active ✓"));
            } else if ui.button("Activate only").clicked() {
                self.activate(i);
            }
            if ui.button("Read metadata").on_hover_text("Re-parse mod.json from the mod package").clicked() {
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
        ui.label(egui::RichText::new(path.display().to_string()).size(11.0).color(DIM));
    }
}

// --- widgets --------------------------------------------------------------------------

/// A clickable mod card. Returns true when clicked.
fn card(ui: &mut egui::Ui, title: &str, subtitle: &str, selected: bool, active: bool) -> bool {
    let fill = if selected { CARD_SEL } else { CARD };
    let stroke = if selected {
        egui::Stroke::new(1.0, ACCENT)
    } else {
        egui::Stroke::NONE
    };
    let resp = egui::Frame::none()
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

    if resp.hovered() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn accent_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(accent_widget(text))
}

/// The accent-filled button as a standalone widget (for `add_enabled`).
fn accent_widget(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(egui::Color32::BLACK).strong())
        .fill(ACCENT)
        .rounding(egui::Rounding::same(6.0))
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
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x0f, 0x12, 0x16);
    visuals.faint_bg_color = CARD;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);
    visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);
    visuals.widgets.active.rounding = egui::Rounding::same(6.0);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
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
