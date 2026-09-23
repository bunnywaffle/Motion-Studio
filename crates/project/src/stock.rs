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
        _ => &[],
    }
}

/// Default color for a stock color slot.
pub fn stock_default_color(plugin: StockPlugin, slot: &str) -> super::Color {
    use super::Color;
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
