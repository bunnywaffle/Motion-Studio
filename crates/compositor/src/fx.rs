//! Stock plug-in per-pixel math.
//!
//! Pure functions shared by the CPU preview (`process_color`), adjustment
//! layers, and headless evaluation. Spatial plug-ins (neighbours / position
//! / time) are the identity here and live in `application::raster`; WGSL
//! twins live in `renderer::effect_filters`. All three must agree — see the
//! parity tests at the bottom.

use project::StockPlugin;

/// Per-pixel pointwise stock kernels live in `project::stock` (next to the
/// plug-in registry, shared with the Node Graph engine); rentered here so
/// existing `compositor::fx` call sites keep their paths.
pub use project::stock::{process_color_stock, process_color_stock_resolved, stock_params_resolved, stock_p, STOCK_MAX_PARAMS};

/// Card 3D execution plan: rotation/projection/area/cull plus the
/// inverse-homography solve, shared by the CPU raster kernel and the GPU
/// chain (which packs `Project` straight into uniforms — same matrix,
/// same divide, parity by construction).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Card3dPlan {
    /// Near-zero rotation: leave the buffer untouched.
    Identity,
    /// Culled backface: clear to transparent.
    Clear,
    /// Degenerate quad (unsolvable): leave untouched.
    Keep,
    /// Resample through the inverse homography (row-major 3x3).
    Project([f32; 9]),
}

