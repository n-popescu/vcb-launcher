extends Node

# mod_main.gd — Mod Loader entry point for the VCB Mod Menu.
#
# Adds a "Mods" button to the Options menu that opens a stock-styled window listing every
# installed mod (name, version, authors, description, details) from the loader's registry.
# All the work is done by a single script extension on the Main scene root.

const MOD_DIR := "npopescu-ModMenu"

var _ext := "res://mods-unpacked/%s/extensions/" % MOD_DIR


func _init() -> void:
	ModLoaderLog.info("Installing VCB Mod Menu…", MOD_DIR)
	ModLoaderMod.install_script_extension(_ext + "main.gd")
