use crate::Color;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Layer compositing blend modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Dissolve,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Add,
    Subtract,
    LinearBurn,
    LinearDodge,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Divide,
}

impl BlendMode {
    /// All 26 compositing blend modes in standard display order.
    pub const ALL: [Self; 26] = [
        Self::Normal,
        Self::Dissolve,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::HardLight,
        Self::SoftLight,
        Self::Difference,
        Self::Exclusion,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
        Self::Add,
        Self::Subtract,
        Self::LinearBurn,
        Self::LinearDodge,
        Self::VividLight,
        Self::LinearLight,
        Self::PinLight,
        Self::HardMix,
        Self::Divide,
    ];

    /// Return the canonical display name of the blend mode.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Dissolve => "Dissolve",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::ColorDodge => "Color Dodge",
            Self::ColorBurn => "Color Burn",
            Self::HardLight => "Hard Light",
            Self::SoftLight => "Soft Light",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::LinearBurn => "Linear Burn",
            Self::LinearDodge => "Linear Dodge (Add)",
            Self::VividLight => "Vivid Light",
            Self::LinearLight => "Linear Light",
            Self::PinLight => "Pin Light",
            Self::HardMix => "Hard Mix",
            Self::Divide => "Divide",
        }
    }

    /// Return the canonical snake_case identifier of the blend mode.
    pub const fn as_snake_case(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Dissolve => "dissolve",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Overlay => "overlay",
            Self::Darken => "darken",
            Self::Lighten => "lighten",
            Self::ColorDodge => "color_dodge",
            Self::ColorBurn => "color_burn",
            Self::HardLight => "hard_light",
            Self::SoftLight => "soft_light",
            Self::Difference => "difference",
            Self::Exclusion => "exclusion",
            Self::Hue => "hue",
            Self::Saturation => "saturation",
            Self::Color => "color",
            Self::Luminosity => "luminosity",
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::LinearBurn => "linear_burn",
            Self::LinearDodge => "linear_dodge",
            Self::VividLight => "vivid_light",
            Self::LinearLight => "linear_light",
            Self::PinLight => "pin_light",
            Self::HardMix => "hard_mix",
            Self::Divide => "divide",
        }
    }

    /// Parse a blend mode from its name (case-insensitive, supporting snake_case, spaces, and hyphens).
    pub fn from_name(name: &str) -> Option<Self> {
        let trimmed = name.trim().to_lowercase();
        let normalized = trimmed.replace(['_', '-'], " ");
        match normalized.as_str() {
            "normal" => Some(Self::Normal),
            "dissolve" => Some(Self::Dissolve),
            "multiply" => Some(Self::Multiply),
            "screen" => Some(Self::Screen),
            "overlay" => Some(Self::Overlay),
            "darken" => Some(Self::Darken),
            "lighten" => Some(Self::Lighten),
            "color dodge" | "colordodge" => Some(Self::ColorDodge),
            "color burn" | "colorburn" => Some(Self::ColorBurn),
            "hard light" | "hardlight" => Some(Self::HardLight),
            "soft light" | "softlight" => Some(Self::SoftLight),
            "difference" => Some(Self::Difference),
            "exclusion" => Some(Self::Exclusion),
            "hue" => Some(Self::Hue),
            "saturation" => Some(Self::Saturation),
            "color" => Some(Self::Color),
            "luminosity" => Some(Self::Luminosity),
            "add" => Some(Self::Add),
            "subtract" => Some(Self::Subtract),
            "linear burn" | "linearburn" => Some(Self::LinearBurn),
            "linear dodge" | "lineardodge" | "linear dodge (add)" => Some(Self::LinearDodge),
            "vivid light" | "vividlight" => Some(Self::VividLight),
            "linear light" | "linearlight" => Some(Self::LinearLight),
            "pin light" | "pinlight" => Some(Self::PinLight),
            "hard mix" | "hardmix" => Some(Self::HardMix),
            "divide" => Some(Self::Divide),
            _ => None,
        }
    }

    /// Blend the source and backdrop RGB values, without applying alpha.
    ///
    /// This follows the W3C compositing model.  [`composite`] should normally
    /// be used instead, as it also handles transparent pixels correctly.
    pub fn blend_rgb(self, backdrop: Color, source: Color) -> Color {
        if matches!(
            self,
            Self::Hue | Self::Saturation | Self::Color | Self::Luminosity
        ) {
            return component_blend(self, backdrop, source);
        }
        let b = |cb: f32, cs: f32| match self {
            Self::Normal | Self::Dissolve => cs,
            Self::Multiply => cb * cs,
            Self::Screen => cb + cs - cb * cs,
            Self::Overlay => if cb <= 0.5 { 2.0 * cb * cs } else { 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs) },
            Self::Darken => cb.min(cs),
            Self::Lighten => cb.max(cs),
            Self::ColorDodge => if cs >= 1.0 { 1.0 } else { (cb / (1.0 - cs)).min(1.0) },
            Self::ColorBurn => if cs <= 0.0 { 0.0 } else { 1.0 - ((1.0 - cb) / cs).min(1.0) },
            Self::HardLight => if cs <= 0.5 { 2.0 * cb * cs } else { 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs) },
            Self::SoftLight => soft_light(cb, cs),
            Self::Difference => (cb - cs).abs(),
            Self::Exclusion => cb + cs - 2.0 * cb * cs,
            Self::Add | Self::LinearDodge => (cb + cs).min(1.0),
            Self::Subtract => (cb - cs).max(0.0),
            Self::LinearBurn => (cb + cs - 1.0).max(0.0),
            Self::VividLight => {
                if cs <= 0.5 {
                    if cs <= 0.0 { 0.0 } else { (1.0 - (1.0 - cb) / (2.0 * cs)).clamp(0.0, 1.0) }
                } else if cs >= 1.0 {
                    1.0
                } else {
                    (cb / (2.0 * (1.0 - cs))).clamp(0.0, 1.0)
                }
            }
            Self::LinearLight => (cb + 2.0 * cs - 1.0).clamp(0.0, 1.0),
            Self::PinLight => {
                if cs > 0.5 {
                    cb.max(2.0 * (cs - 0.5))
                } else {
                    cb.min(2.0 * cs)
                }
            }
            Self::HardMix => {
                let vl = if cs <= 0.5 {
                    if cs <= 0.0 { 0.0 } else { (1.0 - (1.0 - cb) / (2.0 * cs)).clamp(0.0, 1.0) }
                } else if cs >= 1.0 {
                    1.0
                } else {
                    (cb / (2.0 * (1.0 - cs))).clamp(0.0, 1.0)
                };
                if vl >= 0.5 { 1.0 } else { 0.0 }
            }
            Self::Divide => {
                if cb <= 0.0 {
                    0.0
                } else if cs <= 0.0 {
                    1.0
                } else {
                    (cb / cs).min(1.0)
                }
            }
            Self::Hue | Self::Saturation | Self::Color | Self::Luminosity => unreachable!(),
        };
        Color::rgba(b(backdrop.r, source.r), b(backdrop.g, source.g), b(backdrop.b, source.b), source.a)
    }

    /// Composite `source` over `backdrop` using this blend mode.
    ///
    /// Both colors use straight alpha. This is suitable for preview, export,
    /// and GPU pass validation; it is not merely a color swatch approximation.
    pub fn composite(self, backdrop: Color, source: Color) -> Color {
        let sa = source.a;
        let da = backdrop.a;
        let blended = self.blend_rgb(backdrop, source);
        let out_a = sa + da - sa * da;
        if out_a <= f32::EPSILON {
            return Color::TRANSPARENT;
        }
        let channel = |cb: f32, cs: f32, blend: f32| {
            (((1.0 - sa) * da * cb) + ((1.0 - da) * sa * cs) + (sa * da * blend)) / out_a
        };
        Color::rgba(
            channel(backdrop.r, source.r, blended.r),
            channel(backdrop.g, source.g, blended.g),
            channel(backdrop.b, source.b, blended.b),
            out_a,
        )
    }
}

