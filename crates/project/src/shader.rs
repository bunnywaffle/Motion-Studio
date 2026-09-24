use crate::color::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The UI-facing data type of a shader uniform.
///
/// Mapped from GLSL declaration types (`float`, `int`, `bool`, `vec2/3/4`)
/// with optional `// @param` metadata overrides (`color`, `angle`, `enum`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShaderParamType {
    Float,
    Int,
    Bool,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Angle,
    Enum { options: Vec<String> },
}

impl ShaderParamType {
    /// GLSL declaration keyword for this type (`bool` arrives as `i32`
    /// on the GPU; see [`ShaderParamValue::as_float`]).
    pub const fn glsl_keyword(&self) -> &'static str {
        match self {
            Self::Float | Self::Angle => "float",
            Self::Int | Self::Bool | Self::Enum { .. } => "int",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Vec4 | Self::Color => "vec4",
        }
    }

    /// Component count for vector-like types (scalars count as 1).
    pub const fn components(&self) -> usize {
        match self {
            Self::Float | Self::Int | Self::Bool | Self::Angle | Self::Enum { .. } => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 | Self::Color => 4,
        }
    }

    /// Declared internal widget for this type (the property states its UI;
    /// the application auto-creates it — no per-param UI code).
    pub const fn widget_kind(&self) -> crate::widget::WidgetKind {
        use crate::widget::WidgetKind;
        match self {
            Self::Float => WidgetKind::Slider,
            Self::Int => WidgetKind::Integer,
            Self::Bool => WidgetKind::Checkbox,
            Self::Vec2 => WidgetKind::Vec2,
            Self::Vec3 => WidgetKind::Vec3,
            Self::Vec4 => WidgetKind::Vec4,
            Self::Color => WidgetKind::Color,
            Self::Angle => WidgetKind::Angle,
            Self::Enum { .. } => WidgetKind::Dropdown,
        }
    }
}

/// A runtime value for a [`ShaderParam`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShaderParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Color(Color),
}

impl ShaderParamValue {
    /// Flatten to f32 components for uniform buffer upload.
    pub fn as_floats(&self) -> Vec<f32> {
        match self {
            Self::Float(v) => vec![*v],
            Self::Int(v) => vec![*v as f32],
            Self::Bool(v) => vec![if *v { 1.0 } else { 0.0 }],
            Self::Vec2(v) => v.to_vec(),
            Self::Vec3(v) => v.to_vec(),
            Self::Vec4(v) => v.to_vec(),
            Self::Color(c) => vec![c.r, c.g, c.b, c.a],
        }
    }

    /// Human-readable single-line rendering for effect rows.
    pub fn display(&self) -> String {
        match self {
            Self::Float(v) => format!("{v:.3}"),
            Self::Int(v) => format!("{v}"),
            Self::Bool(v) => (if *v { "on" } else { "off" }).to_string(),
            Self::Vec2(v) => format!("{:.2}, {:.2}", v[0], v[1]),
            Self::Vec3(v) => format!("{:.2}, {:.2}, {:.2}", v[0], v[1], v[2]),
            Self::Vec4(v) => format!("{:.2}, {:.2}, {:.2}, {:.2}", v[0], v[1], v[2], v[3]),
            Self::Color(c) => format!(
                "#{:02X}{:02X}{:02X}",
                (c.r.clamp(0.0, 1.0) * 255.0) as u8,
                (c.g.clamp(0.0, 1.0) * 255.0) as u8,
                (c.b.clamp(0.0, 1.0) * 255.0) as u8
            ),
        }
    }
}

/// A single auto-detected shader parameter (one `uniform` + metadata).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderParam {
    pub name: String,
    pub label: String,
    pub param_type: ShaderParamType,
    pub default: ShaderParamValue,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub step: Option<f32>,
    pub group: Option<String>,
}

