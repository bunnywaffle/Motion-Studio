use crate::blend_mode::BlendMode;
use crate::color::Color;
use crate::error::ValidationError;
use crate::marker::Marker;
use crate::matte::TrackMatteMode;
use crate::property::Property;
use crate::timecode::TimeCode;
use crate::transform::Transform;
use serde::{Deserialize, Serialize};

const fn default_true() -> bool {
    true
}

/// Geometric shape types supported by vector shape layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum ShapeType {
    Rectangle {
        width: Property<f32>,
        height: Property<f32>,
        corner_radius: Property<f32>,
    },
    Ellipse {
        radius_x: Property<f32>,
        radius_y: Property<f32>,
    },
    Path {
        path_data: String,
    },
}

/// The visual source and content type backing a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayerSource {
    Solid {
        color: Color,
        width: u32,
        height: u32,
    },
    Image {
        asset_id: String,
    },
    Video {
        asset_id: String,
        media_start: TimeCode,
    },
    Text {
        text: Property<String>,
        font_family: String,
        font_size: Property<f32>,
        fill_color: Property<Color>,
    },
    Shape {
        shape_type: ShapeType,
    },
    NestedComposition {
        composition_id: String,
    },
    Procedural {
        generator_type: String,
    },
}

impl LayerSource {
    /// Return the canonical type name of the layer source.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Solid { .. } => "Solid",
            Self::Image { .. } => "Image",
            Self::Video { .. } => "Video",
            Self::Text { .. } => "Text",
            Self::Shape { .. } => "Shape",
            Self::NestedComposition { .. } => "NestedComposition",
            Self::Procedural { .. } => "Procedural",
        }
    }
}

/// An individual layer on the timeline and composition canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: String,
    pub name: String,
    pub source: LayerSource,
    pub transform: Transform,
    pub opacity: Property<f32>,
    pub blend_mode: BlendMode,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub matte_mode: TrackMatteMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matte_layer_id: Option<String>,
    pub in_point: TimeCode,
    pub out_point: TimeCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}

