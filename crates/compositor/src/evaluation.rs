use crate::error::SceneGraphError;
use crate::graph::SceneGraph;
use crate::node::SceneNode;
use crate::transform::{AffineTransform2D, BoundingBox2D, EvaluatedTransform, TransformResolver};
use project::{
    BlendMode, Composition, LayerSource, LoopMode, Project, TimeCode, TrackMatteMode, Vec2,
};
use std::collections::HashSet;

/// The evaluated state and frame context of an inner composition nested inside a layer.
#[derive(Debug, Clone, PartialEq)]
pub struct NestedCompositionEvaluation {
    /// ID of the nested composition being evaluated.
    pub composition_id: String,
    /// The resolved local timecode inside the nested composition timeline.
    pub resolved_time: TimeCode,
    /// Canvas width of the nested composition.
    pub width: u32,
    /// Canvas height of the nested composition.
    pub height: u32,
    /// The evaluated layer stack of the inner composition at `resolved_time`.
    pub inner_stack: EvaluatedStack,
    /// World transform matrix of the nesting layer in parent/root coordinate space.
    pub nesting_world_matrix: AffineTransform2D,
}

impl NestedCompositionEvaluation {
    /// Compute the world transform matrix in root composition space for an inner layer.
    ///
    /// $M_{\text{inner\_root}} = M_{\text{nesting\_world\_matrix}} \times M_{\text{inner\_layer\_world}}$.
    pub fn inner_layer_world_matrix(&self, inner_layer_id: &str) -> Option<AffineTransform2D> {
        let inner_layer = self.inner_stack.get_layer(inner_layer_id)?;
        Some(self.nesting_world_matrix * inner_layer.world_matrix())
    }

    /// Map a 2D point from the nested composition's coordinate space to root composition space.
    pub fn inner_to_root_point(&self, point: Vec2) -> Vec2 {
        self.nesting_world_matrix.transform_point(point)
    }

    /// Map a 2D point from root composition space back into the nested composition's space.
    /// Returns `None` if the nesting transform is singular / non-invertible.
    pub fn root_to_inner_point(&self, point: Vec2) -> Option<Vec2> {
        self.nesting_world_matrix.transform_point_inverse(point)
    }

    /// Map a 2D bounding box from the nested composition's coordinate space to root composition space.
    pub fn inner_to_root_bbox(&self, bbox: &BoundingBox2D) -> BoundingBox2D {
        self.nesting_world_matrix.transform_bbox(bbox)
    }

    /// Calculate the axis-aligned bounding box of the entire nested composition canvas
    /// transformed into root composition coordinates.
    pub fn canvas_bounds_in_root(&self) -> BoundingBox2D {
        let canvas_bbox = BoundingBox2D::from_origin_size(
            Vec2::ZERO,
            Vec2::new(self.width as f32, self.height as f32),
        );
        self.nesting_world_matrix.transform_bbox(&canvas_bbox)
    }

    /// Calculate the root composition bounding box for a specific inner layer given its untransformed dimensions.
    pub fn inner_layer_root_bounds(
        &self,
        inner_layer_id: &str,
        width: f32,
        height: f32,
    ) -> Option<BoundingBox2D> {
        let inner_layer = self.inner_stack.get_layer(inner_layer_id)?;
        let inner_world_bbox = inner_layer.world_bounds(width, height);
        Some(self.inner_to_root_bbox(&inner_world_bbox))
    }
}

/// An item in the flattened composite render list representing an atomic visual layer
/// with its accumulated world transform and opacity resolved into root composition viewport space.
#[derive(Debug, Clone, PartialEq)]
pub struct FlattenedRenderLayer {
    /// ID of the composition directly containing this layer.
    pub composition_id: String,
    /// Hierarchical path of layer IDs from root down to this layer (e.g. `["comp_a_layer", "inner_layer"]`).
    pub layer_path: Vec<String>,
    /// Unique layer ID.
    pub layer_id: String,
    /// Layer display name.
    pub layer_name: String,
    /// Visual layer source.
    pub source: LayerSource,
    /// Final concatenated world transform matrix in root composition space:
    /// $M_{\text{root}} = M_{\text{nesting\_1}} \times ... \times M_{\text{layer}}$.
    pub root_world_matrix: AffineTransform2D,
    /// Combined effective opacity (product of all ancestor nesting opacities and local layer opacity).
    pub combined_opacity: f32,
    /// Layer blend mode.
    pub blend_mode: BlendMode,
    /// Nesting depth (0 = root composition, 1 = first-level nested comp, etc.).
    pub nesting_depth: usize,
    /// Track matte mode.
    pub matte_mode: TrackMatteMode,
    /// Track matte source layer ID.
    pub matte_source_id: Option<String>,
    /// Relative time offset in frames from layer in-point.
    pub time_offset_frames: i64,
    /// Relative time offset in seconds from layer in-point.
    pub time_offset_seconds: f64,
}

