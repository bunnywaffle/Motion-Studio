use crate::color::Color;
use crate::property::Property;
use serde::{Deserialize, Serialize};

const fn default_true() -> bool {
    true
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
        map_black: Color,
        map_white: Color,
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
        color: Color,
    },
}

impl EffectType {
    /// Return the canonical display name of the effect type.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::BrightnessContrast { .. } => "Brightness & Contrast",
            Self::Tint { .. } => "Tint",
            Self::Invert { .. } => "Invert",
            Self::DropShadow { .. } => "Drop Shadow",
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
            map_black,
            map_white,
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
            color,
        }
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

    /// Return the canonical type name of this effect.
    pub const fn type_name(&self) -> &'static str {
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
        }
        false
    }
}
