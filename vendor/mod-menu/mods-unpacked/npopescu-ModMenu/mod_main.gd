extends Node

# mod_main.gd — Mod Loader entry point for the VCB Mod Menu.
#
# Adds a "Mods" button to the Options menu that opens a stock-styled window listing every
# installed mod (name, version, authors, description, details) from the loader's registry.
#
# NOTE: we do NOT extend the main-scene root script (main.gd) — doing so via
# install_script_extension hard-crashes the Godot Mod Loader on this game. Instead we wait
# (in _process) for the Main scene to exist and build the button + window ourselves.

const MOD_DIR := "npopescu-ModMenu"
const MOD_ROOT := "res://mods-unpacked/npopescu-ModMenu"
const SCRIPTS := MOD_ROOT + "/scripts"
const FLUX_MOD_BUTTON := "res://src/gui/flux/flux_mod_button.tscn"
const MAIN_THEME := "res://src/gui/themes/main_theme.tres"
# The Options popup's button column (Fullscreen / Settings / Shortcuts / Changelog live here).
const OPTIONS_VBOX := "Interface/GUI/VBoxContainer/Header/VBoxContainer/Upper/HelpSettingsAndWindow/BtnOptions/Popup/Panel/MarginContainer/VBoxContainer"
# The header version readout ("Virtual Circuit Board · 1.0.1"). We suffix it with "-modded"
# so a successful Mod Loader load is visible at a glance.
const VERSION_LABEL := "Interface/GUI/VBoxContainer/Header/VBoxContainer/Upper/AlertBar/HBoxContainer/VersionLabel"
const MODDED_SUFFIX := "-modded"

var _built := false
var _mods_window = null
var _options_popup = null


func _init() -> void:
	ModLoaderLog.info("Installing VCB Mod Menu…", MOD_DIR)
	# No script extensions — the button + window are built from _process once the scene is up.


func _ready() -> void:
	set_process(true)


func _process(_delta: float) -> void:
	if _built:
		set_process(false)
		return
	var root := get_tree().root
	var main := root.get_node_or_null("Main")
	if main == null:
		return
	var vbox := main.get_node_or_null(OPTIONS_VBOX)
	if vbox == null:
		vbox = _find_options_vbox(main)
	if vbox == null:
		return
	_built = true
	set_process(false)
	_build(main, vbox)


func _build(main: Node, vbox: Node) -> void:
	# Tag the header version readout so a loaded mod is visible without opening any menu
	# (e.g. "Virtual Circuit Board · 1.0.1-modded"). The Mod Menu ships with every modded
	# install, so its presence is a reliable "modding is active" signal.
	_tag_version_modded(main)

	# The window lives on the GUI layer (NOT inside the Options popup, which hides on focus
	# loss and would take the window down with it).
	var window := _new(SCRIPTS + "/mods_window.gd")
	if window == null:
		return
	window.name = "ModsWindow"
	var theme_res = load(MAIN_THEME)
	if theme_res is Theme:
		window.theme = theme_res
	var host := main.get_node_or_null("Interface/GUI")
	if host == null:
		host = main
	host.add_child(window)
	_mods_window = window
	_options_popup = _find_options_popup(main)

	# The button, added to the Options button column with the stock hover styling.
	if vbox.get_node_or_null("BtnMods") != null:
		return
	var btn := Button.new()
	btn.name = "BtnMods"
	btn.text = "Mods"
	if ResourceLoader.exists(FLUX_MOD_BUTTON):
		var flux_scene = load(FLUX_MOD_BUTTON)
		if flux_scene != null:
			btn.add_child(flux_scene.instance())
	vbox.add_child(btn)
	var _c = btn.connect("pressed", self, "_on_mods_button_pressed")


# Close the Options popup before opening the Mods window, otherwise the Options menu is left
# sitting open behind it. The window lives on the GUI layer (not inside the popup), so hiding
# the popup doesn't take the window down with it.
func _on_mods_button_pressed() -> void:
	if _options_popup != null and is_instance_valid(_options_popup):
		_options_popup.hide()
	if _mods_window != null and is_instance_valid(_mods_window):
		_mods_window.open_window()


func _find_options_popup(main: Node) -> Node:
	var opts := main.find_node("BtnOptions", true, false)
	if opts == null:
		return null
	return opts.get_node_or_null("Popup")


func _find_options_vbox(main: Node) -> Node:
	var opts := main.find_node("BtnOptions", true, false)
	if opts == null:
		return null
	return opts.get_node_or_null("Popup/Panel/MarginContainer/VBoxContainer")


# Append "-modded" to the version label so a successful mod load is visible in-game. The
# label's own _ready has already set its text by the time the Main scene exists, so we just
# suffix it. Guarded to be idempotent and to never crash if the node moves/renames.
func _tag_version_modded(main: Node) -> void:
	var label = main.get_node_or_null(VERSION_LABEL)
	if label == null:
		label = main.find_node("VersionLabel", true, false)
	if not (label is Label):
		return
	var current := str(label.text)
	if current.find(MODDED_SUFFIX) == -1:
		label.text = current + MODDED_SUFFIX


# Instance a mod script, or null (logged) if it can't be loaded — never dereference a null.
func _new(path: String) -> Node:
	if not ResourceLoader.exists(path):
		push_warning("[VCB-ModMenu] missing script: " + path)
		return null
	var scr = load(path)
	if scr == null:
		return null
	return scr.new() as Node
