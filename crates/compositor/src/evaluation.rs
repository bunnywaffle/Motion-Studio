use crate::graph::SceneGraph;
use project::{BlendMode, LayerSource, TimeCode, TrackMatteMode};
use std::collections::HashSet;

/// The evaluated state of a single layer at a specific timeline position.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedLayer {
    pub id: String,
    pub name: String,
    pub layer_index: usize,
    pub source: LayerSource,
    pub blend_mode: BlendMode,
    pub is_active: bool,
    pub is_visible: bool,
    pub is_solo: bool,
    pub local_opacity: f32,
    pub effective_opacity: f32,
    pub matte_mode: TrackMatteMode,
    pub matte_source_id: Option<String>,
    pub is_matte_source: bool,
    pub time_offset_frames: i64,
    pub time_offset_seconds: f64,
    pub parent_id: Option<String>,
}

impl EvaluatedLayer {
    /// Return true if this layer participates in active rendering.
    pub fn is_rendered(&self) -> bool {
        self.is_visible && self.is_active && !self.is_matte_source && self.effective_opacity > 0.0
    }
}

/// The evaluated frame state of an entire composition stack at a specific point in time.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedStack {
    pub time: TimeCode,
    pub composition_id: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub has_solo: bool,
    pub evaluated_layers: Vec<EvaluatedLayer>,
    pub render_list: Vec<String>,
}

impl EvaluatedStack {
    /// Find an evaluated layer by its ID.
    pub fn get_layer(&self, id: &str) -> Option<&EvaluatedLayer> {
        self.evaluated_layers.iter().find(|l| l.id == id)
    }

    /// Return all layers that are active at this timecode (within in/out points).
    pub fn active_layers(&self) -> Vec<&EvaluatedLayer> {
        self.evaluated_layers.iter().filter(|l| l.is_active).collect()
    }

    /// Return all layers that are visible and active at this timecode.
    pub fn visible_layers(&self) -> Vec<&EvaluatedLayer> {
        self.evaluated_layers
            .iter()
            .filter(|l| l.is_visible && l.is_active)
            .collect()
    }

    /// Return the evaluated layers in painter's composite render order (bottom to top).
    pub fn render_layers(&self) -> Vec<&EvaluatedLayer> {
        self.render_list
            .iter()
            .filter_map(|id| self.get_layer(id))
            .collect()
    }

    /// Return the total number of active layers.
    pub fn active_count(&self) -> usize {
        self.evaluated_layers.iter().filter(|l| l.is_active).count()
    }

    /// Return the count of layers scheduled for rendering in this frame.
    pub fn render_count(&self) -> usize {
        self.render_list.len()
    }

    /// Return true if any layer in the stack is currently soloed.
    pub fn has_solo(&self) -> bool {
        self.has_solo
    }
}

/// The layer stack evaluation pipeline executor.
///
/// Evaluates composition layers at a given timeline position, applying:
/// - In/Out boundary conditions (half-open interval `[in_point, out_point)`)
/// - Solo filtering (when any layer is soloed, only soloed layers and their matte sources are visible)
/// - Track matte pairing and consumption
/// - Effective opacity clamping and normalization
/// - Relative frame and second timing offsets
#[derive(Debug, Clone, Default)]
pub struct LayerStackEvaluator {
    /// If true, layers consumed as track mattes are excluded from the main composite render list.
    /// (Standard After Effects behavior: matte layers generate a mask, not direct visual output).
    pub consume_matte_sources: bool,
}

impl LayerStackEvaluator {
    /// Create a standard evaluator with matte source consumption enabled.
    pub fn new() -> Self {
        Self {
            consume_matte_sources: true,
        }
    }

