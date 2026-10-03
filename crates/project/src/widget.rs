//! Declarative property → widget system.
//!
//! A property declares **what kind of UI it wants**; the application
//! automatically creates the appropriate internal widget. No widget is
//! hardcoded into any individual effect:
//!
//! ```text
//! Property
//!  ├── Type (float, color, bool, enum, vec, gradient, curve, …)
//!  ├── Value
//!  ├── Metadata (min, max, step, unit, enum values, widget override)
//!  └── Widget  →  automatic UI
//! ```
//!
//! Mapping (matches the core internal widget library):
//!
//! | Property              | Widget          |
//! |-----------------------|-----------------|
//! | float                 | Number Slider   |
//! | percentage            | Percentage      |
//! | angle                 | Angle Dial      |
//! | integer               | Integer input   |
//! | Color                 | Color Picker    |
//! | gradient (2+ stops)   | Gradient Editor |
//! | curve                 | Curve Editor    |
//! | path                  | Path Editor     |
//! | boolean               | Checkbox        |
//! | enum                  | Dropdown        |
//! | vec2 / vec3 / vec4    | Vector control  |
//!
//! [`EffectType::declarations`] and [`crate::shader::ShaderParam::declaration`]
//! are the declaration sites: every effect parameter states its widget + range
//! once, and every UI surface (Properties panel, timeline rows, …) renders
//! from the same data through `application::widgets`.

use crate::color::Color;
#[cfg(test)]
use crate::effect::{TileCell, TileMode};
use serde::{Deserialize, Serialize};

/// Internal widget kinds the application can auto-create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    /// Drag-scrub + typed entry row (floats).
    #[default]
    Slider,
    /// Plain numeric field.
    Number,
    /// Integer stepper field.
    Integer,
    /// 0..100 percent row.
    Percentage,
    /// Degree dial row (wraps at 360).
    Angle,
    /// Timecode row.
    Time,
    /// Boolean checkbox.
    Checkbox,
    /// Boolean on/off toggle.
    Toggle,
    /// Enum dropdown (expanding option list).
    Dropdown,
    /// Radio button group (small enum sets).
    Radio,
    /// Searchable dropdown (long enum sets).
    SearchableDropdown,
    /// Linked/unlinked 2-component vector.
    Vec2,
    /// Linked/unlinked 3-component vector.
    Vec3,
    /// Linked/unlinked 4-component vector.
    Vec4,
    /// 2D canvas pad.
    XYPad,
    /// Swatches + hex + wheel.
    Color,
    /// Bare color wheel.
    ColorWheel,
    /// Multi-stop gradient bar editor.
    Gradient,
    /// Bezier animation-curve editor.
    Curve,
    /// Vector path editor.
    Path,
    /// Single-line text.
    Text,
}

/// Range / step / display metadata for one declared parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamMeta {
    pub min: f32,
    pub max: f32,
    /// -/+ stepper delta.
    pub step: f32,
    /// Scrub divisor code (`fx:{eff}:{param}:{mult100}`): dx pixels are
    /// divided by `100/mult100`… i.e. stored mult100/100.
    pub mult100: f32,
    /// Decimals in the readout.
    pub decimals: u8,
    /// Unit suffix (`"px"`, `"%"`, `"°"`, `"EV"`, …). A leading space is
    /// significant (`" %"` vs `"%"`) and preserved verbatim.
    pub unit: String,
    /// Show an explicit `+` for non-negative values (`"{:+.2} EV"`).
    pub signed: bool,
    /// Enum options (Dropdown / Radio / SearchableDropdown).
    pub options: Vec<String>,
}

impl ParamMeta {
    pub fn slider(min: f32, max: f32, step: f32, decimals: u8, unit: &str, mult100: f32) -> Self {
        Self {
            min,
            max,
            step,
            mult100,
            decimals,
            unit: unit.to_string(),
            signed: false,
            options: Vec::new(),
        }
    }

    /// Format a value exactly like the legacy effect rows did.
    pub fn display(&self, value: f32) -> String {
        let body = match self.decimals {
            0 => {
                if self.signed {
                    format!("{value:+.0}")
                } else {
                    format!("{value:.0}")
                }
            }
            1 => {
                if self.signed {
                    format!("{value:+.1}")
                } else {
                    format!("{value:.1}")
                }
            }
            _ => {
                if self.signed {
                    format!("{value:+.2}")
                } else {
                    format!("{value:.2}")
                }
            }
        };
        format!("{body}{}", self.unit)
    }
}

