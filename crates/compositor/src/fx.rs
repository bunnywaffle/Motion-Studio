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
}
