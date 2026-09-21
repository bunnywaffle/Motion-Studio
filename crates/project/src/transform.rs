use crate::property::Property;
use crate::timecode::TimeCode;
use crate::vec2::Vec2;
use serde::{Deserialize, Serialize};

/// Spatial transformation state for a layer, comprising anchor point, position, scale, and rotation.
/// Each component is modeled as an animatable `Property`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub anchor_point: Property<Vec2>,
    pub position: Property<Vec2>,
    pub scale: Property<Vec2>,
    pub rotation: Property<f32>,
    /// When true (the default), scale edits apply uniformly to X and Y as a
    /// single value, After Effects-style. When false, X and Y edit separately.
    #[serde(default = "default_scale_uniform")]
    pub scale_uniform: bool,
}

const fn default_scale_uniform() -> bool {
    true
}

impl Transform {
    /// Create a transform with explicit initial values, maintaining standard motion graphics defaults
    /// (anchor: 0,0; pos: 0,0; scale: 100,100; rot: 0).
    pub fn new(anchor_point: Vec2, position: Vec2, scale: Vec2, rotation: f32) -> Self {
        Self {
            anchor_point: Property::with_default("Anchor Point", anchor_point, Vec2::ZERO),
            position: Property::with_default("Position", position, Vec2::ZERO),
            scale: Property::with_default("Scale", scale, Vec2::SCALE_100),
            rotation: Property::with_default("Rotation", rotation, 0.0),
            scale_uniform: true,
        }
    }

    /// Create a transform placed at the specified position with default anchor, scale, and rotation.
    pub fn from_position(position: Vec2) -> Self {
        Self {
            position: Property::with_default("Position", position, Vec2::ZERO),
            ..Default::default()
        }
    }

    /// Reset all transform properties to their default values.
    pub fn reset_all(&mut self) {
        self.anchor_point.reset();
        self.position.reset();
        self.scale.reset();
        self.rotation.reset();
    }

    /// Check if all transform properties are at their default values.
    pub fn is_default(&self) -> bool {
        self.anchor_point.is_default()
            && self.position.is_default()
            && self.scale.is_default()
            && self.rotation.is_default()
    }

    /// Check whether any of the transform properties are currently animated.
    pub fn is_animated(&self) -> bool {
        self.anchor_point.is_animated()
            || self.position.is_animated()
            || self.scale.is_animated()
            || self.rotation.is_animated()
    }

    /// Set uniform-scale mode. When turning uniform mode on with differing
    /// X/Y values, Y snaps to X so the single value is unambiguous.
    pub fn set_scale_uniform(&mut self, uniform: bool) {
        if uniform && !self.scale_uniform {
            let v = self.scale.value;
            self.scale.set_value(Vec2::new(v.x, v.x));
        }
        self.scale_uniform = uniform;
    }

    /// Evaluate all transform components at a given TimeCode.
    /// Returns `(anchor_point, position, scale, rotation)`.
    pub fn evaluate_at(&self, time: &TimeCode) -> (Vec2, Vec2, Vec2, f32) {
        (
            self.anchor_point.evaluate_at(time),
            self.position.evaluate_at(time),
            self.scale.evaluate_at(time),
            self.rotation.evaluate_at(time),
        )
    }

    /// Evaluate all transform components at arbitrary floating-point seconds.
    /// Returns `(anchor_point, position, scale, rotation)`.
    pub fn evaluate_at_seconds(&self, seconds: f64) -> (Vec2, Vec2, Vec2, f32) {
        (
            self.anchor_point.evaluate_at_seconds(seconds),
            self.position.evaluate_at_seconds(seconds),
            self.scale.evaluate_at_seconds(seconds),
            self.rotation.evaluate_at_seconds(seconds),
        )
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            anchor_point: Property::new("Anchor Point", Vec2::ZERO),
            position: Property::new("Position", Vec2::ZERO),
            scale: Property::new("Scale", Vec2::SCALE_100),
            rotation: Property::new("Rotation", 0.0),
            scale_uniform: true,
        }
    }
}
