//! Stock plug-in catalogue: the modular OpenFX-style effect suite.
//!
//! Every effect is one [`StockPlugin`] variant plus one [`OfxEffectDescriptor`]
//! entry in [`STOCK_SUITE`]. Adding a future effect means:
//! 1. append a `StockPlugin` variant,
//! 2. append its descriptor below (params drive factories, clamps, UI,
//!    timeline, and spline keyframes automatically),
//! 3. implement its math in `compositor::fx` (per-pixel) and/or
//!    `application::raster` (spatial) plus a WGSL twin in
//!    `renderer::effect_filters`.
//!
//! No per-effect UI code is needed: panels render generically from the
//! descriptor. Legacy hand-rolled `EffectType` variants stay untouched for
//! project-file compatibility.

use super::ofx::OfxEffectDescriptor;
use crate::Color;

/// One stock effect plug-in. Serialized snake_case inside
/// `EffectType::Stock` (`box_blur`, `corner_pin`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockPlugin {
    // Color
    Curves,
    ColorBalance,
    ColorWheels,
    TemperatureTint,
    Posterize,
    Threshold,
    // Blur
    BoxBlur,
    DirectionalBlur,
    RadialBlur,
    ZoomBlur,
    MotionBlur,
    Defocus,
    Bokeh,
    Bilateral,
    // Light
    Glow,
    Glare,
    Glint,
    LightRays,
    GodRays,
    LensFlare,
    LightLeak,
    Streaks,
    Halo,
    LongShadow,
    Saber,
    // Distortion
    TurbulentDisplace,
    Wave,
    Ripple,
    Twirl,
    Bulge,
    Spherize,
    LensDistortion,
    ChromaticAberration,
    MeshWarp,
    Liquify,
    // Stylize
    EdgeDetect,
    Cartoon,
    Halftone,
    Sketch,
    Emboss,
    Pixelate,
    Mosaic,
    Vhs,
    RgbSplit,
    Scanlines,
    Glitch,
    // Noise / film
    FilmGrain,
    FractalNoise,
    Turbulence,
    Dust,
    Scratches,
    FilmDamage,
    Flicker,
    GateWeave,
    // Keying / matte
    DifferenceKey,
    SpillSuppress,
    MatteChoker,
    MatteBlur,
    Erode,
    Dilate,
    KeyCleaner,
    // Transform / spatial
    TransformFx,
    Crop,
    CornerPin,
    Card3d,
    Mirror,
    Repeat,
    Offset,
    Reframe,
    // Cleanup
    Denoise,
    Deband,
    Degrain,
    Deblur,
    DustRemoval,
    ScratchRemoval,
    DeadPixel,
    // Generators
    Solid,
    FractalGen,
    GridGen,
    Shapes,
    PlasmaGen,
    Particles,
}