impl FlattenedRenderLayer {
    /// Compute root composition bounding box for this layer given its untransformed dimensions.
    pub fn root_bounds(&self, width: f32, height: f32) -> BoundingBox2D {
        let bbox = BoundingBox2D::from_origin_size(Vec2::ZERO, Vec2::new(width, height));
        self.root_world_matrix.transform_bbox(&bbox)
    }

    /// Map a local 2D point on this layer to root composition space.
    pub fn local_to_root_point(&self, point: Vec2) -> Vec2 {
        self.root_world_matrix.transform_point(point)
    }

    /// Map a root composition 2D point to this layer's local space.
    pub fn root_to_local_point(&self, point: Vec2) -> Option<Vec2> {
        self.root_world_matrix.transform_point_inverse(point)
    }
}

/// Descriptor for an offscreen composition render pass.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPassDescriptor {
    pub composition_id: String,
    pub width: u32,
    pub height: u32,
    pub time: TimeCode,
    pub world_matrix: AffineTransform2D,
    pub effective_opacity: f32,
    pub blend_mode: BlendMode,
    pub nesting_depth: usize,
    pub layer_ids: Vec<String>,
}

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
    pub transform: EvaluatedTransform,
    pub nested_composition: Option<Box<NestedCompositionEvaluation>>,
}

impl EvaluatedLayer {
    /// Return true if this layer participates in active rendering.
    pub fn is_rendered(&self) -> bool {
        self.is_visible && self.is_active && !self.is_matte_source && self.effective_opacity > 0.0
    }

    /// Return true if this layer references a nested composition.
    pub fn is_nested_composition(&self) -> bool {
        matches!(self.source, LayerSource::NestedComposition { .. })
    }

    /// Retrieve the nested composition evaluation, if present.
    pub fn nested_evaluation(&self) -> Option<&NestedCompositionEvaluation> {
        self.nested_composition.as_deref()
    }

    /// Return the evaluated local affine transform matrix for this layer.
    pub fn local_matrix(&self) -> AffineTransform2D {
        self.transform.local_matrix
    }

    /// Return the evaluated world affine transform matrix for this layer.
    pub fn world_matrix(&self) -> AffineTransform2D {
        self.transform.world_matrix
    }

    /// Map a local 2D point on this layer to world composition coordinates.
    pub fn local_to_world_point(&self, point: Vec2) -> Vec2 {
        self.transform.local_to_world_point(point)
    }

    /// Map a world 2D point to local coordinates for this layer. Returns `None` if non-invertible.
    pub fn world_to_local_point(&self, point: Vec2) -> Option<Vec2> {
        self.transform.world_to_local_point(point)
    }

    /// Map an axis-aligned bounding box from layer local coordinates to world coordinates.
    pub fn local_to_world_bbox(&self, bbox: &BoundingBox2D) -> BoundingBox2D {
        self.transform.local_to_world_bbox(bbox)
    }

    /// Map an axis-aligned bounding box from world coordinates to layer local coordinates.
    pub fn world_to_local_bbox(&self, bbox: &BoundingBox2D) -> Option<BoundingBox2D> {
        self.transform.world_to_local_bbox(bbox)
    }

