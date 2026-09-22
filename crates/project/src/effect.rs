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
    LumaKey {
        threshold: Property<f32>,
        feather: Property<f32>,
    },
    NoiseGenerator {
        amount: Property<f32>,
        monochrome: bool,
    },
    /// Procedural checkerboard generator (spatial: needs pixel position,
    /// so [`crate::Effect::nudge_param`] handles scalars while rasterizers
    /// and the viewport SVG preview resolve the pattern).
    Checkerboard {
        size: Property<f32>,
        color_a: Color,
        color_b: Color,
    },
    /// Two-color linear gradient generator (spatial).
    GradientRamp {
        color_a: Color,
        color_b: Color,
        angle: Property<f32>,
    },
    /// Fake-3D skew filter in degrees (spatial).
    Perspective {
        skew_x: Property<f32>,
        skew_y: Property<f32>,
    },
    /// Text stroke outline (resolved by text renderers / viewport SVG).
    TextOutline {
        width: Property<f32>,
        color: Color,
    },
    /// Text bevel lighting (resolved by text renderers).
    TextBevel {
        strength: Property<f32>,
        softness: Property<f32>,
    },
    /// Highlight bloom lift (per-pixel approximation; radius is spatial).
    Bloom {
        intensity: Property<f32>,
        radius: Property<f32>,
    },
    /// Mosaic tiler (spatial).
    Tiler {
        tiles_x: Property<f32>,
        tiles_y: Property<f32>,
    },
    /// Turbulent warp distortion (spatial).
    Warp {
        amount: Property<f32>,
        scale: Property<f32>,
    },
    /// Exposure in EV stops (per-pixel gain).
    Exposure {
        exposure: Property<f32>,
    },
    /// Vibrance: saturation weighted toward muted colors (per-pixel).
    Vibrance {
        vibrance: Property<f32>,
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
            Self::LumaKey { .. } => "Luma Key",
            Self::NoiseGenerator { .. } => "Noise Generator",
            Self::Checkerboard { .. } => "Checkerboard",
            Self::GradientRamp { .. } => "Gradient Ramp",
            Self::Perspective { .. } => "Perspective",
            Self::TextOutline { .. } => "Text Outline",
            Self::TextBevel { .. } => "Text Bevel",
            Self::Bloom { .. } => "Bloom",
            Self::Tiler { .. } => "Tiler",
            Self::Warp { .. } => "Warp",
            Self::Exposure { .. } => "Exposure",
            Self::Vibrance { .. } => "Vibrance",
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

    /// Construct a Luma Key effect type (keys out dark / bright pixels by
    /// luminance instead of hue).
    pub fn luma_key(threshold: f32, feather: f32) -> Self {
        Self::LumaKey {
            threshold: Property::new("Threshold", threshold.clamp(0.0, 100.0)),
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

    /// Construct a Checkerboard generator effect type.
    pub fn checkerboard(size: f32, color_a: Color, color_b: Color) -> Self {
        Self::Checkerboard {
            size: Property::new("Size", size.clamp(2.0, 512.0)),
            color_a,
            color_b,
        }
    }

    /// Construct a Gradient Ramp generator effect type.
    pub fn gradient_ramp(color_a: Color, color_b: Color, angle: f32) -> Self {
        Self::GradientRamp {
            color_a,
            color_b,
            angle: Property::new("Angle", angle),
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
            color,
        }
    }

    /// Construct a Text Bevel effect type.
    pub fn text_bevel(strength: f32, softness: f32) -> Self {
        Self::TextBevel {
            strength: Property::new("Strength", strength.clamp(0.0, 100.0)),
            softness: Property::new("Softness", softness.clamp(0.0, 100.0)),
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
        }
    }

    /// Construct a Warp effect type.
    pub fn warp(amount: f32, scale: f32) -> Self {
        Self::Warp {
            amount: Property::new("Amount", amount.clamp(0.0, 100.0)),
            scale: Property::new("Scale", scale.clamp(0.1, 10.0)),
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
    pub fn set_color_value(&mut self, field: &str, next: Color) -> bool {
        match &mut self.effect_type {
            EffectType::Checkerboard { color_a, color_b, .. } => {
                if field.eq_ignore_ascii_case("color_a") || field.eq_ignore_ascii_case("a") {
                    *color_a = next;
                    true
                } else if field.eq_ignore_ascii_case("color_b") || field.eq_ignore_ascii_case("b") {
                    *color_b = next;
                    true
                } else {
                    false
                }
            }
            EffectType::GradientRamp { color_a, color_b, .. } => {
                if field.eq_ignore_ascii_case("color_a") || field.eq_ignore_ascii_case("a") {
                    *color_a = next;
                    true
                } else if field.eq_ignore_ascii_case("color_b") || field.eq_ignore_ascii_case("b") {
                    *color_b = next;
                    true
                } else {
                    false
                }
            }
            EffectType::TextOutline { color, .. } => {
                if field.eq_ignore_ascii_case("color") {
                    *color = next;
                    true
                } else {
                    false
                }
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
            EffectType::LumaKey { threshold, feather } => {
                if param_name.eq_ignore_ascii_case("threshold") {
                    threshold.set_value((threshold.value + delta).clamp(0.0, 100.0));
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
            EffectType::Bloom { intensity, radius } => {
                if param_name.eq_ignore_ascii_case("intensity") {
                    intensity.set_value((intensity.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("radius") {
                    radius.set_value((radius.value + delta).clamp(0.0, 100.0));
                    return true;
                }
            }
            EffectType::Tiler { tiles_x, tiles_y } => {
                if param_name.eq_ignore_ascii_case("tiles_x") || param_name.eq_ignore_ascii_case("x") {
                    tiles_x.set_value((tiles_x.value + delta).clamp(1.0, 32.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("tiles_y") || param_name.eq_ignore_ascii_case("y") {
                    tiles_y.set_value((tiles_y.value + delta).clamp(1.0, 32.0));
                    return true;
                }
            }
            EffectType::Warp { amount, scale } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    amount.set_value((amount.value + delta).clamp(0.0, 100.0));
                    return true;
                } else if param_name.eq_ignore_ascii_case("scale") {
                    scale.set_value((scale.value + delta).clamp(0.1, 10.0));
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
            EffectType::LumaKey { threshold, feather } => {
                if param_name.eq_ignore_ascii_case("threshold") {
                    Some(threshold)
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
            EffectType::Bloom { intensity, radius } => {
                if param_name.eq_ignore_ascii_case("intensity") {
                    Some(intensity)
                } else if param_name.eq_ignore_ascii_case("radius") {
                    Some(radius)
                } else {
                    None
                }
            }
            EffectType::Tiler { tiles_x, tiles_y } => {
                if param_name.eq_ignore_ascii_case("tiles_x") || param_name.eq_ignore_ascii_case("x") {
                    Some(tiles_x)
                } else if param_name.eq_ignore_ascii_case("tiles_y") || param_name.eq_ignore_ascii_case("y") {
                    Some(tiles_y)
                } else {
                    None
                }
            }
            EffectType::Warp { amount, scale } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else if param_name.eq_ignore_ascii_case("scale") {
                    Some(scale)
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
            EffectType::LumaKey { threshold, feather } => {
                if param_name.eq_ignore_ascii_case("threshold") {
                    Some(threshold)
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
            EffectType::Bloom { intensity, radius } => {
                if param_name.eq_ignore_ascii_case("intensity") {
                    Some(intensity)
                } else if param_name.eq_ignore_ascii_case("radius") {
                    Some(radius)
                } else {
                    None
                }
            }
            EffectType::Tiler { tiles_x, tiles_y } => {
                if param_name.eq_ignore_ascii_case("tiles_x") || param_name.eq_ignore_ascii_case("x") {
                    Some(tiles_x)
                } else if param_name.eq_ignore_ascii_case("tiles_y") || param_name.eq_ignore_ascii_case("y") {
                    Some(tiles_y)
                } else {
                    None
                }
            }
            EffectType::Warp { amount, scale } => {
                if param_name.eq_ignore_ascii_case("amount") {
                    Some(amount)
                } else if param_name.eq_ignore_ascii_case("scale") {
                    Some(scale)
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
            // Shader Lab values are dynamic, not `Property<f32>` tracks.
            EffectType::ShaderLab { .. } => None,
        }
    }
}