impl StockPlugin {
    /// All plug-ins in suite order.
    pub const fn all() -> &'static [StockPlugin] {
        &[
            StockPlugin::Curves,
            StockPlugin::ColorBalance,
            StockPlugin::ColorWheels,
            StockPlugin::TemperatureTint,
            StockPlugin::Posterize,
            StockPlugin::Threshold,
            StockPlugin::BoxBlur,
            StockPlugin::DirectionalBlur,
            StockPlugin::RadialBlur,
            StockPlugin::ZoomBlur,
            StockPlugin::MotionBlur,
            StockPlugin::Defocus,
            StockPlugin::Bokeh,
            StockPlugin::Bilateral,
            StockPlugin::Glow,
            StockPlugin::Glare,
            StockPlugin::Glint,
            StockPlugin::LightRays,
            StockPlugin::GodRays,
            StockPlugin::LensFlare,
            StockPlugin::LongShadow,
            StockPlugin::Saber,
            StockPlugin::LightLeak,
            StockPlugin::Streaks,
            StockPlugin::Halo,
            StockPlugin::TurbulentDisplace,
            StockPlugin::Wave,
            StockPlugin::Ripple,
            StockPlugin::Twirl,
            StockPlugin::Bulge,
            StockPlugin::Spherize,
            StockPlugin::LensDistortion,
            StockPlugin::ChromaticAberration,
            StockPlugin::MeshWarp,
            StockPlugin::Liquify,
            StockPlugin::EdgeDetect,
            StockPlugin::Cartoon,
            StockPlugin::Halftone,
            StockPlugin::Sketch,
            StockPlugin::Emboss,
            StockPlugin::Pixelate,
            StockPlugin::Mosaic,
            StockPlugin::Vhs,
            StockPlugin::RgbSplit,
            StockPlugin::Scanlines,
            StockPlugin::Glitch,
            StockPlugin::FilmGrain,
            StockPlugin::FractalNoise,
            StockPlugin::Turbulence,
            StockPlugin::Dust,
            StockPlugin::Scratches,
            StockPlugin::FilmDamage,
            StockPlugin::Flicker,
            StockPlugin::GateWeave,
            StockPlugin::DifferenceKey,
            StockPlugin::SpillSuppress,
            StockPlugin::MatteChoker,
            StockPlugin::MatteBlur,
            StockPlugin::Erode,
            StockPlugin::Dilate,
            StockPlugin::KeyCleaner,
            StockPlugin::TransformFx,
            StockPlugin::Crop,
            StockPlugin::CornerPin,
            StockPlugin::Card3d,
            StockPlugin::Mirror,
            StockPlugin::Repeat,
            StockPlugin::Offset,
            StockPlugin::Reframe,
            StockPlugin::Denoise,
            StockPlugin::Deband,
            StockPlugin::Degrain,
            StockPlugin::Deblur,
            StockPlugin::DustRemoval,
            StockPlugin::ScratchRemoval,
            StockPlugin::DeadPixel,
            StockPlugin::Solid,
            StockPlugin::FractalGen,
            StockPlugin::GridGen,
            StockPlugin::Shapes,
            StockPlugin::PlasmaGen,
            StockPlugin::Particles,
        ]
    }

    /// Reverse-DNS id (`net.sf.openfx.*` family).
    pub const fn plugin_id(self) -> &'static str {
        match self {
            StockPlugin::Curves => "net.sf.openfx.curves",
            StockPlugin::ColorBalance => "net.sf.openfx.color_balance",
            StockPlugin::ColorWheels => "net.sf.openfx.color_wheels",
            StockPlugin::TemperatureTint => "net.sf.openfx.temperature_tint",
            StockPlugin::Posterize => "net.sf.openfx.posterize",
            StockPlugin::Threshold => "net.sf.openfx.threshold",
            StockPlugin::BoxBlur => "net.sf.openfx.box_blur",
            StockPlugin::DirectionalBlur => "net.sf.openfx.directional_blur",
            StockPlugin::RadialBlur => "net.sf.openfx.radial_blur",
            StockPlugin::ZoomBlur => "net.sf.openfx.zoom_blur",
            StockPlugin::MotionBlur => "net.sf.openfx.motion_blur",
            StockPlugin::Defocus => "net.sf.openfx.defocus",
            StockPlugin::Bokeh => "net.sf.openfx.bokeh",
            StockPlugin::Bilateral => "net.sf.openfx.bilateral",
            StockPlugin::Glow => "net.sf.openfx.glow",
            StockPlugin::Glare => "net.sf.openfx.glare",
            StockPlugin::Glint => "net.sf.openfx.glint",
            StockPlugin::LightRays => "net.sf.openfx.light_rays",
            StockPlugin::GodRays => "net.sf.openfx.god_rays",
            StockPlugin::LensFlare => "net.sf.openfx.lens_flare",
            StockPlugin::LongShadow => "net.sf.openfx.long_shadow",
            StockPlugin::Saber => "net.sf.openfx.saber",
            StockPlugin::LightLeak => "net.sf.openfx.light_leak",
            StockPlugin::Streaks => "net.sf.openfx.streaks",
            StockPlugin::Halo => "net.sf.openfx.halo",
            StockPlugin::TurbulentDisplace => "net.sf.openfx.turbulent_displace",
            StockPlugin::Wave => "net.sf.openfx.wave",
            StockPlugin::Ripple => "net.sf.openfx.ripple",
            StockPlugin::Twirl => "net.sf.openfx.twirl",
            StockPlugin::Bulge => "net.sf.openfx.bulge",
            StockPlugin::Spherize => "net.sf.openfx.spherize",
            StockPlugin::LensDistortion => "net.sf.openfx.lens_distortion",
            StockPlugin::ChromaticAberration => "net.sf.openfx.chromatic_aberration",
            StockPlugin::MeshWarp => "net.sf.openfx.mesh_warp",
            StockPlugin::Liquify => "net.sf.openfx.liquify",
            StockPlugin::EdgeDetect => "net.sf.openfx.edge_detect",
            StockPlugin::Cartoon => "net.sf.openfx.cartoon",
            StockPlugin::Halftone => "net.sf.openfx.halftone",
            StockPlugin::Sketch => "net.sf.openfx.sketch",
            StockPlugin::Emboss => "net.sf.openfx.emboss",
            StockPlugin::Pixelate => "net.sf.openfx.pixelate",
            StockPlugin::Mosaic => "net.sf.openfx.mosaic",
            StockPlugin::Vhs => "net.sf.openfx.vhs",
            StockPlugin::RgbSplit => "net.sf.openfx.rgb_split",
            StockPlugin::Scanlines => "net.sf.openfx.scanlines",
            StockPlugin::Glitch => "net.sf.openfx.glitch",
            StockPlugin::FilmGrain => "net.sf.openfx.film_grain",
            StockPlugin::FractalNoise => "net.sf.openfx.fractal_noise",
            StockPlugin::Turbulence => "net.sf.openfx.turbulence",
            StockPlugin::Dust => "net.sf.openfx.dust",
            StockPlugin::Scratches => "net.sf.openfx.scratches",
            StockPlugin::FilmDamage => "net.sf.openfx.film_damage",
            StockPlugin::Flicker => "net.sf.openfx.flicker",
            StockPlugin::GateWeave => "net.sf.openfx.gate_weave",
            StockPlugin::DifferenceKey => "net.sf.openfx.difference_key",
            StockPlugin::SpillSuppress => "net.sf.openfx.spill_suppress",
            StockPlugin::MatteChoker => "net.sf.openfx.matte_choker",
            StockPlugin::MatteBlur => "net.sf.openfx.matte_blur",
            StockPlugin::Erode => "net.sf.openfx.erode",
            StockPlugin::Dilate => "net.sf.openfx.dilate",
            StockPlugin::KeyCleaner => "net.sf.openfx.key_cleaner",
            StockPlugin::TransformFx => "net.sf.openfx.transform",
            StockPlugin::Crop => "net.sf.openfx.crop",
            StockPlugin::CornerPin => "net.sf.openfx.corner_pin",
            StockPlugin::Card3d => "net.sf.openfx.card_3d",
            StockPlugin::Mirror => "net.sf.openfx.mirror",
            StockPlugin::Repeat => "net.sf.openfx.repeat",
            StockPlugin::Offset => "net.sf.openfx.offset",
            StockPlugin::Reframe => "net.sf.openfx.reframe",
            StockPlugin::Denoise => "net.sf.openfx.denoise",
            StockPlugin::Deband => "net.sf.openfx.deband",
            StockPlugin::Degrain => "net.sf.openfx.degrain",
            StockPlugin::Deblur => "net.sf.openfx.deblur",
            StockPlugin::DustRemoval => "net.sf.openfx.dust_removal",
            StockPlugin::ScratchRemoval => "net.sf.openfx.scratch_removal",
            StockPlugin::DeadPixel => "net.sf.openfx.dead_pixel",
            StockPlugin::Solid => "net.sf.openfx.solid",
            StockPlugin::FractalGen => "net.sf.openfx.fractal_gen",
            StockPlugin::GridGen => "net.sf.openfx.grid",
            StockPlugin::Shapes => "net.sf.openfx.shapes",
            StockPlugin::PlasmaGen => "net.sf.openfx.plasma",
            StockPlugin::Particles => "net.sf.openfx.particles",
        }
    }

    /// This plug-in's descriptor (label, category, params).
    pub fn descriptor(self) -> &'static OfxEffectDescriptor {
        super::ofx::ofx_lookup(self.plugin_id()).expect("stock descriptor registered")
    }

    /// True when this plug-in's kernel is a pure per-pixel color transform
    /// (no neighbours, position, or time). These are exactly the plugins
    /// the Node Graph's `StockOp` node may embed, and the kernels the
    /// shared per-pixel twin below dispatches on.
    pub fn is_pointwise(self) -> bool {
        !self.descriptor().spatial
    }
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
    use Color;
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

