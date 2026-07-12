# Patch‑based modding — design & feasibility

How the launcher adds modding to a **shipped** copy of Virtual Circuit Board by *patching*
`vcb.pck` instead of replacing it, and the reasoning/risks behind each decision.

## Goal

Keep the original, closed‑engine game exactly as shipped, but make it able to load mods at
runtime. Concretely: inject the
[Godot Mod Loader](https://godotengine.org/asset-library/asset/1938) (asset **1938**,
v6.3.0, CC0) into `vcb.pck` once, keep the pristine original aside, and let players drop
mod packages into a `mods/` folder.

## Why the Godot Mod Loader

Its explicit purpose is *modifying existing scripts without altering and redistributing the
original game files* — the "patch, don't replace" model this task asked for. It's mature
(used by Brotato, Dome Keeper, Windowkill, …), targets Godot 3.5 (VCB is 3.5.1), is pure
GDScript, and is CC0 so it can be vendored into the launcher. It needs only two autoload
singletons and loads mod `.zip`s from `<game>/mods/`.

## The shipped game, briefly

- **Engine:** Godot 3.5.1, pack format **version 1**.
- **`vcb.pck`** sits next to `vcb.exe`/`vcb.x86_64`; the engine auto‑mounts it at boot.
- **Scripts are encrypted.** VCB shipped its GDScript as encrypted `.gdc` (this is why the
  decompiled `vcb-original` repo has `.autoconverted/*.gde` artifacts, and why the task
  mentioned a "decryption key"). In Godot 3.5 this is *script* encryption, **not**
  whole‑pack/directory encryption — the pck **directory and file data are plaintext**; only
  the contents of script files are AES‑encrypted with a key baked into the engine binary.

That last point is the crux: **we can read and rebuild the pck directory without any key,
and we never need to decrypt the game's scripts** — we copy their bytes verbatim.

## What "adding the Mod Loader" actually requires

The Mod Loader is normally added in the Godot editor, which auto‑registers three things in
`project.godot`. Since VCB is already built, the launcher must reproduce all three inside
the shipped pck:

1. **The addon files** — `res://addons/mod_loader/**` and `res://addons/JSON_Schema_Validator/**`
   (plain GDScript + a few `.tres`). Added to the pck directory.
2. **Two autoloads** — `ModLoaderStore` then `ModLoader`, and they must load **first** (the
   loader extends other scripts before the game instances them). Stored in
   `project.binary` under `autoload/…`.
3. **`class_name` globals** — the addon declares ~24 `class_name`s (`ModLoaderLog`,
   `ModData`, `_ModLoaderPath`, …) and references them by bare name. Those are registered in
   `project.binary`'s `_global_script_classes` / `_global_script_class_icons`. VCB already
   uses these settings (7 of its own classes), so the launcher **merges** the loader's
   classes into the existing arrays rather than overwriting.

## The pipeline (`src/patch.rs`)

```
vcb.pck ──(once)──► vcb.pck.original            # pristine snapshot, kept forever
   │
   ├─ read directory (src/pckbuild.rs)          # v1 pck, no key needed
   ├─ read res://project.binary                 # ECFG settings blob
   ├─ patch project.binary (src/projbin.rs):
   │     • add autoloads ModLoaderStore, ModLoader (before any existing autoload/*)
   │     • merge loader class_names into _global_script_classes / _icons
   └─ write new vcb.pck (src/pckbuild.rs):
         • every original file copied BYTE‑FOR‑BYTE (encrypted .gdc untouched)
         • res://project.binary  → patched bytes
         • res://addons/…        → the embedded Mod Loader
```

The patched pck is written to a temp file and swapped in atomically. Re‑running always
patches **from `vcb.pck.original`**, so it can't double‑inject and is safe after game
updates.

### Format details that had to be exact

- **PCK v1 directory** (`src/pckbuild.rs`): `GDPC` magic, version 1, three version ints,
  16 reserved ints, file count; then per file: padded path, absolute u64 offset, u64 size,
  16‑byte MD5. Original files reuse their recorded MD5; added files use zero MD5 (the engine
  doesn't verify it at load — Godot's own `PCKPacker` also writes zeros). Offsets are
  absolute from the file start.
- **`project.binary`** (`src/projbin.rs`): `ECFG` magic, u32 count, then per setting a
  pascal‑string key and a length‑prefixed `encode_variant` value. Autoload order is set by
  file order (each setting gets an incrementing *order* on load, and autoloads are then run
  sorted by it), so inserting our two before the first existing `autoload/*` makes them run
  first. A minimal Variant codec (String / Array / Dictionary — everything those two
  settings contain) is implemented and round‑trip tested; **all other settings' value bytes
  are copied through untouched**, so the codec never has to understand the full Variant set.

Both formats were implemented against the Godot 3.5 source
(`core/io/file_access_pack.cpp`, `core/project_settings.cpp`, `core/io/marshalls.cpp`) and
are covered by round‑trip + full‑pipeline unit tests (`cargo test`).

## Encryption: what's needed and what isn't

- **Reading / rebuilding the pck:** no key. The directory and file data are plaintext.
- **Preserving the game's scripts:** no key. Their encrypted bytes are copied verbatim.
- **Mods:** no key. Mods are plain GDScript in a `.zip`; `load_resource_pack` + runtime
  compilation handle plain source.

### The one open risk (needs on‑device verification)

The injected Mod Loader scripts are added as **plaintext `.gd`**. A Godot 3.5 build with a
script‑encryption key still loads plaintext `.gd` source at runtime (encryption applies to
files stored encrypted, keyed off the file header — a plain `.gd` is compiled as source), so
this *should* work on the encrypted retail build. This is the assumption that must be
confirmed on a real install: patch, launch, and check the Mod Loader log initialises.

**If** the retail build refuses to run the plaintext injected scripts, the fallback is to
compile + encrypt only the injected `.gd` (to `.gdc`/`.gde`) with VCB's script key before
packing. The pipeline is structured so this becomes a single transform applied to the added
script files (the `Source::Bytes` for `res://addons/**/*.gd`); nothing else changes. The key
would be supplied by the user/launcher config — it is **not** required for the happy path and
is not stored in this repo.

## Relationship to the earlier attempt

An earlier, untested branch (`vcb-rebuild@claude/modding-support`) built a *homemade* loader
into the **rebuild source tree** (`res://src/mods/**` + a `Mods` autoload). That only helps a
game you compile yourself. This approach instead patches the **shipped** game and uses the
mature, standard Mod Loader — so it works on the retail Steam build and gives mod authors a
well‑documented, widely‑used API.

## Bundling / licensing

The Mod Loader is vendored under [`vendor/godot-mod-loader/`](../vendor/godot-mod-loader)
and embedded into the launcher at build time (`build.rs`) so the app stays a single portable
binary. It is CC0; its `LICENSE` files are kept alongside the sources.

## Test coverage

`cargo test` covers: the Variant codec (round‑trip, string padding, autoload ordering, class
merge, idempotency), pck read/write (round‑trip, verbatim original copy + added files), and
the full `enable/disable/re‑apply` pipeline against a synthetic VCB‑like pck (original bytes
preserved, whole addon injected, autoloads first, classes merged, byte‑identical restore).
Not covered here (needs a real Godot runtime): that the retail engine loads the plaintext
injected scripts, and end‑to‑end mod loading in‑game.