/// One declared parameter: identity + widget + metadata + live value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropDecl {
    /// Effect-local param id (`"radius"`, `"color_a"`, …).
    pub field: String,
    /// Human label (`"Radius"`, `"Color A"`, …).
    pub label: String,
    pub widget: WidgetKind,
    pub meta: ParamMeta,
    pub value: PropValue,
}

/// Live value carried by a declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropValue {
    Float { value: f32, animated: bool },
    Int { value: i32 },
    Bool { value: bool, animated: bool },
    Color { value: Color, animated: bool },
    Text(String),
    EnumSel { index: usize },
}

impl PropDecl {
    /// True for animatable scalar rows (the panel's scalar loop skips
    /// colors/bools, which custom arms lay out explicitly).
    pub fn is_scalar(&self) -> bool {
        matches!(self.value, PropValue::Float { .. })
    }

    pub fn scalar(
        field: &str,
        label: &str,
        widget: WidgetKind,
        meta: ParamMeta,
        value: f32,
        animated: bool,
    ) -> Self {
        Self {
            field: field.to_string(),
            label: label.to_string(),
            widget,
            meta,
            value: PropValue::Float { value, animated },
        }
    }

    pub fn color(field: &str, label: &str, value: Color, animated: bool) -> Self {
        Self {
            field: field.to_string(),
            label: label.to_string(),
            widget: WidgetKind::Color,
            meta: ParamMeta::slider(0.0, 1.0, 0.01, 2, "", 100.0),
            value: PropValue::Color { value, animated },
        }
    }

    pub fn boolean(field: &str, label: &str, value: bool, animated: bool) -> Self {
        Self {
            field: field.to_string(),
            label: label.to_string(),
            widget: WidgetKind::Checkbox,
            meta: ParamMeta::slider(0.0, 1.0, 1.0, 0, "", 100.0),
            value: PropValue::Bool { value, animated },
        }
    }