/// Names of the non-animatable color slots a stock plug-in carries
/// (parallel to `EffectType::Stock.colors`). Generators use these; all
/// other plug-ins carry none.
pub fn stock_color_slots(plugin: StockPlugin) -> &'static [&'static str] {
    match plugin {
        StockPlugin::Solid => &["color"],
        StockPlugin::FractalGen => &["color_a", "color_b"],
        StockPlugin::GridGen => &["color"],
        StockPlugin::Shapes => &["color"],
        StockPlugin::PlasmaGen => &["color_a", "color_b"],
        StockPlugin::Particles => &["color"],
        StockPlugin::LongShadow => &["shadow_a", "shadow_b"],
        StockPlugin::Saber => &["core", "glow_inner", "glow_outer"],
        StockPlugin::LightRays | StockPlugin::GodRays | StockPlugin::LensFlare => &["tint"],
        _ => &[],
    }
}

/// Default color for a stock color slot.
pub fn stock_default_color(plugin: StockPlugin, slot: &str) -> Color {
    use Color;
    match plugin {
        StockPlugin::Solid => Color::WHITE,
        StockPlugin::FractalGen | StockPlugin::PlasmaGen => {
            if slot == "color_b" {
                Color::rgb(0.2, 0.5, 1.0)
            } else {
                Color::BLACK
            }
        }
        StockPlugin::GridGen => Color::rgba(1.0, 1.0, 1.0, 0.9),
        StockPlugin::Shapes | StockPlugin::Particles => Color::rgb(0.25, 0.6, 1.0),
        StockPlugin::LongShadow => {
            if slot == "shadow_b" {
                Color::rgba(0.0, 0.0, 0.0, 0.0)
            } else {
                Color::rgba(0.0, 0.0, 0.0, 0.8)
            }
        }
        StockPlugin::Saber => match slot {
            "core" => Color::WHITE,
            "glow_inner" => Color::rgb(0.4, 0.7, 1.0),
            "glow_outer" => Color::rgb(0.1, 0.2, 1.0),
            _ => Color::WHITE,
        },
        StockPlugin::LightRays | StockPlugin::GodRays | StockPlugin::LensFlare => {
            Color::rgba(1.0, 0.9, 0.7, 1.0)
        }
        _ => Color::WHITE,
    }
}

