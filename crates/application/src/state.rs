use compositor::{AffineTransform2D, EvaluatedStack, LayerStackEvaluator, SceneGraph};
use gpui_kit::component::input::InputState;
use gpui_kit::{Entity, Subscription};
use project::{
    Asset, AutoTraceOptions, BlendMode, Color, Composition, Effect, EffectType, Keyframe,
    KeyframeInterpolation,     KeyframeTangent, Layer, LayerSource, Mask, MaskShapeKind, Path,
    PathPointKind, PlaybackClock, Project, Property, ShapeType, TimeCode, TraceRange,
    TrackMatteMode, Vec2,
};
use std::path::PathBuf;
use std::time::Duration;

/// Active editing tool mimicking After Effects tools palette.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditorTool {
    #[default]
    Move,          // 'V' Selection and translation
    Hand,          // 'H' Pan view
    Rotate,        // 'W' Rotation
    Pen,           // 'G' Vector pen / path drawing
    Text,          // 'T' Text layer placement
    ShapeRect,     // 'Q' Rectangle shape
    ShapeEllipse,  // 'Q' Ellipse shape
}

/// Query and cache all installed system fonts across Windows, macOS, and Linux.
pub fn available_system_fonts() -> &'static [String] {
    static FONTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut set = std::collections::BTreeSet::new();
        for face in db.faces() {
            for (fam, _) in &face.families {
                if !fam.starts_with('@') && !fam.is_empty() {
                    set.insert(fam.clone());
                }
            }
        }
        if set.is_empty() {
            set.insert("Arial".to_string());
            set.insert("Helvetica".to_string());
            set.insert("Segoe UI".to_string());
            set.insert("Roboto".to_string());
            set.insert("Times New Roman".to_string());
            set.insert("Courier New".to_string());
            set.insert("Georgia".to_string());
        }
        set.into_iter().collect()
    })
}

/// Pick a sensible default font family for new text layers based on what is
/// actually installed on this machine (Windows, macOS, or Linux).
///
/// Never hardcodes a single family: prefers a list of common families when
/// present, otherwise falls back to the first enumerated system font, and
/// finally to the generic `sans-serif` alias.
pub fn default_font_family() -> String {
    let fonts = available_system_fonts();
    const PREFERRED: &[&str] = &[
        "Inter",
        "Segoe UI",
        "SF Pro Text",
        "Helvetica Neue",
        "Helvetica",
        "Arial",
        "Roboto",
        "Noto Sans",
        "DejaVu Sans",
        "Liberation Sans",
    ];
    for preferred in PREFERRED {
        if fonts.iter().any(|f| f.eq_ignore_ascii_case(preferred)) {
            return preferred.to_string();
        }
    }
    fonts
        .first()
        .cloned()
        .unwrap_or_else(|| "sans-serif".to_string())
}

/// Resolve a requested font family against installed system fonts in an
/// OS-agnostic way: returns the request unchanged when installed, otherwise
/// the machine-appropriate default from [`default_font_family`].
pub fn resolve_font_family(requested: &str) -> String {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return default_font_family();
    }
    if available_system_fonts()
        .iter()
        .any(|f| f.eq_ignore_ascii_case(trimmed))
    {
        trimmed.to_string()
    } else {
        default_font_family()
    }
}

/// Viewport preview resolution. Full rasterizes at the capped box size;
/// Half quarters pixels everywhere (faster interaction AND idle preview
/// on weak machines). Gestures always drop to Half while held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewQuality {
    #[default]
    Full,
    Half,
}

impl PreviewQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Half => "Half",
        }
    }

    /// Resolution divisor for raster output sizes.
    pub const fn divisor(self) -> u32 {
        match self {
            Self::Full => 1,
            Self::Half => 2,
        }
    }
}

/// Central application editor state managing the active project, playback clock,
/// layer selection, active tool, and composition evaluation.
pub struct EditorState {
    pub project: Project,
    pub active_comp_id: String,
    pub clock: PlaybackClock,
    pub selected_layer_id: Option<String>,
    pub is_playing: bool,
    pub active_tool: EditorTool,
    pub timeline_full_width: bool,
    /// Viewport snapping toggle (top toolbar magnet).
    pub snapping: bool,
    /// Scrub key currently open for After Effects-style keyboard entry.
    pub value_edit_key: Option<String>,
    /// Live single-line editor for `value_edit_key` (created on demand).
    pub value_editor: Option<Entity<InputState>>,
    pub value_editor_sub: Option<Subscription>,
    evaluator: LayerStackEvaluator,
    /// Toolbar tool defaults (edited in Properties > Tool Settings, used by
    /// new layers so tools behave consistently across the app).
    pub tool_font_size: f32,
    pub tool_text_color: Color,
    pub tool_shape_fill: Color,
    pub tool_solid_color: Color,
    /// Degrees per click for the Rotate tool.
    pub tool_rotate_step: f32,
    /// True while a drag/scrub gesture is active anywhere. The viewport
    /// rasterizer uses it to pick the cheap Shader Lab probe wash instead
    /// of the full per-pixel interpreter, so scrubbing stays fluid and
    /// full quality lands on release.
    pub preview_fast: bool,
    /// Sticky preview resolution (View menu). Combined with `preview_fast`
    /// (gestures/playback) to pick raster sizes.
    pub preview_quality: PreviewQuality,
    /// On-disk path of the open project, if it was saved/loaded.
    pub project_path: Option<PathBuf>,
    /// Session recent-file list for the File menu (newest first, cap 8).
    pub recent_projects: Vec<PathBuf>,
    /// Undo history (project snapshots with selection context).
    undo_stack: Vec<HistoryEntry>,
    /// Redo history, cleared by every new checkpoint.
    redo_stack: Vec<HistoryEntry>,
    /// True when Timeline Graph / Spline Editor view is toggled open.
    pub spline_editor_open: bool,
    /// Property path currently targeted in the Spline Editor (e.g. "transform.position.y").
    pub spline_prop_path: String,
    /// Mask node-edit target `(layer_id, mask_id)` for the viewport Path
    /// Editor. Pen clicks append to it while set (UI state, not undoable).
    pub active_mask_edit: Option<(String, String)>,
}

/// One undo/redo snapshot: the whole project plus UI context.
/// Projects are small (layers + effects), so full snapshots stay cheap
/// and every mutation path shares one mechanism — predictable and easy
/// to extend (new mutating methods just call `checkpoint()` first).
#[derive(Debug, Clone)]
struct HistoryEntry {
    project: Project,
    active_comp_id: String,
    selected_layer_id: Option<String>,
}

impl HistoryEntry {
    fn capture(s: &EditorState) -> Self {
        Self {
            project: s.project.clone(),
            active_comp_id: s.active_comp_id.clone(),
            selected_layer_id: s.selected_layer_id.clone(),
        }
    }

    fn restore(&self, s: &mut EditorState) {
        s.project = self.project.clone();
        s.active_comp_id = self.active_comp_id.clone();
        s.selected_layer_id = self.selected_layer_id.clone();
    }
}

/// Easing presets for the Graph / Spline Editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EasingPreset {
    Linear,
    EaseIn,
    EaseOut,
    EasyEase,
    Hold,
}

impl EasingPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::EasyEase => "Easy Ease",
            Self::Hold => "Hold",
        }
    }
}

fn apply_easing_to_prop<T: project::Interpolate>(prop: &mut project::Property<T>, easing: EasingPreset) {
    for kf in prop.keyframes_mut() {
        match easing {
            EasingPreset::Linear => {
                kf.interpolation = project::KeyframeInterpolation::Linear;
                kf.in_tangent = Some(project::KeyframeTangent::linear_in());
                kf.out_tangent = Some(project::KeyframeTangent::linear_out());
            }
            EasingPreset::EaseIn => {
                kf.interpolation = project::KeyframeInterpolation::Bezier;
                kf.in_tangent = Some(project::KeyframeTangent::ease_in_in());
                kf.out_tangent = Some(project::KeyframeTangent::ease_in_out());
            }
            EasingPreset::EaseOut => {
                kf.interpolation = project::KeyframeInterpolation::Bezier;
                kf.in_tangent = Some(project::KeyframeTangent::ease_out_in());
                kf.out_tangent = Some(project::KeyframeTangent::ease_out_out());
            }
            EasingPreset::EasyEase => {
                kf.interpolation = project::KeyframeInterpolation::Bezier;
                kf.in_tangent = Some(project::KeyframeTangent::ease_in_out_in());
                kf.out_tangent = Some(project::KeyframeTangent::ease_in_out_out());
            }
            EasingPreset::Hold => {
                kf.interpolation = project::KeyframeInterpolation::Hold;
                kf.in_tangent = None;
                kf.out_tangent = None;
            }
        }
    }
}

// --- Spline / graph editor data + mutations ---
//
// Graph paths name scalar animated values: `transform.anchor_point.x`,
// `transform.position.y`, `transform.scale.x`, `transform.rotation`,
// `opacity`, `text.font_size`, `shape.rect_width`, ...,
// `effect:<effect_id>:<param>`. The Graph panel owns drawing; these
// methods own enumeration, sampling, and keyframe edits.

/// One keyframe point for the graph editor.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphKey {
    pub t: f64,
    pub v: f32,
    pub interp: KeyframeInterpolation,
    pub in_tan: Option<(f32, f32)>,
    pub out_tan: Option<(f32, f32)>,
}

/// One plottable series (a scalar prop with keyframes).
#[derive(Debug, Clone)]
pub struct GraphSeries {
    pub path: String,
    pub label: String,
    pub color: (f32, f32, f32),
    pub keys: Vec<GraphKey>,
}

/// Resolve a graph path to a mutable scalar (`f32`) property on a layer.
///
/// Vec2-backed paths (`transform.position.x`, ...) return `None` here:
/// Vec2 components are edited through the dedicated Vec2 arms in
/// `move_graph_keyframe` / `cycle_graph_key_interp` /
/// `set_graph_key_tangents`, never through this scalar helper.
fn graph_prop_mut<'a>(layer: &'a mut Layer, path: &str) -> Option<&'a mut Property<f32>> {
    let (base, comp) = match path.rsplit_once('.') {
        Some((b, c)) if ["x", "y"].contains(&c) => (b, Some(c)),
        _ => (path, None),
    };
    let _ = comp;
    match base {
        // Vec2-backed bases have no scalar property; see doc comment above.
        "transform.anchor_point" | "transform.position" | "transform.scale" => None,
        "transform.rotation" => Some(&mut layer.transform.rotation),
        "opacity" => Some(&mut layer.opacity),
        "text.font_size" => match &mut layer.source {
            LayerSource::Text { font_size, .. } => Some(font_size),
            _ => None,
        },
        "text.tracking" => match &mut layer.source {
            LayerSource::Text { tracking, .. } => Some(tracking),
            _ => None,
        },
        "text.leading" => match &mut layer.source {
            LayerSource::Text { leading, .. } => Some(leading),
            _ => None,
        },
        "text.stroke_width" => match &mut layer.source {
            LayerSource::Text { stroke_width, .. } => Some(stroke_width),
            _ => None,
        },
        "text.baseline_shift" => match &mut layer.source {
            LayerSource::Text { baseline_shift, .. } => Some(baseline_shift),
            _ => None,
        },
        "text.box_width" => match &mut layer.source {
            LayerSource::Text { box_width, .. } => Some(box_width),
            _ => None,
        },
        "shape.rect_width" => match &mut layer.source {
            LayerSource::Shape { shape_type: ShapeType::Rectangle { width, .. } } => Some(width),
            _ => None,
        },
        "shape.rect_height" => match &mut layer.source {
            LayerSource::Shape { shape_type: ShapeType::Rectangle { height, .. } } => Some(height),
            _ => None,
        },
        "shape.corner_radius" => match &mut layer.source {
            LayerSource::Shape { shape_type: ShapeType::Rectangle { corner_radius, .. } } => {
                Some(corner_radius)
            }
            _ => None,
        },
        "shape.ellipse_rx" => match &mut layer.source {
            LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_x, .. } } => Some(radius_x),
            _ => None,
        },
        "shape.ellipse_ry" => match &mut layer.source {
            LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_y, .. } } => Some(radius_y),
            _ => None,
        },
        _ => {
            let rest = base.strip_prefix("effect:")?;
            let mut parts = rest.splitn(2, ':');
            let eid = parts.next()?;
            let pname = parts.next()?;
            let fx = layer.get_effect_mut(eid)?;
            Some(fx.get_param_property_mut(pname)?)
        }
    }
}

/// Split a graph path into its base and optional Vec2 component.
/// Returns `None` for malformed paths. Scalar paths (rotation,
/// opacity, effect params, ...) carry no component.
fn split_graph_path(path: &str) -> Option<(&str, Option<usize>)> {
    if let Some((b, c)) = path.rsplit_once('.') {
        if c == "x" {
            return Some((b, Some(0)));
        }
        if c == "y" {
            return Some((b, Some(1)));
        }
    }
    // No component suffix: valid for scalar bases, resolved per base.
    Some((path, None))
}

/// Read-only snapshot of one scalar graph property.
fn graph_prop_read(layer: &Layer, path: &str) -> Option<(Vec<GraphKey>, f32)> {
    let (base, comp) = split_graph_path(path)?;
    // Vec2 bases require an explicit component; scalar bases must not
    // have one.
    let is_vec2_base = matches!(
        base,
        "transform.anchor_point" | "transform.position" | "transform.scale"
    );
    if is_vec2_base && comp.is_none() {
        return None;
    }
    if !is_vec2_base && comp.is_some() {
        return None;
    }
    let axis = comp.unwrap_or(1);
    let take_f32 = |prop: &Property<f32>| -> (Vec<GraphKey>, f32) {
        let keys = prop
            .keyframes()
            .iter()
            .map(|k| GraphKey {
                t: k.time_seconds(),
                v: k.value,
                interp: k.interpolation,
                in_tan: k.in_tangent.map(|t| (t.x, t.y)),
                out_tan: k.out_tangent.map(|t| (t.x, t.y)),
            })
            .collect();
        (keys, prop.value)
    };
    let take_vec2 = |prop: &Property<Vec2>| -> (Vec<GraphKey>, f32) {
        let keys = prop
            .keyframes()
            .iter()
            .map(|k| GraphKey {
                t: k.time_seconds(),
                v: if axis == 0 { k.value.x } else { k.value.y },
                interp: k.interpolation,
                in_tan: k.in_tangent.map(|t| (t.x, t.y)),
                out_tan: k.out_tangent.map(|t| (t.x, t.y)),
            })
            .collect();
        let cur = if axis == 0 { prop.value.x } else { prop.value.y };
        (keys, cur)
    };
    Some(match base {
        "transform.anchor_point" => take_vec2(&layer.transform.anchor_point),
        "transform.position" => take_vec2(&layer.transform.position),
        "transform.scale" => take_vec2(&layer.transform.scale),
        "transform.rotation" => take_f32(&layer.transform.rotation),
        "opacity" => take_f32(&layer.opacity),
        "text.font_size" => match &layer.source {
            LayerSource::Text { font_size, .. } => take_f32(font_size),
            _ => return None,
        },
        "text.tracking" => match &layer.source {
            LayerSource::Text { tracking, .. } => take_f32(tracking),
            _ => return None,
        },
        "text.leading" => match &layer.source {
            LayerSource::Text { leading, .. } => take_f32(leading),
            _ => return None,
        },
        "text.stroke_width" => match &layer.source {
            LayerSource::Text { stroke_width, .. } => take_f32(stroke_width),
            _ => return None,
        },
        "text.baseline_shift" => match &layer.source {
            LayerSource::Text { baseline_shift, .. } => take_f32(baseline_shift),
            _ => return None,
        },
        "text.box_width" => match &layer.source {
            LayerSource::Text { box_width, .. } => take_f32(box_width),
            _ => return None,
        },
        "shape.rect_width" => match &layer.source {
            LayerSource::Shape { shape_type: ShapeType::Rectangle { width, .. } } => {
                take_f32(width)
            }
            _ => return None,
        },
        "shape.rect_height" => match &layer.source {
            LayerSource::Shape { shape_type: ShapeType::Rectangle { height, .. } } => {
                take_f32(height)
            }
            _ => return None,
        },
        "shape.corner_radius" => match &layer.source {
            LayerSource::Shape { shape_type: ShapeType::Rectangle { corner_radius, .. } } => {
                take_f32(corner_radius)
            }
            _ => return None,
        },
        "shape.ellipse_rx" => match &layer.source {
            LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_x, .. } } => {
                take_f32(radius_x)
            }
            _ => return None,
        },
        "shape.ellipse_ry" => match &layer.source {
            LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_y, .. } } => {
                take_f32(radius_y)
            }
            _ => return None,
        },
        _ => {
            let rest = base.strip_prefix("effect:")?;
            let mut parts = rest.splitn(2, ':');
            let (eid, pname) = (parts.next()?, parts.next()?);
            take_f32(layer.get_effect(eid)?.get_param_property(pname)?)
        }
    })
}

/// Candidate graph paths for a layer (labels + palette).
fn graph_candidates(layer: &Layer) -> Vec<(String, String)> {
    let mut out = vec![
        ("transform.anchor_point.x".to_string(), "Anchor X".to_string()),
        ("transform.anchor_point.y".to_string(), "Anchor Y".to_string()),
        ("transform.position.x".to_string(), "Position X".to_string()),
        ("transform.position.y".to_string(), "Position Y".to_string()),
        ("transform.scale.x".to_string(), "Scale X".to_string()),
        ("transform.scale.y".to_string(), "Scale Y".to_string()),
        ("transform.rotation".to_string(), "Rotation".to_string()),
        ("opacity".to_string(), "Opacity".to_string()),
    ];
    match &layer.source {
        LayerSource::Text { .. } => {
            out.push(("text.font_size".to_string(), "Font Size".to_string()));
            out.push(("text.tracking".to_string(), "Tracking".to_string()));
            out.push(("text.leading".to_string(), "Leading".to_string()));
            out.push(("text.stroke_width".to_string(), "Stroke Width".to_string()));
            out.push(("text.baseline_shift".to_string(), "Baseline Shift".to_string()));
            out.push(("text.box_width".to_string(), "Box Width".to_string()));
        }
        LayerSource::Shape { shape_type } => match shape_type {
            ShapeType::Rectangle { .. } => {
                out.push(("shape.rect_width".to_string(), "Rect Width".to_string()));
                out.push(("shape.rect_height".to_string(), "Rect Height".to_string()));
                out.push(("shape.corner_radius".to_string(), "Corner Radius".to_string()));
            }
            ShapeType::Ellipse { .. } => {
                out.push(("shape.ellipse_rx".to_string(), "Ellipse RX".to_string()));
                out.push(("shape.ellipse_ry".to_string(), "Ellipse RY".to_string()));
            }
            _ => {}
        },
        _ => {}
    }
    for eff in &layer.effects {
        use std::collections::HashSet;
        let mut seen: HashSet<&str> = HashSet::new();
        // Legacy params by name probe.
        for name in ["radius", "brightness", "contrast", "amount", "distance", "softness", "opacity", "param1", "param2", "param3", "param4", "max_horizontal", "max_vertical", "tolerance", "threshold", "feather", "size", "angle", "skew_x", "skew_y", "width", "strength", "intensity", "tiles_x", "tiles_y", "scale", "exposure", "vibrance", "input_black", "input_white", "gamma", "output_black", "output_white", "hue_shift", "saturation", "lightness"] {
            if eff.get_param_property(name).is_some() {
                seen.insert(name);
                out.push((
                    format!("effect:{}:{name}", eff.id),
                    format!("{} · {}", eff.name, name),
                ));
            }
        }
        // Stock plug-ins enumerate from the descriptor (covers every param
        // without a hardcoded list; already-seen names are skipped).
        for (name, label, _v, _step) in eff.stock_scalar_params() {
            if seen.contains(name) {
                continue;
            }
            out.push((
                format!("effect:{}:{name}", eff.id),
                format!("{} · {label}", eff.name),
            ));
        }
    }
    out
}

const GRAPH_COLORS: [(f32, f32, f32); 8] = [
    (0.95, 0.45, 0.45),
    (0.45, 0.75, 0.95),
    (0.55, 0.85, 0.45),
    (0.95, 0.8, 0.35),
    (0.75, 0.55, 0.95),
    (0.95, 0.6, 0.8),
    (0.5, 0.9, 0.85),
    (0.9, 0.9, 0.9),
];

/// Unique `mask_N` id within a layer.
fn next_mask_id(layer: &Layer) -> String {
    let mut counter = layer.masks.len() + 1;
    let mut id = format!("mask_{counter}");
    while layer.get_mask(&id).is_some() {
        counter += 1;
        id = format!("mask_{counter}");
    }
    id
}

/// All plottable (animated) series for a layer.
fn with_graph_scalar<R>(
    layer: &mut Layer,
    path: &str,
    f: impl FnOnce(&mut Property<f32>) -> R,
) -> Option<R> {
    graph_prop_mut(layer, path).map(f)
}

