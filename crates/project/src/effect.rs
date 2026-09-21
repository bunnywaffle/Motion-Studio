use crate::color::Color;
use crate::property::Property;
use crate::shader::{parse_shader_params, ShaderParam, ShaderParamValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
            Self::ShaderLab { .. } => "Shader Lab",
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
            // Shader Lab values are dynamic, not `Property<f32>` tracks.
            EffectType::ShaderLab { .. } => None,
        }
    }
}
