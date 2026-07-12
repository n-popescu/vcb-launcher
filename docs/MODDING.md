# Modding Virtual Circuit Board

The launcher can turn a stock **Virtual Circuit Board** install into a **mod‑loading**
one by patching its `vcb.pck` a single time with the
[Godot Mod Loader](https://godotengine.org/asset-library/asset/1938)
(GodotModding, [asset 1938](https://godotengine.org/asset-library/asset/1938), CC0). After
that, mods are ordinary Mod Loader packages (`.zip`) you drop into the game's `mods/`
folder — **the game files are never replaced**, so several mods can be installed side by
side and the original stays intact.

> This is different from the launcher's older *swap* model (replacing the whole `vcb.pck`
> with a modded one, one mod at a time). Patch‑based modding is the recommended path; the
> swap model still works for whole‑game mod builds.

---

## For players

1. Point the launcher at your game folder (it auto‑detects Steam) and click
   **Enable modding**. This:
   - copies your pristine `vcb.pck` to `vcb.pck.original` (once), and
   - writes a patched `vcb.pck` that has the Mod Loader baked in.
2. Click **📁 Mods folder** to open the game's `mods/` folder and drop mod `.zip` files
   into it.
3. Launch the game. The Mod Loader loads every mod at startup.

To go back to the unmodified game, click **Disable** (or the header's **⟲ Revert to
vanilla**) — it restores `vcb.pck.original`. After a Steam update re‑downloads `vcb.pck`,
click **Re‑apply** to patch the fresh copy again.

Mods live in `…/Virtual Circuit Board/mods/*.zip` (next to the game executable) — **not**
in the launcher's own `mods/` folder (that one is for the older swap mods).

---

## For mod authors

A mod is a small GDScript package. It runs on the **original, unmodified game** — the
Mod Loader lets your code add UI, react to and drive the game, and even override existing
scripts, all **without shipping any of the game's files**.

### Package layout

A mod is a `.zip` whose contents mount into the game's `res://`. The Mod Loader looks for
each mod under `res://mods-unpacked/<mod_id>/`, so your zip must contain that folder:

```text
Author_MyMod.zip
└── mods-unpacked/
    └── Author-MyMod/            # folder name MUST equal "<namespace>-<name>"
        ├── manifest.json        # required
        ├── mod_main.gd          # required — the entry script
        └── …                    # your scripts, scenes, assets, extensions/
```

The mod **id** is `"<namespace>-<name>"` and the folder must be named exactly that.

### `manifest.json`

```json
{
    "name": "MyMod",
    "namespace": "Author",
    "version_number": "1.0.0",
    "description": "What the mod does.",
    "website_url": "https://github.com/you/mymod",
    "dependencies": [],
    "authors": ["you"],
    "compatible_mod_loader_version": ["6.3.0"],
    "compatible_game_version": ["Godot 3.5.1"]
}
```

Rules the loader enforces:

- `name` and `namespace` — letters, numbers and `_` only, **at least 3 characters**.
- `version_number` — semantic version `MAJOR.MINOR.PATCH` (no leading zeros).
- `compatible_mod_loader_version` is required; use the version the launcher bundles
  (see [`vendor/godot-mod-loader/VERSION`](../vendor/godot-mod-loader/VERSION), currently
  **6.3.0**).

### `mod_main.gd`

The entry script extends `ModLoaderMod` and runs once at startup:

```gdscript
extends ModLoaderMod

const MOD_DIR := "Author-MyMod"

func _init() -> void:
    ModLoaderLog.info("Hello from MyMod!", MOD_DIR)

    # Install a script extension to change existing game behaviour without
    # editing the game's files (see below).
    # install_script_extension("res://mods-unpacked/Author-MyMod/extensions/editor.gd")
```

### Hooking the game

The whole game is driven by an **event bus** — the `E` autoload
(`res://src/singletons/events.gd`) — plus autoloads like `C` (constants/palette). A mod
listens to and emits the same signals the game does:

```gdscript
func _init() -> void:
    var e = get_node("/root/E")
    e.connect("mi_mode_change_requested", self, "_on_sim_mode")   # react to the game
    # drive the game (e.g. pick a trace colour) with e.echo(...) / e.ask(...)
```

See the decompiled game source in the
[`vcb-original`](https://github.com/n-popescu/vcb-original) repo (`src/singletons/events.gd`
for the event catalogue, `src/editor/editor.gd` for tools/layers) to learn what to hook.

### Script extensions (changing existing behaviour)

To modify an existing game script **without replacing it**, ship an extension script that
`extends` the target by its `res://` path and call `install_script_extension` from
`mod_main.gd`. Extensions are how the Mod Loader lets many mods layer changes onto the same
game script. See the
[Mod Loader wiki](https://wiki.godotmodding.com/#/guides/modding/script_extensions).

### Building the zip

Zip the `mods-unpacked/` folder so the internal paths are exactly as shown above:

```bash
# from a working dir that contains mods-unpacked/Author-MyMod/…
zip -r Author_MyMod.zip mods-unpacked
```

Then drop `Author_MyMod.zip` into the game's `mods/` folder and launch.

---

## What a runtime mod can and can't do

A GDScript mod on the original engine **can**:

- add or change UI — toolbar buttons, windows, panels;
- listen to and emit anything on the `E` event bus (tools, colours, simulation control,
  file ops);
- add nodes, run per‑frame logic, open network sockets, ship extra assets/scenes;
- override existing game scripts via script extensions.

It **cannot** change the **native simulation engine**. VCB's simulation lives in the
closed‑source native `Transistor*` classes built into the game binary — the ink‑byte
packing, the classifier/resolver tables, and the tick algorithm. So, for example, a
mod can add a *64‑colour palette UI*, but **64 independent trace channels** need a custom
engine build (see the `vcb-traces` / `vcb-rebuild` repos), not a runtime mod. When a
feature needs the engine, ship it as an engine build plus a companion mod (the mod is the
GDScript/UI half).

---

## How the patch works (short version)

Enabling modding never decrypts or redistributes the game. The launcher:

1. keeps your pristine `vcb.pck` as `vcb.pck.original`;
2. copies every original file **byte‑for‑byte** into a new `vcb.pck` (the game's
   encrypted scripts are untouched);
3. adds the Mod Loader addon (plain GDScript) and rewrites the embedded `project.binary`
   to register the loader's two autoloads and its `class_name` globals.

Full detail — including the one item that still needs on‑device verification — is in
[`PATCHER_DESIGN.md`](PATCHER_DESIGN.md).

---

## Troubleshooting

- **A mod didn't load.** VCB ships with `run/disable_stdout=true`, so Mod Loader logs
  don't reach a normal console. Its log file is written under the game's `user://` data
  dir (`ModLoader.log`); check there for `[ModLoader]` lines and per‑mod errors.
- **"Invalid name or namespace".** `name`/`namespace` must be ≥3 chars, letters/numbers/`_`
  only, and the folder under `mods-unpacked/` must be exactly `<namespace>-<name>`.
- **Nothing changed after a Steam update.** Updates replace `vcb.pck` with a clean copy;
  click **Re‑apply** to patch it again.
