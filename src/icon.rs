//! The launcher's custom artwork, generated procedurally so the app stays a single
//! self-contained binary with no image files to ship.
//!
//! [`app_icon`] wraps the shared, dependency-free rasteriser in [`crate::icon_render`] for
//! egui's runtime window / taskbar icon. The *same* rasteriser is used at build time by
//! `build.rs` to bake the icon into the executable itself (the Windows `.exe` icon, the
//! macOS `.icns`, the Linux desktop `.png`) so the app shows its icon before it is even
//! launched. The matching in-app header logo is drawn as vectors in `main.rs`
//! (`paint_logo`); all three use the app's accent palette and the same "circuit chip with a
//! lit via" motif.

use crate::icon_render;
use eframe::egui;

const N: usize = 128;

/// The window / taskbar icon: a circuit chip with pins and a lit central via, on a
/// transparent background.
pub fn app_icon() -> egui::IconData {
    egui::IconData {
        rgba: icon_render::render_rgba(N),
        width: N as u32,
        height: N as u32,
    }
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
