//! First-class compositor masks.
//!
//! Each [`Mask`] owns an animatable [`Path`](crate::path::Path), a combine
//! [`MaskMode`], and the classic feather / expansion / opacity / invert
//! controls plus its own [`Transform`](crate::transform::Transform).
//! Evaluation resolves everything to plain values
//! (`compositor::EvaluatedMask`); the rasterizer runs the documented
//! pipeline per layer:
//!
//! ```text
//! Path → Mask Coverage → Feather / Expansion → Mask Combination → Layer Alpha
//! ```
//!
//! Coverage combination happens in raster space (exact for all modes, no
//! vector surgery): each mask fills its coverage, feather/expansion shape
//! it, opacity blends it toward the mode neutral, then modes combine in
//! mask order — Add / Subtract / Intersect / Difference / None.

use crate::keyframe::Keyframe;
use crate::path::Path;
use crate::property::Property;
use crate::timecode::TimeCode;
use crate::transform::Transform;
use crate::vec2::Vec2;
use serde::{Deserialize, Serialize};

/// How one mask combines into the layer's accumulated alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    /// Union into the accumulation.
    #[default]
    Add,
    /// Cut out of the accumulation.
    Subtract,
    /// Keep only the overlap with the accumulation.
    Intersect,
    /// Absolute difference with the accumulation.
    Difference,
    /// Present but ignored (solo/mute helper).
    None,
}

impl MaskMode {
    /// Human label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Intersect => "Intersect",
            Self::Difference => "Difference",
            Self::None => "None",
        }
    }

    /// Coverage value that makes this mode a no-op (used to scale opacity:
    /// `effective = neutral + (coverage - neutral) * opacity`).
    pub const fn neutral(self) -> f32 {
        match self {
            Self::Add => 0.0,
            Self::Subtract => 0.0,
            Self::Intersect => 1.0,
            Self::Difference => 0.0,
            Self::None => 0.0,
        }
    }

    /// Cycle to the next combining mode (Add → Subtract → Intersect →
    /// Difference → None → Add). Used by the mode pill UI.
    pub const fn cycle(self) -> Self {
        match self {
            Self::Add => Self::Subtract,
            Self::Subtract => Self::Intersect,
            Self::Intersect => Self::Difference,
            Self::Difference => Self::None,
            Self::None => Self::Add,
        }
    }

    /// All combining modes in cycle order (drives the mode Combobox).
    pub const ALL: [Self; 5] = [
        Self::Add,
        Self::Subtract,
        Self::Intersect,
        Self::Difference,
        Self::None,
    ];

    /// Parse a [`Self::label`] back into a mode (Combobox commit path).
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.label() == label)
    }
}

/// Combine one effective coverage into the accumulation.
/// `has` tracks whether any constraining mask ran yet (None starts at full
/// alpha, i.e. "no masks" shows the whole layer).
pub fn combine_mask_coverage(mode: MaskMode, acc: f32, cov: f32, has: bool) -> (f32, bool) {
    match mode {
        MaskMode::Add => (if has { acc.max(cov) } else { cov }, true),
        MaskMode::Subtract => (if has { acc.min(1.0 - cov) } else { 1.0 - cov }, true),
        MaskMode::Intersect => (if has { acc.min(cov) } else { cov }, true),
        MaskMode::Difference => (if has { (acc - cov).abs() } else { cov }, true),
        MaskMode::None => (acc, has),
    }
}

/// Numeric mask-shape primitive for the Mask Shape dialog
/// (After Effects: Layer > Mask > Mask Shape → Rectangle / Ellipse with an
/// explicit bounding box).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskShapeKind {
    #[default]
    Rectangle,
    Ellipse,
}

