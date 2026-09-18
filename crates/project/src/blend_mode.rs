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
}

impl BlendMode {
    /// All 19 compositing blend modes in standard display order.
    pub const ALL: [Self; 19] = [
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
            _ => None,
        }
    }
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
                ],
            )
        })
    }
}
