use crate::blend_mode::BlendMode;
use crate::clock::LoopMode;
use crate::color::Color;
use crate::effect::Effect;
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

const fn default_speed_one() -> f64 {
    1.0
}

fn default_shape_fill() -> Color {
    Color::WHITE
}

fn default_no_gradient() -> Option<FillGradient> {
    None
}

/// One color stop on a fill gradient: normalized `offset` (0..1) along the
/// gradient axis plus the color at that position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

impl GradientStop {
    pub fn new(offset: f32, color: Color) -> Self {
        Self {
            offset: offset.clamp(0.0, 1.0),
            color,
        }
    }
}

/// Multi-stop linear fill gradient (After Effects-style fill): two or more
/// color stops interpolated along an axis rotated `angle` degrees
/// (0 = left-to-right, 90 = top-to-bottom).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FillGradient {
    #[serde(default)]
    pub stops: Vec<GradientStop>,
    #[serde(default)]
    pub angle: f32,
}

impl Default for FillGradient {
    fn default() -> Self {
        Self::two_color(Color::WHITE, Color::BLACK, 90.0)
    }
}

impl FillGradient {
    /// Two-stop gradient with stops pinned at 0 and 1.
    pub fn two_color(a: Color, b: Color, angle: f32) -> Self {
        Self {
            stops: vec![GradientStop::new(0.0, a), GradientStop::new(1.0, b)],
            angle,
        }
    }

    /// Stops sorted by offset (ascending). The stored order is the UI order;
    /// sampling always uses the sorted view so crossed stops blend correctly.
    pub fn sorted_stops(&self) -> Vec<&GradientStop> {
        let mut stops: Vec<&GradientStop> = self.stops.iter().collect();
        stops.sort_by(|a, b| {
            a.offset
                .partial_cmp(&b.offset)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        stops
    }

    /// Sample the gradient at normalized position `t` (clamped to 0..1).
    /// Fewer than two stops falls back to the single stop (or black).
    pub fn sample(&self, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let stops = self.sorted_stops();
        match stops.as_slice() {
            [] => Color::BLACK,
            [only] => only.color,
            _ => {
                if t <= stops[0].offset {
                    return stops[0].color;
                }
                for pair in stops.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    if t <= b.offset {
                        let span = (b.offset - a.offset).max(1e-6);
                        let f = ((t - a.offset) / span).clamp(0.0, 1.0);
                        return Color::rgba(
                            a.color.r + (b.color.r - a.color.r) * f,
                            a.color.g + (b.color.g - a.color.g) * f,
                            a.color.b + (b.color.b - a.color.b) * f,
                            a.color.a + (b.color.a - a.color.a) * f,
                        );
                    }
                }
                stops[stops.len() - 1].color
            }
        }
    }

    /// Insert a stop keeping offsets ascending. Returns the new stop index.
    pub fn add_stop(&mut self, offset: f32, color: Color) -> usize {
        let offset = offset.clamp(0.0, 1.0);
        let at = self
            .stops
            .iter()
            .position(|s| s.offset > offset)
            .unwrap_or(self.stops.len());
        self.stops.insert(at, GradientStop::new(offset, color));
        at
    }

    /// Move a stop to a new offset, re-sorting ascending. Returns the stop's
    /// new index (`None` for an out-of-range index).
    pub fn set_stop_offset(&mut self, index: usize, offset: f32) -> Option<usize> {
        if index >= self.stops.len() {
            return None;
        }
        let color = self.stops[index].color;
        self.stops.remove(index);
        Some(self.add_stop(offset, color))
    }

    /// Remove a stop (keeps at least… nothing enforced here; the UI enforces
    /// a 2-stop minimum). Returns the removed stop.
    pub fn remove_stop(&mut self, index: usize) -> Option<GradientStop> {
        if index < self.stops.len() {
            Some(self.stops.remove(index))
        } else {
            None
        }
    }

    /// Swap the stop order end-for-end (offsets mirrored).
    pub fn reversed(&self) -> Self {
        Self {
            stops: self
                .stops
                .iter()
                .map(|s| GradientStop::new(1.0 - s.offset, s.color))
                .collect(),
            angle: self.angle,
        }
    }
}

fn default_font_weight() -> u16 {
    400
}

fn default_stroke_color() -> Color {
    Color::BLACK
}

/// Horizontal paragraph alignment for text layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    JustifyLeft,
    JustifyCenter,
    JustifyRight,
    JustifyAll,
}

