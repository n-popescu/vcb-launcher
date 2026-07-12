// Dependency-free rasteriser for the launcher's icon artwork.
//
// This module has NO external dependencies on purpose: it is compiled into the crate (so
// `icon.rs` can hand the pixels to egui as the runtime window/taskbar icon) and also
// `include!`d by `build.rs` and the `gen-icons` bin (so the same art is baked into the
// executable at build time — the Windows `.exe` icon, the macOS `.icns`, the Linux desktop
// `.png`). Keeping it free of `eframe`/`egui` is what lets the build script use it, and it
// must use plain `//` comments (not `//!`) so it stays valid when `include!`d. The motif is
// the app's "circuit chip with a lit via", drawn size-parametrically so one source renders
// every icon resolution.

// Palette (kept in sync with the constants in main.rs / icon.rs).
pub const ACCENT: [u8; 3] = [0x3b, 0xd1, 0x9e];
pub const ACCENT_DIM: [u8; 3] = [0x25, 0x7a, 0x60];
pub const CARD: [u8; 3] = [0x22, 0x29, 0x31];
pub const PANEL: [u8; 3] = [0x12, 0x16, 0x1a];

/// Render the icon into a straight-alpha RGBA8 buffer of `n`x`n` pixels on a transparent
/// background. The geometry is authored at 128 px and scaled by `n/128`, so every size
/// (16…256) is the same picture. `n` should be >= 8.
pub fn render_rgba(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n * n * 4]; // fully transparent to start
    let f = n as f32 / 128.0; // scale factor from the authored 128px geometry
    let c = n as f32 / 2.0;
    let s = |v: f32| v * f; // scale a length

    // Chip pins (behind the body), three on each side.
    for k in -1..=1 {
        let off = k as f32 * s(20.0);
        // left / right
        paint_rrect(&mut buf, n, s(25.0), c + off, s(7.0), s(4.0), s(2.0), ACCENT_DIM, 1.0);
        paint_rrect(&mut buf, n, n as f32 - s(25.0), c + off, s(7.0), s(4.0), s(2.0), ACCENT_DIM, 1.0);
        // top / bottom
        paint_rrect(&mut buf, n, c + off, s(25.0), s(4.0), s(7.0), s(2.0), ACCENT_DIM, 1.0);
        paint_rrect(&mut buf, n, c + off, n as f32 - s(25.0), s(4.0), s(7.0), s(2.0), ACCENT_DIM, 1.0);
    }

    // Chip body: accent border ring with a dark card interior.
    paint_rrect(&mut buf, n, c, c, s(33.0), s(33.0), s(15.0), ACCENT, 1.0);
    paint_rrect(&mut buf, n, c, c, s(28.0), s(28.0), s(11.0), CARD, 1.0);

    // A couple of traces from the via.
    paint_rrect(&mut buf, n, c + s(13.0), c, s(13.0), s(2.0), s(1.5), ACCENT, 1.0);
    paint_rrect(&mut buf, n, c, c + s(13.0), s(2.0), s(13.0), s(1.5), ACCENT, 1.0);

    // Lit via at the centre: soft glow, solid pad, dark hole.
    paint_circle(&mut buf, n, c, c, s(16.0), ACCENT, 0.16);
    paint_circle(&mut buf, n, c, c, s(8.5), ACCENT, 1.0);
    paint_circle(&mut buf, n, c, c, s(3.5), PANEL, 1.0);

    buf
}

// --- tiny anti-aliased rasteriser (straight/unmultiplied alpha) -----------------------

fn sdf_round_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - r
}

#[allow(clippy::too_many_arguments)]
fn paint_rrect(buf: &mut [u8], n: usize, cx: f32, cy: f32, hw: f32, hh: f32, r: f32, rgb: [u8; 3], a: f32) {
    let (x0, x1) = ((cx - hw - 2.0) as i32, (cx + hw + 2.0) as i32);
    let (y0, y1) = ((cy - hh - 2.0) as i32, (cy + hh + 2.0) as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = sdf_round_rect(x as f32 + 0.5, y as f32 + 0.5, cx, cy, hw, hh, r);
            blend(buf, n, x, y, rgb, a * (0.5 - d).clamp(0.0, 1.0));
        }
    }
}

fn paint_circle(buf: &mut [u8], n: usize, cx: f32, cy: f32, rad: f32, rgb: [u8; 3], a: f32) {
    let (x0, x1) = ((cx - rad - 2.0) as i32, (cx + rad + 2.0) as i32);
    let (y0, y1) = ((cy - rad - 2.0) as i32, (cy + rad + 2.0) as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = (((x as f32 + 0.5) - cx).powi(2) + ((y as f32 + 0.5) - cy).powi(2)).sqrt() - rad;
            blend(buf, n, x, y, rgb, a * (0.5 - d).clamp(0.0, 1.0));
        }
    }
}

fn blend(buf: &mut [u8], n: usize, x: i32, y: i32, rgb: [u8; 3], a: f32) {
    if x < 0 || y < 0 || x >= n as i32 || y >= n as i32 || a <= 0.0 {
        return;
    }
    let i = (y as usize * n + x as usize) * 4;
    let a = a.min(1.0);
    let da = buf[i + 3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    if out_a <= 0.0 {
        return;
    }
    for k in 0..3 {
        let src = rgb[k] as f32;
        let dst = buf[i + k] as f32;
        buf[i + k] = ((src * a + dst * da * (1.0 - a)) / out_a).round() as u8;
    }
    buf[i + 3] = (out_a * 255.0).round() as u8;
}
