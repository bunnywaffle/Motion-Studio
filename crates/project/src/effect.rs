use crate::color::Color;
use crate::layer::{FillGradient, GradientStop, GradientType};
use crate::property::Property;
use crate::shader::{parse_shader_params, ShaderParam, ShaderParamValue};
use crate::stock::{stock_default_color, StockPlugin};
use crate::warp::WarpPin;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const fn default_true() -> bool {
    true
}

fn default_tile_offset() -> Property<f32> {
    Property::new("Tile Offset", 0.0)
}

fn default_tile_seed() -> Property<f32> {
    Property::new("Random Seed", 1.0)
}

fn default_tile_amount() -> Property<f32> {
    Property::new("Randomize", 0.0)
}

fn default_text_split_seed() -> Property<f32> {
    Property::new("Random Seed", 12487.0)
}

fn default_text_split_progress() -> Property<f32> {
    Property::new("Progress", 0.0)
}

fn default_text_split_spread() -> Property<f32> {
    Property::new("Spread / Overlap", 40.0)
}

fn default_text_split_locked() -> Property<bool> {
    Property::new("Lock Layout", true)
}

fn default_text_split_zero() -> Property<f32> {
    Property::new("Offset", 0.0)
}

fn default_warp_cols() -> Property<f32> {
    Property::new("Columns", 4.0)
}

fn default_warp_rows() -> Property<f32> {
    Property::new("Rows", 4.0)
}

fn default_text_split_pos_y() -> Property<f32> {
    Property::new("Position Y", -50.0)
}

fn default_text_split_rot() -> Property<f32> {
    Property::new("Rotation", -25.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileMode {
    /// Classic rows × columns mosaic repeat.
    #[default]
    Grid,
    /// Kaleidoscope ring around the center (segments × rings).
    Radial,
    /// Pointy-top hexagonal lattice.
    Hex,
    /// Triangular lattice.
    Triangle,
}

impl TileMode {
    /// All modes in Combobox order.
    pub const ALL: [Self; 4] = [Self::Grid, Self::Radial, Self::Hex, Self::Triangle];

    /// Human label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Grid => "Grid",
            Self::Radial => "Radial (Around)",
            Self::Hex => "Hexagon",
            Self::Triangle => "Triangle",
        }
    }

    /// Parse a [`Self::label`] back into a mode (Combobox commit path).
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.label() == label)
    }

    /// Compact numeric id for shaders / cache keys.
    pub const fn index(self) -> usize {
        match self {
            Self::Grid => 0,
            Self::Radial => 1,
            Self::Hex => 2,
            Self::Triangle => 3,
        }
    }
}

/// Tiler cell aperture shape (grid + radial lattices; hexagon mode always
/// uses hex cells). Non-square cells punch transparency outside the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileCell {
    /// Full rectangular cell (classic mosaic, fully opaque).
    #[default]
    Square,
    /// Rotated-square aperture.
    Diamond,
    /// Inscribed disc aperture.
    Circle,
    /// Inscribed triangle aperture.
    Triangle,
    /// Regular hexagon aperture.
    Hexagon,
}

impl TileCell {
    /// All cell shapes in Combobox order.
    pub const ALL: [Self; 5] = [
        Self::Square,
        Self::Diamond,
        Self::Circle,
        Self::Triangle,
        Self::Hexagon,
    ];

    /// Human label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::Diamond => "Diamond",
            Self::Circle => "Circle",
            Self::Triangle => "Triangle",
            Self::Hexagon => "Hexagon",
        }
    }

    /// Parse a [`Self::label`] back into a cell shape.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.label() == label)
    }

    /// Compact numeric id for shaders / cache keys.
    pub const fn index(self) -> usize {
        match self {
            Self::Square => 0,
            Self::Diamond => 1,
            Self::Circle => 2,
            Self::Triangle => 3,
            Self::Hexagon => 4,
        }
    }
}

/// Unit of subdivision for the 2D text split animator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSplitBy {
    /// Animate each character/glyph independently.
    #[default]
    Character,
    /// Animate word tokens independently.
    Word,
}

impl TextSplitBy {
    pub const ALL: [Self; 2] = [Self::Character, Self::Word];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Character => "By Character",
            Self::Word => "By Word",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.label() == label)
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Character => 0,
            Self::Word => 1,
        }
    }
}

/// Order of token activation across time in the 2D text split animator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSplitOrder {
    /// Left-to-right / start-to-end traversal.
    #[default]
    FromStart,
    /// Right-to-left / end-to-start traversal.
    FromEnd,
    /// Seeded pseudorandom shuffle.
    Random,
}

impl TextSplitOrder {
    pub const ALL: [Self; 3] = [Self::FromStart, Self::FromEnd, Self::Random];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FromStart => "From Start",
            Self::FromEnd => "From End",
            Self::Random => "Random",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.label() == label)
    }

    pub const fn index(self) -> usize {
        match self {
            Self::FromStart => 0,
            Self::FromEnd => 1,
            Self::Random => 2,
        }
    }
}

/// Interpolation easing curve applied to each token's transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSplitEasing {
    #[default]
    EaseInOut,
    Linear,
    EaseIn,
    EaseOut,
}

impl TextSplitEasing {
    pub const ALL: [Self; 4] = [Self::EaseInOut, Self::Linear, Self::EaseIn, Self::EaseOut];

    pub const fn label(self) -> &'static str {
        match self {
            Self::EaseInOut => "Ease In / Out",
            Self::Linear => "Linear",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.label() == label)
    }

    pub const fn index(self) -> usize {
        match self {
            Self::EaseInOut => 0,
            Self::Linear => 1,
            Self::EaseIn => 2,
            Self::EaseOut => 3,
        }
    }

    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => t * (2.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
        }
    }
}

/// The specific algorithm and animatable parameters for an image processing effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EffectType {
    GaussianBlur {
        radius: Property<f32>,
    },
    BrightnessContrast {
        brightness: Property<f32>,
        contrast: Property<f32>,
    },
    Tint {
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        map_black: Property<Color>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        map_white: Property<Color>,
        amount: Property<f32>,
    },
    Invert {
        amount: Property<f32>,
    },
    DropShadow {
        distance: Property<f32>,
        angle: Property<f32>,
        softness: Property<f32>,
        opacity: Property<f32>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        color: Property<Color>,
    },
    OuterGlow {
        size: Property<f32>,
        spread: Property<f32>,
        opacity: Property<f32>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        color: Property<Color>,
        range: Property<f32>,
    },
    GlslShader {
        code: String,
        param1: Property<f32>,
        param2: Property<f32>,
        param3: Property<f32>,
        param4: Property<f32>,
    },
    /// Runtime user-shader effect (Shader Lab): GLSL-style source with
    /// auto-detected `uniform` parameters. `source` is always the last
    /// successfully compiled text; a failed Apply keeps it while reporting
    /// the error in `compile_error`. `values` holds user overrides;
    /// anything missing falls back to the parsed default.
    ShaderLab {
        source: String,
        params: Vec<ShaderParam>,
        values: HashMap<String, ShaderParamValue>,
        compile_error: Option<String>,
    },
    DisplacementMap {
        max_horizontal: Property<f32>,
        max_vertical: Property<f32>,
    },
    ChromaKey {
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        key_color: Property<Color>,
        tolerance: Property<f32>,
        feather: Property<f32>,
    },
    LumaKey {
        threshold: Property<f32>,
        feather: Property<f32>,
    },
    SwapColor {
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        from_color: Property<Color>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        to_color: Property<Color>,
        tolerance: Property<f32>,
        feather: Property<f32>,
    },
    NoiseGenerator {
        amount: Property<f32>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        monochrome: Property<bool>,
    },
    /// Procedural checkerboard generator (spatial: needs pixel position,
    /// so [`crate::Effect::nudge_param`] handles scalars while rasterizers
    /// and the viewport SVG preview resolve the pattern).
    Checkerboard {
        size: Property<f32>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        color_a: Property<Color>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        color_b: Property<Color>,
    },
    /// Two-color linear gradient generator (spatial).
    GradientRamp {
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        color_a: Property<Color>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        color_b: Property<Color>,
        angle: Property<f32>,
        /// Extra stops beyond the endpoints (empty = pure two-color ramp).
        /// When non-empty these win for rendering; `color_a`/`color_b`
        /// mirror the sorted endpoints for export/compat.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        stops: Vec<GradientStop>,
        #[serde(default)]
        gradient_type: GradientType,
    },
    /// Fake-3D skew filter in degrees (spatial).
    Perspective {
        skew_x: Property<f32>,
        skew_y: Property<f32>,
    },
    /// Text stroke outline (resolved by text renderers / viewport SVG).
    TextOutline {
        width: Property<f32>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        color: Property<Color>,
    },
    /// Text bevel lighting (resolved by text renderers).
    TextBevel {
        strength: Property<f32>,
        softness: Property<f32>,
    },
    /// Text split animator (2D): animates text glyphs/words individually
    /// with customizable split unit, order, timing overlap, 2D transform
    /// offsets and alignment anchors.
    TextSplitAnimator {
        #[serde(default)]
        split_by: TextSplitBy,
        #[serde(default)]
        order: TextSplitOrder,
        #[serde(default = "default_text_split_seed")]
        random_seed: Property<f32>,
        #[serde(default = "default_text_split_progress")]
        progress: Property<f32>,
        #[serde(default = "default_text_split_spread")]
        spread: Property<f32>,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default = "default_text_split_locked")]
        lock_layout: Property<bool>,
        #[serde(default)]
        easing: TextSplitEasing,
        #[serde(default = "default_text_split_zero")]
        position_x: Property<f32>,
        #[serde(default = "default_text_split_pos_y")]
        position_y: Property<f32>,
        #[serde(default = "default_text_split_rot")]
        rotation: Property<f32>,
        #[serde(default = "default_text_split_zero")]
        opacity: Property<f32>,
        #[serde(default = "default_text_split_zero")]
        anchor_x: Property<f32>,
        #[serde(default = "default_text_split_zero")]
        anchor_y: Property<f32>,
    },
    /// Highlight bloom lift (per-pixel approximation; radius is spatial).
    Bloom {
        intensity: Property<f32>,
        radius: Property<f32>,
    },
    /// Advanced mosaic tiler (spatial): grid / radial / hexagonal
    /// lattices, shaped cell apertures, mirroring, phase offset and
    /// seeded per-tile randomization. New fields all carry serde
    /// defaults so old project files (tiles only) keep loading.
    Tiler {
        tiles_x: Property<f32>,
        tiles_y: Property<f32>,
        #[serde(default)]
        mode: TileMode,
        #[serde(deserialize_with = "crate::property::de_property_or_value", default)]
        mirror: Property<bool>,
        #[serde(default = "default_tile_offset")]
        offset_x: Property<f32>,
        #[serde(default = "default_tile_offset")]
        offset_y: Property<f32>,
        #[serde(default)]
        cell: TileCell,
        #[serde(default = "default_tile_seed")]
        seed: Property<f32>,
        #[serde(default = "default_tile_amount")]
        amount: Property<f32>,
    },

    /// Warp distortion (spatial): sine wobble plus a draggable pin
    /// lattice (`pins` row-major over `cols` x `rows`, empty = identity,
    /// offsets in layer px).
    Warp {
        amount: Property<f32>,
        scale: Property<f32>,
        #[serde(default = "default_warp_cols")]
        cols: Property<f32>,
        #[serde(default = "default_warp_rows")]
        rows: Property<f32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pins: Vec<WarpPin>,
    },
    /// Exposure in EV stops (per-pixel gain).
    Exposure {
        exposure: Property<f32>,
    },
    /// Vibrance: saturation weighted toward muted colors (per-pixel).
    Vibrance {
        vibrance: Property<f32>,
    },
    /// Levels: input range remap + gamma + output range (per-pixel).
    /// OFX plug-in `net.sf.openfx.levels`.
    Levels {
        input_black: Property<f32>,
        input_white: Property<f32>,
        gamma: Property<f32>,
        output_black: Property<f32>,
        output_white: Property<f32>,
    },
    /// Hue / Saturation / Lightness grade (per-pixel).
    /// OFX plug-in `net.sf.openfx.hue_saturation`.
    HueSaturation {
        hue_shift: Property<f32>,
        saturation: Property<f32>,
        lightness: Property<f32>,
    },
    /// Unsharp-mask sharpen (spatial: needs neighbours).
    /// OFX plug-in `net.sf.openfx.sharpen`.
    Sharpen {
        amount: Property<f32>,
        radius: Property<f32>,
    },
    /// Edge vignette darkening (spatial: needs pixel position).
    /// OFX plug-in `net.sf.openfx.vignette`.
    Vignette {
        amount: Property<f32>,
        softness: Property<f32>,
    },
    /// Modular stock plug-in (see `crate::stock::StockPlugin`): scalar
    /// params are built from the plug-in descriptor, so every stock effect
    /// is keyframable with zero per-effect plumbing. `colors` holds the
    /// animatable color slots in `stock_color_slots` order.
    Stock {
        plugin: StockPlugin,
        #[serde(default)]
        params: Vec<Property<f32>>,
        #[serde(default, deserialize_with = "crate::property::de_vec_property_or_value")]
        colors: Vec<Property<Color>>,
    },
}

