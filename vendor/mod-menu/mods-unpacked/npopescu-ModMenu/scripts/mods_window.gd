extends Popup
# scripts/mods_window.gd
#
# A stock-styled "Installed mods" window, opened from the Mods button in the Options menu.
#
# Layout is master/detail: a clickable LEFT column lists every mod the Godot Mod Loader has
# loaded (name, version, author); selecting one expands its DETAILS on the right — description,
# repository URL (with an "Open repository" button), and an "update available" check that queries
# the mod's GitHub releases and compares the latest tag to the installed version. Everything is
# read live from the loader registry (ModLoaderStore.mod_data); the window changes nothing.
#
# Styling reuses the game's own dialog machinery (FluxModPopup backdrop + stock dialog StyleBox +
# the game Theme) so it reads as a native window.

const FluxModPopupScene := preload("res://src/gui/flux/flux_mod_popup.tscn")
const FluxModButtonScene := preload("res://src/gui/flux/flux_mod_button.tscn")

# Secondary / tertiary text greys, matching the game's stock muted readouts.
const MUTED := Color(0.58, 0.63, 0.71)
const DIM := Color(0.42, 0.47, 0.55)
const ACCENT := Color(0.39, 0.85, 0.55)   # "update available"
const WHITE := Color(1, 1, 1, 1)

var _list: VBoxContainer = null
var _detail: VBoxContainer = null
var _count_label: Label = null

var _mods: Array = []
var _selected_id: String = ""
var _entry_buttons: Dictionary = {}   # mod_id -> Button (for selection highlight)
var _update_status: Dictionary = {}   # mod_id -> {"state": checking|available|current|error, "latest": "x.y.z"}


func _ready() -> void:
	_build_ui()


# Called by the Mods button.
func open_window() -> void:
	_refresh()
	popup_centered(Vector2(760, 470))


# ---------------------------------------------------------------- UI construction --
func _build_ui() -> void:
	var panel := PanelContainer.new()
	panel.name = "Panel"
	panel.anchor_right = 1.0
	panel.anchor_bottom = 1.0
	panel.add_stylebox_override("panel", _make_panel_style())
	add_child(panel)

	var margin := MarginContainer.new()
	margin.add_constant_override("margin_left", 32)
	margin.add_constant_override("margin_right", 32)
	margin.add_constant_override("margin_top", 20)
	margin.add_constant_override("margin_bottom", 20)
	panel.add_child(margin)

	var root := VBoxContainer.new()
	root.add_constant_override("separation", 12)
	margin.add_child(root)

	# Header: centered title + a muted "N mods loaded" line, like the stock dialogs.
	var title := Label.new()
	title.text = "Installed mods"
	title.align = Label.ALIGN_CENTER
	title.add_color_override("font_color", WHITE)
	root.add_child(title)

	_count_label = Label.new()
	_count_label.align = Label.ALIGN_CENTER
	_count_label.add_color_override("font_color", MUTED)
	root.add_child(_count_label)

	root.add_child(HSeparator.new())

	# Body: left list | right details.
	var body := HBoxContainer.new()
	body.add_constant_override("separation", 16)
	body.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	body.size_flags_vertical = Control.SIZE_EXPAND_FILL
	root.add_child(body)

	var left_scroll := ScrollContainer.new()
	left_scroll.rect_min_size = Vector2(240, 360)
	left_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	body.add_child(left_scroll)
	_list = VBoxContainer.new()
	_list.add_constant_override("separation", 4)
	_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left_scroll.add_child(_list)

	body.add_child(VSeparator.new())

	var right_scroll := ScrollContainer.new()
	right_scroll.rect_min_size = Vector2(400, 360)
	right_scroll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	right_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	body.add_child(right_scroll)
	_detail = VBoxContainer.new()
	_detail.add_constant_override("separation", 10)
	_detail.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	right_scroll.add_child(_detail)

	root.add_child(HSeparator.new())

	# Close button: centered, min width, with the game's stock hover (flux), like dialog_warning.
	var close_btn := Button.new()
	close_btn.text = "Close"
	close_btn.rect_min_size = Vector2(72, 0)
	close_btn.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	_add_stock_hover(close_btn)
	var _c = close_btn.connect("pressed", self, "hide")
	root.add_child(close_btn)

	rect_min_size = Vector2(760, 0)

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


