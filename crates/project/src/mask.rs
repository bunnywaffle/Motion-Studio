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

use crate::path::Path;
use crate::property::Property;
use crate::transform::Transform;
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

/// A single mask on a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