impl EffectType {
    /// Return the canonical display name of the effect type.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::BrightnessContrast { .. } => "Brightness & Contrast",
            Self::Tint { .. } => "Tint",
            Self::Invert { .. } => "Invert",
            Self::DropShadow { .. } => "Drop Shadow",
            Self::OuterGlow { .. } => "Outer Glow",
            Self::GlslShader { .. } => "Custom GLSL Shader",
            Self::ShaderLab { .. } => "Shader Lab",
            Self::DisplacementMap { .. } => "Displacement Map",
            Self::ChromaKey { .. } => "Chroma Key",
            Self::LumaKey { .. } => "Luma Key",
            Self::SwapColor { .. } => "Swap Color",
            Self::NoiseGenerator { .. } => "Noise Generator",
            Self::Checkerboard { .. } => "Checkerboard",
            Self::GradientRamp { .. } => "Gradient Ramp",
            Self::Perspective { .. } => "Perspective",
            Self::TextOutline { .. } => "Text Outline",
            Self::TextBevel { .. } => "Text Bevel",
            Self::TextSplitAnimator { .. } => "Text Split Animator (2D)",
            Self::Bloom { .. } => "Bloom",
            Self::Tiler { .. } => "Tiler",
            Self::Warp { .. } => "Warp",
            Self::Exposure { .. } => "Exposure",
            Self::Vibrance { .. } => "Vibrance",
            Self::Levels { .. } => "Levels",
            Self::HueSaturation { .. } => "Hue / Saturation",
            Self::Sharpen { .. } => "Sharpen",
            Self::Vignette { .. } => "Vignette",
            Self::Stock { plugin, .. } => plugin.descriptor().label,
        }
    }

    /// Construct a Gaussian Blur effect type.
    pub fn gaussian_blur(radius: f32) -> Self {
        Self::GaussianBlur {
            radius: Property::new("Blur Radius", radius.max(0.0)),
        }
    }

    /// Construct a Brightness & Contrast effect type.
    pub fn brightness_contrast(brightness: f32, contrast: f32) -> Self {
        Self::BrightnessContrast {
            brightness: Property::new("Brightness", brightness.clamp(-100.0, 100.0)),
            contrast: Property::new("Contrast", contrast.clamp(-100.0, 100.0)),
        }
    }

    /// Construct a Tint effect type.
    pub fn tint(map_black: Color, map_white: Color, amount: f32) -> Self {
        Self::Tint {
            map_black: Property::new("Map Black", map_black),
            map_white: Property::new("Map White", map_white),
            amount: Property::new("Amount to Tint", amount.clamp(0.0, 100.0)),
        }
    }

    /// Construct an Invert effect type.
    pub fn invert(amount: f32) -> Self {
        Self::Invert {
            amount: Property::new("Invert Amount", amount.clamp(0.0, 100.0)),
        }
    }

    /// Construct a Drop Shadow effect type.
    pub fn drop_shadow(
        distance: f32,
        angle: f32,
        softness: f32,
        opacity: f32,
        color: Color,
    ) -> Self {
        Self::DropShadow {
            distance: Property::new("Distance", distance.max(0.0)),
            angle: Property::new("Angle", angle),
            softness: Property::new("Softness", softness.max(0.0)),
            opacity: Property::new("Opacity", opacity.clamp(0.0, 100.0)),
            color: Property::new("Shadow Color", color),
        }
    }

    /// Construct an Outer Glow effect type.
    pub fn outer_glow(
        size: f32,
        spread: f32,
        opacity: f32,
        color: Color,
        range: f32,
    ) -> Self {
        Self::OuterGlow {
            size: Property::new("Size", size.max(0.0)),
            spread: Property::new("Spread", spread.clamp(0.0, 100.0)),
            opacity: Property::new("Opacity", opacity.clamp(0.0, 100.0)),
            color: Property::new("Glow Color", color),
            range: Property::new("Range", range.clamp(0.0, 100.0)),
        }
    }

    /// Construct a custom GLSL / WGSL shader effect type.
    pub fn glsl_shader(code: impl Into<String>, p1: f32, p2: f32, p3: f32, p4: f32) -> Self {
        Self::GlslShader {
            code: code.into(),
            param1: Property::new("Param 1 (Speed/Time)", p1),
            param2: Property::new("Param 2 (Intensity)", p2),
            param3: Property::new("Param 3 (Scale/Freq)", p3),
            param4: Property::new("Param 4 (Tint/Phase)", p4),
        }
    }

    /// Construct a Displacement Map effect type.
    pub fn displacement(max_horizontal: f32, max_vertical: f32) -> Self {
        Self::DisplacementMap {
            max_horizontal: Property::new("Max Horizontal", max_horizontal.clamp(-500.0, 500.0)),
            max_vertical: Property::new("Max Vertical", max_vertical.clamp(-500.0, 500.0)),
        }
    }

    /// Construct a Chroma Key effect type.
    pub fn chroma_key(key_color: Color, tolerance: f32, feather: f32) -> Self {
        Self::ChromaKey {
            key_color: Property::new("Key Color", key_color),
            tolerance: Property::new("Tolerance", tolerance.clamp(0.0, 100.0)),
            feather: Property::new("Feather", feather.clamp(0.0, 100.0)),
        }
    }

    /// Construct a Luma Key effect type (keys out dark / bright pixels by
    /// luminance instead of hue).
    pub fn luma_key(threshold: f32, feather: f32) -> Self {
        Self::LumaKey {
            threshold: Property::new("Threshold", threshold.clamp(0.0, 100.0)),
            feather: Property::new("Feather", feather.clamp(0.0, 100.0)),
        }
    }

    /// Construct a Swap Color effect type (recolors pixels near
    /// `from_color` to `to_color` with tolerance + feather falloff).
    pub fn swap_color(from_color: Color, to_color: Color, tolerance: f32, feather: f32) -> Self {
        Self::SwapColor {
            from_color: Property::new("From Color", from_color),
            to_color: Property::new("To Color", to_color),
            tolerance: Property::new("Tolerance", tolerance.clamp(0.0, 100.0)),
            feather: Property::new("Feather", feather.clamp(0.0, 100.0)),
        }
    }

    /// Construct a Shader Lab runtime-shader effect type.
    pub fn shader_lab(source: impl Into<String>) -> Self {
        let source = source.into();
        let params = parse_shader_params(&source);
        Self::ShaderLab {
            source,
            params,
            values: HashMap::new(),
            compile_error: None,
        }
    }

    /// Construct a Noise Generator effect type.
    pub fn noise_generator(amount: f32, monochrome: bool) -> Self {
        Self::NoiseGenerator {
            amount: Property::new("Amount", amount.clamp(0.0, 100.0)),
            monochrome: Property::new("Monochrome", monochrome),
        }
    }

    /// Construct a Checkerboard generator effect type.
    pub fn checkerboard(size: f32, color_a: Color, color_b: Color) -> Self {
        Self::Checkerboard {
            size: Property::new("Size", size.clamp(2.0, 512.0)),
            color_a: Property::new("Color A", color_a),
            color_b: Property::new("Color B", color_b),
        }
    }

    /// Construct a Gradient Ramp generator effect type.
    pub fn gradient_ramp(color_a: Color, color_b: Color, angle: f32) -> Self {
        Self::GradientRamp {
            color_a: Property::new("Color A", color_a),
            color_b: Property::new("Color B", color_b),
            angle: Property::new("Angle", angle),
            stops: Vec::new(),
            gradient_type: GradientType::Linear,
        }
    }

    /// Construct a Gradient Ramp generator effect with explicit gradient type.
    pub fn gradient_ramp_with_type(color_a: Color, color_b: Color, angle: f32, gradient_type: GradientType) -> Self {
        Self::GradientRamp {
            color_a: Property::new("Color A", color_a),
            color_b: Property::new("Color B", color_b),
            angle: Property::new("Angle", angle),
            stops: Vec::new(),
            gradient_type,
        }
    }

    /// Gradient ramp type if this is a Gradient Ramp effect.
    pub fn gradient_ramp_type(&self) -> Option<GradientType> {
        match self {
            Self::GradientRamp { gradient_type, .. } => Some(*gradient_type),
            _ => None,
        }
    }

    /// Set the gradient ramp type.
    pub fn set_gradient_ramp_type(&mut self, g_type: GradientType) -> bool {
        match self {
            Self::GradientRamp { gradient_type, .. } => {
                *gradient_type = g_type;
                true
            }
            _ => false,
        }
    }

    /// Effective gradient stops for a ramp: explicit `stops` when two or
    /// more are stored, otherwise the classic endpoint pair.
    pub fn gradient_ramp_stops(&self) -> Option<Vec<GradientStop>> {
        match self {
            Self::GradientRamp { color_a, color_b, stops, .. } => {
                if stops.len() >= 2 {
                    let mut sorted = stops.clone();
                    sorted.sort_by(|a, b| {
                        a.offset
                            .partial_cmp(&b.offset)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    Some(sorted)
                } else {
                    Some(vec![
                        GradientStop::new(0.0, color_a.value),
                        GradientStop::new(1.0, color_b.value),
                    ])
                }
            }
            _ => None,
        }
    }

    /// Replace a ramp's explicit stops (fewer than two clears back to the
    /// endpoint pair) and mirror the sorted endpoints into
    /// `color_a`/`color_b` for export/compat readers.
    pub fn set_gradient_ramp_stops(&mut self, stops: Vec<GradientStop>) -> bool {
        match self {
            Self::GradientRamp { color_a, color_b, stops: slot, .. } => {
                if stops.len() >= 2 {
                    let grad = FillGradient { stops, angle: 0.0, gradient_type: GradientType::Linear };
                    let sorted = grad.sorted_stops();
                    color_a.set_value(sorted[0].color);
                    color_b.set_value(sorted[sorted.len() - 1].color);
                    *slot = sorted.into_iter().cloned().collect();
                } else {
                    slot.clear();
                }
                true
            }
            _ => false,
        }
    }

    /// Mirror a legacy endpoint write into the matching sorted end stop.
    fn sync_ramp_endpoint(stops: &mut [GradientStop], first: bool, color: Color) {
        if stops.len() < 2 {
            return;
        }
        let at = if first {
            stops
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.offset
                        .partial_cmp(&b.offset)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
        } else {
            stops
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| {
                    a.offset
                        .partial_cmp(&b.offset)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
        };
        if let Some(i) = at {
            stops[i].color = color;
        }
    }

    /// Construct a Perspective skew effect type (degrees).
    pub fn perspective(skew_x: f32, skew_y: f32) -> Self {
        Self::Perspective {
            skew_x: Property::new("Skew X", skew_x.clamp(-60.0, 60.0)),
            skew_y: Property::new("Skew Y", skew_y.clamp(-60.0, 60.0)),
        }
    }

    /// Construct a Text Outline effect type.
    pub fn text_outline(width: f32, color: Color) -> Self {
        Self::TextOutline {
            width: Property::new("Width", width.clamp(0.0, 50.0)),
            color: Property::new("Color", color),
        }
    }

    /// Construct a Text Bevel effect type.
    pub fn text_bevel(strength: f32, softness: f32) -> Self {
        Self::TextBevel {
            strength: Property::new("Strength", strength.clamp(0.0, 100.0)),
            softness: Property::new("Softness", softness.clamp(0.0, 100.0)),
        }
    }

    /// Construct a 2D Text Split Animator effect type.
    pub fn text_split_animator() -> Self {
        Self::TextSplitAnimator {
            split_by: TextSplitBy::Character,
            order: TextSplitOrder::FromStart,
            random_seed: default_text_split_seed(),
            progress: default_text_split_progress(),
            spread: default_text_split_spread(),
            lock_layout: default_text_split_locked(),
            easing: TextSplitEasing::EaseInOut,
            position_x: default_text_split_zero(),
            position_y: default_text_split_pos_y(),
            rotation: default_text_split_rot(),
            opacity: default_text_split_zero(),
            anchor_x: default_text_split_zero(),
            anchor_y: default_text_split_zero(),
        }
    }

    /// Construct a Bloom effect type.
    pub fn bloom(intensity: f32, radius: f32) -> Self {
        Self::Bloom {
            intensity: Property::new("Intensity", intensity.clamp(0.0, 100.0)),
            radius: Property::new("Radius", radius.clamp(0.0, 100.0)),
        }
    }

    /// Construct a Tiler effect type.
    pub fn tiler(tiles_x: f32, tiles_y: f32) -> Self {
        Self::Tiler {
            tiles_x: Property::new("Tiles X", tiles_x.clamp(1.0, 32.0)),
            tiles_y: Property::new("Tiles Y", tiles_y.clamp(1.0, 32.0)),
            mode: TileMode::Grid,
            mirror: Property::new("Mirror", false),
            offset_x: default_tile_offset(),
            offset_y: default_tile_offset(),
            cell: TileCell::Square,
            seed: default_tile_seed(),
            amount: default_tile_amount(),
        }
    }

    /// Construct a Warp effect type.
    pub fn warp(amount: f32, scale: f32) -> Self {
        Self::Warp {
            amount: Property::new("Amount", amount.clamp(0.0, 100.0)),
            scale: Property::new("Scale", scale.clamp(0.1, 10.0)),
            cols: default_warp_cols(),
            rows: default_warp_rows(),
            pins: Vec::new(),
        }
    }

    /// Construct an Exposure effect type (EV stops).
    pub fn exposure(exposure: f32) -> Self {
        Self::Exposure {
            exposure: Property::new("Exposure", exposure.clamp(-10.0, 10.0)),
        }
    }

    /// Construct a Vibrance effect type.
    pub fn vibrance(vibrance: f32) -> Self {
        Self::Vibrance {
            vibrance: Property::new("Vibrance", vibrance.clamp(-100.0, 100.0)),
        }
    }

    /// Construct a Levels effect type (0-255 ranges, gamma).
    pub fn levels(
        input_black: f32,
        input_white: f32,
        gamma: f32,
        output_black: f32,
        output_white: f32,
    ) -> Self {
        Self::Levels {
            input_black: Property::new("Input Black", input_black.clamp(0.0, 255.0)),
            input_white: Property::new("Input White", input_white.clamp(0.0, 255.0)),
            gamma: Property::new("Gamma", gamma.clamp(0.1, 9.9)),
            output_black: Property::new("Output Black", output_black.clamp(0.0, 255.0)),
            output_white: Property::new("Output White", output_white.clamp(0.0, 255.0)),
        }
    }

    /// Construct a Hue / Saturation effect type.
    pub fn hue_saturation(hue_shift: f32, saturation: f32, lightness: f32) -> Self {
        Self::HueSaturation {
            hue_shift: Property::new("Hue Shift", hue_shift.clamp(-180.0, 180.0)),
            saturation: Property::new("Saturation", saturation.clamp(-100.0, 100.0)),
            lightness: Property::new("Lightness", lightness.clamp(-100.0, 100.0)),
        }
    }

    /// Construct a Sharpen (unsharp mask) effect type.
    pub fn sharpen(amount: f32, radius: f32) -> Self {
        Self::Sharpen {
            amount: Property::new("Amount", amount.clamp(0.0, 200.0)),
            radius: Property::new("Radius", radius.clamp(0.0, 20.0)),
        }
    }

    /// Construct a Vignette effect type.
    pub fn vignette(amount: f32, softness: f32) -> Self {
        Self::Vignette {
            amount: Property::new("Amount", amount.clamp(0.0, 100.0)),
            softness: Property::new("Softness", softness.clamp(0.0, 100.0)),
        }
    }

    /// Stable OpenFX-style plug-in id for this effect type
    /// (`net.sf.openfx.*`, see [`crate::ofx::OFX_SUITE`]).
    pub const fn ofx_plugin_id(&self) -> &'static str {
        match self {
            Self::GaussianBlur { .. } => "net.sf.openfx.blur",
            Self::BrightnessContrast { .. } => "net.sf.openfx.brightness_contrast",
            Self::Tint { .. } => "net.sf.openfx.tint",
            Self::Invert { .. } => "net.sf.openfx.invert",
            Self::DropShadow { .. } => "net.sf.openfx.drop_shadow",
            Self::OuterGlow { .. } => "net.sf.openfx.outer_glow",
            Self::GlslShader { .. } => "net.sf.openfx.custom.glsl",
            Self::ShaderLab { .. } => "net.sf.openfx.custom.shader_lab",
            Self::DisplacementMap { .. } => "net.sf.openfx.displacement",
            Self::ChromaKey { .. } => "net.sf.openfx.chroma_key",
            Self::LumaKey { .. } => "net.sf.openfx.luma_key",
            Self::SwapColor { .. } => "net.sf.openfx.swap_color",
            Self::NoiseGenerator { .. } => "net.sf.openfx.noise",
            Self::Checkerboard { .. } => "net.sf.openfx.checkerboard",
            Self::GradientRamp { .. } => "net.sf.openfx.gradient_ramp",
            Self::Perspective { .. } => "net.sf.openfx.perspective",
            Self::TextOutline { .. } => "net.sf.openfx.text_outline",
            Self::TextBevel { .. } => "net.sf.openfx.text_bevel",
            Self::TextSplitAnimator { .. } => "net.sf.openfx.text_split_animator",
            Self::Bloom { .. } => "net.sf.openfx.bloom",
            Self::Tiler { .. } => "net.sf.openfx.tiler",
            Self::Warp { .. } => "net.sf.openfx.warp",
            Self::Exposure { .. } => "net.sf.openfx.exposure",
            Self::Vibrance { .. } => "net.sf.openfx.vibrance",
            Self::Levels { .. } => "net.sf.openfx.levels",
            Self::HueSaturation { .. } => "net.sf.openfx.hue_saturation",
            Self::Sharpen { .. } => "net.sf.openfx.sharpen",
            Self::Vignette { .. } => "net.sf.openfx.vignette",
            Self::Stock { plugin, .. } => plugin.plugin_id(),
        }
    }

    /// Build stock scalar params from the plug-in descriptor
    /// (defaults become both value and default value).
    pub fn stock_params(plugin: StockPlugin) -> Vec<Property<f32>> {
        crate::stock::stock_default_params(plugin)
    }

    /// Build stock color slots from the plug-in descriptor.
    pub fn stock_colors(plugin: StockPlugin) -> Vec<Property<Color>> {
        crate::stock::stock_color_slots(plugin)
            .iter()
            .map(|slot| Property::new(*slot, stock_default_color(plugin, slot)))
            .collect()
    }
}

/// A layer effect applied sequentially in the layer's post-processing stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub effect_type: EffectType,
}

