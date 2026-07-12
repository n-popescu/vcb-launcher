//! Read and patch a Godot 3.x `project.binary` — the compiled project settings that
//! live inside an exported `.pck` (`res://project.binary`).
//!
//! The launcher patches it to register the Godot Mod Loader: two autoload singletons
//! (`ModLoaderStore`, `ModLoader`) plus the loader's `class_name` globals, which are
//! stored in the `_global_script_classes` / `_global_script_class_icons` settings.
//!
//! ## Format (`ProjectSettings::_save_settings_binary`, Godot 3.5)
//! ```text
//! "ECFG"                      4-byte magic
//! count                       u32 LE — number of settings
//! count × {
//!   key_len  u32 LE           pascal string length (no padding)
//!   key      key_len bytes    UTF-8, e.g. "autoload/E" or "_global_script_classes"
//!   val_len  u32 LE           length of the encoded value blob
//!   value    val_len bytes    an `encode_variant` blob (first u32 is the type tag)
//! }
//! ```
//! On load, settings are `set()` in file order and each is assigned an incrementing
//! *order*; autoloads are later instanced sorted by that order. So inserting our two
//! autoloads **before the first existing `autoload/*` entry** makes them load first —
//! which the Mod Loader requires (it must run before the scripts it extends).
//!
//! We only ever decode the two settings we touch (`_global_script_classes` = Array of
//! Dictionaries; `_global_script_class_icons` = Dictionary) plus the String values we
//! add. Every other setting's value bytes are copied through untouched.

// Variant type tags (low byte of the encode_variant header), Godot 3.5.
const T_STRING: u32 = 4;
const T_DICTIONARY: u32 = 18;
const T_ARRAY: u32 = 19;
const ENCODE_MASK: u32 = 0xFF;

const MAGIC: &[u8; 4] = b"ECFG";

/// A `class_name` global to register, mirroring one `_global_script_classes` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct GlobalClass {
    pub base: String,
    pub class: String,
    pub path: String,
}

/// A minimal Variant model — only the shapes `project.binary` uses for the two
/// settings we edit (all keys/values there are strings).
#[derive(Clone, Debug, PartialEq)]
enum GVal {
    Str(String),
    Array(Vec<GVal>),
    Dict(Vec<(GVal, GVal)>),
}

// --- little-endian readers -----------------------------------------------------------
fn rd_u32(b: &[u8], p: &mut usize) -> Option<u32> {
    let v = b.get(*p..*p + 4)?;
    *p += 4;
    Some(u32::from_le_bytes(v.try_into().unwrap()))
}

fn wr_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

// --- Variant decode (subset) ---------------------------------------------------------
fn decode(b: &[u8], p: &mut usize) -> Option<GVal> {
    let type_tag = rd_u32(b, p)? & ENCODE_MASK;
    match type_tag {
        T_STRING => Some(GVal::Str(decode_string(b, p)?)),
        T_ARRAY => {
            let count = (rd_u32(b, p)? & 0x7FFF_FFFF) as usize;
            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(decode(b, p)?);
            }
            Some(GVal::Array(items))
        }
        T_DICTIONARY => {
            let count = (rd_u32(b, p)? & 0x7FFF_FFFF) as usize;
            let mut pairs = Vec::with_capacity(count);
            for _ in 0..count {
                let k = decode(b, p)?;
                let v = decode(b, p)?;
                pairs.push((k, v));
            }
            Some(GVal::Dict(pairs))
        }
        _ => None, // a type we don't model — refuse rather than corrupt
    }
}

