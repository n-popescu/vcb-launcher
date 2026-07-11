//! Zipped-mod support.
//!
//! A zipped mod is a `.zip` bundling the mod's Godot `.pck` (the one that gets installed
//! as `vcb.pck`) together with a `mod.json` describing it, e.g.:
//!
//! ```text
//! multiplayer.zip
//! ├── vcb.pck        # or any *.pck
//! └── mod.json
//! ```
//!
//! The launcher reads the metadata straight from the zip and, on activation, extracts the
//! bundled `.pck` over the game's `vcb.pck` — the folder-per-mod and bare-`.pck` layouts
//! still work exactly as before; this is just a third accepted shape.

use crate::meta::{self, ModMeta};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use zip::result::ZipError;
use zip::ZipArchive;

fn zip_err(e: ZipError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("zip: {e}"))
}

/// True if `path` looks like a zip by extension.
pub fn is_zip(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
}

/// The in-zip name of the `.pck` to install, preferring one at the archive root. `None`
/// when the zip has no `.pck` (so it isn't a mod we can install).
pub fn pck_entry_name(zip_path: &Path) -> Option<String> {
    let file = File::open(zip_path).ok()?;
    let mut zip = ZipArchive::new(file).ok()?;
    let mut nested: Option<String> = None;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).ok()?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.to_lowercase().ends_with(".pck") {
            if !name.contains('/') {
                return Some(name); // a root-level .pck wins
            }
            nested.get_or_insert(name);
        }
    }
    nested
}

fn read_entry_bytes(zip_path: &Path, name: &str) -> Option<Vec<u8>> {
    let file = File::open(zip_path).ok()?;
    let mut zip = ZipArchive::new(file).ok()?;
    let mut ef = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    ef.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// In-zip name of a `mod.json`, preferring one at the archive root.
fn mod_json_name(zip: &mut ZipArchive<File>) -> Option<String> {
    let mut nested: Option<String> = None;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).ok()?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let base = name.rsplit('/').next().unwrap_or("");
        if base.eq_ignore_ascii_case("mod.json") {
            if !name.contains('/') {
                return Some(name);
            }
            nested.get_or_insert(name);
        }
    }
    nested
}

/// Read a zipped mod's metadata: first from a `mod.json` in the zip (the documented
/// layout), else from a `res://mod.json` embedded in the bundled `.pck`.
pub fn read_meta(zip_path: &Path) -> Option<ModMeta> {
    let file = File::open(zip_path).ok()?;
    let mut zip = ZipArchive::new(file).ok()?;

    if let Some(name) = mod_json_name(&mut zip) {
        if let Ok(mut ef) = zip.by_name(&name) {
            let mut buf = Vec::new();
            if ef.read_to_end(&mut buf).is_ok() {
                if let Ok(m) = serde_json::from_slice::<ModMeta>(&buf) {
                    return Some(m);
                }
            }
        }
    }

    // Fall back to metadata embedded inside the bundled .pck.
    let pck = pck_entry_name(zip_path)?;
    let bytes = read_entry_bytes(zip_path, &pck)?;
    if let Ok(Some(json)) = crate::pck::extract_from_bytes(&bytes, meta::EMBED_PATH) {
        if let Ok(m) = serde_json::from_slice::<ModMeta>(&json) {
            return Some(m);
        }
    }
    None
}

/// Fingerprint the `.pck` bundled inside `zip_path`, matching [`crate::install::fingerprint`]
/// of the file it would install as (so "currently installed" detection works for zips too).
pub fn pck_fingerprint(zip_path: &Path) -> Option<u64> {
    let name = pck_entry_name(zip_path)?;
    let bytes = read_entry_bytes(zip_path, &name)?;
    Some(crate::install::fingerprint_bytes(&bytes))
}

/// Extract the bundled `.pck` to `dest` (the game's `vcb.pck`), streaming so we never hold
/// the whole pack in memory.
pub fn extract_pck_to(zip_path: &Path, dest: &Path) -> io::Result<()> {
    let name = pck_entry_name(zip_path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "the zip contains no .pck to install",
        )
    })?;
    let file = File::open(zip_path)?;
    let mut zip = ZipArchive::new(file).map_err(zip_err)?;
    let mut ef = zip.by_name(&name).map_err(zip_err)?;
    let mut out = File::create(dest)?;
    io::copy(&mut ef, &mut out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};
    use zip::CompressionMethod;

    fn make_zip(tag: &str, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("vcbl_ziptest_{}_{}.zip", std::process::id(), tag));
        let file = File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn reads_meta_pck_and_fingerprint_from_zip() {
        let json = br#"{"schema":1,"id":"multiplayer","name":"VCB Multiplayer","version":"2.0.0"}"#;
        // A pck bigger than the fingerprint sample window, to exercise head+tail hashing.
        let pck: Vec<u8> = (0..600_000u32).map(|i| (i % 251) as u8).collect();
        let zip = make_zip("ok", &[("mod.json", json), ("vcb.pck", &pck)]);

        assert!(is_zip(&zip));
        assert_eq!(pck_entry_name(&zip).as_deref(), Some("vcb.pck"));

        let meta = read_meta(&zip).expect("metadata");
        assert_eq!(meta.name, "VCB Multiplayer");
        assert_eq!(meta.version, "2.0.0");

        // The zip's fingerprint must equal the fingerprint of the file it installs as.
        let dest = std::env::temp_dir().join(format!("vcbl_ziptest_{}_out.pck", std::process::id()));
        extract_pck_to(&zip, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), pck, "extracted bytes match the bundled pck");
        assert_eq!(pck_fingerprint(&zip), Some(crate::install::fingerprint(&dest).unwrap()));

        let _ = std::fs::remove_file(&zip);
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn zip_without_pck_is_not_a_mod() {
        let zip = make_zip("nopck", &[("readme.txt", b"hello")]);
        assert!(pck_entry_name(&zip).is_none());
        let _ = std::fs::remove_file(&zip);
    }
}