impl Effect {
    /// Create a new generic layer effect.
    pub fn new(id: impl Into<String>, name: impl Into<String>, effect_type: EffectType) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled: true,
            effect_type,
        }
    }

    /// Factory for creating a Gaussian Blur effect with an initial blur radius.
    pub fn gaussian_blur(id: impl Into<String>, radius: f32) -> Self {
        Self::new(id, "Gaussian Blur", EffectType::gaussian_blur(radius))
    }

    /// Factory for creating a Brightness & Contrast adjustment effect.
    pub fn brightness_contrast(id: impl Into<String>, brightness: f32, contrast: f32) -> Self {
        Self::new(
            id,
            "Brightness & Contrast",
            EffectType::brightness_contrast(brightness, contrast),
        )
    }

    /// Factory for creating a two-color Tint mapping effect.
    pub fn tint(id: impl Into<String>, map_black: Color, map_white: Color, amount: f32) -> Self {
        Self::new(id, "Tint", EffectType::tint(map_black, map_white, amount))
    }

    /// Factory for creating an Invert color effect.
    pub fn invert(id: impl Into<String>, amount: f32) -> Self {
        Self::new(id, "Invert", EffectType::invert(amount))
    }

    /// Factory for creating a Drop Shadow effect.
    pub fn drop_shadow(
        id: impl Into<String>,
        distance: f32,
        angle: f32,
        softness: f32,
        opacity: f32,
        color: Color,
    ) -> Self {
        Self::new(
            id,
            "Drop Shadow",
            EffectType::drop_shadow(distance, angle, softness, opacity, color),
        )
    }

    /// Factory for creating a custom GLSL / WGSL shader effect with default template.
    pub fn glsl_shader(id: impl Into<String>, code: impl Into<String>) -> Self {
        Self::new(
            id,
            "Custom GLSL Shader",
            EffectType::glsl_shader(code, 1.0, 50.0, 1.0, 100.0),
        )
    }

    /// Factory for creating a Displacement Map effect.
    pub fn displacement(id: impl Into<String>, max_horizontal: f32, max_vertical: f32) -> Self {
        Self::new(id, "Displacement Map", EffectType::displacement(max_horizontal, max_vertical))
    }

    /// Factory for creating a Chroma Key effect.
    pub fn chroma_key(id: impl Into<String>, key_color: Color, tolerance: f32, feather: f32) -> Self {
        Self::new(id, "Chroma Key", EffectType::chroma_key(key_color, tolerance, feather))
    }

    /// Factory for creating a Luma Key effect.
    pub fn luma_key(id: impl Into<String>, threshold: f32, feather: f32) -> Self {
        Self::new(id, "Luma Key", EffectType::luma_key(threshold, feather))
    }

    /// Factory for creating a Swap Color effect.
    pub fn swap_color(
        id: impl Into<String>,
        from_color: Color,
        to_color: Color,
        tolerance: f32,
        feather: f32,
    ) -> Self {
        Self::new(
            id,
            "Swap Color",
            EffectType::swap_color(from_color, to_color, tolerance, feather),
        )
    }

    /// Factory for creating a Noise Generator effect.
    pub fn noise_generator(id: impl Into<String>, amount: f32, monochrome: bool) -> Self {
        Self::new(id, "Noise Generator", EffectType::noise_generator(amount, monochrome))
    }

    /// Factory for creating a Checkerboard effect.
    pub fn checkerboard(id: impl Into<String>, size: f32, color_a: Color, color_b: Color) -> Self {
        Self::new(id, "Checkerboard", EffectType::checkerboard(size, color_a, color_b))
    }

    /// Factory for creating a Gradient Ramp effect.
    pub fn gradient_ramp(id: impl Into<String>, color_a: Color, color_b: Color, angle: f32) -> Self {
        Self::new(id, "Gradient Ramp", EffectType::gradient_ramp(color_a, color_b, angle))
    }

    /// Factory for creating a Perspective effect.
    pub fn perspective(id: impl Into<String>, skew_x: f32, skew_y: f32) -> Self {
        Self::new(id, "Perspective", EffectType::perspective(skew_x, skew_y))
    }

    /// Factory for creating a Text Outline effect.
    pub fn text_outline(id: impl Into<String>, width: f32, color: Color) -> Self {
        Self::new(id, "Text Outline", EffectType::text_outline(width, color))
    }

    /// Factory for creating a Text Bevel effect.
    pub fn text_bevel(id: impl Into<String>, strength: f32, softness: f32) -> Self {
        Self::new(id, "Text Bevel", EffectType::text_bevel(strength, softness))
    }

    /// Factory for creating a Bloom effect.
    pub fn bloom(id: impl Into<String>, intensity: f32, radius: f32) -> Self {
        Self::new(id, "Bloom", EffectType::bloom(intensity, radius))
    }

    /// Factory for creating a Tiler effect.
    pub fn tiler(id: impl Into<String>, tiles_x: f32, tiles_y: f32) -> Self {
        Self::new(id, "Tiler", EffectType::tiler(tiles_x, tiles_y))
    }

    /// Factory for creating a Warp effect.
    pub fn warp(id: impl Into<String>, amount: f32, scale: f32) -> Self {
        Self::new(id, "Warp", EffectType::warp(amount, scale))
    }

    /// Factory for creating an Exposure effect.
    pub fn exposure(id: impl Into<String>, exposure: f32) -> Self {
        Self::new(id, "Exposure", EffectType::exposure(exposure))
    }

    /// Factory for creating a Vibrance effect.
    pub fn vibrance(id: impl Into<String>, vibrance: f32) -> Self {
        Self::new(id, "Vibrance", EffectType::vibrance(vibrance))
    }

    /// Factory for creating a Levels effect.
    pub fn levels(
        id: impl Into<String>,
        input_black: f32,
        input_white: f32,
        gamma: f32,
        output_black: f32,
        output_white: f32,
    ) -> Self {
        Self::new(
            id,
            "Levels",
            EffectType::levels(input_black, input_white, gamma, output_black, output_white),
        )
    }

    /// Factory for creating a Hue / Saturation effect.
    pub fn hue_saturation(
        id: impl Into<String>,
        hue_shift: f32,
        saturation: f32,
        lightness: f32,
    ) -> Self {
        Self::new(
            id,
            "Hue / Saturation",
            EffectType::hue_saturation(hue_shift, saturation, lightness),
        )
    }

    /// Factory for creating a Sharpen effect.
    pub fn sharpen(id: impl Into<String>, amount: f32, radius: f32) -> Self {
        Self::new(id, "Sharpen", EffectType::sharpen(amount, radius))
    }

    /// Factory for creating a Vignette effect.
    pub fn vignette(id: impl Into<String>, amount: f32, softness: f32) -> Self {
        Self::new(id, "Vignette", EffectType::vignette(amount, softness))
    }

    /// Factory for creating a modular stock plug-in effect. Scalar params
    /// and color slots come from the plug-in descriptor; the display name
    /// is the plug-in label.
    pub fn stock(id: impl Into<String>, plugin: StockPlugin) -> Self {
        let label = plugin.descriptor().label;
        Self::new(
            id,
            label,
            EffectType::Stock {
                params: EffectType::stock_params(plugin),
                colors: EffectType::stock_colors(plugin),
                plugin,
            },
        )
    }

    /// Factory for creating a Shader Lab runtime-shader effect.
    pub fn shader_lab(id: impl Into<String>, source: impl Into<String>) -> Self {
        Self::new(id, "Shader Lab", EffectType::shader_lab(source))
    }

    /// Return the standard default GLSL fragment shader code template.
    pub const fn default_glsl_code() -> &'static str {
        r#"// Custom GLSL Fragment Shader