impl MaskShapeKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
        }
    }

    pub const fn cycle(self) -> Self {
        match self {
            Self::Rectangle => Self::Ellipse,
            Self::Ellipse => Self::Rectangle,
        }
    }

    /// Closed path for the `x, y, w, h` bounding box (layer-local coords).
    pub fn path(self, x: f32, y: f32, w: f32, h: f32) -> Path {
        let (w, h) = (w.max(1.0), h.max(1.0));
        match self {
            Self::Rectangle => Path::rectangle(x, y, w, h),
            Self::Ellipse => Path::ellipse(x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0),
        }
    }
}

/// A single mask on a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Locked masks reject every mutation except enable/rename/delete
    /// (After Effects: locked masks cannot be changed).
    #[serde(default)]
    pub locked: bool,
    /// Animatable Bézier path in layer-local coords (morphs when every
    /// keyframe shares topology, steps otherwise).
    pub path: Property<Path>,
    #[serde(default)]
    pub mode: MaskMode,
    /// Mask opacity in 0..100 (blends coverage toward the mode neutral).
    pub opacity: Property<f32>,
    /// Feather/softness in px.
    pub feather: Property<f32>,
    /// Expansion in px (negative contracts).
    pub expansion: Property<f32>,
    #[serde(default)]
    pub invert: bool,
    /// Mask-local transform (animatable) applied to the path at raster time.
    pub transform: Transform,
}

const fn default_true() -> bool {
    true
}

