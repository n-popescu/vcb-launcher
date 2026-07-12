//! Read the full directory of a Godot 3.x `.pck`, and write a new one.
//!
//! Used to patch the game's `vcb.pck`: we read every entry, copy the original file
//! bytes through **verbatim** (so the game's encrypted `.gdc` scripts stay exactly as
//! shipped — no decryption key needed), add the Mod Loader's files, and swap in a
//! patched `project.binary`.
//!
//! ## Format (pack format version 1 — Godot 3.x)
//! ```text
//! header  : magic "GDPC" (u32), version (u32=1), ver_major, ver_minor, ver_patch,
//!           16×u32 reserved, file_count (u32)                     [88 bytes total]
//! entry   : path_len (u32, includes NUL padding), path bytes (padded to 4),
//!           offset (u64, absolute), size (u64), md5[16]
//! data    : each file's bytes at its absolute offset
//! ```
//! The engine reads paths by their stored length and stops at the first NUL, so the
//! 4-byte NUL padding is invisible to it; offsets are absolute from the file start.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

const PCK_MAGIC: u32 = 0x4350_4447; // 'G','D','P','C' read little-endian
const PACK_FORMAT_VERSION: u32 = 1; // Godot 3.x
const HEADER_SIZE: u64 = 4 + 4 * 4 + 16 * 4 + 4; // magic+versions+reserved+count = 88

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

#[derive(Clone, Debug)]
pub struct PckHeader {
    pub ver_major: u32,
    pub ver_minor: u32,
    pub ver_patch: u32,
}

#[derive(Clone, Debug)]
pub struct PckEntry {
    pub path: String,
    pub offset: u64,
    pub size: u64,
    pub md5: [u8; 16],
}

pub struct Pck {
    pub header: PckHeader,
    pub entries: Vec<PckEntry>,
}

#[cfg(test)]
impl Pck {
    pub fn find(&self, path: &str) -> Option<&PckEntry> {
        self.entries.iter().find(|e| e.path == path)
    }
    pub fn has(&self, path: &str) -> bool {
        self.entries.iter().any(|e| e.path == path)
    }
}

/// Read the directory of a Godot 3.x pck. Returns `Ok(None)` if it's not a v1 pck we
/// understand (e.g. a Godot 4 pack, or not a pck at all).
pub fn read_dir(path: &Path) -> io::Result<Option<Pck>> {
    let f = File::open(path)?;
    let mut r = BufReader::new(f);
    if read_u32(&mut r)? != PCK_MAGIC {
        return Ok(None);
    }
    let version = read_u32(&mut r)?;
    if version != PACK_FORMAT_VERSION {
        return Ok(None); // Godot 4 (v2) etc. — not handled by this patcher
    }
    let ver_major = read_u32(&mut r)?;
    let ver_minor = read_u32(&mut r)?;
    let ver_patch = read_u32(&mut r)?;
    for _ in 0..16 {
        let _ = read_u32(&mut r)?;
    }
    let file_count = read_u32(&mut r)?;
    if file_count > 5_000_000 {
        return Ok(None);
    }

    let mut entries = Vec::with_capacity(file_count as usize);
    for _ in 0..file_count {
        let sl = read_u32(&mut r)? as usize;
        if sl > 4096 {
            return Ok(None);
        }
        let mut pbuf = vec![0u8; sl];
        r.read_exact(&mut pbuf)?;
        while pbuf.last() == Some(&0) {
            pbuf.pop();
        }
        let path = String::from_utf8_lossy(&pbuf).into_owned();
        let offset = read_u64(&mut r)?;
        let size = read_u64(&mut r)?;
        let mut md5 = [0u8; 16];
        r.read_exact(&mut md5)?;
        entries.push(PckEntry { path, offset, size, md5 });
    }

    Ok(Some(Pck {
        header: PckHeader { ver_major, ver_minor, ver_patch },
        entries,
    }))
}

/// Where a file's bytes come from when writing a new pck.
pub enum Source<'a> {
    /// Copy `size` bytes from `offset` in the original pck (keeps encrypted content intact).
    Original { offset: u64, size: u64, md5: [u8; 16] },
    /// New or replacement content held in memory.
    Bytes(&'a [u8]),
}

pub struct OutFile<'a> {
    pub path: String,
    pub source: Source<'a>,
}

fn padded_path_len(path: &str) -> usize {
    let n = path.len();
    n + ((4 - (n % 4)) % 4)
}

fn dir_entry_size(path: &str) -> u64 {
    // path_len(4) + padded path + offset(8) + size(8) + md5(16)
    (4 + padded_path_len(path) + 8 + 8 + 16) as u64
}

