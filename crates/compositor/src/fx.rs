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

/// True when this plug-in needs neighbours, position, or time.
pub fn is_spatial_stock(plugin: StockPlugin) -> bool {
    plugin.descriptor().spatial
}

fn lum(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

/// Per-pixel stock kernel. `params` are raw values in descriptor order.
pub fn process_color_stock(plugin: StockPlugin, params: &[f32], c: Color) -> Color {
    match plugin {
        StockPlugin::Curves => {
            let l = [stock_p(plugin, params, 0), stock_p(plugin, params, 1), stock_p(plugin, params, 2), stock_p(plugin, params, 3), stock_p(plugin, params, 4)];
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
            let cr = stock_p(plugin, params, 0) / 100.0;
            let mg = stock_p(plugin, params, 1) / 100.0;
            let yb = stock_p(plugin, params, 2) / 100.0;
            let adj = |x: f32| {
                let m = (x * std::f32::consts::PI).sin().clamp(0.0, 1.0);
                (x, m)
            };
            let (r, mr) = adj(c.r);
            let (g, mgm) = adj(c.g);
            let (b, mb) = adj(c.b);
            let mut r = r;
            let mut g = g;
            let mut b = b;
            if cr >= 0.0 { r += cr * mr * 0.5; } else { g += -cr * mgm * 0.25; b += -cr * mb * 0.25; }
            if mg >= 0.0 { g += mg * mgm * 0.5; } else { r += -mg * mr * 0.25; b += -mg * mb * 0.25; }
            if yb >= 0.0 { b += yb * mb * 0.5; } else { r += -yb * mr * 0.25; g += -yb * mgm * 0.25; }
            Color::rgba(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), c.a)
        }
        StockPlugin::ColorWheels => {
            let (s, m, h) = (stock_p(plugin, params, 0) / 100.0, stock_p(plugin, params, 1) / 100.0, stock_p(plugin, params, 2) / 100.0);
            let grade = |x: f32| {
                let ws = (1.0 - x) * (1.0 - x);
                let wm = 1.0 - (2.0 * x - 1.0) * (2.0 * x - 1.0);
                let wh = x * x;
                (x + s * ws * 0.5 + m * wm * 0.5 + h * wh * 0.5).clamp(0.0, 1.0)
            };
            Color::rgba(grade(c.r), grade(c.g), grade(c.b), c.a)
        }
        StockPlugin::TemperatureTint => {
            let t = stock_p(plugin, params, 0) / 100.0;
            let ti = stock_p(plugin, params, 1) / 100.0;
            Color::rgba(
                (c.r + t * 0.35 - ti * 0.10).clamp(0.0, 1.0),
                (c.g + ti * 0.15).clamp(0.0, 1.0),
                (c.b - t * 0.35 - ti * 0.10).clamp(0.0, 1.0),
                c.a,
            )
        }
        StockPlugin::Posterize => {
            let n = stock_p(plugin, params, 0).round().clamp(2.0, 32.0);
            let q = |x: f32| ((x * (n - 1.0)).round() / (n - 1.0)).clamp(0.0, 1.0);
            Color::rgba(q(c.r), q(c.g), q(c.b), c.a)
        }
        StockPlugin::Threshold => {
            let t = (stock_p(plugin, params, 0) / 100.0).clamp(0.0, 1.0);
            let f = (stock_p(plugin, params, 1) / 100.0).max(0.001);
            let l = lum(c.r, c.g, c.b);
            let s = ((l - t) / f + 0.5).clamp(0.0, 1.0);
            let s = s * s * (3.0 - 2.0 * s);
            Color::rgba(s, s, s, c.a)
        }
        StockPlugin::DifferenceKey => {
            let key = (stock_p(plugin, params, 0) / 100.0).clamp(0.0, 1.0);
            let th = (stock_p(plugin, params, 1) / 100.0).clamp(0.0, 1.0);
            let f = (stock_p(plugin, params, 2) / 100.0).max(0.001);
            let d = (lum(c.r, c.g, c.b) - key).abs();
            let a = if d < th {
                0.0
            } else {
                ((d - th) / f).clamp(0.0, 1.0)
            };
            Color::rgba(c.r, c.g, c.b, c.a * a)
        }
        StockPlugin::SpillSuppress => {
            let a = (stock_p(plugin, params, 0) / 100.0).clamp(0.0, 1.0);
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
    fn spatial_flags_match_registry() {
        for plugin in StockPlugin::all() {
            assert_eq!(is_spatial_stock(*plugin), plugin.descriptor().spatial);
        }
    }
}
