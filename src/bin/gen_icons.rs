// Emit the launcher's icon as image files, rendered from the same procedural source as the
// runtime icon and the embedded Windows resource (`src/icon_render.rs`, pulled in with
// `include!` so there is still no committed image). CI uses these to give the macOS `.app`
// an `.icns` (via `iconutil -c icns` on the emitted `.iconset`) and the Linux build a
// desktop `.png`. Windows doesn't need this — its icon is embedded by `build.rs`.
//
//   cargo run --release --bin gen-icons -- <out-dir>   (defaults to ./dist/icon)

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/icon_render.rs"));

use std::fs;
use std::io::BufWriter;
use std::path::Path;

fn write_png(path: &Path, n: usize) {
    let file = fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    let mut enc = png::Encoder::new(BufWriter::new(file), n as u32, n as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&render_rgba(n)))
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "dist/icon".to_string());
    let out = Path::new(&out);
    fs::create_dir_all(out).expect("create out dir");

    // Generic / Linux desktop icon.
    write_png(&out.join("vcb-launcher.png"), 256);

    // macOS iconset: `iconutil -c icns "VCB Mod Launcher.iconset"` turns this into an .icns.
    let iconset = out.join("VCB Mod Launcher.iconset");
    fs::create_dir_all(&iconset).expect("create iconset dir");
    for &(name, n) in &[
        ("icon_16x16.png", 16usize), ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32), ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128), ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256), ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512), ("icon_512x512@2x.png", 1024),
    ] {
        write_png(&iconset.join(name), n);
    }

    println!("wrote icon files to {}", out.display());
}
