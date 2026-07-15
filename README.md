# vcb-launcher

A small, **portable GUI mod launcher** for [Virtual Circuit Board](https://store.steampowered.com/app/367020/).

It patches your `vcb.pck` **once** with the
[Godot Mod Loader](https://godotengine.org/asset-library/asset/1938) so the game can load many
mods at runtime from a `mods/` folder — the original game files are never replaced. A **▶ Launch
game** button starts the (patched) game with every mod loaded. See
**[docs/MODDING.md](docs/MODDING.md)** and **[docs/PATCHER_DESIGN.md](docs/PATCHER_DESIGN.md)**.

Single self-contained executable — no installer, no runtime, no Python. Windows + Linux + macOS.

<!-- A screenshot can go here once the UI is built for your platform. -->

## Runtime modding (patch + Mod Loader)

Click **Enable modding** and the launcher snapshots your pristine `vcb.pck` to
`vcb.pck.original`, then writes a patched `vcb.pck` with the Godot Mod Loader baked in (original
game files copied verbatim — no decryption key needed). It also installs a small **mod list**
mod (fetched from the [`vcb-modmenu`](https://github.com/n-popescu/vcb-modmenu) repo's latest
release), so **Options ▸ Mods** in‑game shows every installed mod. Drop Mod Loader mods (`.zip`)
into the game's `mods/` folder (**📁 Mods folder**) and press **▶ Launch game** — the Mod Loader
loads every mod at startup.

**Disable** restores the original; **Re‑apply** re‑patches after a Steam update. Full player +
mod‑author guide: **[docs/MODDING.md](docs/MODDING.md)**.

The top‑right button toggles the launcher's look between **Classic** (the calm dark theme) and
**Liquid Glass** (luminous, translucent frosted panels over a soft, gently drifting gradient). Your
choice is remembered between runs. Both themes are lightly animated — the logo "breathes", picking a
mod fades its details in, list rows glow on hover with an accent bar marking the selection, update
badges pulse, and dialogs fade in.

Beside those controls the launcher shows a **Mod Loader** status: *up to date* or *out of
date*, checked against the [Godot Mod Loader](https://github.com/GodotModding/godot-mod-loader)
releases on startup. The launcher ships with a built‑in Mod Loader as an offline seed, but it
isn't stuck on that version — when a newer release exists an **Update** button appears that
downloads it into a `modloader/` cache and re‑applies the patch, so `vcb.pck` is always baked
with the newest Mod Loader without waiting for a launcher update.

### The game folder

On first run the launcher **auto‑detects** your Steam copy of the game (it scans every Steam
library folder for the one holding `vcb.pck` / the `vcb` executable). You can also paste the
folder path up top and press **Use**, or press **Auto‑detect**. The folder you set is
remembered for next time (see [Settings](#settings)).

> The launcher always runs the **original game executable** (the one with the correct,
> closed‑source simulation engine) — it only changes which `vcb.pck` sits next to it.

> The first time you enable modding over a **clean** install, the launcher snapshots your
> original `vcb.pck` to `vcb.pck.original`. If your `vcb.pck` was *already* a mod, there's no
> clean original to back up — use Steam's *Verify integrity of game files* to get one, then
> enable modding.

### Updates & installed mods

An **Updates** card shows the launcher version and a **Check for updates** button. It checks
GitHub for a newer launcher *and* for newer versions of every mod in your game folder in one
click. When a launcher update is available it downloads the new build, swaps it in, and offers
**Restart now** (the update also applies the next time you open the launcher).

The **Mods in your game folder** box is a master/detail view, like the in-game Mod Menu: a
clickable **list on the left** (each mod's name, version, and an update indicator), and the
selected mod's **full details on the right** — its description, repository link, and a single-mod
**Update** button beside the version when a newer release exists. **Update all** (top of the box)
updates every out-of-date mod in one go; **Check for updates** re-checks everything. Each mod is
checked against its repo's latest release (from the manifest's `website_url`); mods whose manifest
has no GitHub `website_url` are listed but can't be auto-updated.

### Auto re-apply after an update

The launcher keeps `vcb.pck` patched for you automatically:

- After it **updates the Godot Mod Loader** (the **Update** button by the Mod Loader status), it
  re-applies the patch from your pristine backup so the new Mod Loader is baked in immediately.
- On the **first launch of a newly-updated launcher**, if modding is enabled it re-applies the
  patch once, so a new build's Mod Loader seed / patch improvements land without you pressing
  **Re-apply**.

## Settings

The launcher stores its tiny settings file (`launcher_config.json` — the chosen game folder and
your skipped‑update version) in the OS's **per‑user config directory**, so it **survives an app
update**:

| OS | Location |
|---|---|
| Windows | `%APPDATA%\vcb-launcher\launcher_config.json` |
| macOS | `~/Library/Application Support/vcb-launcher/launcher_config.json` |
| Linux | `~/.config/vcb-launcher/launcher_config.json` |

Older builds kept this file next to the executable, which on macOS is wiped when you replace the
`.app` on update. The launcher **migrates** a next‑to‑the‑exe `launcher_config.json` into the new
location automatically the first time it runs, so upgrading keeps your saved game folder.

## Staying up to date

On startup the launcher quietly checks its own GitHub **Releases** for a newer version (in a
background thread — the window opens instantly, and if you're offline or there's no release
yet, nothing happens). You can also check any time with **Check for updates** in the Updates
card. When a newer version exists it shows a small prompt:

- **Update now** downloads the build for your platform. On **Windows/Linux** (single binary)
  it swaps the running executable in place, then offers **Restart now** — the update also
  applies automatically the next time you open the launcher. On **macOS** it saves the `.app`
  zip next to the app and reveals it in Finder for you to unzip and replace.
- **Cancel** dismisses it for this run. Since the check happens at every startup, you'll be
  reminded again next time you open the launcher.
- **Don't show again until the next version** stops the prompt for this version only — it
  returns automatically once an even newer release is out.

The same **Check for updates** button also re‑checks every mod installed in your game folder;
see [Updates & installed mods](#updates--installed-mods) above.

## Download

CI builds **Linux**, **Windows**, and **macOS** (universal Intel + Apple Silicon) binaries
automatically on every commit — grab them from the run's **Artifacts** on the Actions tab.
Releases are **published automatically**: bump `version` in `Cargo.toml` and merge to `main`
and CI tags `v<version>` and cuts a GitHub **Release** (built binaries + auto-generated notes);
pushing a `v*` tag by hand does the same. Merging without a version bump re-releases nothing.
This is what the in-app self-updater checks against. Or build from source below.

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

- `src/pck.rs` — a tiny reader for the Godot `.pck` format; extracts a packed file (e.g.
  `res://mod.json`) by its `res://` path.
- `src/pckbuild.rs` — reads a full Godot 3.x pck directory and **writes** a new one (used to
  patch `vcb.pck`, copying original files verbatim).
- `src/projbin.rs` — reads and patches the embedded `project.binary` (adds the Mod Loader
  autoloads + merges its `class_name` globals) via a minimal Variant codec.
- `src/patch.rs` — orchestrates runtime modding: snapshot → inject a Mod Loader addon file
  set → repack; enable/disable/re‑apply. The addon set is source‑agnostic
  (`enable_modding_with`): the embedded seed by default, or a web‑downloaded copy. Also reads
  the Mod Loader version baked into a pck (`applied_version`).
- `src/modloader.rs` — the web‑updatable Mod Loader: a `modloader/` cache next to the
  launcher, GitHub latest‑release lookup + zipball download/extract, and version parsing
  (`const MODLOADER_VERSION`). The launcher patches from the cached copy when present, else
  the seed embedded by `build.rs`.
- `src/modmenu.rs` — the web‑updatable **Mod Menu** (`Options ▸ Mods`): a `modmenu/` cache
  next to the launcher, GitHub latest‑release lookup + `npopescu-ModMenu.zip` download, and
  install into the game's `mods/` folder on enable/Re‑apply. It's fetched from its own repo
  ([`vcb-modmenu`](https://github.com/n-popescu/vcb-modmenu)) at runtime — no longer vendored —
  so it always picks up the latest upstream release. Best‑effort: an offline first run with no
  cached copy simply skips it.
- `src/gamemods.rs` — the **installed‑mods** list: scans the game's `mods/` folder for Mod
  Loader `.zip` packages, reads each manifest (name / `version_number` / `website_url`),
  derives its GitHub repo, checks that repo's latest release, and downloads + swaps the zip in
  place to update it (with unit tests for the manifest/repo parsing, asset selection, and scan).
- `src/steam.rs` — Steam library discovery (Windows registry + common paths; Linux
  native + Flatpak) and game-folder detection.
- `src/config.rs` — persists the chosen game folder, the skipped‑update version, and the
  last‑run launcher version (for the first‑boot auto re‑apply) in the OS's per‑user config
  directory (see [Settings](#settings)), migrating a legacy next‑to‑the‑exe file.
- `src/net.rs` — a tiny blocking HTTPS client (`ureq` + rustls) used by the update checker.
- `src/update.rs` — the self-updater: checks GitHub Releases for a newer launcher, compares
  versions, and downloads + swaps in the right per-platform artifact (with unit tests for the
  version compare and asset selection).
- `src/launch.rs` — launches the original game executable (Windows `vcb.exe`, Linux
  `vcb.x86_64`, or via Wine on macOS/Linux, resolving Wine on the `.app`'s minimal PATH).
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
