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
    GlslShader {
        code: String,
        param1: Property<f32>,
        param2: Property<f32>,
        param3: Property<f32>,
        param4: Property<f32>,
    },
    DisplacementMap {
        max_horizontal: Property<f32>,
        max_vertical: Property<f32>,
    },
    ChromaKey {
        key_color: Color,
        tolerance: Property<f32>,
        feather: Property<f32>,
    },
    NoiseGenerator {
        amount: Property<f32>,
        monochrome: bool,
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
            Self::GlslShader { .. } => "Custom GLSL Shader",
            Self::DisplacementMap { .. } => "Displacement Map",
            Self::ChromaKey { .. } => "Chroma Key",
            Self::NoiseGenerator { .. } => "Noise Generator",
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
            key_color,
            tolerance: Property::new("Tolerance", tolerance.clamp(0.0, 100.0)),
            feather: Property::new("Feather", feather.clamp(0.0, 100.0)),
        }
    }

    /// Construct a Noise Generator effect type.
    pub fn noise_generator(amount: f32, monochrome: bool) -> Self {
        Self::NoiseGenerator {
            amount: Property::new("Amount", amount.clamp(0.0, 100.0)),
            monochrome,
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

    /// Factory for creating a Noise Generator effect.
    pub fn noise_generator(id: impl Into<String>, amount: f32, monochrome: bool) -> Self {
        Self::new(id, "Noise Generator", EffectType::noise_generator(amount, monochrome))
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
            EffectType::NoiseGenerator { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    amount.set_value((amount.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
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
            EffectType::NoiseGenerator { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else {
                    None
                }
            }
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
            EffectType::NoiseGenerator { amount, .. } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else {
                    None
                }
            }
        }
    }
}
