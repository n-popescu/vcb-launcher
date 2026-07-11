//! Minimal reader for the Godot `.pck` package format.
//!
//! A mod is shipped as a Godot `.pck`. Because every installed mod ends up under the
//! game's single `vcb.pck` name, the launcher can't tell mods apart by filename — it reads
//! a `res://mod.json` packed *inside* each `.pck`. This module parses just enough of the
//! PCK directory to extract one file by its `res://` path.
//!
//! Format (Godot 3.x, pack format version 1; a best-effort v2/Godot-4 path is included):
//! header  = magic("GDPC") u32, version u32, ver_major/minor/rev u32, [v2: flags u32 +
//!           file_base u64], 16×u32 reserved, file_count u32
//! entry   = path_len u32, path bytes (NUL-padded to 4), offset u64, size u64, md5[16],
//!           [v2: flags u32]
//! offsets are absolute from the start of a standalone `.pck`.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const PCK_MAGIC: u32 = 0x4350_4447; // bytes 'G','D','P','C' read little-endian

fn read_u32(f: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    f.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(f: &mut impl Read) -> io::Result<u64> {
    let mut b = [0u8; 8];
    f.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

/// Extract the bytes of the packed file at `want` (e.g. `"res://mod.json"`).
/// Returns `Ok(None)` when the file isn't a pck we understand or the entry is absent.
pub fn extract_file(pck_path: &Path, want: &str) -> io::Result<Option<Vec<u8>>> {
    let mut f = File::open(pck_path)?;

    if read_u32(&mut f)? != PCK_MAGIC {
        return Ok(None);
    }
    let version = read_u32(&mut f)?; // pack format version (1 = Godot 3.x, 2 = Godot 4.x)
    let _ver_major = read_u32(&mut f)?;
    let _ver_minor = read_u32(&mut f)?;
    let _ver_rev = read_u32(&mut f)?;
    if version >= 2 {
        let _flags = read_u32(&mut f)?;
        let _file_base = read_u64(&mut f)?;
    }
    for _ in 0..16 {
        let _reserved = read_u32(&mut f)?;
    }
    let file_count = read_u32(&mut f)?;

    // Guard against a corrupt/hostile count blowing up the loop.
    if file_count > 5_000_000 {
        return Ok(None);
    }

    for _ in 0..file_count {
        let sl = read_u32(&mut f)? as usize;
        if sl > 4096 {
            return Ok(None); // implausible path length; bail rather than allocate wildly
        }
        let mut pbuf = vec![0u8; sl];
        f.read_exact(&mut pbuf)?;
        while pbuf.last() == Some(&0) {
            pbuf.pop(); // strip the 4-byte NUL padding Godot writes
        }
        let path = String::from_utf8_lossy(&pbuf).to_string();

        let ofs = read_u64(&mut f)?;
        let size = read_u64(&mut f)?;
        let mut md5 = [0u8; 16];
        f.read_exact(&mut md5)?;
        if version >= 2 {
            let _entry_flags = read_u32(&mut f)?;
        }

        if path == want {
            if size > 64 * 1024 * 1024 {
                return Ok(None); // metadata should be tiny; refuse to read a huge blob
            }
            f.seek(SeekFrom::Start(ofs))?;
            let mut data = vec![0u8; size as usize];
            f.read_exact(&mut data)?;
            return Ok(Some(data));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Build a minimal Godot v1 .pck holding a single file, matching the format the parser
    // (and Godot 3.x) reads: 88-byte header, then one 4-byte-padded entry, then the data.
    fn build_pck(path: &str, data: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&PCK_MAGIC.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // pack format version
        out.extend_from_slice(&3u32.to_le_bytes()); // major
        out.extend_from_slice(&5u32.to_le_bytes()); // minor
        out.extend_from_slice(&1u32.to_le_bytes()); // rev
        for _ in 0..16 {
            out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        }
        out.extend_from_slice(&1u32.to_le_bytes()); // file_count

        let pb = path.as_bytes();
        let pad = (4 - (pb.len() % 4)) % 4;
        let stored_len = (pb.len() + pad) as u32;
        let entry_size = 4 + stored_len as usize + 8 + 8 + 16;
        let data_ofs = (out.len() + entry_size) as u64;

        out.extend_from_slice(&stored_len.to_le_bytes());
        out.extend_from_slice(pb);
        out.extend(std::iter::repeat(0u8).take(pad));
        out.extend_from_slice(&data_ofs.to_le_bytes());
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&[0u8; 16]); // md5 (ignored)
        out.extend_from_slice(data);
        out
    }

    fn temp_file(tag: &str, bytes: &[u8]) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("vcbl_test_{}_{}.pck", std::process::id(), tag));
        let mut f = File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    #[test]
    fn extracts_embedded_json() {
        let json = br#"{"schema":1,"id":"multiplayer","name":"VCB Multiplayer"}"#;
        let pck = build_pck("res://mod.json", json);
        let path = temp_file("embed", &pck);

        let got = extract_file(&path, "res://mod.json").unwrap();
        assert_eq!(got.as_deref(), Some(&json[..]));

        let missing = extract_file(&path, "res://nope.json").unwrap();
        assert!(missing.is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_non_pck() {
        let path = temp_file("nonpck", b"not a pck file at all");
        assert!(extract_file(&path, "res://mod.json").unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }
}
