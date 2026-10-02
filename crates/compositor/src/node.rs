use project::{
    BlendMode, Effect, Layer, LayerSource, LoopMode, Mask, Property, TimeCode, TrackMatteMode,
    Transform,
};

/// A node in the compositor scene graph, representing a layer with its transform, visual properties,
/// and hierarchical parenting connections.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneNode {
    pub id: String,
    pub name: String,
    pub layer_index: usize,
    pub source: LayerSource,
    pub transform: Transform,
    pub opacity: Property<f32>,
    pub blend_mode: BlendMode,
    pub visible: bool,
    pub locked: bool,
    pub solo: bool,
    pub matte_mode: TrackMatteMode,
    pub matte_layer_id: Option<String>,
    pub in_point: TimeCode,
    pub out_point: TimeCode,
    pub parent_id: Option<String>,
    pub children_ids: Vec<String>,
    pub start_offset: Option<TimeCode>,
    pub time_stretch: f64,
    pub time_remapping: Option<Property<f64>>,
    pub loop_mode: LoopMode,
    pub effects: Vec<Effect>,
    pub masks: Vec<Mask>,
    pub modifier_graphs: std::collections::HashMap<String, project::ModifierGraph>,
    pub property_links: std::collections::HashMap<String, project::PropertyLink>,
}

impl SceneNode {
    /// Create a SceneNode from an existing Layer and its composition layer stack index.
    pub fn from_layer(layer: &Layer, layer_index: usize) -> Self {
        Self {
            id: layer.id.clone(),
            name: layer.name.clone(),
            layer_index,
            source: layer.source.clone(),
            transform: layer.transform.clone(),
            opacity: layer.opacity.clone(),
            blend_mode: layer.blend_mode,
            visible: layer.visible,
            locked: layer.locked,
            solo: layer.solo,
            matte_mode: layer.matte_mode,
            matte_layer_id: layer.matte_layer_id.clone(),
            in_point: layer.in_point,
            out_point: layer.out_point,
            parent_id: layer.parent_id.clone(),
            children_ids: Vec::new(),
            start_offset: layer.start_offset,
            time_stretch: layer.time_stretch,
            time_remapping: layer.time_remapping.clone(),
            loop_mode: layer.loop_mode,
            effects: layer.effects.clone(),
            masks: layer.masks.clone(),
            modifier_graphs: layer.modifier_graphs.clone(),
            property_links: layer.property_links.clone(),
        }
    }

    /// Calculate the normalized progression factor ($0.0 \to 1.0$) of this node at a given timecode.
    pub fn progression_factor(&self, current_time: &TimeCode) -> f32 {
        let in_s = self.in_point.seconds();
        let out_s = self.out_point.seconds();
        let cur_s = current_time.seconds();
        let span = (out_s - in_s).max(1e-6);
        ((cur_s - in_s) / span).clamp(0.0, 1.0) as f32
    }

    /// Check if this node is active at the specified timecode (in_point <= time < out_point).
    pub fn is_active_at(&self, time: &TimeCode) -> bool {
        if (self.in_point.frame_rate() - time.frame_rate()).abs() < 1e-4
            && (self.out_point.frame_rate() - time.frame_rate()).abs() < 1e-4
        {
            time.frames() >= self.in_point.frames() && time.frames() < self.out_point.frames()
        } else {
            let t = time.seconds();
            t >= self.in_point.seconds() - 1e-6 && t < self.out_point.seconds() - 1e-6
        }
    }

    /// Check if this node is both active and marked visible at the specified timecode.
    pub fn is_visible_at(&self, time: &TimeCode) -> bool {
        self.visible && self.is_active_at(time)
    }

    /// Return true if this node has a parent node.
    pub fn has_parent(&self) -> bool {
        self.parent_id.is_some()
    }

    /// Return true if this node is a root node (no parent).
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    /// Return true if this node has no children.
    pub fn is_leaf(&self) -> bool {
        self.children_ids.is_empty()
    }

    /// Return true if this node has solo mode enabled.
    pub const fn is_solo(&self) -> bool {
        self.solo
    }

    /// Return true if track matte is active on this node.
    pub const fn has_matte(&self) -> bool {
        self.matte_mode.is_enabled()
    }
}
