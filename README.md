# vcb-launcher

A small, **portable GUI mod launcher** for [Virtual Circuit Board](https://store.steampowered.com/app/367020/).

It supports two modding models, each on its own tab:

- **Runtime modding (recommended).** Patch your `vcb.pck` **once** with the
  [Godot Mod Loader](https://godotengine.org/asset-library/asset/1938) so the game can load
  many mods at runtime from a `mods/` folder — the original game files are never replaced.
  A **▶ Launch game** button starts the (patched) game with every mod loaded. See
  **[docs/MODDING.md](docs/MODDING.md)** and **[docs/PATCHER_DESIGN.md](docs/PATCHER_DESIGN.md)**.
- **Legacy — whole‑pck swap.** Swap a mod's `vcb.pck` into your install (one mod at a time),
  keeping a one‑time backup of your original so you can always go back. Lives under the
  **Legacy** tab; opening it once shows a short heads‑up that it's the older, best‑effort
  path (dismissible with *Don't show again*).

Single self-contained executable — no installer, no runtime, no Python. Windows + Linux.

<!-- A screenshot can go here once the UI is built for your platform. -->

## Runtime modding (patch + Mod Loader)

On the **Runtime modding** tab, click **Enable modding** and the launcher snapshots your
pristine `vcb.pck` to `vcb.pck.original`, then writes a patched `vcb.pck` with the Godot
Mod Loader baked in (original game files copied verbatim — no decryption key needed). It
also installs a small built-in **mod list** mod, so **Options ▸ Mods** in‑game shows every
installed mod. Drop Mod Loader mods (`.zip`) into the game's `mods/` folder (**📁 Mods
folder**) and press **▶ Launch game** — the Mod Loader loads every mod at startup.
**Disable** restores the original; **Re‑apply** re‑patches after a Steam update. Full
player + mod‑author guide: **[docs/MODDING.md](docs/MODDING.md)**.

## Legacy — whole‑pck swap

Everything below lives under the **Legacy** tab. It's the launcher's original model; a
one‑time notice explains that it's now best‑effort and points you at runtime modding for
anything that supports it.

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

The launcher opens on the **Runtime modding** tab (the recommended path — see above and
[docs/MODDING.md](docs/MODDING.md)). The steps below are the **Legacy — whole‑pck swap**
flow, which lives on the **Legacy** tab (opening it the first time shows a one‑time notice
that it's the older, best‑effort model).

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
4. Open the **Legacy** tab, pick a mod on the left and press **▶ Launch modded** — the
   launcher copies it in as `vcb.pck` (backing up your original first) and starts the game.
   **Activate only** just swaps the file if you'd rather launch from Steam. Select **Vanilla
   game** to **Restore vanilla** or **▶ Launch vanilla**.

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

## Staying up to date

On startup the launcher quietly checks its own GitHub **Releases** for a newer version (in a
background thread — the window opens instantly, and if you're offline or there's no release
yet, nothing happens). When a newer version exists it shows a small prompt:

- **Update now** downloads the build for your platform. On **Windows/Linux** (single binary)
  it swaps the running executable in place and relaunches; on **macOS** it saves the `.app`
  zip next to the app and reveals it in Finder for you to unzip and replace.
- **Cancel** dismisses it for this run. Since the check happens at every startup, you'll be
  reminded again next time you open the launcher.
- **Don't show again until the next version** stops the prompt for this version only — it
  returns automatically once an even newer release is out. (Stored in `launcher_config.json`.)

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
- `src/pckbuild.rs` — reads a full Godot 3.x pck directory and **writes** a new one (used to
  patch `vcb.pck`, copying original files verbatim).
- `src/projbin.rs` — reads and patches the embedded `project.binary` (adds the Mod Loader
  autoloads + merges its `class_name` globals) via a minimal Variant codec.
- `src/patch.rs` — orchestrates runtime modding: snapshot → inject the (embedded) Mod
  Loader → repack; enable/disable/re‑apply. The Mod Loader is vendored under
  `vendor/godot-mod-loader/` and embedded by `build.rs`.
- `src/bundled.rs` — writes the bundled **Mod Menu** (`Options ▸ Mods`) into the game's
  `mods/` folder on enable. Its files are vendored under `vendor/mod-menu/` and embedded by
  `build.rs`.
- `src/archive.rs` — zipped-mod support: reads metadata from a `.zip` and extracts its
  bundled `.pck` on activation.
- `src/meta.rs` — the `mod.json` schema + embedded/sidecar/zip lookup.
- `src/steam.rs` — Steam library discovery (Windows registry + common paths; Linux
  native + Flatpak) and game-folder detection.
- `src/config.rs` — persists the chosen game folder, the legacy-mode warning preference, and
  the skipped-update version (`launcher_config.json`).
- `src/net.rs` — a tiny blocking HTTPS client (`ureq` + rustls) used by the update checker.
- `src/update.rs` — the self-updater: checks GitHub Releases for a newer launcher, compares
  versions, and downloads + swaps in the right per-platform artifact (with unit tests for the
  version compare and asset selection).
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
