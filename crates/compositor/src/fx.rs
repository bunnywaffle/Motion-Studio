//! Stock plug-in per-pixel math.
//!
//! Pure functions shared by the CPU preview (`process_color`), adjustment
//! layers, and headless evaluation. Spatial plug-ins (neighbours / position
//! / time) are the identity here and live in `application::raster`; WGSL
//! twins live in `renderer::effect_filters`. All three must agree — see the
//! parity tests at the bottom.

use project::{Color, StockPlugin};

/// Read param `i` with the descriptor default as fallback (old project
/// files may carry shorter vectors).
pub fn stock_p(plugin: StockPlugin, params: &[f32], i: usize) -> f32 {
    params.get(i).copied().unwrap_or_else(|| {
        plugin.descriptor().params.get(i).map(|p| p.default).unwrap_or(0.0)
    })
}

/// Resolved param slots for the hot per-pixel kernels (max descriptor
/// length is 9; 12 slots leave headroom). Unpack once per layer/frame —
/// never inside a pixel loop, where descriptor lookups stall.
pub const STOCK_MAX_PARAMS: usize = 12;

/// Unpack raw params against descriptor defaults into a fixed stack array.
pub fn stock_params_resolved(plugin: StockPlugin, params: &[f32]) -> [f32; STOCK_MAX_PARAMS] {
    let desc = plugin.descriptor();
    let mut out = [0.0f32; STOCK_MAX_PARAMS];
    for (i, v) in out.iter_mut().enumerate() {
        *v = params
            .get(i)
            .copied()
            .unwrap_or_else(|| desc.params.get(i).map(|p| p.default).unwrap_or(0.0));
    }
    out
}

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

/// True when this plug-in needs neighbours, position, or time.
pub fn is_spatial_stock(plugin: StockPlugin) -> bool {
    plugin.descriptor().spatial
}

