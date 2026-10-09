//! OpenFX-style effect plug-in registry.
//!
//! Motion Effect cannot load native OpenFX (`.ofx` / C ABI) binaries from
//! this Rust/GPUI process, so effects ship as an **in-process OFX suite**:
//! every effect is described by an [`OfxEffectDescriptor`] with a stable
//! reverse-DNS plug-in id in the `net.sf.openfx.*` family (the stock-effect
//! namespace documented on <https://openeffects.org>), a category, and a
//! typed parameter list with ranges and defaults.
//!
//! The registry is the single catalogue behind:
//! - the Effects panel (one honest row per plug-in — no aliases),
//! - [`crate::EffectType::ofx_plugin_id`] / compositor evaluated twins,
//! - the WGSL export filter library (`renderer::effect_filters`),
//! - keyframable parameter discovery (timeline + spline editor).
//!
//! `ShaderLab` (`net.sf.openfx.custom.shader_lab`) and `GlslShader`
//! (`net.sf.openfx.custom.glsl`) are the suite's Custom-category plug-ins:
//! user-programmable shaders stay first-class alongside the stock set.

/// OpenFX-style effect category (matches the Effects panel accordion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfxCategory {
    Blur,
    Color,
    Light,
    Key,
    Distort,
    Stylize,
    Noise,
    Generate,
    Spatial,
    Cleanup,
    Text,
    Custom,
}

impl OfxCategory {
    /// Human panel label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Blur => "Blur & Sharpen",
            Self::Color => "Color Correction",
            Self::Light => "Light",
            Self::Key => "Keying & Matte",
            Self::Distort => "Distort & Perspective",
            Self::Stylize => "Stylize",
            Self::Noise => "Noise & Film",
            Self::Generate => "Generate",
            Self::Spatial => "Transform & Spatial",
            Self::Cleanup => "Cleanup & Repair",
            Self::Text => "Text",
            Self::Custom => "Custom Shaders (GLSL/WGSL)",
        }
    }
}

/// One keyframable scalar parameter of an OFX plug-in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OfxParamDescriptor {
    /// Property name (`Effect::get_param_property` key).
    pub name: &'static str,
    /// Human label.
    pub label: &'static str,
    /// Valid range (UI clamp).
    pub min: f32,
    /// Valid range (UI clamp).
    pub max: f32,
    /// Factory default.
    pub default: f32,
}

/// One effect plug-in in the suite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OfxEffectDescriptor {
    /// Stable reverse-DNS id (`net.sf.openfx.*`).
    pub id: &'static str,
    /// Human label (`EffectType::type_name`).
    pub label: &'static str,
    /// Panel category.
    pub category: OfxCategory,
    /// Keyframable scalar parameters.
    pub params: &'static [OfxParamDescriptor],
    /// True when the plug-in needs neighbours or pixel position
    /// (identity in `process_color`, resolved by rasterizers instead).
    pub spatial: bool,
}

const NO_PARAMS: &[OfxParamDescriptor] = &[];

