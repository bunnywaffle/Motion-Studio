use crate::error::SceneGraphError;
use crate::graph::SceneGraph;
use crate::node::SceneNode;
use crate::transform::{AffineTransform2D, BoundingBox2D, EvaluatedTransform, TransformResolver};
use project::{
    BlendMode, Color, Composition, EffectType, LayerSource, LoopMode, Project, TimeCode,
    TrackMatteMode, Vec2,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
        let root_matrix = self.inner_layer_world_matrix(inner_layer_id)?;
        let local_bbox = BoundingBox2D::from_origin_size(Vec2::ZERO, Vec2::new(width, height));
        Some(root_matrix.transform_bbox(&local_bbox))
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
    /// Track matte source layer full hierarchical path, if applicable.
    pub matte_source_path: Option<Vec<String>>,
    /// True if this layer (or its ancestor nesting composition) is consumed as a track matte.
    pub is_matte_source: bool,
    /// Relative time offset in frames from layer in-point.
    pub time_offset_frames: i64,
    /// Relative time offset in seconds from layer in-point.
    pub time_offset_seconds: f64,
    /// Evaluated post-processing effects applied to this layer.
    pub effects: Vec<EvaluatedEffect>,
}

impl FlattenedRenderLayer {
    /// True when this entry is an After Effects-style adjustment layer.
    /// Adjustment entries carry no pixels of their own; their `effects`
    /// apply to the composite of the layers beneath them.
    pub fn is_adjustment(&self) -> bool {
        matches!(self.source, LayerSource::Adjustment)
    }

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
    /// ID of the layer nesting this composition (None for the root composition pass).
    pub nesting_layer_id: Option<String>,
    pub width: u32,
    pub height: u32,
    pub time: TimeCode,
    pub world_matrix: AffineTransform2D,
    pub effective_opacity: f32,
    pub blend_mode: BlendMode,
    pub nesting_depth: usize,
    pub layer_ids: Vec<String>,
}

/// Evaluated parameter values for an effect at a specific timecode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvaluatedEffectType {
    GaussianBlur {
        radius: f32,
    },
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
    Tint {
        map_black: Color,
        map_white: Color,
        amount: f32,
    },
    Invert {
        amount: f32,
    },
    DropShadow {
        distance: f32,
        angle: f32,
        softness: f32,
        opacity: f32,
        color: Color,
    },
    GlslShader {
        code: String,
        param1: f32,
        param2: f32,
        param3: f32,
        param4: f32,
    },
    DisplacementMap {
        max_horizontal: f32,
        max_vertical: f32,
    },
    ChromaKey {
        key_color: Color,
        tolerance: f32,
        feather: f32,
    },
    LumaKey {
        threshold: f32,
        feather: f32,
    },
    NoiseGenerator {
        amount: f32,
        monochrome: bool,
    },
    /// Runtime user-shader effect. Spatial (runs on the GPU over the whole
    /// tile), so [`Self::process_color`] is the identity; the renderer
    /// compiles `source` (see `renderer::shader_lab`) and uploads `values`
    /// as uniforms. `source_hash` keys the pipeline cache.
    ShaderLab {
        source_hash: u64,
        values: HashMap<String, project::ShaderParamValue>,
    },
}