fn lum(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

/// Per-pixel stock kernel. `params` are raw values in descriptor order.
pub fn process_color_stock(plugin: StockPlugin, params: &[f32], c: Color) -> Color {
    process_color_stock_resolved(plugin, &stock_params_resolved(plugin, params), c)
}

/// Per-pixel stock kernel over pre-resolved params (see
/// [`stock_params_resolved`]). Hot raster loops must call this variant so
/// descriptor lookups happen once per layer, not once per pixel.
pub fn process_color_stock_resolved(
    plugin: StockPlugin,
    vals: &[f32; STOCK_MAX_PARAMS],
    c: Color,
) -> Color {
    match plugin {
        StockPlugin::Curves => {
            let l = [vals[0], vals[1], vals[2], vals[3], vals[4]];
            let zones = [0.1f32, 0.3, 0.5, 0.7, 0.9];
            let grade = |x: f32| {
                let mut y = x;
                for (lift, z) in l.iter().zip(zones.iter()) {
                    let w = (1.0 - ((x - z).abs() / 0.35)).clamp(0.0, 1.0);
                    y += lift / 100.0 * w * 0.6;
                }
                y.clamp(0.0, 1.0)
            };
            Color::rgba(grade(c.r), grade(c.g), grade(c.b), c.a)
        }
        StockPlugin::ColorBalance => {
            // 9 params, tone-major: shadows/midtones/highlights × CMY.
            // Each zone shifts with the classic cross-channel balance math,
            // weighted by its tonal mask (same masks as Color Wheels).
            let zone = |x: f32| {
                let ws = (1.0 - x) * (1.0 - x);
                let wm = 1.0 - (2.0 * x - 1.0) * (2.0 * x - 1.0);
                let wh = x * x;
                (ws, wm, wh)
            };
            let apply = |x: f32, ws: f32, wm: f32, wh: f32, sh: f32, mi: f32, hi: f32| {
                // Positive pushes the channel, negative pushes its complement
                // pair (cyan↔red, magenta↔green, yellow↔blue live per channel
                // here; complements resolve in the channel loop below).
                (x + (sh * ws + mi * wm + hi * wh) / 100.0 * 0.5).clamp(0.0, 1.0)
            };
            let (sr, sg, sb) = (zone(c.r), zone(c.g), zone(c.b));
            let mut r = apply(c.r, sr.0, sr.1, sr.2, vals[0], vals[3], vals[6]);
            let mut g = apply(c.g, sg.0, sg.1, sg.2, vals[1], vals[4], vals[7]);
            let mut b = apply(c.b, sb.0, sb.1, sb.2, vals[2], vals[5], vals[8]);
            // Negative (complementary) side: push the other two channels.
            let neg = |v: f32, w: f32| (-v).max(0.0) * w * 0.25 / 100.0;
            g += neg(vals[0], sr.0) + neg(vals[3], sr.1) + neg(vals[6], sr.2);
            b += neg(vals[0], sr.0) + neg(vals[3], sr.1) + neg(vals[6], sr.2);
            r += neg(vals[1], sg.0) + neg(vals[4], sg.1) + neg(vals[7], sg.2);
            b += neg(vals[1], sg.0) + neg(vals[4], sg.1) + neg(vals[7], sg.2);
            r += neg(vals[2], sb.0) + neg(vals[5], sb.1) + neg(vals[8], sb.2);
            g += neg(vals[2], sb.0) + neg(vals[5], sb.1) + neg(vals[8], sb.2);
            Color::rgba(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), c.a)
        }
        StockPlugin::ColorWheels => {
            let (s, m, h) = (vals[0] / 100.0, vals[1] / 100.0, vals[2] / 100.0);
            let grade = |x: f32| {
                let ws = (1.0 - x) * (1.0 - x);
                let wm = 1.0 - (2.0 * x - 1.0) * (2.0 * x - 1.0);
                let wh = x * x;
                (x + s * ws * 0.5 + m * wm * 0.5 + h * wh * 0.5).clamp(0.0, 1.0)
            };
            Color::rgba(grade(c.r), grade(c.g), grade(c.b), c.a)
        }
        StockPlugin::TemperatureTint => {
            let t = vals[0] / 100.0;
            let ti = vals[1] / 100.0;
            Color::rgba(
                (c.r + t * 0.35 - ti * 0.10).clamp(0.0, 1.0),
                (c.g + ti * 0.15).clamp(0.0, 1.0),
                (c.b - t * 0.35 - ti * 0.10).clamp(0.0, 1.0),
                c.a,
            )
        }
        StockPlugin::Posterize => {
            let n = vals[0].round().clamp(2.0, 32.0);
            let q = |x: f32| ((x * (n - 1.0)).round() / (n - 1.0)).clamp(0.0, 1.0);
            Color::rgba(q(c.r), q(c.g), q(c.b), c.a)
        }
        StockPlugin::Threshold => {
            let t = (vals[0] / 100.0).clamp(0.0, 1.0);
            let f = (vals[1] / 100.0).max(0.001);
            let l = lum(c.r, c.g, c.b);
            let s = ((l - t) / f + 0.5).clamp(0.0, 1.0);
            let s = s * s * (3.0 - 2.0 * s);
            Color::rgba(s, s, s, c.a)
        }
        StockPlugin::DifferenceKey => {
            let key = (vals[0] / 100.0).clamp(0.0, 1.0);
            let th = (vals[1] / 100.0).clamp(0.0, 1.0);
            let f = (vals[2] / 100.0).max(0.001);
            let d = (lum(c.r, c.g, c.b) - key).abs();
            let a = if d < th {
                0.0
            } else {
                ((d - th) / f).clamp(0.0, 1.0)
            };
            Color::rgba(c.r, c.g, c.b, c.a * a)
        }
        StockPlugin::SpillSuppress => {
            let a = (vals[0] / 100.0).clamp(0.0, 1.0);
            let cap = (c.r + c.b) * 0.5;
            Color::rgba(c.r, c.g * (1.0 - a) + cap.min(c.g) * a, c.b, c.a)
        }
        // Everything else is spatial (neighbours / position / time).
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_nan(plugin: StockPlugin, params: &[f32], c: Color) -> Color {
        let o = process_color_stock(plugin, params, c);
        for v in [o.r, o.g, o.b, o.a] {
            assert!(v.is_finite(), "{plugin:?} produced {v}");
        }
        o
    }

    #[test]
    fn every_plugin_processes_without_nan() {
        let samples = [
            Color::rgba(0.0, 0.0, 0.0, 1.0),
            Color::rgba(1.0, 1.0, 1.0, 1.0),
            Color::rgba(0.8, 0.2, 0.4, 0.7),
            Color::rgba(0.1, 0.9, 0.3, 0.0),
        ];
        for plugin in StockPlugin::all() {
            let desc = plugin.descriptor();
            // Defaults, extremes, and mid values.
            let mut sets = vec![desc.params.iter().map(|p| p.default).collect::<Vec<_>>()];
            sets.push(desc.params.iter().map(|p| p.min).collect::<Vec<_>>());
            sets.push(desc.params.iter().map(|p| p.max).collect::<Vec<_>>());
            for params in sets {
                for c in samples {
                    no_nan(*plugin, &params, c);
                }
            }
        }
    }

    #[test]
    fn curves_identity_and_lift() {
        let p = StockPlugin::Curves;
        let c = Color::rgba(0.5, 0.5, 0.5, 1.0);
        let o = process_color_stock(p, &[0.0; 5], c);
        assert!((o.r - 0.5).abs() < 1e-5);
        let o = process_color_stock(p, &[0.0, 0.0, 100.0, 0.0, 0.0], c);
        assert!(o.r > 0.6);
    }

    #[test]
    fn threshold_binarizes() {
        let p = StockPlugin::Threshold;
        let w = process_color_stock(p, &[50.0, 0.0], Color::rgba(0.9, 0.9, 0.9, 1.0));
        let b = process_color_stock(p, &[50.0, 0.0], Color::rgba(0.1, 0.1, 0.1, 1.0));
        assert!(w.r > 0.99 && b.r < 0.01);
    }

    #[test]
    fn resolved_params_match_unresolved() {
        // The hot-path variant must agree exactly with the resolving one.
        let samples = [
            Color::rgba(0.0, 0.0, 0.0, 1.0),
            Color::rgba(0.8, 0.2, 0.4, 0.7),
        ];
        for plugin in StockPlugin::all() {
            let desc = plugin.descriptor();
            let sets = [
                desc.params.iter().map(|p| p.default).collect::<Vec<_>>(),
                desc.params.iter().map(|p| p.max).collect::<Vec<_>>(),
                vec![],
            ];
            for params in sets {
                let resolved = stock_params_resolved(*plugin, &params);
                for c in samples {
                    let a = process_color_stock(*plugin, &params, c);
                    let b = process_color_stock_resolved(*plugin, &resolved, c);
                    assert!(
                        (a.r - b.r).abs() < 1e-6
                            && (a.g - b.g).abs() < 1e-6
                            && (a.b - b.b).abs() < 1e-6
                            && (a.a - b.a).abs() < 1e-6,
                        "{plugin:?} {params:?} {c:?}: {a:?} vs {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn color_balance_zones_isolate() {
        // Params are tone-major: shadows/midtones/highlights × CMY.
        let p = StockPlugin::ColorBalance;
        let dark = Color::rgba(0.1, 0.1, 0.1, 1.0);
        let mid = Color::rgba(0.5, 0.5, 0.5, 1.0);
        let bright = Color::rgba(0.9, 0.9, 0.9, 1.0);
        // Identity at all zeros.
        for c in [dark, mid, bright] {
            let o = process_color_stock(p, &[0.0; 9], c);
            assert!((o.r - c.r).abs() < 1e-5 && (o.g - c.g).abs() < 1e-5 && (o.b - c.b).abs() < 1e-5);
        }
        // Shadows red (+100) lifts darks, spares brights.
        let sh_red = [100.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let d = process_color_stock(p, &sh_red, dark);
        let b = process_color_stock(p, &sh_red, bright);
        assert!(d.r - dark.r > 0.2, "{d:?}");
        assert!((b.r - bright.r).abs() < 0.05, "{b:?}");
        // Highlights blue (+100) lifts brights, spares darks.
        let hi_blue = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 100.0];
        let d = process_color_stock(p, &hi_blue, dark);
        let b = process_color_stock(p, &hi_blue, bright);
        assert!((d.b - dark.b).abs() < 0.05, "{d:?}");
        assert!(b.b - bright.b > 0.05, "{b:?}");
        // Midtones green (+100) moves mids far more than extremes
        // (broad cosine falloff, same as Color Wheels).
        let mid_green = [0.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 0.0];
        let m = process_color_stock(p, &mid_green, mid);
        let dm = m.g - mid.g;
        assert!(dm > 0.3, "{m:?}");
        let d = process_color_stock(p, &mid_green, dark);
        let b = process_color_stock(p, &mid_green, bright);
        assert!(d.g - dark.g < dm / 2.0 && b.g - bright.g < dm / 2.0);
        // Negative (cyan) spills into green and blue.
        let sh_cyan = [-100.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let d = process_color_stock(p, &sh_cyan, dark);
        assert!(d.g > dark.g && d.b > dark.b, "{d:?}");
    }

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