    /// Evaluate the scene graph at the given timecode.
    pub fn evaluate(&self, graph: &SceneGraph, time: &TimeCode) -> EvaluatedStack {
        let stack_nodes = graph.layer_stack_order();

        // 1. Detect if any layer in the stack has solo enabled
        let any_solo = stack_nodes.iter().any(|n| n.is_solo());

        // 2. Identify track matte relationships
        // Map: layer_id -> matte_source_layer_id
        let mut matte_sources_set = HashSet::new();
        let mut layer_matte_pairs = Vec::with_capacity(stack_nodes.len());

        for (idx, node) in stack_nodes.iter().enumerate() {
            let mut resolved_matte_source: Option<String> = None;

            if node.has_matte() {
                if let Some(ref explicit_id) = node.matte_layer_id {
                    // Explicit track matte target
                    if graph.get_node(explicit_id).is_some() && explicit_id != &node.id {
                        resolved_matte_source = Some(explicit_id.clone());
                    }
                } else if idx > 0 {
                    // Adjacent track matte: layer immediately above in stack order
                    let adjacent_above = &stack_nodes[idx - 1];
                    resolved_matte_source = Some(adjacent_above.id.clone());
                }

                if let Some(ref source_id) = resolved_matte_source {
                    matte_sources_set.insert(source_id.clone());
                }
            }

            layer_matte_pairs.push(resolved_matte_source);
        }

        // 3. If solo is active, identify which matte sources belong to soloed layers
        let mut solo_eligible_set = HashSet::new();
        if any_solo {
            for (idx, node) in stack_nodes.iter().enumerate() {
                if node.is_solo() {
                    solo_eligible_set.insert(node.id.clone());
                    // Also make its matte source eligible so the matte is evaluated
                    if let Some(ref matte_id) = layer_matte_pairs[idx] {
                        solo_eligible_set.insert(matte_id.clone());
                    }
                }
            }
        }

        // 4. Evaluate each layer's state
        let mut evaluated_layers = Vec::with_capacity(stack_nodes.len());

        for (idx, node) in stack_nodes.iter().enumerate() {
            // Half-open interval [in_point, out_point)
            let is_active = time.frames() >= node.in_point.frames()
                && time.frames() < node.out_point.frames();

            let solo_eligible = if any_solo {
                solo_eligible_set.contains(&node.id)
            } else {
                true
            };

            let is_visible = is_active && node.visible && solo_eligible;

            let local_opacity = (*node.opacity.value()).clamp(0.0, 100.0);
            let effective_opacity = if is_visible {
                local_opacity / 100.0
            } else {
                0.0
            };

            let is_matte_source = matte_sources_set.contains(&node.id);
            let matte_source_id = layer_matte_pairs[idx].clone();

            let time_offset_frames = time.frames() - node.in_point.frames();
            let time_offset_seconds = time.seconds() - node.in_point.seconds();

            evaluated_layers.push(EvaluatedLayer {
                id: node.id.clone(),
                name: node.name.clone(),
                layer_index: node.layer_index,
                source: node.source.clone(),
                blend_mode: node.blend_mode,
                is_active,
                is_visible,
                is_solo: node.solo,
                local_opacity,
                effective_opacity,
                matte_mode: node.matte_mode,
                matte_source_id,
                is_matte_source,
                time_offset_frames,
                time_offset_seconds,
                parent_id: node.parent_id.clone(),
            });
        }

        // 5. Generate painter's composite render list (bottom-to-top order)
        // Composite order in SceneGraph is index (len-1) down to 0
        let composite_nodes = graph.composite_order();
        let mut render_list = Vec::with_capacity(composite_nodes.len());

        for node in composite_nodes {
            if let Some(eval_layer) = evaluated_layers.iter().find(|l| l.id == node.id) {
                // Determine if this layer should be in the direct composite pass:
                // Must be active and visible
                let should_render = eval_layer.is_visible
                    && eval_layer.is_active
                    && (!self.consume_matte_sources || !eval_layer.is_matte_source);

                if should_render {
                    render_list.push(node.id.clone());
                }
            }
        }

        EvaluatedStack {
            time: *time,
            composition_id: graph.composition_id.clone(),
            width: graph.width,
            height: graph.height,
            frame_rate: graph.frame_rate,
            has_solo: any_solo,
            evaluated_layers,
            render_list,
        }
    }
}