// Uniforms:
//   uniform float param1; // Speed / Time
//   uniform float param2; // Intensity / Color Boost (0-100)
//   uniform float param3; // Scale / Frequency
//   uniform float param4; // Opacity / Blend (0-100)
void mainImage(out vec4 fragColor, in vec2 uv, in vec4 inColor) {
    float intensity = param2 / 100.0;
    vec3 col = inColor.rgb * (1.0 + intensity * 0.5);
    fragColor = vec4(clamp(col, 0.0, 1.0), inColor.a * (param4 / 100.0));
}"#
    }

    /// Update the custom shader code string.
    pub fn set_glsl_code(&mut self, new_code: impl Into<String>) -> bool {
        if let EffectType::GlslShader { code, .. } = &mut self.effect_type {
            *code = new_code.into();
            true
        } else {
            false
        }
    }

    /// Return the custom shader code string if this is a GlslShader effect.
    pub fn glsl_code(&self) -> Option<&str> {
        if let EffectType::GlslShader { code, .. } = &self.effect_type {
            Some(code.as_str())
        } else {
            None
        }
    }

    /// Replace the Shader Lab source after a successful compile: re-parses
    /// parameters (adding/removing UI automatically) and prunes overrides
    /// for removed uniforms. Returns the fresh parameter list.
    pub fn set_shader_source(&mut self, new_source: impl Into<String>) -> Option<Vec<ShaderParam>> {
        if let EffectType::ShaderLab { source, params, values, compile_error } = &mut self.effect_type {
            *source = new_source.into();
            *params = parse_shader_params(source);
            values.retain(|k, _| params.iter().any(|p| &p.name == k));
            *compile_error = None;
            Some(params.clone())
        } else {
            None
        }
    }

    /// Record a failed Apply: the last-good `source` keeps running while the
    /// error is shown in the effect panel.
    pub fn set_shader_error(&mut self, error: Option<String>) -> bool {
        if let EffectType::ShaderLab { compile_error, .. } = &mut self.effect_type {
            *compile_error = error;
            true
        } else {
            false
        }
    }

    /// Override a Shader Lab parameter value. Returns false for unknown names
    /// or non-ShaderLab effects.
    pub fn set_shader_value(&mut self, name: &str, value: ShaderParamValue) -> bool {
        if let EffectType::ShaderLab { params, values, .. } = &mut self.effect_type {
            if params.iter().any(|p| p.name == name) {
                values.insert(name.to_string(), value);
                return true;
            }
        }
        false
    }

    /// Nudge a scalar Shader Lab parameter by `delta` (uses its step or 0.05).
    pub fn nudge_shader_value(&mut self, name: &str, delta: f32) -> bool {
        if let EffectType::ShaderLab { params, values, .. } = &mut self.effect_type {
            let param = match params.iter().find(|p| p.name == name) {
                Some(p) => p.clone(),
                None => return false,
            };
            let cur = values
                .get(name)
                .cloned()
                .unwrap_or_else(|| param.default.clone());
            let step = param.step.unwrap_or(0.05);
            let next = match cur {
                ShaderParamValue::Float(v) => param.coerce_float(v + delta * step),
                ShaderParamValue::Int(v) => param.coerce_float(v as f32 + delta * step.max(1.0)),
                ShaderParamValue::Bool(v) => {
                    if delta.abs() > 0.0 {
                        ShaderParamValue::Bool(!v)
                    } else {
                        ShaderParamValue::Bool(v)
                    }
                }
                ShaderParamValue::Vec2(v) => {
                    ShaderParamValue::Vec2([param.coerce_float(v[0] + delta * step).as_floats()[0]; 2])
                }
                ShaderParamValue::Vec3(v) => {
                    let c = param.coerce_float(v[0] + delta * step).as_floats()[0];
                    ShaderParamValue::Vec3([c, c, c])
                }
                ShaderParamValue::Vec4(v) => {
                    let c = param.coerce_float(v[0] + delta * step).as_floats()[0];
                    ShaderParamValue::Vec4([c, c, c, v[3]])
                }
                ShaderParamValue::Color(c) => {
                    let d = delta * step;
                    ShaderParamValue::Color(Color::rgba(
                        (c.r + d).clamp(0.0, 1.0),
                        (c.g + d).clamp(0.0, 1.0),
                        (c.b + d).clamp(0.0, 1.0),
                        c.a,
                    ))
                }
            };
            values.insert(name.to_string(), next);
            return true;
        }
        false
    }

    /// Set a named color field (`color_a` / `color_b` / `color`) on effects
    /// that carry swatch colors (Checkerboard, Gradient Ramp, Text Outline).
    /// Returns false for unknown fields or effects without colors.
    pub fn set_color_value(&mut self, field: &str, next: Color) -> bool {        match &mut self.effect_type {
            EffectType::Tint { map_black, map_white, .. } => {
                if field.eq_ignore_ascii_case("map_black") || field.eq_ignore_ascii_case("black") {
                    map_black.set_value(next);
                    true
                } else if field.eq_ignore_ascii_case("map_white") || field.eq_ignore_ascii_case("white") {
                    map_white.set_value(next);
                    true
                } else {
                    false
                }
            }
            EffectType::DropShadow { color, .. } => {
                if field.eq_ignore_ascii_case("color") {
                    color.set_value(next);
                    true
                } else {
                    false
                }
            }
            EffectType::OuterGlow { color, .. } => {
                if field.eq_ignore_ascii_case("color") {
                    color.set_value(next);
                    true
                } else {
                    false
                }
            }
            EffectType::ChromaKey { key_color, .. } => {
                if field.eq_ignore_ascii_case("key_color") || field.eq_ignore_ascii_case("color") {
                    key_color.set_value(next);
                    true
                } else {
                    false
                }
            }
            EffectType::SwapColor { from_color, to_color, .. } => {
                if field.eq_ignore_ascii_case("from_color") || field.eq_ignore_ascii_case("from") {
                    from_color.set_value(next);
                    true
                } else if field.eq_ignore_ascii_case("to_color") || field.eq_ignore_ascii_case("to") {
                    to_color.set_value(next);
                    true
                } else {
                    false
                }
            }            EffectType::Checkerboard { color_a, color_b, .. } => {
                if field.eq_ignore_ascii_case("color_a") || field.eq_ignore_ascii_case("a") {
                    color_a.set_value(next);
                    true
                } else if field.eq_ignore_ascii_case("color_b") || field.eq_ignore_ascii_case("b") {
                    color_b.set_value(next);
                    true
                } else {
                    false
                }
            }
            EffectType::GradientRamp { color_a, color_b, stops, .. } => {
                if field.eq_ignore_ascii_case("color_a") || field.eq_ignore_ascii_case("a") {
                    color_a.set_value(next);
                    EffectType::sync_ramp_endpoint(stops, true, next);
                    true
                } else if field.eq_ignore_ascii_case("color_b") || field.eq_ignore_ascii_case("b") {
                    color_b.set_value(next);
                    EffectType::sync_ramp_endpoint(stops, false, next);
                    true
                } else {
                    false
                }
            }
            EffectType::TextOutline { color, .. }
                if field.eq_ignore_ascii_case("color") => {
                    color.set_value(next);
                    true
                }
            _ => false,
        }
    }

    /// Mutable access to a named color property (same fields as
    /// [`Self::set_color_value`]) for animation toggles and playhead commits.
    pub fn get_color_property_mut(&mut self, field: &str) -> Option<&mut Property<Color>> {
        match &mut self.effect_type {
            EffectType::Tint { map_black, map_white, .. } => {
                if field.eq_ignore_ascii_case("map_black") || field.eq_ignore_ascii_case("black") {
                    Some(map_black)
                } else if field.eq_ignore_ascii_case("map_white") || field.eq_ignore_ascii_case("white") {
                    Some(map_white)
                } else {
                    None
                }
            }
            EffectType::DropShadow { color, .. } => {
                if field.eq_ignore_ascii_case("color") { Some(color) } else { None }
            }
            EffectType::OuterGlow { color, .. } => {
                if field.eq_ignore_ascii_case("color") { Some(color) } else { None }
            }
            EffectType::ChromaKey { key_color, .. } => {
                if field.eq_ignore_ascii_case("key_color") || field.eq_ignore_ascii_case("color") {
                    Some(key_color)
                } else {
                    None
                }
            }
            EffectType::SwapColor { from_color, to_color, .. } => {
                if field.eq_ignore_ascii_case("from_color") || field.eq_ignore_ascii_case("from") {
                    Some(from_color)
                } else if field.eq_ignore_ascii_case("to_color") || field.eq_ignore_ascii_case("to") {
                    Some(to_color)
                } else {
                    None
                }
            }
            EffectType::Checkerboard { color_a, color_b, .. } => {
                if field.eq_ignore_ascii_case("color_a") || field.eq_ignore_ascii_case("a") {
                    Some(color_a)
                } else if field.eq_ignore_ascii_case("color_b") || field.eq_ignore_ascii_case("b") {
                    Some(color_b)
                } else {
                    None
                }
            }
            EffectType::GradientRamp { color_a, color_b, .. } => {
                if field.eq_ignore_ascii_case("color_a") || field.eq_ignore_ascii_case("a") {
                    Some(color_a)
                } else if field.eq_ignore_ascii_case("color_b") || field.eq_ignore_ascii_case("b") {
                    Some(color_b)
                } else {
                    None
                }
            }
            EffectType::TextOutline { color, .. }
                if field.eq_ignore_ascii_case("color") =>
            {
                Some(color)
            }
            EffectType::Stock { plugin, colors, .. } => {
                let idx = crate::stock::stock_color_slots(*plugin)
                    .iter()
                    .position(|s| s.eq_ignore_ascii_case(field))?;
                colors.get_mut(idx)
            }
            _ => None,
        }
    }

    /// Read access to a named color property (same fields as
    /// [`Self::get_color_property_mut`]) for keyframe-state lookups.
    pub fn get_color_property(&self, field: &str) -> Option<&Property<Color>> {
        match &self.effect_type {
            EffectType::Tint { map_black, map_white, .. } => {
                if field.eq_ignore_ascii_case("map_black") || field.eq_ignore_ascii_case("black") {
                    Some(map_black)
                } else if field.eq_ignore_ascii_case("map_white") || field.eq_ignore_ascii_case("white") {
                    Some(map_white)
                } else {
                    None
                }
            }
            EffectType::DropShadow { color, .. } => {
                if field.eq_ignore_ascii_case("color") { Some(color) } else { None }
            }
            EffectType::OuterGlow { color, .. } => {
                if field.eq_ignore_ascii_case("color") { Some(color) } else { None }
            }
            EffectType::ChromaKey { key_color, .. } => {
                if field.eq_ignore_ascii_case("key_color") || field.eq_ignore_ascii_case("color") {
                    Some(key_color)
                } else {
                    None
                }
            }
            EffectType::SwapColor { from_color, to_color, .. } => {
                if field.eq_ignore_ascii_case("from_color") || field.eq_ignore_ascii_case("from") {
                    Some(from_color)
                } else if field.eq_ignore_ascii_case("to_color") || field.eq_ignore_ascii_case("to") {
                    Some(to_color)
                } else {
                    None
                }
            }
            EffectType::Checkerboard { color_a, color_b, .. } => {
                if field.eq_ignore_ascii_case("color_a") || field.eq_ignore_ascii_case("a") {
                    Some(color_a)
                } else if field.eq_ignore_ascii_case("color_b") || field.eq_ignore_ascii_case("b") {
                    Some(color_b)
                } else {
                    None
                }
            }
            EffectType::GradientRamp { color_a, color_b, .. } => {
                if field.eq_ignore_ascii_case("color_a") || field.eq_ignore_ascii_case("a") {
                    Some(color_a)
                } else if field.eq_ignore_ascii_case("color_b") || field.eq_ignore_ascii_case("b") {
                    Some(color_b)
                } else {
                    None
                }
            }
            EffectType::TextOutline { color, .. }
                if field.eq_ignore_ascii_case("color") =>
            {
                Some(color)
            }
            EffectType::Stock { plugin, colors, .. } => {
                let idx = crate::stock::stock_color_slots(*plugin)
                    .iter()
                    .position(|s| s.eq_ignore_ascii_case(field))?;
                colors.get(idx)
            }
            _ => None,
        }
    }

    /// Read access to a named boolean property (`monochrome`, `mirror`)
    /// for keyframe-state lookups.
    pub fn get_bool_property(&self, field: &str) -> Option<&Property<bool>> {
        match &self.effect_type {
            EffectType::NoiseGenerator { monochrome, .. }
                if field.eq_ignore_ascii_case("monochrome") =>
            {
                Some(monochrome)
            }
            EffectType::Tiler { mirror, .. } if field.eq_ignore_ascii_case("mirror") => {
                Some(mirror)
            }
            EffectType::TextSplitAnimator { lock_layout, .. }
                if field.eq_ignore_ascii_case("lock_layout") =>
            {
                Some(lock_layout)
            }
            _ => None,
        }
    }

    /// Mutable access to a named boolean property (`monochrome`, `mirror`)
    /// for animation toggles and playhead commits.
    pub fn get_bool_property_mut(&mut self, field: &str) -> Option<&mut Property<bool>> {
        match &mut self.effect_type {
            EffectType::NoiseGenerator { monochrome, .. }
                if field.eq_ignore_ascii_case("monochrome") =>
            {
                Some(monochrome)
            }
            EffectType::Tiler { mirror, .. } if field.eq_ignore_ascii_case("mirror") => {
                Some(mirror)
            }
            EffectType::TextSplitAnimator { lock_layout, .. }
                if field.eq_ignore_ascii_case("lock_layout") =>
            {
                Some(lock_layout)
            }
            _ => None,
        }
    }

    /// Set a named enum option by index (`mode`, `cell`, …). Returns false
    /// for unknown fields or effects without that enum.
    pub fn set_enum_value(&mut self, field: &str, index: usize) -> bool {
        match &mut self.effect_type {
            EffectType::Tiler { mode, cell, .. } => {
                if field.eq_ignore_ascii_case("mode") {
                    if let Some(m) = TileMode::ALL.get(index).copied() {
                        *mode = m;
                        return true;
                    }
                } else if field.eq_ignore_ascii_case("cell") {
                    if let Some(c) = TileCell::ALL.get(index).copied() {
                        *cell = c;
                        return true;
                    }
                }
                false
            }
            EffectType::TextSplitAnimator { split_by, order, easing, .. } => {
                if field.eq_ignore_ascii_case("split_by") {
                    if let Some(s) = TextSplitBy::ALL.get(index).copied() {
                        *split_by = s;
                        return true;
                    }
                } else if field.eq_ignore_ascii_case("order") {
                    if let Some(o) = TextSplitOrder::ALL.get(index).copied() {
                        *order = o;
                        return true;
                    }
                } else if field.eq_ignore_ascii_case("easing") {
                    if let Some(e) = TextSplitEasing::ALL.get(index).copied() {
                        *easing = e;
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Read a named enum option index. Returns None for unknown fields.
    pub fn get_enum_value(&self, field: &str) -> Option<usize> {
        match &self.effect_type {
            EffectType::Tiler { mode, cell, .. } => {
                if field.eq_ignore_ascii_case("mode") {
                    Some(mode.index())
                } else if field.eq_ignore_ascii_case("cell") {
                    Some(cell.index())
                } else {
                    None
                }
            }
            EffectType::TextSplitAnimator { split_by, order, easing, .. } => {
                if field.eq_ignore_ascii_case("split_by") {
                    Some(split_by.index())
                } else if field.eq_ignore_ascii_case("order") {
                    Some(order.index())
                } else if field.eq_ignore_ascii_case("easing") {
                    Some(easing.index())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Set a named boolean flag (`mirror`, …). Returns false for unknown
    /// fields or effects without that flag.
    pub fn set_bool_value(&mut self, field: &str, next: bool) -> bool {
        match &mut self.effect_type {
            EffectType::NoiseGenerator { monochrome, .. }
                if field.eq_ignore_ascii_case("monochrome") =>
            {
                monochrome.set_value(next);
                true
            }
            EffectType::Tiler { mirror, .. } if field.eq_ignore_ascii_case("mirror") => {
                mirror.set_value(next);
                true
            }
            EffectType::TextSplitAnimator { lock_layout, .. }
                if field.eq_ignore_ascii_case("lock_layout") =>
            {
                lock_layout.set_value(next);
                true
            }
            _ => false,
        }
    }

    /// Current Shader Lab source (last-good), if this is a Shader Lab effect.
    pub fn shader_source(&self) -> Option<&str> {
        if let EffectType::ShaderLab { source, .. } = &self.effect_type {
            Some(source.as_str())
        } else {
            None
        }
    }

    /// Parsed Shader Lab parameters, if this is a Shader Lab effect.
    pub fn shader_params(&self) -> Option<&[ShaderParam]> {
        if let EffectType::ShaderLab { params, .. } = &self.effect_type {
            Some(params.as_slice())
        } else {
            None
        }
    }

    /// Shader Lab value overrides, if this is a Shader Lab effect.
    pub fn shader_values(&self) -> Option<&HashMap<String, ShaderParamValue>> {
        if let EffectType::ShaderLab { values, .. } = &self.effect_type {
            Some(values)
        } else {
            None
        }
    }

    /// Last Shader Lab compile error, if any.
    pub fn shader_error(&self) -> Option<&str> {
        if let EffectType::ShaderLab { compile_error, .. } = &self.effect_type {
            compile_error.as_deref()
        } else {
            None
        }
    }

    /// Defaults merged with overrides, in declaration order.
    pub fn resolved_shader_values(&self) -> Vec<(String, ShaderParamValue)> {
        if let EffectType::ShaderLab { params, values, .. } = &self.effect_type {
            params
                .iter()
                .map(|p| {
                    (
                        p.name.clone(),
                        values.get(&p.name).cloned().unwrap_or_else(|| p.default.clone()),
                    )
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Return the canonical type name of this effect.
    pub fn type_name(&self) -> &'static str {
        self.effect_type.type_name()
    }

    /// Toggle the enabled / bypassed state of this effect.
    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Nudge a numeric parameter by a delta value.
    pub fn nudge_param(&mut self, param_name: &str, delta: f32) -> bool {
        match &mut self.effect_type {
            EffectType::GaussianBlur { radius } => {
                if param_name.eq_ignore_ascii_case("radius") {
                    radius.set_value((radius.value + delta).max(0.0));
                    return true;
                }
            }
            EffectType::BrightnessContrast {
                brightness,
                contrast,
            } => {
                if param_name.eq_ignore_ascii_case("brightness") {
                    brightness.set_value((brightness.value + delta).clamp(-100.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("contrast") {
                    contrast.set_value((contrast.value + delta).clamp(-100.0, 100.0));
                    return true;
                }
            }
            EffectType::Tint { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    amount.set_value((amount.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::Invert { amount } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    amount.set_value((amount.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::DropShadow {
                distance,
                angle,
                softness,
                opacity,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("distance") {
                    distance.set_value((distance.value + delta).max(0.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("angle") {
                    let mut a = angle.value + delta;
                    while a < 0.0 {
                        a += 360.0;
                    }
                    angle.set_value(a % 360.0);
                    return true;
                } else if param_name.eq_ignore_ascii_case("softness") {
                    softness.set_value((softness.value + delta).max(0.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    opacity.set_value((opacity.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::OuterGlow {
                size,
                spread,
                opacity,
                range,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("size") || param_name.eq_ignore_ascii_case("radius") {
                    size.set_value((size.value + delta).max(0.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("spread") {
                    spread.set_value((spread.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    opacity.set_value((opacity.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("range") {
                    range.set_value((range.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::GlslShader {
                param1,
                param2,
                param3,
                param4,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("param1") || param_name.eq_ignore_ascii_case("p1") {
                    param1.set_value(param1.value + delta);
                    return true;
                } else if param_name.eq_ignore_ascii_case("param2") || param_name.eq_ignore_ascii_case("p2") {
                    param2.set_value(param2.value + delta);
                    return true;
                } else if param_name.eq_ignore_ascii_case("param3") || param_name.eq_ignore_ascii_case("p3") {
                    param3.set_value(param3.value + delta);
                    return true;
                } else if param_name.eq_ignore_ascii_case("param4") || param_name.eq_ignore_ascii_case("p4") {
                    param4.set_value(param4.value + delta);
                    return true;
                }
            }
            EffectType::DisplacementMap { max_horizontal, max_vertical } => {
                if param_name.eq_ignore_ascii_case("max_horizontal") || param_name.eq_ignore_ascii_case("horizontal") {
                    max_horizontal.set_value((max_horizontal.value + delta).clamp(-500.0, 500.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("max_vertical") || param_name.eq_ignore_ascii_case("vertical") {
                    max_vertical.set_value((max_vertical.value + delta).clamp(-500.0, 500.0));
                    return true;
                }
            }
            EffectType::ChromaKey { tolerance, feather, .. } => {
                if param_name.eq_ignore_ascii_case("tolerance") {
                    tolerance.set_value((tolerance.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("feather") {
                    feather.set_value((feather.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::LumaKey { threshold, feather } => {
                if param_name.eq_ignore_ascii_case("threshold") {
                    threshold.set_value((threshold.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("feather") {
                    feather.set_value((feather.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::SwapColor { tolerance, feather, .. } => {
                if param_name.eq_ignore_ascii_case("tolerance") {
                    tolerance.set_value((tolerance.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("feather") {
                    feather.set_value((feather.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::NoiseGenerator { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    amount.set_value((amount.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::Checkerboard { size, .. } => {
                if param_name.eq_ignore_ascii_case("size") {
                    size.set_value((size.value + delta).clamp(2.0, 512.0));
                    return true;
                }
            }
            EffectType::GradientRamp { angle, .. } => {
                if param_name.eq_ignore_ascii_case("angle") {
                    angle.set_value(angle.value + delta);
                    return true;
                }
            }
            EffectType::Perspective { skew_x, skew_y } => {
                if param_name.eq_ignore_ascii_case("skew_x") || param_name.eq_ignore_ascii_case("skewx") {
                    skew_x.set_value((skew_x.value + delta).clamp(-60.0, 60.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("skew_y") || param_name.eq_ignore_ascii_case("skewy") {
                    skew_y.set_value((skew_y.value + delta).clamp(-60.0, 60.0));
                    return true;
                }
            }
            EffectType::TextOutline { width, .. } => {
                if param_name.eq_ignore_ascii_case("width") {
                    width.set_value((width.value + delta).clamp(0.0, 50.0));
                    return true;
                }
            }
            EffectType::TextBevel { strength, softness } => {
                if param_name.eq_ignore_ascii_case("strength") {
                    strength.set_value((strength.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("softness") {
                    softness.set_value((softness.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::TextSplitAnimator {
                random_seed,
                progress,
                spread,
                position_x,
                position_y,
                rotation,
                opacity,
                anchor_x,
                anchor_y,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("random_seed") || param_name.eq_ignore_ascii_case("seed") {
                    random_seed.set_value((random_seed.value + delta).max(0.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("progress") {
                    progress.set_value((progress.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("spread") {
                    spread.set_value((spread.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("position_x") || param_name.eq_ignore_ascii_case("pos_x") {
                    position_x.set_value(position_x.value + delta);
                    return true;
                } else if param_name.eq_ignore_ascii_case("position_y") || param_name.eq_ignore_ascii_case("pos_y") {
                    position_y.set_value(position_y.value + delta);
                    return true;
                } else if param_name.eq_ignore_ascii_case("rotation") || param_name.eq_ignore_ascii_case("rot") {
                    rotation.set_value(rotation.value + delta);
                    return true;
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    opacity.set_value((opacity.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("anchor_x") {
                    anchor_x.set_value(anchor_x.value + delta);
                    return true;
                } else if param_name.eq_ignore_ascii_case("anchor_y") {
                    anchor_y.set_value(anchor_y.value + delta);
                    return true;
                }
            }
            EffectType::Bloom { intensity, radius } => {
                if param_name.eq_ignore_ascii_case("intensity") {
                    intensity.set_value((intensity.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("radius") {
                    radius.set_value((radius.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::Tiler { tiles_x, tiles_y, offset_x, offset_y, seed, amount, .. } => {
                if param_name.eq_ignore_ascii_case("tiles_x") || param_name.eq_ignore_ascii_case("x") {
                    tiles_x.set_value((tiles_x.value + delta).clamp(1.0, 32.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("tiles_y") || param_name.eq_ignore_ascii_case("y") {
                    tiles_y.set_value((tiles_y.value + delta).clamp(1.0, 32.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("offset_x") {
                    offset_x.set_value((offset_x.value + delta).clamp(0.0, 1.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("offset_y") {
                    offset_y.set_value((offset_y.value + delta).clamp(0.0, 1.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("seed") {
                    seed.set_value((seed.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("amount") || param_name.eq_ignore_ascii_case("randomize") {
                    amount.set_value((amount.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::Warp { amount, scale, cols, rows, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    amount.set_value((amount.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("scale") {
                    scale.set_value((scale.value + delta).clamp(0.1, 10.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("cols")
                    || param_name.eq_ignore_ascii_case("columns")
                {
                    cols.set_value((cols.value + delta).clamp(2.0, 8.0).round());
                    return true;
                } else if param_name.eq_ignore_ascii_case("rows") {
                    rows.set_value((rows.value + delta).clamp(2.0, 8.0).round());
                    return true;
                }
            }
            EffectType::Exposure { exposure } => {
                if param_name.eq_ignore_ascii_case("exposure") || param_name.eq_ignore_ascii_case("ev") {
                    exposure.set_value((exposure.value + delta).clamp(-10.0, 10.0));
                    return true;
                }
            }
            EffectType::Vibrance { vibrance } => {
                if param_name.eq_ignore_ascii_case("vibrance") {
                    vibrance.set_value((vibrance.value + delta).clamp(-100.0, 100.0));
                    return true;
                }
            }
            EffectType::Levels { input_black, input_white, gamma, output_black, output_white } => {
                if param_name.eq_ignore_ascii_case("input_black") {
                    input_black.set_value((input_black.value + delta).clamp(0.0, 255.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("input_white") {
                    input_white.set_value((input_white.value + delta).clamp(0.0, 255.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("gamma") {
                    gamma.set_value((gamma.value + delta).clamp(0.1, 9.9));
                    return true;
                } else if param_name.eq_ignore_ascii_case("output_black") {
                    output_black.set_value((output_black.value + delta).clamp(0.0, 255.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("output_white") {
                    output_white.set_value((output_white.value + delta).clamp(0.0, 255.0));
                    return true;
                }
            }
            EffectType::HueSaturation { hue_shift, saturation, lightness } => {
                if param_name.eq_ignore_ascii_case("hue_shift") || param_name.eq_ignore_ascii_case("hue") {
                    hue_shift.set_value((hue_shift.value + delta).clamp(-180.0, 180.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("saturation") || param_name.eq_ignore_ascii_case("sat") {
                    saturation.set_value((saturation.value + delta).clamp(-100.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("lightness") || param_name.eq_ignore_ascii_case("light") {
                    lightness.set_value((lightness.value + delta).clamp(-100.0, 100.0));
                    return true;
                }
            }
            EffectType::Sharpen { amount, radius } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    amount.set_value((amount.value + delta).clamp(0.0, 200.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("radius") {
                    radius.set_value((radius.value + delta).clamp(0.0, 20.0));
                    return true;
                }
            }
            EffectType::Vignette { amount, softness } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    amount.set_value((amount.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("softness") {
                    softness.set_value((softness.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::Stock { plugin, params, .. } => {
                let desc = plugin.descriptor();
                for (i, p) in desc.params.iter().enumerate() {
                    if param_name.eq_ignore_ascii_case(p.name) {
                        if let Some(prop) = params.get_mut(i) {
                            prop.set_value((prop.value + delta).clamp(p.min, p.max));
                            return true;
                        }
                    }
                }
            }
            // Shader Lab values are dynamic (see nudge_shader_value).
            EffectType::ShaderLab { .. } => {}
        }
        false
    }

    /// Retrieve an immutable reference to an animatable parameter property by name.
    pub fn get_param_property(&self, param_name: &str) -> Option<&Property<f32>> {
        match &self.effect_type {
            EffectType::GaussianBlur { radius } => {
                if param_name.eq_ignore_ascii_case("radius") {
                    Some(radius)
                } else {
                    None
                }
            }
            EffectType::BrightnessContrast {
                brightness,
                contrast,
            } => {
                if param_name.eq_ignore_ascii_case("brightness") {
                    Some(brightness)
                } else if param_name.eq_ignore_ascii_case("contrast") {
                    Some(contrast)
                } else {
                    None
                }
            }
            EffectType::Tint { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else {
                    None
                }
            }
            EffectType::Invert { amount } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else {
                    None
                }
            }
            EffectType::DropShadow {
                distance,
                angle,
                softness,
                opacity,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("distance") {
                    Some(distance)
                } else if param_name.eq_ignore_ascii_case("angle") {
                    Some(angle)
                } else if param_name.eq_ignore_ascii_case("softness") {
                    Some(softness)
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    Some(opacity)
                } else {
                    None
                }
            }
            EffectType::OuterGlow {
                size,
                spread,
                opacity,
                range,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("size") || param_name.eq_ignore_ascii_case("radius") {
                    Some(size)
                } else if param_name.eq_ignore_ascii_case("spread") {
                    Some(spread)
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    Some(opacity)
                } else if param_name.eq_ignore_ascii_case("range") {
                    Some(range)
                } else {
                    None
                }
            }
            EffectType::GlslShader {
                param1,
                param2,
                param3,
                param4,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("param1") || param_name.eq_ignore_ascii_case("p1") {
                    Some(param1)
                } else if param_name.eq_ignore_ascii_case("param2") || param_name.eq_ignore_ascii_case("p2") {
                    Some(param2)
                } else if param_name.eq_ignore_ascii_case("param3") || param_name.eq_ignore_ascii_case("p3") {
                    Some(param3)
                } else if param_name.eq_ignore_ascii_case("param4") || param_name.eq_ignore_ascii_case("p4") {
                    Some(param4)
                } else {
                    None
                }
            }
            EffectType::DisplacementMap { max_horizontal, max_vertical } => {
                if param_name.eq_ignore_ascii_case("max_horizontal") || param_name.eq_ignore_ascii_case("horizontal") {
                    Some(max_horizontal)
                } else if param_name.eq_ignore_ascii_case("max_vertical") || param_name.eq_ignore_ascii_case("vertical") {
                    Some(max_vertical)
                } else {
                    None
                }
            }
            EffectType::ChromaKey { tolerance, feather, .. } => {
                if param_name.eq_ignore_ascii_case("tolerance") {
                    Some(tolerance)
                } else if param_name.eq_ignore_ascii_case("feather") {
                    Some(feather)
                } else {
                    None
                }
            }
            EffectType::LumaKey { threshold, feather } => {
                if param_name.eq_ignore_ascii_case("threshold") {
                    Some(threshold)
                } else if param_name.eq_ignore_ascii_case("feather") {
                    Some(feather)
                } else {
                    None
                }
            }
            EffectType::SwapColor { tolerance, feather, .. } => {
                if param_name.eq_ignore_ascii_case("tolerance") {
                    Some(tolerance)
                } else if param_name.eq_ignore_ascii_case("feather") {
                    Some(feather)
                } else {
                    None
                }
            }
            EffectType::NoiseGenerator { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else {
                    None
                }
            }
            EffectType::Checkerboard { size, .. } => {
                if param_name.eq_ignore_ascii_case("size") {
                    Some(size)
                } else {
                    None
                }
            }
            EffectType::GradientRamp { angle, .. } => {
                if param_name.eq_ignore_ascii_case("angle") {
                    Some(angle)
                } else {
                    None
                }
            }
            EffectType::Perspective { skew_x, skew_y } => {
                if param_name.eq_ignore_ascii_case("skew_x") || param_name.eq_ignore_ascii_case("skewx") {
                    Some(skew_x)
                } else if param_name.eq_ignore_ascii_case("skew_y") || param_name.eq_ignore_ascii_case("skewy") {
                    Some(skew_y)
                } else {
                    None
                }
            }
            EffectType::TextOutline { width, .. } => {
                if param_name.eq_ignore_ascii_case("width") {
                    Some(width)
                } else {
                    None
                }
            }
            EffectType::TextBevel { strength, softness } => {
                if param_name.eq_ignore_ascii_case("strength") {
                    Some(strength)
                } else if param_name.eq_ignore_ascii_case("softness") {
                    Some(softness)
                } else {
                    None
                }
            }
            EffectType::TextSplitAnimator {
                random_seed,
                progress,
                spread,
                position_x,
                position_y,
                rotation,
                opacity,
                anchor_x,
                anchor_y,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("random_seed") || param_name.eq_ignore_ascii_case("seed") {
                    Some(random_seed)
                } else if param_name.eq_ignore_ascii_case("progress") {
                    Some(progress)
                } else if param_name.eq_ignore_ascii_case("spread") {
                    Some(spread)
                } else if param_name.eq_ignore_ascii_case("position_x") || param_name.eq_ignore_ascii_case("pos_x") {
                    Some(position_x)
                } else if param_name.eq_ignore_ascii_case("position_y") || param_name.eq_ignore_ascii_case("pos_y") {
                    Some(position_y)
                } else if param_name.eq_ignore_ascii_case("rotation") || param_name.eq_ignore_ascii_case("rot") {
                    Some(rotation)
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    Some(opacity)
                } else if param_name.eq_ignore_ascii_case("anchor_x") {
                    Some(anchor_x)
                } else if param_name.eq_ignore_ascii_case("anchor_y") {
                    Some(anchor_y)
                } else {
                    None
                }
            }
            EffectType::Bloom { intensity, radius } => {
                if param_name.eq_ignore_ascii_case("intensity") {
                    Some(intensity)
                } else if param_name.eq_ignore_ascii_case("radius") {
                    Some(radius)
                } else {
                    None
                }
            }
            EffectType::Tiler { tiles_x, tiles_y, offset_x, offset_y, seed, amount, .. } => {
                if param_name.eq_ignore_ascii_case("tiles_x") || param_name.eq_ignore_ascii_case("x") {
                    Some(tiles_x)
                } else if param_name.eq_ignore_ascii_case("tiles_y") || param_name.eq_ignore_ascii_case("y") {
                    Some(tiles_y)
                } else if param_name.eq_ignore_ascii_case("offset_x") {
                    Some(offset_x)
                } else if param_name.eq_ignore_ascii_case("offset_y") {
                    Some(offset_y)
                } else if param_name.eq_ignore_ascii_case("seed") {
                    Some(seed)
                } else if param_name.eq_ignore_ascii_case("amount") || param_name.eq_ignore_ascii_case("randomize") {
                    Some(amount)
                } else {
                    None
                }
            }
            EffectType::Warp { amount, scale, cols, rows, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else if param_name.eq_ignore_ascii_case("scale") {
                    Some(scale)
                } else if param_name.eq_ignore_ascii_case("cols")
                    || param_name.eq_ignore_ascii_case("columns")
                {
                    Some(cols)
                } else if param_name.eq_ignore_ascii_case("rows") {
                    Some(rows)
                } else {
                    None
                }
            }
            EffectType::Exposure { exposure } => {
                if param_name.eq_ignore_ascii_case("exposure") || param_name.eq_ignore_ascii_case("ev") {
                    Some(exposure)
                } else {
                    None
                }
            }
            EffectType::Vibrance { vibrance } => {
                if param_name.eq_ignore_ascii_case("vibrance") {
                    Some(vibrance)
                } else {
                    None
                }
            }
            EffectType::Levels { input_black, input_white, gamma, output_black, output_white } => {
                if param_name.eq_ignore_ascii_case("input_black") {
                    Some(input_black)
                } else if param_name.eq_ignore_ascii_case("input_white") {
                    Some(input_white)
                } else if param_name.eq_ignore_ascii_case("gamma") {
                    Some(gamma)
                } else if param_name.eq_ignore_ascii_case("output_black") {
                    Some(output_black)
                } else if param_name.eq_ignore_ascii_case("output_white") {
                    Some(output_white)
                } else {
                    None
                }
            }
            EffectType::HueSaturation { hue_shift, saturation, lightness } => {
                if param_name.eq_ignore_ascii_case("hue_shift") || param_name.eq_ignore_ascii_case("hue") {
                    Some(hue_shift)
                } else if param_name.eq_ignore_ascii_case("saturation") || param_name.eq_ignore_ascii_case("sat") {
                    Some(saturation)
                } else if param_name.eq_ignore_ascii_case("lightness") || param_name.eq_ignore_ascii_case("light") {
                    Some(lightness)
                } else {
                    None
                }
            }
            EffectType::Sharpen { amount, radius } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else if param_name.eq_ignore_ascii_case("radius") {
                    Some(radius)
                } else {
                    None
                }
            }
            EffectType::Vignette { amount, softness } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else if param_name.eq_ignore_ascii_case("softness") {
                    Some(softness)
                } else {
                    None
                }
            }
            EffectType::Stock { plugin, params, .. } => {
                let idx = plugin
                    .descriptor()
                    .params
                    .iter()
                    .position(|p| param_name.eq_ignore_ascii_case(p.name))?;
                params.get(idx)
            }
            // Shader Lab values are dynamic, not `Property<f32>` tracks.
            EffectType::ShaderLab { .. } => None,
        }
    }

    /// Retrieve a mutable reference to an animatable parameter property by name.
    pub fn get_param_property_mut(&mut self, param_name: &str) -> Option<&mut Property<f32>> {
        match &mut self.effect_type {
            EffectType::GaussianBlur { radius } => {
                if param_name.eq_ignore_ascii_case("radius") {
                    Some(radius)
                } else {
                    None
                }
            }
            EffectType::BrightnessContrast {
                brightness,
                contrast,
            } => {
                if param_name.eq_ignore_ascii_case("brightness") {
                    Some(brightness)
                } else if param_name.eq_ignore_ascii_case("contrast") {
                    Some(contrast)
                } else {
                    None
                }
            }
            EffectType::Tint { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else {
                    None
                }
            }
            EffectType::Invert { amount } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else {
                    None
                }
            }
            EffectType::DropShadow {
                distance,
                angle,
                softness,
                opacity,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("distance") {
                    Some(distance)
                } else if param_name.eq_ignore_ascii_case("angle") {
                    Some(angle)
                } else if param_name.eq_ignore_ascii_case("softness") {
                    Some(softness)
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    Some(opacity)
                } else {
                    None
                }
            }
            EffectType::OuterGlow {
                size,
                spread,
                opacity,
                range,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("size") || param_name.eq_ignore_ascii_case("radius") {
                    Some(size)
                } else if param_name.eq_ignore_ascii_case("spread") {
                    Some(spread)
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    Some(opacity)
                } else if param_name.eq_ignore_ascii_case("range") {
                    Some(range)
                } else {
                    None
                }
            }
            EffectType::GlslShader {
                param1,
                param2,
                param3,
                param4,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("param1") || param_name.eq_ignore_ascii_case("p1") {
                    Some(param1)
                } else if param_name.eq_ignore_ascii_case("param2") || param_name.eq_ignore_ascii_case("p2") {
                    Some(param2)
                } else if param_name.eq_ignore_ascii_case("param3") || param_name.eq_ignore_ascii_case("p3") {
                    Some(param3)
                } else if param_name.eq_ignore_ascii_case("param4") || param_name.eq_ignore_ascii_case("p4") {
                    Some(param4)
                } else {
                    None
                }
            }
            EffectType::DisplacementMap { max_horizontal, max_vertical } => {
                if param_name.eq_ignore_ascii_case("max_horizontal") || param_name.eq_ignore_ascii_case("horizontal") {
                    Some(max_horizontal)
                } else if param_name.eq_ignore_ascii_case("max_vertical") || param_name.eq_ignore_ascii_case("vertical") {
                    Some(max_vertical)
                } else {
                    None
                }
            }
            EffectType::ChromaKey { tolerance, feather, .. } => {
                if param_name.eq_ignore_ascii_case("tolerance") {
                    Some(tolerance)
                } else if param_name.eq_ignore_ascii_case("feather") {
                    Some(feather)
                } else {
                    None
                }
            }
            EffectType::LumaKey { threshold, feather } => {
                if param_name.eq_ignore_ascii_case("threshold") {
                    Some(threshold)
                } else if param_name.eq_ignore_ascii_case("feather") {
                    Some(feather)
                } else {
                    None
                }
            }
            EffectType::SwapColor { tolerance, feather, .. } => {
                if param_name.eq_ignore_ascii_case("tolerance") {
                    Some(tolerance)
                } else if param_name.eq_ignore_ascii_case("feather") {
                    Some(feather)
                } else {
                    None
                }
            }
            EffectType::NoiseGenerator { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else {
                    None
                }
            }
            EffectType::Checkerboard { size, .. } => {
                if param_name.eq_ignore_ascii_case("size") {
                    Some(size)
                } else {
                    None
                }
            }
            EffectType::GradientRamp { angle, .. } => {
                if param_name.eq_ignore_ascii_case("angle") {
                    Some(angle)
                } else {
                    None
                }
            }
            EffectType::Perspective { skew_x, skew_y } => {
                if param_name.eq_ignore_ascii_case("skew_x") || param_name.eq_ignore_ascii_case("skewx") {
                    Some(skew_x)
                } else if param_name.eq_ignore_ascii_case("skew_y") || param_name.eq_ignore_ascii_case("skewy") {
                    Some(skew_y)
                } else {
                    None
                }
            }
            EffectType::TextOutline { width, .. } => {
                if param_name.eq_ignore_ascii_case("width") {
                    Some(width)
                } else {
                    None
                }
            }
            EffectType::TextBevel { strength, softness } => {
                if param_name.eq_ignore_ascii_case("strength") {
                    Some(strength)
                } else if param_name.eq_ignore_ascii_case("softness") {
                    Some(softness)
                } else {
                    None
                }
            }
            EffectType::TextSplitAnimator {
                random_seed,
                progress,
                spread,
                position_x,
                position_y,
                rotation,
                opacity,
                anchor_x,
                anchor_y,
                ..
            } => {
                if param_name.eq_ignore_ascii_case("random_seed") || param_name.eq_ignore_ascii_case("seed") {
                    Some(random_seed)
                } else if param_name.eq_ignore_ascii_case("progress") {
                    Some(progress)
                } else if param_name.eq_ignore_ascii_case("spread") {
                    Some(spread)
                } else if param_name.eq_ignore_ascii_case("position_x") || param_name.eq_ignore_ascii_case("pos_x") {
                    Some(position_x)
                } else if param_name.eq_ignore_ascii_case("position_y") || param_name.eq_ignore_ascii_case("pos_y") {
                    Some(position_y)
                } else if param_name.eq_ignore_ascii_case("rotation") || param_name.eq_ignore_ascii_case("rot") {
                    Some(rotation)
                } else if param_name.eq_ignore_ascii_case("opacity") {
                    Some(opacity)
                } else if param_name.eq_ignore_ascii_case("anchor_x") {
                    Some(anchor_x)
                } else if param_name.eq_ignore_ascii_case("anchor_y") {
                    Some(anchor_y)
                } else {
                    None
                }
            }
            EffectType::Bloom { intensity, radius } => {
                if param_name.eq_ignore_ascii_case("intensity") {
                    Some(intensity)
                } else if param_name.eq_ignore_ascii_case("radius") {
                    Some(radius)
                } else {
                    None
                }
            }
            EffectType::Tiler { tiles_x, tiles_y, offset_x, offset_y, seed, amount, .. } => {
                if param_name.eq_ignore_ascii_case("tiles_x") || param_name.eq_ignore_ascii_case("x") {
                    Some(tiles_x)
                } else if param_name.eq_ignore_ascii_case("tiles_y") || param_name.eq_ignore_ascii_case("y") {
                    Some(tiles_y)
                } else if param_name.eq_ignore_ascii_case("offset_x") {
                    Some(offset_x)
                } else if param_name.eq_ignore_ascii_case("offset_y") {
                    Some(offset_y)
                } else if param_name.eq_ignore_ascii_case("seed") {
                    Some(seed)
                } else if param_name.eq_ignore_ascii_case("amount") || param_name.eq_ignore_ascii_case("randomize") {
                    Some(amount)
                } else {
                    None
                }
            }
            EffectType::Warp { amount, scale, cols, rows, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else if param_name.eq_ignore_ascii_case("scale") {
                    Some(scale)
                } else if param_name.eq_ignore_ascii_case("cols")
                    || param_name.eq_ignore_ascii_case("columns")
                {
                    Some(cols)
                } else if param_name.eq_ignore_ascii_case("rows") {
                    Some(rows)
                } else {
                    None
                }
            }
            EffectType::Exposure { exposure } => {
                if param_name.eq_ignore_ascii_case("exposure") || param_name.eq_ignore_ascii_case("ev") {
                    Some(exposure)
                } else {
                    None
                }
            }
            EffectType::Vibrance { vibrance } => {
                if param_name.eq_ignore_ascii_case("vibrance") {
                    Some(vibrance)
                } else {
                    None
                }
            }
            EffectType::Levels { input_black, input_white, gamma, output_black, output_white } => {
                if param_name.eq_ignore_ascii_case("input_black") {
                    Some(input_black)
                } else if param_name.eq_ignore_ascii_case("input_white") {
                    Some(input_white)
                } else if param_name.eq_ignore_ascii_case("gamma") {
                    Some(gamma)
                } else if param_name.eq_ignore_ascii_case("output_black") {
                    Some(output_black)
                } else if param_name.eq_ignore_ascii_case("output_white") {
                    Some(output_white)
                } else {
                    None
                }
            }
            EffectType::HueSaturation { hue_shift, saturation, lightness } => {
                if param_name.eq_ignore_ascii_case("hue_shift") || param_name.eq_ignore_ascii_case("hue") {
                    Some(hue_shift)
                } else if param_name.eq_ignore_ascii_case("saturation") || param_name.eq_ignore_ascii_case("sat") {
                    Some(saturation)
                } else if param_name.eq_ignore_ascii_case("lightness") || param_name.eq_ignore_ascii_case("light") {
                    Some(lightness)
                } else {
                    None
                }
            }
            EffectType::Sharpen { amount, radius } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else if param_name.eq_ignore_ascii_case("radius") {
                    Some(radius)
                } else {
                    None
                }
            }
            EffectType::Vignette { amount, softness } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else if param_name.eq_ignore_ascii_case("softness") {
                    Some(softness)
                } else {
                    None
                }
            }
            EffectType::Stock { plugin, params, .. } => {
                let idx = plugin
                    .descriptor()
                    .params
                    .iter()
                    .position(|p| param_name.eq_ignore_ascii_case(p.name))?;
                params.get_mut(idx)
            }
            // Shader Lab values are dynamic, not `Property<f32>` tracks.
            EffectType::ShaderLab { .. } => None,
        }
    }

    /// Names of the animatable color slots on this effect, if any.
    pub fn color_slots(&self) -> Vec<&'static str> {
        match &self.effect_type {
            EffectType::Stock { plugin, colors, .. } => {
                crate::stock::stock_color_slots(*plugin)
                    .iter()
                    .take(colors.len())
                    .copied()
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Read a stock color slot by name.
    pub fn stock_color(&self, slot: &str) -> Option<Color> {
        match &self.effect_type {
            EffectType::Stock { plugin, colors, .. } => {
                let idx = crate::stock::stock_color_slots(*plugin)
                    .iter()
                    .position(|s| s.eq_ignore_ascii_case(slot))?;
                colors.get(idx).map(|p| p.value)
            }
            _ => None,
        }
    }

    /// Write a stock color slot by name.
    pub fn set_stock_color(&mut self, slot: &str, next: Color) -> bool {
        match &mut self.effect_type {
            EffectType::Stock { plugin, colors, .. } => {
                let slots = crate::stock::stock_color_slots(*plugin);
                match slots.iter().position(|s| s.eq_ignore_ascii_case(slot)) {
                    Some(idx) if idx < colors.len() => {
                        colors[idx].set_value(next);
                        true
                    }
                    _ => false,
                }
            }
            _ => self.set_color_value(slot, next),
        }
    }

    /// All scalar params as `(name, label, value, step)` for generic UI
    /// (Properties rows, timeline lanes, spline legend). Only meaningful
    /// for stock plug-ins; legacy variants keep their bespoke editors.
    pub fn stock_scalar_params(&self) -> Vec<(&'static str, &'static str, f32, f32)> {
        match &self.effect_type {
            EffectType::Stock { plugin, params, .. } => {
                let desc = plugin.descriptor();
                desc.params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let v = params.get(i).map(|q| q.value).unwrap_or(p.default);
                        let step = ((p.max - p.min) / 40.0).clamp(0.01, 10.0);
                        (p.name, p.label, v, step)
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Stock plug-in of this effect, if it is one.
    pub fn stock_plugin(&self) -> Option<StockPlugin> {
        match &self.effect_type {
            EffectType::Stock { plugin, .. } => Some(*plugin),
            _ => None,
        }
    }

    /// Declarative widget descriptions for every parameter (the property
    /// declares its UI; the application auto-creates widgets from these —
    /// no per-effect UI code). Order matches the historic panel layout.
    pub fn declarations(&self) -> Vec<crate::widget::PropDecl> {
        use crate::widget::{ParamMeta, PropDecl, PropValue, WidgetKind};
        fn scalar(
            field: &'static str,
            label: &'static str,
            widget: WidgetKind,
            meta: ParamMeta,
            prop: &Property<f32>,
        ) -> PropDecl {
            PropDecl::scalar(field, label, widget, meta, prop.value, prop.is_animated())
        }
        let px1 = |min: f32, max: f32, step: f32, mult100: f32| {
            ParamMeta::slider(min, max, step, 1, "px", mult100)
        };
        match &self.effect_type {
            EffectType::GaussianBlur { radius } => vec![scalar(
                "radius",
                "Radius",
                WidgetKind::Slider,
                px1(0.0, 200.0, 5.0, 100.0),
                radius,
            )],
            EffectType::BrightnessContrast { brightness, contrast } => vec![
                scalar("brightness", "Brightness", WidgetKind::Slider, ParamMeta::slider(-100.0, 100.0, 5.0, 1, "", 100.0), brightness),
                scalar("contrast", "Contrast", WidgetKind::Slider, ParamMeta::slider(-100.0, 100.0, 5.0, 1, "", 100.0), contrast),
            ],
            EffectType::Tint { map_black, map_white, amount } => vec![
                PropDecl::color("map_black", "Map Black", map_black.value, map_black.is_animated()),
                PropDecl::color("map_white", "Map White", map_white.value, map_white.is_animated()),
                scalar("amount", "Amount", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 10.0, 0, " %", 100.0), amount),
            ],
            EffectType::Invert { amount } => vec![scalar(
                "amount",
                "Amount",
                WidgetKind::Percentage,
                ParamMeta::slider(0.0, 100.0, 10.0, 0, " %", 100.0),
                amount,
            )],
            EffectType::DropShadow { distance, angle, softness, opacity, color } => vec![
                scalar("distance", "Distance", WidgetKind::Slider, px1(0.0, 200.0, 2.0, 50.0), distance),
                scalar("angle", "Angle", WidgetKind::Angle, ParamMeta::slider(0.0, 360.0, 15.0, 1, "°", 360.0), angle),
                scalar("softness", "Softness", WidgetKind::Slider, px1(0.0, 100.0, 2.0, 50.0), softness),
                scalar("opacity", "Opacity", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 10.0, 0, " %", 100.0), opacity),
                PropDecl::color("color", "Color", color.value, color.is_animated()),
            ],
            EffectType::OuterGlow { size, spread, opacity, color, range } => vec![
                scalar("size", "Size", WidgetKind::Slider, px1(0.0, 250.0, 2.0, 50.0), size),
                scalar("spread", "Spread", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, " %", 100.0), spread),
                scalar("opacity", "Opacity", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, " %", 100.0), opacity),
                PropDecl::color("color", "Color", color.value, color.is_animated()),
                scalar("range", "Range", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, " %", 100.0), range),
            ],
            EffectType::GlslShader { param1, param2, param3, param4, .. } => vec![
                scalar("param1", "P1 (Speed)", WidgetKind::Slider, ParamMeta::slider(-100.0, 100.0, 0.5, 2, "", 10.0), param1),
                scalar("param2", "P2 (Intensity)", WidgetKind::Slider, ParamMeta::slider(-100.0, 100.0, 5.0, 1, "", 100.0), param2),
                scalar("param3", "P3 (Scale)", WidgetKind::Slider, ParamMeta::slider(-100.0, 100.0, 0.5, 2, "", 10.0), param3),
                scalar("param4", "P4 (Opacity)", WidgetKind::Slider, ParamMeta::slider(-100.0, 100.0, 5.0, 1, "", 100.0), param4),
            ],
            EffectType::ShaderLab { .. } => Vec::new(),
            EffectType::DisplacementMap { max_horizontal, max_vertical } => vec![
                scalar("max_horizontal", "Max Horizontal", WidgetKind::Slider, px1(-500.0, 500.0, 5.0, 100.0), max_horizontal),
                scalar("max_vertical", "Max Vertical", WidgetKind::Slider, px1(-500.0, 500.0, 5.0, 100.0), max_vertical),
            ],
            EffectType::ChromaKey { key_color, tolerance, feather } => vec![
                PropDecl::color("key_color", "Key Color", key_color.value, key_color.is_animated()),
                scalar("tolerance", "Tolerance", WidgetKind::Slider, ParamMeta::slider(0.0, 100.0, 5.0, 1, "", 100.0), tolerance),
                scalar("feather", "Feather", WidgetKind::Slider, ParamMeta::slider(0.0, 100.0, 2.0, 1, "", 100.0), feather),
            ],
            EffectType::LumaKey { threshold, feather } => vec![
                scalar("threshold", "Threshold", WidgetKind::Slider, ParamMeta::slider(0.0, 100.0, 5.0, 1, "", 100.0), threshold),
                scalar("feather", "Feather", WidgetKind::Slider, ParamMeta::slider(0.0, 100.0, 2.0, 1, "", 100.0), feather),
            ],
            EffectType::SwapColor { from_color, to_color, tolerance, feather } => vec![
                PropDecl::color("from_color", "From Color", from_color.value, from_color.is_animated()),
                PropDecl::color("to_color", "To Color", to_color.value, to_color.is_animated()),
                scalar("tolerance", "Tolerance", WidgetKind::Slider, ParamMeta::slider(0.0, 100.0, 5.0, 1, "", 100.0), tolerance),
                scalar("feather", "Feather", WidgetKind::Slider, ParamMeta::slider(0.0, 100.0, 2.0, 1, "", 100.0), feather),
            ],
            EffectType::NoiseGenerator { amount, monochrome } => vec![
                scalar("amount", "Amount", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 1, "%", 100.0), amount),
                PropDecl::boolean("monochrome", "Monochrome", monochrome.value, monochrome.is_animated()),
            ],
            EffectType::Checkerboard { size, color_a, color_b } => vec![
                PropDecl::color("color_a", "Color A", color_a.value, color_a.is_animated()),
                PropDecl::color("color_b", "Color B", color_b.value, color_b.is_animated()),
                scalar("size", "Size", WidgetKind::Slider, ParamMeta::slider(2.0, 512.0, 4.0, 0, "px", 100.0), size),
            ],
            EffectType::GradientRamp { color_a, color_b, angle, .. } => vec![
                PropDecl::color("color_a", "Start", color_a.value, color_a.is_animated()),
                PropDecl::color("color_b", "End", color_b.value, color_b.is_animated()),
                scalar("angle", "Angle", WidgetKind::Angle, ParamMeta::slider(0.0, 360.0, 5.0, 0, "°", 100.0), angle),
            ],
            EffectType::Perspective { skew_x, skew_y } => vec![
                scalar("skew_x", "Skew X", WidgetKind::Angle, ParamMeta::slider(-60.0, 60.0, 1.0, 1, "°", 100.0), skew_x),
                scalar("skew_y", "Skew Y", WidgetKind::Angle, ParamMeta::slider(-60.0, 60.0, 1.0, 1, "°", 100.0), skew_y),
            ],
            EffectType::TextOutline { width, color } => vec![
                PropDecl::color("color", "Color", color.value, color.is_animated()),
                scalar("width", "Width", WidgetKind::Slider, px1(0.0, 64.0, 1.0, 100.0), width),
            ],
            EffectType::TextBevel { strength, softness } => vec![
                scalar("strength", "Strength", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, "%", 100.0), strength),
                scalar("softness", "Softness", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, "%", 100.0), softness),
            ],
            EffectType::TextSplitAnimator {
                split_by,
                order,
                random_seed,
                progress,
                spread,
                lock_layout,
                easing,
                position_x,
                position_y,
                rotation,
                opacity,
                anchor_x,
                anchor_y,
            } => vec![
                PropDecl::enumeration(
                    "split_by",
                    "Split By",
                    TextSplitBy::ALL.iter().map(|s| s.label().to_string()).collect(),
                    split_by.index(),
                ),
                PropDecl::enumeration(
                    "order",
                    "Order",
                    TextSplitOrder::ALL.iter().map(|o| o.label().to_string()).collect(),
                    order.index(),
                ),
                scalar("random_seed", "Random Seed", WidgetKind::Integer, ParamMeta::slider(0.0, 99999.0, 1.0, 0, "", 100.0), random_seed),
                scalar("progress", "Progress", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 1.0, 0, "%", 100.0), progress),
                scalar("spread", "Spread / Overlap", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 1.0, 0, "%", 100.0), spread),
                PropDecl::boolean("lock_layout", "Lock Layout", lock_layout.value, lock_layout.is_animated()),
                PropDecl::enumeration(
                    "easing",
                    "Easing Curve",
                    TextSplitEasing::ALL.iter().map(|e| e.label().to_string()).collect(),
                    easing.index(),
                ),
                scalar("position_x", "Position X", WidgetKind::Slider, ParamMeta::slider(-1000.0, 1000.0, 1.0, 1, " px", 100.0), position_x),
                scalar("position_y", "Position Y", WidgetKind::Slider, ParamMeta::slider(-1000.0, 1000.0, 1.0, 1, " px", 100.0), position_y),
                scalar("rotation", "Rotation (Angle)", WidgetKind::Angle, ParamMeta::slider(-360.0, 360.0, 1.0, 1, " deg", 100.0), rotation),
                scalar("opacity", "Opacity", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 1.0, 0, "%", 100.0), opacity),
                scalar("anchor_x", "Anchor Point X", WidgetKind::Slider, ParamMeta::slider(-500.0, 500.0, 1.0, 1, " px", 100.0), anchor_x),
                scalar("anchor_y", "Anchor Point Y", WidgetKind::Slider, ParamMeta::slider(-500.0, 500.0, 1.0, 1, " px", 100.0), anchor_y),
            ],
            EffectType::Bloom { intensity, radius } => vec![
                scalar("intensity", "Intensity", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, "%", 100.0), intensity),
                scalar("radius", "Radius", WidgetKind::Slider, px1(0.0, 128.0, 2.0, 100.0), radius),
            ],
            EffectType::Tiler { tiles_x, tiles_y, mode, mirror, offset_x, offset_y, cell, seed, amount } => {
                vec![
                    PropDecl::enumeration(
                        "mode",
                        "Layout",
                        TileMode::ALL.iter().map(|m| m.label().to_string()).collect(),
                        mode.index(),
                    ),
                    scalar("tiles_x", "Tiles X", WidgetKind::Integer, ParamMeta::slider(1.0, 64.0, 1.0, 0, "", 100.0), tiles_x),
                    scalar("tiles_y", "Tiles Y", WidgetKind::Integer, ParamMeta::slider(1.0, 64.0, 1.0, 0, "", 100.0), tiles_y),
                    PropDecl::enumeration(
                        "cell",
                        "Cell Shape",
                        TileCell::ALL.iter().map(|c| c.label().to_string()).collect(),
                        cell.index(),
                    ),
                    PropDecl::boolean("mirror", "Mirror Tiles", mirror.value, mirror.is_animated()),
                    scalar("offset_x", "Offset X", WidgetKind::Slider, ParamMeta::slider(0.0, 1.0, 0.01, 2, "", 100.0), offset_x),
                    scalar("offset_y", "Offset Y", WidgetKind::Slider, ParamMeta::slider(0.0, 1.0, 0.01, 2, "", 100.0), offset_y),
                    scalar("seed", "Random Seed", WidgetKind::Slider, ParamMeta::slider(0.0, 100.0, 1.0, 0, "", 100.0), seed),
                    scalar("amount", "Randomize", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, "%", 100.0), amount),
                ]
            }
            EffectType::Warp { amount, scale, cols, rows, .. } => vec![
                scalar("amount", "Amount", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, "%", 100.0), amount),
                scalar("scale", "Scale", WidgetKind::Slider, ParamMeta::slider(0.1, 32.0, 0.2, 1, "", 100.0), scale),
                scalar("cols", "Columns", WidgetKind::Integer, ParamMeta::slider(2.0, 8.0, 1.0, 0, "", 100.0), cols),
                scalar("rows", "Rows", WidgetKind::Integer, ParamMeta::slider(2.0, 8.0, 1.0, 0, "", 100.0), rows),
            ],
            EffectType::Exposure { exposure } => vec![PropDecl {
                field: "exposure".to_string(),
                label: "Exposure".to_string(),
                widget: WidgetKind::Slider,
                meta: ParamMeta { signed: true, unit: " EV".to_string(), ..ParamMeta::slider(-8.0, 8.0, 0.25, 2, "", 100.0) },
                value: PropValue::Float { value: exposure.value, animated: exposure.is_animated() },
            }],
            EffectType::Vibrance { vibrance } => vec![PropDecl {
                field: "vibrance".to_string(),
                label: "Vibrance".to_string(),
                widget: WidgetKind::Slider,
                meta: ParamMeta { signed: true, ..ParamMeta::slider(-100.0, 100.0, 5.0, 0, "", 100.0) },
                value: PropValue::Float { value: vibrance.value, animated: vibrance.is_animated() },
            }],
            EffectType::Levels { input_black, input_white, gamma, output_black, output_white } => vec![
                scalar("input_black", "Input Black", WidgetKind::Slider, ParamMeta::slider(0.0, 255.0, 5.0, 0, "", 100.0), input_black),
                scalar("input_white", "Input White", WidgetKind::Slider, ParamMeta::slider(0.0, 255.0, 5.0, 0, "", 100.0), input_white),
                scalar("gamma", "Gamma", WidgetKind::Slider, ParamMeta::slider(0.1, 8.0, 0.1, 2, "", 100.0), gamma),
                scalar("output_black", "Output Black", WidgetKind::Slider, ParamMeta::slider(0.0, 255.0, 5.0, 0, "", 100.0), output_black),
                scalar("output_white", "Output White", WidgetKind::Slider, ParamMeta::slider(0.0, 255.0, 5.0, 0, "", 100.0), output_white),
            ],
            EffectType::HueSaturation { hue_shift, saturation, lightness } => vec![
                PropDecl {
                    field: "hue_shift".to_string(),
                    label: "Hue Shift".to_string(),
                    widget: WidgetKind::Angle,
                    meta: ParamMeta { signed: true, ..ParamMeta::slider(-180.0, 180.0, 5.0, 0, "°", 100.0) },
                    value: PropValue::Float { value: hue_shift.value, animated: hue_shift.is_animated() },
                },
                PropDecl {
                    field: "saturation".to_string(),
                    label: "Saturation".to_string(),
                    widget: WidgetKind::Slider,
                    meta: ParamMeta { signed: true, ..ParamMeta::slider(-100.0, 100.0, 5.0, 0, "", 100.0) },
                    value: PropValue::Float { value: saturation.value, animated: saturation.is_animated() },
                },
                PropDecl {
                    field: "lightness".to_string(),
                    label: "Lightness".to_string(),
                    widget: WidgetKind::Slider,
                    meta: ParamMeta { signed: true, ..ParamMeta::slider(-100.0, 100.0, 5.0, 0, "", 100.0) },
                    value: PropValue::Float { value: lightness.value, animated: lightness.is_animated() },
                },
            ],
            EffectType::Sharpen { amount, radius } => vec![
                scalar("amount", "Amount", WidgetKind::Percentage, ParamMeta::slider(0.0, 200.0, 5.0, 0, "%", 100.0), amount),
                scalar("radius", "Radius", WidgetKind::Slider, px1(0.0, 32.0, 0.5, 100.0), radius),
            ],
            EffectType::Vignette { amount, softness } => vec![
                scalar("amount", "Amount", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, "%", 100.0), amount),
                scalar("softness", "Softness", WidgetKind::Percentage, ParamMeta::slider(0.0, 100.0, 5.0, 0, "%", 100.0), softness),
            ],
            EffectType::Stock { .. } => {
                let mut out = Vec::new();
                for (name, label, value, step) in self.stock_scalar_params() {
                    out.push(PropDecl::scalar(
                        name,
                        label,
                        WidgetKind::Slider,
                        ParamMeta::slider(f32::NEG_INFINITY, f32::INFINITY, step, 2, "", 100.0),
                        value,
                        self.get_param_property(name).map(|p| p.is_animated()).unwrap_or(false),
                    ));
                }
                for slot in self.color_slots() {
                    if let Some(prop) = self.get_color_property(slot) {
                        out.push(PropDecl::color(slot, slot, prop.value, prop.is_animated()));
                    }
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Color {
        Color::rgb(1.0, 0.0, 0.0)
    }

    #[test]
    fn every_color_field_accepts_custom_colors() {
        // (factory, field) pairs mirroring the Properties color rows: each
        // must round-trip an arbitrary custom color (wheel), not just
        // presets.
        let mut cases: Vec<(Effect, &'static str)> = vec![
            (Effect::tint("t", Color::BLACK, Color::WHITE, 100.0), "map_black"),
            (Effect::tint("t", Color::BLACK, Color::WHITE, 100.0), "map_white"),
            (Effect::drop_shadow("d", 5.0, 45.0, 5.0, 80.0, Color::BLACK), "color"),
            (
                Effect::chroma_key("c", Color::from_hex("#00FF00").unwrap(), 30.0, 10.0),
                "key_color",
            ),
            (
                Effect::checkerboard("cb", 32.0, Color::BLACK, Color::WHITE),
                "color_a",
            ),
            (
                Effect::checkerboard("cb", 32.0, Color::BLACK, Color::WHITE),
                "color_b",
            ),
            (
                Effect::gradient_ramp("g", Color::BLACK, Color::WHITE, 90.0),
                "color_a",
            ),
            (
                Effect::gradient_ramp("g", Color::BLACK, Color::WHITE, 90.0),
                "color_b",
            ),
            (Effect::text_outline("o", 3.0, Color::BLACK), "color"),
        ];
        for (fx, field) in cases.iter_mut() {
            assert!(fx.set_color_value(field, red()), "{field}");
            assert_eq!(fx_color_of(fx, field), Some(red()), "{field}");
        }
        assert!(!cases[0].0.set_color_value("nope", red()));
        // Stock generator slots route through the same setter.
        let mut stock = Effect::stock("s", StockPlugin::Solid);
        assert!(stock.set_stock_color("color", red()));
        assert_eq!(stock.stock_color("color"), Some(red()));
    }

    #[test]
    fn stock_colors_are_keyframable_properties() {
        use crate::keyframe::Keyframe;
        use crate::timecode::TimeCode;
        let mut fx = Effect::stock("s", StockPlugin::Solid);
        let prop = fx.get_color_property_mut("color").expect("stock color slot");
        prop.add_keyframe(Keyframe::new(TimeCode::from_frames(0, 30.0), Color::BLACK));
        prop.add_keyframe(Keyframe::new(TimeCode::from_frames(30, 30.0), Color::WHITE));
        assert!(fx.get_color_property("color").unwrap().is_animated());
        let mid = fx
            .get_color_property("color")
            .unwrap()
            .evaluate_at(&TimeCode::from_frames(15, 30.0));
        assert!((mid.r - 0.5).abs() < 1e-5, "{mid:?}");
    }

    #[test]
    fn bare_stock_colors_migrate_to_properties() {
        // Pre-keyframe stock files store bare slot colors.
        let fx: Effect = serde_json::from_str(
            r#"{"id": "s", "name": "Solid", "effect_type": {"type": "stock", "plugin": "solid", "params": [], "colors": [{"r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0}]}}"#,
        )
        .unwrap();
        assert_eq!(fx.stock_color("color"), Some(red()));
    }

    #[test]
    fn bool_props_toggle_through_shared_accessors() {
        let mut fx = Effect::noise_generator("n", 50.0, false);
        assert!(!fx.get_bool_property("monochrome").unwrap().value);
        fx.get_bool_property_mut("monochrome").unwrap().set_value(true);
        assert!(fx.get_bool_property("monochrome").unwrap().value);
        assert!(fx.set_bool_value("monochrome", false));
        assert!(fx.get_bool_property("mirror").is_none());
    }

    fn fx_color_of(fx: &Effect, field: &str) -> Option<Color> {
        match &fx.effect_type {
            EffectType::Tint { map_black, map_white, .. } => match field {
                "map_black" => Some(map_black.value),
                "map_white" => Some(map_white.value),
                _ => None,
            },
            EffectType::DropShadow { color, .. } => Some(color.value),
            EffectType::ChromaKey { key_color, .. } => Some(key_color.value),
            EffectType::SwapColor { from_color, to_color, .. } => match field {
                "from_color" => Some(from_color.value),
                "to_color" => Some(to_color.value),
                _ => None,
            },
            EffectType::Checkerboard { color_a, color_b, .. } => match field {
                "color_a" => Some(color_a.value),
                "color_b" => Some(color_b.value),
                _ => None,
            },
            EffectType::GradientRamp { color_a, color_b, .. } => match field {
                "color_a" => Some(color_a.value),
                "color_b" => Some(color_b.value),
                _ => None,
            },
            EffectType::TextOutline { color, .. } => Some(color.value),
            _ => None,
        }
    }
}
