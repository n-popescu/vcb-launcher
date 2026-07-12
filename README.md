# vcb-launcher

A small, **portable GUI mod launcher** for [Virtual Circuit Board](https://store.steampowered.com/app/367020/).
It swaps a mod's `vcb.pck` into your Steam install (keeping the game's executable
untouched) and keeps a one-time backup of your original so you can always go back.

Single self-contained executable — no installer, no runtime, no Python. Windows + Linux.

<!-- A screenshot can go here once the UI is built for your platform. -->

## What it does

- **Auto-detects** your Steam copy of the game (scans every Steam library folder for the
  one holding `vcb.pck` / the `vcb` executable). You can also point it at the folder
  manually.
- Lists the mods you've dropped into a `mods/` folder next to the launcher.
- **Reads each mod's metadata** (name, version, author, description) from a `mod.json`
  packed *inside* the `.pck`, so mods are identifiable even though they all install under
  the single `vcb.pck` name. A sidecar `mod.json` next to the `.pck` also works.
- **Activate** a mod → backs up the original `vcb.pck` once (to `vcb.pck.original`) and
  copies the mod over `vcb.pck`.
- **Revert to vanilla** → an always-visible button (top-right) puts the backup back so
  you're one click away from the unmodded game at any time. (Selecting **Vanilla game**
  and pressing **Restore vanilla** does the same thing.)
- **Zipped mods** → drop a `.zip` bundling a `vcb.pck` + `mod.json` into `mods/` and the
  launcher reads it and installs it just like a loose `.pck`.
- **Remembers the game folder** → the folder you set (or auto-detect) is saved to
  `launcher_config.json` next to the launcher, so it's already filled in next launch.

## Using it

1. Put the launcher executable anywhere. On first run it creates a `mods/` folder next to
   itself.
2. Drop mod packages into `mods/`. Each mod is a Godot `.pck`. Because every installed mod
   is named `vcb.pck`, you can keep them apart however you like:
   - one `.pck` per subfolder — `mods/multiplayer/vcb.pck`, `mods/traces/vcb.pck`, … , or
   - distinctly-named files — `mods/multiplayer.pck`, `mods/traces.pck`, … , or
   - a **zipped mod** — `mods/multiplayer.zip` containing a `vcb.pck` and a `mod.json`.
   The launcher scans `mods/` recursively and identifies each by its embedded metadata.
3. Launch it. On first run it auto-detects the game; after that it reuses the folder you
   last used (remembered in `launcher_config.json`). If it can't find it, paste the game
   folder path up top and press **Use**.
4. Pick a mod on the left and press **▶ Launch modded** — the launcher copies it in as
   `vcb.pck` (backing up your original first) and starts the game. **Activate only** just
   swaps the file if you'd rather launch from Steam. Select **Vanilla game** to
   **Restore vanilla** or **▶ Launch vanilla**.

> **One mod at a time.** Activating a mod replaces `vcb.pck`, so exactly one mod is live.
> Combining mods needs a mod-loader (planned) and more mods to test with.

> The launcher always runs the **original game executable** (the one with the correct,
> closed-source simulation engine) — it only changes which `vcb.pck` sits next to it. Every
> mod is expected to target that original exe.

> The first activation over a **clean** install snapshots your original `vcb.pck` to
> `vcb.pck.original`. If your `vcb.pck` was *already* a mod the first time you use the
> launcher, there's no clean original to back up — use Steam's *Verify integrity of game
> files* to get one, then activate.

## Mod metadata

Mods are identified by a `mod.json` the launcher reads in this order:

1. embedded inside the `.pck` at `res://mod.json` (preferred), else
2. a sidecar `mod.json` in the same folder as the `.pck`.

```jsonc
{
  "schema": 1,
  "id": "multiplayer",
  "name": "VCB Multiplayer",
  "version": "1.1.0",
  "author": "n-popescu",
  "description": "…",
  "game": "Virtual Circuit Board",
  "engine": "Godot 3.5.1",
  "homepage": "https://github.com/n-popescu/vcb-mp"
}
```

The [`vcb-mp`](https://github.com/n-popescu/vcb-mp) mod ships this file
(see its `MOD_METADATA.md` for how to make sure it's packed into the exported `vcb.pck`).

## Download

CI builds **Linux**, **Windows**, and **macOS** (universal Intel + Apple Silicon) binaries
automatically on every commit — grab them from the run's **Artifacts** on the Actions tab.
Pushing a `v*` tag additionally publishes them to a GitHub **Release**. Or build from source
below.

## Build from source

Requires a [Rust toolchain](https://rustup.rs).

```bash
cargo build --release
# binary at target/release/vcb-launcher[.exe]
```

On Linux, building needs the usual X11/OpenGL dev packages (egui/winit); on Debian/Ubuntu:

```bash
sudo apt-get install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
                        libxkbcommon-dev libgl1-mesa-dev
```

## How it works

- `src/pck.rs` — a tiny reader for the Godot `.pck` format; extracts `res://mod.json`
  from a file or from bytes (for `.pck`s inside a zip).
- `src/archive.rs` — zipped-mod support: reads metadata from a `.zip` and extracts its
  bundled `.pck` on activation.
- `src/meta.rs` — the `mod.json` schema + embedded/sidecar/zip lookup.
- `src/steam.rs` — Steam library discovery (Windows registry + common paths; Linux
  native + Flatpak) and game-folder detection.
- `src/config.rs` — persists the chosen game folder (`launcher_config.json`).
- `src/install.rs` — backup / restore / install (`.pck` and `.zip`) and "which mod is
  active" detection.
- `src/scan.rs` — finds `.pck`s and zipped mods under `mods/` and reads their metadata.
- `src/icon_render.rs` — a dependency-free rasteriser for the procedurally-drawn icon
  (the "circuit chip with a lit via" motif), size-parametric so one source renders every
  resolution.
- `src/icon.rs` — wraps `icon_render` as the runtime egui window/taskbar icon.
- `build.rs` + `src/bin/gen_icons.rs` — bake that same icon **into the executable at build
  time** so it shows on the app before it's launched: `build.rs` embeds a multi-resolution
  `.ico` as the Windows `.exe` icon; CI wraps the macOS binary in a `.app` with a generated
  `.icns` and ships a `.desktop` + `.png` on Linux (a bare ELF can't embed an icon). Still
  no image file committed — it's all generated from `icon_render`.
- `src/main.rs` — the [egui](https://github.com/emilk/egui) UI.

Built with Rust + egui/eframe, so the whole app is one portable binary with no external
runtime.

## License

MIT — see [LICENSE](LICENSE).