    /// Calculate the world-space bounding box for this layer given its untransformed dimensions.
    pub fn world_bounds(&self, width: f32, height: f32) -> BoundingBox2D {
        self.transform.world_bounds(width, height)
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

    /// Retrieve the evaluated transform of a layer by ID, if present.
    pub fn get_transform(&self, id: &str) -> Option<&EvaluatedTransform> {
        self.get_layer(id).map(|l| &l.transform)
    }

    /// Return true if any layer in this evaluated stack contains a nested composition.
    pub fn has_nested_compositions(&self) -> bool {
        self.evaluated_layers
            .iter()
            .any(|l| l.nested_composition.is_some())
    }

    /// Retrieve references to all nested composition evaluations in this stack.
    pub fn nested_evaluations(&self) -> Vec<&NestedCompositionEvaluation> {
        self.evaluated_layers
            .iter()
            .filter_map(|l| l.nested_evaluation())
            .collect()
    }

    /// Retrieve the nested composition evaluation for a specific layer ID, if present.
    pub fn get_nested_evaluation(&self, layer_id: &str) -> Option<&NestedCompositionEvaluation> {
        self.get_layer(layer_id)?.nested_evaluation()
    }

    /// Recursively find an evaluated layer given a hierarchical path of layer IDs
    /// (e.g. `["nested_layer_b", "inner_layer_c"]`).
    pub fn get_layer_deep(&self, layer_path: &[&str]) -> Option<&EvaluatedLayer> {
        if layer_path.is_empty() {
            return None;
        }
        let first_id = layer_path[0];
        let layer = self.get_layer(first_id)?;
        if layer_path.len() == 1 {
            Some(layer)
        } else {
            let nested = layer.nested_composition.as_ref()?;
            nested.inner_stack.get_layer_deep(&layer_path[1..])
        }
    }

    /// Return the flattened composite render list for downstream rasterization/rendering.
    ///
    /// Layers are returned in painter's composite order (bottom to top).
    /// Nested composition layers are expanded into their inner layers in composite order,
    /// with world transforms concatenated ($M_{\text{root}} = M_{\text{nesting}} \times M_{\text{inner}}$)
    /// and effective opacities multiplied.
    pub fn flattened_render_list(&self) -> Vec<FlattenedRenderLayer> {
        self.flattened_render_list_internal(AffineTransform2D::IDENTITY, 1.0, 0, &[])
    }

    pub(crate) fn flattened_render_list_internal(
        &self,
        parent_matrix: AffineTransform2D,
        parent_opacity: f32,
        depth: usize,
        path_prefix: &[String],
    ) -> Vec<FlattenedRenderLayer> {
        let mut result = Vec::new();

        // Composite order: render_list contains layer IDs from bottom to top
        for layer_id in &self.render_list {
            if let Some(layer) = self.get_layer(layer_id) {
                let mut current_path = path_prefix.to_vec();
                current_path.push(layer.id.clone());

                let current_world_matrix = parent_matrix * layer.world_matrix();
                let current_opacity = parent_opacity * layer.effective_opacity;

                if let Some(ref nested) = layer.nested_composition {
                    let inner_layers = nested.inner_stack.flattened_render_list_internal(
                        current_world_matrix,
                        current_opacity,
                        depth + 1,
                        &current_path,
                    );
                    result.extend(inner_layers);
                } else {
                    result.push(FlattenedRenderLayer {
                        composition_id: self.composition_id.clone(),
                        layer_path: current_path,
                        layer_id: layer.id.clone(),
                        layer_name: layer.name.clone(),
                        source: layer.source.clone(),
                        root_world_matrix: current_world_matrix,
                        combined_opacity: current_opacity,
                        blend_mode: layer.blend_mode,
                        nesting_depth: depth,
                        matte_mode: layer.matte_mode,
                        matte_source_id: layer.matte_source_id.clone(),
                        time_offset_frames: layer.time_offset_frames,
                        time_offset_seconds: layer.time_offset_seconds,
                    });
                }
            }
        }

        result
    }

    /// Collect all composition render passes in bottom-up dependency order.
    pub fn collect_render_passes(&self) -> Vec<RenderPassDescriptor> {
        let mut passes = Vec::new();
        self.collect_render_passes_internal(
            AffineTransform2D::IDENTITY,
            1.0,
            BlendMode::Normal,
            0,
            &mut passes,
        );
        passes
    }

    fn collect_render_passes_internal(
        &self,
        world_matrix: AffineTransform2D,
        opacity: f32,
        blend_mode: BlendMode,
        depth: usize,
        passes: &mut Vec<RenderPassDescriptor>,
    ) {
        // Collect nested children passes first (leaf-first dependency order)
        for layer_id in &self.render_list {
            if let Some(layer) = self.get_layer(layer_id) {
                if let Some(ref nested) = layer.nested_composition {
                    let child_world = world_matrix * layer.world_matrix();
                    let child_opacity = opacity * layer.effective_opacity;
                    nested.inner_stack.collect_render_passes_internal(
                        child_world,
                        child_opacity,
                        layer.blend_mode,
                        depth + 1,
                        passes,
                    );
                }
            }
        }

        // Add this composition pass
        passes.push(RenderPassDescriptor {
            composition_id: self.composition_id.clone(),
            width: self.width,
            height: self.height,
            time: self.time,
            world_matrix,
            effective_opacity: opacity,
            blend_mode,
            nesting_depth: depth,
            layer_ids: self.render_list.clone(),
        });
    }
}

/// Resolve the local time in a nested composition given the nesting layer node, parent time, and target composition.
pub fn resolve_nested_time(
    node: &SceneNode,
    parent_time: &TimeCode,
    nested_comp: &Composition,
) -> TimeCode {
    if let Some(ref remapping) = node.time_remapping {
        // Time remapping enabled: property value defines local seconds
        let local_seconds = remapping.evaluate_at(parent_time);
        clamp_or_loop_seconds(
            local_seconds,
            nested_comp.duration.seconds(),
            node.loop_mode,
            nested_comp.frame_rate,
        )
    } else {
        // Frame rate alignment
        let same_rate = (parent_time.frame_rate() - nested_comp.frame_rate).abs() < 1e-6;
        if same_rate {
            let elapsed_frames = parent_time.frames() - node.in_point.frames();
            let offset_frames = node.start_offset.map(|tc| tc.frames()).unwrap_or(0);
            let raw_frames = (elapsed_frames as f64) * node.time_stretch + (offset_frames as f64);
            let target_frames = raw_frames.round() as i64;
            let duration_frames = nested_comp.duration.frames();

            let final_frames = if duration_frames <= 0 {
                0
            } else {
                match node.loop_mode {
                    LoopMode::Once => target_frames.clamp(0, duration_frames),
                    LoopMode::Loop => target_frames.rem_euclid(duration_frames),
                    LoopMode::PingPong => {
                        let period = 2 * duration_frames;
                        let rem = target_frames.rem_euclid(period);
                        if rem <= duration_frames {
                            rem
                        } else {
                            period - rem
                        }
                    }
                }
            };
            TimeCode::from_frames(final_frames, nested_comp.frame_rate)
        } else {
            let elapsed_seconds = parent_time.seconds() - node.in_point.seconds();
            let offset_seconds = node.start_offset.map(|tc| tc.seconds()).unwrap_or(0.0);
            let target_seconds = elapsed_seconds * node.time_stretch + offset_seconds;
            clamp_or_loop_seconds(
                target_seconds,
                nested_comp.duration.seconds(),
                node.loop_mode,
                nested_comp.frame_rate,
            )
        }
    }
}

fn clamp_or_loop_seconds(
    target_seconds: f64,
    duration_seconds: f64,
    loop_mode: LoopMode,
    frame_rate: f64,
) -> TimeCode {
    if duration_seconds <= 0.0 {
        return TimeCode::zero(frame_rate);
    }
    let resolved_sec = match loop_mode {
        LoopMode::Once => target_seconds.clamp(0.0, duration_seconds),
        LoopMode::Loop => {
            let rem = target_seconds % duration_seconds;
            if rem < 0.0 {
                rem + duration_seconds
            } else {
                rem
            }
        }
        LoopMode::PingPong => {
            let period = 2.0 * duration_seconds;
            let rem = target_seconds % period;
            let positive_rem = if rem < 0.0 { rem + period } else { rem };
            if positive_rem <= duration_seconds {
                positive_rem
            } else {
                period - positive_rem
            }
        }
    };
    TimeCode::from_seconds(resolved_sec, frame_rate)
}

/// The layer stack evaluation pipeline executor.
///
/// Evaluates composition layers at a given timeline position, applying:
/// - In/Out boundary conditions (half-open interval `[in_point, out_point)`)
/// - Solo filtering (when any layer is soloed, only soloed layers and their matte sources are visible)
/// - Track matte pairing and consumption
/// - Effective opacity clamping and normalization
/// - Relative frame and second timing offsets
/// - Recursive nested composition evaluation with temporal alignment, time stretch, spatial matrix concatenation, and cycle protection
#[derive(Debug, Clone)]
pub struct LayerStackEvaluator {
    /// If true, layers consumed as track mattes are excluded from the main composite render list.
    pub consume_matte_sources: bool,
    /// Maximum allowed nested composition depth to prevent infinite recursion.
    pub max_nesting_depth: usize,
}

impl Default for LayerStackEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl LayerStackEvaluator {
    /// Default maximum nesting depth (industry standard 32 levels).
    pub const DEFAULT_MAX_NESTING_DEPTH: usize = 32;