fn soft_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 { ((16.0 * cb - 12.0) * cb + 4.0) * cb } else { cb.sqrt() };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

fn component_blend(mode: BlendMode, backdrop: Color, source: Color) -> Color {
    let (bh, bs, bl) = rgb_to_hsl(backdrop);
    let (sh, ss, sl) = rgb_to_hsl(source);
    let (h, s, l) = match mode {
        BlendMode::Hue => (sh, bs, bl),
        BlendMode::Saturation => (bh, ss, bl),
        BlendMode::Color => (sh, ss, bl),
        BlendMode::Luminosity => (bh, bs, sl),
        _ => unreachable!("component_blend is only called for component modes"),
    };
    let (r, g, b) = hsl_to_rgb(h, s, l);
    Color::rgba(r, g, b, source.a)
}

fn rgb_to_hsl(c: Color) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let l = (max + min) * 0.5;
    let delta = max - min;
    if delta <= f32::EPSILON { return (0.0, 0.0, l); }
    let s = delta / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == c.r { ((c.g - c.b) / delta).rem_euclid(6.0) }
        else if max == c.g { (c.b - c.r) / delta + 2.0 }
        else { (c.r - c.g) / delta + 4.0 } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s <= f32::EPSILON { return (l, l, l); }
    let hue = |n: f32| {
        let k = (n + h * 12.0).rem_euclid(12.0);
        l - s * l.min(1.0 - l) * (-1.0f32).max((k - 3.0).min(9.0 - k).min(1.0))
    };
    (hue(0.0), hue(8.0), hue(4.0))
}

impl fmt::Display for BlendMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for BlendMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_snake_case())
    }
}

impl<'de> Deserialize<'de> for BlendMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        BlendMode::from_name(&s).ok_or_else(|| {
            serde::de::Error::unknown_variant(
                &s,
                &[
                    "normal", "dissolve", "multiply", "screen", "overlay", "darken", "lighten",
                    "color_dodge", "color_burn", "hard_light", "soft_light", "difference",
                    "exclusion", "hue", "saturation", "color", "luminosity", "add", "subtract",
                    "linear_burn", "linear_dodge", "vivid_light", "linear_light", "pin_light",
                    "hard_mix", "divide",
                ],
            )
        })
    }
}