fn decode_string(b: &[u8], p: &mut usize) -> Option<String> {
    let strlen = rd_u32(b, p)? as usize;
    let pad = (4 - (strlen % 4)) % 4;
    let bytes = b.get(*p..*p + strlen)?;
    *p += strlen + pad;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

// --- Variant encode (subset) ---------------------------------------------------------
fn encode(v: &GVal, out: &mut Vec<u8>) {
    match v {
        GVal::Str(s) => {
            wr_u32(out, T_STRING);
            encode_string(s, out);
        }
        GVal::Array(items) => {
            wr_u32(out, T_ARRAY);
            wr_u32(out, items.len() as u32);
            for it in items {
                encode(it, out);
            }
        }
        GVal::Dict(pairs) => {
            wr_u32(out, T_DICTIONARY);
            wr_u32(out, pairs.len() as u32);
            for (k, val) in pairs {
                encode(k, out);
                encode(val, out);
            }
        }
    }
}

fn encode_string(s: &str, out: &mut Vec<u8>) {
    let bytes = s.as_bytes();
    wr_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    let pad = (4 - (bytes.len() % 4)) % 4;
    out.extend(std::iter::repeat_n(0u8, pad));
}

fn encode_value(v: &GVal) -> Vec<u8> {
    let mut out = Vec::new();
    encode(v, &mut out);
    out
}

/// A parsed `project.binary`: ordered (key, raw-encoded-value) pairs.
pub struct ProjectBinary {
    entries: Vec<(String, Vec<u8>)>,
}

impl ProjectBinary {
    /// Parse the `ECFG` blob. Returns `None` if it isn't a project.binary we understand.
    pub fn parse(data: &[u8]) -> Option<ProjectBinary> {
        if data.get(0..4)? != MAGIC {
            return None;
        }
        let mut p = 4usize;
        let count = rd_u32(data, &mut p)?;
        if count > 1_000_000 {
            return None;
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let key_len = rd_u32(data, &mut p)? as usize;
            if key_len > 65536 {
                return None;
            }
            let key_bytes = data.get(p..p + key_len)?;
            p += key_len;
            let key = String::from_utf8_lossy(key_bytes).into_owned();
            let val_len = rd_u32(data, &mut p)? as usize;
            let val = data.get(p..p + val_len)?.to_vec();
            p += val_len;
            entries.push((key, val));
        }
        Some(ProjectBinary { entries })
    }

    /// Re-serialize to the `ECFG` binary format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        wr_u32(&mut out, self.entries.len() as u32);
        for (key, val) in &self.entries {
            let kb = key.as_bytes();
            wr_u32(&mut out, kb.len() as u32);
            out.extend_from_slice(kb);
            wr_u32(&mut out, val.len() as u32);
            out.extend_from_slice(val);
        }
        out
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|(k, _)| k == key)
    }

    /// Register autoload singletons, loading **first** (inserted before any existing
    /// `autoload/*`). `autoloads` are `(name, res_path)`; the stored value is
    /// `"*<res_path>"` (the `*` marks it an enabled singleton). Already-present names
    /// are skipped, so patching twice is a no-op.
    pub fn add_autoloads(&mut self, autoloads: &[(&str, &str)]) {
        let insert_at = self
            .entries
            .iter()
            .position(|(k, _)| k.starts_with("autoload/"))
            .unwrap_or(self.entries.len());

        let mut block: Vec<(String, Vec<u8>)> = Vec::new();
        for (name, path) in autoloads {
            let key = format!("autoload/{}", name);
            if self.has_key(&key) {
                continue;
            }
            let value = encode_value(&GVal::Str(format!("*{}", path)));
            block.push((key, value));
        }
        // Splice the block in as a unit so ModLoaderStore stays before ModLoader.
        for (i, e) in block.into_iter().enumerate() {
            self.entries.insert(insert_at + i, e);
        }
    }

    /// Merge `class_name` globals into `_global_script_classes` (Array of Dict) and
    /// `_global_script_class_icons` (Dict). Existing classes (matched by name) are left
    /// untouched, so this is idempotent and preserves the game's own classes.
    pub fn merge_global_classes(&mut self, classes: &[GlobalClass]) {
        // --- _global_script_classes -----------------------------------------------
        let mut arr = match self.index_of("_global_script_classes") {
            Some(i) => match decode(&self.entries[i].1, &mut 0usize) {
                Some(GVal::Array(items)) => items,
                _ => Vec::new(),
            },
            None => Vec::new(),
        };
        let existing: Vec<String> = arr.iter().filter_map(class_name_of).collect();
        for c in classes {
            if existing.iter().any(|e| e == &c.class) {
                continue;
            }
            arr.push(GVal::Dict(vec![
                (GVal::Str("base".into()), GVal::Str(c.base.clone())),
                (GVal::Str("class".into()), GVal::Str(c.class.clone())),
                (GVal::Str("language".into()), GVal::Str("GDScript".into())),
                (GVal::Str("path".into()), GVal::Str(c.path.clone())),
            ]));
        }
        self.set_value("_global_script_classes", encode_value(&GVal::Array(arr)));

        // --- _global_script_class_icons --------------------------------------------
        let mut icons = match self.index_of("_global_script_class_icons") {
            Some(i) => match decode(&self.entries[i].1, &mut 0usize) {
                Some(GVal::Dict(pairs)) => pairs,
                _ => Vec::new(),
            },
            None => Vec::new(),
        };
        for c in classes {
            let present = icons
                .iter()
                .any(|(k, _)| matches!(k, GVal::Str(s) if s == &c.class));
            if !present {
                icons.push((GVal::Str(c.class.clone()), GVal::Str(String::new())));
            }
        }
        self.set_value(
            "_global_script_class_icons",
            encode_value(&GVal::Dict(icons)),
        );
    }

    /// Replace an existing setting's value, or append it if absent.
    fn set_value(&mut self, key: &str, value: Vec<u8>) {
        match self.index_of(key) {
            Some(i) => self.entries[i].1 = value,
            None => self.entries.push((key.to_string(), value)),
        }
    }
}