impl EvaluatedEffectType {
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::BrightnessContrast { .. } => "Brightness & Contrast",
            Self::Tint { .. } => "Tint",
            Self::Invert { .. } => "Invert",
            Self::DropShadow { .. } => "Drop Shadow",
            Self::GlslShader { .. } => "Custom GLSL Shader",
            Self::DisplacementMap { .. } => "Displacement Map",
            Self::ChromaKey { .. } => "Chroma Key",
            Self::LumaKey { .. } => "Luma Key",
            Self::NoiseGenerator { .. } => "Noise Generator",
            Self::ShaderLab { .. } => "Shader Lab",
        }
    }

    /// True for effects that need neighboring pixels (currently Gaussian blur).
    /// Such effects are the identity in [`Self::process_color`] and must be
    /// resolved by the rasterizer / preview renderer instead.
    pub const fn is_spatial(&self) -> bool {
        matches!(self, Self::GaussianBlur { .. } | Self::ShaderLab { .. })
    }

    /// Blur radius in pixels when this is a Gaussian blur, otherwise `None`.
    pub const fn blur_radius(&self) -> Option<f32> {
        match self {
            Self::GaussianBlur { radius } => Some(*radius),
            _ => None,
        }
    }

    /// Visually process an input Color through this evaluated effect algorithm.
    ///
    /// NOTE: Gaussian blur is a *spatial* effect — it mixes neighboring pixels
    /// and therefore cannot change a single isolated color sample. It is
    /// intentionally the identity here; real diffusion happens in the
    /// rasterizer (`renderer::blur`, WGSL blur pipeline) and in the viewport
    /// preview (multi-tap sprite approximation). Use [`Self::is_spatial`] /
    /// [`Self::blur_radius`] to branch on it.
    pub fn process_color(&self, c: Color) -> Color {
        match self {
            Self::GaussianBlur { .. } => c,
            Self::BrightnessContrast { brightness, contrast } => {
                let b = *brightness / 100.0;
                let k = (1.0 + *contrast / 100.0).max(0.0);
                Color::rgba(
                    ((c.r - 0.5) * k + 0.5 + b).clamp(0.0, 1.0),
                    ((c.g - 0.5) * k + 0.5 + b).clamp(0.0, 1.0),
                    ((c.b - 0.5) * k + 0.5 + b).clamp(0.0, 1.0),
                    c.a,
                )
            }
            Self::Tint { map_black, map_white, amount } => {
                let t = (*amount / 100.0).clamp(0.0, 1.0);
                let lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
                let tr = map_black.r * (1.0 - lum) + map_white.r * lum;
                let tg = map_black.g * (1.0 - lum) + map_white.g * lum;
                let tb = map_black.b * (1.0 - lum) + map_white.b * lum;
                Color::rgba(
                    c.r * (1.0 - t) + tr * t,
                    c.g * (1.0 - t) + tg * t,
                    c.b * (1.0 - t) + tb * t,
                    c.a,
                )
            }
            Self::Invert { amount } => {
                let t = (*amount / 100.0).clamp(0.0, 1.0);
                Color::rgba(
                    c.r * (1.0 - t) + (1.0 - c.r) * t,
                    c.g * (1.0 - t) + (1.0 - c.g) * t,
                    c.b * (1.0 - t) + (1.0 - c.b) * t,
                    c.a,
                )
            }
            Self::DropShadow { color: _, opacity, .. } => {
                let op = (*opacity / 100.0).clamp(0.0, 1.0);
                Color::rgba(c.r, c.g, c.b, (c.a * (1.0 + 0.15 * op)).clamp(0.0, 1.0))
            }
            Self::GlslShader { param1: _, param2, param3, param4, .. } => {
                let gain = 1.0 + (*param2 / 100.0);
                let shift = *param3 / 100.0;
                let mod_alpha = if *param4 != 0.0 { (*param4 / 100.0).clamp(0.0, 1.0) } else { 1.0 };
                Color::rgba(
                    (c.r * gain + shift).clamp(0.0, 1.0),
                    (c.g * gain).clamp(0.0, 1.0),
                    (c.b * gain - shift * 0.5).clamp(0.0, 1.0),
                    (c.a * mod_alpha).clamp(0.0, 1.0),
                )
            }
            Self::DisplacementMap { max_horizontal, max_vertical } => {
                let shift_r = *max_horizontal * 0.002;
                let shift_b = *max_vertical * 0.002;
                Color::rgba(
                    (c.r * (1.0 + shift_r)).clamp(0.0, 1.0),
                    c.g,
                    (c.b * (1.0 - shift_b)).clamp(0.0, 1.0),
                    c.a,
                )
            }
            Self::ChromaKey { key_color, tolerance, feather } => {
                let dr = c.r - key_color.r;
                let dg = c.g - key_color.g;
                let db = c.b - key_color.b;
                let dist = (dr * dr + dg * dg + db * db).sqrt();
                let tol_threshold = (*tolerance / 100.0).max(0.01);
                let f_threshold = (*feather / 100.0).max(0.001);
                if dist < tol_threshold {
                    let alpha_mult = if dist < (tol_threshold - f_threshold).max(0.0) {
                        0.0
                    } else {
                        ((dist - (tol_threshold - f_threshold).max(0.0)) / f_threshold).clamp(0.0, 1.0)
                    };
                    Color::rgba(c.r, c.g, c.b, c.a * alpha_mult)
                } else {
                    c
                }
            }
            Self::LumaKey { threshold, feather } => {
                let lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
                let cut = (*threshold / 100.0).clamp(0.0, 1.0);
                let f = (*feather / 100.0).max(0.001);
                if lum < cut {
                    let alpha_mult = if lum < (cut - f).max(0.0) {
                        0.0
                    } else {
                        ((lum - (cut - f).max(0.0)) / f).clamp(0.0, 1.0)
                    };
                    Color::rgba(c.r, c.g, c.b, c.a * alpha_mult)
                } else {
                    c
                }
            }
            Self::NoiseGenerator { amount, monochrome } => {
                let n_amount = (*amount / 100.0).clamp(0.0, 1.0);
                if *monochrome {
                    let hash = ((c.r * 12.9898 + c.g * 78.233 + c.b * 45.164).sin() * 43758.5453).fract();
                    let noise = (hash - 0.5) * n_amount;
                    Color::rgba(
                        (c.r + noise).clamp(0.0, 1.0),
                        (c.g + noise).clamp(0.0, 1.0),
                        (c.b + noise).clamp(0.0, 1.0),
                        c.a,
                    )
                } else {
                    let hr = ((c.r * 12.9898).sin() * 43758.5453).fract();
                    let hg = ((c.g * 78.2330).sin() * 43758.5453).fract();
                    let hb = ((c.b * 45.1640).sin() * 43758.5453).fract();
                    Color::rgba(
                        (c.r + (hr - 0.5) * n_amount).clamp(0.0, 1.0),
                        (c.g + (hg - 0.5) * n_amount).clamp(0.0, 1.0),
                        (c.b + (hb - 0.5) * n_amount).clamp(0.0, 1.0),
                        c.a,
                    )
                }
            }
            // Shader Lab runs on the GPU over whole tiles (see
            // `renderer::shader_lab`); a lone color sample is unchanged.
            Self::ShaderLab { .. } => c,
        }
    }
}

