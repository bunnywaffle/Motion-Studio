use crate::property::Property;
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
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            anchor_point: Property::new("Anchor Point", Vec2::ZERO),
            position: Property::new("Position", Vec2::ZERO),
            scale: Property::new("Scale", Vec2::SCALE_100),
            rotation: Property::new("Rotation", 0.0),
        }
    }
}