fn default_stroke_position() -> String {
    "center".to_string()
}

fn default_paint_order() -> String {
    "fill_over_stroke".to_string()
}

fn default_vertical_align() -> String {
    "top".to_string()
}

/// Geometric shape types supported by vector shape layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum ShapeType {
    Rectangle {
        width: Property<f32>,
        height: Property<f32>,
        corner_radius: Property<f32>,
        #[serde(default = "default_shape_fill")]
        fill: Color,
        /// Linear fill gradient (None = solid `fill`).
        #[serde(default = "default_no_gradient", skip_serializing_if = "Option::is_none")]
        fill_gradient: Option<FillGradient>,
    },
    Ellipse {
        radius_x: Property<f32>,
        radius_y: Property<f32>,
        #[serde(default = "default_shape_fill")]
        fill: Color,
        /// Linear fill gradient (None = solid `fill`).
        #[serde(default = "default_no_gradient", skip_serializing_if = "Option::is_none")]
        fill_gradient: Option<FillGradient>,
    },
    Path {
        path_data: String,
        #[serde(default = "default_shape_fill")]
        fill: Color,
        /// Linear fill gradient (None = solid `fill`).
        #[serde(default = "default_no_gradient", skip_serializing_if = "Option::is_none")]
        fill_gradient: Option<FillGradient>,
    },
}