    /// Dropdown over a fixed option list, carrying the selected index.
    pub fn enumeration(field: &str, label: &str, options: Vec<String>, selected: usize) -> Self {
        let mut meta = ParamMeta::slider(0.0, options.len().saturating_sub(1) as f32, 1.0, 0, "", 100.0);
        meta.options = options;
        Self {
            field: field.to_string(),
            label: label.to_string(),
            widget: WidgetKind::Dropdown,
            meta,
            value: PropValue::EnumSel { index: selected },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{Effect, EffectType};
    use crate::property::Property;
    use crate::stock::StockPlugin;
    use crate::timecode::TimeCode;

    fn p(v: f32) -> Property<f32> {
        Property::new("P", v)
    }

    fn c(v: Color) -> Property<Color> {
        Property::new("C", v)
    }

    fn b(v: bool) -> Property<bool> {
        Property::new("B", v)
    }

    fn fx(t: EffectType) -> Effect {
        Effect::new("e", "E", t)
    }

    #[test]
    fn every_effect_declares_widgets() {
        // One instance per variant: each must declare at least one widget
        // with a non-empty field id + label (the panel renders from this).
        let cases: Vec<Effect> = vec![
            fx(EffectType::GaussianBlur { radius: p(5.0) }),
            fx(EffectType::BrightnessContrast { brightness: p(0.0), contrast: p(0.0) }),
            fx(EffectType::Tint { map_black: c(Color::BLACK), map_white: c(Color::WHITE), amount: p(100.0) }),
            fx(EffectType::Invert { amount: p(100.0) }),
            fx(EffectType::DropShadow { distance: p(5.0), angle: p(45.0), softness: p(5.0), opacity: p(80.0), color: c(Color::BLACK) }),
            fx(EffectType::GlslShader { code: String::new(), param1: p(0.0), param2: p(0.0), param3: p(0.0), param4: p(0.0) }),
            fx(EffectType::DisplacementMap { max_horizontal: p(0.0), max_vertical: p(0.0) }),
            fx(EffectType::ChromaKey { key_color: c(Color::GREEN), tolerance: p(30.0), feather: p(10.0) }),
            fx(EffectType::LumaKey { threshold: p(50.0), feather: p(5.0) }),
            fx(EffectType::SwapColor { from_color: c(Color::GREEN), to_color: c(Color::RED), tolerance: p(30.0), feather: p(10.0) }),
            fx(EffectType::NoiseGenerator { amount: p(50.0), monochrome: b(false) }),
            fx(EffectType::Checkerboard { size: p(32.0), color_a: c(Color::BLACK), color_b: c(Color::WHITE) }),
            fx(EffectType::GradientRamp { color_a: c(Color::BLACK), color_b: c(Color::WHITE), angle: p(90.0), stops: Vec::new(), gradient_type: crate::layer::GradientType::Linear }),
            fx(EffectType::Perspective { skew_x: p(0.0), skew_y: p(0.0) }),
            fx(EffectType::TextOutline { width: p(3.0), color: c(Color::BLACK) }),
            fx(EffectType::TextBevel { strength: p(50.0), softness: p(10.0) }),
            fx(EffectType::Bloom { intensity: p(50.0), radius: p(5.0) }),
            fx(EffectType::Tiler { tiles_x: p(4.0), tiles_y: p(4.0), mode: TileMode::Grid, mirror: b(false), offset_x: p(0.0), offset_y: p(0.0), cell: TileCell::Square, seed: p(1.0), amount: p(0.0) }),
            fx(EffectType::Warp { amount: p(50.0), scale: p(1.0), pins: Vec::new() }),
            fx(EffectType::Exposure { exposure: p(1.5) }),
            fx(EffectType::Vibrance { vibrance: p(20.0) }),
            fx(EffectType::Levels { input_black: p(0.0), input_white: p(255.0), gamma: p(1.0), output_black: p(0.0), output_white: p(255.0) }),
            fx(EffectType::HueSaturation { hue_shift: p(0.0), saturation: p(0.0), lightness: p(0.0) }),
            fx(EffectType::Sharpen { amount: p(50.0), radius: p(1.0) }),
            fx(EffectType::Vignette { amount: p(50.0), softness: p(50.0) }),
            Effect::stock("s", StockPlugin::Solid),
        ];
        assert_eq!(cases.len(), 26);
        for fx in &cases {
            let decls = fx.declarations();
            assert!(!decls.is_empty(), "{:?}", fx.effect_type);
            for d in &decls {
                assert!(!d.field.is_empty() && !d.label.is_empty(), "{d:?}");
            }
        }
        // Widget kinds follow the mapping table.
        let tint = &cases[2].declarations();
        assert!(tint.iter().any(|d| d.widget == WidgetKind::Color));
        assert!(tint.iter().any(|d| d.widget == WidgetKind::Percentage));
        let noise = &cases[10].declarations();
        assert!(noise.iter().any(|d| d.widget == WidgetKind::Checkbox));
        let tiler = &cases[17].declarations();
        assert!(tiler.iter().any(|d| d.widget == WidgetKind::Dropdown));
        assert!(tiler.iter().any(|d| d.widget == WidgetKind::Integer));
        assert!(tiler.iter().any(|d| d.widget == WidgetKind::Checkbox));
        assert!(tiler.iter().any(|d| d.widget == WidgetKind::Percentage));
        let modes: Vec<String> = TileMode::ALL.iter().map(|m| m.label().to_string()).collect();
        assert!(tiler.iter().any(|d| d.field == "mode" && d.meta.options == modes));
        let _ = TimeCode::from_frames(0, 30.0);
    }

    #[test]
    fn meta_display_matches_legacy_rows() {
        // Readouts must equal the historic panel formats.
        let m = ParamMeta::slider(0.0, 100.0, 5.0, 1, "px", 100.0);
        assert_eq!(m.display(32.0), "32.0px");
        let m = ParamMeta::slider(0.0, 360.0, 15.0, 1, "°", 360.0);
        assert_eq!(m.display(45.0), "45.0°");
        let m = ParamMeta { signed: true, unit: " EV".to_string(), ..ParamMeta::slider(-8.0, 8.0, 0.25, 2, "", 100.0) };
        assert_eq!(m.display(1.5), "+1.50 EV");
        let m = ParamMeta::slider(0.0, 100.0, 10.0, 0, " %", 100.0);
        assert_eq!(m.display(80.0), "80 %");
        // Shader param types declare their widgets.
        use crate::shader::ShaderParamType;
        assert_eq!(ShaderParamType::Float.widget_kind(), WidgetKind::Slider);
        assert_eq!(ShaderParamType::Bool.widget_kind(), WidgetKind::Checkbox);
        assert_eq!(ShaderParamType::Vec3.widget_kind(), WidgetKind::Vec3);
        assert_eq!(ShaderParamType::Angle.widget_kind(), WidgetKind::Angle);
        assert!(matches!(ShaderParamType::Enum { options: Vec::new() }.widget_kind(), WidgetKind::Dropdown));
    }
}
