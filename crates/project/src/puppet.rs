//! Puppet warp pins: arbitrary control points with keyframable offsets.
//!
//! Unlike the fixed [`crate::warp`] lattice, pins are placed freely
//! (double-click the layer) and deform by inverse-distance weighting, so
//! unpinned areas stay put. An empty pin vec is the identity warp and old
//! project files (no `pins` key) load as identity.

use crate::property::de_property_or_value;
use crate::vec2::Vec2;
use serde::{Deserialize, Serialize};

fn default_pin_component() -> crate::Property<f32> {
    crate::Property::new("Offset", 0.0)
}

/// One puppet pin: rest position (layer px) plus a keyframable offset.
/// Pin components resolve through the standard `pin_{i}_{x|y}` effect
/// params, so timeline lanes, spline series, and nudges all work with no
/// per-pin UI code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PuppetPin {
    /// Rest position, layer px.
    pub x: f32,
    pub y: f32,
    #[serde(deserialize_with = "de_property_or_value", default = "default_pin_component")]
    pub dx: crate::Property<f32>,
    #[serde(deserialize_with = "de_property_or_value", default = "default_pin_component")]
    pub dy: crate::Property<f32>,
}

impl PuppetPin {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y, dx: default_pin_component(), dy: default_pin_component() }
    }

    /// Rest tip (offset applied) in layer px.
    pub fn tip(&self) -> Vec2 {
        Vec2::new(self.x + self.dx.value, self.y + self.dy.value)
    }
}

/// Evaluated (time-resolved) pin for the deformer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PuppetDeform {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
}

/// Largest absolute pin offset (render-box padding + effect hashing).
pub fn max_offset(pins: &[PuppetDeform]) -> f32 {
    pins
        .iter()
        .map(|p| p.dx.abs().max(p.dy.abs()))
        .fold(0.0f32, f32::max)
}

/// Inverse-distance-weighted pin offset at layer-local (`x`, `y`).
/// Exact hits return the pin offset; pins beyond `expansion` are ignored
/// (`expansion <= 0` = unlimited). Higher `stiffness` localizes the falloff.
pub fn sample_offset(
    pins: &[PuppetDeform],
    stiffness: f32,
    expansion: f32,
    x: f32,
    y: f32,
) -> (f32, f32) {
    let power = stiffness.clamp(0.5, 8.0);
    let mut num_x = 0.0f32;
    let mut num_y = 0.0f32;
    let mut den = 0.0f32;
    for p in pins {
        let dx = x - p.x;
        let dy = y - p.y;
        let d2 = dx * dx + dy * dy;
        if d2 < 1e-6 {
            return (p.dx, p.dy);
        }
        let d = d2.sqrt();
        if expansion > 0.0 && d > expansion {
            continue;
        }
        let w = 1.0 / d.powf(power);
        num_x += p.dx * w;
        num_y += p.dy * w;
        den += w;
    }
    if den <= 0.0 {
        (0.0, 0.0)
    } else {
        (num_x / den, num_y / den)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(x: f32, y: f32, dx: f32, dy: f32) -> PuppetDeform {
        PuppetDeform { x, y, dx, dy }
    }

    #[test]
    fn identity_when_empty_or_zero() {
        assert_eq!(sample_offset(&[], 2.0, 0.0, 10.0, 20.0), (0.0, 0.0));
        assert_eq!(sample_offset(&[pin(0.0, 0.0, 0.0, 0.0)], 2.0, 0.0, 50.0, 50.0), (0.0, 0.0));
        assert_eq!(max_offset(&[]), 0.0);
    }

    #[test]
    fn exact_hit_returns_pin_offset() {
        let pins = [pin(10.0, 20.0, 30.0, -15.0)];
        assert_eq!(sample_offset(&pins, 2.0, 0.0, 10.0, 20.0), (30.0, -15.0));
    }

    #[test]
    fn opposing_pins_cancel_at_midpoint() {
        let pins = [pin(0.0, 0.0, 40.0, 0.0), pin(100.0, 0.0, -40.0, 0.0)];
        let (x, y) = sample_offset(&pins, 2.0, 0.0, 50.0, 0.0);
        assert!(x.abs() < 1e-4 && y.abs() < 1e-4, "{x} {y}");
    }

    #[test]
    fn near_pin_dominates_far_pin() {
        let pins = [pin(0.0, 0.0, 60.0, 0.0), pin(1000.0, 0.0, -60.0, 0.0)];
        let (x, _) = sample_offset(&pins, 2.0, 0.0, 10.0, 0.0);
        assert!(x > 55.0, "{x}");
    }

    #[test]
    fn expansion_limits_influence() {
        let pins = [pin(0.0, 0.0, 60.0, 0.0)];
        assert_eq!(sample_offset(&pins, 2.0, 5.0, 100.0, 0.0), (0.0, 0.0));
        let (x, _) = sample_offset(&pins, 2.0, 0.0, 100.0, 0.0);
        assert!((x - 60.0).abs() < 1e-4, "{x}");
    }

    #[test]
    fn max_offset_reports_largest_drag() {
        let pins = [pin(0.0, 0.0, 10.0, -25.0), pin(5.0, 5.0, 3.0, 4.0)];
        assert_eq!(max_offset(&pins), 25.0);
    }
}