/// Build a `project.binary` blob for tests (in this crate, across modules).
/// `string_settings` are plain `key => String` settings; `global_classes` are
/// `(base, class, path)`; `autoloads` are `(name, res_path)` stored as `"*res_path"`.
#[cfg(test)]
pub(crate) fn build_project_binary_for_test(
    string_settings: &[(&str, &str)],
    global_classes: &[(&str, &str, &str)],
    autoloads: &[(&str, &str)],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    let count = string_settings.len() + 2 /* the two class settings */ + autoloads.len();
    wr_u32(&mut out, count as u32);

    let put = |out: &mut Vec<u8>, key: &str, val: &GVal| {
        let kb = key.as_bytes();
        wr_u32(out, kb.len() as u32);
        out.extend_from_slice(kb);
        let v = encode_value(val);
        wr_u32(out, v.len() as u32);
        out.extend_from_slice(&v);
    };

    for (k, v) in string_settings {
        put(&mut out, k, &GVal::Str((*v).to_string()));
    }
    let classes: Vec<GVal> = global_classes
        .iter()
        .map(|(base, class, path)| {
            GVal::Dict(vec![
                (GVal::Str("base".into()), GVal::Str((*base).into())),
                (GVal::Str("class".into()), GVal::Str((*class).into())),
                (GVal::Str("language".into()), GVal::Str("GDScript".into())),
                (GVal::Str("path".into()), GVal::Str((*path).into())),
            ])
        })
        .collect();
    put(&mut out, "_global_script_classes", &GVal::Array(classes));
    let icons: Vec<(GVal, GVal)> = global_classes
        .iter()
        .map(|(_, class, _)| (GVal::Str((*class).into()), GVal::Str(String::new())))
        .collect();
    put(&mut out, "_global_script_class_icons", &GVal::Dict(icons));
    for (name, path) in autoloads {
        put(&mut out, &format!("autoload/{}", name), &GVal::Str(format!("*{}", path)));
    }
    out
}

/// Read back the class names in `_global_script_classes` (test helper for other modules).
#[cfg(test)]
pub(crate) fn read_global_class_names_for_test(data: &[u8]) -> Vec<String> {
    let pb = ProjectBinary::parse(data).expect("parse");
    let idx = pb.index_of("_global_script_classes").expect("has classes");
    match decode(&pb.entries[idx].1, &mut 0usize) {
        Some(GVal::Array(items)) => items.iter().filter_map(class_name_of).collect(),
        _ => Vec::new(),
    }
}

/// Read back the ordered autoload keys (test helper for other modules).
#[cfg(test)]
pub(crate) fn read_autoload_order_for_test(data: &[u8]) -> Vec<String> {
    let pb = ProjectBinary::parse(data).expect("parse");
    pb.entries
        .iter()
        .filter(|(k, _)| k.starts_with("autoload/"))
        .map(|(k, _)| k.clone())
        .collect()
}