/// Build a stock effect's scalar params from its descriptor.
pub fn stock_default_params(plugin: StockPlugin) -> Vec<super::Property<f32>> {
    plugin
        .descriptor()
        .params
        .iter()
        .map(|p| super::Property::with_default(p.label, p.default, p.default))
        .collect()
}

/// Resolve a plug-in id back to its variant (for registry-driven UI).
pub fn stock_from_id(id: &str) -> Option<StockPlugin> {
    StockPlugin::all().iter().copied().find(|p| p.plugin_id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::process_color_stock;
    use crate::Color;

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
    fn pointwise_flags_match_spatial() {
        for plugin in StockPlugin::all() {
            assert_eq!(plugin.is_pointwise(), !plugin.descriptor().spatial);
        }
    }

    #[test]
    fn posterize_snaps_channels() {
        let p = [2.0];
        let o = process_color_stock(StockPlugin::Posterize, &p, Color::rgba(0.7, 0.2, 0.9, 1.0));
        assert!(o.r > 0.9 && o.g < 0.1 && o.b > 0.9, "{o:?}");
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
    fn every_plugin_has_matching_descriptor() {
        assert_eq!(StockPlugin::all().len(), super::super::ofx::STOCK_SUITE.len());
        for plugin in StockPlugin::all() {
            let d = plugin.descriptor();
            assert_eq!(d.id, plugin.plugin_id());
            assert!(!d.label.is_empty());
            assert!(!d.params.is_empty(), "{} has no params", d.id);
        }
    }

    #[test]
    fn plugin_ids_roundtrip() {
        for plugin in StockPlugin::all() {
            let d = super::super::ofx::ofx_lookup(plugin.plugin_id())
                .unwrap_or_else(|| panic!("missing {}", plugin.plugin_id()));
            assert_eq!(d.label, plugin.descriptor().label);
        }
    }
}