impl ShaderParam {
    /// Resolve the live value: override from `values` or the default.
    pub fn resolve<'a>(
        &'a self,
        values: &'a HashMap<String, ShaderParamValue>,
    ) -> std::borrow::Cow<'a, ShaderParamValue> {
        match values.get(&self.name) {
            Some(v) => std::borrow::Cow::Borrowed(v),
            None => std::borrow::Cow::Owned(self.default.clone()),
        }
    }

    /// Clamp / round a raw float edit into a value of this param's type.
    pub fn coerce_float(&self, v: f32) -> ShaderParamValue {
        let v = v.clamp(self.min.unwrap_or(f32::NEG_INFINITY), self.max.unwrap_or(f32::INFINITY));
        match &self.param_type {
            ShaderParamType::Float | ShaderParamType::Angle => ShaderParamValue::Float(v),
            ShaderParamType::Int => ShaderParamValue::Int(v.round() as i32),
            ShaderParamType::Bool => ShaderParamValue::Bool(v >= 0.5),
            ShaderParamType::Enum { options } => {
                let max_idx = options.len().saturating_sub(1).max(0) as f32;
                ShaderParamValue::Int(v.round().clamp(0.0, max_idx) as i32)
            }
            ShaderParamType::Vec2 => ShaderParamValue::Vec2([v, v]),
            ShaderParamType::Vec3 => ShaderParamValue::Vec3([v, v, v]),
            ShaderParamType::Vec4 => ShaderParamValue::Vec4([v, v, v, 1.0]),
            ShaderParamType::Color => ShaderParamValue::Color(Color::rgba(
                v.clamp(0.0, 1.0),
                v.clamp(0.0, 1.0),
                v.clamp(0.0, 1.0),
                1.0,
            )),
        }
    }
}

/// Parse `// @param ...` metadata: `key=value` pairs separated by spaces.
/// Quoted values may contain spaces (`name="My Brightness"`).
fn parse_param_metadata(comment: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = comment.trim();
    // Strip leading `// @param`.
    if let Some(stripped) = rest.strip_prefix("//") {
        rest = stripped.trim();
    }
    if let Some(stripped) = rest.strip_prefix("@param") {
        rest = stripped.trim();
    } else {
        return out;
    }
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            break;
        }
        let key = rest[key_start..i].trim().to_string();
        i += 1; // '='
        let value = if i < bytes.len() && bytes[i] == b'"' {
            i += 1;
            let val_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let v = rest[val_start..i].to_string();
            i += 1; // closing quote
            v
        } else {
            let val_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            rest[val_start..i].to_string()
        };
        if !key.is_empty() && !value.is_empty() {
            out.insert(key, value);
        }
    }
    out
}

fn parse_float_list(s: &str) -> Vec<f32> {
    s.split(|c| c == ',' || c == ' ' || c == '\t')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f32>().ok())
        .collect()
}

fn default_label(name: &str) -> String {
    let mut label = String::new();
    for (i, ch) in name.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() {
            label.push(' ');
        }
        if i == 0 {
            label.extend(ch.to_uppercase());
        } else {
            label.push(ch);
        }
    }
    label.replace('_', " ")
}

/// Parse one `uniform <type> <name> [= default];` declaration line into
/// `(glsl_type, name, default_literal)`. Returns `None` for anything else
/// (reserved built-ins like `time`/`resolution` are skipped by the caller).
fn parse_uniform_decl(line: &str) -> Option<(String, String, Option<String>)> {
    let line = line.trim().trim_end_matches(';').trim();
    let rest = line.strip_prefix("uniform")?;
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    // Split `type name [= default]`.
    let mut parts = rest.splitn(2, char::is_whitespace);
    let ty = parts.next()?.trim().to_string();
    let rest = parts.next()?.trim();
    if ty.is_empty() || rest.is_empty() {
        return None;
    }
    // Precision qualifiers (`highp float x`) — fold qualifier into nothing.
    let (ty, rest) = match ty.as_str() {
        "highp" | "mediump" | "lowp" => {
            let mut p2 = rest.splitn(2, char::is_whitespace);
            (p2.next()?.trim().to_string(), p2.next().unwrap_or("").trim())
        }
        _ => (ty, rest),
    };
    if rest.is_empty() {
        return None;
    }
    let (name, default) = match rest.split_once('=') {
        Some((n, d)) => (n.trim().to_string(), Some(d.trim().to_string())),
        None => (rest.split_whitespace().next().unwrap_or("").to_string(), None),
    };
    // Reject array uniforms and empty names.
    if name.is_empty() || name.contains('[') || ty.contains('[') {
        return None;
    }
    Some((ty, name, default))
}