# Background for a left-column entry: transparent normally, tinted when active/hovered.
func _entry_style(active: bool) -> StyleBoxFlat:
	var sb := StyleBoxFlat.new()
	sb.bg_color = Color(0.16, 0.20, 0.25, 1.0) if active else Color(0, 0, 0, 0)
	sb.set_corner_radius_all(4)
	return sb


# ------------------------------------------------------------------- list refresh ---
func _refresh() -> void:
	if _list == null:
		return
	_entry_buttons.clear()
	for child in _list.get_children():
		child.queue_free()
	_mods = _get_mods()
	if _count_label:
		var n := _mods.size()
		_count_label.text = "%d mod loaded" % n if n == 1 else "%d mods loaded" % n
	for e in _mods:
		_list.add_child(_make_left_entry(e))
	# Keep the current selection if it still exists, else select the first mod.
	var keep := _selected_id
	_selected_id = ""
	if keep != "" and not _find_mod(keep).empty():
		_select(keep)
	elif not _mods.empty():
		_select(str(_mods[0].get("id", "")))
	else:
		_clear_detail("No mods are installed.")


# One clickable mod row: a Button (for hover + click) with a two-line VBox on top (name; then
# "v<version> · <author>"). The child controls ignore the mouse so the click lands on the Button.
func _make_left_entry(e: Dictionary) -> Control:
	var id := str(e.get("id", ""))
	var btn := Button.new()
	btn.focus_mode = Control.FOCUS_NONE
	btn.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	btn.rect_min_size = Vector2(0, 46)
	btn.add_stylebox_override("normal", _entry_style(false))
	btn.add_stylebox_override("hover", _entry_style(true))
	btn.add_stylebox_override("pressed", _entry_style(true))

	var vb := VBoxContainer.new()
	vb.mouse_filter = Control.MOUSE_FILTER_IGNORE
	vb.add_constant_override("separation", 1)
	vb.anchor_right = 1.0
	vb.anchor_bottom = 1.0
	vb.margin_left = 10
	vb.margin_right = -10
	vb.margin_top = 5
	vb.margin_bottom = -5
	btn.add_child(vb)

	var name_lbl := Label.new()
	name_lbl.mouse_filter = Control.MOUSE_FILTER_IGNORE
	name_lbl.text = str(e.get("name", id))
	name_lbl.clip_text = true
	vb.add_child(name_lbl)

	var sub := Label.new()
	sub.mouse_filter = Control.MOUSE_FILTER_IGNORE
	sub.clip_text = true
	sub.add_color_override("font_color", MUTED)
	var ver := str(e.get("version", ""))
	var author := str(e.get("authors", ""))
	var subtext := ""
	if ver != "":
		subtext = "v" + ver
	if author != "":
		subtext += ("  ·  " if subtext != "" else "") + author
	sub.text = subtext
	vb.add_child(sub)

	var _c = btn.connect("pressed", self, "_on_entry_pressed", [id])
	_entry_buttons[id] = btn
	return btn


func _on_entry_pressed(id: String) -> void:
	_select(id)


func _select(id: String) -> void:
	_selected_id = id
	for mid in _entry_buttons:
		var b = _entry_buttons[mid]
		if is_instance_valid(b):
			b.add_stylebox_override("normal", _entry_style(str(mid) == id))
	var e := _find_mod(id)
	if e.empty():
		_clear_detail("Select a mod to see details.")
		return
	_show_detail(e)
	_maybe_check_update(e)


# ------------------------------------------------------------------- detail pane ---
func _clear_detail(msg: String) -> void:
	if _detail == null:
		return
	for child in _detail.get_children():
		child.queue_free()
	var lbl := Label.new()
	lbl.text = msg
	lbl.autowrap = true
	lbl.add_color_override("font_color", MUTED)
	_detail.add_child(lbl)