/// The full stock suite: 23 filters/generators/keyers + 2 custom shader
/// plug-ins. `id`s are stable — project files and WGSL export depend on
/// them (see `ofx_lookup`).
pub const OFX_SUITE: &[OfxEffectDescriptor] = &[
    OfxEffectDescriptor {
        id: "net.sf.openfx.blur",
        label: "Gaussian Blur",
        category: OfxCategory::Blur,
        params: &[OfxParamDescriptor { name: "radius", label: "Radius", min: 0.0, max: 100.0, default: 10.0 }],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.sharpen",
        label: "Sharpen",
        category: OfxCategory::Blur,
        params: &[
            OfxParamDescriptor { name: "amount", label: "Amount", min: 0.0, max: 200.0, default: 50.0 },
            OfxParamDescriptor { name: "radius", label: "Radius", min: 0.0, max: 20.0, default: 2.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.brightness_contrast",
        label: "Brightness & Contrast",
        category: OfxCategory::Color,
        params: &[
            OfxParamDescriptor { name: "brightness", label: "Brightness", min: -100.0, max: 100.0, default: 0.0 },
            OfxParamDescriptor { name: "contrast", label: "Contrast", min: -100.0, max: 100.0, default: 0.0 },
        ],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.levels",
        label: "Levels",
        category: OfxCategory::Color,
        params: &[
            OfxParamDescriptor { name: "input_black", label: "Input Black", min: 0.0, max: 255.0, default: 0.0 },
            OfxParamDescriptor { name: "input_white", label: "Input White", min: 0.0, max: 255.0, default: 255.0 },
            OfxParamDescriptor { name: "gamma", label: "Gamma", min: 0.1, max: 9.9, default: 1.0 },
            OfxParamDescriptor { name: "output_black", label: "Output Black", min: 0.0, max: 255.0, default: 0.0 },
            OfxParamDescriptor { name: "output_white", label: "Output White", min: 0.0, max: 255.0, default: 255.0 },
        ],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.hue_saturation",
        label: "Hue / Saturation",
        category: OfxCategory::Color,
        params: &[
            OfxParamDescriptor { name: "hue_shift", label: "Hue Shift", min: -180.0, max: 180.0, default: 0.0 },
            OfxParamDescriptor { name: "saturation", label: "Saturation", min: -100.0, max: 100.0, default: 0.0 },
            OfxParamDescriptor { name: "lightness", label: "Lightness", min: -100.0, max: 100.0, default: 0.0 },
        ],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.tint",
        label: "Tint",
        category: OfxCategory::Color,
        params: &[OfxParamDescriptor { name: "amount", label: "Amount to Tint", min: 0.0, max: 100.0, default: 100.0 }],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.invert",
        label: "Invert",
        category: OfxCategory::Color,
        params: &[OfxParamDescriptor { name: "amount", label: "Invert Amount", min: 0.0, max: 100.0, default: 100.0 }],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.exposure",
        label: "Exposure",
        category: OfxCategory::Color,
        params: &[OfxParamDescriptor { name: "exposure", label: "Exposure", min: -10.0, max: 10.0, default: 0.0 }],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.vibrance",
        label: "Vibrance",
        category: OfxCategory::Color,
        params: &[OfxParamDescriptor { name: "vibrance", label: "Vibrance", min: -100.0, max: 100.0, default: 0.0 }],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.chroma_key",
        label: "Chroma Key",
        category: OfxCategory::Key,
        params: &[
            OfxParamDescriptor { name: "tolerance", label: "Tolerance", min: 0.0, max: 100.0, default: 30.0 },
            OfxParamDescriptor { name: "feather", label: "Feather", min: 0.0, max: 100.0, default: 10.0 },
        ],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.luma_key",
        label: "Luma Key",
        category: OfxCategory::Key,
        params: &[
            OfxParamDescriptor { name: "threshold", label: "Threshold", min: 0.0, max: 100.0, default: 20.0 },
            OfxParamDescriptor { name: "feather", label: "Feather", min: 0.0, max: 100.0, default: 10.0 },
        ],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.swap_color",
        label: "Swap Color",
        category: OfxCategory::Color,
        params: &[
            OfxParamDescriptor { name: "tolerance", label: "Tolerance", min: 0.0, max: 100.0, default: 30.0 },
            OfxParamDescriptor { name: "feather", label: "Feather", min: 0.0, max: 100.0, default: 10.0 },
        ],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.drop_shadow",
        label: "Drop Shadow",
        category: OfxCategory::Distort,
        params: &[
            OfxParamDescriptor { name: "distance", label: "Distance", min: 0.0, max: 200.0, default: 8.0 },
            OfxParamDescriptor { name: "angle", label: "Angle", min: 0.0, max: 360.0, default: 45.0 },
            OfxParamDescriptor { name: "softness", label: "Softness", min: 0.0, max: 100.0, default: 10.0 },
            OfxParamDescriptor { name: "opacity", label: "Opacity", min: 0.0, max: 100.0, default: 75.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.outer_glow",
        label: "Outer Glow",
        category: OfxCategory::Stylize,
        params: &[
            OfxParamDescriptor { name: "size", label: "Size", min: 0.0, max: 250.0, default: 20.0 },
            OfxParamDescriptor { name: "spread", label: "Spread", min: 0.0, max: 100.0, default: 0.0 },
            OfxParamDescriptor { name: "opacity", label: "Opacity", min: 0.0, max: 100.0, default: 75.0 },
            OfxParamDescriptor { name: "range", label: "Range", min: 0.0, max: 100.0, default: 50.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.displacement",
        label: "Displacement Map",
        category: OfxCategory::Distort,
        params: &[
            OfxParamDescriptor { name: "max_horizontal", label: "Max Horizontal", min: -500.0, max: 500.0, default: 50.0 },
            OfxParamDescriptor { name: "max_vertical", label: "Max Vertical", min: -500.0, max: 500.0, default: 50.0 },
            OfxParamDescriptor { name: "source_mode", label: "Map Self/Noise", min: 0.0, max: 1.0, default: 0.0 },
            OfxParamDescriptor { name: "channel_h", label: "Channel H", min: 0.0, max: 4.0, default: 4.0 },
            OfxParamDescriptor { name: "channel_v", label: "Channel V", min: 0.0, max: 4.0, default: 4.0 },
            OfxParamDescriptor { name: "map_scale", label: "Map Scale", min: 0.1, max: 10.0, default: 1.0 },
            OfxParamDescriptor { name: "wrap", label: "Wrap Clamp/Repeat/Mirror", min: 0.0, max: 2.0, default: 0.0 },
            OfxParamDescriptor { name: "evolution", label: "Evolution", min: 0.0, max: 360.0, default: 0.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.perspective",
        label: "Perspective",
        category: OfxCategory::Distort,
        params: &[
            OfxParamDescriptor { name: "skew_x", label: "Skew X", min: -60.0, max: 60.0, default: 0.0 },
            OfxParamDescriptor { name: "skew_y", label: "Skew Y", min: -60.0, max: 60.0, default: 0.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.tiler",
        label: "Tiler",
        category: OfxCategory::Distort,
        params: &[
            OfxParamDescriptor { name: "tiles_x", label: "Tiles X", min: 1.0, max: 32.0, default: 2.0 },
            OfxParamDescriptor { name: "tiles_y", label: "Tiles Y", min: 1.0, max: 32.0, default: 2.0 },
            OfxParamDescriptor { name: "offset_x", label: "Offset X", min: 0.0, max: 1.0, default: 0.0 },
            OfxParamDescriptor { name: "offset_y", label: "Offset Y", min: 0.0, max: 1.0, default: 0.0 },
            OfxParamDescriptor { name: "seed", label: "Random Seed", min: 0.0, max: 100.0, default: 1.0 },
            OfxParamDescriptor { name: "amount", label: "Randomize", min: 0.0, max: 100.0, default: 0.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.warp",
        label: "Warp",
        category: OfxCategory::Distort,
        params: &[
            OfxParamDescriptor { name: "amount", label: "Amount", min: 0.0, max: 100.0, default: 30.0 },
            OfxParamDescriptor { name: "scale", label: "Scale", min: 0.1, max: 10.0, default: 1.0 },
            OfxParamDescriptor { name: "cols", label: "Columns", min: 2.0, max: 8.0, default: 4.0 },
            OfxParamDescriptor { name: "rows", label: "Rows", min: 2.0, max: 8.0, default: 4.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.trim_path",
        label: "Trim Path",
        category: OfxCategory::Stylize,
        params: &[
            OfxParamDescriptor { name: "start", label: "Start", min: 0.0, max: 100.0, default: 0.0 },
            OfxParamDescriptor { name: "end", label: "End", min: 0.0, max: 100.0, default: 100.0 },
            OfxParamDescriptor { name: "offset", label: "Offset", min: 0.0, max: 100.0, default: 0.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.sine_path",
        label: "Sine Path",
        category: OfxCategory::Distort,
        params: &[
            OfxParamDescriptor { name: "amplitude", label: "Amplitude", min: 0.0, max: 200.0, default: 20.0 },
            OfxParamDescriptor { name: "frequency", label: "Frequency", min: 0.1, max: 10.0, default: 1.0 },
            OfxParamDescriptor { name: "phase", label: "Phase", min: 0.0, max: 360.0, default: 0.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.instance_path",
        label: "Instance Path",
        category: OfxCategory::Generate,
        params: &[
            OfxParamDescriptor { name: "count", label: "Count", min: 1.0, max: 32.0, default: 5.0 },
            OfxParamDescriptor { name: "spread", label: "Spread", min: 0.0, max: 100.0, default: 100.0 },
            OfxParamDescriptor { name: "offset", label: "Offset", min: 0.0, max: 100.0, default: 0.0 },
            OfxParamDescriptor { name: "follow", label: "Follow", min: 0.0, max: 100.0, default: 100.0 },
            OfxParamDescriptor { name: "scale", label: "Scale", min: 10.0, max: 200.0, default: 100.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.puppet",
        label: "Puppet Warp",
        category: OfxCategory::Distort,
        params: &[
            OfxParamDescriptor { name: "expansion", label: "Expansion", min: 0.0, max: 2000.0, default: 0.0 },
            OfxParamDescriptor { name: "stiffness", label: "Stiffness", min: 0.5, max: 8.0, default: 2.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.bloom",
        label: "Bloom",
        category: OfxCategory::Stylize,
        params: &[
            OfxParamDescriptor { name: "intensity", label: "Intensity", min: 0.0, max: 100.0, default: 40.0 },
            OfxParamDescriptor { name: "radius", label: "Radius", min: 0.0, max: 100.0, default: 10.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.noise",
        label: "Noise Generator",
        category: OfxCategory::Stylize,
        params: &[OfxParamDescriptor { name: "amount", label: "Amount", min: 0.0, max: 100.0, default: 25.0 }],
        spatial: false,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.vignette",
        label: "Vignette",
        category: OfxCategory::Stylize,
        params: &[
            OfxParamDescriptor { name: "amount", label: "Amount", min: 0.0, max: 100.0, default: 50.0 },
            OfxParamDescriptor { name: "softness", label: "Softness", min: 0.0, max: 100.0, default: 50.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.cel_shading",
        label: "Cel Shading",
        category: OfxCategory::Stylize,
        params: &[
            OfxParamDescriptor { name: "levels", label: "Levels", min: 2.0, max: 8.0, default: 4.0 },
            OfxParamDescriptor { name: "edge", label: "Edge", min: 0.0, max: 100.0, default: 60.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.oil_paint",
        label: "Oil Painting",
        category: OfxCategory::Stylize,
        params: &[
            OfxParamDescriptor { name: "radius", label: "Radius", min: 1.0, max: 4.0, default: 2.0 },
            OfxParamDescriptor { name: "amount", label: "Amount", min: 0.0, max: 100.0, default: 100.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.checkerboard",
        label: "Checkerboard",
        category: OfxCategory::Generate,
        params: &[OfxParamDescriptor { name: "size", label: "Size", min: 2.0, max: 512.0, default: 32.0 }],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.gradient_ramp",
        label: "Gradient Ramp",
        category: OfxCategory::Generate,
        params: &[
            OfxParamDescriptor { name: "angle", label: "Angle", min: -360.0, max: 360.0, default: 90.0 },
            OfxParamDescriptor { name: "center_x", label: "Center X", min: 0.0, max: 100.0, default: 50.0 },
            OfxParamDescriptor { name: "center_y", label: "Center Y", min: 0.0, max: 100.0, default: 50.0 },
            OfxParamDescriptor { name: "radius", label: "Radius", min: 1.0, max: 200.0, default: 71.0 },
            OfxParamDescriptor { name: "dither", label: "Dither", min: 0.0, max: 100.0, default: 30.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.text_outline",
        label: "Text Outline",
        category: OfxCategory::Text,
        params: &[
            OfxParamDescriptor { name: "width", label: "Width", min: 0.0, max: 50.0, default: 3.0 },
            OfxParamDescriptor { name: "offset", label: "Offset", min: -50.0, max: 50.0, default: 0.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.text_bevel",
        label: "Text Bevel",
        category: OfxCategory::Text,
        params: &[
            OfxParamDescriptor { name: "strength", label: "Strength", min: 0.0, max: 100.0, default: 60.0 },
            OfxParamDescriptor { name: "softness", label: "Softness", min: 0.0, max: 100.0, default: 30.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.text_split_animator",
        label: "Text Split Animator (2D)",
        category: OfxCategory::Text,
        params: &[
            OfxParamDescriptor { name: "progress", label: "Progress", min: 0.0, max: 100.0, default: 0.0 },
            OfxParamDescriptor { name: "spread", label: "Spread / Overlap", min: 0.0, max: 100.0, default: 40.0 },
            OfxParamDescriptor { name: "position_x", label: "Position X", min: -1000.0, max: 1000.0, default: 0.0 },
            OfxParamDescriptor { name: "position_y", label: "Position Y", min: -1000.0, max: 1000.0, default: -50.0 },
            OfxParamDescriptor { name: "rotation", label: "Rotation", min: -360.0, max: 360.0, default: -25.0 },
            OfxParamDescriptor { name: "opacity", label: "Opacity", min: 0.0, max: 100.0, default: 0.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.custom.glsl",
        label: "Custom GLSL Shader",
        category: OfxCategory::Custom,
        params: &[
            OfxParamDescriptor { name: "param1", label: "Param 1 (Speed/Time)", min: -1000.0, max: 1000.0, default: 1.0 },
            OfxParamDescriptor { name: "param2", label: "Param 2 (Intensity)", min: -1000.0, max: 1000.0, default: 50.0 },
            OfxParamDescriptor { name: "param3", label: "Param 3 (Scale/Freq)", min: -1000.0, max: 1000.0, default: 1.0 },
            OfxParamDescriptor { name: "param4", label: "Param 4 (Tint/Phase)", min: -1000.0, max: 1000.0, default: 100.0 },
        ],
        spatial: true,
    },
    OfxEffectDescriptor {
        id: "net.sf.openfx.custom.shader_lab",
        label: "Shader Lab",
        category: OfxCategory::Custom,
        params: NO_PARAMS,
        spatial: true,
    },
];

/// Look up a plug-in descriptor by id (legacy + stock suites).
pub fn ofx_lookup(id: &str) -> Option<&'static OfxEffectDescriptor> {
    OFX_SUITE
        .iter()
        .find(|d| d.id == id)
        .or_else(|| STOCK_SUITE.iter().find(|d| d.id == id))
}

/// Compact descriptor constructor for the stock suite.
const fn sd(
    id: &'static str,
    label: &'static str,
    category: OfxCategory,
    params: &'static [OfxParamDescriptor],
    spatial: bool,
) -> OfxEffectDescriptor {
    OfxEffectDescriptor { id, label, category, params, spatial }
}

macro_rules! pd {
    ($n:expr, $l:expr, $a:expr, $b:expr, $c:expr) => {
        OfxParamDescriptor { name: $n, label: $l, min: $a, max: $b, default: $c }
    };
}

/// Stock suite descriptors, parallel to [`crate::stock::StockPlugin`].
/// Every entry needs `params` in the exact order the CPU/WGSL kernels read
/// them (index-based), and matching `colors` documented in
/// [`crate::stock::stock_color_slots`].
pub const STOCK_SUITE: &[OfxEffectDescriptor] = &[
    // Color
    sd("net.sf.openfx.curves", "Curves", OfxCategory::Color, &[
        pd!("shadows", "Shadows", -100.0, 100.0, 0.0),
        pd!("darks", "Darks", -100.0, 100.0, 0.0),
        pd!("mids", "Mids", -100.0, 100.0, 0.0),
        pd!("lights", "Lights", -100.0, 100.0, 0.0),
        pd!("highlights", "Highlights", -100.0, 100.0, 0.0),
    ], false),
    sd("net.sf.openfx.color_balance", "Color Balance", OfxCategory::Color, &[
        pd!("shadows_cyan_red", "Shadows Cyan / Red", -100.0, 100.0, 0.0),
        pd!("shadows_magenta_green", "Shadows Magenta / Green", -100.0, 100.0, 0.0),
        pd!("shadows_yellow_blue", "Shadows Yellow / Blue", -100.0, 100.0, 0.0),
        pd!("midtones_cyan_red", "Midtones Cyan / Red", -100.0, 100.0, 0.0),
        pd!("midtones_magenta_green", "Midtones Magenta / Green", -100.0, 100.0, 0.0),
        pd!("midtones_yellow_blue", "Midtones Yellow / Blue", -100.0, 100.0, 0.0),
        pd!("highlights_cyan_red", "Highlights Cyan / Red", -100.0, 100.0, 0.0),
        pd!("highlights_magenta_green", "Highlights Magenta / Green", -100.0, 100.0, 0.0),
        pd!("highlights_yellow_blue", "Highlights Yellow / Blue", -100.0, 100.0, 0.0),
    ], false),
    sd("net.sf.openfx.color_wheels", "Color Wheels", OfxCategory::Color, &[
        pd!("shadows", "Shadows", -100.0, 100.0, 0.0),
        pd!("midtones", "Midtones", -100.0, 100.0, 0.0),
        pd!("highlights", "Highlights", -100.0, 100.0, 0.0),
    ], false),
    sd("net.sf.openfx.temperature_tint", "Temperature / Tint", OfxCategory::Color, &[
        pd!("temperature", "Temperature", -100.0, 100.0, 0.0),
        pd!("tint", "Tint", -100.0, 100.0, 0.0),
    ], false),
    sd("net.sf.openfx.posterize", "Posterize", OfxCategory::Color, &[
        pd!("levels", "Levels", 2.0, 32.0, 6.0),
    ], false),
    sd("net.sf.openfx.threshold", "Threshold", OfxCategory::Color, &[
        pd!("threshold", "Threshold", 0.0, 100.0, 50.0),
        pd!("feather", "Feather", 0.0, 100.0, 0.0),
    ], false),
    // Blur (spatial convolutions)
    sd("net.sf.openfx.box_blur", "Box Blur", OfxCategory::Blur, &[
        pd!("radius", "Radius", 0.0, 60.0, 5.0),
    ], true),
    sd("net.sf.openfx.directional_blur", "Directional Blur", OfxCategory::Blur, &[
        pd!("angle", "Angle", 0.0, 360.0, 0.0),
        pd!("length", "Length", 0.0, 200.0, 20.0),
    ], true),
    sd("net.sf.openfx.radial_blur", "Radial Blur", OfxCategory::Blur, &[
        pd!("amount", "Amount", 0.0, 100.0, 30.0),
        pd!("falloff", "Falloff", 0.0, 100.0, 50.0),
    ], true),
    sd("net.sf.openfx.zoom_blur", "Zoom Blur", OfxCategory::Blur, &[
        pd!("amount", "Amount", 0.0, 100.0, 30.0),
        pd!("falloff", "Falloff", 0.0, 100.0, 50.0),
    ], true),
    sd("net.sf.openfx.motion_blur", "Motion Blur", OfxCategory::Blur, &[
        pd!("angle", "Angle", 0.0, 360.0, 0.0),
        pd!("length", "Length", 0.0, 200.0, 20.0),
    ], true),
    sd("net.sf.openfx.defocus", "Lens Defocus", OfxCategory::Blur, &[
        pd!("radius", "Radius", 0.0, 40.0, 5.0),
    ], true),
    sd("net.sf.openfx.bokeh", "Bokeh Blur", OfxCategory::Blur, &[
        pd!("radius", "Radius", 0.0, 40.0, 8.0),
        pd!("threshold", "Highlight Threshold", 0.0, 100.0, 70.0),
    ], true),
    sd("net.sf.openfx.bilateral", "Bilateral Blur", OfxCategory::Blur, &[
        pd!("radius", "Radius", 0.0, 10.0, 3.0),
        pd!("range", "Range", 0.0, 100.0, 30.0),
    ], true),
    // Light
    sd("net.sf.openfx.glow", "Glow", OfxCategory::Light, &[
        pd!("intensity", "Intensity", 0.0, 100.0, 50.0),
        pd!("radius", "Radius", 0.0, 60.0, 12.0),
        pd!("threshold", "Threshold", 0.0, 100.0, 55.0),
    ], true),
    sd("net.sf.openfx.glare", "Glare", OfxCategory::Light, &[
        pd!("intensity", "Intensity", 0.0, 100.0, 60.0),
        pd!("length", "Length", 0.0, 200.0, 60.0),
        pd!("angle", "Angle", 0.0, 360.0, 0.0),
    ], true),
    sd("net.sf.openfx.glint", "Glint", OfxCategory::Light, &[
        pd!("intensity", "Intensity", 0.0, 100.0, 70.0),
        pd!("size", "Size", 0.0, 100.0, 30.0),
    ], true),
    sd("net.sf.openfx.light_rays", "Light Rays", OfxCategory::Light, &[
        pd!("intensity", "Intensity", 0.0, 200.0, 50.0),
        pd!("length", "Length", 0.0, 200.0, 80.0),
        pd!("angle", "Angle", 0.0, 360.0, 90.0),
        pd!("threshold", "Threshold", 0.0, 100.0, 55.0),
        pd!("knee", "Soft Knee", 0.0, 50.0, 12.0),
        pd!("density", "Density", 0.0, 100.0, 60.0),
        pd!("decay", "Decay", 80.0, 100.0, 96.0),
        pd!("exposure", "Exposure", 0.0, 200.0, 80.0),
        pd!("samples", "Samples", 8.0, 64.0, 24.0),
        pd!("jitter", "Dither", 0.0, 1.0, 1.0),
        pd!("blend", "Blend Add/Screen", 0.0, 1.0, 0.0),
        pd!("mix", "Mix", 0.0, 100.0, 80.0),
    ], true),
    sd("net.sf.openfx.god_rays", "God Rays", OfxCategory::Light, &[
        pd!("intensity", "Intensity", 0.0, 200.0, 50.0),
        pd!("density", "Density", 0.0, 100.0, 60.0),
        pd!("decay", "Decay", 80.0, 100.0, 96.0),
        pd!("center_x", "Center X", 0.0, 100.0, 50.0),
        pd!("center_y", "Center Y", 0.0, 100.0, 35.0),
        pd!("threshold", "Threshold", 0.0, 100.0, 55.0),
        pd!("knee", "Soft Knee", 0.0, 50.0, 12.0),
        pd!("weight", "Weight", 0.0, 100.0, 40.0),
        pd!("exposure", "Exposure", 0.0, 200.0, 80.0),
        pd!("samples", "Samples", 8.0, 64.0, 32.0),
        pd!("jitter", "Dither", 0.0, 1.0, 1.0),
        pd!("beams", "Beam Structure", 0.0, 100.0, 30.0),
    ], true),
    sd("net.sf.openfx.lens_flare", "Lens Flare", OfxCategory::Light, &[
        pd!("intensity", "Intensity", 0.0, 200.0, 60.0),
        pd!("position", "Position", 0.0, 100.0, 30.0),
        pd!("ghosts", "Ghosts", 0.0, 10.0, 4.0),
        pd!("center_x", "Center X", 0.0, 100.0, 50.0),
        pd!("center_y", "Center Y", 0.0, 100.0, 50.0),
        pd!("threshold", "Threshold", 0.0, 100.0, 60.0),
        pd!("dispersal", "Dispersal", 0.0, 100.0, 55.0),
        pd!("halo_width", "Halo Width", 0.0, 100.0, 40.0),
        pd!("halo_intensity", "Halo", 0.0, 100.0, 45.0),
        pd!("chroma", "Chromatic Aberration", 0.0, 8.0, 1.5),
        pd!("streak_length", "Streak Length", 0.0, 200.0, 60.0),
        pd!("streak_intensity", "Streak", 0.0, 100.0, 40.0),
    ], true),
    sd("net.sf.openfx.long_shadow", "Long Shadow", OfxCategory::Light, &[
        pd!("angle", "Angle", 0.0, 360.0, 135.0),
        pd!("distance", "Distance", 0.0, 1024.0, 120.0),
        pd!("steps", "Steps", 1.0, 64.0, 32.0),
        pd!("fade", "Fade Curve", 0.0, 100.0, 60.0),
        pd!("opacity", "Opacity", 0.0, 100.0, 85.0),
        pd!("softness", "Softness", 0.0, 60.0, 2.0),
        pd!("expand", "Expand", -50.0, 50.0, 0.0),
        pd!("mode", "Mode Behind/Cutout/Only", 0.0, 2.0, 0.0),
        pd!("strength", "Strength", 0.0, 100.0, 100.0),
        pd!("step_size", "Step Size", 1.0, 8.0, 1.0),
    ], true),
    sd("net.sf.openfx.saber", "Saber", OfxCategory::Light, &[
        pd!("core_width", "Core Width", 0.0, 50.0, 6.0),
        pd!("glow_width", "Glow Width", 0.0, 200.0, 42.0),
        pd!("softness", "Softness", 0.0, 100.0, 35.0),
        pd!("threshold", "Core Threshold", 0.0, 100.0, 45.0),
        pd!("distort_amount", "Distort Amount", 0.0, 100.0, 25.0),
        pd!("distort_scale", "Distort Scale", 0.1, 10.0, 2.0),
        pd!("distort_speed", "Distort Speed", 0.0, 100.0, 30.0),
        pd!("flicker_amount", "Flicker Amount", 0.0, 100.0, 15.0),
        pd!("flicker_speed", "Flicker Speed", 0.0, 100.0, 30.0),
        pd!("evolution", "Evolution", 0.0, 360.0, 0.0),
        pd!("intensity", "Intensity", 0.0, 200.0, 100.0),
        pd!("blend", "Blend Add/Screen", 0.0, 1.0, 0.0),
    ], true),
    sd("net.sf.openfx.light_leak", "Light Leak", OfxCategory::Color, &[
        pd!("intensity", "Intensity", 0.0, 100.0, 40.0),
        pd!("hue_shift", "Hue", -180.0, 180.0, -20.0),
        pd!("position", "Position", 0.0, 100.0, 15.0),
    ], true),
    sd("net.sf.openfx.streaks", "Streaks", OfxCategory::Light, &[
        pd!("intensity", "Intensity", 0.0, 100.0, 60.0),
        pd!("length", "Length", 0.0, 200.0, 80.0),
        pd!("angle", "Angle", 0.0, 360.0, 0.0),
    ], true),
    sd("net.sf.openfx.halo", "Halo", OfxCategory::Light, &[
        pd!("intensity", "Intensity", 0.0, 100.0, 50.0),
        pd!("radius", "Radius", 0.0, 100.0, 35.0),
        pd!("warmth", "Warmth", -100.0, 100.0, 40.0),
    ], true),
    // Distortion
    sd("net.sf.openfx.turbulent_displace", "Turbulent Displace", OfxCategory::Distort, &[
        pd!("amount", "Amount", 0.0, 200.0, 40.0),
        pd!("scale", "Scale", 0.1, 10.0, 1.5),
        pd!("seed", "Seed", 0.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.wave", "Wave", OfxCategory::Distort, &[
        pd!("amplitude", "Amplitude", 0.0, 200.0, 12.0),
        pd!("wavelength", "Wavelength", 2.0, 512.0, 64.0),
        pd!("direction", "Direction", 0.0, 360.0, 90.0),
    ], true),
    sd("net.sf.openfx.ripple", "Ripple", OfxCategory::Distort, &[
        pd!("amplitude", "Amplitude", 0.0, 200.0, 10.0),
        pd!("wavelength", "Wavelength", 2.0, 512.0, 48.0),
    ], true),
    sd("net.sf.openfx.twirl", "Twirl", OfxCategory::Distort, &[
        pd!("angle", "Angle", -720.0, 720.0, 120.0),
        pd!("radius", "Radius", 0.0, 100.0, 40.0),
    ], true),
    sd("net.sf.openfx.bulge", "Bulge", OfxCategory::Distort, &[
        pd!("amount", "Amount", -100.0, 100.0, 60.0),
        pd!("radius", "Radius", 0.0, 100.0, 40.0),
    ], true),
    sd("net.sf.openfx.spherize", "Spherize", OfxCategory::Distort, &[
        pd!("amount", "Amount", -100.0, 100.0, 70.0),
        pd!("radius", "Radius", 0.0, 100.0, 40.0),
    ], true),
    sd("net.sf.openfx.lens_distortion", "Lens Distortion", OfxCategory::Distort, &[
        pd!("amount", "Distortion", -100.0, 100.0, 25.0),
        pd!("zoom", "Zoom", 50.0, 200.0, 100.0),
    ], true),
    sd("net.sf.openfx.chromatic_aberration", "Chromatic Aberration", OfxCategory::Distort, &[
        pd!("amount", "Amount", 0.0, 100.0, 8.0),
        pd!("angle", "Angle", 0.0, 360.0, 0.0),
    ], true),
    sd("net.sf.openfx.mesh_warp", "Mesh Warp", OfxCategory::Distort, &[
        pd!("density", "Grid Density", 2.0, 8.0, 4.0),
        pd!("warp", "Warp", 0.0, 100.0, 40.0),
        pd!("ripple", "Ripple", 0.0, 100.0, 25.0),
    ], true),
    sd("net.sf.openfx.liquify", "Liquify", OfxCategory::Distort, &[
        pd!("pos_x", "Center X", -100.0, 100.0, 0.0),
        pd!("pos_y", "Center Y", -100.0, 100.0, 0.0),
        pd!("radius", "Radius", 0.0, 100.0, 30.0),
        pd!("strength", "Strength", -100.0, 100.0, 50.0),
        pd!("twirl", "Twirl", -180.0, 180.0, 0.0),
    ], true),
    // Stylize
    sd("net.sf.openfx.edge_detect", "Edge Detect", OfxCategory::Stylize, &[
        pd!("threshold", "Threshold", 0.0, 100.0, 20.0),
        pd!("invert", "Invert", 0.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.cartoon", "Cartoon", OfxCategory::Stylize, &[
        pd!("levels", "Levels", 2.0, 8.0, 4.0),
        pd!("edge", "Edge", 0.0, 100.0, 50.0),
    ], true),
    sd("net.sf.openfx.halftone", "Halftone", OfxCategory::Stylize, &[
        pd!("size", "Dot Size", 2.0, 32.0, 6.0),
        pd!("angle", "Angle", 0.0, 90.0, 15.0),
    ], true),
    sd("net.sf.openfx.sketch", "Sketch", OfxCategory::Stylize, &[
        pd!("intensity", "Intensity", 0.0, 100.0, 70.0),
        pd!("invert", "Invert", 0.0, 100.0, 100.0),
    ], true),
    sd("net.sf.openfx.emboss", "Emboss", OfxCategory::Stylize, &[
        pd!("strength", "Strength", 0.0, 100.0, 60.0),
        pd!("angle", "Angle", 0.0, 360.0, 135.0),
    ], true),
    sd("net.sf.openfx.pixelate", "Pixelate", OfxCategory::Stylize, &[
        pd!("size", "Cell Size", 1.0, 128.0, 12.0),
    ], true),
    sd("net.sf.openfx.mosaic", "Mosaic", OfxCategory::Stylize, &[
        pd!("size", "Tile Size", 1.0, 128.0, 12.0),
        pd!("levels", "Color Levels", 2.0, 16.0, 6.0),
    ], true),
    sd("net.sf.openfx.vhs", "VHS", OfxCategory::Stylize, &[
        pd!("tracking", "Tracking", 0.0, 100.0, 25.0),
        pd!("noise_amount", "Noise", 0.0, 100.0, 30.0),
        pd!("chroma_shift", "Chroma Shift", 0.0, 40.0, 6.0),
    ], true),
    sd("net.sf.openfx.rgb_split", "RGB Split", OfxCategory::Stylize, &[
        pd!("amount", "Amount", 0.0, 100.0, 10.0),
        pd!("angle", "Angle", 0.0, 360.0, 0.0),
    ], true),
    sd("net.sf.openfx.scanlines", "Scanlines", OfxCategory::Stylize, &[
        pd!("size", "Line Size", 1.0, 16.0, 3.0),
        pd!("intensity", "Intensity", 0.0, 100.0, 40.0),
    ], true),
    sd("net.sf.openfx.glitch", "Glitch", OfxCategory::Stylize, &[
        pd!("amount", "Amount", 0.0, 100.0, 40.0),
        pd!("seed", "Seed", 0.0, 100.0, 1.0),
    ], true),
    // Noise / film
    sd("net.sf.openfx.film_grain", "Film Grain", OfxCategory::Noise, &[
        pd!("amount", "Amount", 0.0, 100.0, 25.0),
        pd!("size", "Grain Size", 1.0, 8.0, 1.0),
        pd!("monochrome", "Monochrome", 0.0, 100.0, 100.0),
    ], true),
    sd("net.sf.openfx.fractal_noise", "Fractal Noise", OfxCategory::Noise, &[
        pd!("scale", "Scale", 0.1, 10.0, 1.5),
        pd!("octaves", "Octaves", 1.0, 8.0, 4.0),
        pd!("seed", "Seed", 0.0, 100.0, 0.0),
        pd!("evolution", "Evolution", 0.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.turbulence", "Turbulence", OfxCategory::Noise, &[
        pd!("scale", "Scale", 0.1, 10.0, 1.5),
        pd!("amount", "Amount", 0.0, 100.0, 40.0),
        pd!("seed", "Seed", 0.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.dust", "Dust", OfxCategory::Noise, &[
        pd!("amount", "Amount", 0.0, 100.0, 20.0),
        pd!("size", "Size", 1.0, 12.0, 2.0),
        pd!("seed", "Seed", 0.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.scratches", "Scratches", OfxCategory::Noise, &[
        pd!("amount", "Amount", 0.0, 100.0, 20.0),
        pd!("length", "Length", 0.0, 100.0, 40.0),
        pd!("seed", "Seed", 0.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.film_damage", "Film Damage", OfxCategory::Noise, &[
        pd!("amount", "Amount", 0.0, 100.0, 30.0),
        pd!("seed", "Seed", 0.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.flicker", "Flicker", OfxCategory::Noise, &[
        pd!("amount", "Amount", 0.0, 100.0, 15.0),
        pd!("speed", "Speed", 0.0, 100.0, 30.0),
    ], true),
    sd("net.sf.openfx.gate_weave", "Gate Weave", OfxCategory::Noise, &[
        pd!("amount", "Amount", 0.0, 100.0, 20.0),
        pd!("speed", "Speed", 0.0, 100.0, 30.0),
    ], true),
    // Keying / matte
    sd("net.sf.openfx.difference_key", "Difference Key", OfxCategory::Key, &[
        pd!("key_luma", "Key Luminance", 0.0, 100.0, 50.0),
        pd!("threshold", "Threshold", 0.0, 100.0, 15.0),
        pd!("feather", "Feather", 0.0, 100.0, 10.0),
    ], false),
    sd("net.sf.openfx.spill_suppress", "Spill Suppression", OfxCategory::Key, &[
        pd!("amount", "Amount", 0.0, 100.0, 80.0),
    ], false),
    sd("net.sf.openfx.matte_choker", "Matte Choker", OfxCategory::Key, &[
        pd!("choke", "Choke", -100.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.matte_blur", "Matte Blur", OfxCategory::Key, &[
        pd!("radius", "Radius", 0.0, 40.0, 4.0),
    ], true),
    sd("net.sf.openfx.erode", "Erode", OfxCategory::Key, &[
        pd!("radius", "Radius", 0.0, 40.0, 2.0),
    ], true),
    sd("net.sf.openfx.dilate", "Dilate", OfxCategory::Key, &[
        pd!("radius", "Radius", 0.0, 40.0, 2.0),
    ], true),
    sd("net.sf.openfx.key_cleaner", "Key Cleaner", OfxCategory::Key, &[
        pd!("despill", "Despill", 0.0, 100.0, 70.0),
        pd!("choke", "Choke", -100.0, 100.0, 0.0),
    ], true),
    // Transform / spatial
    sd("net.sf.openfx.transform", "Transform", OfxCategory::Spatial, &[
        pd!("pos_x", "Position X", -500.0, 500.0, 0.0),
        pd!("pos_y", "Position Y", -500.0, 500.0, 0.0),
        pd!("scale", "Scale", 1.0, 400.0, 100.0),
        pd!("rotation", "Rotation", -360.0, 360.0, 0.0),
    ], true),
    sd("net.sf.openfx.crop", "Crop", OfxCategory::Spatial, &[
        pd!("left", "Left", 0.0, 100.0, 0.0),
        pd!("top", "Top", 0.0, 100.0, 0.0),
        pd!("right", "Right", 0.0, 100.0, 0.0),
        pd!("bottom", "Bottom", 0.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.corner_pin", "Corner Pin", OfxCategory::Spatial, &[
        pd!("ul_x", "Upper Left X", 0.0, 1.0, 0.0),
        pd!("ul_y", "Upper Left Y", 0.0, 1.0, 0.0),
        pd!("ur_x", "Upper Right X", 0.0, 1.0, 1.0),
        pd!("ur_y", "Upper Right Y", 0.0, 1.0, 0.0),
        pd!("lr_x", "Lower Right X", 0.0, 1.0, 1.0),
        pd!("lr_y", "Lower Right Y", 0.0, 1.0, 1.0),
        pd!("ll_x", "Lower Left X", 0.0, 1.0, 0.0),
        pd!("ll_y", "Lower Left Y", 0.0, 1.0, 1.0),
    ], true),
    sd("net.sf.openfx.card_3d", "Card 3D", OfxCategory::Spatial, &[
        pd!("rotation_x", "Rotation X", -180.0, 180.0, 0.0),
        pd!("rotation_y", "Rotation Y", -180.0, 180.0, 0.0),
        pd!("distance", "Distance", 50.0, 800.0, 300.0),
        pd!("pivot_x", "Pivot X", 0.0, 100.0, 50.0),
        pd!("pivot_y", "Pivot Y", 0.0, 100.0, 50.0),
        pd!("cull_backface", "Cull Backface", 0.0, 1.0, 1.0),
    ], true),
    sd("net.sf.openfx.mirror", "Mirror", OfxCategory::Spatial, &[
        pd!("mode", "Mode", 0.0, 2.0, 0.0),
        pd!("center", "Center", 0.0, 100.0, 50.0),
    ], true),
    sd("net.sf.openfx.repeat", "Repeat", OfxCategory::Spatial, &[
        pd!("tiles_x", "Tiles X", 1.0, 32.0, 2.0),
        pd!("tiles_y", "Tiles Y", 1.0, 32.0, 2.0),
    ], true),
    sd("net.sf.openfx.offset", "Offset", OfxCategory::Spatial, &[
        pd!("shift_x", "Shift X", -100.0, 100.0, 0.0),
        pd!("shift_y", "Shift Y", -100.0, 100.0, 0.0),
    ], true),
    sd("net.sf.openfx.reframe", "Reframe", OfxCategory::Spatial, &[
        pd!("scale", "Scale", 10.0, 400.0, 100.0),
        pd!("offset_x", "Offset X", -100.0, 100.0, 0.0),
        pd!("offset_y", "Offset Y", -100.0, 100.0, 0.0),
    ], true),
    // Cleanup / repair
    sd("net.sf.openfx.denoise", "Denoise", OfxCategory::Cleanup, &[
        pd!("radius", "Radius", 0.0, 10.0, 2.0),
        pd!("strength", "Strength", 0.0, 100.0, 80.0),
    ], true),
    sd("net.sf.openfx.deband", "Deband", OfxCategory::Cleanup, &[
        pd!("radius", "Radius", 0.0, 20.0, 6.0),
        pd!("threshold", "Threshold", 0.0, 100.0, 25.0),
    ], true),
    sd("net.sf.openfx.degrain", "Degrain", OfxCategory::Cleanup, &[
        pd!("amount", "Amount", 0.0, 100.0, 50.0),
    ], true),
    sd("net.sf.openfx.deblur", "Deblur", OfxCategory::Cleanup, &[
        pd!("amount", "Amount", 0.0, 200.0, 60.0),
        pd!("radius", "Radius", 0.0, 10.0, 2.0),
    ], true),
    sd("net.sf.openfx.dust_removal", "Dust Removal", OfxCategory::Cleanup, &[
        pd!("threshold", "Threshold", 0.0, 100.0, 20.0),
        pd!("radius", "Radius", 0.0, 10.0, 2.0),
    ], true),
    sd("net.sf.openfx.scratch_removal", "Scratch Removal", OfxCategory::Cleanup, &[
        pd!("threshold", "Threshold", 0.0, 100.0, 20.0),
        pd!("width", "Width", 1.0, 12.0, 3.0),
    ], true),
    sd("net.sf.openfx.dead_pixel", "Dead Pixel Removal", OfxCategory::Cleanup, &[
        pd!("threshold", "Threshold", 0.0, 100.0, 30.0),
    ], true),
    // Generators (fill the frame; color slots documented in stock.rs)
    sd("net.sf.openfx.solid", "Solid", OfxCategory::Generate, &[
        pd!("opacity", "Opacity", 0.0, 100.0, 100.0),
    ], true),
    sd("net.sf.openfx.fractal_gen", "Fractal", OfxCategory::Generate, &[
        pd!("scale", "Scale", 0.1, 10.0, 1.5),
        pd!("octaves", "Octaves", 1.0, 8.0, 4.0),
        pd!("seed", "Seed", 0.0, 100.0, 0.0),
        pd!("opacity", "Opacity", 0.0, 100.0, 100.0),
    ], true),
    sd("net.sf.openfx.grid", "Grid", OfxCategory::Generate, &[
        pd!("size", "Cell Size", 2.0, 256.0, 32.0),
        pd!("line", "Line Width", 1.0, 32.0, 2.0),
        pd!("opacity", "Opacity", 0.0, 100.0, 100.0),
    ], true),
    sd("net.sf.openfx.shapes", "Shapes", OfxCategory::Generate, &[
        pd!("shape", "Shape", 0.0, 2.0, 0.0),
        pd!("size", "Size", 0.0, 100.0, 40.0),
        pd!("softness", "Softness", 0.0, 100.0, 10.0),
        pd!("opacity", "Opacity", 0.0, 100.0, 100.0),
    ], true),
    sd("net.sf.openfx.plasma", "Plasma", OfxCategory::Generate, &[
        pd!("scale", "Scale", 0.1, 10.0, 1.5),
        pd!("speed", "Speed", 0.0, 100.0, 20.0),
        pd!("opacity", "Opacity", 0.0, 100.0, 100.0),
    ], true),
    sd("net.sf.openfx.particles", "Particles", OfxCategory::Generate, &[
        pd!("count", "Count", 1.0, 500.0, 80.0),
        pd!("size", "Size", 1.0, 16.0, 3.0),
        pd!("speed", "Speed", 0.0, 100.0, 30.0),
    ], true),
];

/// All plug-ins in one category, in suite order (legacy + stock).
pub fn ofx_in_category(category: OfxCategory) -> Vec<&'static OfxEffectDescriptor> {
    OFX_SUITE
        .iter()
        .chain(STOCK_SUITE.iter())
        .filter(|d| d.category == category)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::EffectType;
    use crate::Color;
    use std::collections::HashSet;

    #[test]
    fn suite_ids_unique_and_namespaced() {
        let mut seen = HashSet::new();
        assert!(!OFX_SUITE.is_empty());
        for d in OFX_SUITE {
            assert!(d.id.starts_with("net.sf.openfx."), "bad id {}", d.id);
            assert!(seen.insert(d.id), "duplicate id {}", d.id);
        }
    }

    #[test]
    fn lookup_roundtrips_every_entry() {
        for d in OFX_SUITE {
            assert_eq!(ofx_lookup(d.id).map(|e| e.label), Some(d.label));
        }
        assert!(ofx_lookup("net.sf.openfx.nope").is_none());
    }

    #[test]
    fn every_category_has_entries() {
        for cat in [
            OfxCategory::Blur,
            OfxCategory::Color,
            OfxCategory::Light,
            OfxCategory::Key,
            OfxCategory::Distort,
            OfxCategory::Stylize,
            OfxCategory::Noise,
            OfxCategory::Generate,
            OfxCategory::Spatial,
            OfxCategory::Cleanup,
            OfxCategory::Text,
            OfxCategory::Custom,
        ] {
            assert!(!ofx_in_category(cat).is_empty(), "empty {:?}", cat);
        }
    }

    fn every_effect_type() -> Vec<EffectType> {
        vec![
            EffectType::gaussian_blur(5.0),
            EffectType::brightness_contrast(0.0, 0.0),
            EffectType::tint(Color::BLACK, Color::WHITE, 100.0),
            EffectType::invert(100.0),
            EffectType::drop_shadow(8.0, 45.0, 10.0, 75.0, Color::BLACK),
            EffectType::glsl_shader("void main() {}", 1.0, 2.0, 3.0, 4.0),
            EffectType::shader_lab("void main() {}"),
            EffectType::displacement(10.0, 10.0),
            EffectType::chroma_key(Color::GREEN, 30.0, 10.0),
            EffectType::luma_key(20.0, 10.0),
            EffectType::swap_color(Color::GREEN, Color::RED, 30.0, 10.0),
            EffectType::noise_generator(25.0, true),
            EffectType::checkerboard(32.0, Color::BLACK, Color::WHITE),
            EffectType::gradient_ramp(Color::BLACK, Color::WHITE, 90.0),
            EffectType::perspective(0.0, 0.0),
            EffectType::text_outline(3.0, Color::BLACK),
            EffectType::text_bevel(60.0, 30.0),
            EffectType::bloom(40.0, 10.0),
            EffectType::tiler(2.0, 2.0),
            EffectType::warp(30.0, 1.0),
            EffectType::exposure(0.0),
            EffectType::vibrance(0.0),
            EffectType::levels(0.0, 255.0, 1.0, 0.0, 255.0),
            EffectType::hue_saturation(0.0, 0.0, 0.0),
            EffectType::sharpen(50.0, 2.0),
            EffectType::vignette(50.0, 50.0),
        ]
    }

    #[test]
    fn every_effect_type_has_registered_plugin() {
        for t in every_effect_type() {
            let desc = ofx_lookup(t.ofx_plugin_id())
                .unwrap_or_else(|| panic!("unregistered plugin for {}", t.type_name()));
            assert_eq!(desc.label, t.type_name());
        }
        // Each suite is internally unique (legacy suite keeps the four
        // descriptors that double as the stock ids' canonical entries —
        // lookup prefers legacy, params match by name).
        for suite in [OFX_SUITE, STOCK_SUITE] {
            let mut seen = std::collections::HashSet::new();
            for d in suite {
                assert!(seen.insert(d.id), "duplicate plug-in id {}", d.id);
            }
        }
    }

    #[test]
    fn descriptor_params_resolve_to_properties() {
        for t in every_effect_type() {
            // Skip the dynamic-uniform plug-in (no static Property tracks).
            if matches!(t, EffectType::ShaderLab { .. }) {
                continue;
            }
            let desc = ofx_lookup(t.ofx_plugin_id()).unwrap();
            let fx = crate::effect::Effect::new("e", desc.label, t);
            for p in desc.params {
                assert!(
                    fx.get_param_property(p.name).is_some(),
                    "{} missing param {}",
                    desc.id,
                    p.name
                );
            }
        }
    }
}
