// Bake the launcher's procedural icon into the executable so it shows on the app *before*
// it is launched, not just in the running window/taskbar.
//
// The icon art comes from the exact same rasteriser the app uses at runtime
// (`src/icon_render.rs`), pulled in here with `include!` so there is still no image file to
// ship — it is generated at build time. On Windows this embeds a multi-resolution `.ico`
// as the executable's icon resource (what Explorer shows). A bare macOS/Linux binary can't
// carry a file icon, so there is nothing to embed there — macOS uses a `.app` bundle and
// Linux a `.desktop` file, both produced from the `gen-icons` helper in CI; we still emit
// the `.ico` into OUT_DIR on every target for reuse.

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/icon_render.rs"));

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/icon_render.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let ico_path = Path::new(&out_dir).join("vcb-launcher.ico");
    write_ico(&ico_path);
    embed_windows_icon(&ico_path);
}

/// Encode the icon as a multi-resolution `.ico` (16..256 px) from the procedural source.
fn write_ico(path: &Path) {
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for &n in &[16usize, 24, 32, 48, 64, 128, 256] {
        let image = ico::IconImage::from_rgba_data(n as u32, n as u32, render_rgba(n));
        dir.add_entry(ico::IconDirEntry::encode(&image).expect("encode .ico entry"));
    }
    let file = std::fs::File::create(path).expect("create .ico");
    dir.write(file).expect("write .ico");
}

// `winresource` is a Windows-host build-dependency (see Cargo.toml), so gate the reference
// to host-Windows compilation. CI builds the Windows target natively on a Windows runner,
// where host == target, so this runs exactly when the .exe is being produced.
#[cfg(windows)]
fn embed_windows_icon(ico_path: &Path) {
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico_path.to_str().expect("utf-8 ico path"));
    if let Err(e) = res.compile() {
        // Don't fail the build over the cosmetic icon; warn so it's visible in the log.
        println!("cargo:warning=could not embed the Windows icon resource: {e}");
    }
}

#[cfg(not(windows))]
fn embed_windows_icon(_ico_path: &Path) {}
