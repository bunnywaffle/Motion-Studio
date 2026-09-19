use crate::clock::PlaybackClock;
use crate::color::Color;
use crate::error::ValidationError;
use crate::frame_rate::FrameRate;
use crate::layer::{Layer, LayerSource, ShapeType};
use crate::marker::Marker;
use crate::timecode::TimeCode;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A composition containing ordered layers, dimensions, frame timing, background color, and markers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Composition {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub duration: TimeCode,
    pub background_color: Color,
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}

impl Composition {
    /// Create a new composition with explicit properties.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        width: u32,
        height: u32,
        frame_rate: f64,
        duration: TimeCode,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            width,
            height,
            frame_rate,
            duration,
            background_color: Color::BLACK,
            layers: Vec::new(),
            markers: Vec::new(),
        }
    }

    /// Convenience constructor for standard 1920x1080 30fps compositions.
    pub fn hd_1080p_30fps(
        id: impl Into<String>,
        name: impl Into<String>,
        duration_seconds: f64,
    ) -> Self {
        let frame_rate = 30.0;
        Self {
            id: id.into(),
            name: name.into(),
            width: 1920,
            height: 1080,
            frame_rate,
            duration: TimeCode::from_seconds(duration_seconds, frame_rate),
            background_color: Color::BLACK,
            layers: Vec::new(),
            markers: Vec::new(),
        }
    }

    /// Return the aspect ratio (width / height) as a floating-point number.
    pub fn aspect_ratio(&self) -> f64 {
        if self.height > 0 {
            self.width as f64 / self.height as f64
        } else {
            0.0
        }
    }

    /// Return the composition total duration in frames.
    pub fn duration_frames(&self) -> i64 {
        self.duration.frames()
    }

    /// Return the composition total duration in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.duration.seconds()
    }

    // --- Layer Stack Management ---

    /// Append a layer to the top of the layer stack.
    pub fn add_layer(&mut self, layer: Layer) -> Result<(), ValidationError> {
        if self.layers.iter().any(|l| l.id == layer.id) {
            return Err(ValidationError::DuplicateLayerId(layer.id));
        }
        self.layers.push(layer);
        Ok(())
    }

    /// Insert a layer at a specific position in the layer stack (0 = top).
    pub fn insert_layer(&mut self, index: usize, layer: Layer) -> Result<(), ValidationError> {
        if index > self.layers.len() {
            return Err(ValidationError::IndexOutOfBounds {
                index,
                len: self.layers.len(),
            });
        }
        if self.layers.iter().any(|l| l.id == layer.id) {
            return Err(ValidationError::DuplicateLayerId(layer.id));
        }
        self.layers.insert(index, layer);
        Ok(())
    }

    /// Remove a layer by its ID.
    pub fn remove_layer(&mut self, layer_id: &str) -> Option<Layer> {
        if let Some(index) = self.layer_index(layer_id) {
            // Also unparent any children that were parented to this layer
            for l in &mut self.layers {
                if l.parent_id.as_deref() == Some(layer_id) {
                    l.parent_id = None;
                }
            }
            Some(self.layers.remove(index))
        } else {
            None
        }
    }

    /// Find a layer index by ID.
    pub fn layer_index(&self, layer_id: &str) -> Option<usize> {
        self.layers.iter().position(|l| l.id == layer_id)
    }

    /// Get an immutable reference to a layer by ID.
    pub fn get_layer(&self, layer_id: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == layer_id)
    }

    /// Get a mutable reference to a layer by ID.
    pub fn get_layer_mut(&mut self, layer_id: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == layer_id)
    }

    /// Move a layer from one position in the stack to another.
    pub fn move_layer(&mut self, from_index: usize, to_index: usize) -> Result<(), ValidationError> {
        let len = self.layers.len();
        if from_index >= len {
            return Err(ValidationError::IndexOutOfBounds {
                index: from_index,
                len,
            });
        }
        if to_index >= len {
            return Err(ValidationError::IndexOutOfBounds {
                index: to_index,
                len,
            });
        }
        if from_index == to_index {
            return Ok(());
        }
        let layer = self.layers.remove(from_index);
        self.layers.insert(to_index, layer);
        Ok(())
    }

    /// Reorder a layer identified by ID to a new index.
    pub fn reorder_layer(
        &mut self,
        layer_id: &str,
        new_index: usize,
    ) -> Result<(), ValidationError> {
        let current_index = self
            .layer_index(layer_id)
            .ok_or_else(|| ValidationError::LayerNotFound(layer_id.to_string()))?;
        self.move_layer(current_index, new_index)
    }

    // --- Hierarchy & Parenting Queries ---

    /// Return all root layers (layers without a parent).
    pub fn root_layers(&self) -> Vec<&Layer> {
        self.layers.iter().filter(|l| l.parent_id.is_none()).collect()
    }

    /// Return the parent layer of the given layer ID, if it exists.
    pub fn get_parent(&self, layer_id: &str) -> Option<&Layer> {
        let layer = self.get_layer(layer_id)?;
        let parent_id = layer.parent_id.as_deref()?;
        self.get_layer(parent_id)
    }

    /// Return all immediate child layers parented to the given layer ID.
    pub fn get_children(&self, parent_id: &str) -> Vec<&Layer> {
        self.layers
            .iter()
            .filter(|l| l.parent_id.as_deref() == Some(parent_id))
            .collect()
    }

    /// Return the ordered chain of ancestor layers from immediate parent up to root.
    /// Cycle-safe: terminates early if a cycle is encountered.
    pub fn get_ancestor_chain(&self, layer_id: &str) -> Vec<&Layer> {
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        visited.insert(layer_id);
        let mut current_id = layer_id;
        while let Some(parent) = self.get_parent(current_id) {
            if !visited.insert(&parent.id) {
                break;
            }
            chain.push(parent);
            current_id = &parent.id;
        }
        chain
    }

    // --- Markers ---

    /// Add a timeline marker to the composition.
    pub fn add_marker(&mut self, marker: Marker) {
        self.markers.push(marker);
    }

    /// Remove a timeline marker by ID.
    pub fn remove_marker(&mut self, marker_id: &str) -> Option<Marker> {
        if let Some(idx) = self.markers.iter().position(|m| m.id == marker_id) {
            Some(self.markers.remove(idx))
        } else {
            None
        }
    }

    // --- Validation ---

    /// Validate the composition structure: dimensions, timing, layer timing, and parenting hierarchies.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.width == 0 || self.height == 0 {
            return Err(ValidationError::InvalidDimensions {
                width: self.width,
                height: self.height,
            });
        }
        if self.frame_rate <= 0.0 || !self.frame_rate.is_finite() {
            return Err(ValidationError::InvalidFrameRate(self.frame_rate));
        }
        if self.duration.frames() <= 0 {
            return Err(ValidationError::InvalidDuration(format!(
                "composition duration must be > 0 frames (got {})",
                self.duration.frames()
            )));
        }

        // Validate layers individual timing and unique IDs
        let mut seen_ids = HashSet::new();
        for layer in &self.layers {
            if !seen_ids.insert(&layer.id) {
                return Err(ValidationError::DuplicateLayerId(layer.id.clone()));
            }
            layer.validate()?;
        }

        // Validate parenting hierarchy
        self.validate_parenting()?;

        Ok(())
    }

    /// Validate that all parent references point to existing layers and there are no cycles.
    pub fn validate_parenting(&self) -> Result<(), ValidationError> {
        let valid_ids: HashSet<&str> = self.layers.iter().map(|l| l.id.as_str()).collect();

        for layer in &self.layers {
            if let Some(ref parent_id) = layer.parent_id {
                // Check self-parenting
                if parent_id == &layer.id {
                    return Err(ValidationError::SelfParenting(layer.id.clone()));
                }

                // Check target existence
                if !valid_ids.contains(parent_id.as_str()) {
                    return Err(ValidationError::ParentNotFound {
                        layer_id: layer.id.clone(),
                        parent_id: parent_id.clone(),
                    });
                }

                // Cycle detection using chain traversal
                let mut visited = HashSet::new();
                visited.insert(&layer.id);
                let mut current_parent_id = parent_id.clone();

                let mut cycle_path = vec![layer.id.clone()];

                while let Some(parent) = self.get_layer(&current_parent_id) {
                    cycle_path.push(parent.id.clone());
                    if !visited.insert(&parent.id) {
                        return Err(ValidationError::ParentCycleDetected {
                            layer_id: layer.id.clone(),
                            cycle: cycle_path,
                        });
                    }
                    match &parent.parent_id {
                        Some(next_parent) => current_parent_id = next_parent.clone(),
                        None => break,
                    }
                }
            }
        }

        Ok(())
    }

    /// Return the rational `FrameRate` representation of this composition's frame rate.
    pub fn frame_rate_info(&self) -> FrameRate {
        FrameRate::from_fps(self.frame_rate)
    }

    /// Create an initialized `PlaybackClock` / `Transport` for this composition.
    pub fn clock(&self) -> PlaybackClock {
        PlaybackClock::from_composition(self)
    }

    /// Collect all unique keyframe timestamps across all layers and properties.
    pub fn all_keyframe_times(&self) -> Vec<TimeCode> {
        let mut times = Vec::new();
        for layer in &self.layers {
            for kf in &layer.opacity.keyframes {
                times.push(kf.time);
            }
            for kf in &layer.transform.anchor_point.keyframes {
                times.push(kf.time);
            }
            for kf in &layer.transform.position.keyframes {
                times.push(kf.time);
            }
            for kf in &layer.transform.scale.keyframes {
                times.push(kf.time);
            }
            for kf in &layer.transform.rotation.keyframes {
                times.push(kf.time);
            }
            if let LayerSource::Shape { shape_type } = &layer.source {
                match shape_type {
                    ShapeType::Rectangle {
                        width,
                        height,
                        corner_radius,
                    } => {
                        for kf in &width.keyframes {
                            times.push(kf.time);
                        }
                        for kf in &height.keyframes {
                            times.push(kf.time);
                        }
                        for kf in &corner_radius.keyframes {
                            times.push(kf.time);
                        }
                    }
                    ShapeType::Ellipse { radius_x, radius_y } => {
                        for kf in &radius_x.keyframes {
                            times.push(kf.time);
                        }
                        for kf in &radius_y.keyframes {
                            times.push(kf.time);
                        }
                    }
                    ShapeType::Path { .. } => {}
                }
            }
        }
        times.sort_by_key(|a| a.frames());
        times.dedup_by(|a, b| a.frames() == b.frames());
        times
    }

    /// Collect all unique marker timestamps on the composition and all layers.
    pub fn all_marker_times(&self) -> Vec<TimeCode> {
        let mut times = Vec::new();
        for m in &self.markers {
            times.push(m.time);
        }
        for layer in &self.layers {
            for m in &layer.markers {
                times.push(m.time);
            }
        }
        times.sort_by_key(|a| a.frames());
        times.dedup_by(|a, b| a.frames() == b.frames());
        times
    }
}