/// Uniforms provided by the runtime wrapper (not user parameters).
pub const RESERVED_UNIFORMS: &[&str] = &["time", "resolution", "frame", "duration", "uv", "color"];

fn default_value_for(ty: &ShaderParamType, literal: Option<&str>) -> ShaderParamValue {
    if let Some(lit) = literal {
        let lit = lit.trim();
        match ty {
            ShaderParamType::Float | ShaderParamType::Angle => {
                if let Ok(v) = lit.parse::<f32>() {
                    return ShaderParamValue::Float(v);
                }
            }
            ShaderParamType::Int => {
                if let Ok(v) = lit.parse::<i32>() {
                    return ShaderParamValue::Int(v);
                }
            }
            ShaderParamType::Enum { .. } => {
                if let Ok(v) = lit.parse::<i32>() {
                    return ShaderParamValue::Int(v.max(0));
                }
            }
            ShaderParamType::Bool => {
                if lit.eq_ignore_ascii_case("true") {
                    return ShaderParamValue::Bool(true);
                }
                if lit.eq_ignore_ascii_case("false") {
                    return ShaderParamValue::Bool(false);
                }
            }
            ShaderParamType::Vec2 | ShaderParamType::Vec3 | ShaderParamType::Vec4 | ShaderParamType::Color => {
                // `vec3(1.0, 0.5, 0.0)` or `1.0, 0.5, 0.0`.
                let inner = lit
                    .trim_start_matches(|c: char| c.is_alphabetic() || c == '<')
                    .trim_start_matches('>')
                    .trim()
                    .trim_start_matches('(')
                    .trim_end_matches(')');
                let nums = parse_float_list(inner);
                match ty {
                    ShaderParamType::Vec2 if nums.len() >= 2 => {
                        return ShaderParamValue::Vec2([nums[0], nums[1]]);
                    }
                    ShaderParamType::Vec3 if nums.len() >= 3 => {
                        return ShaderParamValue::Vec3([nums[0], nums[1], nums[2]]);
                    }
                    ShaderParamType::Vec4 if nums.len() >= 4 => {
                        return ShaderParamValue::Vec4([nums[0], nums[1], nums[2], nums[3]]);
                    }
                    ShaderParamType::Color if nums.len() >= 3 => {
                        return ShaderParamValue::Color(Color::rgba(
                            nums[0],
                            nums[1],
                            nums[2],
                            *nums.get(3).unwrap_or(&1.0),
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
    match ty {
        ShaderParamType::Float | ShaderParamType::Angle => ShaderParamValue::Float(0.0),
        ShaderParamType::Int => ShaderParamValue::Int(0),
        ShaderParamType::Bool => ShaderParamValue::Bool(false),
        ShaderParamType::Vec2 => ShaderParamValue::Vec2([0.0, 0.0]),
        ShaderParamType::Vec3 => ShaderParamValue::Vec3([0.0, 0.0, 0.0]),
        ShaderParamType::Vec4 => ShaderParamValue::Vec4([0.0, 0.0, 0.0, 1.0]),
        ShaderParamType::Color => ShaderParamValue::Color(Color::WHITE),
        ShaderParamType::Enum { .. } => ShaderParamValue::Int(0),
    }
}

/// Parse all user uniforms in `source` into ordered [`ShaderParam`]s.
///
/// Only `// @param`-adjacent and bare `uniform` declarations become UI;
/// reserved runtime names (`time`, `resolution`, `frame`, `duration`) are
/// skipped. Adding or removing a uniform automatically changes the UI.
pub fn parse_shader_params(source: &str) -> Vec<ShaderParam> {
    let mut out = Vec::new();
    let mut pending_meta: HashMap<String, String> = HashMap::new();
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.starts_with("//") {
            if line.contains("@param") {
                pending_meta = parse_param_metadata(line);
            }
            continue;
        }
        let Some((glsl_ty, name, default_lit)) = parse_uniform_decl(line) else {
            // Any non-uniform, non-comment line resets pending metadata only
            // when it is actual code (blank lines keep it).
            if !line.is_empty() && !line.starts_with('#') {
                pending_meta.clear();
            }
            continue;
        };
        if RESERVED_UNIFORMS.iter().any(|r| r.eq_ignore_ascii_case(&name)) {
            pending_meta.clear();
            continue;
        }
        let meta = std::mem::take(&mut pending_meta);
        let ty = match meta.get("type").map(|s| s.to_lowercase()).as_deref() {
            Some("color") => ShaderParamType::Color,
            Some("angle") => ShaderParamType::Angle,
            Some("enum") => ShaderParamType::Enum {
                options: meta
                    .get("options")
                    .map(|s| {
                        s.split(',')
                            .map(|o| o.trim().trim_matches('"').to_string())
                            .filter(|o| !o.is_empty())
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            Some("bool") => ShaderParamType::Bool,
            Some("int") => ShaderParamType::Int,
            Some("float") => ShaderParamType::Float,
            Some("vec2") => ShaderParamType::Vec2,
            Some("vec3") => ShaderParamType::Vec3,
            Some("vec4") => ShaderParamType::Vec4,
            _ => match glsl_ty.as_str() {
                "float" => {
                    if name.to_lowercase().contains("color") || name.to_lowercase().contains("tint") {
                        ShaderParamType::Color
                    } else {
                        ShaderParamType::Float
                    }
                }
                "int" => ShaderParamType::Int,
                "bool" => ShaderParamType::Bool,
                "vec2" => ShaderParamType::Vec2,
                "vec3" => ShaderParamType::Vec3,
                "vec4" => {
                    if name.to_lowercase().contains("color") || name.to_lowercase().contains("tint") {
                        ShaderParamType::Color
                    } else {
                        ShaderParamType::Vec4
                    }
                }
                _ => continue, // sampler2D, mat4, ... are not UI parameters.
            },
        };
        // A `float` declared as color without a vec default still gets a
        // white default; coerce happens at value resolution.
        let mut param = ShaderParam {
            label: meta
                .get("name")
                .or_else(|| meta.get("label"))
                .cloned()
                .unwrap_or_else(|| default_label(&name)),
            name: name.clone(),
            min: meta.get("min").and_then(|s| s.parse::<f32>().ok()),
            max: meta.get("max").and_then(|s| s.parse::<f32>().ok()),
            step: meta.get("step").and_then(|s| s.parse::<f32>().ok()),
            group: meta.get("group").cloned(),
            default: default_value_for(&ty, default_lit.as_deref()),
            param_type: ty,
        };
        // Fill a color default from a scalar literal gracefully.
        if matches!(param.param_type, ShaderParamType::Color)
            && matches!(param.default, ShaderParamValue::Float(_))
        {
            param.default = ShaderParamValue::Color(Color::WHITE);
        }
        out.push(param);
    }
    out
}

/// Optional `// @meta name="..."` document header (portable effect files).
pub fn parse_shader_meta(source: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.starts_with("//") && line.contains("@meta") {
            let mut meta = parse_param_metadata(&line.replace("@meta", "@param"));
            for (k, v) in meta.drain() {
                out.insert(k, v);
            }
        }
    }
    out
}

/// Serialize params back into `// @param` lines (used by Export).
pub fn params_to_metadata(params: &[ShaderParam]) -> String {
    let mut s = String::new();
    for p in params {
        s.push_str(&format!("// @param name=\"{}\"", p.label));
        match &p.param_type {
            ShaderParamType::Float => s.push_str(" type=float"),
            ShaderParamType::Int => s.push_str(" type=int"),
            ShaderParamType::Bool => s.push_str(" type=bool"),
            ShaderParamType::Vec2 => s.push_str(" type=vec2"),
            ShaderParamType::Vec3 => s.push_str(" type=vec3"),
            ShaderParamType::Vec4 => s.push_str(" type=vec4"),
            ShaderParamType::Color => s.push_str(" type=color"),
            ShaderParamType::Angle => s.push_str(" type=angle"),
            ShaderParamType::Enum { options } => {
                s.push_str(&format!(" type=enum options=\"{}\"", options.join(",")));
            }
        }
        if let Some(min) = p.min {
            s.push_str(&format!(" min={min}"));
        }
        if let Some(max) = p.max {
            s.push_str(&format!(" max={max}"));
        }
        if let Some(step) = p.step {
            s.push_str(&format!(" step={step}"));
        }
        if let Some(group) = &p.group {
            s.push_str(&format!(" group=\"{group}\""));
        }
        s.push('\n');
    }
    s
}

/// Built-in presets demonstrating the simple shader model:
/// `vec4 effect(vec2 uv, vec4 color)` with auto-detected uniforms.
pub mod presets {
    /// Brightness / contrast / saturation grade.
    pub const GRADE: &str = r#"// @meta name="Grade"
// @param name="Brightness" type=float min=-2 max=2 step=0.01
uniform float brightness = 0.0;
// @param name="Contrast" type=float min=-2 max=2 step=0.01
uniform float contrast = 0.0;
// @param name="Saturation" type=float min=0 max=2 step=0.01
uniform float saturation = 1.0;

vec4 effect(vec2 uv, vec4 color) {
    vec3 c = color.rgb + vec3(brightness);
    c = (c - vec3(0.5)) * (contrast + 1.0) + vec3(0.5);
    float lum = c.r * 0.299 + c.g * 0.587 + c.b * 0.114;
    c = mix(vec3(lum), c, saturation);
    return vec4(clamp(c, vec3(0.0), vec3(1.0)), color.a);
}
"#;

    /// Vignette with color tint.
    pub const VIGNETTE: &str = r#"// @meta name="Vignette"
// @param name="Strength" type=float min=0 max=2 step=0.01
uniform float strength = 0.8;
// @param name="Tint" type=color
uniform vec4 tint = vec4(0.0, 0.0, 0.0, 1.0);

vec4 effect(vec2 uv, vec4 color) {
    vec2 d = uv - vec2(0.5);
    float v = 1.0 - smoothstep(0.2, 0.8, length(d) * (strength + 0.001) * 2.0);
    vec3 c = mix(tint.rgb, color.rgb, v);
    return vec4(c, color.a);
}
"#;

    /// CRT scanlines with RGB shift.
    pub const SCANLINES: &str = r#"// @meta name="Scanlines"
// @param name="Density" type=float min=10 max=800 step=1
uniform float density = 240.0;
// @param name="Amount" type=float min=0 max=1 step=0.01
uniform float amount = 0.35;
// @param name="Flicker" type=bool
uniform bool flicker = false;

vec4 effect(vec2 uv, vec4 color) {
    float phase = time * float(flicker) * 8.0;
    float line = sin(uv.y * density * 6.2831 + phase) * 0.5 + 0.5;
    vec3 c = color.rgb * (1.0 - amount * (1.0 - line));
    return vec4(c, color.a);
}
"#;

    /// Duotone tint mapper.
    pub const DUOTONE: &str = r#"// @meta name="Duotone"
// @param name="Shadows" type=color
uniform vec4 shadows = vec4(0.05, 0.1, 0.3, 1.0);
// @param name="Highlights" type=color
uniform vec4 highlights = vec4(0.95, 0.8, 0.4, 1.0);
// @param name="Mix" type=float min=0 max=1 step=0.01 group="Balance"
uniform float mixAmount = 1.0;

vec4 effect(vec2 uv, vec4 color) {
    float lum = color.r * 0.299 + color.g * 0.587 + color.b * 0.114;
    vec3 duo = mix(shadows.rgb, highlights.rgb, lum);
    vec3 c = mix(color.rgb, duo, mixAmount);
    return vec4(c, color.a);
}
"#;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_uniforms_with_metadata() {
        let src = presets::GRADE;
        let params = parse_shader_params(src);
        assert_eq!(params.len(), 3);
        assert_eq!(params[0].name, "brightness");
        assert_eq!(params[0].label, "Brightness");
        assert_eq!(params[0].param_type, ShaderParamType::Float);
        assert_eq!(params[0].min, Some(-2.0));
        assert_eq!(params[0].max, Some(2.0));
        assert_eq!(params[2].default, ShaderParamValue::Float(1.0));
    }

    #[test]
    fn test_parse_color_bool_and_reserved() {
        let src = "uniform float time;\nuniform vec2 resolution;\n// @param name=\"Tint\" type=color\nuniform vec4 tint;\n// @param name=\"On\" type=bool\nuniform bool enabled = true;\n";
        let params = parse_shader_params(src);
        // time/resolution are runtime-provided, not UI params.
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_type, ShaderParamType::Color);
        assert_eq!(params[1].param_type, ShaderParamType::Bool);
        assert_eq!(params[1].default, ShaderParamValue::Bool(true));
    }

    #[test]
    fn test_parse_enum_and_groups() {
        let src = "// @param name=\"Mode\" type=enum options=\"Soft,Hard,Glow\" group=\"Style\"\nuniform int mode = 1;\n";
        let params = parse_shader_params(src);
        assert_eq!(params.len(), 1);
        match &params[0].param_type {
            ShaderParamType::Enum { options } => {
                assert_eq!(options, &vec!["Soft".to_string(), "Hard".to_string(), "Glow".to_string()]);
            }
            other => panic!("expected enum, got {other:?}"),
        }
        assert_eq!(params[0].group.as_deref(), Some("Style"));
        assert_eq!(params[0].default, ShaderParamValue::Int(1));
    }

    #[test]
    fn test_add_remove_uniform_changes_ui() {
        let a = "uniform float brightness = 0.0;\n";
        let b = "uniform float brightness = 0.0;\nuniform float contrast = 0.0;\n";
        assert_eq!(parse_shader_params(a).len(), 1);
        assert_eq!(parse_shader_params(b).len(), 2);
    }

    #[test]
    fn test_meta_and_value_display() {
        let meta = parse_shader_meta(presets::VIGNETTE);
        assert_eq!(meta.get("name").map(String::as_str), Some("Vignette"));
        assert_eq!(ShaderParamValue::Float(1.5).display(), "1.500");
        assert_eq!(
            ShaderParamValue::Color(Color::WHITE).display(),
            "#FFFFFF"
        );
    }

    #[test]
    fn test_coerce_float_rounds_int_and_bool() {
        let int_p = ShaderParam {
            name: "n".into(),
            label: "N".into(),
            param_type: ShaderParamType::Int,
            default: ShaderParamValue::Int(0),
            min: None,
            max: None,
            step: None,
            group: None,
        };
        assert_eq!(int_p.coerce_float(2.6), ShaderParamValue::Int(3));
        let bool_p = ShaderParam {
            param_type: ShaderParamType::Bool,
            ..int_p.clone()
        };
        assert_eq!(bool_p.coerce_float(0.2), ShaderParamValue::Bool(false));
    }
}