/// The visual source and content type backing a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum LayerSource {
    Solid {
        color: Color,
        width: u32,
        height: u32,
        /// Linear fill gradient (None = solid `color`).
        #[serde(default = "default_no_gradient", skip_serializing_if = "Option::is_none")]
        fill_gradient: Option<FillGradient>,
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
        /// Linear fill gradient (None = solid `fill_color`).
        #[serde(default = "default_no_gradient", skip_serializing_if = "Option::is_none")]
        fill_gradient: Option<FillGradient>,
        /// Font weight 100..900 (400 normal, 700 bold).
        #[serde(default = "default_font_weight")]
        weight: u16,
        /// Faux italic slant when the family lacks an italic face.
        #[serde(default)]
        italic: bool,
        /// Extra inter-character advance in px (tracking).
        #[serde(default)]
        tracking: Property<f32>,
        /// Line height in px (0 = auto 1.2x size).
        #[serde(default)]
        leading: Property<f32>,
        #[serde(default)]
        align: TextAlign,
        /// Render uppercase glyphs.
        #[serde(default)]
        all_caps: bool,
        /// Outline width in px (0 = off).
        #[serde(default)]
        stroke_width: Property<f32>,
        #[serde(default = "default_stroke_color")]
        stroke_color: Color,
        /// Linear stroke gradient (None = solid `stroke_color`).
        #[serde(default = "default_no_gradient", skip_serializing_if = "Option::is_none")]
        stroke_gradient: Option<FillGradient>,
        /// Vertical glyph offset in px.
        #[serde(default)]
        baseline_shift: Property<f32>,
        /// Wrap width in px (0 = point text, no wrap).
        #[serde(default)]
        box_width: Property<f32>,
        /// Wrap box height in px (0 = auto).
        #[serde(default)]
        box_height: Property<f32>,
        #[serde(default)]
        underline: bool,
        #[serde(default)]
        small_caps: bool,
        #[serde(default)]
        superscript: bool,
        #[serde(default)]
        subscript: bool,
        #[serde(default = "default_stroke_position")]
        stroke_position: String,
        #[serde(default = "default_paint_order")]
        paint_order: String,
        #[serde(default = "default_vertical_align")]
        vertical_align: String,
        /// Optional baseline path: glyphs flow along it (pen on a text
        /// layer appends here). None = straight horizontal layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_path: Option<crate::path::Path>,
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
    Adjustment,
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
            Self::Adjustment => "Adjustment",
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<TimeCode>,
    #[serde(default = "default_speed_one")]
    pub time_stretch: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_remapping: Option<Property<f64>>,
    #[serde(default)]
    pub loop_mode: LoopMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// First-class vector masks, applied bottom-to-top in vec order
    /// (Path → Coverage → Feather/Expansion → Combination → Alpha).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<crate::mask::Mask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_color: Option<Color>,
    /// Interactive modifier graphs attached to properties on this layer,
    /// keyed by property path (e.g. "transform.rotation", "transform.position.x").
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub modifier_graphs: std::collections::HashMap<String, crate::modifier::ModifierGraph>,
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
            start_offset: None,
            time_stretch: 1.0,
            time_remapping: None,
            loop_mode: LoopMode::default(),
            effects: Vec::new(),
            masks: Vec::new(),
            label_color: None,
            modifier_graphs: std::collections::HashMap::new(),
        }
    }

    /// Calculate the normalized progression factor ($0.0 \to 1.0$) of this layer at a given timecode.
    pub fn progression_factor(&self, current_time: &TimeCode) -> f32 {
        let in_s = self.in_point.seconds();
        let out_s = self.out_point.seconds();
        let cur_s = current_time.seconds();
        let span = (out_s - in_s).max(1e-6);
        ((cur_s - in_s) / span).clamp(0.0, 1.0) as f32
    }

    /// Retrieve the modifier graph attached to a property path on this layer, if any.
    pub fn get_modifier_graph(&self, prop_path: &str) -> Option<&crate::modifier::ModifierGraph> {
        self.modifier_graphs.get(prop_path)
    }

    /// Set or update the modifier graph for a property path on this layer.
    pub fn set_modifier_graph(&mut self, prop_path: impl Into<String>, graph: crate::modifier::ModifierGraph) {
        self.modifier_graphs.insert(prop_path.into(), graph);
    }

    /// Remove the modifier graph for a property path on this layer.
    pub fn remove_modifier_graph(&mut self, prop_path: &str) -> Option<crate::modifier::ModifierGraph> {
        self.modifier_graphs.remove(prop_path)
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
                fill_gradient: None,
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

    /// Factory for creating an Image layer referencing an asset with known resolution.
    pub fn image_with_dimensions(
        id: impl Into<String>,
        name: impl Into<String>,
        asset_id: impl Into<String>,
        width: u32,
        height: u32,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        let mut layer = Self::image(id, name, asset_id, in_point, out_point);
        layer.transform.anchor_point.set_value(crate::vec2::Vec2::new(
            (width / 2) as f32,
            (height / 2) as f32,
        ));
        layer
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
                fill_gradient: None,
                weight: default_font_weight(),
                italic: false,
                tracking: Property::new("Tracking", 0.0),
                leading: Property::new("Leading", 0.0),
                align: TextAlign::default(),
                all_caps: false,
                stroke_width: Property::new("Stroke Width", 0.0),
                stroke_color: default_stroke_color(),
                stroke_gradient: None,
                baseline_shift: Property::new("Baseline Shift", 0.0),
                box_width: Property::new("Box Width", 0.0),
                box_height: Property::new("Box Height", 0.0),
                underline: false,
                small_caps: false,
                superscript: false,
                subscript: false,
                stroke_position: default_stroke_position(),
                paint_order: default_paint_order(),
                vertical_align: default_vertical_align(),
                text_path: None,
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

    /// Factory for creating an Adjustment layer.
    pub fn adjustment(
        id: impl Into<String>,
        name: impl Into<String>,
        in_point: TimeCode,
        out_point: TimeCode,
    ) -> Self {
        Self::new(id, name, LayerSource::Adjustment, in_point, out_point)
    }

    /// Check if this layer is an adjustment layer.
    pub fn is_adjustment(&self) -> bool {
        matches!(self.source, LayerSource::Adjustment)
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

    /// Return the After Effects-style label color for this layer.
    pub fn label_color(&self, index: usize) -> Color {
        if let Some(color) = self.label_color {
            return color;
        }
        match &self.source {
            LayerSource::Solid { color, .. } => *color,
            LayerSource::Image { .. } => Color::rgb(0.93, 0.45, 0.65), // Pink / Lavender
            LayerSource::Video { .. } => Color::rgb(0.24, 0.51, 0.96), // AE Royal Blue
            LayerSource::Text { .. } => Color::rgb(0.96, 0.58, 0.19),  // AE Orange
            LayerSource::Shape { .. } => Color::rgb(0.13, 0.77, 0.55), // AE Teal / Green
            LayerSource::NestedComposition { .. } => Color::rgb(0.68, 0.42, 0.88), // AE Purple
            LayerSource::Procedural { .. } => {
                const PALETTE: [Color; 6] = [
                    Color::rgb(0.93, 0.45, 0.65),
                    Color::rgb(0.24, 0.51, 0.96),
                    Color::rgb(0.96, 0.58, 0.19),
                    Color::rgb(0.13, 0.77, 0.55),
                    Color::rgb(0.68, 0.42, 0.88),
                    Color::rgb(0.92, 0.80, 0.20),
                ];
                PALETTE[index % PALETTE.len()]
            }
            LayerSource::Adjustment => Color::rgb(0.95, 0.45, 0.25), // AE Coral / Peach
        }
    }

    /// Set an explicit label color.
    pub fn set_label_color(&mut self, color: Option<Color>) {
        self.label_color = color;
    }

    /// Builder to set an explicit label color.
    pub fn with_label_color(mut self, color: Color) -> Self {
        self.label_color = Some(color);
        self
    }

    /// Builder to set track matte mode and optional matte layer ID.
    pub fn with_matte(mut self, mode: TrackMatteMode, matte_layer_id: Option<impl Into<String>>) -> Self {
        self.set_matte(mode, matte_layer_id);
        self
    }

    /// Set the start offset timecode.
    pub fn set_start_offset(&mut self, offset: Option<TimeCode>) {
        self.start_offset = offset;
    }

    /// Builder to set start offset timecode.
    pub fn with_start_offset(mut self, offset: TimeCode) -> Self {
        self.start_offset = Some(offset);
        self
    }

    /// Set the time stretch / speed multiplier.
    pub fn set_time_stretch(&mut self, time_stretch: f64) {
        self.time_stretch = time_stretch;
    }

    /// Builder to set time stretch / speed multiplier.
    pub fn with_time_stretch(mut self, time_stretch: f64) -> Self {
        self.time_stretch = time_stretch;
        self
    }

    /// Set the animated time remapping property.
    pub fn set_time_remapping(&mut self, time_remapping: Option<Property<f64>>) {
        self.time_remapping = time_remapping;
    }

    /// Builder to set animated time remapping property.
    pub fn with_time_remapping(mut self, time_remapping: Property<f64>) -> Self {
        self.time_remapping = Some(time_remapping);
        self
    }

    /// Set the loop mode.
    pub fn set_loop_mode(&mut self, loop_mode: LoopMode) {
        self.loop_mode = loop_mode;
    }

    /// Builder to set loop mode.
    pub fn with_loop_mode(mut self, loop_mode: LoopMode) -> Self {
        self.loop_mode = loop_mode;
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

    /// Add an effect to this layer's post-processing stack and return its ID.
    pub fn add_effect(&mut self, effect: Effect) -> String {
        let id = effect.id.clone();
        self.effects.push(effect);
        id
    }

    /// Remove an effect from this layer by its unique ID.
    pub fn remove_effect(&mut self, effect_id: &str) -> Option<Effect> {
        let pos = self.effects.iter().position(|e| e.id == effect_id)?;
        Some(self.effects.remove(pos))
    }

    /// Retrieve an immutable reference to an effect on this layer.
    pub fn get_effect(&self, effect_id: &str) -> Option<&Effect> {
        self.effects.iter().find(|e| e.id == effect_id)
    }

    /// Retrieve a mutable reference to an effect on this layer.
    pub fn get_effect_mut(&mut self, effect_id: &str) -> Option<&mut Effect> {
        self.effects.iter_mut().find(|e| e.id == effect_id)
    }

    /// Check if this layer has any effects.
    pub fn has_effects(&self) -> bool {
        !self.effects.is_empty()
    }

    /// Retrieve a mask on this layer by id.
    pub fn get_mask(&self, mask_id: &str) -> Option<&crate::mask::Mask> {
        self.masks.iter().find(|m| m.id == mask_id)
    }

    /// Retrieve a mutable reference to a mask on this layer.
    pub fn get_mask_mut(&mut self, mask_id: &str) -> Option<&mut crate::mask::Mask> {
        self.masks.iter_mut().find(|m| m.id == mask_id)
    }

    /// Check if this layer has any masks.
    pub fn has_masks(&self) -> bool {
        !self.masks.is_empty()
    }

    /// Check if this layer has any enabled masks.
    pub fn has_enabled_masks(&self) -> bool {
        self.masks.iter().any(|m| m.enabled)
    }

    /// Remove a mask by id. Returns the removed mask, if present.
    pub fn remove_mask(&mut self, mask_id: &str) -> Option<crate::mask::Mask> {
        let idx = self.masks.iter().position(|m| m.id == mask_id)?;
        Some(self.masks.remove(idx))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 1e-5
            && (a.g - b.g).abs() < 1e-5
            && (a.b - b.b).abs() < 1e-5
            && (a.a - b.a).abs() < 1e-5
    }

    #[test]
    fn fill_gradient_samples_endpoints_and_midpoints() {
        let g = FillGradient::two_color(Color::BLACK, Color::WHITE, 0.0);
        assert!(close(g.sample(0.0), Color::BLACK));
        assert!(close(g.sample(1.0), Color::WHITE));
        let mid = g.sample(0.5);
        assert!((mid.r - 0.5).abs() < 1e-5, "{mid:?}");
        // Clamped outside 0..1.
        assert!(close(g.sample(-2.0), Color::BLACK));
        assert!(close(g.sample(2.0), Color::WHITE));
    }

    #[test]
    fn fill_gradient_three_stops_interpolate_per_segment() {
        let g = FillGradient {
            stops: vec![
                GradientStop::new(0.0, Color::BLACK),
                GradientStop::new(0.5, Color::RED),
                GradientStop::new(1.0, Color::WHITE),
            ],
            angle: 90.0,
        };
        assert!(close(g.sample(0.5), Color::RED));
        let q = g.sample(0.25);
        assert!((q.r - 0.5).abs() < 1e-5 && q.g < 1e-5, "{q:?}");
        // Unsorted storage still samples in offset order.
        let mut shuffled = g.clone();
        shuffled.stops.swap(0, 2);
        assert!(close(shuffled.sample(0.5), Color::RED));
    }

    #[test]
    fn fill_gradient_edits_keep_sorted_order() {
        let mut g = FillGradient::two_color(Color::BLACK, Color::WHITE, 0.0);
        let at = g.add_stop(0.25, Color::RED);
        assert_eq!(at, 1);
        assert_eq!(g.stops.len(), 3);
        // Drag the last stop before the middle one: re-sorted, index follows.
        let at = g.set_stop_offset(2, 0.1).unwrap();
        assert_eq!(at, 1);
        assert!(close(g.stops[1].color, Color::WHITE));
        assert!(g.stops.windows(2).all(|w| w[0].offset <= w[1].offset));
        // Reversal mirrors offsets end-for-end.
        let rev = g.reversed();
        assert!(close(rev.sample(0.9), Color::WHITE), "{rev:?}");
        assert_eq!(g.remove_stop(5), None);
        assert!(g.remove_stop(0).is_some());
        assert_eq!(g.stops.len(), 2);
    }

    #[test]
    fn fill_gradient_empty_and_single_fall_back() {
        let g = FillGradient { stops: vec![], angle: 0.0 };
        assert!(close(g.sample(0.3), Color::BLACK));
        let g = FillGradient {
            stops: vec![GradientStop::new(0.7, Color::RED)],
            angle: 0.0,
        };
        assert!(close(g.sample(0.0), Color::RED));
    }
}