impl Layer {
    /// Create a new layer with default transform, 100% opacity, normal blend mode, and visible.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        source: LayerSource,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            source,
            transform: Transform::default(),
            opacity: Property::new("Opacity", 100.0),
            blend_mode: BlendMode::Normal,
            visible: true,
            locked: false,
            solo: false,
            matte_mode: TrackMatteMode::None,
            matte_layer_id: None,
            in_point,
            out_point,
            parent_id: None,
            markers: Vec::new(),
        }
    }

    /// Factory for creating a Solid layer.
    pub fn solid(
        id: impl Into<String>,
        name: impl Into<String>,
        color: Color,
        width: u32,
        height: u32,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self::new(
            id,
            name,
            LayerSource::Solid {
                color,
                width,
                height,
            },
            in_point,
            out_point,
        )
    }

    /// Factory for creating an Image layer referencing an asset.
    pub fn image(
        id: impl Into<String>,
        name: impl Into<String>,
        asset_id: impl Into<String>,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self::new(
            id,
            name,
            LayerSource::Image {
                asset_id: asset_id.into(),
            },
            in_point,
            out_point,
        )
    }

    /// Factory for creating a Video layer referencing an asset.
    pub fn video(
        id: impl Into<String>,
        name: impl Into<String>,
        asset_id: impl Into<String>,
        media_start: TimeCode,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self::new(
            id,
            name,
            LayerSource::Video {
                asset_id: asset_id.into(),
                media_start,
            },
            in_point,
            out_point,
        )
    }

    /// Factory for creating a Text layer.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        id: impl Into<String>,
        name: impl Into<String>,
        text: impl Into<String>,
        font_family: impl Into<String>,
        font_size: f32,
        fill_color: Color,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self::new(
            id,
            name,
            LayerSource::Text {
                text: Property::new("Source Text", text.into()),
                font_family: font_family.into(),
                font_size: Property::new("Font Size", font_size),
                fill_color: Property::new("Fill Color", fill_color),
            },
            in_point,
            out_point,
        )
    }

    /// Factory for creating a Shape layer.
    pub fn shape(
        id: impl Into<String>,
        name: impl Into<String>,
        shape_type: ShapeType,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self::new(id, name, LayerSource::Shape { shape_type }, in_point, out_point)
    }

    /// Factory for creating a Nested Composition (pre-comp) layer.
    pub fn nested_composition(
        id: impl Into<String>,
        name: impl Into<String>,
        composition_id: impl Into<String>,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self::new(
            id,
            name,
            LayerSource::NestedComposition {
                composition_id: composition_id.into(),
            },
            in_point,
            out_point,
        )
    }

    /// Factory for creating a Procedural generator layer.
    pub fn procedural(
        id: impl Into<String>,
        name: impl Into<String>,
        generator_type: impl Into<String>,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self::new(
            id,
            name,
            LayerSource::Procedural {
                generator_type: generator_type.into(),
            },
            in_point,
            out_point,
        )
    }

    /// Check if this layer is active at the specified timecode (in_point <= time < out_point).
    pub fn is_active_at(&self, time: &TimeCode) -> bool {
        time.frames() >= self.in_point.frames() && time.frames() < self.out_point.frames()
    }

    /// Return the layer duration in frames.
    pub fn duration_frames(&self) -> i64 {
        self.out_point.frames() - self.in_point.frames()
    }

    /// Return the layer duration in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.out_point.seconds() - self.in_point.seconds()
    }

    /// Update layer timing, validating that `in_point <= out_point`.
    pub fn set_timing(
        &mut self,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Result<(), ValidationError> {
        if in_point.frames() > out_point.frames() {
            return Err(ValidationError::InvalidLayerTiming {
                layer_id: self.id.clone(),
                in_point_frames: in_point.frames(),
                out_point_frames: out_point.frames(),
            });
        }
        self.in_point = in_point;
        self.out_point = out_point;
        Ok(())
    }

    /// Set the parent layer ID.
    pub fn set_parent(&mut self, parent_id: Option<impl Into<String>>) {
        self.parent_id = parent_id.map(Into::into);
    }

    /// Add a marker to this layer.
    pub fn add_marker(&mut self, marker: Marker) {
        self.markers.push(marker);
    }

    /// Return the asset ID referenced by this layer's source, if any.
    pub fn referenced_asset_id(&self) -> Option<&str> {
        match &self.source {
            LayerSource::Image { asset_id } => Some(asset_id),
            LayerSource::Video { asset_id, .. } => Some(asset_id),
            _ => None,
        }
    }

    /// Return the composition ID referenced if this layer is a nested composition.
    pub fn referenced_composition_id(&self) -> Option<&str> {
        match &self.source {
            LayerSource::NestedComposition { composition_id } => Some(composition_id),
            _ => None,
        }
    }

    /// Check if the layer is currently visible.
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Check if the layer is currently locked.
    pub const fn is_locked(&self) -> bool {
        self.locked
    }

    /// Check if the layer has solo enabled.
    pub const fn is_solo(&self) -> bool {
        self.solo
    }

    /// Set whether the layer is soloed.
    pub fn set_solo(&mut self, solo: bool) {
        self.solo = solo;
    }

    /// Builder to set solo mode.
    pub fn with_solo(mut self, solo: bool) -> Self {
        self.solo = solo;
        self
    }

    /// Check if track matte is enabled on this layer.
    pub const fn has_matte(&self) -> bool {
        self.matte_mode.is_enabled()
    }

    /// Configure track matte mode and optional matte layer ID.
    pub fn set_matte(&mut self, mode: TrackMatteMode, matte_layer_id: Option<impl Into<String>>) {
        self.matte_mode = mode;
        self.matte_layer_id = matte_layer_id.map(Into::into);
    }

    /// Builder to set track matte mode and optional matte layer ID.
    pub fn with_matte(mut self, mode: TrackMatteMode, matte_layer_id: Option<impl Into<String>>) -> Self {
        self.set_matte(mode, matte_layer_id);
        self
    }

    /// Check if the layer is a solid color layer.
    pub fn is_solid(&self) -> bool {
        matches!(self.source, LayerSource::Solid { .. })
    }

    /// Check if the layer is an image asset layer.
    pub fn is_image(&self) -> bool {
        matches!(self.source, LayerSource::Image { .. })
    }

    /// Check if the layer is a video asset layer.
    pub fn is_video(&self) -> bool {
        matches!(self.source, LayerSource::Video { .. })
    }

    /// Check if the layer is a text layer.
    pub fn is_text(&self) -> bool {
        matches!(self.source, LayerSource::Text { .. })
    }

    /// Check if the layer is a vector shape layer.
    pub fn is_shape(&self) -> bool {
        matches!(self.source, LayerSource::Shape { .. })
    }

    /// Check if the layer is a nested composition (pre-comp) layer.
    pub fn is_nested_composition(&self) -> bool {
        matches!(self.source, LayerSource::NestedComposition { .. })
    }

    /// Check if the layer is a procedural generator layer.
    pub fn is_procedural(&self) -> bool {
        matches!(self.source, LayerSource::Procedural { .. })
    }

    /// Validate the internal integrity of this layer.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.in_point.frames() > self.out_point.frames() {
            return Err(ValidationError::InvalidLayerTiming {
                layer_id: self.id.clone(),
                in_point_frames: self.in_point.frames(),
                out_point_frames: self.out_point.frames(),
            });
        }
        Ok(())
    }
}
