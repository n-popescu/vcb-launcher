extends Popup
# scripts/mods_window.gd
#
# A stock-styled "Installed mods" window (like the Multiplayer window), opened from the
# Mods button in the Options menu. It lists every mod the Godot Mod Loader has loaded, with
# each mod's name, version, authors, description and details — read live from the loader's
# registry (ModLoaderStore.mod_data). Pure UI + a read of the loader state; it changes
# nothing.
#
# Styling reuses the game's own dialog machinery — the shared FluxModPopup backdrop + the
# stock dialog StyleBoxFlat + the game Theme — so it reads as a native window.

const FluxModPopupScene := preload("res://src/gui/flux/flux_mod_popup.tscn")

var _list: VBoxContainer = null
var _empty_label: Label = null

func _ready() -> void:
	_build_ui()

# Called by the Mods button.
func open_window() -> void:
	_refresh()
	popup_centered()
	set_as_minsize()

# ---------------------------------------------------------------- UI construction --
func _build_ui() -> void:
	var panel := PanelContainer.new()
	panel.name = "Panel"
	panel.anchor_right = 1.0
	panel.anchor_bottom = 1.0
	panel.add_stylebox_override("panel", _make_panel_style())
	add_child(panel)

	var margin := MarginContainer.new()
	margin.add_constant_override("margin_left", 30)
	margin.add_constant_override("margin_right", 30)
	margin.add_constant_override("margin_top", 20)
	margin.add_constant_override("margin_bottom", 20)
	panel.add_child(margin)

	var root := VBoxContainer.new()
	root.add_constant_override("separation", 8)
	margin.add_child(root)

	var title := Label.new()
	title.text = "Installed mods"
	title.align = Label.ALIGN_CENTER
	root.add_child(title)
	root.add_child(HSeparator.new())

	# Scrollable list so any number of mods fits.
	var scroll := ScrollContainer.new()
	scroll.rect_min_size = Vector2(440, 320)
	scroll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	root.add_child(scroll)

	_list = VBoxContainer.new()
	_list.add_constant_override("separation", 12)
	_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.add_child(_list)

	_empty_label = Label.new()
	_empty_label.text = "No mods are installed."
	_empty_label.align = Label.ALIGN_CENTER
	_empty_label.autowrap = true
	root.add_child(_empty_label)

	root.add_child(HSeparator.new())
	var close_btn := Button.new()
	close_btn.text = "Close"
	var _c = close_btn.connect("pressed", self, "hide")
	root.add_child(close_btn)

	rect_min_size = Vector2(500, 0)

	# Stock backdrop + centered scale/fade entrance, exactly like the built-in dialogs.
	var flux := FluxModPopupScene.instance()
	flux.is_keep_centered_on_resize = true
	add_child(flux)

func _make_panel_style() -> StyleBoxFlat:
	var sb := StyleBoxFlat.new()
	sb.bg_color = Color(0.0745098, 0.0941176, 0.12549, 1)
	sb.border_color = Color(0.164706, 0.207843, 0.254902, 1)
	sb.set_border_width_all(1)
	sb.set_corner_radius_all(8)
	sb.corner_detail = 5
	sb.shadow_color = Color(0.054902, 0.0745098, 0.117647, 0.156863)
	sb.shadow_size = 16
	sb.set_default_margin(MARGIN_LEFT, 4)
	sb.set_default_margin(MARGIN_TOP, 4)
	sb.set_default_margin(MARGIN_RIGHT, 4)
	sb.set_default_margin(MARGIN_BOTTOM, 4)
	return sb

# ------------------------------------------------------------------- list refresh ---
func _refresh() -> void:
	if _list == null:
		return
	for child in _list.get_children():
		child.queue_free()
	var mods := _get_mods()
	if _empty_label:
		_empty_label.visible = mods.empty()
	for entry in mods:
		_list.add_child(_make_mod_entry(entry))

# Read the loader's registry. Uses the ModLoaderStore autoload (what ModLoaderMod.get_mod_data_all
# returns), guarded so a missing/renamed field can never crash the window.
func _get_mods() -> Array:
	var out := []
	var store = get_tree().root.get_node_or_null("/root/ModLoaderStore")
	if store == null:
		return out
	var mod_data = store.get("mod_data")
	if typeof(mod_data) != TYPE_DICTIONARY:
		return out
	for mod_id in mod_data:
		var md = mod_data[mod_id]
		if md == null:
			continue
		var mani = md.get("manifest")
		if mani == null:
			continue
		out.append({
			"id": str(mod_id),
			"name": _s(mani.get("name"), str(mod_id)),
			"version": _s(mani.get("version_number"), ""),
			"description": _s(mani.get("description"), ""),
			"authors": _join(mani.get("authors")),
			"website": _s(mani.get("website_url"), ""),
			"dependencies": _join(mani.get("dependencies")),
		})
	out.sort_custom(self, "_sort_by_name")
	return out

func _sort_by_name(a: Dictionary, b: Dictionary) -> bool:
	return String(a.get("name", "")).to_lower() < String(b.get("name", "")).to_lower()

func _s(value, fallback: String) -> String:
	if value == null:
		return fallback
	var text := str(value).strip_edges()
	return text if text != "" else fallback

func _join(value) -> String:
	if value == null:
		return ""
	var parts := []
	for item in value:
		var text := str(item).strip_edges()
		if text != "":
			parts.append(text)
	return PoolStringArray(parts).join(", ")

# One mod's card in the list.
func _make_mod_entry(e: Dictionary) -> Control:
	var box := VBoxContainer.new()
	box.add_constant_override("separation", 3)
	box.size_flags_horizontal = Control.SIZE_EXPAND_FILL

	var header := HBoxContainer.new()
	header.add_constant_override("separation", 8)
	header.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var name_lbl := Label.new()
	name_lbl.text = str(e.get("name", ""))
	name_lbl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(name_lbl)
	var ver := str(e.get("version", ""))
	if ver != "":
		var ver_lbl := Label.new()
		ver_lbl.text = "v" + ver
		ver_lbl.add_color_override("font_color", Color(0.58, 0.63, 0.71))
		header.add_child(ver_lbl)
	box.add_child(header)

	var authors := str(e.get("authors", ""))
	if authors != "":
		var by := Label.new()
		by.text = "by " + authors
		by.add_color_override("font_color", Color(0.58, 0.63, 0.71))
		box.add_child(by)

	var desc := str(e.get("description", ""))
	if desc != "":
		var desc_lbl := Label.new()
		desc_lbl.text = desc
		desc_lbl.autowrap = true
		box.add_child(desc_lbl)

	# Dim technical line: id · website · dependencies.
	var tech := []
	var id := str(e.get("id", ""))
	if id != "":
		tech.append(id)
	var website := str(e.get("website", ""))
	if website != "":
		tech.append(website)
	var deps := str(e.get("dependencies", ""))
	if deps != "":
		tech.append("needs: " + deps)
	if not tech.empty():
		var tech_lbl := Label.new()
		tech_lbl.text = PoolStringArray(tech).join("   \u00b7   ")
		tech_lbl.autowrap = true
		tech_lbl.add_color_override("font_color", Color(0.42, 0.47, 0.55))
		box.add_child(tech_lbl)

	box.add_child(HSeparator.new())
	return box
