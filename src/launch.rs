//! Launch the game.
//!
//! The launcher always runs the ORIGINAL game executable (the one with the correct, closed
//! simulation engine); it only swaps which `vcb.pck` sits next to it (see `patch.rs`). Whatever
//! is currently installed is what launches.

use std::io;
use std::path::Path;
use std::process::Command;

pub fn launch_game(game_dir: &Path) -> io::Result<()> {
    let mut cmd = build_launch_command(game_dir)?;
    cmd.current_dir(game_dir);
    cmd.spawn()?;
    Ok(())
}

#[cfg(windows)]
fn build_launch_command(game_dir: &Path) -> io::Result<Command> {
    let exe = game_dir.join("vcb.exe");
    if exe.is_file() {
        return Ok(Command::new(exe));
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "vcb.exe not found in the game folder"))
}

#[cfg(target_os = "linux")]
fn build_launch_command(game_dir: &Path) -> io::Result<Command> {
    for name in ["vcb.x86_64", "vcb"] {
        let p = game_dir.join(name);
        if p.is_file() {
            return Ok(Command::new(p));
        }
    }
    // Wine users run the original Windows build (the one with the correct engine).
    let win = game_dir.join("vcb.exe");
    if win.is_file() {
        let mut c = Command::new("wine");
        c.arg(win);
        return Ok(c);
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "no vcb executable found in the game folder"))
}

#[cfg(target_os = "macos")]
fn build_launch_command(game_dir: &Path) -> io::Result<Command> {
    use std::path::PathBuf;
    // A native macOS build (rare) runs directly.
    let bare = game_dir.join("vcb");
    if bare.is_file() {
        return Ok(Command::new(bare));
    }
    // Otherwise run the original Windows build through Wine. IMPORTANT: when the launcher is
    // opened as a .app (double-clicked in Finder) it inherits only a minimal PATH — typically
    // /usr/bin:/bin:/usr/sbin:/sbin — which excludes Homebrew (/opt/homebrew/bin, /usr/local/bin)
    // and MacPorts. So a bare `Command::new("wine")` fails with "No such file or directory"
    // (os error 2) even though Wine is installed and on the user's interactive PATH — which is
    // why launching worked from a terminal but not from the .app. Resolve Wine to an absolute
    // path ourselves, and put its directory on the child's PATH so it can find wineserver etc.
    let win = game_dir.join("vcb.exe");
    if win.is_file() {
        let wine = find_wine().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Wine isn't installed or couldn't be found on PATH. Install it \
                 (e.g. `brew install --cask wine-stable`) and try again.",
            )
        })?;
        let mut c = Command::new(&wine);
        c.arg(win);
        if let Some(dir) = wine.parent() {
            prepend_to_path(&mut c, dir);
        }
        return Ok(c);
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "no vcb executable found in the game folder"))
}

// Locate a Wine executable as an absolute path. Searches the inherited PATH first (so a
// terminal-launched run uses exactly the `wine` on the user's PATH), then the standard install
// locations a GUI-launched .app won't have on its minimal PATH.
#[cfg(target_os = "macos")]
fn find_wine() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    const NAMES: [&str; 4] = ["wine", "wine64", "wine-stable", "wine-development"];

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in NAMES {
                let cand = dir.join(name);
                if is_executable_file(&cand) {
                    return Some(cand);
                }
            }
        }
    }

    const COMMON_DIRS: [&str; 3] = [
        "/opt/homebrew/bin", // Homebrew (Apple Silicon)
        "/usr/local/bin",    // Homebrew (Intel)
        "/opt/local/bin",    // MacPorts
    ];
    for dir in COMMON_DIRS {
        for name in NAMES {
            let cand = Path::new(dir).join(name);
            if is_executable_file(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

// Prepend `dir` to the child process's PATH (Wine looks up its own helper binaries — wineserver,
// wine64-preloader, … — via PATH, and the .app's minimal PATH usually lacks Wine's directory).
#[cfg(target_os = "macos")]
fn prepend_to_path(cmd: &mut Command, dir: &Path) {
    use std::path::PathBuf;
    let mut dirs: Vec<PathBuf> = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&existing));
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        cmd.env("PATH", joined);
    }
}