    /// Create a standard evaluator with matte source consumption enabled and default max nesting depth (32).
    pub fn new() -> Self {
        Self {
            consume_matte_sources: true,
            max_nesting_depth: Self::DEFAULT_MAX_NESTING_DEPTH,
        }
    }

    /// Set the maximum nesting depth allowed during recursive pre-comp evaluation.
    pub fn with_max_nesting_depth(mut self, max_depth: usize) -> Self {
        self.max_nesting_depth = max_depth;
        self
    }

    /// Set whether matte sources are consumed from the composite render list.
    pub fn with_consume_matte_sources(mut self, consume: bool) -> Self {
        self.consume_matte_sources = consume;
        self
    }

    /// Evaluate the scene graph at the given timecode without project context
    /// (nested composition sources evaluate to `None`).
    pub fn evaluate(&self, graph: &SceneGraph, time: &TimeCode) -> EvaluatedStack {
        let mut visited = vec![graph.composition_id.clone()];
        self.evaluate_internal(
            graph,
            None,
            time,
            0,
            &mut visited,
            AffineTransform2D::IDENTITY,
        )
        .unwrap_or_else(|_| EvaluatedStack {
            time: *time,
            composition_id: graph.composition_id.clone(),
            width: graph.width,
            height: graph.height,
            frame_rate: graph.frame_rate,
            has_solo: false,
            evaluated_layers: Vec::new(),
            render_list: Vec::new(),
        })
    }

