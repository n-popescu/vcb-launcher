//! Install / back up / restore the game's `vcb.pck`.
//!
//! Installing a mod copies its `.pck` over `<game>/vcb.pck`, keeping the game executable
//! untouched. The first time we touch a *vanilla* `vcb.pck` we copy it aside to
//! `vcb.pck.original` so "Restore vanilla" always has a real restore point.

use crate::archive;
use crate::meta;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hasher;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PCK_NAME: &str = "vcb.pck";
pub const BACKUP_NAME: &str = "vcb.pck.original";

pub fn pck_path(game_dir: &Path) -> PathBuf {
    game_dir.join(PCK_NAME)
}
pub fn backup_path(game_dir: &Path) -> PathBuf {
    game_dir.join(BACKUP_NAME)
}
pub fn has_backup(game_dir: &Path) -> bool {
    backup_path(game_dir).is_file()
}

/// A `.pck` is "vanilla" if it carries no mod metadata.
pub fn is_vanilla(pck: &Path) -> bool {
    meta::read(pck).is_none()
}

/// Snapshot the current `vcb.pck` to the one-time vanilla backup, but only if it's a
/// genuine vanilla pack and we don't already have a backup.
fn snapshot_vanilla_if_needed(game_dir: &Path) -> io::Result<()> {
    let pck = pck_path(game_dir);
    let backup = backup_path(game_dir);
    if !backup.exists() && pck.is_file() && is_vanilla(&pck) {
        fs::copy(&pck, &backup)?;
    }
    Ok(())
}

/// Install `mod_pck` (a `.pck` on disk) as the active `vcb.pck`. Preserves a one-time
/// vanilla backup.
pub fn install(game_dir: &Path, mod_pck: &Path) -> io::Result<()> {
    snapshot_vanilla_if_needed(game_dir)?;
    fs::copy(mod_pck, pck_path(game_dir))?;
    Ok(())
}

/// Install a **zipped mod**: extract the `.pck` bundled inside `zip_path` over the game's
/// `vcb.pck`. Preserves the same one-time vanilla backup as [`install`].
pub fn install_zip(game_dir: &Path, zip_path: &Path) -> io::Result<()> {
    snapshot_vanilla_if_needed(game_dir)?;
    archive::extract_pck_to(zip_path, &pck_path(game_dir))?;
    Ok(())
}

/// Restore the vanilla `vcb.pck` from the backup. Errors if no backup exists.
pub fn restore(game_dir: &Path) -> io::Result<()> {
    let backup = backup_path(game_dir);
    if !backup.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no vanilla backup (vcb.pck.original) exists yet",
        ));
    }
    fs::copy(&backup, pck_path(game_dir))?;
    Ok(())
}

/// How many bytes we sample from each end when fingerprinting.
const SAMPLE: u64 = 256 * 1024;

fn hash_parts(len: u64, head: &[u8], tail: Option<&[u8]>) -> u64 {
    let mut h = DefaultHasher::new();
    h.write_u64(len);
    h.write(head);
    if let Some(t) = tail {
        h.write(t);
    }
    h.finish()
}

/// Cheap content signature (length + sampled head/tail bytes) used to tell which mod is
/// currently installed without hashing tens of MB in full.
pub fn fingerprint(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    let len = meta.len();
    let mut f = fs::File::open(path).ok()?;

    let mut head = vec![0u8; SAMPLE.min(len) as usize];
    f.read_exact(&mut head).ok()?;

    let tail = if len > SAMPLE {
        use std::io::{Seek, SeekFrom};
        f.seek(SeekFrom::End(-(SAMPLE as i64))).ok()?;
        let mut buf = vec![0u8; SAMPLE as usize];
        f.read_exact(&mut buf).ok()?;
        Some(buf)
    } else {
        None
    };
    Some(hash_parts(len, &head, tail.as_deref()))
}

/// Same signature as [`fingerprint`], computed over bytes already in memory (the
/// decompressed `.pck` from inside a zipped mod). Kept byte-identical to [`fingerprint`]
/// so a zipped mod matches the file it installs as.
pub fn fingerprint_bytes(data: &[u8]) -> u64 {
    let len = data.len() as u64;
    let head = &data[..SAMPLE.min(len) as usize];
    let tail = if len > SAMPLE {
        Some(&data[data.len() - SAMPLE as usize..])
    } else {
        None
    };
    hash_parts(len, head, tail)
}

/// What the game's current `vcb.pck` matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Active {
    Missing,
    Vanilla,
    Mod(u64), // fingerprint of the installed pck
    Unknown,
}

pub fn active_state(game_dir: &Path) -> Active {
    let pck = pck_path(game_dir);
    if !pck.is_file() {
        return Active::Missing;
    }
    if is_vanilla(&pck) {
        return Active::Vanilla;
    }
    match fingerprint(&pck) {
        Some(fp) => Active::Mod(fp),
        None => Active::Unknown,
    }
}

// --- launching the game --------------------------------------------------------------
// The launcher always runs the ORIGINAL game executable (the one with the correct, closed
// simulation engine); it only swaps which vcb.pck sits next to it. Whatever mod is
// currently installed is what launches.
pub fn launch_game(game_dir: &Path) -> io::Result<()> {
    let mut cmd = build_launch_command(game_dir).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "no vcb executable found in the game folder")
    })?;
    cmd.current_dir(game_dir);
    cmd.spawn()?;
    Ok(())
}

#[cfg(windows)]
fn build_launch_command(game_dir: &Path) -> Option<Command> {
    let exe = game_dir.join("vcb.exe");
    if exe.is_file() {
        return Some(Command::new(exe));
    }
    None
}

#[cfg(target_os = "linux")]
fn build_launch_command(game_dir: &Path) -> Option<Command> {
    for name in ["vcb.x86_64", "vcb"] {
        let p = game_dir.join(name);
        if p.is_file() {
            return Some(Command::new(p));
        }
    }
    // Wine users run the original Windows build (the one with the correct engine).
    let win = game_dir.join("vcb.exe");
    if win.is_file() {
        let mut c = Command::new("wine");
        c.arg(win);
        return Some(c);
    }
    None
}

#[cfg(target_os = "macos")]
fn build_launch_command(game_dir: &Path) -> Option<Command> {
    let bare = game_dir.join("vcb");
    if bare.is_file() {
        return Some(Command::new(bare));
    }
    let win = game_dir.join("vcb.exe");
    if win.is_file() {
        let mut c = Command::new("wine");
        c.arg(win);
        return Some(c);
    }
    None
}
