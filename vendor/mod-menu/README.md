# Vendored: VCB Mod Menu

This is a **verbatim copy** of the `npopescu-ModMenu` runtime mod, whose canonical source is
[`vcb-mp/mod-menu/`](https://github.com/n-popescu/vcb-mp/tree/main/mod-menu). The launcher
embeds these files at build time (`build.rs`) and writes them into the game's `mods/` folder
as `npopescu-ModMenu.zip` whenever **Enable modding** runs, so the in-game **Options ▸ Mods**
list is installed automatically.

Keep this copy in sync with `vcb-mp/mod-menu/mods-unpacked/npopescu-ModMenu/` — if you change
the mod there, copy the change here too (and vice versa).