impl EditorState {
    /// Return all installed system fonts across Windows, macOS, and Linux.
    pub fn available_system_fonts() -> &'static [String] {
        available_system_fonts()
    }

    /// Return the machine-appropriate default font family for new text layers.
    pub fn default_font_family() -> String {
        default_font_family()
    }

    /// Resolve a requested font family against installed system fonts,
    /// falling back to the machine default when it is not installed.
    pub fn resolve_font_family(requested: &str) -> String {
        resolve_font_family(requested)
    }

    /// Create a new EditorState pre-seeded with a starter composition and animated demo layers.
    pub fn new() -> Self {
        let mut project = Project::new("proj_default", "Motion Studio Project");
        let fps = 30.0;
        let duration_secs = 5.0;
        let mut comp = Composition::hd_1080p_30fps("comp_main", "Main Composition", duration_secs);

        let tc0 = TimeCode::from_frames(0, fps);
        let tc150 = TimeCode::from_frames(150, fps);

        // Layer 1: Dark background canvas solid (centered at 0, 0)
        let mut bg_solid = Layer::solid(
            "layer_bg",
            "Background Solid",
            Color::from_hex("#121316").unwrap_or(Color::BLACK),
            1920,
            1080,
            tc0,
            tc150,
        );
        bg_solid.transform.position.set_value(Vec2::ZERO);
        bg_solid.transform.anchor_point.set_value(Vec2::new(960.0, 540.0));

        // Layer 2: Animated accent solid with Position and Rotation keyframes (centered at 0, 0)
        let mut accent = Layer::solid(
            "layer_accent",
            "Animated Box",
            Color::from_hex("#3B82F6").unwrap_or(Color::BLUE),
            300,
            300,
            tc0,
            tc150,
        );
        accent.transform.anchor_point.set_value(Vec2::new(150.0, 150.0));
        accent.transform.position.set_value(Vec2::ZERO);

        // Position animation: horizontal sway around center (0, 0)
        accent.transform.position.add_keyframe(Keyframe::bezier(
            TimeCode::from_frames(0, fps),
            Vec2::new(-200.0, 0.0),
            None,
            Some(KeyframeTangent::ease_in_out_out()),
        ));
        accent.transform.position.add_keyframe(Keyframe::bezier(
            TimeCode::from_frames(60, fps),
            Vec2::new(200.0, 0.0),
            Some(KeyframeTangent::ease_in_out_in()),
            Some(KeyframeTangent::ease_in_out_out()),
        ));
        accent.transform.position.add_keyframe(Keyframe::bezier(
            TimeCode::from_frames(120, fps),
            Vec2::new(-200.0, 0.0),
            Some(KeyframeTangent::ease_in_out_in()),
            None,
        ));

        // Rotation animation: continuous spin
        accent.transform.rotation.add_keyframe(Keyframe::linear(TimeCode::from_frames(0, fps), 0.0));
        accent.transform.rotation.add_keyframe(Keyframe::linear(TimeCode::from_frames(120, fps), 360.0));

        // Layer 3: Title badge with opacity fade-in (offset below center)
        let mut title_card = Layer::solid(
            "layer_badge",
            "Accent Badge",
            Color::from_hex("#10B981").unwrap_or(Color::GREEN),
            400,
            120,
            TimeCode::from_frames(15, fps),
            tc150,
        );
        title_card.transform.anchor_point.set_value(Vec2::new(200.0, 60.0));
        title_card.transform.position.set_value(Vec2::new(0.0, 240.0));
        title_card.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(15, fps), 0.0));
        title_card.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(45, fps), 100.0));

        comp.add_layer(accent).unwrap();
        comp.add_layer(title_card).unwrap();
        // Background solid added LAST so it sits at the bottom of the stack
        // (index 0 is the topmost layer, After Effects convention).
        comp.add_layer(bg_solid).unwrap();

        let clock = PlaybackClock::from_composition(&comp);
        let active_comp_id = "comp_main".to_string();
        let selected_layer_id = Some("layer_accent".to_string());

        project.add_composition(comp).unwrap();

        Self {
            project,
            active_comp_id,
            clock,
            selected_layer_id,
            is_playing: false,
            active_tool: EditorTool::Move,
            timeline_full_width: false,
            snapping: true,
            value_edit_key: None,
            value_editor: None,
            value_editor_sub: None,
            evaluator: LayerStackEvaluator::new(),
            tool_font_size: 48.0,
            tool_text_color: Color::WHITE,
            tool_shape_fill: Color::from_rgba_u8(168, 85, 247, 255),
            tool_solid_color: Color::from_rgba_u8(59, 130, 246, 255),
            tool_rotate_step: 15.0,
            preview_fast: false,
            preview_quality: PreviewQuality::Full,
            project_path: None,
            recent_projects: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            spline_editor_open: false,
            spline_prop_path: "transform.position".to_string(),
            active_mask_edit: None,
        }
    }

    /// Nudge one component of a vector/color Shader Lab parameter.
    pub fn nudge_shaderlab_component(
        &mut self,
        effect_id: &str,
        param_name: &str,
        index: usize,
        delta: f32,
    ) -> Result<(), String> {
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        let comp = self
            .active_composition()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;
        let effect = layer
            .get_effect(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        let params = effect.shader_params().ok_or_else(|| "Not a Shader Lab effect".to_string())?.to_vec();
        let param = params
            .iter()
            .find(|p| p.name == param_name)
            .ok_or_else(|| format!("Shader parameter {param_name} not found"))?;
        let step = param.step.unwrap_or(0.05);
        let cur = effect
            .resolved_shader_values()
            .into_iter()
            .find(|(n, _)| n == param_name)
            .map(|(_, v)| v);
        let bump = |x: f32| x + delta * step;
        let next = match cur {
            Some(project::ShaderParamValue::Vec2(a)) => {
                let mut b = a;
                if index < 2 { b[index] = bump(b[index]); }
                project::ShaderParamValue::Vec2(b)
            }
            Some(project::ShaderParamValue::Vec3(a)) => {
                let mut b = a;
                if index < 3 { b[index] = bump(b[index]); }
                project::ShaderParamValue::Vec3(b)
            }
            Some(project::ShaderParamValue::Vec4(a)) => {
                let mut b = a;
                if index < 4 { b[index] = bump(b[index]); }
                project::ShaderParamValue::Vec4(b)
            }
            Some(project::ShaderParamValue::Color(c)) => {
                let mut b = [c.r, c.g, c.b, c.a];
                if index < 4 { b[index] = (bump(b[index])).clamp(0.0, 1.0); }
                project::ShaderParamValue::Color(Color::rgba(b[0], b[1], b[2], b[3]))
            }
            Some(project::ShaderParamValue::Float(v)) => param.coerce_float(bump(v)),
            Some(project::ShaderParamValue::Int(v)) => param.coerce_float(v as f32 + delta),
            _ => param.coerce_float(delta),
        };
        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer_mut = comp_mut
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;
        let effect_mut = layer_mut
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if effect_mut.set_shader_value(param_name, next) {
            Ok(())
        } else {
            Err(format!("Shader parameter {param_name} not found"))
        }
    }

    /// Toggle whether the timeline spans the full width of the application.
    pub fn toggle_timeline_full_width(&mut self) {
        self.timeline_full_width = !self.timeline_full_width;
    }

    /// Toggle viewport snapping (top toolbar magnet).
    pub fn toggle_snapping(&mut self) {
        self.snapping = !self.snapping;
    }

    /// Nudge a Shader Lab parameter on the selected layer's effect.
    pub fn nudge_shaderlab_param(
        &mut self,
        effect_id: &str,
        param_name: &str,
        delta: f32,
    ) -> Result<(), String> {
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;
        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if effect.nudge_shader_value(param_name, delta) {
            Ok(())
        } else {
            Err(format!("Shader parameter {param_name} not found"))
        }
    }

    /// Set a Shader Lab parameter from a raw float (typed entry / scrub-set).
    /// Vector and color types fill from the single value via `coerce_float`;
    /// per-component keys use `set_shaderlab_component`.
    pub fn set_shaderlab_param(
        &mut self,
        effect_id: &str,
        param_name: &str,
        v: f32,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;
        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        let params = effect.shader_params().ok_or_else(|| "Not a Shader Lab effect".to_string())?.to_vec();
        let param = params
            .iter()
            .find(|p| p.name == param_name)
            .ok_or_else(|| format!("Shader parameter {param_name} not found"))?;
        let value = param.coerce_float(v);
        if effect.set_shader_value(param_name, value) {
            Ok(())
        } else {
            Err(format!("Shader parameter {param_name} not found"))
        }
    }

    /// Set one component of a vector/color Shader Lab parameter.
    pub fn set_shaderlab_component(
        &mut self,
        effect_id: &str,
        param_name: &str,
        index: usize,
        v: f32,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;
        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        let resolved = effect.resolved_shader_values();
        let current = resolved
            .iter()
            .find(|(n, _)| n == param_name)
            .map(|(_, v)| v.clone());
        let params = effect.shader_params().ok_or_else(|| "Not a Shader Lab effect".to_string())?.to_vec();
        let param = params
            .iter()
            .find(|p| p.name == param_name)
            .ok_or_else(|| format!("Shader parameter {param_name} not found"))?;
        let lo = param.min.unwrap_or(f32::NEG_INFINITY);
        let hi = param.max.unwrap_or(f32::INFINITY);
        let v = v.clamp(lo, hi);
        let next = match current {
            Some(project::ShaderParamValue::Vec2(mut a)) => {
                if index < 2 { a[index] = v; }
                project::ShaderParamValue::Vec2(a)
            }
            Some(project::ShaderParamValue::Vec3(mut a)) => {
                if index < 3 { a[index] = v; }
                project::ShaderParamValue::Vec3(a)
            }
            Some(project::ShaderParamValue::Vec4(mut a)) => {
                if index < 4 { a[index] = v; }
                project::ShaderParamValue::Vec4(a)
            }
            Some(project::ShaderParamValue::Color(c)) => {
                let mut a = [c.r, c.g, c.b, c.a];
                if index < 4 { a[index] = v.clamp(0.0, 1.0); }
                project::ShaderParamValue::Color(Color::rgba(a[0], a[1], a[2], a[3]))
            }
            _ => param.coerce_float(v),
        };
        if effect.set_shader_value(param_name, next) {
            Ok(())
        } else {
            Err(format!("Shader parameter {param_name} not found"))
        }
    }

    /// Nudge every component of a vector/color Shader Lab parameter
    /// together (linked-vector scrub from the vector widget).
    pub fn nudge_shaderlab_linked(
        &mut self,
        effect_id: &str,
        param_name: &str,
        delta: f32,
    ) -> Result<(), String> {
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        let count = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let layer = comp
                .get_layer(&selected_id)
                .ok_or_else(|| format!("Layer {selected_id} not found"))?;
            let effect = layer
                .get_effect(effect_id)
                .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
            let params = effect.shader_params().ok_or_else(|| "Not a Shader Lab effect".to_string())?.to_vec();
            let param = params
                .iter()
                .find(|p| p.name == param_name)
                .ok_or_else(|| format!("Shader parameter {param_name} not found"))?;
            param.param_type.components()
        };
        for i in 0..count {
            let _ = self.nudge_shaderlab_component(effect_id, param_name, i, delta);
        }
        Ok(())
    }

    /// Set every component of a vector/color Shader Lab parameter to one
    /// value (linked-vector typed entry), in a single undo step.
    pub fn set_shaderlab_all_components(
        &mut self,
        effect_id: &str,
        param_name: &str,
        v: f32,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;
        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        let params = effect.shader_params().ok_or_else(|| "Not a Shader Lab effect".to_string())?.to_vec();
        let param = params
            .iter()
            .find(|p| p.name == param_name)
            .ok_or_else(|| format!("Shader parameter {param_name} not found"))?;
        let lo = param.min.unwrap_or(f32::NEG_INFINITY);
        let hi = param.max.unwrap_or(f32::INFINITY);
        let v = v.clamp(lo, hi);
        let next = match param.param_type {
            project::ShaderParamType::Vec2 => project::ShaderParamValue::Vec2([v, v]),
            project::ShaderParamType::Vec3 => project::ShaderParamValue::Vec3([v, v, v]),
            project::ShaderParamType::Vec4 => project::ShaderParamValue::Vec4([v, v, v, v]),
            project::ShaderParamType::Color => project::ShaderParamValue::Color(Color::rgba(
                v.clamp(0.0, 1.0),
                v.clamp(0.0, 1.0),
                v.clamp(0.0, 1.0),
                1.0,
            )),
            _ => param.coerce_float(v),
        };
        if effect.set_shader_value(param_name, next) {
            Ok(())
        } else {
            Err(format!("Shader parameter {param_name} not found"))
        }
    }

    /// Set several color fields on one effect atomically (gradient editor
    /// presets / stop reversal = one undo step).
    pub fn set_effect_color_pair(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        pairs: &[(&str, Color)],
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        for (field, color) in pairs {
            if !(effect.set_color_value(field, *color) || effect.set_stock_color(field, *color)) {
                return Err(format!("Color field {field} not found on effect {effect_id}"));
            }
        }
        Ok(())
    }

    /// Validate and apply new Shader Lab source on the selected layer.
    /// Success swaps in the source (UI regenerates); failure keeps the
    /// last-good source running and records the error for the panel.
    pub fn apply_shader_source(
        &mut self,
        effect_id: &str,
        source: &str,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        match renderer::shader_lab::compile_source(source) {
            Err(msg) => {
                let comp = self
                    .active_composition_mut()
                    .ok_or_else(|| "No active composition".to_string())?;
                let layer = comp
                    .get_layer_mut(&selected_id)
                    .ok_or_else(|| format!("Layer {selected_id} not found"))?;
                let effect = layer
                    .get_effect_mut(effect_id)
                    .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
                effect.set_shader_error(Some(msg.clone()));
                Err(msg)
            }
            Ok(_) => {
                let comp = self
                    .active_composition_mut()
                    .ok_or_else(|| "No active composition".to_string())?;
                let layer = comp
                    .get_layer_mut(&selected_id)
                    .ok_or_else(|| format!("Layer {selected_id} not found"))?;
                let effect = layer
                    .get_effect_mut(effect_id)
                    .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
                effect.set_shader_source(source);
                Ok(())
            }
        }
    }

    /// Apply typed text from the value editor to the open scrub key.
    /// Accepts plain numbers with an optional unit suffix ("px", "%", "deg").
    /// Returns true when a value was applied.
    pub fn commit_typed_value(&mut self, text: &str) -> bool {
        let Some(prop) = self.value_edit_key.clone() else {
            return false;
        };
        let numeric: String = text
            .trim()
            .chars()
            .take_while(|c| {
                c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+' || *c == 'e' || *c == 'E'
            })
            .collect();
        match numeric.parse::<f32>() {
            Ok(v) => {
                // Timeline rows use `tl:<layer_id>:<key>` edit keys (layer ids
                // never contain ':', so the first segment is the layer).
                if let Some(rest) = prop.strip_prefix("tl:") {
                    if let Some((lid, key)) = rest.split_once(':') {
                        return self.set_timeline_value(lid, key, v);
                    }
                    return false;
                }
                self.set_scrub_value(&prop, v)
            }
            Err(_) => false,
        }
    }

    /// Close keyboard entry, dropping the editor. Returns true when open.
    /// Also ends fast-preview: the next render recomputes full quality.
    pub fn end_value_edit_state(&mut self) -> bool {
        self.preview_fast = false;
        if self.value_edit_key.is_some() || self.value_editor.is_some() {
            self.value_edit_key = None;
            self.value_editor = None;
            self.value_editor_sub = None;
            true
        } else {
            false
        }
    }

    /// Return reference to the active composition.
    pub fn active_composition(&self) -> Option<&Composition> {
        self.project.get_composition(&self.active_comp_id)
    }

    /// Return mutable reference to the active composition.
    pub fn active_composition_mut(&mut self) -> Option<&mut Composition> {
        self.project.get_composition_mut(&self.active_comp_id)
    }

    /// Return reference to the currently selected layer.
    pub fn selected_layer(&self) -> Option<&Layer> {
        let comp = self.active_composition()?;
        let sel_id = self.selected_layer_id.as_ref()?;
        comp.get_layer(sel_id)
    }

    /// Return mutable reference to the currently selected layer.
    pub fn selected_layer_mut(&mut self) -> Option<&mut Layer> {
        let sel_id = self.selected_layer_id.clone()?;
        let comp = self.active_composition_mut()?;
        comp.get_layer_mut(&sel_id)
    }

    /// Set the selected layer ID.
    pub fn select_layer(&mut self, layer_id: Option<String>) {
        self.selected_layer_id = layer_id;
    }

    // --- Undo / redo (snapshot history) ---

    /// Record the current state for undo. Call BEFORE performing a
    /// structural or value mutation (gesture starts, menu actions,
    /// button clicks). Coalesces rapid repeats from the same caller.
    pub fn checkpoint(&mut self) {
        let entry = HistoryEntry::capture(self);
        // Coalesce: skip when identical to the last checkpoint (bursty
        // wheel ticks and drag frames share one undo step).
        if self.undo_stack.last().map(|e| {
            e.project == entry.project
                && e.active_comp_id == entry.active_comp_id
                && e.selected_layer_id == entry.selected_layer_id
        }).unwrap_or(false) {
            return;
        }
        self.undo_stack.push(entry);
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Step back to the last checkpoint. Returns false when empty.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(HistoryEntry::capture(self));
        entry.restore(self);
        true
    }

    /// Re-apply an undone checkpoint. Returns false when empty.
    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(HistoryEntry::capture(self));
        entry.restore(self);
        true
    }

    // --- Project file management ---

    /// Start a fresh project (used by File > New Project). Clears history.
    pub fn new_project(&mut self, name: &str) {
        let clean = if name.trim().is_empty() { "Untitled Project" } else { name.trim() };
        self.project = Project::new(
            format!("proj_{}", clean.to_lowercase().replace(' ', "_")),
            clean,
        );
        let mut comp = Composition::hd_1080p_30fps("comp_main", "Main Composition", 5.0);
        // Seed the same starter layers as a fresh session so a new
        // project never opens empty.
        let fps = 30.0;
        let tc0 = TimeCode::from_frames(0, fps);
        let tc150 = TimeCode::from_frames(150, fps);
        let mut bg = Layer::solid(
            "layer_bg",
            "Background Solid",
            Color::from_hex("#121316").unwrap_or(Color::BLACK),
            1920,
            1080,
            tc0,
            tc150,
        );
        bg.transform.position.set_value(Vec2::ZERO);
        bg.transform.anchor_point.set_value(Vec2::new(960.0, 540.0));
        let _ = comp.add_layer(bg);
        self.project.add_composition(comp).ok();
        self.active_comp_id = "comp_main".to_string();
        self.selected_layer_id = None;
        self.is_playing = false;
        self.clock = PlaybackClock::from_composition(
            self.project.get_composition(&self.active_comp_id).unwrap(),
        );
        self.project_path = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Serialize the project to a JSON file and remember the path.
    pub fn save_project_to(&mut self, path: &std::path::Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.project)
            .map_err(|e| format!("Serialize failed: {e}"))?;
        std::fs::write(path, text).map_err(|e| format!("Write failed: {e}"))?;
        self.project_path = Some(path.to_path_buf());
        self.remember_recent(path);
        Ok(())
    }

    /// Save to the remembered path, or fail when the project is untitled.
    pub fn save_project(&mut self) -> Result<(), String> {
        let path = self.project_path.clone().ok_or_else(|| "No path yet".to_string())?;
        self.save_project_to(&path)
    }

    /// Load a project JSON file, replacing the session. Clears history.
    pub fn load_project_from(&mut self, path: &std::path::Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Read failed: {e}"))?;
        let project: Project =
            serde_json::from_str(&text).map_err(|e| format!("Parse failed: {e}"))?;
        let Some(first) = project.compositions.first() else {
            return Err("Project has no compositions".to_string());
        };
        self.active_comp_id = first.id.clone();
        self.project = project;
        self.selected_layer_id = None;
        self.is_playing = false;
        self.clock = PlaybackClock::from_composition(
            self.project.get_composition(&self.active_comp_id).unwrap(),
        );
        self.project_path = Some(path.to_path_buf());
        self.remember_recent(path);
        self.undo_stack.clear();
        self.redo_stack.clear();
        Ok(())
    }

    /// File stem for window titles and menu labels.
    pub fn project_display_name(&self) -> String {
        if let Some(path) = &self.project_path {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                return stem.to_string();
            }
        }
        self.project.name.clone()
    }

    fn remember_recent(&mut self, path: &std::path::Path) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_path_buf());
        self.recent_projects.truncate(8);
    }

    /// Toggle play/pause transport state.
    pub fn toggle_playback(&mut self) {
        self.is_playing = !self.is_playing;
        if self.is_playing {
            self.clock.play();
        } else {
            self.clock.pause();
        }
    }

    /// Set the sticky viewport preview resolution (View menu).
    pub fn set_preview_quality(&mut self, quality: PreviewQuality) {
        self.preview_quality = quality;
    }

    /// Effective raster divisor right now: gestures/playback always halve,
    /// otherwise the sticky preference applies.
    pub fn preview_divisor(&self) -> u32 {
        if self.is_playing || self.preview_fast {
            2
        } else {
            self.preview_quality.divisor()
        }
    }

    /// Play transport.
    pub fn play(&mut self) {
        self.is_playing = true;
        self.clock.play();
    }

    /// Pause transport.
    pub fn pause(&mut self) {
        self.is_playing = false;
        self.clock.pause();
    }

    /// Advance the clock by delta time `dt`. Returns true if the visual frame changed.
    pub fn tick(&mut self, dt: Duration) -> bool {
        if !self.is_playing {
            return false;
        }
        let res = self.clock.tick(dt);
        res.frame_changed
    }

    /// Step forward exactly 1 frame.
    pub fn step_forward(&mut self) {
        self.pause();
        self.clock.step_next_frame();
    }

    /// Step backward exactly 1 frame.
    pub fn step_backward(&mut self) {
        self.pause();
        self.clock.step_prev_frame();
    }

    /// Seek to a specific frame number.
    pub fn seek_frame(&mut self, frame: i64) {
        self.clock.seek_frame(frame);
    }

    /// Return the current quantized timecode of the playhead.
    pub fn current_timecode(&self) -> TimeCode {
        self.clock.timecode()
    }

    /// Return the current frame index.
    pub fn current_frame(&self) -> i64 {
        self.clock.current_frame()
    }

    /// Jump transport to beginning of composition.
    pub fn jump_to_start(&mut self) {
        self.pause();
        self.clock.jump_to_comp_start();
    }

    /// Jump transport to end of composition.
    pub fn jump_to_end(&mut self) {
        self.pause();
        self.clock.jump_to_comp_end();
    }

    /// Toggle visibility (eye icon) of a layer.
    pub fn toggle_layer_visibility(&mut self, layer_id: &str) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.visible = !layer.visible;
            }
        }
    }

    /// Toggle solo state of a layer.
    pub fn toggle_layer_solo(&mut self, layer_id: &str) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.set_solo(!layer.is_solo());
            }
        }
    }

    /// Add a new colored solid layer into the active composition.
    /// Create a new composition with user-chosen settings (After Effects-style
    /// "New Composition" dialog) and make it active. The name is auto-derived
    /// (`Composition N`) when `name` is empty.
    pub fn add_composition(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        frame_rate: f64,
        duration_secs: f64,
        background: Color,
    ) -> Result<String, String> {
        self.checkpoint();
        let mut counter = self.project.compositions.len() + 1;
        let mut id = format!("comp_{counter}");
        while self.project.get_composition(&id).is_some() {
            counter += 1;
            id = format!("comp_{counter}");
        }
        let fps = if frame_rate > 0.0 { frame_rate } else { 30.0 };
        let comp_name = if name.trim().is_empty() {
            format!("Composition {counter}")
        } else {
            name.trim().to_string()
        };
        let mut comp = Composition::new(
            &id,
            comp_name,
            width.max(1),
            height.max(1),
            fps,
            TimeCode::from_seconds(duration_secs.max(1.0), fps),
        );
        comp.background_color = background;
        self.project
            .add_composition(comp)
            .map_err(|e| format!("Failed to add composition: {e:?}"))?;
        self.active_comp_id = id.clone();
        self.selected_layer_id = None;
        Ok(id)
    }

    pub fn add_solid_layer(
        &mut self,
        name: &str,
        color: Color,
        width: u32,
        height: u32,
    ) -> Result<String, String> {
        self.checkpoint();
        let (id, in_pt, out_pt, _comp_w, _comp_h) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_solid_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_solid_{counter}");
            }
            (
                id,
                TimeCode::zero(comp.frame_rate),
                comp.duration,
                comp.width,
                comp.height,
            )
        };

        let mut layer = Layer::solid(&id, name, color, width, height, in_pt, out_pt);
        layer.transform.position.set_value(Vec2::ZERO);
        layer.transform.anchor_point.set_value(Vec2::new(
            (width / 2) as f32,
            (height / 2) as f32,
        ));

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
            .map_err(|e| format!("Failed to add layer: {e:?}"))?;

        self.selected_layer_id = Some(id.clone());
        Ok(id)
    }

    /// Add a new Adjustment layer to the active composition.
    pub fn add_adjustment_layer(&mut self, name: Option<&str>) -> Result<String, String> {
        self.checkpoint();
        let (id, in_pt, out_pt, comp_w, comp_h) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_adjustment_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_adjustment_{counter}");
            }
            (
                id,
                TimeCode::zero(comp.frame_rate),
                comp.duration,
                comp.width,
                comp.height,
            )
        };

        let layer_name = name.unwrap_or("Adjustment Layer 1");
        let mut layer = Layer::adjustment(&id, layer_name, in_pt, out_pt);
        layer.transform.position.set_value(Vec2::ZERO);
        layer.transform.anchor_point.set_value(Vec2::new(
            (comp_w / 2) as f32,
            (comp_h / 2) as f32,
        ));

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
            .map_err(|e| format!("Failed to add adjustment layer: {e:?}"))?;

        self.selected_layer_id = Some(id.clone());
        Ok(id)
    }

    /// Import a media file from disk (image or video), register it in project assets,
    /// and add a new centered layer to the active composition.
    pub fn import_media_file(&mut self, path: PathBuf) -> Result<String, String> {
        self.checkpoint();
        let (_comp_w, _comp_h, frame_rate, duration) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            (comp.width, comp.height, comp.frame_rate, comp.duration)
        };

        // Determine dimensions: if image, use image::image_dimensions; otherwise default 1920x1080
        let (width, height) = match image::image_dimensions(&path) {
            Ok((w, h)) => (w, h),
            Err(_) => (1920, 1080),
        };

        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Media")
            .to_string();

        let asset_id = {
            let mut counter = self.project.assets.len() + 1;
            let mut id = format!("asset_media_{counter}");
            while self.project.assets.iter().any(|a| a.id == id) {
                counter += 1;
                id = format!("asset_media_{counter}");
            }
            id
        };

        let asset = Asset::from_path(&asset_id, &file_stem, &path);
        let is_image = asset.is_image();
        self.project.assets.push(asset);

        let layer_id = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_media_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_media_{counter}");
            }
            id
        };

        let in_pt = TimeCode::zero(frame_rate);
        let out_pt = duration;

        let mut layer = if is_image {
            Layer::image_with_dimensions(&layer_id, &file_stem, &asset_id, width, height, in_pt, out_pt)
        } else {
            Layer::video(&layer_id, &file_stem, &asset_id, in_pt, in_pt, out_pt)
        };

        layer.transform.position.set_value(Vec2::ZERO);
        layer.transform.anchor_point.set_value(Vec2::new(
            (width / 2) as f32,
            (height / 2) as f32,
        ));

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
            .map_err(|e| format!("Failed to add media layer: {e:?}"))?;

        self.selected_layer_id = Some(layer_id.clone());
        Ok(layer_id)
    }

    /// Add an existing asset from the project into the active composition as a layer.
    pub fn add_asset_layer(&mut self, asset_id: &str) -> Result<String, String> {
        self.checkpoint();
        let (_comp_w, _comp_h, frame_rate, duration) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            (comp.width, comp.height, comp.frame_rate, comp.duration)
        };

        let asset = self
            .project
            .get_asset(asset_id)
            .cloned()
            .ok_or_else(|| format!("Asset {asset_id} not found"))?;

        let is_image = asset.is_image();
        let name = asset.name.clone();

        let (width, height) = match image::image_dimensions(&asset.path) {
            Ok((w, h)) => (w, h),
            Err(_) => (1920, 1080),
        };

        let layer_id = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_asset_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_asset_{counter}");
            }
            id
        };

        let in_pt = TimeCode::zero(frame_rate);
        let out_pt = duration;

        let mut layer = if is_image {
            Layer::image_with_dimensions(&layer_id, &name, asset_id, width, height, in_pt, out_pt)
        } else {
            Layer::video(&layer_id, &name, asset_id, in_pt, in_pt, out_pt)
        };

        layer.transform.position.set_value(Vec2::ZERO);
        layer.transform.anchor_point.set_value(Vec2::new(
            (width / 2) as f32,
            (height / 2) as f32,
        ));

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
            .map_err(|e| format!("Failed to add asset layer: {e:?}"))?;

        self.selected_layer_id = Some(layer_id.clone());
        Ok(layer_id)
    }

    /// Import a generated sample image (400x400 PNG gradient) for immediate testing.
    pub fn import_sample_image(&mut self) -> Result<String, String> {
        let sample_path = std::env::temp_dir().join("motion_studio_sample_gradient.png");
        let mut imgbuf = image::ImageBuffer::new(400, 400);
        for (x, y, pixel) in imgbuf.enumerate_pixels_mut() {
            let r = (x as f32 / 400.0 * 255.0) as u8;
            let g = (y as f32 / 400.0 * 255.0) as u8;
            let b = 220u8;
            *pixel = image::Rgba([r, g, b, 255]);
        }
        imgbuf
            .save(&sample_path)
            .map_err(|e| format!("Failed to create sample image: {e}"))?;
        self.import_media_file(sample_path)
    }

    /// Import a generated sample video placeholder for immediate testing.
    pub fn import_sample_video(&mut self) -> Result<String, String> {
        let sample_path = std::env::temp_dir().join("motion_studio_sample_footage.mp4");
        if !sample_path.exists() {
            let _ = std::fs::write(&sample_path, b"DEMO_MP4_VIDEO_FOOTAGE");
        }
        self.import_media_file(sample_path)
    }

    /// Pre-defined GLSL shader presets that users can immediately apply and learn from.
    pub const GLSL_PRESETS: &'static [(&'static str, &'static str)] = &[
        ("Default Boost", project::Effect::default_glsl_code()),
        (
            "Color Wave",
            "// Color Wave Shader\nvoid mainImage(out vec4 fragColor, in vec2 uv, in vec4 inColor) {\n    float wave = sin(uv.x * param3 * 10.0 + param1 * 3.0) * 0.5 + 0.5;\n    vec3 col = mix(inColor.rgb, vec3(wave, 1.0 - wave, 0.8), param2 / 100.0);\n    fragColor = vec4(col, inColor.a * (param4 / 100.0));\n}",
        ),
        (
            "Glow Shimmer",
            "// Glow Shimmer Shader\nvoid mainImage(out vec4 fragColor, in vec2 uv, in vec4 inColor) {\n    float glow = 1.0 + (param2 / 50.0) * sin(param1 * 4.0);\n    fragColor = vec4(clamp(inColor.rgb * glow, 0.0, 1.0), inColor.a);\n}",
        ),
        (
            "CRT Scanlines",
            "// CRT Scanlines Shader\nvoid mainImage(out vec4 fragColor, in vec2 uv, in vec4 inColor) {\n    float line = sin(uv.y * param3 * 100.0) * 0.5 + 0.5;\n    vec3 col = inColor.rgb * (1.0 - (param2 / 100.0) * (1.0 - line));\n    fragColor = vec4(col, inColor.a);\n}",
        ),
    ];

    /// Delete the currently selected layer from the active composition.
    pub fn delete_selected_layer(&mut self) -> Result<String, String> {
        self.checkpoint();
        let sel_id = self
            .selected_layer_id
            .take()
            .ok_or_else(|| "No layer selected to delete".to_string())?;

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;

        comp.remove_layer(&sel_id)
            .ok_or_else(|| format!("Layer {sel_id} not found in composition"))?;

        // Automatically select the nearest remaining layer if any
        self.selected_layer_id = comp.layers.last().map(|l| l.id.clone());
        Ok(sel_id)
    }

    /// Remove a layer by its ID.
    pub fn remove_layer_by_id(&mut self, layer_id: &str) -> Result<(), String> {
        self.checkpoint();
        let is_selected = self.selected_layer_id.as_deref() == Some(layer_id);
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;

        comp.remove_layer(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found in composition"))?;

        if is_selected {
            self.selected_layer_id = comp.layers.last().map(|l| l.id.clone());
        }
        Ok(())
    }

    /// Move the currently selected layer up in the stack (toward index 0, on top visually).
    pub fn move_selected_layer_up(&mut self) -> Result<(), String> {
        self.checkpoint();
        let sel_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.move_layer_up(&sel_id)
    }

    /// Move the currently selected layer down in the stack (away from index 0, lower visually).
    pub fn move_selected_layer_down(&mut self) -> Result<(), String> {
        self.checkpoint();
        let sel_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.move_layer_down(&sel_id)
    }

    /// Move a layer identified by ID up in the visual stack.
    pub fn move_layer_up(&mut self, layer_id: &str) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;

        let current_idx = comp
            .layer_index(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        if current_idx > 0 {
            comp.move_layer(current_idx, current_idx - 1)
                .map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }

    /// Move a layer identified by ID down in the visual stack.
    pub fn move_layer_down(&mut self, layer_id: &str) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;

        let current_idx = comp
            .layer_index(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        if current_idx + 1 < comp.layers.len() {
            comp.move_layer(current_idx, current_idx + 1)
                .map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }

    /// Reorder a layer directly to a specific target index.
    pub fn reorder_layer(&mut self, layer_id: &str, new_index: usize) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;

        comp.reorder_layer(layer_id, new_index)
            .map_err(|e| format!("{e:?}"))
    }

    /// Drag-and-drop reorder: move a layer to the hovered row's slot (one
    /// undo step). No-op when the target equals the current index.
    pub fn move_layer_to(&mut self, layer_id: &str, index: usize) -> Result<(), String> {
        let current = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            comp.layer_index(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?
        };
        if current == index {
            return Ok(());
        }
        self.checkpoint();
        self.reorder_layer(layer_id, index)
    }

    /// Delete an asset from project by its asset ID.
    pub fn delete_asset(&mut self, asset_id: &str) -> Result<(), String> {
        let idx = self
            .project
            .assets
            .iter()
            .position(|a| a.id == asset_id)
            .ok_or_else(|| format!("Asset {asset_id} not found"))?;
        self.project.assets.remove(idx);
        Ok(())
    }

    /// Update custom GLSL / WGSL shader code on an effect.
    pub fn update_glsl_code(&mut self, effect_id: &str, code: &str) -> Result<(), String> {
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found"))?;

        if effect.set_glsl_code(code) {
            Ok(())
        } else {
            Err(format!("Effect {effect_id} is not a GLSL shader effect"))
        }
    }

    /// Add an effect of `effect_type` to the currently selected layer.
    pub fn add_effect_to_selected_layer(
        &mut self,
        effect_type: EffectType,
    ) -> Result<String, String> {
        self.checkpoint();
        let type_name = effect_type.type_name();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;

        let slug = type_name.to_lowercase().replace([' ', '&'], "_");
        let fx_id = format!("fx_{slug}_{}", layer.effects.len() + 1);

        let effect = Effect::new(&fx_id, type_name, effect_type);
        let id = layer.add_effect(effect);
        Ok(id)
    }

    /// Remove an effect by ID from a specific layer.
    pub fn remove_layer_effect(&mut self, layer_id: &str, effect_id: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        layer
            .remove_effect(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        Ok(())
    }

    /// Remove an effect by ID from the currently selected layer.
    pub fn remove_effect_from_selected_layer(&mut self, effect_id: &str) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.remove_layer_effect(&selected_id, effect_id)
    }

    /// Toggle enabled state of an effect on a specific layer.
    pub fn toggle_layer_effect_enabled(&mut self, layer_id: &str, effect_id: &str) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        effect.toggle_enabled();
        Ok(())
    }

    /// Toggle enabled state of an effect on the currently selected layer.
    pub fn toggle_effect_enabled(&mut self, effect_id: &str) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.toggle_layer_effect_enabled(&selected_id, effect_id)
    }

    // --- Masks (first-class vector masks on the shared Path model) ---

    /// Add a mask with a default centered rectangle path.
    pub fn add_mask_to_layer(&mut self, layer_id: &str) -> Result<String, String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let id = next_mask_id(layer);
        let name = format!("Mask {}", layer.masks.len() + 1);
        layer.masks.push(Mask::new(&id, name));
        Ok(id)
    }

    /// Add a mask wrapping an explicit path (pen / shape interop).
    pub fn add_mask_path_to_layer(
        &mut self,
        layer_id: &str,
        name: &str,
        path: Path,
    ) -> Result<String, String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let id = next_mask_id(layer);
        layer.masks.push(Mask::with_path(&id, name, path));
        Ok(id)
    }

    /// Remove a mask from a layer.
    pub fn remove_layer_mask(&mut self, layer_id: &str, mask_id: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        layer
            .remove_mask(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if self.active_mask_edit.as_ref().map(|(_, m)| m == mask_id).unwrap_or(false) {
            self.active_mask_edit = None;
        }
        Ok(())
    }

    /// Toggle a mask's enabled state.
    pub fn toggle_mask_enabled(&mut self, layer_id: &str, mask_id: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        mask.toggle_enabled();
        Ok(())
    }

    /// Cycle a mask's combine mode (Add → Subtract → Intersect →
    /// Difference → None).
    pub fn cycle_mask_mode(&mut self, layer_id: &str, mask_id: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        mask.mode = mask.mode.cycle();
        Ok(())
    }

    /// Toggle a mask's invert flag.
    pub fn toggle_mask_invert(&mut self, layer_id: &str, mask_id: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        mask.invert = !mask.invert;
        Ok(())
    }

    /// Nudge a mask scalar param (opacity / feather / expansion).
    pub fn nudge_mask_param(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        param_name: &str,
        delta: f32,
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        if mask.nudge_param(param_name, delta) {
            Ok(())
        } else {
            Err(format!("Unknown mask param {param_name}"))
        }
    }

    /// Toggle a mask-path keyframe at the playhead (diamond behavior: adds
    /// the evaluated path, or removes the keyframe sitting at the playhead).
    /// Returns true when a keyframe now exists at the playhead.
    pub fn toggle_mask_path_keyframe_at_current_time(
        &mut self,
        layer_id: &str,
        mask_id: &str,
    ) -> Result<bool, String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        Ok(mask.path.toggle_keyframe(current_tc, mask.path.evaluate_at(&current_tc)))
    }

    /// Seek the playhead to the neighbouring mask-path keyframe.
    /// `dir < 0` seeks previous, `dir > 0` seeks next. Returns false when
    /// there is no keyframe in that direction.
    pub fn seek_mask_path_keyframe(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        dir: i8,
    ) -> Result<bool, String> {
        let current_tc = self.clock.timecode();
        let next = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let layer = comp
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?;
            let mask = layer
                .get_mask(mask_id)
                .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
            if dir < 0 {
                mask.path.previous_keyframe_time(&current_tc)
            } else {
                mask.path.next_keyframe_time(&current_tc)
            }
        };
        match next {
            Some(tc) => {
                self.clock.seek(tc);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Rewrite one mask node (checkpointed discrete edit).
    pub fn set_mask_point(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        index: usize,
        pos: Vec2,
    ) -> Result<(), String> {
        self.checkpoint();
        self.move_mask_point_live(layer_id, mask_id, index, pos)
    }

    /// Rewrite one mask node without checkpointing (node drags; the drag
    /// start checkpoints once, same-time keyframes collapse).
    pub fn move_mask_point_live(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        index: usize,
        pos: Vec2,
    ) -> Result<(), String> {
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        let node = mask
            .path
            .value
            .points
            .get_mut(index)
            .ok_or_else(|| format!("Mask node {index} out of range"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        node.move_to(pos);
        if mask.path.is_animated() {
            let snapshot = mask.path.value.clone();
            mask.path.add_keyframe(Keyframe::new(current_tc, snapshot));
        }
        Ok(())
    }

    /// Rewrite one mask tangent handle from an absolute tip (checkpointed).
    pub fn set_mask_handle(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        index: usize,
        is_in: bool,
        tip: Vec2,
    ) -> Result<(), String> {
        self.checkpoint();
        self.move_mask_handle_live(layer_id, mask_id, index, is_in, tip)
    }

    /// Rewrite one mask tangent handle without checkpointing (drag path).
    pub fn move_mask_handle_live(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        index: usize,
        is_in: bool,
        tip: Vec2,
    ) -> Result<(), String> {
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        let node = mask
            .path
            .value
            .points
            .get_mut(index)
            .ok_or_else(|| format!("Mask node {index} out of range"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        if is_in {
            node.set_in_abs(tip);
        } else {
            node.set_out_abs(tip);
        }
        if mask.path.is_animated() {
            let snapshot = mask.path.value.clone();
            mask.path.add_keyframe(Keyframe::new(current_tc, snapshot));
        }
        Ok(())
    }

    /// Delete a mask node (keeps ≥... allows emptying to a moveless path).
    pub fn delete_mask_point(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        index: usize,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        mask.path.value.remove_point(index).ok_or_else(|| {
            format!("Mask node {index} out of range")
        })?;
        if mask.path.is_animated() {
            let snapshot = mask.path.value.clone();
            mask.path.add_keyframe(Keyframe::new(current_tc, snapshot));
        }
        Ok(())
    }

    /// Append a corner node to a mask path.
    pub fn append_mask_point(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        pos: Vec2,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        mask.path.value.line_to(pos);
        if mask.path.is_animated() {
            let snapshot = mask.path.value.clone();
            mask.path.add_keyframe(Keyframe::new(current_tc, snapshot));
        }
        Ok(())
    }

    /// Cycle a mask node's kind (Corner → Smooth → Symmetric → Auto).
    pub fn cycle_mask_point_kind(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        index: usize,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        let node = mask
            .path
            .value
            .points
            .get_mut(index)
            .ok_or_else(|| format!("Mask node {index} out of range"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        node.convert_to(match node.kind {
            PathPointKind::Corner => PathPointKind::Smooth,
            PathPointKind::Smooth => PathPointKind::Symmetric,
            PathPointKind::Symmetric => PathPointKind::Auto,
            PathPointKind::Auto => PathPointKind::Corner,
        });
        if mask.path.is_animated() {
            let snapshot = mask.path.value.clone();
            mask.path.add_keyframe(Keyframe::new(current_tc, snapshot));
        }
        Ok(())
    }

    /// Close / open a mask path.
    pub fn set_mask_closed(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        closed: bool,
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        mask.path.value.closed = closed;
        Ok(())
    }

    /// Toggle keyframe animation for a mask scalar param: enables with a
    /// keyframe at the playhead, or clears all keyframes when animated.
    pub fn toggle_mask_param_animation(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        param_name: &str,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        let prop = mask
            .get_param_property_mut(param_name)
            .ok_or_else(|| format!("Unknown mask param {param_name}"))?;
        if prop.is_animated() {
            prop.clear_keyframes();
        } else {
            let val = prop.value;
            prop.add_keyframe(Keyframe::new(current_tc, val));
        }
        Ok(())
    }

    /// Set the viewport Path Editor target (None exits edit mode).
    pub fn set_active_mask_edit(&mut self, target: Option<(String, String)>) {
        self.active_mask_edit = target;
    }

    // --- Mask creation methods (After Effects parity) ---------------------

    /// Rename a mask (blank names fall back to "Mask").
    pub fn rename_mask(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        name: &str,
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        mask.rename(name);
        Ok(())
    }

    /// Lock / unlock a mask (locked masks reject every mutation except
    /// enable/rename/delete).
    pub fn set_mask_locked(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        locked: bool,
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        mask.locked = locked;
        Ok(())
    }

    /// Rewrite a mask path from a numeric bounding box (the Mask Shape
    /// dialog: Rectangle / Ellipse + explicit x/y/w/h in layer-local px).
    /// Snapshots a keyframe when the path is animated.
    pub fn set_mask_shape_numeric(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        kind: MaskShapeKind,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let mask = layer
            .get_mask_mut(mask_id)
            .ok_or_else(|| format!("Mask {mask_id} not found on layer"))?;
        if mask.locked {
            return Err(format!("Mask {mask_id} is locked"));
        }
        mask.path.set_value(kind.path(x, y, w, h));
        if mask.path.is_animated() {
            let snapshot = mask.path.value.clone();
            mask.path.add_keyframe(Keyframe::new(current_tc, snapshot));
        }
        Ok(())
    }

    /// Add a rectangular mask exactly covering the layer's content box
    /// (After Effects: double-click a shape tool → mask the size of the
    /// layer).
    pub fn add_layer_sized_mask(&mut self, layer_id: &str) -> Result<String, String> {
        let (w, h) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let layer = comp
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?;
            self.layer_content_dims(layer)
        };
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let id = next_mask_id(layer);
        let name = format!("Mask {}", layer.masks.len() + 1);
        layer
            .masks
            .push(Mask::with_path(&id, name, Path::rectangle(0.0, 0.0, w, h)));
        Ok(id)
    }

    /// Add a centered rectangle/ellipse mask (Mask Shape dialog primer:
    /// callers open the numeric dialog right after for exact numbers).
    pub fn add_shaped_mask(
        &mut self,
        layer_id: &str,
        kind: MaskShapeKind,
    ) -> Result<String, String> {
        let (w, h) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let layer = comp
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?;
            self.layer_content_dims(layer)
        };
        let (bw, bh) = (w.min(400.0).max(8.0), h.min(300.0).max(8.0));
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let id = next_mask_id(layer);
        let name = format!("Mask {}", layer.masks.len() + 1);
        let path = kind.path((w - bw) / 2.0, (h - bh) / 2.0, bw, bh);
        layer.masks.push(Mask::with_path(&id, name, path));
        Ok(id)
    }

    /// Content-box dims for a layer in layer-local px (mirrors the raster
    /// estimates so full-size masks line up with pixels).
    fn layer_content_dims(&self, layer: &Layer) -> (f32, f32) {
        match &layer.source {
            LayerSource::Solid { width, height, .. } => (*width as f32, *height as f32),
            LayerSource::Image { asset_id } => self
                .project
                .get_asset(asset_id)
                .and_then(|a| image::image_dimensions(&a.path).ok())
                .map(|(w, h)| (w as f32, h as f32))
                .unwrap_or((1920.0, 1080.0)),
            LayerSource::Video { .. } => (1920.0, 1080.0),
            LayerSource::Text { text, font_size, .. } => {
                let len = text.value.chars().count().max(1) as f32;
                let fs = font_size.value;
                ((len * fs * 0.6 + 40.0).max(100.0), (fs * 1.4 + 20.0).max(40.0))
            }
            LayerSource::Shape { shape_type } => match shape_type {
                ShapeType::Rectangle { width, height, .. } => (width.value, height.value),
                ShapeType::Ellipse { radius_x, radius_y, .. } => {
                    (radius_x.value * 2.0, radius_y.value * 2.0)
                }
                ShapeType::Path { path_data, .. } => {
                    match Path::from_svg(path_data).frame(8.0) {
                        Some((_, size)) => (size.x, size.y),
                        None => (400.0, 300.0),
                    }
                }
            },
            LayerSource::NestedComposition { composition_id } => self
                .project
                .get_composition(composition_id)
                .map(|c| (c.width as f32, c.height as f32))
                .unwrap_or((1920.0, 1080.0)),
            _ => self
                .active_composition()
                .map(|c| (c.width as f32, c.height as f32))
                .unwrap_or((1920.0, 1080.0)),
        }
    }

    /// Copy a shape layer's path into a mask on `dst_layer_id` (After
    /// Effects: convert a shape path to a mask path). Coordinates transfer
    /// raw (source-local); reposition afterwards when the layers differ —
    /// the same offset caveat AE documents for path pasting.
    pub fn shape_path_to_mask(
        &mut self,
        src_layer_id: &str,
        dst_layer_id: &str,
    ) -> Result<String, String> {
        let path = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let src = comp
                .get_layer(src_layer_id)
                .ok_or_else(|| format!("Layer {src_layer_id} not found"))?;
            match &src.source {
                LayerSource::Shape { shape_type } => match shape_type {
                    ShapeType::Rectangle { width, height, .. } => {
                        Path::rectangle(0.0, 0.0, width.value, height.value)
                    }
                    ShapeType::Ellipse { radius_x, radius_y, .. } => Path::ellipse(
                        radius_x.value,
                        radius_y.value,
                        radius_x.value,
                        radius_y.value,
                    ),
                    ShapeType::Path { path_data, .. } => Path::from_svg(path_data),
                },
                _ => return Err(format!("Layer {src_layer_id} is not a shape layer")),
            }
        };
        self.add_mask_path_to_layer(dst_layer_id, "Shape Path", path)
    }

    /// Paste a layer's Position keyframes into a new mask path on
    /// `dst_layer_id` (After Effects: motion path → mask path). Each
    /// position sample is mapped from composition space into the
    /// destination's layer space at its own keyframe time.
    pub fn motion_path_to_mask(
        &mut self,
        src_layer_id: &str,
        dst_layer_id: &str,
    ) -> Result<String, String> {
        let current_tc = self.clock.timecode();
        // Snapshot source samples (read-only first for clean borrows).
        let samples: Vec<(TimeCode, Vec2)> = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let src = comp
                .get_layer(src_layer_id)
                .ok_or_else(|| format!("Layer {src_layer_id} not found"))?;
            let keys = src.transform.position.keyframes();
            if keys.is_empty() {
                vec![(current_tc.clone(), src.transform.position.value)]
            } else {
                keys.iter().map(|k| (k.time.clone(), k.value)).collect()
            }
        };
        if samples.len() < 2 {
            return Err("Need at least 2 position samples for a motion path".to_string());
        }
        // Map into the destination's layer space (per-sample time).
        let mut pts = Vec::with_capacity(samples.len());
        for (tc, comp_pos) in &samples {
            let local = self
                .layer_world_matrix_at(dst_layer_id, tc)
                .and_then(|w| w.inverse())
                .map(|inv| inv.transform_point(*comp_pos))
                .ok_or_else(|| format!("Cannot map into layer {dst_layer_id}"))?;
            pts.push(local);
        }
        let mut path = Path::new();
        for p in pts {
            path.line_to(p);
        }
        // One mask-path keyframe per motion keyframe (all holding the full
        // trajectory shape, exactly like AE's paste result).
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(dst_layer_id)
            .ok_or_else(|| format!("Layer {dst_layer_id} not found"))?;
        let id = next_mask_id(layer);
        let mut mask = Mask::with_path(&id, "Motion Path", path);
        mask.path.set_animated(true);
        for (tc, _) in &samples {
            mask.path.add_keyframe(Keyframe::new(tc.clone(), mask.path.value.clone()));
        }
        layer.masks.push(mask);
        Ok(id)
    }

    // --- Auto-trace + Create Masks From Text ------------------------------
    //
    // After Effects parity: Layer > Auto-trace converts a channel into
    // Bézier masks (current frame or work-area keyframes, same layer or a
    // new solid); Layer > Create Masks From Text rasterizes the text and
    // traces it (holes become Subtract masks).

    /// Evaluate the active composition at an explicit timecode.
    fn evaluate_at(&self, tc: &TimeCode) -> Result<compositor::EvaluatedStack, String> {
        let graph = SceneGraph::from_project(&self.project, &self.active_comp_id)
            .map_err(|e| format!("Scene graph error: {e:?}"))?;
        Ok(self.evaluator.evaluate(&graph, tc))
    }

    /// Decode an image asset for tracing (native resolution; the tracer
    /// downsamples internally and scales contours back up).
    fn decoded_image_native(&self, asset_id: &str) -> Option<std::sync::Arc<image::RgbaImage>> {
        let asset = self.project.get_asset(asset_id)?;
        Some(std::sync::Arc::new(image::open(&asset.path).ok()?.to_rgba8()))
    }

    /// Build the 0..1 trace field for an evaluated layer's local content.
    fn trace_field_for(
        &self,
        layer: &compositor::EvaluatedLayer,
        comp_w: f32,
        comp_h: f32,
        channel: project::TraceChannel,
    ) -> Option<(Vec<f32>, u32, u32, Vec2)> {
        use crate::raster::layer::{layer_base_dims, path_frame, raster_layer_content};
        let mut assets = std::collections::HashMap::new();
        if let LayerSource::Image { asset_id } = &layer.source {
            assets.insert(asset_id.clone(), self.decoded_image_native(asset_id)?);
        }
        let (base_w, base_h) = layer_base_dims(layer, comp_w, comp_h, &assets);
        let content = raster_layer_content(layer, base_w, base_h, &assets)?;
        // Content origin in layer-local coords (path shapes are framed).
        let (ox, oy) = match &layer.source {
            LayerSource::Shape { shape_type: ShapeType::Path { path_data, .. } } => {
                let (o, _, _) = path_frame(path_data);
                (o.x, o.y)
            }
            _ => (0.0, 0.0),
        };
        let mut field = Vec::with_capacity((content.w * content.h) as usize);
        for p in &content.px {
            let a = p.a.max(1e-6);
            field.push(match channel {
                project::TraceChannel::Alpha => p.a,
                project::TraceChannel::Luminance => {
                    ((0.299 * p.r + 0.587 * p.g + 0.114 * p.b) / a).clamp(0.0, 1.0)
                }
                project::TraceChannel::Red => (p.r / a).clamp(0.0, 1.0),
                project::TraceChannel::Green => (p.g / a).clamp(0.0, 1.0),
                project::TraceChannel::Blue => (p.b / a).clamp(0.0, 1.0),
            });
        }
        Some((field, content.w, content.h, Vec2::new(ox, oy)))
    }

    /// Downsample a field past the pixel budget, returning the field, its
    /// size, and the upsample factor for traced points.
    fn fit_trace_budget(field: Vec<f32>, w: u32, h: u32) -> (Vec<f32>, u32, u32, f32) {
        const BUDGET: u64 = 1_500_000;
        let (mut field, mut w, mut h, mut up) = (field, w, h, 1.0f32);
        while (w as u64) * (h as u64) > BUDGET && w > 32 && h > 32 {
            let (nw, nh) = (w / 2, h / 2);
            let mut small = vec![0.0f32; (nw * nh) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    let mut sum = 0.0f32;
                    for oy in 0..2 {
                        for ox in 0..2 {
                            sum += field[((y * 2 + oy) * w + x * 2 + ox) as usize];
                        }
                    }
                    small[(y * nw + x) as usize] = sum * 0.25;
                }
            }
            field = small;
            (w, h) = (nw, nh);
            up *= 2.0;
        }
        (field, w, h, up)
    }

    /// Trace one frame's content into mask-local paths (origin applied,
    /// budget-scaled back up).
    fn trace_frame_paths(
        &self,
        layer: &compositor::EvaluatedLayer,
        comp_w: f32,
        comp_h: f32,
        opts: &AutoTraceOptions,
    ) -> Option<Vec<(Path, bool)>> {
        let (field, w, h, origin) = self.trace_field_for(layer, comp_w, comp_h, opts.channel)?;
        let (field, w, h, up) = Self::fit_trace_budget(field, w, h);
        let contours = project::trace::trace_field(w, h, &field, opts);
        let mut out = Vec::with_capacity(contours.len());
        for c in contours {
            let mut path = c.to_path();
            for pt in path.points.iter_mut() {
                pt.pos = (pt.pos * up) + origin;
            }
            out.push((path, c.is_hole));
        }
        Some(out)
    }

    /// Auto-trace a layer's channel into masks (Layer > Auto-trace).
    /// Returns the created mask ids (islands as Add, holes as Subtract).
    pub fn auto_trace_masks(
        &mut self,
        layer_id: &str,
        opts: &AutoTraceOptions,
    ) -> Result<Vec<String>, String> {
        let frame_rate: f64;
        let comp_w: f32;
        let comp_h: f32;
        let frames: Vec<TimeCode> = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            frame_rate = comp.frame_rate;
            comp_w = comp.width as f32;
            comp_h = comp.height as f32;
            match opts.range {
                TraceRange::CurrentFrame => vec![self.clock.timecode()],
                TraceRange::WorkArea => {
                    let (a, b) = (
                        self.clock.work_area_in().frames(),
                        self.clock.work_area_out().frames(),
                    );
                    let (lo, hi) = (a.min(b), a.max(b));
                    // Cap: a full shot at 30fps would bury the project.
                    const MAX_TRACE_FRAMES: i64 = 120;
                    (lo..=hi.min(lo + MAX_TRACE_FRAMES - 1))
                        .map(|f| TimeCode::from_frames(f, frame_rate))
                        .collect()
                }
            }
        };
        if frames.is_empty() {
            return Err("Empty trace range".to_string());
        }
        // Trace every frame first (read-only).
        let mut per_frame: Vec<Vec<(Path, bool)>> = Vec::with_capacity(frames.len());
        for tc in &frames {
            let stack = self.evaluate_at(tc)?;
            let layer = stack
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?
                .clone();
            let paths = self
                .trace_frame_paths(&layer, comp_w, comp_h, opts)
                .unwrap_or_default();
            per_frame.push(paths);
        }
        if per_frame.iter().all(|p| p.is_empty()) {
            return Err("Auto-trace found no edges (try a lower Threshold)".to_string());
        }
        // Target layer: same layer, or a fresh solid sized to the content.
        let target_id = if opts.apply_to_new_layer {
            let (src_transform, (w, h)) = {
                let stack = self.evaluate_at(&frames[0])?;
                let layer = stack
                    .get_layer(layer_id)
                    .ok_or_else(|| format!("Layer {layer_id} not found"))?
                    .clone();
                let mut assets = std::collections::HashMap::new();
                if let LayerSource::Image { asset_id } = &layer.source {
                    if let Some(img) = self.decoded_image_native(asset_id) {
                        assets.insert(asset_id.clone(), img);
                    }
                }
                let (bw, bh) = crate::raster::layer::layer_base_dims(&layer, comp_w, comp_h, &assets);
                let src_transform = self
                    .active_composition()
                    .and_then(|c| c.get_layer(layer_id))
                    .map(|l| l.transform.clone())
                    .unwrap_or_default();
                (src_transform, (bw.ceil().max(8.0), bh.ceil().max(8.0)))
            };
            self.checkpoint();
            let comp = self
                .active_composition_mut()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_traced_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_traced_{counter}");
            }
            let tc0 = TimeCode::zero(frame_rate);
            let dur = comp.duration.clone();
            let mut solid = Layer::solid(&id, "Traced Masks", Color::WHITE, w as u32, h as u32, tc0, dur);
            // Match the source transform so traced coords line up.
            solid.transform = src_transform;
            comp.insert_layer(0, solid)
                .map_err(|e| format!("Cannot add layer: {e:?}"))?;
            id
        } else {
            layer_id.to_string()
        };
        // Create one mask per first-frame contour; keyframe the rest when
        // the topology matches (count + hole flags), else hold.
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&target_id)
            .ok_or_else(|| format!("Layer {target_id} not found"))?;
        let first = &per_frame[0];
        let mut ids = Vec::with_capacity(first.len());
        for (i, (path, is_hole)) in first.iter().enumerate() {
            let id = next_mask_id(layer);
            let name = format!("Trace {}", i + 1);
            let mut mask = Mask::with_path(&id, name, path.clone());
            if *is_hole {
                mask.mode = project::MaskMode::Subtract;
            }
            if frames.len() > 1 {
                mask.path.set_animated(true);
                for (tc, frame_paths) in frames.iter().zip(per_frame.iter()) {
                    let same_shape = frame_paths.len() == first.len()
                        && frame_paths.iter().zip(first.iter()).all(|((_, h1), (_, h2))| h1 == h2);
                    // Index-matched keyframes while the topology holds;
                    // frames with different topology simply hold.
                    if same_shape {
                        if let Some((fp, _)) = frame_paths.get(i) {
                            mask.path.add_keyframe(Keyframe::new(tc.clone(), fp.clone()));
                        }
                    }
                }
                // Guarantee the playhead frame holds.
                if !mask.path.has_keyframe_at(&frames[0]) {
                    mask.path.add_keyframe(Keyframe::new(frames[0].clone(), path.clone()));
                }
            } else {
                mask.path.set_animated(true);
                mask.path.add_keyframe(Keyframe::new(frames[0].clone(), path.clone()));
            }
            ids.push(id.clone());
            layer.masks.push(mask);
        }
        Ok(ids)
    }

    /// Create masks from a text layer's characters (Layer > Create Masks
    /// From Text): the text is rasterized, auto-traced, and the outlines
    /// land as masks on a new solid (compound characters get Subtract
    /// holes). The text layer itself is hidden, like AE's Video switch.
    /// Returns `(solid_id, mask_ids)`.
    pub fn create_masks_from_text(
        &mut self,
        layer_id: &str,
    ) -> Result<(String, Vec<String>), String> {
        let current_tc = self.clock.timecode();
        let frame_rate: f64;
        let comp_w: f32;
        let comp_h: f32;
        let (src_transform, src_name, base_w, base_h, traced): (
            project::Transform,
            String,
            f32,
            f32,
            Vec<(Path, bool)>,
        ) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            frame_rate = comp.frame_rate;
            comp_w = comp.width as f32;
            comp_h = comp.height as f32;
            let src = comp
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?;
            if !matches!(&src.source, LayerSource::Text { .. }) {
                return Err(format!("Layer {layer_id} is not a text layer"));
            }
            let stack = self.evaluate_at(&current_tc)?;
            let elayer = stack
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?
                .clone();
            let assets = std::collections::HashMap::new();
            let (bw, bh) =
                crate::raster::layer::layer_base_dims(&elayer, comp_w, comp_h, &assets);
            // Text needs a lower threshold to keep soft glyph edges.
            let mut opts = AutoTraceOptions::default();
            opts.threshold_pct = 20.0;
            opts.min_area_px = 4.0;
            opts.tolerance_px = 0.75;
            let traced = self
                .trace_frame_paths(&elayer, comp_w, comp_h, &opts)
                .unwrap_or_default();
            (src.transform.clone(), src.name.clone(), bw, bh, traced)
        };
        if traced.is_empty() {
            return Err("Text has no traceable outlines".to_string());
        }
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        // Hide the source text (AE turns its Video switch off).
        if let Some(src) = comp.get_layer_mut(layer_id) {
            src.visible = false;
        }
        let mut counter = comp.layers.len() + 1;
        let mut solid_id = format!("layer_text_masks_{counter}");
        while comp.get_layer(&solid_id).is_some() {
            counter += 1;
            solid_id = format!("layer_text_masks_{counter}");
        }
        let (w, h) = (base_w.ceil().max(8.0) as u32, base_h.ceil().max(8.0) as u32);
        let mut solid = Layer::solid(
            &solid_id,
            format!("{src_name} Masks"),
            Color::WHITE,
            w,
            h,
            TimeCode::zero(frame_rate),
            comp.duration.clone(),
        );
        solid.transform = src_transform;
        comp.insert_layer(0, solid)
            .map_err(|e| format!("Cannot add layer: {e:?}"))?;
        let layer = comp
            .get_layer_mut(&solid_id)
            .ok_or_else(|| "Layer vanished".to_string())?;
        let mut ids = Vec::with_capacity(traced.len());
        for (i, (path, is_hole)) in traced.iter().enumerate() {
            let id = next_mask_id(layer);
            let mut mask = Mask::with_path(&id, format!("Char {}", i + 1), path.clone());
            if *is_hole {
                mask.mode = project::MaskMode::Subtract;
            }
            mask.path.set_animated(true);
            mask.path.add_keyframe(Keyframe::new(current_tc.clone(), path.clone()));
            ids.push(id.clone());
            layer.masks.push(mask);
        }
        self.selected_layer_id = Some(solid_id.clone());
        Ok((solid_id, ids))
    }

    /// Clear a text layer's baseline path (straight layout resumes).
    pub fn clear_text_path(&mut self, layer_id: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { text_path, .. } => {
                *text_path = None;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Append a point to a text layer's baseline path (pen integration).
    pub fn append_text_path_point(&mut self, layer_id: &str, pos: Vec2) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { text_path, .. } => {
                let path = text_path.get_or_insert_with(Path::new);
                path.line_to(pos);
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Nudge an effect parameter on a specified layer.
    pub fn nudge_layer_effect_param(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        param_name: &str,
        delta: f32,
    ) -> Result<(), String> {
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;

        if let Some(prop) = effect.get_param_property_mut(param_name) {
            let current = if prop.is_animated() {
                prop.evaluate_at(&current_tc)
            } else {
                prop.value
            };
            let new_val = (current + delta).max(0.0);
            prop.set_value(new_val);
            if prop.is_animated() {
                prop.add_keyframe(Keyframe::new(current_tc, new_val));
            }
            Ok(())
        } else if effect.nudge_param(param_name, delta) {
            Ok(())
        } else {
            Err(format!(
                "Parameter {param_name} not found on effect {effect_id}"
            ))
        }
    }

    /// Nudge an effect parameter on the currently selected layer.
    pub fn nudge_effect_param(
        &mut self,
        effect_id: &str,
        param_name: &str,
        delta: f32,
    ) -> Result<(), String> {
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.nudge_layer_effect_param(&selected_id, effect_id, param_name, delta)
    }

    /// Set custom GLSL shader code on the selected layer.
    pub fn set_glsl_code(
        &mut self,
        effect_id: &str,
        code: String,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if let project::EffectType::GlslShader { code: c, .. } = &mut effect.effect_type {
            *c = code;
            Ok(())
        } else {
            Err(format!("Effect {effect_id} is not a GlslShader"))
        }
    }

    /// Set chroma key color on the selected layer.
    pub fn set_chroma_key_color(
        &mut self,
        effect_id: &str,
        key_color: Color,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if let project::EffectType::ChromaKey { key_color: kc, .. } = &mut effect.effect_type {
            *kc = key_color;
            Ok(())
        } else {
            Err(format!("Effect {effect_id} is not ChromaKey"))
        }
    }

    /// Set tint colors on the selected layer's tint effect.
    pub fn set_tint_colors(
        &mut self,
        effect_id: &str,
        map_black: Option<Color>,
        map_white: Option<Color>,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if let project::EffectType::Tint { map_black: mb, map_white: mw, .. } = &mut effect.effect_type {
            if let Some(c) = map_black {
                *mb = c;
            }
            if let Some(c) = map_white {
                *mw = c;
            }
            Ok(())
        } else {
            Err(format!("Effect {effect_id} is not Tint"))
        }
    }

    /// Set drop shadow color on the selected layer's drop shadow effect.
    pub fn set_drop_shadow_color(
        &mut self,
        effect_id: &str,
        color: Color,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if let project::EffectType::DropShadow { color: c, .. } = &mut effect.effect_type {
            *c = color;
            Ok(())
        } else {
            Err(format!("Effect {effect_id} is not DropShadow"))
        }
    }

    /// Toggle noise monochrome on the selected layer.
    pub fn toggle_noise_monochrome(
        &mut self,
        effect_id: &str,
    ) -> Result<(), String> {
        self.checkpoint();
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if let project::EffectType::NoiseGenerator { monochrome, .. } = &mut effect.effect_type {
            *monochrome = !*monochrome;
            Ok(())
        } else {
            Err(format!("Effect {effect_id} is not NoiseGenerator"))
        }
    }

    /// Nudge position on the specified layer.
    pub fn nudge_layer_position(&mut self, layer_id: &str, dx: f32, dy: f32) {
        let delta = Vec2::new(dx, dy);
        let current_tc = self.clock.timecode();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        if let Some(layer) = comp.get_layer_mut(layer_id) {
            let current = if layer.transform.position.is_animated() {
                layer.transform.position.evaluate_at(&current_tc)
            } else {
                layer.transform.position.value
            };
            let new_val = current + delta;
            layer.transform.position.set_value(new_val);
            if layer.transform.position.is_animated() {
                layer.transform.position.add_keyframe(Keyframe::new(current_tc, new_val));
            }
        }
    }

    /// Nudge position of the selected layer by `(dx, dy)`.
    pub fn nudge_position(&mut self, dx: f32, dy: f32) {
        if let Some(id) = self.selected_layer_id.clone() {
            self.nudge_layer_position(&id, dx, dy);
        }
    }

    /// Nudge anchor point on the specified layer.
    pub fn nudge_layer_anchor(&mut self, layer_id: &str, dx: f32, dy: f32) {
        let delta = Vec2::new(dx, dy);
        let current_tc = self.clock.timecode();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        if let Some(layer) = comp.get_layer_mut(layer_id) {
            let current = if layer.transform.anchor_point.is_animated() {
                layer.transform.anchor_point.evaluate_at(&current_tc)
            } else {
                layer.transform.anchor_point.value
            };
            let new_val = current + delta;
            layer.transform.anchor_point.set_value(new_val);
            if layer.transform.anchor_point.is_animated() {
                layer.transform.anchor_point.add_keyframe(Keyframe::new(current_tc, new_val));
            }
        }
    }

    /// Nudge anchor point of the selected layer by `(dx, dy)`.
    pub fn nudge_anchor(&mut self, dx: f32, dy: f32) {
        if let Some(id) = self.selected_layer_id.clone() {
            self.nudge_layer_anchor(&id, dx, dy);
        }
    }

    /// Nudge scale on the specified layer.
    pub fn nudge_layer_scale(&mut self, layer_id: &str, dx: f32, dy: f32) {
        let current_tc = self.clock.timecode();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        if let Some(layer) = comp.get_layer_mut(layer_id) {
            let current = if layer.transform.scale.is_animated() {
                layer.transform.scale.evaluate_at(&current_tc)
            } else {
                layer.transform.scale.value
            };
            // Uniform mode (default): both axes move together as one value.
            let delta = if layer.transform.scale_uniform {
                Vec2::new(dx + dy, dx + dy)
            } else {
                Vec2::new(dx, dy)
            };
            let new_val = current + delta;
            layer.transform.scale.set_value(new_val);
            if layer.transform.scale.is_animated() {
                layer.transform.scale.add_keyframe(Keyframe::new(current_tc, new_val));
            }
        }
    }

    /// Toggle uniform vs. separate-dimensions scale editing on a layer.
    pub fn toggle_layer_scale_link(&mut self, layer_id: &str) {
        let current_tc = self.clock.timecode();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                let next = !layer.transform.scale_uniform;
                layer.transform.set_scale_uniform(next);
                if layer.transform.scale.is_animated() {
                    let v = layer.transform.scale.evaluate_at(&current_tc);
                    let snapped = if next { Vec2::new(v.x, v.x) } else { v };
                    layer.transform.scale.set_value(snapped);
                    layer.transform.scale.add_keyframe(Keyframe::new(current_tc, snapped));
                }
            }
        }
    }

    /// Toggle uniform scale editing on the selected layer.
    pub fn toggle_selected_scale_link(&mut self) {
        if let Some(id) = self.selected_layer_id.clone() {
            self.toggle_layer_scale_link(&id);
        }
    }

    /// Read the current raw numeric value behind a scrub key (for keyboard
    /// entry prefill). Mirrors [`Self::set_scrub_value`]; `None` for unknown
    /// keys or missing selection.
    pub fn scrub_current_value(&self, key: &str) -> Option<f32> {
        let current_tc = self.clock.timecode();
        let comp = self.active_composition()?;
        let lid = self.selected_layer_id.as_deref()?;
        let layer = comp.get_layer(lid)?;
        let eval_or = |p: &Property<f32>| {
            if p.is_animated() {
                p.evaluate_at(&current_tc)
            } else {
                p.value
            }
        };
        let eval_or_vec = |p: &Property<Vec2>| {
            if p.is_animated() {
                p.evaluate_at(&current_tc)
            } else {
                p.value
            }
        };
        match key {
            "pos_x" => Some(eval_or_vec(&layer.transform.position).x),
            "pos_y" => Some(eval_or_vec(&layer.transform.position).y),
            "anchor_x" => Some(eval_or_vec(&layer.transform.anchor_point).x),
            "anchor_y" => Some(eval_or_vec(&layer.transform.anchor_point).y),
            "scale_x" | "scale_u" => Some(eval_or_vec(&layer.transform.scale).x),
            "scale_y" => Some(eval_or_vec(&layer.transform.scale).y),
            "rotation" => Some(eval_or(&layer.transform.rotation)),
            "opacity" => Some(eval_or(&layer.opacity)),
            "font_size" => match &layer.source {
                LayerSource::Text { font_size, .. } => Some(eval_or(font_size)),
                _ => None,
            },
            "font_weight" | "text_weight" => match &layer.source {
                LayerSource::Text { weight, .. } => Some(*weight as f32),
                _ => None,
            },
            _ if key.split(':').next().unwrap_or(key).starts_with("text_") => match &layer.source {
                LayerSource::Text {
                    tracking,
                    leading,
                    stroke_width,
                    baseline_shift,
                    box_width,
                    box_height,
                    ..
                } => {
                    let prop = match key.split(':').next().unwrap_or(key) {
                        "text_tracking" => tracking,
                        "text_leading" => leading,
                        "text_stroke_w" => stroke_width,
                        "text_baseline" => baseline_shift,
                        "text_box_h" => box_height,
                        _ => box_width,
                    };
                    Some(eval_or(prop))
                }
                _ => None,
            },
            "solid_w" => match &layer.source {
                LayerSource::Solid { width, .. } => Some(*width as f32),
                _ => None,
            },
            "solid_h" => match &layer.source {
                LayerSource::Solid { height, .. } => Some(*height as f32),
                _ => None,
            },
            "rect_w" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Rectangle { width, .. } } => {
                    Some(width.value)
                }
                _ => None,
            },
            "rect_h" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Rectangle { height, .. } } => {
                    Some(height.value)
                }
                _ => None,
            },
            "rect_cr" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Rectangle { corner_radius, .. } } => {
                    Some(corner_radius.value)
                }
                _ => None,
            },
            "ellipse_rx" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_x, .. } } => {
                    Some(radius_x.value)
                }
                _ => None,
            },
            "ellipse_ry" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_y, .. } } => {
                    Some(radius_y.value)
                }
                _ => None,
            },
            _ => {
                // Shader Lab scalar (sl:) and component (slc:) keys.
                let (rest, is_component) = match (key.strip_prefix("slc:"), key.strip_prefix("sl:")) {
                    (Some(r), _) => (r, true),
                    (_, Some(r)) => (r, false),
                    _ => {
                        let rest = key.strip_prefix("fx:")?;
                        let parts: Vec<&str> = rest.split(':').collect();
                        if parts.len() < 2 {
                            return None;
                        }
                        let prop = layer.get_effect(parts[0])?.get_param_property(parts[1])?;
                        return Some(if prop.is_animated() {
                            prop.evaluate_at(&current_tc)
                        } else {
                            prop.value
                        });
                    }
                };
                let parts: Vec<&str> = rest.split(':').collect();
                if parts.len() < 2 {
                    return None;
                }
                let effect = layer.get_effect(parts[0])?;
                let resolved = effect.resolved_shader_values();
                let value = resolved.iter().find(|(n, _)| n == parts[1])?.1.clone();
                let idx = if is_component {
                    parts.get(2)?.parse::<usize>().ok()?
                } else {
                    0
                };
                Some(match value {
                    project::ShaderParamValue::Float(v) => v,
                    project::ShaderParamValue::Int(v) => v as f32,
                    project::ShaderParamValue::Bool(v) => if v { 1.0 } else { 0.0 },
                    project::ShaderParamValue::Vec2(a) => a.get(idx).copied().unwrap_or(0.0),
                    project::ShaderParamValue::Vec3(a) => a.get(idx).copied().unwrap_or(0.0),
                    project::ShaderParamValue::Vec4(a) => a.get(idx).copied().unwrap_or(0.0),
                    project::ShaderParamValue::Color(c) => [c.r, c.g, c.b, c.a].get(idx).copied().unwrap_or(0.0),
                })
            }
        }
    }

    /// Set a scrubbed property to an absolute value (keyboard entry).
    /// Returns false for unknown keys. Mirrors the drag/wheel key universe
    /// (`anchor_x`, `pos_*`, `scale_*`, `rotation`, `opacity`, `solid_*`,
    /// `font_size`, `rect_*`, `ellipse_*`, `fx:<effect>:<param>`).
    pub fn set_scrub_value(&mut self, key: &str, v: f32) -> bool {
        if !v.is_finite() {
            return false;
        }
        let current_tc = self.clock.timecode();
        match key {
            "pos_x" | "pos_y" | "anchor_x" | "anchor_y" => {
                let (cx0, cy0) = {
                    let comp = match self.active_composition() {
                        Some(c) => c,
                        None => return false,
                    };
                    let layer = match comp.get_layer(self.selected_layer_id.as_deref().unwrap_or("")) {
                        Some(l) => l,
                        None => return false,
                    };
                    let p = if key.starts_with("pos_") {
                        if layer.transform.position.is_animated() {
                            layer.transform.position.evaluate_at(&current_tc)
                        } else {
                            layer.transform.position.value
                        }
                    } else if layer.transform.anchor_point.is_animated() {
                        layer.transform.anchor_point.evaluate_at(&current_tc)
                    } else {
                        layer.transform.anchor_point.value
                    };
                    (p.x, p.y)
                };
                match key {
                    "pos_x" => self.nudge_position(v - cx0, 0.0),
                    "pos_y" => self.nudge_position(0.0, v - cy0),
                    "anchor_x" => self.nudge_anchor(v - cx0, 0.0),
                    _ => self.nudge_anchor(0.0, v - cy0),
                }
                true
            }
            "scale_x" | "scale_y" | "scale_u" => {
                let (linked, cx0, cy0) = {
                    let comp = match self.active_composition() {
                        Some(c) => c,
                        None => return false,
                    };
                    let layer = match comp.get_layer(self.selected_layer_id.as_deref().unwrap_or("")) {
                        Some(l) => l,
                        None => return false,
                    };
                    let s = if layer.transform.scale.is_animated() {
                        layer.transform.scale.evaluate_at(&current_tc)
                    } else {
                        layer.transform.scale.value
                    };
                    (layer.transform.scale_uniform, s.x, s.y)
                };
                if linked || key == "scale_u" {
                    // Uniform: a single value drives both axes.
                    self.nudge_scale(v - cx0, 0.0);
                } else if key == "scale_x" {
                    self.nudge_scale(v - cx0, 0.0);
                } else {
                    self.nudge_scale(0.0, v - cy0);
                }
                true
            }
            "rotation" => {
                let cur = {
                    let comp = match self.active_composition() {
                        Some(c) => c,
                        None => return false,
                    };
                    let layer = match comp.get_layer(self.selected_layer_id.as_deref().unwrap_or("")) {
                        Some(l) => l,
                        None => return false,
                    };
                    if layer.transform.rotation.is_animated() {
                        layer.transform.rotation.evaluate_at(&current_tc)
                    } else {
                        layer.transform.rotation.value
                    }
                };
                self.nudge_rotation(v - cur);
                true
            }
            "opacity" => {
                let cur = {
                    let comp = match self.active_composition() {
                        Some(c) => c,
                        None => return false,
                    };
                    let layer = match comp.get_layer(self.selected_layer_id.as_deref().unwrap_or("")) {
                        Some(l) => l,
                        None => return false,
                    };
                    if layer.opacity.is_animated() {
                        layer.opacity.evaluate_at(&current_tc)
                    } else {
                        layer.opacity.value
                    }
                };
                self.nudge_opacity(v - cur);
                true
            }
            "font_size" => {
                if let Some(id) = self.selected_layer_id.clone() {
                    self.set_layer_font_size(&id, v).is_ok()
                } else {
                    false
                }
            }
            "solid_w" | "solid_h" => {
                let (id, w, h) = {
                    let comp = match self.active_composition() {
                        Some(c) => c,
                        None => return false,
                    };
                    let lid = match self.selected_layer_id.clone() {
                        Some(l) => l,
                        None => return false,
                    };
                    let layer = match comp.get_layer(&lid) {
                        Some(l) => l,
                        None => return false,
                    };
                    match &layer.source {
                        LayerSource::Solid { width, height, .. } => (lid, *width, *height),
                        _ => return false,
                    }
                };
                let (nw, nh) = if key == "solid_w" {
                    (v.max(1.0) as u32, h)
                } else {
                    (w, v.max(1.0) as u32)
                };
                self.set_layer_solid_dimensions(&id, nw, nh).is_ok()
            }
            "rect_w" | "rect_h" | "rect_cr" => {
                if let Some(id) = self.selected_layer_id.clone() {
                    let (w, h, cr) = {
                        let comp = match self.active_composition() {
                            Some(c) => c,
                            None => return false,
                        };
                        let layer = match comp.get_layer(&id) {
                            Some(l) => l,
                            None => return false,
                        };
                        match &layer.source {
                            LayerSource::Shape {
                                shape_type: ShapeType::Rectangle { width, height, corner_radius, .. },
                            } => (width.value, height.value, corner_radius.value),
                            _ => return false,
                        }
                    };
                    match key {
                        "rect_w" => self.set_layer_rect_dimensions(&id, v.max(1.0), h, cr).is_ok(),
                        "rect_h" => self.set_layer_rect_dimensions(&id, w, v.max(1.0), cr).is_ok(),
                        _ => self.set_layer_rect_dimensions(&id, w, h, v.max(0.0)).is_ok(),
                    }
                } else {
                    false
                }
            }
            "ellipse_rx" | "ellipse_ry" => {
                if let Some(id) = self.selected_layer_id.clone() {
                    let (rx, ry) = {
                        let comp = match self.active_composition() {
                            Some(c) => c,
                            None => return false,
                        };
                        let layer = match comp.get_layer(&id) {
                            Some(l) => l,
                            None => return false,
                        };
                        match &layer.source {
                            LayerSource::Shape {
                                shape_type: ShapeType::Ellipse { radius_x, radius_y, .. },
                            } => (radius_x.value, radius_y.value),
                            _ => return false,
                        }
                    };
                    let (nrx, nry) = if key == "ellipse_rx" {
                        (v.max(1.0), ry)
                    } else {
                        (rx, v.max(1.0))
                    };
                    self.set_layer_ellipse_radii(&id, nrx, nry).is_ok()
                } else {
                    false
                }
            }
            _ => {
                if let Some(rest) = key.strip_prefix("slcl:") {
                    // slcl:<effect>:<param> (linked typed entry: one value
                    // for every component).
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 2 {
                        return self
                            .set_shaderlab_all_components(parts[0], parts[1], v)
                            .is_ok();
                    }
                    return false;
                }
                if let Some(rest) = key.strip_prefix("slc:") {
                    // slc:<effect>:<param>:<index>
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 3 {
                        if let Ok(idx) = parts[2].parse::<usize>() {
                            return self
                                .set_shaderlab_component(parts[0], parts[1], idx, v)
                                .is_ok();
                        }
                    }
                    return false;
                }
                if let Some(rest) = key.strip_prefix("sl:") {
                    // sl:<effect>:<param>
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 2 {
                        return self.set_shaderlab_param(parts[0], parts[1], v).is_ok();
                    }
                    return false;
                }
                if let Some(rest) = key.strip_prefix("fx:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 2 {
                        let (lid, cur) = {
                            let comp = match self.active_composition() {
                                Some(c) => c,
                                None => return false,
                            };
                            let lid = match self.selected_layer_id.clone() {
                                Some(l) => l,
                                None => return false,
                            };
                            let layer = match comp.get_layer(&lid) {
                                Some(l) => l,
                                None => return false,
                            };
                            let fx = match layer.get_effect(parts[0]) {
                                Some(f) => f,
                                None => return false,
                            };
                            let prop = match fx.get_param_property(parts[1]) {
                                Some(p) => p,
                                None => return false,
                            };
                            let cur = if prop.is_animated() {
                                prop.evaluate_at(&current_tc)
                            } else {
                                prop.value
                            };
                            (lid, cur)
                        };
                        return self
                            .nudge_layer_effect_param(&lid, parts[0], parts[1], v - cur)
                            .is_ok();
                    }
                }
                if let Some(rest) = key.strip_prefix("mask:") {
                    // mask:<layer>:<mask>:<param> (absolute set via delta).
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 3 {
                        let (lid, mid, param) = (parts[0], parts[1], parts[2]);
                        let cur = {
                            let comp = match self.active_composition() {
                                Some(c) => c,
                                None => return false,
                            };
                            let layer = match comp.get_layer(lid) {
                                Some(l) => l,
                                None => return false,
                            };
                            let mask = match layer.get_mask(mid) {
                                Some(m) => m,
                                None => return false,
                            };
                            let prop = match mask.get_param_property(param) {
                                Some(p) => p,
                                None => return false,
                            };
                            if prop.is_animated() {
                                prop.evaluate_at(&current_tc)
                            } else {
                                prop.value
                            }
                        };
                        return self.nudge_mask_param(lid, mid, param, v - cur).is_ok();
                    }
                }
                // Text scalar scrub keys (`text_tracking`, `text_leading`,
                // `text_stroke_w`, `text_baseline`, `text_box_w`); the key
                // carries an optional `:<mult100>` suffix.
                let text_field = match key.split(':').next().unwrap_or(key) {
                    "text_tracking" => Some("tracking"),
                    "text_leading" => Some("leading"),
                    "text_stroke_w" => Some("stroke_width"),
                    "text_baseline" => Some("baseline_shift"),
                    "text_box_w" => Some("box_width"),
                    "text_box_h" => Some("box_height"),
                    _ => None,
                };
                if let Some(field) = text_field {
                    let lid = match self.selected_layer_id.clone() {
                        Some(l) => l,
                        None => return false,
                    };
                    return self.set_layer_text_scalar(&lid, field, v).is_ok();
                }
                if key == "font_weight" || key == "text_weight" {
                    let lid = match self.selected_layer_id.clone() {
                        Some(l) => l,
                        None => return false,
                    };
                    return self.set_layer_font_weight(&lid, v.clamp(100.0, 900.0).round() as u16).is_ok();
                }
                false
            }
        }
    }

    /// Nudge scale of the selected layer by `(dx, dy)` percent.
    pub fn nudge_scale(&mut self, dx: f32, dy: f32) {
        if let Some(id) = self.selected_layer_id.clone() {
            self.nudge_layer_scale(&id, dx, dy);
        }
    }

    /// Set absolute rotation on a layer (viewport gizmo). Keyframe-aware.
    pub fn set_layer_rotation(&mut self, layer_id: &str, deg: f32) {
        if !deg.is_finite() {
            return;
        }
        let current_tc = self.clock.timecode();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.transform.rotation.set_value(deg);
                if layer.transform.rotation.is_animated() {
                    layer.transform.rotation.add_keyframe(Keyframe::new(current_tc, deg));
                }
            }
        }
    }

    /// Set absolute scale on a layer in percent (viewport gizmo).
    /// Keyframe-aware; bypasses the uniform link (the gizmo drives axes).
    pub fn set_layer_scale(&mut self, layer_id: &str, x: f32, y: f32) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let current_tc = self.clock.timecode();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                let v = Vec2::new(x.max(1.0), y.max(1.0));
                layer.transform.scale.set_value(v);
                if layer.transform.scale.is_animated() {
                    layer.transform.scale.add_keyframe(Keyframe::new(current_tc, v));
                }
            }
        }
    }

    /// Set absolute position on a layer in composition px (viewport gizmo).
    pub fn set_layer_position(&mut self, layer_id: &str, pos: Vec2) {
        if !pos.x.is_finite() || !pos.y.is_finite() {
            return;
        }
        let current_tc = self.clock.timecode();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.transform.position.set_value(pos);
                if layer.transform.position.is_animated() {
                    layer.transform.position.add_keyframe(Keyframe::new(current_tc, pos));
                }
            }
        }
    }

    /// Set absolute anchor on a layer in layer-local px (viewport gizmo).
    pub fn set_layer_anchor(&mut self, layer_id: &str, anchor: Vec2) {
        if !anchor.x.is_finite() || !anchor.y.is_finite() {
            return;
        }
        let current_tc = self.clock.timecode();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.transform.anchor_point.set_value(anchor);
                if layer.transform.anchor_point.is_animated() {
                    layer.transform.anchor_point.add_keyframe(Keyframe::new(current_tc, anchor));
                }
            }
        }
    }

    /// Pan-Behind-style anchor move: shifts the pivot by a layer-local delta
    /// while counter-moving position so rendered pixels stay put.
    pub fn move_layer_anchor(&mut self, layer_id: &str, d_local: Vec2) {
        if !d_local.x.is_finite() || !d_local.y.is_finite() {
            return;
        }
        // World-space shift of the pivot under the current rotation/scale.
        let (anchor, world_shift) = {
            let comp = match self.active_composition() {
                Some(c) => c,
                None => return,
            };
            let layer = match comp.get_layer(layer_id) {
                Some(l) => l,
                None => return,
            };
            let current_tc = self.clock.timecode();
            let rot = layer.transform.rotation.evaluate_at(&current_tc).to_radians();
            let sc = layer.transform.scale.evaluate_at(&current_tc);
            let (sx, sy) = (sc.x / 100.0, sc.y / 100.0);
            let (cos, sin) = (rot.cos(), rot.sin());
            // R * S * d (layer-local delta into world px).
            let wx = (d_local.x * sx) * cos - (d_local.y * sy) * sin;
            let wy = (d_local.x * sx) * sin + (d_local.y * sy) * cos;
            let anchor = if layer.transform.anchor_point.is_animated() {
                layer.transform.anchor_point.evaluate_at(&current_tc)
            } else {
                layer.transform.anchor_point.value
            };
            let pos = if layer.transform.position.is_animated() {
                layer.transform.position.evaluate_at(&current_tc)
            } else {
                layer.transform.position.value
            };
            (anchor, (pos, Vec2::new(wx, wy)))
        };
        let (pos, shift) = world_shift;
        self.set_layer_anchor(layer_id, anchor + d_local);
        self.set_layer_position(layer_id, pos + shift);
    }

    /// Reset a layer pivot to its content center, keeping pixels in place.
    /// Every new layer already spawns centered; this repairs drifted pivots.
    pub fn reset_layer_anchor_center(&mut self, layer_id: &str) {
        self.checkpoint();
        let (bw, bh) = match self.content_size(layer_id) {
            Some(s) => s,
            None => return,
        };
        let current = {
            let comp = match self.active_composition() {
                Some(c) => c,
                None => return,
            };
            let layer = match comp.get_layer(layer_id) {
                Some(l) => l,
                None => return,
            };
            let current_tc = self.clock.timecode();
            if layer.transform.anchor_point.is_animated() {
                layer.transform.anchor_point.evaluate_at(&current_tc)
            } else {
                layer.transform.anchor_point.value
            }
        };
        let target = Vec2::new(bw / 2.0, bh / 2.0);
        self.move_layer_anchor(layer_id, target - current);
    }

    /// Untransformed content size (layer-local px) behind a layer, mirroring
    /// the viewer estimate so pivots land on the true content center.
    pub fn content_size(&self, layer_id: &str) -> Option<(f32, f32)> {
        let comp = self.active_composition()?;
        let layer = comp.get_layer(layer_id)?;
        Some(match &layer.source {
            LayerSource::Solid { width, height, .. } => (*width as f32, *height as f32),
            LayerSource::Image { asset_id } => {
                if let Some(asset) = self.project.get_asset(asset_id) {
                    let (w, h) = image::image_dimensions(&asset.path).unwrap_or((1920, 1080));
                    (w as f32, h as f32)
                } else {
                    (400.0, 300.0)
                }
            }
            LayerSource::Video { .. } => (1920.0, 1080.0),
            LayerSource::Text { text, font_size, .. } => {
                let len = text.value.chars().count().max(1) as f32;
                let fs = font_size.value;
                ((len * fs * 0.6 + 40.0).max(100.0), (fs * 1.4 + 20.0).max(40.0))
            }
            LayerSource::Shape { shape_type } => match shape_type {
                ShapeType::Rectangle { width, height, .. } => (width.value, height.value),
                ShapeType::Ellipse { radius_x, radius_y, .. } => {
                    (radius_x.value * 2.0, radius_y.value * 2.0)
                }
                // Path bounds come from the evaluated world box; fall back
                // to the composition center region size here.
                ShapeType::Path { .. } => {
                    let c = self.active_composition()?;
                    (c.width as f32 / 4.0, c.height as f32 / 4.0)
                }
            },
            LayerSource::Adjustment => {
                let c = self.active_composition()?;
                (c.width as f32, c.height as f32)
            }
            _ => (400.0, 300.0),
        })
    }

    /// Set a shape layer fill color.
    pub fn set_layer_shape_fill(&mut self, layer_id: &str, color: Color) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Rectangle { fill, .. },
            }
            | LayerSource::Shape {
                shape_type: ShapeType::Ellipse { fill, .. },
            }
            | LayerSource::Shape {
                shape_type: ShapeType::Path { fill, .. },
            } => {
                *fill = color;
                Ok(())
            }
            _ => Err("Not a shape layer".to_string()),
        }
    }

    /// Set a color field on an effect (`color_a` / `color_b` / `color`).
    /// Used by checker, gradient, and outline swatches.
    pub fn set_effect_color(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        field: &str,
        color: Color,
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if effect.set_color_value(field, color) {
            Ok(())
        } else if effect.set_stock_color(field, color) {
            Ok(())
        } else {
            Err(format!("Color field {field} not found on effect {effect_id}"))
        }
    }

    /// Nudge rotation on the specified layer.
    pub fn nudge_layer_rotation(&mut self, layer_id: &str, ddeg: f32) {
        let current_tc = self.clock.timecode();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        if let Some(layer) = comp.get_layer_mut(layer_id) {
            let current = if layer.transform.rotation.is_animated() {
                layer.transform.rotation.evaluate_at(&current_tc)
            } else {
                layer.transform.rotation.value
            };
            let new_val = current + ddeg;
            layer.transform.rotation.set_value(new_val);
            if layer.transform.rotation.is_animated() {
                layer.transform.rotation.add_keyframe(Keyframe::new(current_tc, new_val));
            }
        }
    }

    /// Nudge rotation of the selected layer by `ddeg` degrees.
    pub fn nudge_rotation(&mut self, ddeg: f32) {
        if let Some(id) = self.selected_layer_id.clone() {
            self.nudge_layer_rotation(&id, ddeg);
        }
    }

    /// Nudge opacity on the specified layer.
    pub fn nudge_layer_opacity(&mut self, layer_id: &str, dop: f32) {
        let current_tc = self.clock.timecode();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        if let Some(layer) = comp.get_layer_mut(layer_id) {
            let current = if layer.opacity.is_animated() {
                layer.opacity.evaluate_at(&current_tc)
            } else {
                layer.opacity.value
            };
            let new_val = (current + dop).clamp(0.0, 100.0);
            layer.opacity.set_value(new_val);
            if layer.opacity.is_animated() {
                layer.opacity.add_keyframe(Keyframe::new(current_tc, new_val));
            }
        }
    }

    /// Nudge opacity of the selected layer by `dop` percent.
    pub fn nudge_opacity(&mut self, dop: f32) {
        if let Some(id) = self.selected_layer_id.clone() {
            self.nudge_layer_opacity(&id, dop);
        }
    }

    /// Nudge a timeline value row for an arbitrary layer (After Effects-style
    /// timeline scrubbing — no +/- buttons). `key` is one of `anchor_x`,
    /// `anchor_y`, `pos_x`, `pos_y`, `scale_x`, `scale_y`, `rotation`,
    /// `opacity`, or `fx:<effect_id>:<param>`.
    pub fn nudge_timeline_value(&mut self, layer_id: &str, key: &str, delta: f32) {
        if !delta.is_finite() || delta == 0.0 {
            return;
        }
        match key {
            "anchor_x" => self.nudge_layer_anchor(layer_id, delta, 0.0),
            "anchor_y" => self.nudge_layer_anchor(layer_id, 0.0, delta),
            "pos_x" => self.nudge_layer_position(layer_id, delta, 0.0),
            "pos_y" => self.nudge_layer_position(layer_id, 0.0, delta),
            "scale_x" => self.nudge_layer_scale(layer_id, delta, 0.0),
            "scale_y" => self.nudge_layer_scale(layer_id, 0.0, delta),
            "rotation" => self.nudge_layer_rotation(layer_id, delta),
            "opacity" => self.nudge_layer_opacity(layer_id, delta),
            _ => {
                if let Some(rest) = key.strip_prefix("fx:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 2 {
                        let _ =
                            self.nudge_layer_effect_param(layer_id, parts[0], parts[1], delta);
                    }
                }
            }
        }
    }

    /// Current value behind a timeline row (prefills keyboard entry).
    /// Accepts the same key universe as [`Self::nudge_timeline_value`].
    pub fn timeline_current_value(&self, layer_id: &str, key: &str) -> Option<f32> {
        let current_tc = self.clock.timecode();
        let comp = self.active_composition()?;
        let layer = comp.get_layer(layer_id)?;
        let eval_vec = |p: &Property<Vec2>| {
            if p.is_animated() {
                p.evaluate_at(&current_tc)
            } else {
                p.value
            }
        };
        let eval_num = |p: &Property<f32>| {
            if p.is_animated() {
                p.evaluate_at(&current_tc)
            } else {
                p.value
            }
        };
        match key {
            "anchor_x" => Some(eval_vec(&layer.transform.anchor_point).x),
            "anchor_y" => Some(eval_vec(&layer.transform.anchor_point).y),
            "pos_x" => Some(eval_vec(&layer.transform.position).x),
            "pos_y" => Some(eval_vec(&layer.transform.position).y),
            "scale_x" => Some(eval_vec(&layer.transform.scale).x),
            "scale_y" => Some(eval_vec(&layer.transform.scale).y),
            "rotation" => Some(eval_num(&layer.transform.rotation)),
            "opacity" => Some(eval_num(&layer.opacity)),
            _ => {
                let rest = key.strip_prefix("fx:")?;
                let parts: Vec<&str> = rest.split(':').collect();
                if parts.len() < 2 {
                    return None;
                }
                let prop = layer.get_effect(parts[0])?.get_param_property(parts[1])?;
                Some(eval_num(prop))
            }
        }
    }

    /// Set a timeline row to an absolute value (typed entry). Returns false
    /// for unknown keys or missing layers/effects.
    pub fn set_timeline_value(&mut self, layer_id: &str, key: &str, v: f32) -> bool {
        if !v.is_finite() {
            return false;
        }
        let lid = layer_id.to_string();
        match key {
            "anchor_x" | "anchor_y" | "pos_x" | "pos_y" => {
                let (cx0, cy0) = match self.timeline_current_value(&lid, key) {
                    Some(cur) => {
                        let other = match key {
                            "anchor_x" => self
                                .timeline_current_value(&lid, "anchor_y")
                                .unwrap_or(0.0),
                            "anchor_y" => self
                                .timeline_current_value(&lid, "anchor_x")
                                .unwrap_or(0.0),
                            "pos_x" => {
                                self.timeline_current_value(&lid, "pos_y").unwrap_or(0.0)
                            }
                            _ => self.timeline_current_value(&lid, "pos_x").unwrap_or(0.0),
                        };
                        if key.ends_with("_x") {
                            (cur, other)
                        } else {
                            (other, cur)
                        }
                    }
                    None => return false,
                };
                match key {
                    "anchor_x" => self.nudge_layer_anchor(&lid, v - cx0, 0.0),
                    "anchor_y" => self.nudge_layer_anchor(&lid, 0.0, v - cy0),
                    "pos_x" => self.nudge_layer_position(&lid, v - cx0, 0.0),
                    _ => self.nudge_layer_position(&lid, 0.0, v - cy0),
                }
                true
            }
            "scale_x" | "scale_y" => {
                let cur = match self.timeline_current_value(&lid, key) {
                    Some(c) => c,
                    None => return false,
                };
                if key == "scale_x" {
                    self.nudge_layer_scale(&lid, v - cur, 0.0);
                } else {
                    self.nudge_layer_scale(&lid, 0.0, v - cur);
                }
                true
            }
            "rotation" => {
                let cur = match self.timeline_current_value(&lid, key) {
                    Some(c) => c,
                    None => return false,
                };
                self.nudge_layer_rotation(&lid, v - cur);
                true
            }
            "opacity" => {
                let cur = match self.timeline_current_value(&lid, key) {
                    Some(c) => c,
                    None => return false,
                };
                self.nudge_layer_opacity(&lid, v - cur);
                true
            }
            _ => {
                let rest = match key.strip_prefix("fx:") {
                    Some(r) => r,
                    None => return false,
                };
                let parts: Vec<&str> = rest.split(':').collect();
                if parts.len() < 2 {
                    return false;
                }
                let cur = match self.timeline_current_value(&lid, key) {
                    Some(c) => c,
                    None => return false,
                };
                self.nudge_layer_effect_param(&lid, parts[0], parts[1], v - cur)
                    .is_ok()
            }
        }
    }

    /// Toggle stopwatch / animation status for a property path on the specified layer.
    /// In After Effects:
    /// - If toggled ON: records an initial keyframe at the current playback time with the current value.
    /// - If toggled OFF: clears all keyframes on the property.
    pub fn toggle_layer_property_animation(&mut self, layer_id: &str, prop_path: &str) {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return,
        };

        match prop_path {
            "transform.anchor_point" => {
                if layer.transform.anchor_point.is_animated() {
                    layer.transform.anchor_point.clear_keyframes();
                } else {
                    let val = layer.transform.anchor_point.value;
                    layer.transform.anchor_point.add_keyframe(Keyframe::new(current_tc, val));
                }
            }
            "transform.position" => {
                if layer.transform.position.is_animated() {
                    layer.transform.position.clear_keyframes();
                } else {
                    let val = layer.transform.position.value;
                    layer.transform.position.add_keyframe(Keyframe::new(current_tc, val));
                }
            }
            "transform.scale" => {
                if layer.transform.scale.is_animated() {
                    layer.transform.scale.clear_keyframes();
                } else {
                    let val = layer.transform.scale.value;
                    layer.transform.scale.add_keyframe(Keyframe::new(current_tc, val));
                }
            }
            "transform.rotation" => {
                if layer.transform.rotation.is_animated() {
                    layer.transform.rotation.clear_keyframes();
                } else {
                    let val = layer.transform.rotation.value;
                    layer.transform.rotation.add_keyframe(Keyframe::new(current_tc, val));
                }
            }
            "opacity" => {
                if layer.opacity.is_animated() {
                    layer.opacity.clear_keyframes();
                } else {
                    let val = layer.opacity.value;
                    layer.opacity.add_keyframe(Keyframe::new(current_tc, val));
                }
            }
            "text.source" => {
                if let LayerSource::Text { text, .. } = &mut layer.source {
                    if text.is_animated() { text.clear_keyframes(); }
                    else { text.add_keyframe(Keyframe::new(current_tc, text.value.clone())); }
                }
            }
            "text.font_size" => {
                if let LayerSource::Text { font_size, .. } = &mut layer.source {
                    if font_size.is_animated() { font_size.clear_keyframes(); }
                    else { font_size.add_keyframe(Keyframe::new(current_tc, font_size.value)); }
                }
            }
            "text.tracking" => {
                if let LayerSource::Text { tracking, .. } = &mut layer.source {
                    if tracking.is_animated() { tracking.clear_keyframes(); }
                    else { tracking.add_keyframe(Keyframe::new(current_tc, tracking.value)); }
                }
            }
            "text.leading" => {
                if let LayerSource::Text { leading, .. } = &mut layer.source {
                    if leading.is_animated() { leading.clear_keyframes(); }
                    else { leading.add_keyframe(Keyframe::new(current_tc, leading.value)); }
                }
            }
            "text.stroke_width" => {
                if let LayerSource::Text { stroke_width, .. } = &mut layer.source {
                    if stroke_width.is_animated() { stroke_width.clear_keyframes(); }
                    else { stroke_width.add_keyframe(Keyframe::new(current_tc, stroke_width.value)); }
                }
            }
            "text.baseline_shift" => {
                if let LayerSource::Text { baseline_shift, .. } = &mut layer.source {
                    if baseline_shift.is_animated() { baseline_shift.clear_keyframes(); }
                    else { baseline_shift.add_keyframe(Keyframe::new(current_tc, baseline_shift.value)); }
                }
            }
            "text.box_width" => {
                if let LayerSource::Text { box_width, .. } = &mut layer.source {
                    if box_width.is_animated() { box_width.clear_keyframes(); }
                    else { box_width.add_keyframe(Keyframe::new(current_tc, box_width.value)); }
                }
            }
            "text.fill_color" => {
                if let LayerSource::Text { fill_color, .. } = &mut layer.source {
                    if fill_color.is_animated() { fill_color.clear_keyframes(); }
                    else { fill_color.add_keyframe(Keyframe::new(current_tc, fill_color.value)); }
                }
            }
            "shape.rect_width" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { width, .. } } = &mut layer.source {
                    if width.is_animated() { width.clear_keyframes(); }
                    else { let val = width.value; width.add_keyframe(Keyframe::new(current_tc, val)); }
                }
            }
            "shape.rect_height" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { height, .. } } = &mut layer.source {
                    if height.is_animated() { height.clear_keyframes(); }
                    else { let val = height.value; height.add_keyframe(Keyframe::new(current_tc, val)); }
                }
            }
            "shape.corner_radius" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { corner_radius, .. } } = &mut layer.source {
                    if corner_radius.is_animated() { corner_radius.clear_keyframes(); }
                    else { let val = corner_radius.value; corner_radius.add_keyframe(Keyframe::new(current_tc, val)); }
                }
            }
            "shape.ellipse_rx" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { radius_x, .. } } = &mut layer.source {
                    if radius_x.is_animated() { radius_x.clear_keyframes(); }
                    else { let val = radius_x.value; radius_x.add_keyframe(Keyframe::new(current_tc, val)); }
                }
            }
            "shape.ellipse_ry" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { radius_y, .. } } = &mut layer.source {
                    if radius_y.is_animated() { radius_y.clear_keyframes(); }
                    else { let val = radius_y.value; radius_y.add_keyframe(Keyframe::new(current_tc, val)); }
                }
            }
            _ => {
                if let Some(rest) = prop_path.strip_prefix("effect:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let fx_id = parts[0];
                        let param_name = parts[1];
                        if let Some(fx) = layer.get_effect_mut(fx_id) {
                            if let Some(prop) = fx.get_param_property_mut(param_name) {
                                if prop.is_animated() {
                                    prop.clear_keyframes();
                                } else {
                                    let val = prop.value;
                                    prop.add_keyframe(Keyframe::new(current_tc, val));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Toggle a keyframe at the current playback timecode for a property path on the layer.

    /// Add a keyframe at an absolute time (value sampled from the track).
    pub fn add_graph_keyframe(&mut self, layer_id: &str, path: &str, t_s: f64) -> bool {
        self.checkpoint();
        let (fps, current) = match self.active_composition() {
            Some(c) => match self.evaluate_graph_param(layer_id, path, t_s) {
                Some(v) => (c.frame_rate, v),
                None => return false,
            },
            None => return false,
        };
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return false,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return false,
        };
        let tc = TimeCode::from_seconds(t_s.max(0.0), fps);
        let (base, comp_sfx) = match split_graph_path(path) {
            Some(v) => v,
            None => return false,
        };
        let axis = comp_sfx.unwrap_or(0);
        match base {
            "transform.anchor_point" | "transform.position" | "transform.scale" => {
                let prop = match base {
                    "transform.anchor_point" => &mut layer.transform.anchor_point,
                    "transform.position" => &mut layer.transform.position,
                    _ => &mut layer.transform.scale,
                };
                // Full Vec2 from the evaluated track, with the graphed
                // component pinned to the sampled value.
                let full = if prop.is_animated() {
                    prop.evaluate_at(&tc)
                } else {
                    prop.value
                };
                let v = if axis == 0 {
                    Vec2::new(current, full.y)
                } else {
                    Vec2::new(full.x, current)
                };
                prop.add_keyframe(Keyframe::new(tc, v));
                true
            }
            _ => {
                let mut done = false;
                match with_graph_scalar(layer, path, |p| {
                    p.add_keyframe(Keyframe::new(tc, current));
                    done = true;
                }) {

                    Some(_) => done,

                    None => return false,

                }
            }
        }
    }

    /// Delete the keyframe near an absolute time.
    pub fn remove_graph_keyframe(&mut self, layer_id: &str, path: &str, at_s: f64) -> bool {
        self.checkpoint();
        let fps = match self.active_composition() {
            Some(c) => c.frame_rate,
            None => return false,
        };
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return false,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return false,
        };
        let tc = TimeCode::from_seconds(at_s.max(0.0), fps);
        let (base, _) = match split_graph_path(path) {
            Some(v) => v,
            None => return false,
        };
        let found = match base {
            "transform.anchor_point" => {
                layer.transform.anchor_point.remove_keyframe_at(&tc).is_some()
            }
            "transform.position" => layer.transform.position.remove_keyframe_at(&tc).is_some(),
            "transform.scale" => layer.transform.scale.remove_keyframe_at(&tc).is_some(),
            _ => {
                let mut done = false;
                match with_graph_scalar(layer, path, |p| {
                    done = p.remove_keyframe_at(&tc).is_some();
                }) {

                    Some(_) => done,

                    None => return false,

                }
            }
        };
        found
    }

    /// Cycle a keyframe's interpolation Linear -> Bezier -> Hold.
    pub fn cycle_graph_key_interp(&mut self, layer_id: &str, path: &str, at_s: f64) -> bool {
        self.checkpoint();
        let fps = match self.active_composition() {
            Some(c) => c.frame_rate,
            None => return false,
        };
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return false,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return false,
        };
        let tol = 0.5 / fps.max(1.0);
        let cycle = |interp: &mut KeyframeInterpolation| {
            *interp = match interp {
                KeyframeInterpolation::Linear => KeyframeInterpolation::Bezier,
                KeyframeInterpolation::Bezier => KeyframeInterpolation::Hold,
                KeyframeInterpolation::Hold => KeyframeInterpolation::Linear,
            };
        };
        let (base, _) = match split_graph_path(path) {
            Some(v) => v,
            None => return false,
        };
        match base {
            "transform.anchor_point" => {
                let kfs = layer.transform.anchor_point.keyframes_mut();
                let kf = match kfs.iter_mut().find(|k| (k.time_seconds() - at_s).abs() <= tol) {
                    Some(k) => k,
                    None => return false,
                };
                cycle(&mut kf.interpolation);
                true
            }
            "transform.position" => {
                let kfs = layer.transform.position.keyframes_mut();
                let kf = match kfs.iter_mut().find(|k| (k.time_seconds() - at_s).abs() <= tol) {
                    Some(k) => k,
                    None => return false,
                };
                cycle(&mut kf.interpolation);
                true
            }
            "transform.scale" => {
                let kfs = layer.transform.scale.keyframes_mut();
                let kf = match kfs.iter_mut().find(|k| (k.time_seconds() - at_s).abs() <= tol) {
                    Some(k) => k,
                    None => return false,
                };
                cycle(&mut kf.interpolation);
                true
            }
            _ => {
                let mut done = false;
                match with_graph_scalar(layer, path, |p| {
                    if let Some(kf) = p
                        .keyframes_mut()
                        .iter_mut()
                        .find(|k| (k.time_seconds() - at_s).abs() <= tol)
                    {
                        cycle(&mut kf.interpolation);
                        done = true;
                    }
                }) {

                    Some(_) => done,

                    None => return false,

                }
            }
        }
    }

    /// Set bezier tangents on a keyframe (forces Bezier interpolation).
    /// One undo step.
    pub fn set_graph_key_tangents(
        &mut self,
        layer_id: &str,
        path: &str,
        at_s: f64,
        in_tan: Option<(f32, f32)>,
        out_tan: Option<(f32, f32)>,
    ) -> bool {
        self.checkpoint();
        self.set_graph_key_tangents_live(layer_id, path, at_s, in_tan, out_tan)
    }

    /// Live tangent write without a checkpoint (handle drags; the caller
    /// checkpoints once on drag start).
    pub fn set_graph_key_tangents_live(
        &mut self,
        layer_id: &str,
        path: &str,
        at_s: f64,
        in_tan: Option<(f32, f32)>,
        out_tan: Option<(f32, f32)>,
    ) -> bool {
        let fps = match self.active_composition() {
            Some(c) => c.frame_rate,
            None => return false,
        };
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return false,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return false,
        };
        let tol = 0.5 / fps.max(1.0);
        let apply = |interp: &mut KeyframeInterpolation,
                     it: &mut Option<KeyframeTangent>,
                     ot: &mut Option<KeyframeTangent>| {
            *interp = KeyframeInterpolation::Bezier;
            if let Some((x, y)) = in_tan {
                *it = Some(KeyframeTangent::new(x, y));
            }
            if let Some((x, y)) = out_tan {
                *ot = Some(KeyframeTangent::new(x, y));
            }
        };
        let (base, _) = match split_graph_path(path) {
            Some(v) => v,
            None => return false,
        };
        match base {
            "transform.anchor_point" => {
                let kfs = layer.transform.anchor_point.keyframes_mut();
                let kf = match kfs.iter_mut().find(|k| (k.time_seconds() - at_s).abs() <= tol) {
                    Some(k) => k,
                    None => return false,
                };
                apply(&mut kf.interpolation, &mut kf.in_tangent, &mut kf.out_tangent);
                true
            }
            "transform.position" => {
                let kfs = layer.transform.position.keyframes_mut();
                let kf = match kfs.iter_mut().find(|k| (k.time_seconds() - at_s).abs() <= tol) {
                    Some(k) => k,
                    None => return false,
                };
                apply(&mut kf.interpolation, &mut kf.in_tangent, &mut kf.out_tangent);
                true
            }
            "transform.scale" => {
                let kfs = layer.transform.scale.keyframes_mut();
                let kf = match kfs.iter_mut().find(|k| (k.time_seconds() - at_s).abs() <= tol) {
                    Some(k) => k,
                    None => return false,
                };
                apply(&mut kf.interpolation, &mut kf.in_tangent, &mut kf.out_tangent);
                true
            }
            _ => {
                let mut done = false;
                match with_graph_scalar(layer, path, |p| {
                    if let Some(kf) = p
                        .keyframes_mut()
                        .iter()
                        .position(|k| (k.time_seconds() - at_s).abs() <= tol)
                    {
                        let kf = &mut p.keyframes_mut()[kf];
                        apply(&mut kf.interpolation, &mut kf.in_tangent, &mut kf.out_tangent);
                        done = true;
                    }
                }) {

                    Some(_) => done,

                    None => return false,

                }
            }
        }
    }

    pub fn graph_series(&self, layer_id: &str) -> Vec<GraphSeries> {
        let comp = match self.active_composition() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let layer = match comp.get_layer(layer_id) {
            Some(l) => l,
            None => return Vec::new(),
        };
        graph_candidates(layer)
            .into_iter()
            .enumerate()
            .filter_map(|(i, (path, label))| {
                let (keys, _) = graph_prop_read(layer, &path)?;
                if keys.is_empty() {
                    return None;
                }
                Some(GraphSeries {
                    path,
                    label,
                    color: GRAPH_COLORS[i % GRAPH_COLORS.len()],
                    keys,
                })
            })
            .collect()
    }

    /// Sample a graph path at absolute seconds (for curve drawing).
    pub fn evaluate_graph_param(&self, layer_id: &str, path: &str, seconds: f64) -> Option<f32> {
        let comp = self.active_composition()?;
        let layer = comp.get_layer(layer_id)?;
        let fps = comp.frame_rate;
        let tc = TimeCode::from_seconds(seconds.max(0.0), fps);
        let (base, comp_sfx) = split_graph_path(path)?;
        let is_vec2_base = matches!(
            base,
            "transform.anchor_point" | "transform.position" | "transform.scale"
        );
        if is_vec2_base != comp_sfx.is_some() {
            return None;
        }
        let axis = comp_sfx.unwrap_or(1);
        let get_vec = |p: &Property<Vec2>| {
            let v = if p.is_animated() { p.evaluate_at(&tc) } else { p.value };
            if axis == 0 { v.x } else { v.y }
        };
        let get_f32 = |p: &Property<f32>| {
            if p.is_animated() { p.evaluate_at(&tc) } else { p.value }
        };
        Some(match base {
            "transform.anchor_point" => get_vec(&layer.transform.anchor_point),
            "transform.position" => get_vec(&layer.transform.position),
            "transform.scale" => get_vec(&layer.transform.scale),
            "transform.rotation" => get_f32(&layer.transform.rotation),
            "opacity" => get_f32(&layer.opacity),
            "text.font_size" => match &layer.source {
                LayerSource::Text { font_size, .. } => get_f32(font_size),
                _ => return None,
            },
            "text.tracking" => match &layer.source {
                LayerSource::Text { tracking, .. } => get_f32(tracking),
                _ => return None,
            },
            "text.leading" => match &layer.source {
                LayerSource::Text { leading, .. } => get_f32(leading),
                _ => return None,
            },
            "text.stroke_width" => match &layer.source {
                LayerSource::Text { stroke_width, .. } => get_f32(stroke_width),
                _ => return None,
            },
            "text.baseline_shift" => match &layer.source {
                LayerSource::Text { baseline_shift, .. } => get_f32(baseline_shift),
                _ => return None,
            },
            "text.box_width" => match &layer.source {
                LayerSource::Text { box_width, .. } => get_f32(box_width),
                _ => return None,
            },
            "shape.rect_width" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Rectangle { width, .. } } => get_f32(width),
                _ => return None,
            },
            "shape.rect_height" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Rectangle { height, .. } } => get_f32(height),
                _ => return None,
            },
            "shape.corner_radius" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Rectangle { corner_radius, .. } } => {
                    get_f32(corner_radius)
                }
                _ => return None,
            },
            "shape.ellipse_rx" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_x, .. } } => get_f32(radius_x),
                _ => return None,
            },
            "shape.ellipse_ry" => match &layer.source {
                LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_y, .. } } => get_f32(radius_y),
                _ => return None,
            },
            _ => {
                let rest = base.strip_prefix("effect:")?;
                let mut parts = rest.splitn(2, ':');
                let (eid, pname) = (parts.next()?, parts.next()?);
                get_f32(layer.get_effect(eid)?.get_param_property(pname)?)
            }
        })
    }

    /// Find a keyframe index on a scalar prop near `at_s` (half-frame window).

    /// Move a keyframe to a new time/value, preserving interpolation.
    pub fn move_graph_keyframe(
        &mut self,
        layer_id: &str,
        path: &str,
        at_s: f64,
        new_t_s: f64,
        new_v: f32,
    ) -> bool {
        self.checkpoint();
        self.move_graph_keyframe_live(layer_id, path, at_s, new_t_s, new_v)
    }

    /// Move a keyframe without creating an undo checkpoint (for live drags;
    /// callers checkpoint once on drag start).
    pub fn move_graph_keyframe_live(
        &mut self,
        layer_id: &str,
        path: &str,
        at_s: f64,
        new_t_s: f64,
        new_v: f32,
    ) -> bool {
        let fps = match self.active_composition() {
            Some(c) => c.frame_rate,
            None => return false,
        };
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return false,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return false,
        };
        let (base, comp_sfx) = match split_graph_path(path) {
            Some(v) => v,
            None => return false,
        };
        let axis = comp_sfx.unwrap_or(0);
        let tol = 0.5 / fps.max(1.0);
        let new_tc = TimeCode::from_seconds(new_t_s.max(0.0), fps);
        match base {
            "transform.anchor_point" | "transform.position" | "transform.scale" => {
                let prop = match base {
                    "transform.anchor_point" => &mut layer.transform.anchor_point,
                    "transform.position" => &mut layer.transform.position,
                    _ => &mut layer.transform.scale,
                };
                let idx = match prop
                    .keyframes()
                    .iter()
                    .position(|k| (k.time_seconds() - at_s).abs() <= tol)
                {
                    Some(i) => i,
                    None => return false,
                };
                let mut kf = prop.keyframes()[idx].clone();
                let mut v = kf.value;
                if axis == 0 {
                    v.x = new_v;
                } else {
                    v.y = new_v;
                }
                kf.value = v;
                kf.time = new_tc;
                prop.keyframes_mut().remove(idx);
                prop.add_keyframe(kf);
                true
            }
            _ => {
                let mut done = false;
                match with_graph_scalar(layer, path, |p| {
                    let idx = match p
                        .keyframes()
                        .iter()
                        .position(|k| (k.time_seconds() - at_s).abs() <= tol)
                    {
                        Some(i) => i,
                        None => return,
                    };
                    let mut kf = p.keyframes()[idx].clone();
                    kf.value = new_v;
                    kf.time = new_tc;
                    p.keyframes_mut().remove(idx);
                    p.add_keyframe(kf);
                    done = true;
                }) {

                    Some(_) => done,

                    None => return false,

                }
            }
        }
    }

    /// Helper: run a closure over a scalar (f32) graph property.

    pub fn toggle_layer_keyframe_at_current_time(&mut self, layer_id: &str, prop_path: &str) {
        self.checkpoint();        let current_tc = self.clock.timecode();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return,
        };

        match prop_path {
            "transform.anchor_point" => {
                let val = if layer.transform.anchor_point.is_animated() {
                    layer.transform.anchor_point.evaluate_at(&current_tc)
                } else {
                    layer.transform.anchor_point.value
                };
                layer.transform.anchor_point.toggle_keyframe(current_tc, val);
            }
            "transform.position" => {
                let val = if layer.transform.position.is_animated() {
                    layer.transform.position.evaluate_at(&current_tc)
                } else {
                    layer.transform.position.value
                };
                layer.transform.position.toggle_keyframe(current_tc, val);
            }
            "transform.scale" => {
                let val = if layer.transform.scale.is_animated() {
                    layer.transform.scale.evaluate_at(&current_tc)
                } else {
                    layer.transform.scale.value
                };
                layer.transform.scale.toggle_keyframe(current_tc, val);
            }
            "transform.rotation" => {
                let val = if layer.transform.rotation.is_animated() {
                    layer.transform.rotation.evaluate_at(&current_tc)
                } else {
                    layer.transform.rotation.value
                };
                layer.transform.rotation.toggle_keyframe(current_tc, val);
            }
            "opacity" => {
                let val = if layer.opacity.is_animated() {
                    layer.opacity.evaluate_at(&current_tc)
                } else {
                    layer.opacity.value
                };
                layer.opacity.toggle_keyframe(current_tc, val);
            }
            "text.font_size" => {
                if let LayerSource::Text { ref mut font_size, .. } = layer.source {
                    let val = if font_size.is_animated() {
                        font_size.evaluate_at(&current_tc)
                    } else {
                        font_size.value
                    };
                    font_size.toggle_keyframe(current_tc, val);
                }
            }
            "text.tracking" => {
                if let LayerSource::Text { ref mut tracking, .. } = layer.source {
                    let val = if tracking.is_animated() {
                        tracking.evaluate_at(&current_tc)
                    } else {
                        tracking.value
                    };
                    tracking.toggle_keyframe(current_tc, val);
                }
            }
            "text.leading" => {
                if let LayerSource::Text { ref mut leading, .. } = layer.source {
                    let val = if leading.is_animated() {
                        leading.evaluate_at(&current_tc)
                    } else {
                        leading.value
                    };
                    leading.toggle_keyframe(current_tc, val);
                }
            }
            "text.stroke_width" => {
                if let LayerSource::Text { ref mut stroke_width, .. } = layer.source {
                    let val = if stroke_width.is_animated() {
                        stroke_width.evaluate_at(&current_tc)
                    } else {
                        stroke_width.value
                    };
                    stroke_width.toggle_keyframe(current_tc, val);
                }
            }
            "text.baseline_shift" => {
                if let LayerSource::Text { ref mut baseline_shift, .. } = layer.source {
                    let val = if baseline_shift.is_animated() {
                        baseline_shift.evaluate_at(&current_tc)
                    } else {
                        baseline_shift.value
                    };
                    baseline_shift.toggle_keyframe(current_tc, val);
                }
            }
            "text.box_width" => {
                if let LayerSource::Text { ref mut box_width, .. } = layer.source {
                    let val = if box_width.is_animated() {
                        box_width.evaluate_at(&current_tc)
                    } else {
                        box_width.value
                    };
                    box_width.toggle_keyframe(current_tc, val);
                }
            }
            "text.fill_color" => {
                if let LayerSource::Text { ref mut fill_color, .. } = layer.source {
                    let val = if fill_color.is_animated() {
                        fill_color.evaluate_at(&current_tc)
                    } else {
                        fill_color.value
                    };
                    fill_color.toggle_keyframe(current_tc, val);
                }
            }
            "shape.rect_width" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref mut width, .. } } = layer.source {
                    let val = if width.is_animated() {
                        width.evaluate_at(&current_tc)
                    } else {
                        width.value
                    };
                    width.toggle_keyframe(current_tc, val);
                }
            }
            "shape.rect_height" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref mut height, .. } } = layer.source {
                    let val = if height.is_animated() {
                        height.evaluate_at(&current_tc)
                    } else {
                        height.value
                    };
                    height.toggle_keyframe(current_tc, val);
                }
            }
            "shape.corner_radius" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref mut corner_radius, .. } } = layer.source {
                    let val = if corner_radius.is_animated() {
                        corner_radius.evaluate_at(&current_tc)
                    } else {
                        corner_radius.value
                    };
                    corner_radius.toggle_keyframe(current_tc, val);
                }
            }
            "shape.ellipse_rx" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref mut radius_x, .. } } = layer.source {
                    let val = if radius_x.is_animated() {
                        radius_x.evaluate_at(&current_tc)
                    } else {
                        radius_x.value
                    };
                    radius_x.toggle_keyframe(current_tc, val);
                }
            }
            "shape.ellipse_ry" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref mut radius_y, .. } } = layer.source {
                    let val = if radius_y.is_animated() {
                        radius_y.evaluate_at(&current_tc)
                    } else {
                        radius_y.value
                    };
                    radius_y.toggle_keyframe(current_tc, val);
                }
            }
            _ => {
                if let Some(rest) = prop_path.strip_prefix("effect:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let fx_id = parts[0];
                        let param_name = parts[1];
                        if let Some(fx) = layer.get_effect_mut(fx_id) {
                            if let Some(prop) = fx.get_param_property_mut(param_name) {
                                let val = if prop.is_animated() {
                                    prop.evaluate_at(&current_tc)
                                } else {
                                    prop.value
                                };
                                prop.toggle_keyframe(current_tc, val);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Seek the playback clock to `time_seconds`.
    pub fn seek(&mut self, time_seconds: f64) {
        self.clock.seek_seconds(time_seconds);
    }

    /// Toggle a keyframe at the current playhead time for a property path on the layer.
    pub fn toggle_layer_property_keyframe_at_playhead(&mut self, layer_id: &str, prop_path: &str) {
        self.toggle_layer_keyframe_at_current_time(layer_id, prop_path);
    }

    /// Seek to the previous keyframe for the given property path on the layer.
    pub fn seek_previous_keyframe(&mut self, layer_id: &str, prop_path: &str) {
        let current_tc = self.clock.timecode();
        let prev_time = {
            let comp = match self.active_composition() {
                Some(c) => c,
                None => return,
            };
            let layer = match comp.get_layer(layer_id) {
                Some(l) => l,
                None => return,
            };
            match prop_path {
                "transform.anchor_point" => layer.transform.anchor_point.previous_keyframe_time(&current_tc),
                "transform.position" => layer.transform.position.previous_keyframe_time(&current_tc),
                "transform.scale" => layer.transform.scale.previous_keyframe_time(&current_tc),
                "transform.rotation" => layer.transform.rotation.previous_keyframe_time(&current_tc),
                "opacity" => layer.opacity.previous_keyframe_time(&current_tc),
                "text.font_size" => {
                    if let LayerSource::Text { ref font_size, .. } = layer.source {
                        font_size.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.tracking" => {
                    if let LayerSource::Text { ref tracking, .. } = layer.source {
                        tracking.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.leading" => {
                    if let LayerSource::Text { ref leading, .. } = layer.source {
                        leading.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.stroke_width" => {
                    if let LayerSource::Text { ref stroke_width, .. } = layer.source {
                        stroke_width.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.baseline_shift" => {
                    if let LayerSource::Text { ref baseline_shift, .. } = layer.source {
                        baseline_shift.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.box_width" => {
                    if let LayerSource::Text { ref box_width, .. } = layer.source {
                        box_width.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.fill_color" => {
                    if let LayerSource::Text { ref fill_color, .. } = layer.source {
                        fill_color.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.rect_width" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref width, .. } } = layer.source {
                        width.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.rect_height" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref height, .. } } = layer.source {
                        height.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.corner_radius" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref corner_radius, .. } } = layer.source {
                        corner_radius.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.ellipse_rx" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref radius_x, .. } } = layer.source {
                        radius_x.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.ellipse_ry" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref radius_y, .. } } = layer.source {
                        radius_y.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                _ => {
                    if let Some(rest) = prop_path.strip_prefix("effect:") {
                        let parts: Vec<&str> = rest.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let fx_id = parts[0];
                            let param_name = parts[1];
                            layer.get_effect(fx_id)
                                .and_then(|fx| fx.get_param_property(param_name))
                                .and_then(|prop| prop.previous_keyframe_time(&current_tc))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            }
        };

        if let Some(tc) = prev_time {
            self.clock.seek(tc);
        }
    }

    /// Seek to the next keyframe for the given property path on the layer.
    pub fn seek_next_keyframe(&mut self, layer_id: &str, prop_path: &str) {
        let current_tc = self.clock.timecode();
        let next_time = {
            let comp = match self.active_composition() {
                Some(c) => c,
                None => return,
            };
            let layer = match comp.get_layer(layer_id) {
                Some(l) => l,
                None => return,
            };
            match prop_path {
                "transform.anchor_point" => layer.transform.anchor_point.next_keyframe_time(&current_tc),
                "transform.position" => layer.transform.position.next_keyframe_time(&current_tc),
                "transform.scale" => layer.transform.scale.next_keyframe_time(&current_tc),
                "transform.rotation" => layer.transform.rotation.next_keyframe_time(&current_tc),
                "opacity" => layer.opacity.next_keyframe_time(&current_tc),
                "text.font_size" => {
                    if let LayerSource::Text { ref font_size, .. } = layer.source {
                        font_size.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.tracking" => {
                    if let LayerSource::Text { ref tracking, .. } = layer.source {
                        tracking.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.leading" => {
                    if let LayerSource::Text { ref leading, .. } = layer.source {
                        leading.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.stroke_width" => {
                    if let LayerSource::Text { ref stroke_width, .. } = layer.source {
                        stroke_width.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.baseline_shift" => {
                    if let LayerSource::Text { ref baseline_shift, .. } = layer.source {
                        baseline_shift.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.box_width" => {
                    if let LayerSource::Text { ref box_width, .. } = layer.source {
                        box_width.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "text.fill_color" => {
                    if let LayerSource::Text { ref fill_color, .. } = layer.source {
                        fill_color.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.rect_width" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref width, .. } } = layer.source {
                        width.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.rect_height" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref height, .. } } = layer.source {
                        height.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.corner_radius" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref corner_radius, .. } } = layer.source {
                        corner_radius.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.ellipse_rx" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref radius_x, .. } } = layer.source {
                        radius_x.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.ellipse_ry" => {
                    if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref radius_y, .. } } = layer.source {
                        radius_y.next_keyframe_time(&current_tc)
                    } else { None }
                }
                _ => {
                    if let Some(rest) = prop_path.strip_prefix("effect:") {
                        let parts: Vec<&str> = rest.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let fx_id = parts[0];
                            let param_name = parts[1];
                            layer.get_effect(fx_id)
                                .and_then(|fx| fx.get_param_property(param_name))
                                .and_then(|prop| prop.next_keyframe_time(&current_tc))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            }
        };

        if let Some(tc) = next_time {
            self.clock.seek(tc);
        }
    }

    /// Toggle Timeline Spline / Graph Editor view.
    pub fn toggle_spline_editor(&mut self) {
        self.spline_editor_open = !self.spline_editor_open;
    }

    /// Set the active property inspected in the Spline Editor.
    pub fn set_spline_prop_path(&mut self, path: &str) {
        self.spline_prop_path = path.to_string();
    }

    /// Apply an easing preset (Linear, Ease In, Ease Out, Easy Ease, Hold) to all keyframes
    /// of the specified property path on the layer.
    pub fn set_layer_property_easing(&mut self, layer_id: &str, prop_path: &str, easing: EasingPreset) {
        self.checkpoint();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return,
        };
        match prop_path {
            "transform.anchor_point" => apply_easing_to_prop(&mut layer.transform.anchor_point, easing),
            "transform.position" | "transform.position.x" | "transform.position.y" => {
                apply_easing_to_prop(&mut layer.transform.position, easing)
            }
            "transform.scale" | "transform.scale.x" | "transform.scale.y" => {
                apply_easing_to_prop(&mut layer.transform.scale, easing)
            }
            "transform.rotation" => apply_easing_to_prop(&mut layer.transform.rotation, easing),
            "opacity" => apply_easing_to_prop(&mut layer.opacity, easing),
            _ => {
                if let Some(rest) = prop_path.strip_prefix("effect:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        if let Some(fx) = layer.get_effect_mut(parts[0]) {
                            if let Some(prop) = fx.get_param_property_mut(parts[1]) {
                                apply_easing_to_prop(prop, easing);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Return normalized keyframe points `(time_ratio, scalar_value, interpolation)` for the spline graph.
    pub fn get_spline_keyframe_points(&self, layer_id: &str, prop_path: &str) -> Vec<(f32, f32, project::KeyframeInterpolation)> {
        let comp = match self.active_composition() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let duration = comp.duration_seconds().max(0.01) as f32;
        let layer = match comp.get_layer(layer_id) {
            Some(l) => l,
            None => return Vec::new(),
        };
        let mut pts = Vec::new();
        match prop_path {
            "transform.position" | "transform.position.x" => {
                for kf in layer.transform.position.keyframes() {
                    let t = (kf.time.seconds() as f32 / duration).clamp(0.0, 1.0);
                    pts.push((t, kf.value.x, kf.interpolation));
                }
            }
            "transform.position.y" => {
                for kf in layer.transform.position.keyframes() {
                    let t = (kf.time.seconds() as f32 / duration).clamp(0.0, 1.0);
                    pts.push((t, kf.value.y, kf.interpolation));
                }
            }
            "transform.rotation" => {
                for kf in layer.transform.rotation.keyframes() {
                    let t = (kf.time.seconds() as f32 / duration).clamp(0.0, 1.0);
                    pts.push((t, kf.value, kf.interpolation));
                }
            }
            "transform.scale" | "transform.scale.x" => {
                for kf in layer.transform.scale.keyframes() {
                    let t = (kf.time.seconds() as f32 / duration).clamp(0.0, 1.0);
                    pts.push((t, kf.value.x, kf.interpolation));
                }
            }
            "transform.scale.y" => {
                for kf in layer.transform.scale.keyframes() {
                    let t = (kf.time.seconds() as f32 / duration).clamp(0.0, 1.0);
                    pts.push((t, kf.value.y, kf.interpolation));
                }
            }
            "opacity" => {
                for kf in layer.opacity.keyframes() {
                    let t = (kf.time.seconds() as f32 / duration).clamp(0.0, 1.0);
                    pts.push((t, kf.value, kf.interpolation));
                }
            }
            _ => {}
        }
        pts
    }

    /// Evaluates `sample_count` curve samples across composition duration for spline graph plotting.
    pub fn get_spline_curve_samples(&self, layer_id: &str, prop_path: &str, sample_count: usize) -> Vec<(f32, f32)> {
        let comp = match self.active_composition() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let total_dur = comp.duration_seconds().max(0.01);
        let layer = match comp.get_layer(layer_id) {
            Some(l) => l,
            None => return Vec::new(),
        };
        let samples = sample_count.max(10);
        let mut curve = Vec::with_capacity(samples);
        for i in 0..samples {
            let frac = i as f64 / (samples - 1) as f64;
            let time_sec = frac * total_dur;
            let val = match prop_path {
                "transform.position" | "transform.position.x" => {
                    layer.transform.position.evaluate_at_seconds(time_sec).x
                }
                "transform.position.y" => {
                    layer.transform.position.evaluate_at_seconds(time_sec).y
                }
                "transform.rotation" => {
                    layer.transform.rotation.evaluate_at_seconds(time_sec)
                }
                "transform.scale" | "transform.scale.x" => {
                    layer.transform.scale.evaluate_at_seconds(time_sec).x
                }
                "transform.scale.y" => {
                    layer.transform.scale.evaluate_at_seconds(time_sec).y
                }
                "opacity" => {
                    layer.opacity.evaluate_at_seconds(time_sec)
                }
                _ => 0.0,
            };
            curve.push((frac as f32, val));
        }
        curve
    }

    /// Update project and active composition settings (Project Manager).
    pub fn update_project_settings(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration_secs: f64,
    ) {
        self.checkpoint();
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            self.project.name = trimmed.to_string();
        }
        if let Some(comp) = self.active_composition_mut() {
            if !trimmed.is_empty() {
                comp.name = trimmed.to_string();
            }
            comp.width = width.clamp(320, 7680);
            comp.height = height.clamp(240, 4320);
            comp.frame_rate = fps.clamp(1.0, 120.0);
            let frames = (duration_secs * comp.frame_rate).round() as i64;
            comp.duration = project::TimeCode::from_frames(frames, comp.frame_rate);
        }
    }

    /// Create a new composition in the project and set it as active (Project Manager).
    pub fn create_composition(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration_secs: f64,
    ) -> String {
        self.checkpoint();
        let fps = fps.clamp(1.0, 120.0);
        let frames = (duration_secs * fps).round() as i64;
        let mut counter = self.project.compositions.len() + 1;
        let mut id = format!("comp_{counter}");
        while self.project.compositions.iter().any(|c| c.id == id) {
            counter += 1;
            id = format!("comp_{counter}");
        }
        let duration = project::TimeCode::from_frames(frames, fps);
        let comp = project::Composition::new(&id, name, width.clamp(320, 7680), height.clamp(240, 4320), fps, duration);
        let _ = self.project.add_composition(comp);
        self.active_comp_id = id.clone();
        self.selected_layer_id = None;
        id
    }

    /// Switch active composition by ID.
    pub fn set_active_composition(&mut self, comp_id: &str) {
        if self.project.compositions.iter().any(|c| c.id == comp_id) {
            self.active_comp_id = comp_id.to_string();
            self.selected_layer_id = None;
        }
    }

    /// Delete a composition by ID (guards against deleting the only composition).
    pub fn delete_composition(&mut self, comp_id: &str) -> Result<(), String> {
        if self.project.compositions.len() <= 1 {
            return Err("Cannot delete the only composition in the project".to_string());
        }
        self.checkpoint();
        self.project.remove_composition(comp_id);
        if self.active_comp_id == comp_id {
            if let Some(first) = self.project.compositions.first() {
                self.active_comp_id = first.id.clone();
                self.selected_layer_id = None;
            }
        }
        Ok(())
    }

    /// Set blend mode on the specified layer.
    pub fn set_layer_blend_mode(&mut self, layer_id: &str, mode: BlendMode) {
        self.checkpoint();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.blend_mode = mode;
            }
        }
    }

    /// Set track matte mode and optional target matte layer ID on the specified layer.
    pub fn set_layer_track_matte(&mut self, layer_id: &str, mode: TrackMatteMode, target_id: Option<String>) {
        self.checkpoint();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.set_matte(mode, target_id);
            }
        }
    }

    /// Set parent layer ID on the specified layer.
    ///
    /// After Effects semantics: any layer can parent any other layer, and
    /// (un)parenting preserves the child's world transform — the layer does
    /// not jump. The child's local position/rotation/scale are recomputed
    /// from `new_parent_world^-1 * child_world` (anchor unchanged).
    /// Rejects self-parenting, missing parents, and parent cycles.
    /// Returns true when the parenting changed.
    pub fn set_layer_parent(&mut self, layer_id: &str, parent_id: Option<String>) -> bool {
        if let Some(ref pid) = parent_id {
            if pid == layer_id {
                return false;
            }
            // Parent must exist; its ancestor chain must not contain the
            // child (otherwise parenting would create a cycle).
            let comp = match self.active_composition() {
                Some(c) => c,
                None => return false,
            };
            if comp.get_layer(pid).is_none() {
                return false;
            }
            let mut cursor: Option<&str> = Some(pid);
            let mut depth = 0;
            while let Some(id) = cursor {
                if id == layer_id {
                    return false;
                }
                depth += 1;
                if depth > 1024 {
                    return false;
                }
                cursor = comp.get_layer(id).and_then(|l| l.parent_id.as_deref());
            }
        }
        // No-op when nothing changes.
        if let Some(comp) = self.active_composition() {
            if let Some(layer) = comp.get_layer(layer_id) {
                if layer.parent_id == parent_id {
                    return false;
                }
            }
        }
        // --- Transform compensation: preserve world transform (zero visual jump) ---
        let child_world = self.layer_world_matrix_fast(layer_id);
        let new_parent_world = match &parent_id {
            Some(pid) => self.layer_world_matrix_fast(pid),
            None => Some(AffineTransform2D::IDENTITY),
        };
        let compensated = match (child_world, new_parent_world) {
            (Some(cw), Some(pw)) => {
                pw.inverse().and_then(|inv_pw| {
                    let new_local = if parent_id.is_some() { inv_pw * cw } else { cw };
                    let anchor = self.active_composition()
                        .and_then(|c| c.get_layer(layer_id))
                        .map(|l| *l.transform.anchor_point.value())
                        .unwrap_or(Vec2::ZERO);
                    AffineTransform2D::decompose_components(new_local, anchor)
                })
            }
            _ => None,
        };

        // --- Mutation phase (single undo step). ---
        self.checkpoint();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                if let Some((pos, scale, rot)) = compensated {
                    layer.transform.position.set_value(pos);
                    layer.transform.scale.set_value(scale);
                    layer.transform.rotation.set_value(rot);
                }
                layer.set_parent(parent_id);
            }
        }
        true
    }

    /// Cheap current world matrix for a layer (property reads + matrix
    /// multiplies only — no scene-graph evaluation). Used by gizmo drags so
    /// every mousemove does not pay a full `evaluate_current_frame`.
    /// Returns None on missing layers or parent cycles.
    pub fn layer_world_matrix_fast(&self, layer_id: &str) -> Option<AffineTransform2D> {
        let tc = self.clock.timecode();
        self.layer_world_matrix_at(layer_id, &tc)
    }

    /// Same cheap path at an explicit timecode (motion-path conversion).
    pub fn layer_world_matrix_at(
        &self,
        layer_id: &str,
        tc: &TimeCode,
    ) -> Option<AffineTransform2D> {
        let comp = self.active_composition()?;
        let mut chain: Vec<(Vec2, Vec2, Vec2, f32)> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<&str> = Some(layer_id);
        while let Some(id) = cursor {
            if seen.iter().any(|s| s == id) {
                return None;
            }
            seen.push(id.to_string());
            let layer = comp.get_layer(id)?;
            chain.push(layer.transform.evaluate_at(tc));
            cursor = layer.parent_id.as_deref();
        }
        let mut world = AffineTransform2D::IDENTITY;
        for (anchor, pos, scale, rot) in chain.iter().rev() {
            let local =
                AffineTransform2D::from_transform_components(*pos, *scale, *rot, *anchor);
            world = world * local;
        }
        Some(world)
    }

    /// Current world matrix + anchor for gizmo drag mapping.
    /// Same cheap path as [`Self::layer_world_matrix_fast`].
    pub fn layer_drag_frame(&self, layer_id: &str) -> Option<(AffineTransform2D, Vec2)> {
        let comp = self.active_composition()?;
        let tc = self.clock.timecode();
        let layer = comp.get_layer(layer_id)?;
        let anchor = layer.transform.anchor_point.evaluate_at(&tc);
        Some((self.layer_world_matrix_fast(layer_id)?, anchor))
    }

    /// Map a composition-space point into a layer's local coords (mask and
    /// text-path editing). Returns None for missing layers / cycles /
    /// singular matrices.
    pub fn comp_to_layer_local(&self, layer_id: &str, p: Vec2) -> Option<Vec2> {
        self.layer_world_matrix_fast(layer_id)?
            .inverse()
            .map(|inv| inv.transform_point(p))
    }

    /// Toggle lock state on the specified layer.
    pub fn toggle_layer_lock(&mut self, layer_id: &str) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.locked = !layer.locked;
            }
        }
    }

    /// Toggle visibility of the currently selected layer.
    pub fn toggle_selected_layer_visibility(&mut self) {
        self.checkpoint();
        if let Some(id) = self.selected_layer_id.clone() {
            self.toggle_layer_visibility(&id);
        }
    }

    /// Toggle solo state of the currently selected layer.
    pub fn toggle_selected_layer_solo(&mut self) {
        self.checkpoint();
        if let Some(id) = self.selected_layer_id.clone() {
            self.toggle_layer_solo(&id);
        }
    }

    /// Evaluate the current frame of the active composition using the scene graph and layer stack evaluator.
    pub fn evaluate_current_frame(&self) -> Result<EvaluatedStack, String> {
        let graph = SceneGraph::from_project(&self.project, &self.active_comp_id)
            .map_err(|e| format!("Scene graph error: {e:?}"))?;
        let current_tc = self.clock.timecode();
        Ok(self.evaluator.evaluate(&graph, &current_tc))
    }

    /// Set the active editor tool (Move, Hand, Rotate, Pen, Text, ShapeRect, ShapeEllipse).
    pub fn set_tool(&mut self, tool: EditorTool) {
        self.active_tool = tool;
    }

    /// Cycle between Shape tool variants (Rectangle <-> Ellipse).
    pub fn cycle_shape_tool(&mut self) {
        self.active_tool = match self.active_tool {
            EditorTool::ShapeRect => EditorTool::ShapeEllipse,
            _ => EditorTool::ShapeRect,
        };
    }

    /// Add a new Text layer with given text and optional position.
    pub fn add_text_layer(&mut self, text: &str, pos: Option<Vec2>) -> Result<String, String> {
        self.checkpoint();
        let (_comp_w, _comp_h, frame_rate, duration) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            (comp.width, comp.height, comp.frame_rate, comp.duration)
        };

        let layer_id = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_text_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_text_{counter}");
            }
            id
        };

        let in_pt = TimeCode::zero(frame_rate);
        let out_pt = duration;
        let font_size = self.tool_font_size.max(1.0);
        let fill_color = self.tool_text_color;
        let mut layer = Layer::text(
            &layer_id,
            if text.is_empty() { "Text Layer" } else { text },
            text,
            Self::default_font_family(),
            font_size,
            fill_color,
            in_pt,
            out_pt,
        );

        let target_pos = pos.unwrap_or(Vec2::ZERO);
        layer.transform.position.set_value(target_pos);
        // Center the text block on the spawn point: anchor at half of the
        // estimated block size so position (0, 0) lands it in the viewport
        // center like every other new layer (mirrors the viewer estimate).
        let est_w = (text.chars().count().max(1) as f32 * font_size * 0.6 + 40.0).max(100.0);
        let est_h: f32 = (font_size * 1.4 + 20.0).max(40.0);
        layer.transform.anchor_point.set_value(Vec2::new(est_w / 2.0, est_h / 2.0));

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
            .map_err(|e| format!("Failed to add text layer: {e:?}"))?;

        self.selected_layer_id = Some(layer_id.clone());
        Ok(layer_id)
    }

    /// Add a new Rectangle Shape layer with given dimensions and optional position.
    pub fn add_rectangle_shape_layer(
        &mut self,
        width: f32,
        height: f32,
        pos: Option<Vec2>,
    ) -> Result<String, String> {
        self.checkpoint();
        let (_comp_w, _comp_h, frame_rate, duration) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            (comp.width, comp.height, comp.frame_rate, comp.duration)
        };

        let layer_id = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_rect_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_rect_{counter}");
            }
            id
        };

        let in_pt = TimeCode::zero(frame_rate);
        let out_pt = duration;
        let fill = self.tool_shape_fill;
        let mut layer = Layer::shape(
            &layer_id,
            "Rectangle Shape",
            ShapeType::Rectangle {
                width: Property::new("Width", width),
                height: Property::new("Height", height),
                corner_radius: Property::new("Corner Radius", 0.0),
                fill,
            },
            in_pt,
            out_pt,
        );

        let target_pos = pos.unwrap_or(Vec2::ZERO);
        layer.transform.position.set_value(target_pos);
        layer.transform.anchor_point.set_value(Vec2::new(width / 2.0, height / 2.0));

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
            .map_err(|e| format!("Failed to add rectangle layer: {e:?}"))?;

        self.selected_layer_id = Some(layer_id.clone());
        Ok(layer_id)
    }

    /// Add a new Ellipse Shape layer with given radii and optional position.
    pub fn add_ellipse_shape_layer(
        &mut self,
        radius_x: f32,
        radius_y: f32,
        pos: Option<Vec2>,
    ) -> Result<String, String> {
        self.checkpoint();
        let (_comp_w, _comp_h, frame_rate, duration) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            (comp.width, comp.height, comp.frame_rate, comp.duration)
        };

        let layer_id = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_ellipse_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_ellipse_{counter}");
            }
            id
        };

        let in_pt = TimeCode::zero(frame_rate);
        let out_pt = duration;
        let fill = self.tool_shape_fill;
        let mut layer = Layer::shape(
            &layer_id,
            "Ellipse Shape",
            ShapeType::Ellipse {
                radius_x: Property::new("Radius X", radius_x),
                radius_y: Property::new("Radius Y", radius_y),
                fill,
            },
            in_pt,
            out_pt,
        );

        let target_pos = pos.unwrap_or(Vec2::ZERO);
        layer.transform.position.set_value(target_pos);
        layer.transform.anchor_point.set_value(Vec2::new(radius_x, radius_y));

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
            .map_err(|e| format!("Failed to add ellipse layer: {e:?}"))?;

        self.selected_layer_id = Some(layer_id.clone());
        Ok(layer_id)
    }

    /// Pen press at a composition-space point with the topmost picked
    /// layer (if any). Deterministic routing that does not depend on prior
    /// selection or sibling hit-test order:
    /// 1. active mask edit target → append (layer-local),
    /// 2. picked Path shape → select + append (layer-local),
    /// 3. picked Text layer → select + baseline point (layer-local),
    /// 4. otherwise → new Path layer.
    /// Returns the affected layer id.
    pub fn pen_press_at(
        &mut self,
        point: Vec2,
        picked: Option<String>,
    ) -> Result<String, String> {
        self.checkpoint();

        // 1. Mask editing wins while armed and valid.
        if let Some((lid, mid)) = self.active_mask_edit.clone() {
            let mask_opt = self
                .active_composition()
                .and_then(|c| c.get_layer(&lid))
                .and_then(|l| l.get_mask(&mid))
                .cloned();
            if let Some(mask) = mask_opt {
                let local = self.comp_to_layer_local(&lid, point).unwrap_or(point);
                // Close the mask when clicking near the first point (if it has >= 3 points)
                if mask.path.value.points.len() >= 3 {
                    let first_p = mask.path.value.points[0].pos;
                    if (local - first_p).length() <= 24.0 {
                        if let Some(comp) = self.active_composition_mut() {
                            if let Some(layer) = comp.get_layer_mut(&lid) {
                                if let Some(m) = layer.get_mask_mut(&mid) {
                                    m.path.value.close();
                                }
                            }
                        }
                        self.active_mask_edit = None;
                        return Ok(lid);
                    }
                }
                self.append_mask_point(&lid, &mid, local)?;
                return Ok(lid);
            } else {
                self.active_mask_edit = None;
            }
        }

        // 2. If the currently selected layer is an unclosed Path shape or Text layer, continue drawing on it!
        if let Some(sel_id) = self.selected_layer_id.clone() {
            let (is_unclosed_path, is_text) = self
                .active_composition()
                .and_then(|c| c.get_layer(&sel_id))
                .map(|l| {
                    let is_unclosed_path = match &l.source {
                        LayerSource::Shape {
                            shape_type: ShapeType::Path { path_data, .. },
                        } => !Path::from_svg(path_data).closed,
                        _ => false,
                    };
                    let is_text = matches!(&l.source, LayerSource::Text { .. });
                    (is_unclosed_path, is_text)
                })
                .unwrap_or((false, false));

            if is_unclosed_path {
                let local = self.comp_to_layer_local(&sel_id, point);
                let p = local.unwrap_or(point);
                let comp = self
                    .active_composition_mut()
                    .ok_or_else(|| "No active composition".to_string())?;
                let layer = comp
                    .get_layer_mut(&sel_id)
                    .ok_or_else(|| format!("Layer {sel_id} not found"))?;
                if let LayerSource::Shape {
                    shape_type: ShapeType::Path { path_data, .. },
                } = &mut layer.source
                {
                    let path = Path::from_svg(path_data);
                    if path.points.len() >= 3 {
                        let first_p = path.points[0].pos;
                        if (p - first_p).length() <= 24.0 {
                            let mut p_closed = path;
                            p_closed.close();
                            *path_data = p_closed.to_svg();
                            return Ok(sel_id);
                        }
                    }
                    path_data.push_str(&format!(" L {:.1} {:.1}", p.x, p.y));
                    return Ok(sel_id);
                }
            } else if is_text {
                let local = self.comp_to_layer_local(&sel_id, point).unwrap_or(point);
                self.append_text_path_point(&sel_id, local)?;
                return Ok(sel_id);
            }
        }

        // 3. Ignore background solid as a hit-test target for mask creation
        let is_bg = picked.as_deref().map(|id| {
            id == "layer_bg" || self.active_composition().and_then(|c| c.get_layer(id)).map(|l| l.name.to_lowercase().contains("background")).unwrap_or(false)
        }).unwrap_or(false);
        let picked = if is_bg { None } else { picked };

        // 4. Route by picked layer under the cursor
        if let Some(pid) = picked {
            let (is_path_shape, is_text) = self
                .active_composition()
                .and_then(|c| c.get_layer(&pid))
                .map(|l| (
                    matches!(&l.source, LayerSource::Shape { shape_type: ShapeType::Path { .. } }),
                    matches!(&l.source, LayerSource::Text { .. }),
                ))
                .unwrap_or((false, false));

            let local = self.comp_to_layer_local(&pid, point).unwrap_or(point);
            self.select_layer(Some(pid.clone()));

            if is_path_shape {
                let comp = self
                    .active_composition_mut()
                    .ok_or_else(|| "No active composition".to_string())?;
                let layer = comp
                    .get_layer_mut(&pid)
                    .ok_or_else(|| format!("Layer {pid} not found"))?;
                if let LayerSource::Shape {
                    shape_type: ShapeType::Path { path_data, .. },
                } = &mut layer.source
                {
                    let mut path = Path::from_svg(path_data);
                    if path.points.len() >= 3 {
                        let first_p = path.points[0].pos;
                        if (local - first_p).length() <= 24.0 {
                            path.close();
                            *path_data = path.to_svg();
                            return Ok(pid);
                        }
                    }
                    path.line_to(local);
                    *path_data = path.to_svg();
                    return Ok(pid);
                }
            } else if is_text {
                self.append_text_path_point(&pid, local)?;
                return Ok(pid);
            } else {
                // For any other layer (Solid, Image, Video, Shape Rect/Ellipse):
                // PEN TOOL CREATES & EDITS A MASK BY DEFAULT!
                let comp = self
                    .active_composition_mut()
                    .ok_or_else(|| "No active composition".to_string())?;
                let layer = comp
                    .get_layer_mut(&pid)
                    .ok_or_else(|| format!("Layer {pid} not found"))?;

                // If layer already has an unclosed mask, continue drawing on it:
                if let Some(unclosed_m) = layer.masks.iter_mut().rev().find(|m| !m.path.value.closed) {
                    let mid = unclosed_m.id.clone();
                    if unclosed_m.path.value.points.len() >= 3 {
                        let first_p = unclosed_m.path.value.points[0].pos;
                        if (local - first_p).length() <= 24.0 {
                            unclosed_m.path.value.close();
                            self.active_mask_edit = None;
                            return Ok(pid);
                        }
                    }
                    unclosed_m.path.value.line_to(local);
                    self.active_mask_edit = Some((pid.clone(), mid));
                    return Ok(pid);
                }

                // Otherwise create a new mask starting with this point:
                let mid = next_mask_id(layer);
                let name = format!("Mask {}", layer.masks.len() + 1);
                let mut initial_path = Path::new();
                initial_path.line_to(local);
                layer.masks.push(Mask::with_path(&mid, name, initial_path));
                self.active_mask_edit = Some((pid.clone(), mid));
                return Ok(pid);
            }
        }

        // 5. No layer picked under cursor (empty space or background): fresh path layer
        let (frame_rate, duration) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            (comp.frame_rate, comp.duration)
        };

        let layer_id = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("layer_path_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("layer_path_{counter}");
            }
            id
        };

        let in_pt = TimeCode::zero(frame_rate);
        let out_pt = duration;
        let fill = self.tool_shape_fill;
        let layer = Layer::shape(
            &layer_id,
            "Path",
            ShapeType::Path {
                path_data: format!("M {:.1} {:.1}", point.x, point.y),
                fill,
            },
            in_pt,
            out_pt,
        );

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
            .map_err(|e| format!("Failed to add path layer: {e:?}"))?;

        self.selected_layer_id = Some(layer_id.clone());
        Ok(layer_id)
    }

    /// Text-tool press: select the topmost text layer under the cursor for
    /// in-Properties editing, else create a new text layer at the point.
    pub fn text_press_at(
        &mut self,
        point: Vec2,
        picked: Option<String>,
    ) -> Result<String, String> {
        if let Some(pid) = picked {
            let is_text = self
                .active_composition()
                .and_then(|c| c.get_layer(&pid))
                .map(|l| matches!(l.source, LayerSource::Text { .. }))
                .unwrap_or(false);
            if is_text {
                self.checkpoint();
                self.select_layer(Some(pid.clone()));
                return Ok(pid);
            }
        }
        self.add_text_layer("New Text Layer", Some(point))
    }

    /// Add a vector path point using the Pen tool. Delegates to `pen_press_at`
    /// to preserve mask-by-default behavior and point-by-point drawing.
    pub fn add_pen_point(&mut self, point: Vec2) -> Result<String, String> {
        self.pen_press_at(point, None)
    }

    /// Duplicate the specified layer in the active composition.
    pub fn duplicate_layer(&mut self, layer_id: &str) -> Result<String, String> {
        self.checkpoint();
        let comp = self
            .active_composition()
            .ok_or_else(|| "No active composition".to_string())?;

        let layer = comp
            .get_layer(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?
            .clone();

        let new_id = {
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("{layer_id}_copy_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("{layer_id}_copy_{counter}");
            }
            id
        };

        let mut dup_layer = layer;
        dup_layer.id = new_id.clone();
        dup_layer.name = format!("{} Copy", dup_layer.name);

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, dup_layer)
            .map_err(|e| format!("Failed to add duplicated layer: {e:?}"))?;

        self.selected_layer_id = Some(new_id.clone());
        Ok(new_id)
    }

    /// Duplicate the currently selected layer.
    pub fn duplicate_selected_layer(&mut self) -> Result<String, String> {
        self.checkpoint();
        let sel_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.duplicate_layer(&sel_id)
    }

    /// Calculate the center point of a layer's content in local coordinates.
    pub fn layer_content_center(&self, layer: &Layer) -> Vec2 {
        let (w, h) = match &layer.source {
            LayerSource::Solid { width, height, .. } => (*width as f32, *height as f32),
            LayerSource::Image { asset_id } => {
                if let Some(asset) = self.project.get_asset(asset_id) {
                    let (dw, dh) = image::image_dimensions(&asset.path).unwrap_or((1920, 1080));
                    (dw as f32, dh as f32)
                } else {
                    (1920.0, 1080.0)
                }
            }
            LayerSource::Video { .. } => (1920.0, 1080.0),
            LayerSource::Text { font_size, text, box_width, box_height, .. } => {
                let bw = box_width.value;
                let bh = box_height.value;
                if bw > 0.0 && bh > 0.0 {
                    (bw, bh)
                } else {
                    let len = text.value.chars().count().max(1) as f32;
                    let fs = font_size.value;
                    let est_w = if bw > 0.0 { bw } else { (len * fs * 0.6 + 40.0).max(100.0) };
                    let est_h = if bh > 0.0 { bh } else { (fs * 1.4 + 20.0).max(40.0) };
                    (est_w, est_h)
                }
            }
            LayerSource::Shape { shape_type } => match shape_type {
                ShapeType::Rectangle { width, height, .. } => (width.value, height.value),
                ShapeType::Ellipse { radius_x, radius_y, .. } => (radius_x.value * 2.0, radius_y.value * 2.0),
                ShapeType::Path { path_data, .. } => {
                    match project::Path::from_svg(path_data).frame(8.0) {
                        Some((origin, size)) => return origin + size * 0.5,
                        None => (400.0, 300.0),
                    }
                }
            },
            LayerSource::Adjustment => {
                if let Some(comp) = self.active_composition() {
                    (comp.width as f32, comp.height as f32)
                } else {
                    (1920.0, 1080.0)
                }
            }
            _ => (400.0, 300.0),
        };
        Vec2::new(w * 0.5, h * 0.5)
    }

    /// Reset transform properties of the specified layer to defaults:
    /// Position is reset to (0, 0), and Anchor Point is reset to the center of the content.
    pub fn reset_layer_transform(&mut self, layer_id: &str) {
        self.checkpoint();
        let center = if let Some(comp) = self.active_composition() {
            if let Some(layer) = comp.get_layer(layer_id) {
                self.layer_content_center(layer)
            } else {
                Vec2::ZERO
            }
        } else {
            Vec2::ZERO
        };
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.transform = project::Transform::default();
                layer.transform.position.set_value(Vec2::ZERO);
                layer.transform.anchor_point.set_value(center);
                layer.transform.scale.set_value(Vec2::SCALE_100);
                layer.transform.rotation.set_value(0.0);
                layer.opacity.set_value(100.0);
                // Also clear keyframes so the reset takes full effect:
                layer.transform.position.keyframes_mut().clear();
                layer.transform.anchor_point.keyframes_mut().clear();
                layer.transform.scale.keyframes_mut().clear();
                layer.transform.rotation.keyframes_mut().clear();
                layer.opacity.keyframes_mut().clear();
            }
        }
    }

    /// Move an existing keyframe on the given property path to a new timecode (in seconds).
    /// Returns true if a keyframe was found and moved.
    pub fn move_layer_keyframe_time(
        &mut self,
        layer_id: &str,
        prop_path: &str,
        from_time_s: f64,
        to_time_s: f64,
    ) -> bool {
        let fps = match self.active_composition() {
            Some(c) => c.frame_rate,
            None => return false,
        };
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return false,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return false,
        };
        let tol = 0.5 / fps.max(1.0);
        let new_tc = TimeCode::from_seconds(to_time_s.max(0.0), fps);

        match prop_path {
            "transform.anchor_point" => {
                let prop = &mut layer.transform.anchor_point;
                if let Some(idx) = prop.keyframes().iter().position(|k| (k.time_seconds() - from_time_s).abs() <= tol) {
                    let mut kf = prop.keyframes()[idx].clone();
                    kf.time = new_tc;
                    prop.keyframes_mut().remove(idx);
                    prop.add_keyframe(kf);
                    return true;
                }
            }
            "transform.position" => {
                let prop = &mut layer.transform.position;
                if let Some(idx) = prop.keyframes().iter().position(|k| (k.time_seconds() - from_time_s).abs() <= tol) {
                    let mut kf = prop.keyframes()[idx].clone();
                    kf.time = new_tc;
                    prop.keyframes_mut().remove(idx);
                    prop.add_keyframe(kf);
                    return true;
                }
            }
            "transform.scale" => {
                let prop = &mut layer.transform.scale;
                if let Some(idx) = prop.keyframes().iter().position(|k| (k.time_seconds() - from_time_s).abs() <= tol) {
                    let mut kf = prop.keyframes()[idx].clone();
                    kf.time = new_tc;
                    prop.keyframes_mut().remove(idx);
                    prop.add_keyframe(kf);
                    return true;
                }
            }
            "transform.rotation" => {
                let prop = &mut layer.transform.rotation;
                if let Some(idx) = prop.keyframes().iter().position(|k| (k.time_seconds() - from_time_s).abs() <= tol) {
                    let mut kf = prop.keyframes()[idx].clone();
                    kf.time = new_tc;
                    prop.keyframes_mut().remove(idx);
                    prop.add_keyframe(kf);
                    return true;
                }
            }
            "opacity" => {
                let prop = &mut layer.opacity;
                if let Some(idx) = prop.keyframes().iter().position(|k| (k.time_seconds() - from_time_s).abs() <= tol) {
                    let mut kf = prop.keyframes()[idx].clone();
                    kf.time = new_tc;
                    prop.keyframes_mut().remove(idx);
                    prop.add_keyframe(kf);
                    return true;
                }
            }
            _ => {
                let mut moved = false;
                let _ = with_graph_scalar(layer, prop_path, |p| {
                    if let Some(idx) = p.keyframes().iter().position(|k| (k.time_seconds() - from_time_s).abs() <= tol) {
                        let mut kf = p.keyframes()[idx].clone();
                        kf.time = new_tc;
                        p.keyframes_mut().remove(idx);
                        p.add_keyframe(kf);
                        moved = true;
                    }
                });
                if moved {
                    return true;
                }
            }
        }
        false
    }

    /// Duplicate an effect on the specified layer.
    pub fn duplicate_layer_effect(&mut self, layer_id: &str, effect_id: &str) -> Result<String, String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        let effect = layer
            .get_effect(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found"))?
            .clone();

        let mut counter = layer.effects.len() + 1;
        let mut new_id = format!("{effect_id}_copy_{counter}");
        while layer.get_effect(&new_id).is_some() {
            counter += 1;
            new_id = format!("{effect_id}_copy_{counter}");
        }

        let mut dup_effect = effect;
        dup_effect.id = new_id.clone();
        dup_effect.name = format!("{} Copy", dup_effect.name);
        layer.add_effect(dup_effect);

        Ok(new_id)
    }

    /// Add keyframes to all spatial and opacity properties of the specified layer at the current playhead time.
    pub fn add_keyframe_to_all_transforms_at_playhead(&mut self, layer_id: &str) {
        self.checkpoint();
        let tc = self.clock.timecode();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                // Position
                let pos = layer.transform.position.evaluate_at(&tc);
                layer.transform.position.set_animated(true);
                layer.transform.position.add_keyframe(Keyframe::new(tc, pos));

                // Scale
                let sc = layer.transform.scale.evaluate_at(&tc);
                layer.transform.scale.set_animated(true);
                layer.transform.scale.add_keyframe(Keyframe::new(tc, sc));

                // Rotation
                let rot = layer.transform.rotation.evaluate_at(&tc);
                layer.transform.rotation.set_animated(true);
                layer.transform.rotation.add_keyframe(Keyframe::new(tc, rot));

                // Anchor Point
                let anc = layer.transform.anchor_point.evaluate_at(&tc);
                layer.transform.anchor_point.set_animated(true);
                layer.transform.anchor_point.add_keyframe(Keyframe::new(tc, anc));

                // Opacity
                let op = layer.opacity.evaluate_at(&tc);
                layer.opacity.set_animated(true);
                layer.opacity.add_keyframe(Keyframe::new(tc, op));
            }
        }
    }

    /// Set the solid color on a Solid layer.
    pub fn set_layer_solid_color(&mut self, layer_id: &str, color: Color) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Solid { color: c, .. } => {
                *c = color;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Solid layer")),
        }
    }

    /// Set dimensions on a Solid layer.
    pub fn set_layer_solid_dimensions(
        &mut self,
        layer_id: &str,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Solid { width: w, height: h, .. } => {
                *w = width.max(1);
                *h = height.max(1);
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Solid layer")),
        }
    }

    /// Nudge RGB components of a Solid layer's color.
    pub fn nudge_layer_solid_color(
        &mut self,
        layer_id: &str,
        dr: f32,
        dg: f32,
        db: f32,
    ) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Solid { color: c, .. } => {
                c.r = (c.r + dr).clamp(0.0, 1.0);
                c.g = (c.g + dg).clamp(0.0, 1.0);
                c.b = (c.b + db).clamp(0.0, 1.0);
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Solid layer")),
        }
    }

    /// Set text content on a Text layer.
    pub fn set_layer_text(&mut self, layer_id: &str, text: &str) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Text { text: t, .. } => {
                let value = text.to_string();
                t.set_value(value.clone());
                if t.is_animated() {
                    t.add_keyframe(Keyframe::new(current_tc, value));
                }
                layer.name = if text.is_empty() {
                    "Text Layer".to_string()
                } else if text.len() > 24 {
                    format!("{}...", &text[..24])
                } else {
                    text.to_string()
                };
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set font size on a Text layer.
    pub fn set_layer_font_size(&mut self, layer_id: &str, size: f32) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Text { font_size, .. } => {
                let value = size.max(4.0);
                font_size.set_value(value);
                if font_size.is_animated() {
                    font_size.add_keyframe(Keyframe::new(current_tc, value));
                }
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Nudge font size on a Text layer.
    pub fn nudge_layer_font_size(&mut self, layer_id: &str, delta: f32) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Text { font_size, .. } => {
                let current = font_size.value;
                font_size.set_value((current + delta).max(4.0));
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set fill color on a Text layer.
    pub fn set_layer_text_color(&mut self, layer_id: &str, color: Color) -> Result<(), String> {
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Text { fill_color, .. } => {
                fill_color.set_value(color);
                if fill_color.is_animated() {
                    fill_color.add_keyframe(Keyframe::new(current_tc, color));
                }
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set the font family on a Text layer.
    pub fn set_layer_font_family(&mut self, layer_id: &str, family: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { font_family, .. } => {
                *font_family = family.trim().to_string();
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set font weight (100..900) on a Text layer.
    pub fn set_layer_font_weight(&mut self, layer_id: &str, weight: u16) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { weight: w, .. } => {
                *w = weight.clamp(100, 900);
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Toggle faux italic on a Text layer.
    pub fn toggle_layer_italic(&mut self, layer_id: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { italic, .. } => {
                *italic = !*italic;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Toggle all-caps on a Text layer.
    pub fn toggle_layer_caps(&mut self, layer_id: &str) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { all_caps, .. } => {
                *all_caps = !*all_caps;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set faux italic on a Text layer.
    pub fn set_layer_italic(&mut self, layer_id: &str, val: bool) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { italic, .. } => {
                *italic = val;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set all-caps on a Text layer.
    pub fn set_layer_caps(&mut self, layer_id: &str, val: bool) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { all_caps, .. } => {
                *all_caps = val;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set paragraph alignment on a Text layer.
    pub fn set_layer_text_align(&mut self, layer_id: &str, align: project::TextAlign) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { align: a, .. } => {
                *a = align;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set a scalar text property (tracking / leading / stroke_width /
    /// baseline_shift / box_width / box_height) on a Text layer. Keyframe-aware.
    pub fn set_layer_text_scalar(&mut self, layer_id: &str, field: &str, v: f32) -> Result<(), String> {
        self.checkpoint();
        if !v.is_finite() {
            return Err("Non-finite value".to_string());
        }
        let current_tc = self.clock.timecode();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { tracking, leading, stroke_width, baseline_shift, box_width, box_height, .. } => {
                let prop = match field {
                    "tracking" => tracking,
                    "leading" => leading,
                    "stroke_width" => stroke_width,
                    "baseline_shift" => baseline_shift,
                    "box_width" => box_width,
                    "box_height" => box_height,
                    _ => return Err(format!("Unknown text field {field}")),
                };
                let value = match field {
                    "tracking" => v.clamp(-50.0, 200.0),
                    "leading" => v.max(0.0),
                    "stroke_width" => v.clamp(0.0, 50.0),
                    "baseline_shift" => v.clamp(-500.0, 500.0),
                    _ => v.max(0.0),
                };
                prop.set_value(value);
                if prop.is_animated() {
                    prop.add_keyframe(Keyframe::new(current_tc, value));
                }
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set faux underline on a Text layer.
    pub fn set_layer_underline(&mut self, layer_id: &str, val: bool) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { underline, .. } => {
                *underline = val;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set small-caps mode on a Text layer.
    pub fn set_layer_small_caps(&mut self, layer_id: &str, val: bool) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { small_caps, .. } => {
                *small_caps = val;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set superscript mode on a Text layer.
    pub fn set_layer_superscript(&mut self, layer_id: &str, val: bool) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { superscript, .. } => {
                *superscript = val;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set subscript mode on a Text layer.
    pub fn set_layer_subscript(&mut self, layer_id: &str, val: bool) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { subscript, .. } => {
                *subscript = val;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set stroke position on a Text layer ("center", "inside", "outside").
    pub fn set_layer_stroke_position(&mut self, layer_id: &str, pos: impl Into<String>) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { stroke_position, .. } => {
                *stroke_position = pos.into();
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set paint order on a Text layer ("fill_over_stroke", "stroke_over_fill").
    pub fn set_layer_paint_order(&mut self, layer_id: &str, order: impl Into<String>) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { paint_order, .. } => {
                *paint_order = order.into();
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set vertical alignment on a Text layer ("top", "middle", "bottom").
    pub fn set_layer_vertical_align(&mut self, layer_id: &str, align: impl Into<String>) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { vertical_align, .. } => {
                *vertical_align = align.into();
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set stroke color on a Text layer.
    pub fn set_layer_stroke_color(&mut self, layer_id: &str, color: Color) -> Result<(), String> {
        self.checkpoint();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { stroke_color, .. } => {
                *stroke_color = color;
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Text layer")),
        }
    }

    /// Set dimensions and corner radius on a Rectangle shape layer.
    pub fn set_layer_rect_dimensions(
        &mut self,
        layer_id: &str,
        width: f32,
        height: f32,
        corner_radius: f32,
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Rectangle { width: w, height: h, corner_radius: cr, .. },
            } => {
                w.set_value(width.max(1.0));
                h.set_value(height.max(1.0));
                cr.set_value(corner_radius.max(0.0));
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Rectangle shape layer")),
        }
    }

    /// Nudge dimensions and corner radius on a Rectangle shape layer.
    pub fn nudge_layer_rect_dimensions(
        &mut self,
        layer_id: &str,
        dw: f32,
        dh: f32,
        dcr: f32,
    ) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Rectangle { width: w, height: h, corner_radius: cr, .. },
            } => {
                let cur_w = w.value;
                let cur_h = h.value;
                let cur_cr = cr.value;
                w.set_value((cur_w + dw).max(1.0));
                h.set_value((cur_h + dh).max(1.0));
                cr.set_value((cur_cr + dcr).max(0.0));
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not a Rectangle shape layer")),
        }
    }

    /// Set radii on an Ellipse shape layer.
    pub fn set_layer_ellipse_radii(
        &mut self,
        layer_id: &str,
        radius_x: f32,
        radius_y: f32,
    ) -> Result<(), String> {
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Ellipse { radius_x: rx, radius_y: ry, .. },
            } => {
                rx.set_value(radius_x.max(1.0));
                ry.set_value(radius_y.max(1.0));
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not an Ellipse shape layer")),
        }
    }

    /// Nudge radii on an Ellipse shape layer.
    pub fn nudge_layer_ellipse_radii(
        &mut self,
        layer_id: &str,
        drx: f32,
        dry: f32,
    ) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Ellipse { radius_x: rx, radius_y: ry, .. },
            } => {
                let cur_rx = rx.value;
                let cur_ry = ry.value;
                rx.set_value((cur_rx + drx).max(1.0));
                ry.set_value((cur_ry + dry).max(1.0));
                Ok(())
            }
            _ => Err(format!("Layer {layer_id} is not an Ellipse shape layer")),
        }
    }

    /// Trim layer In-Point to `in_point`.
    pub fn trim_layer_in_point(&mut self, layer_id: &str, in_point: TimeCode) -> Result<(), String> {
        let fps = match self.active_composition() {
            Some(c) => c.frame_rate,
            None => return Err("No active composition".to_string()),
        };
        let comp = self.active_composition_mut().unwrap();
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        let one_frame = 1i64;
        let max_in_frames = layer.out_point.frames() - one_frame;
        let clamped_frames = in_point.frames().max(0).min(max_in_frames);
        layer.in_point = TimeCode::from_frames(clamped_frames, fps);
        Ok(())
    }

    /// Trim layer Out-Point to `out_point`.
    pub fn trim_layer_out_point(&mut self, layer_id: &str, out_point: TimeCode) -> Result<(), String> {
        let (fps, comp_dur_frames) = match self.active_composition() {
            Some(c) => (c.frame_rate, c.duration.frames()),
            None => return Err("No active composition".to_string()),
        };
        let comp = self.active_composition_mut().unwrap();
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        let min_out_frames = layer.in_point.frames() + 1;
        let clamped_frames = out_point.frames().max(min_out_frames).min(comp_dur_frames);
        layer.out_point = TimeCode::from_frames(clamped_frames, fps);
        Ok(())
    }

    /// Nudge layer In-Point by `delta_frames`.
    pub fn nudge_layer_in_point(&mut self, layer_id: &str, delta_frames: i64) -> Result<(), String> {
        // Per-tick trim nudges share the trim-start checkpoint (no spam).
        let (current_in, fps) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let layer = comp
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?;
            (layer.in_point, comp.frame_rate)
        };
        let new_tc = TimeCode::from_frames(current_in.frames() + delta_frames, fps);
        self.trim_layer_in_point(layer_id, new_tc)
    }

    /// Nudge layer Out-Point by `delta_frames`.
    pub fn nudge_layer_out_point(&mut self, layer_id: &str, delta_frames: i64) -> Result<(), String> {
        // Per-tick trim nudges share the trim-start checkpoint (no spam).
        let (current_out, fps) = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let layer = comp
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?;
            (layer.out_point, comp.frame_rate)
        };
        let new_tc = TimeCode::from_frames(current_out.frames() + delta_frames, fps);
        self.trim_layer_out_point(layer_id, new_tc)
    }

    /// Move/slip the entire layer strip forward or backward in time by `delta_frames`,
    /// keeping its duration constant.
    pub fn slip_layer(&mut self, layer_id: &str, delta_frames: i64) -> Result<(), String> {
        let (fps, comp_dur_frames) = match self.active_composition() {
            Some(c) => (c.frame_rate, c.duration.frames()),
            None => return Err("No active composition".to_string()),
        };
        let comp = self.active_composition_mut().unwrap();
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        let duration_frames = layer.out_point.frames() - layer.in_point.frames();
        let mut new_in = layer.in_point.frames() + delta_frames;
        let mut new_out = layer.out_point.frames() + delta_frames;

        if new_in < 0 {
            new_in = 0;
            new_out = duration_frames;
        }
        if new_out > comp_dur_frames {
            new_out = comp_dur_frames;
            new_in = (new_out - duration_frames).max(0);
        }

        layer.in_point = TimeCode::from_frames(new_in, fps);
        layer.out_point = TimeCode::from_frames(new_out, fps);
        Ok(())
    }

    /// After Effects shortcut `[`: Trim selected layer In-Point to the current playhead position.
    pub fn trim_selected_layer_in_to_playhead(&mut self) -> Result<(), String> {
        let sel_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        let current_tc = self.clock.timecode();
        self.trim_layer_in_point(&sel_id, current_tc)
    }

    /// After Effects shortcut `]`: Trim selected layer Out-Point to the current playhead position.
    pub fn trim_selected_layer_out_to_playhead(&mut self) -> Result<(), String> {
        let sel_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        let current_tc = self.clock.timecode();
        self.trim_layer_out_point(&sel_id, current_tc)
    }

    /// Reset layer duration to span the full composition duration.
    pub fn reset_layer_duration_to_comp(&mut self, layer_id: &str) -> Result<(), String> {
        let (fps, comp_dur) = match self.active_composition() {
            Some(c) => (c.frame_rate, c.duration),
            None => return Err("No active composition".to_string()),
        };
        let comp = self.active_composition_mut().unwrap();
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        layer.in_point = TimeCode::zero(fps);
        layer.out_point = comp_dur;
        Ok(())
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}