/// The evaluated state of an individual layer effect at target timecode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatedEffect {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub effect_type: EvaluatedEffectType,
}

impl EvaluatedEffect {
    /// Process a color through this evaluated effect if enabled.
    pub fn process_color(&self, color: Color) -> Color {
        if !self.enabled {
            return color;
        }
        self.effect_type.process_color(color)
    }
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
    pub effects: Vec<EvaluatedEffect>,
}

impl EvaluatedLayer {
    /// Return the visually processed effective color for this layer after sequentially applying all enabled effects.
    pub fn processed_color(&self, base_color: Color) -> Color {
        let mut color = base_color;
        for effect in &self.effects {
            if effect.enabled {
                color = effect.process_color(color);
            }
        }
        color
    }

    /// Return true if this layer participates in active rendering.
    pub fn is_rendered(&self) -> bool {
        self.is_visible && self.is_active && !self.is_matte_source && self.effective_opacity > 0.0
    }

    /// Return true if this layer references a nested composition.
    pub fn is_nested_composition(&self) -> bool {
        matches!(self.source, LayerSource::NestedComposition { .. })
    }

    /// Return true if this layer is an adjustment layer.
    pub fn is_adjustment(&self) -> bool {
        matches!(self.source, LayerSource::Adjustment)
    }

    /// True when this layer acts as an After Effects-style adjustment layer:
    /// active, visible, and carrying the Adjustment source. Such layers render
    /// nothing themselves — their enabled effects apply to the composite of
    /// all layers beneath them (see
    /// [`EvaluatedStack::adjustment_effects_applying_to`]).
    pub fn applies_as_adjustment(&self) -> bool {
        self.is_adjustment() && self.is_active && self.is_visible
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
        self.flattened_render_list_internal(AffineTransform2D::IDENTITY, 1.0, 0, &[], false)
    }