/// Solve the projective map from unit-square corners to `dst` quad
/// corners (8x8 Gaussian elimination). Row-major homography, or None.
#[allow(clippy::needless_range_loop)]
fn homography_unit(dst: [(f32, f32); 4]) -> Option<[f32; 9]> {
    let src = [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let mut m = [[0.0f32; 9]; 8];
    for (i, ((sx, sy), (dx, dy))) in src.iter().zip(dst.iter()).enumerate() {
        m[i * 2] = [*sx, *sy, 1.0, 0.0, 0.0, 0.0, -dx * sx, -dx * sy, *dx];
        m[i * 2 + 1] = [0.0, 0.0, 0.0, *sx, *sy, 1.0, -dy * sx, -dy * sy, *dy];
    }
    for col in 0..8 {
        let mut piv = col;
        for row in col..8 {
            if m[row][col].abs() > m[piv][col].abs() {
                piv = row;
            }
        }
        if m[piv][col].abs() < 1e-9 {
            return None;
        }
        m.swap(col, piv);
        for row in (col + 1)..8 {
            let f = m[row][col] / m[col][col];
            for k in col..9 {
                m[row][k] -= f * m[col][k];
            }
        }
    }
    let mut h = [0.0f32; 8];
    for i in (0..8).rev() {
        let mut s = m[i][8];
        for k in (i + 1)..8 {
            s -= m[i][k] * h[k];
        }
        h[i] = s / m[i][i];
    }
    Some([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0])
}

fn invert_homography(h: [f32; 9]) -> Option<[f32; 9]> {
    let det =
        h[0] * (h[4] - h[5] * h[7]) - h[1] * (h[3] - h[5] * h[6]) + h[2] * (h[3] * h[7] - h[4] * h[6]);
    if det.abs() < 1e-9 {
        return None;
    }
    let id = 1.0 / det;
    Some([
        (h[4] - h[5] * h[7]) * id,
        (h[2] * h[7] - h[1]) * id,
        (h[1] * h[5] - h[2] * h[4]) * id,
        (h[5] * h[6] - h[3]) * id,
        (h[0] - h[2] * h[6]) * id,
        (h[2] * h[3] - h[0] * h[5]) * id,
        (h[3] * h[7] - h[4] * h[6]) * id,
        (h[1] * h[6] - h[0] * h[7]) * id,
        (h[0] * h[4] - h[1] * h[3]) * id,
    ])
}

/// Plan a Card 3D pass over `params` (descriptor order: rotation_x,
/// rotation_y, distance, pivot_x, pivot_y, cull) for a `w`x`h` buffer.
/// Aspect-corrected so rotation is circular on any frame.
pub fn card_3d_plan(params: &[f32], w: f32, h: f32) -> Card3dPlan {
    let g = |i: usize| params.get(i).copied().unwrap_or(0.0);
    let rx = g(0).to_radians();
    let ry = g(1).to_radians();
    if rx.abs() < 0.001 && ry.abs() < 0.001 {
        return Card3dPlan::Identity;
    }
    let dist = g(2).clamp(50.0, 800.0);
    let (pvx, pvy) = (g(3) / 100.0, g(4) / 100.0);
    let cull = g(5) >= 0.5;
    let aspect = (w / h.max(1.0)).max(1e-3);
    let (cx, sx) = (rx.cos(), rx.sin());
    let (cy, sy) = (ry.cos(), ry.sin());
    let mut quad = [(0.0f32, 0.0f32); 4];
    for (i, (ux, uy)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
        .into_iter()
        .enumerate()
    {
        let (mut x, mut y, mut z) = ((ux - pvx) * aspect, uy - pvy, 0.0);
        (y, z) = (y * cx - z * sx, y * sx + z * cx);
        (x, z) = (x * cy + z * sy, -x * sy + z * cy);
        let denom = (dist - z).max(1.0);
        quad[i] = (x / aspect * (dist / denom) + pvx, y * (dist / denom) + pvy);
    }
    let mut area = 0.0f32;
    for i in 0..4 {
        let (ax, ay) = quad[i];
        let (bx, by) = quad[(i + 1) % 4];
        area += ax * by - bx * ay;
    }
    if cull && area <= 0.0 {
        return Card3dPlan::Clear;
    }
    match homography_unit(quad).and_then(invert_homography) {
        Some(inv) => Card3dPlan::Project(inv),
        None => Card3dPlan::Keep,
    }
}

/// Plan a Corner Pin pass over descriptor-order params (ul_x, ul_y, ur_x,
/// ur_y, lr_x, lr_y, ll_x, ll_y, unit space). Same solve + inverse as the
/// CPU kernel (shared so both paths project through one matrix); the GPU
/// twin reuses the Card3D project body.
pub fn corner_pin_plan(params: &[f32]) -> Card3dPlan {
    let g = |i: usize| params.get(i).copied().unwrap_or(0.0);
    let quad = [
        (g(0).clamp(0.0, 1.0), g(1).clamp(0.0, 1.0)),
        (g(2).clamp(0.0, 1.0), g(3).clamp(0.0, 1.0)),
        (g(4).clamp(0.0, 1.0), g(5).clamp(0.0, 1.0)),
        (g(6).clamp(0.0, 1.0), g(7).clamp(0.0, 1.0)),
    ];
    if quad[0] == (0.0, 0.0) && quad[1] == (1.0, 0.0) && quad[2] == (1.0, 1.0) && quad[3] == (0.0, 1.0) {
        return Card3dPlan::Identity;
    }
    match homography_unit(quad).and_then(invert_homography) {
        Some(inv) => Card3dPlan::Project(inv),
        None => Card3dPlan::Keep,
    }
}

/// True when this plug-in needs neighbours, position, or time.
pub fn is_spatial_stock(plugin: StockPlugin) -> bool {
    plugin.descriptor().spatial
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_flags_match_registry() {
        for plugin in StockPlugin::all() {
            assert_eq!(is_spatial_stock(*plugin), plugin.descriptor().spatial);
        }
    }

    #[test]
    fn card_3d_plan_identity_cull_project() {
        // Identity rotation plans untouched.
        assert!(matches!(
            card_3d_plan(&[0.0, 0.0, 300.0, 50.0, 50.0, 1.0], 64.0, 64.0),
            Card3dPlan::Identity
        ));
        // Edge-on with culling clears.
        assert!(matches!(
            card_3d_plan(&[0.0, 90.0, 300.0, 50.0, 50.0, 1.0], 64.0, 64.0),
            Card3dPlan::Clear
        ));
        // Moderate tilt projects; identity matrix at rest is near-exact.
        match card_3d_plan(&[15.0, 0.0, 300.0, 50.0, 50.0, 1.0], 64.0, 64.0) {
            Card3dPlan::Project(inv) => {
                // Center maps near itself for a small tilt about center.
                let w = inv[6] * 0.5 + inv[7] * 0.5 + inv[8];
                let sx = (inv[0] * 0.5 + inv[1] * 0.5 + inv[2]) / w;
                let sy = (inv[3] * 0.5 + inv[4] * 0.5 + inv[5]) / w;
                assert!((sx - 0.5).abs() < 0.05 && (sy - 0.5).abs() < 0.05, "{sx},{sy}");
            }
            other => panic!("expected Project, got {other:?}"),
        }
    }
}