func _show_detail(e: Dictionary) -> void:
	if _detail == null:
		return
	for child in _detail.get_children():
		child.queue_free()

	var name_lbl := Label.new()
	name_lbl.text = str(e.get("name", ""))
	name_lbl.autowrap = true
	name_lbl.add_color_override("font_color", WHITE)
	_detail.add_child(name_lbl)

	var meta := Label.new()
	meta.add_color_override("font_color", MUTED)
	var ver := str(e.get("version", ""))
	var author := str(e.get("authors", ""))
	var metatext := ""
	if ver != "":
		metatext = "v" + ver
	if author != "":
		metatext += ("   ·   by " if metatext != "" else "by ") + author
	meta.text = metatext
	_detail.add_child(meta)

	# Update status line.
	var status_lbl := Label.new()
	status_lbl.autowrap = true
	_apply_update_text(status_lbl, e)
	_detail.add_child(status_lbl)

	_detail.add_child(HSeparator.new())

	var desc := str(e.get("description", ""))
	if desc != "":
		var desc_lbl := Label.new()
		desc_lbl.text = desc
		desc_lbl.autowrap = true
		_detail.add_child(desc_lbl)

	# Repository row + "Open repository" button.
	var website := str(e.get("website", ""))
	if website != "":
		var repo_row := HBoxContainer.new()
		repo_row.add_constant_override("separation", 8)
		repo_row.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		var repo_key := Label.new()
		repo_key.text = "Repository:"
		repo_key.add_color_override("font_color", DIM)
		repo_row.add_child(repo_key)
		var url_lbl := Label.new()
		url_lbl.text = website
		url_lbl.autowrap = true
		url_lbl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		url_lbl.add_color_override("font_color", MUTED)
		repo_row.add_child(url_lbl)
		_detail.add_child(repo_row)

		var open_btn := Button.new()
		open_btn.text = "Open repository"
		open_btn.focus_mode = Control.FOCUS_NONE
		open_btn.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
		_add_stock_hover(open_btn)
		var _c = open_btn.connect("pressed", self, "_open_url", [website])
		_detail.add_child(open_btn)

	var deps := str(e.get("dependencies", ""))
	if deps != "":
		var deps_lbl := Label.new()
		deps_lbl.text = "Requires: " + deps
		deps_lbl.autowrap = true
		deps_lbl.add_color_override("font_color", DIM)
		_detail.add_child(deps_lbl)

	var id_lbl := Label.new()
	id_lbl.text = str(e.get("id", ""))
	id_lbl.autowrap = true
	id_lbl.add_color_override("font_color", DIM)
	_detail.add_child(id_lbl)


func _open_url(url: String) -> void:
	if url.strip_edges() != "":
		OS.shell_open(url)


# ------------------------------------------------------------- update checking ---
# Fill the status Label for a mod from its cached update state (or kick off a check).
func _apply_update_text(lbl: Label, e: Dictionary) -> void:
	var website := str(e.get("website", ""))
	if _repo_from_website(website) == "":
		lbl.text = "Updates: not tracked (no GitHub repository)."
		lbl.add_color_override("font_color", DIM)
		return
	var id := str(e.get("id", ""))
	var st = _update_status.get(id, null)
	var state := str(st.get("state", "")) if typeof(st) == TYPE_DICTIONARY else "checking"
	if state == "available":
		lbl.text = "Update available: v" + str(st.get("latest", "")) + "   (installed v" + str(e.get("version", "")) + ")"
		lbl.add_color_override("font_color", ACCENT)
	elif state == "current":
		lbl.text = "Up to date (v" + str(e.get("version", "")) + ")."
		lbl.add_color_override("font_color", MUTED)
	elif state == "error":
		lbl.text = "Couldn't check for updates."
		lbl.add_color_override("font_color", DIM)
	else:
		lbl.text = "Checking for updates…"
		lbl.add_color_override("font_color", MUTED)


func _maybe_check_update(e: Dictionary) -> void:
	var id := str(e.get("id", ""))
	if _repo_from_website(str(e.get("website", ""))) == "":
		return
	if _update_status.has(id):
		return  # already checked or in flight
	_update_status[id] = {"state": "checking", "latest": ""}
	var repo := _repo_from_website(str(e.get("website", "")))
	var http := HTTPRequest.new()
	add_child(http)
	var _c = http.connect("request_completed", self, "_on_update_checked", [id, http])
	var url := "https://api.github.com/repos/" + repo + "/releases/latest"
	var headers := ["User-Agent: VCB-ModMenu", "Accept: application/vnd.github+json"]
	var err = http.request(url, headers, true, HTTPClient.METHOD_GET)
	if err != OK:
		_update_status[id] = {"state": "error", "latest": ""}
		if is_instance_valid(http):
			http.queue_free()
		_refresh_detail_if_selected(id)


func _on_update_checked(result: int, response_code: int, _headers, body, id, http) -> void:
	if is_instance_valid(http):
		http.queue_free()
	var latest := ""
	if result == HTTPRequest.RESULT_SUCCESS and response_code == 200:
		var txt := (body as PoolByteArray).get_string_from_utf8()
		var parsed := JSON.parse(txt)
		if parsed.error == OK and typeof(parsed.result) == TYPE_DICTIONARY:
			latest = _strip_v(str((parsed.result as Dictionary).get("tag_name", "")))
	if latest == "":
		_update_status[id] = {"state": "error", "latest": ""}
	elif _version_greater(latest, _installed_version(str(id))):
		_update_status[id] = {"state": "available", "latest": latest}
	else:
		_update_status[id] = {"state": "current", "latest": latest}
	_refresh_detail_if_selected(str(id))