    pub(crate) fn flattened_render_list_internal(
        &self,
        parent_matrix: AffineTransform2D,
        parent_opacity: f32,
        depth: usize,
        path_prefix: &[String],
        parent_is_matte_source: bool,
    ) -> Vec<FlattenedRenderLayer> {
        let mut result = Vec::new();

        // Composite order: render_list contains layer IDs from bottom to top
        for layer_id in &self.render_list {
            if let Some(layer) = self.get_layer(layer_id) {
                let mut current_path = path_prefix.to_vec();
                current_path.push(layer.id.clone());

                let current_world_matrix = parent_matrix * layer.world_matrix();
                let current_opacity = parent_opacity * layer.effective_opacity;
                let current_is_matte = parent_is_matte_source || layer.is_matte_source;

                if let Some(ref nested) = layer.nested_composition {
                    let inner_layers = nested.inner_stack.flattened_render_list_internal(
                        current_world_matrix,
                        current_opacity,
                        depth + 1,
                        &current_path,
                        current_is_matte,
                    );
                    result.extend(inner_layers);
                } else {
                    let matte_source_path = layer.matte_source_id.as_ref().map(|src_id| {
                        let mut p = path_prefix.to_vec();
                        p.push(src_id.clone());
                        p
                    });

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
                        matte_source_path,
                        is_matte_source: current_is_matte,
                        time_offset_frames: layer.time_offset_frames,
                        time_offset_seconds: layer.time_offset_seconds,
                        effects: layer.effects.clone(),
                    });
                }
            }
        }

        result
    }

    /// Retrieve a flattened render layer by its unique layer ID from the composite list.
    pub fn get_flattened_layer(&self, id: &str) -> Option<FlattenedRenderLayer> {
        self.flattened_render_list().into_iter().find(|l| l.layer_id == id)
    }

    /// Retrieve the concatenated root world transform matrix for a deeply nested layer path.
    pub fn deep_layer_root_matrix(&self, layer_path: &[&str]) -> Option<AffineTransform2D> {
        let flattened = self.flattened_render_list();
        flattened
            .iter()
            .find(|l| l.layer_path.as_slice() == layer_path)
            .map(|l| l.root_world_matrix)
    }

    /// After Effects adjustment-layer semantics: return the enabled effects of
    /// every active, visible adjustment layer positioned *above* `layer_id`
    /// in composite (bottom-to-top) order, i.e. the adjustments that apply
    /// to that layer's composite. Layers are affected only by adjustments
    /// stacked above them; adjustments below have no effect on them.
    pub fn adjustment_effects_applying_to(&self, layer_id: &str) -> Vec<EvaluatedEffect> {
        let pos = self.render_list.iter().position(|id| id == layer_id);
        let Some(pos) = pos else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for above_id in &self.render_list[pos + 1..] {
            if let Some(adj) = self.get_layer(above_id) {
                if adj.applies_as_adjustment() {
                    out.extend(adj.effects.iter().filter(|e| e.enabled).cloned());
                }
            }
        }
        out
    }

    /// Collect all composition render passes in bottom-up dependency order.
    pub fn collect_render_passes(&self) -> Vec<RenderPassDescriptor> {
        let mut passes = Vec::new();
        self.collect_render_passes_internal(
            None,
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
        nesting_layer_id: Option<String>,
        world_matrix: AffineTransform2D,
        opacity: f32,
        blend_mode: BlendMode,
        depth: usize,
        passes: &mut Vec<RenderPassDescriptor>,
    ) {
        // Collect nested children passes first (leaf-first dependency order).
        // Include any nested comp that is in self.render_list OR is an active, visible matte source
        for layer in &self.evaluated_layers {
            let is_needed = self.render_list.contains(&layer.id)
                || (layer.is_visible && layer.is_active && layer.is_matte_source);
            if is_needed {
                if let Some(ref nested) = layer.nested_composition {
                    let child_world = world_matrix * layer.world_matrix();
                    let child_opacity = opacity * layer.effective_opacity;
                    nested.inner_stack.collect_render_passes_internal(
                        Some(layer.id.clone()),
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
            nesting_layer_id,
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
        let time_stretch = if node.time_stretch.is_finite() {
            node.time_stretch
        } else {
            1.0
        };

        // Frame rate alignment
        let same_rate = (parent_time.frame_rate() - nested_comp.frame_rate).abs() < 1e-6;
        if same_rate {
            let in_point_frames = if (node.in_point.frame_rate() - parent_time.frame_rate()).abs() < 1e-6 {
                node.in_point.frames()
            } else {
                TimeCode::from_seconds(node.in_point.seconds(), parent_time.frame_rate()).frames()
            };
            let elapsed_frames = parent_time.frames() - in_point_frames;
            let offset_frames = node.start_offset.map(|tc| {
                if (tc.frame_rate() - nested_comp.frame_rate).abs() < 1e-6 {
                    tc.frames()
                } else {
                    TimeCode::from_seconds(tc.seconds(), nested_comp.frame_rate).frames()
                }
            }).unwrap_or(0);
            let raw_frames = (elapsed_frames as f64) * time_stretch + (offset_frames as f64);
            let target_frames = if raw_frames.is_finite() {
                raw_frames.round() as i64
            } else {
                0
            };
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
            let target_seconds = elapsed_seconds * time_stretch + offset_seconds;
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
    if !target_seconds.is_finite() || duration_seconds <= 0.0 {
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

            let mut evaluated_effects = Vec::with_capacity(node.effects.len());
            for eff in &node.effects {
                if eff.enabled {
                    let eval_type = match &eff.effect_type {
                        EffectType::GaussianBlur { radius } => {
                            EvaluatedEffectType::GaussianBlur {
                                radius: radius.evaluate_at(time),
                            }
                        }
                        EffectType::BrightnessContrast {
                            brightness,
                            contrast,
                        } => EvaluatedEffectType::BrightnessContrast {
                            brightness: brightness.evaluate_at(time),
                            contrast: contrast.evaluate_at(time),
                        },
                        EffectType::Tint {
                            map_black,
                            map_white,
                            amount,
                        } => EvaluatedEffectType::Tint {
                            map_black: *map_black,
                            map_white: *map_white,
                            amount: amount.evaluate_at(time),
                        },
                        EffectType::Invert { amount } => EvaluatedEffectType::Invert {
                            amount: amount.evaluate_at(time),
                        },
                        EffectType::DropShadow {
                            distance,
                            angle,
                            softness,
                            opacity,
                            color,
                        } => EvaluatedEffectType::DropShadow {
                            distance: distance.evaluate_at(time),
                            angle: angle.evaluate_at(time),
                            softness: softness.evaluate_at(time),
                            opacity: opacity.evaluate_at(time),
                            color: *color,
                        },
                        EffectType::GlslShader {
                            code,
                            param1,
                            param2,
                            param3,
                            param4,
                        } => EvaluatedEffectType::GlslShader {
                            code: code.clone(),
                            param1: param1.evaluate_at(time),
                            param2: param2.evaluate_at(time),
                            param3: param3.evaluate_at(time),
                            param4: param4.evaluate_at(time),
                        },
                        EffectType::DisplacementMap {
                            max_horizontal,
                            max_vertical,
                        } => EvaluatedEffectType::DisplacementMap {
                            max_horizontal: max_horizontal.evaluate_at(time),
                            max_vertical: max_vertical.evaluate_at(time),
                        },
                        EffectType::ChromaKey {
                            key_color,
                            tolerance,
                            feather,
                        } => EvaluatedEffectType::ChromaKey {
                            key_color: *key_color,
                            tolerance: tolerance.evaluate_at(time),
                            feather: feather.evaluate_at(time),
                        },
                        EffectType::LumaKey { threshold, feather } => {
                            EvaluatedEffectType::LumaKey {
                                threshold: threshold.evaluate_at(time),
                                feather: feather.evaluate_at(time),
                            }
                        }
                        EffectType::NoiseGenerator {
                            amount,
                            monochrome,
                        } => EvaluatedEffectType::NoiseGenerator {
                            amount: amount.evaluate_at(time),
                            monochrome: *monochrome,
                        },
                        EffectType::ShaderLab { source, params, values, .. } => {
                            use std::collections::hash_map::DefaultHasher;
                            use std::hash::{Hash, Hasher};
                            let mut hasher = DefaultHasher::new();
                            source.hash(&mut hasher);
                            let source_hash = hasher.finish();
                            let resolved: HashMap<String, project::ShaderParamValue> = params
                                .iter()
                                .map(|p| {
                                    (
                                        p.name.clone(),
                                        values.get(&p.name).cloned().unwrap_or_else(|| p.default.clone()),
                                    )
                                })
                                .collect();
                            EvaluatedEffectType::ShaderLab {
                                source_hash,
                                values: resolved,
                            }
                        }
                    };
                    evaluated_effects.push(EvaluatedEffect {
                        id: eff.id.clone(),
                        name: eff.name.clone(),
                        enabled: true,
                        effect_type: eval_type,
                    });
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
                effects: evaluated_effects,
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