impl Mask {
    /// New mask with a default 200×200 rectangle path.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            enabled: true,
            locked: false,
            path: Property::new("Mask Path", Path::rectangle(-100.0, -100.0, 200.0, 200.0)),
            mode: MaskMode::Add,
            opacity: Property::new("Mask Opacity", 100.0),
            feather: Property::new("Mask Feather", 0.0),
            expansion: Property::new("Mask Expansion", 0.0),
            invert: false,
            transform: Transform::default(),
        }
    }

    /// New mask wrapping an explicit path.
    pub fn with_path(id: impl Into<String>, name: impl Into<String>, path: Path) -> Self {
        let mut mask = Self::new(id, name);
        mask.path = Property::new("Mask Path", path);
        mask
    }

    /// Toggle enabled / bypassed.
    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Toggle the edit lock.
    pub fn toggle_locked(&mut self) {
        self.locked = !self.locked;
    }

    /// Rename (blank names fall back to "Mask").
    pub fn rename(&mut self, name: &str) {
        let clean = name.trim();
        self.name = if clean.is_empty() { "Mask".to_string() } else { clean.to_string() };
    }

    /// Retrieve a scalar param property by name for keyframing/scrubbing.
    pub fn get_param_property(&self, name: &str) -> Option<&Property<f32>> {
        if name.eq_ignore_ascii_case("opacity") {
            Some(&self.opacity)
        } else if name.eq_ignore_ascii_case("feather") {
            Some(&self.feather)
        } else if name.eq_ignore_ascii_case("expansion") {
            Some(&self.expansion)
        } else {
            None
        }
    }

    /// Mutable variant of [`Self::get_param_property`].
    pub fn get_param_property_mut(&mut self, name: &str) -> Option<&mut Property<f32>> {
        if name.eq_ignore_ascii_case("opacity") {
            Some(&mut self.opacity)
        } else if name.eq_ignore_ascii_case("feather") {
            Some(&mut self.feather)
        } else if name.eq_ignore_ascii_case("expansion") {
            Some(&mut self.expansion)
        } else {
            None
        }
    }

    /// Nudge a scalar param (clamped to its range). Returns false for
    /// unknown names.
    pub fn nudge_param(&mut self, name: &str, delta: f32) -> bool {
        if name.eq_ignore_ascii_case("opacity") {
            self.opacity.set_value((self.opacity.value + delta).clamp(0.0, 100.0));
            true
        } else if name.eq_ignore_ascii_case("feather") {
            self.feather.set_value((self.feather.value + delta).max(0.0));
            true
        } else if name.eq_ignore_ascii_case("expansion") {
            self.expansion.set_value((self.expansion.value + delta).clamp(-500.0, 500.0));
            true
        } else {
            false
        }
    }

    /// Transform component names served to the inspector scrub pipeline:
    /// `position.x`, `position.y`, `rotation`, `scale.x`, `scale.y`.
    /// Vec2 components animate on the parent `Property<Vec2>` (same as
    /// layer transform scrub).
    pub fn transform_value(&self, name: &str, time: &TimeCode) -> Option<f32> {
        match name.to_lowercase().as_str() {
            "position.x" => Some(self.transform.position.evaluate_at(time).x),
            "position.y" => Some(self.transform.position.evaluate_at(time).y),
            "rotation" => Some(self.transform.rotation.evaluate_at(time)),
            "scale.x" => Some(self.transform.scale.evaluate_at(time).x),
            "scale.y" => Some(self.transform.scale.evaluate_at(time).y),
            _ => None,
        }
    }

    /// True when the property behind a transform component name is animated.
    pub fn transform_animated(&self, name: &str) -> bool {
        match name.to_lowercase().as_str() {
            "position.x" | "position.y" => self.transform.position.is_animated(),
            "rotation" => self.transform.rotation.is_animated(),
            "scale.x" | "scale.y" => self.transform.scale.is_animated(),
            _ => false,
        }
    }

    /// Toggle animation on the property behind a transform component name
    /// (stopwatch behavior: clear keyframes, else key the current value).
    /// Returns false for unknown names.
    pub fn toggle_transform_animation(&mut self, name: &str, time: TimeCode) -> bool {
        match name.to_lowercase().as_str() {
            "position.x" | "position.y" => {
                let p = &mut self.transform.position;
                if p.is_animated() {
                    p.clear_keyframes();
                } else {
                    let v = p.value;
                    p.add_keyframe(Keyframe::new(time, v));
                }
                true
            }
            "rotation" => {
                let p = &mut self.transform.rotation;
                if p.is_animated() {
                    p.clear_keyframes();
                } else {
                    let v = p.value;
                    p.add_keyframe(Keyframe::new(time, v));
                }
                true
            }
            "scale.x" | "scale.y" => {
                let p = &mut self.transform.scale;
                if p.is_animated() {
                    p.clear_keyframes();
                } else {
                    let v = p.value;
                    p.add_keyframe(Keyframe::new(time, v));
                }
                true
            }
            _ => false,
        }
    }

    /// Nudge a transform component (evaluated-when-animated, keyframed
    /// when animated — mirrors layer position/scale nudging). Scale clamps
    /// at 0.1 to avoid degenerate flips. Returns false for unknown names.
    pub fn nudge_transform(&mut self, name: &str, delta: f32, time: &TimeCode) -> bool {
        match name.to_lowercase().as_str() {
            "position.x" | "position.y" => {
                let p = &mut self.transform.position;
                let cur = if p.is_animated() { p.evaluate_at(time) } else { p.value };
                let new_val = if name.ends_with(".x") {
                    Vec2::new(cur.x + delta, cur.y)
                } else {
                    Vec2::new(cur.x, cur.y + delta)
                };
                p.set_value(new_val);
                if p.is_animated() {
                    p.add_keyframe(Keyframe::new(*time, new_val));
                }
                true
            }
            "rotation" => {
                let p = &mut self.transform.rotation;
                let cur = if p.is_animated() { p.evaluate_at(time) } else { p.value };
                p.set_value(cur + delta);
                if p.is_animated() {
                    p.add_keyframe(Keyframe::new(*time, cur + delta));
                }
                true
            }
            "scale.x" | "scale.y" => {
                let p = &mut self.transform.scale;
                let cur = if p.is_animated() { p.evaluate_at(time) } else { p.value };
                let new_val = if name.ends_with(".x") {
                    Vec2::new((cur.x + delta).max(0.1), cur.y)
                } else {
                    Vec2::new(cur.x, (cur.y + delta).max(0.1))
                };
                p.set_value(new_val);
                if p.is_animated() {
                    p.add_keyframe(Keyframe::new(*time, new_val));
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_components_roundtrip() {
        let tc = TimeCode::from_frames(10, 30.0);
        let mut m = Mask::with_path("m", "M", Path::rectangle(0.0, 0.0, 10.0, 10.0));
        // Defaults: pos 0, rot 0, scale 100.
        assert_eq!(m.transform_value("position.x", &tc), Some(0.0));
        assert_eq!(m.transform_value("scale.x", &tc), Some(100.0));
        assert_eq!(m.transform_value("bogus", &tc), None);
        assert!(!m.transform_animated("rotation"));
        // Nudge writes through to the parent property.
        assert!(m.nudge_transform("position.x", 5.0, &tc));
        assert!(m.nudge_transform("scale.y", -10.0, &tc));
        assert!(m.nudge_transform("rotation", 45.0, &tc));
        assert!(!m.nudge_transform("bogus", 1.0, &tc));
        assert_eq!(m.transform_value("position.x", &tc), Some(5.0));
        assert_eq!(m.transform_value("scale.y", &tc), Some(90.0));
        assert_eq!(m.transform_value("rotation", &tc), Some(45.0));
        // Stopwatch toggles animation on the parent property.
        assert!(m.toggle_transform_animation("position.x", tc));
        assert!(m.transform_animated("position.y"));
        assert!(m.toggle_transform_animation("position.x", tc));
        assert!(!m.transform_animated("position.x"));
        assert!(!m.toggle_transform_animation("bogus", tc));
    }

    #[test]
    fn combine_modes_behave() {
        // Start empty: first Add sets, first Subtract inverts.
        assert_eq!(combine_mask_coverage(MaskMode::Add, 1.0, 0.4, false), (0.4, true));
        assert_eq!(combine_mask_coverage(MaskMode::Subtract, 1.0, 0.4, false), (0.6, true));
        assert_eq!(combine_mask_coverage(MaskMode::Intersect, 1.0, 0.4, false), (0.4, true));
        // Accumulation.
        assert_eq!(combine_mask_coverage(MaskMode::Add, 0.3, 0.7, true), (0.7, true));
        assert_eq!(combine_mask_coverage(MaskMode::Subtract, 0.8, 0.3, true), (0.7, true));
        assert_eq!(combine_mask_coverage(MaskMode::Intersect, 0.8, 0.3, true), (0.3, true));
        let (d, _) = combine_mask_coverage(MaskMode::Difference, 0.8, 0.3, true);
        assert!((d - 0.5).abs() < 1e-6);
        // None never constrains.
        assert_eq!(combine_mask_coverage(MaskMode::None, 0.5, 1.0, false), (0.5, false));
        // Neutrals are true no-ops.
        for mode in [MaskMode::Add, MaskMode::Subtract, MaskMode::Intersect, MaskMode::Difference] {
            let n = mode.neutral();
            let (v, has) = combine_mask_coverage(mode, 0.62, n, true);
            assert!((v - 0.62).abs() < 1e-6, "{mode:?}");
            assert!(has);
        }
        // Mode cycling covers all combining modes.
        let mut m = MaskMode::Add;
        for _ in 0..4 {
            m = m.cycle();
        }
        assert_eq!(m, MaskMode::None);
        assert_eq!(m.cycle(), MaskMode::Add);
    }

    #[test]
    fn mask_params_roundtrip() {
        let mut m = Mask::new("m1", "Mask 1");
        assert!(m.get_param_property("opacity").is_some());
        assert!(m.get_param_property("feather").is_some());
        assert!(m.get_param_property("expansion").is_some());
        assert!(m.get_param_property("nope").is_none());
        assert!(m.nudge_param("opacity", -30.0));
        assert!((m.opacity.value - 70.0).abs() < 1e-5);
        assert!(!m.nudge_param("nope", 1.0));
        m.toggle_enabled();
        assert!(!m.enabled);
    }
}