func _refresh_detail_if_selected(id: String) -> void:
	if _selected_id == id:
		var e := _find_mod(id)
		if not e.empty():
			_show_detail(e)


func _installed_version(id: String) -> String:
	var e := _find_mod(id)
	return str(e.get("version", "")) if not e.empty() else ""


# "https://github.com/owner/repo(.git)(/…)" -> "owner/repo"; "" if not a GitHub repo URL.
func _repo_from_website(url: String) -> String:
	var u := url.strip_edges()
	var marker := "github.com/"
	var idx := u.find(marker)
	if idx == -1:
		return ""
	var rest := u.substr(idx + marker.length())
	rest = rest.split("?")[0]
	rest = rest.split("#")[0]
	if rest.ends_with(".git"):
		rest = rest.substr(0, rest.length() - 4)
	while rest.ends_with("/"):
		rest = rest.substr(0, rest.length() - 1)
	var parts := rest.split("/")
	if parts.size() < 2 or str(parts[0]) == "" or str(parts[1]) == "":
		return ""
	return str(parts[0]) + "/" + str(parts[1])


func _strip_v(tag: String) -> String:
	var t := tag.strip_edges()
	if t.begins_with("v") or t.begins_with("V"):
		t = t.substr(1)
	return t


# True if version string a is strictly newer than b (numeric, component-wise).
func _version_greater(a: String, b: String) -> bool:
	var pa := _ver_parts(a)
	var pb := _ver_parts(b)
	var n := int(max(pa.size(), pb.size()))
	for i in range(n):
		var va: int = pa[i] if i < pa.size() else 0
		var vb: int = pb[i] if i < pb.size() else 0
		if va > vb:
			return true
		if va < vb:
			return false
	return false


func _ver_parts(s: String) -> Array:
	var out := []
	for piece in s.split("."):
		var digits := ""
		for ch in str(piece):
			if ch >= "0" and ch <= "9":
				digits += ch
			else:
				break
		out.append(int(digits) if digits != "" else 0)
	return out


# ------------------------------------------------------------- loader registry ---
# Read the loader's registry. Uses the ModLoaderStore autoload (what ModLoaderMod.get_mod_data_all
# returns), guarded so a missing/renamed field can never crash the window.
func _get_mods() -> Array:
	var out := []
	var store = get_tree().root.get_node_or_null("/root/ModLoaderStore")
	if store == null:
		return out
	var mod_data = store.get("mod_data")
	if typeof(mod_data) == TYPE_DICTIONARY:
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
	# The Mod Loader is itself the "mod" that discovers and loads every other mod, so pin it to
	# the top of the list (above the alphabetically-sorted user mods).
	var loader := _mod_loader_entry(store)
	if not loader.empty():
		out.push_front(loader)
	return out


func _find_mod(id: String) -> Dictionary:
	for e in _mods:
		if str(e.get("id", "")) == id:
			return e
	return {}


# Synthesize a list entry for the Godot Mod Loader itself. Its version is read from the
# ModLoaderStore script's MODLOADER_VERSION constant (via the constant map, so a missing/renamed
# constant can never crash the window).
func _mod_loader_entry(store) -> Dictionary:
	if store == null:
		return {}
	var version := ""
	var scr = store.get_script()
	if scr != null and scr.has_method("get_script_constant_map"):
		var consts = scr.get_script_constant_map()
		if typeof(consts) == TYPE_DICTIONARY and consts.has("MODLOADER_VERSION"):
			version = str(consts["MODLOADER_VERSION"])
	return {
		"id": "GodotModding-ModLoader",
		"name": "Godot Mod Loader",
		"version": version,
		"description": "The runtime mod loader that discovers and loads the mods listed here.",
		"authors": "KANA, GodotModding",
		"website": "https://github.com/GodotModding/godot-mod-loader",
		"dependencies": "",
	}


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


# Attach the game's stock animated hover fill (FluxModButton) to a plain Button.
func _add_stock_hover(btn: Button) -> void:
	if FluxModButtonScene == null:
		return
	var flux = FluxModButtonScene.instance()
	if flux != null:
		btn.add_child(flux)