/// The `"class"` field of a `_global_script_classes` Dict entry, if present.
fn class_name_of(entry: &GVal) -> Option<String> {
    if let GVal::Dict(pairs) = entry {
        for (k, v) in pairs {
            if let (GVal::Str(k), GVal::Str(v)) = (k, v) {
                if k == "class" {
                    return Some(v.clone());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Encode one setting the way _save_settings_binary does.
    fn put(out: &mut Vec<u8>, key: &str, value: &GVal) {
        let kb = key.as_bytes();
        wr_u32(out, kb.len() as u32);
        out.extend_from_slice(kb);
        let v = encode_value(value);
        wr_u32(out, v.len() as u32);
        out.extend_from_slice(&v);
    }

    fn class_dict(base: &str, class: &str, path: &str) -> GVal {
        GVal::Dict(vec![
            (GVal::Str("base".into()), GVal::Str(base.into())),
            (GVal::Str("class".into()), GVal::Str(class.into())),
            (GVal::Str("language".into()), GVal::Str("GDScript".into())),
            (GVal::Str("path".into()), GVal::Str(path.into())),
        ])
    }

    // Build a small but realistic project.binary.
    fn sample() -> Vec<u8> {
        let mut body = Vec::new();
        let n = 4u32;
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        wr_u32(&mut out, n);

        put(&mut body, "application/config/name", &GVal::Str("Virtual Circuit Board".into()));
        put(
            &mut body,
            "_global_script_classes",
            &GVal::Array(vec![class_dict("Node", "Editor", "res://src/editor/editor.gd")]),
        );
        put(
            &mut body,
            "_global_script_class_icons",
            &GVal::Dict(vec![(GVal::Str("Editor".into()), GVal::Str(String::new()))]),
        );
        put(&mut body, "autoload/E", &GVal::Str("*res://src/singletons/events.gd".into()));

        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn round_trips_unchanged() {
        let data = sample();
        let pb = ProjectBinary::parse(&data).expect("parse");
        assert_eq!(pb.to_bytes(), data, "byte-identical round-trip");
    }

    #[test]
    fn string_padding_is_correct() {
        // Lengths 0..7 exercise every padding case; each must be padded to a mult of 4.
        for n in 0..8usize {
            let s: String = "x".repeat(n);
            let enc = encode_value(&GVal::Str(s.clone()));
            assert_eq!(enc.len() % 4, 0, "value stays 4-aligned for len {n}");
            let mut p = 0;
            assert_eq!(decode(&enc, &mut p), Some(GVal::Str(s)));
            assert_eq!(p, enc.len(), "consumed exactly the padded blob");
        }
    }

    #[test]
    fn adds_autoloads_first_and_in_order() {
        let mut pb = ProjectBinary::parse(&sample()).unwrap();
        pb.add_autoloads(&[
            ("ModLoaderStore", "res://addons/mod_loader/mod_loader_store.gd"),
            ("ModLoader", "res://addons/mod_loader/mod_loader.gd"),
        ]);
        let keys: Vec<&str> = pb.entries.iter().map(|(k, _)| k.as_str()).collect();
        let store = keys.iter().position(|k| *k == "autoload/ModLoaderStore").unwrap();
        let core = keys.iter().position(|k| *k == "autoload/ModLoader").unwrap();
        let game = keys.iter().position(|k| *k == "autoload/E").unwrap();
        assert!(store < core && core < game, "store, then core, then game autoloads: {keys:?}");

        // Re-parse to confirm the value decodes to the enabled-singleton string.
        let re = ProjectBinary::parse(&pb.to_bytes()).unwrap();
        let (_, val) = re.entries.iter().find(|(k, _)| k == "autoload/ModLoader").unwrap();
        assert_eq!(
            decode(val, &mut 0usize),
            Some(GVal::Str("*res://addons/mod_loader/mod_loader.gd".into()))
        );
    }

    #[test]
    fn merges_classes_keeping_existing() {
        let mut pb = ProjectBinary::parse(&sample()).unwrap();
        pb.merge_global_classes(&[
            GlobalClass { base: "Node".into(), class: "ModLoaderLog".into(), path: "res://addons/mod_loader/api/log.gd".into() },
            // "Editor" already exists in the sample — must not be duplicated.
            GlobalClass { base: "Node".into(), class: "Editor".into(), path: "res://dupe.gd".into() },
        ]);

        let re = ProjectBinary::parse(&pb.to_bytes()).unwrap();
        let (_, arr_bytes) = re.entries.iter().find(|(k, _)| k == "_global_script_classes").unwrap();
        let arr = match decode(arr_bytes, &mut 0usize) {
            Some(GVal::Array(a)) => a,
            _ => panic!("array"),
        };
        let names: Vec<String> = arr.iter().filter_map(class_name_of).collect();
        assert_eq!(names, vec!["Editor".to_string(), "ModLoaderLog".to_string()]);

        let (_, icons_bytes) = re.entries.iter().find(|(k, _)| k == "_global_script_class_icons").unwrap();
        if let Some(GVal::Dict(pairs)) = decode(icons_bytes, &mut 0usize) {
            let icon_names: Vec<String> = pairs
                .iter()
                .filter_map(|(k, _)| if let GVal::Str(s) = k { Some(s.clone()) } else { None })
                .collect();
            assert_eq!(icon_names, vec!["Editor".to_string(), "ModLoaderLog".to_string()]);
        } else {
            panic!("icons dict");
        }
    }

    #[test]
    fn patching_twice_is_idempotent() {
        let mut pb = ProjectBinary::parse(&sample()).unwrap();
        let classes = [GlobalClass {
            base: "Node".into(),
            class: "ModLoaderLog".into(),
            path: "res://addons/mod_loader/api/log.gd".into(),
        }];
        let autoloads = [("ModLoaderStore", "res://addons/mod_loader/mod_loader_store.gd")];

        pb.add_autoloads(&autoloads);
        pb.merge_global_classes(&classes);
        let once = pb.to_bytes();

        let mut pb2 = ProjectBinary::parse(&once).unwrap();
        pb2.add_autoloads(&autoloads);
        pb2.merge_global_classes(&classes);
        assert_eq!(pb2.to_bytes(), once, "second identical patch changes nothing");
    }

    #[test]
    fn rejects_non_ecfg() {
        assert!(ProjectBinary::parse(b"not a project binary").is_none());
    }
}