    /// Evaluate a composition within a Project context, recursively resolving nested compositions
    /// with temporal alignment, time stretch/speed factors, spatial matrix concatenation, and cycle protection.
    pub fn evaluate_with_project(
        &self,
        graph: &SceneGraph,
        project: &Project,
        time: &TimeCode,
    ) -> Result<EvaluatedStack, SceneGraphError> {
        let mut visited = vec![graph.composition_id.clone()];
        self.evaluate_internal(
            graph,
            Some(project),
            time,
            0,
            &mut visited,
            AffineTransform2D::IDENTITY,
        )
    }

    /// Evaluate a composition by its ID within a Project context.
    pub fn evaluate_composition(
        &self,
        project: &Project,
        comp_id: &str,
        time: &TimeCode,
    ) -> Result<EvaluatedStack, SceneGraphError> {
        let graph = SceneGraph::from_project(project, comp_id)?;
        self.evaluate_with_project(&graph, project, time)
    }

    fn evaluate_internal(
        &self,
        graph: &SceneGraph,
        project_opt: Option<&Project>,
        time: &TimeCode,
        depth: usize,
        visited_comps: &mut Vec<String>,
        parent_world_matrix: AffineTransform2D,
    ) -> Result<EvaluatedStack, SceneGraphError> {
        let stack_nodes = graph.layer_stack_order();

        // 1. Detect if any layer in the stack has solo enabled
        let any_solo = stack_nodes.iter().any(|n| n.is_solo());

        // 2. Identify track matte relationships
        let mut matte_sources_set = HashSet::new();
        let mut layer_matte_pairs = Vec::with_capacity(stack_nodes.len());

        for (idx, node) in stack_nodes.iter().enumerate() {
            let mut resolved_matte_source: Option<String> = None;

            if node.has_matte() {
                if let Some(ref explicit_id) = node.matte_layer_id {
                    if graph.get_node(explicit_id).is_some() && explicit_id != &node.id {
                        resolved_matte_source = Some(explicit_id.clone());
                    }
                } else if idx > 0 {
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
                    if let Some(ref matte_id) = layer_matte_pairs[idx] {
                        solo_eligible_set.insert(matte_id.clone());
                    }
                }
            }
        }

        // 4. Resolve hierarchical transforms using topological evaluation order at target timecode
        let transforms = TransformResolver::resolve_scene_graph_at(graph, time).unwrap_or_default();

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

            let local_opacity = node.opacity.evaluate_at(time).clamp(0.0, 100.0);
            let effective_opacity = if is_visible {
                local_opacity / 100.0
            } else {
                0.0
            };

            let is_matte_source = matte_sources_set.contains(&node.id);
            let matte_source_id = layer_matte_pairs[idx].clone();

            let time_offset_frames = time.frames() - node.in_point.frames();
            let time_offset_seconds = time.seconds() - node.in_point.seconds();

            let transform = transforms
                .get(&node.id)
                .copied()
                .unwrap_or_else(|| EvaluatedTransform::from_node_transform_at(&node.transform, time));

            // Nested composition recursive evaluation
            let mut nested_composition = None;
            if let LayerSource::NestedComposition { ref composition_id } = node.source {
                if is_active {
                    if let Some(project) = project_opt {
                        // Recursion depth guard
                        if depth >= self.max_nesting_depth {
                            return Err(SceneGraphError::MaxNestingDepthExceeded {
                                depth: depth + 1,
                                max_depth: self.max_nesting_depth,
                            });
                        }

                        // Circular nesting guard
                        if visited_comps.contains(composition_id) {
                            let mut cycle = visited_comps.clone();
                            cycle.push(composition_id.clone());
                            return Err(SceneGraphError::CircularNestedComposition {
                                composition_id: composition_id.clone(),
                                cycle,
                            });
                        }

                        // Lookup child composition
                        let nested_comp = project.get_composition(composition_id).ok_or_else(|| {
                            SceneGraphError::CompositionNotFound(composition_id.clone())
                        })?;

                        // Resolve local timecode in nested composition
                        let resolved_time = resolve_nested_time(node, time, nested_comp);

                        // Build child scene graph
                        let nested_graph = SceneGraph::from_composition(nested_comp)?;

                        // Concatenate world transform matrix
                        let nesting_world_matrix = parent_world_matrix * transform.world_matrix;

                        // Recurse into child composition
                        visited_comps.push(composition_id.clone());
                        let inner_stack = self.evaluate_internal(
                            &nested_graph,
                            Some(project),
                            &resolved_time,
                            depth + 1,
                            visited_comps,
                            nesting_world_matrix,
                        )?;
                        visited_comps.pop();

                        nested_composition = Some(Box::new(NestedCompositionEvaluation {
                            composition_id: composition_id.clone(),
                            resolved_time,
                            width: nested_comp.width,
                            height: nested_comp.height,
                            inner_stack,
                            nesting_world_matrix,
                        }));
                    }
                }
            }

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
                transform,
                nested_composition,
            });
        }

        // 5. Generate painter's composite render list (bottom-to-top order)
        let composite_nodes = graph.composite_order();
        let mut render_list = Vec::with_capacity(composite_nodes.len());

        for node in composite_nodes {
            if let Some(eval_layer) = evaluated_layers.iter().find(|l| l.id == node.id) {
                let should_render = eval_layer.is_visible
                    && eval_layer.is_active
                    && (!self.consume_matte_sources || !eval_layer.is_matte_source);

                if should_render {
                    render_list.push(node.id.clone());
                }
            }
        }

        Ok(EvaluatedStack {
            time: *time,
            composition_id: graph.composition_id.clone(),
            width: graph.width,
            height: graph.height,
            frame_rate: graph.frame_rate,
            has_solo: any_solo,
            evaluated_layers,
            render_list,
        })
    }
}