/// Write a new v1 pck at `dest`. `Original` sources are streamed from `orig_path`.
pub fn write_pck(
    dest: &Path,
    orig_path: &Path,
    header: &PckHeader,
    files: &[OutFile],
) -> io::Result<()> {
    // Pass 1: lay out the data section right after the directory and assign offsets.
    let dir_size: u64 = files.iter().map(|f| dir_entry_size(&f.path)).sum();
    let data_start = HEADER_SIZE + dir_size;

    struct Placed {
        offset: u64,
        size: u64,
        md5: [u8; 16],
    }
    let mut placed = Vec::with_capacity(files.len());
    let mut cursor = data_start;
    for f in files {
        let (size, md5) = match &f.source {
            Source::Original { size, md5, .. } => (*size, *md5),
            Source::Bytes(b) => (b.len() as u64, [0u8; 16]),
        };
        placed.push(Placed { offset: cursor, size, md5 });
        cursor += size;
    }

    let out = File::create(dest)?;
    let mut w = BufWriter::new(out);

    // --- header ---
    w.write_all(&PCK_MAGIC.to_le_bytes())?;
    w.write_all(&PACK_FORMAT_VERSION.to_le_bytes())?;
    w.write_all(&header.ver_major.to_le_bytes())?;
    w.write_all(&header.ver_minor.to_le_bytes())?;
    w.write_all(&header.ver_patch.to_le_bytes())?;
    for _ in 0..16 {
        w.write_all(&0u32.to_le_bytes())?;
    }
    w.write_all(&(files.len() as u32).to_le_bytes())?;

    // --- directory ---
    for (f, p) in files.iter().zip(&placed) {
        let pb = f.path.as_bytes();
        let pad = (4 - (pb.len() % 4)) % 4;
        w.write_all(&((pb.len() + pad) as u32).to_le_bytes())?;
        w.write_all(pb)?;
        for _ in 0..pad {
            w.write_all(&[0u8])?;
        }
        w.write_all(&p.offset.to_le_bytes())?;
        w.write_all(&p.size.to_le_bytes())?;
        w.write_all(&p.md5)?;
    }

    // --- data ---
    // One shared reader over the original pck, opened only if any file copies from it.
    let mut orig: Option<BufReader<File>> = None;
    for f in files {
        match &f.source {
            Source::Original { offset, size, .. } => {
                let r = match orig {
                    Some(ref mut r) => r,
                    None => orig.insert(BufReader::new(File::open(orig_path)?)),
                };
                r.seek(SeekFrom::Start(*offset))?;
                let mut remaining = *size;
                let mut buf = [0u8; 64 * 1024];
                while remaining > 0 {
                    let want = remaining.min(buf.len() as u64) as usize;
                    r.read_exact(&mut buf[..want])?;
                    w.write_all(&buf[..want])?;
                    remaining -= want as u64;
                }
            }
            Source::Bytes(b) => w.write_all(b)?,
        }
    }

    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("vcbl_pckb_{}_{}", std::process::id(), tag))
    }

    // Minimal helper to build a pck from in-memory files (all `Bytes`, so `orig_path`
    // is never read).
    fn build_original(path: &Path, files: &[(&str, &[u8])]) {
        let header = PckHeader { ver_major: 3, ver_minor: 5, ver_patch: 1 };
        let out: Vec<OutFile> = files
            .iter()
            .map(|(p, b)| OutFile { path: p.to_string(), source: Source::Bytes(b) })
            .collect();
        write_pck(path, path, &header, &out).unwrap();
    }

    #[test]
    fn read_back_written_pck() {
        let p = tmp("rw.pck");
        let files = [
            ("res://project.binary", &b"ECFG-fake"[..]),
            ("res://addons/mod_loader/mod_loader.gd", &b"extends Node\n"[..]),
            ("res://icon.png", &[0u8, 1, 2, 3, 4, 5, 6][..]),
        ];
        build_original(&p, &files);

        let pck = read_dir(&p).unwrap().expect("valid pck");
        assert_eq!(pck.entries.len(), 3);
        assert_eq!(pck.header.ver_major, 3);
        assert_eq!(pck.header.ver_minor, 5);

        // Every file reads back byte-identical via its recorded offset/size.
        let mut f = File::open(&p).unwrap();
        for (name, want) in files {
            let e = pck.find(name).expect(name);
            assert_eq!(e.size, want.len() as u64);
            f.seek(SeekFrom::Start(e.offset)).unwrap();
            let mut got = vec![0u8; e.size as usize];
            f.read_exact(&mut got).unwrap();
            assert_eq!(&got, want, "content of {name}");
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn copies_original_bytes_and_adds_new() {
        // Build an original, then rewrite it copying one entry via Original and adding one.
        let orig = tmp("orig.pck");
        build_original(
            &orig,
            &[("res://keep.bin", &[9u8; 1000][..]), ("res://project.binary", &b"OLD"[..])],
        );
        let src = read_dir(&orig).unwrap().unwrap();
        let keep = src.find("res://keep.bin").unwrap().clone();

        let dest = tmp("dest.pck");
        let new_pb = b"NEWPROJECTBINARY";
        let files = vec![
            OutFile {
                path: "res://keep.bin".into(),
                source: Source::Original { offset: keep.offset, size: keep.size, md5: keep.md5 },
            },
            OutFile { path: "res://project.binary".into(), source: Source::Bytes(new_pb) },
            OutFile { path: "res://addons/mod_loader/mod_loader.gd".into(), source: Source::Bytes(b"extends Node") },
        ];
        write_pck(&dest, &orig, &src.header, &files).unwrap();

        let out = read_dir(&dest).unwrap().unwrap();
        assert_eq!(out.entries.len(), 3);
        let mut f = File::open(&dest).unwrap();

        let e = out.find("res://keep.bin").unwrap();
        f.seek(SeekFrom::Start(e.offset)).unwrap();
        let mut got = vec![0u8; e.size as usize];
        f.read_exact(&mut got).unwrap();
        assert_eq!(got, vec![9u8; 1000], "original bytes copied verbatim");

        let e = out.find("res://project.binary").unwrap();
        f.seek(SeekFrom::Start(e.offset)).unwrap();
        let mut got = vec![0u8; e.size as usize];
        f.read_exact(&mut got).unwrap();
        assert_eq!(&got, new_pb, "project.binary replaced");

        let _ = std::fs::remove_file(&orig);
        let _ = std::fs::remove_file(&dest);
    }
}
