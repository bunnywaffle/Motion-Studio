//! Warp lattice model shared by every pipeline and UI surface.
//!
//! A warp effect carries a fixed [`WARP_GRID`] x [`WARP_GRID`] pin lattice
//! over the layer content box. Each pin stores a pixel offset; sampling
//! bilinearly interpolates offsets (AE Mesh Warp behaviour: dragging a pin
//! drags nearby pixels, identity pins are a no-op). Pins live in layer px
//! so raster, overlay, and export agree without conversions.

use crate::vec2::Vec2;
use serde::{Deserialize, Serialize};

/// Pins per lattice side (4x4 grid, row-major storage).
pub const WARP_GRID: usize = 4;

/// Pin count for a full lattice.
pub const WARP_PIN_COUNT: usize = WARP_GRID * WARP_GRID;

/// One lattice control point: pixel offset from its rest position.
/// Rest positions are implicit (`pin_base`), so an all-zero vec is the
/// identity warp and old project files (no `pins` key) load as identity.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct WarpPin {
    #[serde(default)]
    pub dx: f32,
    #[serde(default)]
    pub dy: f32,
}

impl WarpPin {
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    pub const fn is_identity(self) -> bool {
        self.dx == 0.0 && self.dy == 0.0
    }
}

/// Rest position of pin (`col`, `row`) inside a content box with `origin`
/// (layer-local) and `size` (content dims).
pub fn pin_base(col: usize, row: usize, origin: Vec2, size: Vec2) -> Vec2 {
    let span = (WARP_GRID - 1).max(1) as f32;
    Vec2::new(
        origin.x + col.min(WARP_GRID - 1) as f32 / span * size.x,
        origin.y + row.min(WARP_GRID - 1) as f32 / span * size.y,
    )
}

/// Row-major index of pin (`col`, `row`), clamped into the grid.
pub fn pin_index(col: usize, row: usize) -> usize {
    let c = if col < WARP_GRID { col } else { WARP_GRID - 1 };
    let r = if row < WARP_GRID { row } else { WARP_GRID - 1 };
    r * WARP_GRID + c
}

/// Bilinearly interpolated pin offset at normalized content position
/// (`u`, `v` in 0..1, clamped). Short grids reuse the last pin; an empty
/// or short slice behaves as identity for the missing entries.
pub fn sample_offset(pins: &[WarpPin], u: f32, v: f32) -> (f32, f32) {
    let at = |col: usize, row: usize| -> (f32, f32) {
        pins
            .get(pin_index(col, row))
            .map(|p| (p.dx, p.dy))
            .unwrap_or((0.0, 0.0))
    };
    let span = (WARP_GRID - 1).max(1) as f32;
    let gx = u.clamp(0.0, 1.0) * span;
    let gy = v.clamp(0.0, 1.0) * span;
    let (cx, cy) = (gx.floor() as usize, gy.floor() as usize);
    let (fx, fy) = (gx - cx as f32, gy - cy as f32);
    let (ax, ay) = at(cx, cy);
    let (bx, by) = at(cx + 1, cy);
    let (cx_, cy_) = at(cx, cy + 1);
    let (dx, dy) = at(cx + 1, cy + 1);
    let top_x = ax * (1.0 - fx) + bx * fx;
    let top_y = ay * (1.0 - fx) + by * fx;
    let bot_x = cx_ * (1.0 - fx) + dx * fx;
    let bot_y = cy_ * (1.0 - fx) + dy * fx;
    (
        top_x * (1.0 - fy) + bot_x * fy,
        top_y * (1.0 - fy) + bot_y * fy,
    )
}

/// Largest absolute pin offset (bounds growth for render boxes).
pub fn max_offset(pins: &[WarpPin]) -> f32 {
    pins.iter().fold(0.0f32, |m, p| {
        m.max(p.dx.abs()).max(p.dy.abs())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin_grid(dx: f32, dy: f32) -> Vec<WarpPin> {
        vec![WarpPin::new(dx, dy); WARP_PIN_COUNT]
    }

    #[test]
    fn identity_pins_sample_zero_everywhere() {
        let pins = vec![WarpPin::default(); WARP_PIN_COUNT];
        assert_eq!(sample_offset(&pins, 0.0, 0.0), (0.0, 0.0));
        assert_eq!(sample_offset(&pins, 0.37, 0.71), (0.0, 0.0));
        assert_eq!(sample_offset(&pins, 1.0, 1.0), (0.0, 0.0));
        assert_eq!(sample_offset(&[], 0.5, 0.5), (0.0, 0.0));
    }

    #[test]
    fn single_pin_falls_off_bilinearly() {
        // Bottom-right pin pushed +60x: exact at the corner, half one
        // cell in, zero at the opposite corner.
        let mut pins = vec![WarpPin::default(); WARP_PIN_COUNT];
        pins[pin_index(3, 3)] = WarpPin::new(60.0, 0.0);
        let (x, _) = sample_offset(&pins, 1.0, 1.0);
        assert!((x - 60.0).abs() < 1e-5, "{x}");
        let (mid, _) = sample_offset(&pins, 5.0 / 6.0, 1.0);
        assert!((mid - 30.0).abs() < 1e-4, "{mid}");
        let (far, _) = sample_offset(&pins, 0.0, 0.0);
        assert!(far.abs() < 1e-6, "{far}");
    }

    #[test]
    fn uniform_grid_samples_its_own_offset() {
        let (x, y) = sample_offset(&pin_grid(8.0, -4.0), 0.25, 0.75);
        assert!((x - 8.0).abs() < 1e-5 && (y + 4.0).abs() < 1e-5, "{x} {y}");
    }

    #[test]
    fn pin_base_spans_box_corners() {
        let o = Vec2::new(10.0, 20.0);
        let s = Vec2::new(100.0, 200.0);
        assert_eq!(pin_base(0, 0, o, s), Vec2::new(10.0, 20.0));
        assert_eq!(pin_base(3, 3, o, s), Vec2::new(110.0, 220.0));
        let mid = pin_base(1, 2, o, s);
        assert!((mid.x - (10.0 + 100.0 / 3.0)).abs() < 1e-5, "{mid:?}");
    }
}
