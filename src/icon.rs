//! The launcher's custom artwork, generated procedurally so the app stays a single
//! self-contained binary with no image files to ship.
//!
//! [`app_icon`] rasterises the window / taskbar icon; the matching in-app header logo is
//! drawn as vectors in `main.rs` (`paint_logo`). Both use the app's accent palette and the
//! same "circuit chip with a lit via" motif.

use eframe::egui;

// Palette (kept in sync with the constants in main.rs).
const ACCENT: [u8; 3] = [0x3b, 0xd1, 0x9e];
const ACCENT_DIM: [u8; 3] = [0x25, 0x7a, 0x60];
const CARD: [u8; 3] = [0x22, 0x29, 0x31];
const PANEL: [u8; 3] = [0x12, 0x16, 0x1a];

const N: usize = 128;

/// The window / taskbar icon: a circuit chip with pins and a lit central via, on a
/// transparent background.
pub fn app_icon() -> egui::IconData {
    let mut buf = vec![0u8; N * N * 4]; // fully transparent to start
    let c = N as f32 / 2.0;

    // Chip pins (behind the body), three on each side.
    for k in -1..=1 {
        let off = k as f32 * 20.0;
        // left / right
        paint_rrect(&mut buf, 25.0, c + off, 7.0, 4.0, 2.0, ACCENT_DIM, 1.0);
        paint_rrect(&mut buf, N as f32 - 25.0, c + off, 7.0, 4.0, 2.0, ACCENT_DIM, 1.0);
        // top / bottom
        paint_rrect(&mut buf, c + off, 25.0, 4.0, 7.0, 2.0, ACCENT_DIM, 1.0);
        paint_rrect(&mut buf, c + off, N as f32 - 25.0, 4.0, 7.0, 2.0, ACCENT_DIM, 1.0);
    }

    // Chip body: accent border ring with a dark card interior.
    paint_rrect(&mut buf, c, c, 33.0, 33.0, 15.0, ACCENT, 1.0);
    paint_rrect(&mut buf, c, c, 28.0, 28.0, 11.0, CARD, 1.0);

    // A couple of traces from the via.
    paint_rrect(&mut buf, c + 13.0, c, 13.0, 2.0, 1.5, ACCENT, 1.0);
    paint_rrect(&mut buf, c, c + 13.0, 2.0, 13.0, 1.5, ACCENT, 1.0);

    // Lit via at the centre: soft glow, solid pad, dark hole.
    paint_circle(&mut buf, c, c, 16.0, ACCENT, 0.16);
    paint_circle(&mut buf, c, c, 8.5, ACCENT, 1.0);
    paint_circle(&mut buf, c, c, 3.5, PANEL, 1.0);

    egui::IconData {
        rgba: buf,
        width: N as u32,
        height: N as u32,
    }
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
fn paint_rrect(buf: &mut [u8], cx: f32, cy: f32, hw: f32, hh: f32, r: f32, rgb: [u8; 3], a: f32) {
    let (x0, x1) = ((cx - hw - 2.0) as i32, (cx + hw + 2.0) as i32);
    let (y0, y1) = ((cy - hh - 2.0) as i32, (cy + hh + 2.0) as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = sdf_round_rect(x as f32 + 0.5, y as f32 + 0.5, cx, cy, hw, hh, r);
            blend(buf, x, y, rgb, a * (0.5 - d).clamp(0.0, 1.0));
        }
    }
}

fn paint_circle(buf: &mut [u8], cx: f32, cy: f32, rad: f32, rgb: [u8; 3], a: f32) {
    let (x0, x1) = ((cx - rad - 2.0) as i32, (cx + rad + 2.0) as i32);
    let (y0, y1) = ((cy - rad - 2.0) as i32, (cy + rad + 2.0) as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = (((x as f32 + 0.5) - cx).powi(2) + ((y as f32 + 0.5) - cy).powi(2)).sqrt() - rad;
            blend(buf, x, y, rgb, a * (0.5 - d).clamp(0.0, 1.0));
        }
    }
}

fn blend(buf: &mut [u8], x: i32, y: i32, rgb: [u8; 3], a: f32) {
    if x < 0 || y < 0 || x >= N as i32 || y >= N as i32 || a <= 0.0 {
        return;
    }
    let i = (y as usize * N + x as usize) * 4;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_expected_shape() {
        let icon = app_icon();
        assert_eq!(icon.width, N as u32);
        assert_eq!(icon.height, N as u32);
        assert_eq!(icon.rgba.len(), N * N * 4);
        // The lit via at the centre must be opaque.
        let i = ((N / 2) * N + N / 2) * 4;
        assert_eq!(icon.rgba[i + 3], 255);
    }
}
