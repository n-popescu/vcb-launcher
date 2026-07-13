extends "res://src/main/main.gd"

# extensions/main.gd
#
# Adds a "Mods" button to the Options popup (next to Fullscreen / Settings / Shortcuts /
# Changelog) and a stock-styled ModsWindow it opens. Runs once, after the scene is ready.
# Everything is guarded so a missing node/resource logs and skips rather than crashing.
#
# Uses _mm_-prefixed members so it never collides with another mod that also extends
# main.gd (e.g. the multiplayer runtime mod) — the Mod Loader chains such extensions.

const MOD_ROOT: = "res://mods-unpacked/npopescu-ModMenu"
const SCRIPTS: = MOD_ROOT + "/scripts"
const FLUX_MOD_BUTTON: = "res://src/gui/flux/flux_mod_button.tscn"
const MAIN_THEME: = "res://src/gui/themes/main_theme.tres"
# The Options popup's button column (Fullscreen / Settings / Shortcuts / Changelog live here).
const OPTIONS_VBOX: = "Interface/GUI/VBoxContainer/Header/VBoxContainer/Upper/HelpSettingsAndWindow/BtnOptions/Popup/Panel/MarginContainer/VBoxContainer"

var _mm_built: = false

func _ready() -> void :
	._ready()
	call_deferred("_mm_build")

func _mm_build() -> void :
	if _mm_built:
		return
	_mm_built = true

	# The window lives on the GUI layer (NOT inside the Options popup, which hides on focus
	# loss and would take the window down with it).
	var window: = _mm_new(SCRIPTS + "/mods_window.gd")
	if window == null:
		return
	window.name = "ModsWindow"
	var theme_res = load(MAIN_THEME)
	if theme_res is Theme:
		window.theme = theme_res
	var host: = get_node_or_null("Interface/GUI")
	if host == null:
		host = self
	host.add_child(window)

	# The button, added to the Options button column with the stock hover styling.
	var vbox: = get_node_or_null(OPTIONS_VBOX)
	if vbox == null:
		vbox = _mm_find_options_vbox()
	if vbox == null:
		push_warning("[VCB-ModMenu] Options button column not found — Mods button not added")
		return
	if vbox.get_node_or_null("BtnMods") != null:
		return
	var btn: = Button.new()
	btn.name = "BtnMods"
	btn.text = "Mods"
	if ResourceLoader.exists(FLUX_MOD_BUTTON):
		var flux_scene = load(FLUX_MOD_BUTTON)
		if flux_scene != null:
			btn.add_child(flux_scene.instance())
	vbox.add_child(btn)
	var _c = btn.connect("pressed", window, "open_window")

func _mm_find_options_vbox() -> Node:
	var opts: = find_node("BtnOptions", true, false)
	if opts == null:
		return null
	return opts.get_node_or_null("Popup/Panel/MarginContainer/VBoxContainer")

# Instance a mod script, or null (logged) if it can't be loaded — never dereference a null.
func _mm_new(path: String) -> Node:
	if not ResourceLoader.exists(path):
		push_warning("[VCB-ModMenu] missing script: " + path)
		return null
	var scr = load(path)
	if scr == null:
		return null
	var inst = scr.new()
	return inst as Node
