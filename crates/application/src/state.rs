use compositor::{AffineTransform2D, EvaluatedStack, LayerStackEvaluator, SceneGraph};
use gpui_kit::component::input::InputState;
use gpui_kit::{Entity, Subscription};
use project::{
    Asset, AutoTraceOptions, BlendMode, Color, Composition, Effect, EffectType, FillGradient,
    GradientType, Keyframe, KeyframeInterpolation, KeyframeTangent, Layer, LayerSource,
    Mask, MaskShapeKind, Path, PathPointKind, PlaybackClock, Project, Property, ShapeType,
    TimeCode, TraceRange, TrackMatteMode, Vec2, WarpPin,
};
use std::path::{Path as StdPath, PathBuf};
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

/// Pen path paint mode for newly drawn paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathDrawMode {
    /// Fill plus outline (today's look).
    #[default]
    Both,
    /// Fill only (no stroke).
    Fill,
    /// Outline only (transparent fill).
    Stroke,
}

/// Viewport preview resolution. Full rasterizes at the capped box size (100% native quality);
/// Half renders at 1/2 resolution; Quarter at 1/4 resolution; Auto adapts during playback
/// on heavy compositions while maintaining full quality when paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewQuality {
    #[default]
    Full,
    Half,
    Quarter,
    Auto,
}

impl PreviewQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Half => "Half",
            Self::Quarter => "Quarter",
            Self::Auto => "Auto",
        }
    }

    /// Resolution divisor for raster output sizes.
    pub const fn divisor(self) -> u32 {
        match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
            Self::Auto => 1,
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
    /// Pen path paint mode + stroke width for newly drawn paths.
    pub tool_path_mode: PathDrawMode,
    pub tool_path_width: f32,
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
    /// Last timestamp 'M' was pressed in timeline for M / MM double-tap detection.
    pub last_m_press_time: Option<std::time::Instant>,
    /// True when MM shortcut expanded all mask properties.
    pub timeline_masks_reveal_all: bool,
    /// True when M shortcut toggled mask path visibility.
    pub timeline_masks_reveal_path: bool,
    /// Copied property link `(layer_id, prop_path)` for Copy Link / Paste Link.
    pub copied_property_link: Option<(String, String)>,
    /// Brief highlight toast when property link is copied or pasted.
    pub property_link_toast: Option<String>,
    /// Copied layer snapshot for Copy / Paste Layer (context menus).
    pub layer_clipboard: Option<Layer>,
    /// Copied asset id for Copy / Paste Asset (project panel menus).
    pub asset_clipboard: Option<String>,
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
    Hold,
    SineIn, SineOut, SineInOut,
    QuadIn, QuadOut, QuadInOut,
    CubicIn, CubicOut, CubicInOut,
    QuartIn, QuartOut, QuartInOut,
    QuintIn, QuintOut, QuintInOut,
    ExpoIn, ExpoOut, ExpoInOut,
    CircIn, CircOut, CircInOut,
    BackIn, BackOut, BackInOut,
    EasyEase,
    AppleDecel,
    Punchy,
}

impl EasingPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Hold => "Hold",
            Self::SineIn => "Sine In",
            Self::SineOut => "Sine Out",
            Self::SineInOut => "Sine In Out",
            Self::QuadIn => "Quad In",
            Self::QuadOut => "Quad Out",
            Self::QuadInOut => "Quad In Out",
            Self::CubicIn => "Cubic In",
            Self::CubicOut => "Cubic Out",
            Self::CubicInOut => "Cubic In Out",
            Self::QuartIn => "Quart In",
            Self::QuartOut => "Quart Out",
            Self::QuartInOut => "Quart In Out",
            Self::QuintIn => "Quint In",
            Self::QuintOut => "Quint Out",
            Self::QuintInOut => "Quint In Out",
            Self::ExpoIn => "Expo In",
            Self::ExpoOut => "Expo Out",
            Self::ExpoInOut => "Expo In Out",
            Self::CircIn => "Circ In",
            Self::CircOut => "Circ Out",
            Self::CircInOut => "Circ In Out",
            Self::BackIn => "Back In",
            Self::BackOut => "Back Out",
            Self::BackInOut => "Back In Out",
            Self::EasyEase => "Easy Ease",
            Self::AppleDecel => "Apple Decel",
            Self::Punchy => "Punchy",
        }
    }
}

fn apply_easing_to_prop<T: project::Interpolate>(prop: &mut project::Property<T>, easing: EasingPreset) {
    for kf in prop.keyframes_mut() {
        match easing {
            EasingPreset::Linear => {
                kf.interpolation = project::KeyframeInterpolation::Linear;
                kf.out_tangent = Some(project::KeyframeTangent::new(1.0/3.0, 1.0/3.0));
                kf.in_tangent = Some(project::KeyframeTangent::new(2.0/3.0, 2.0/3.0));
            }
            EasingPreset::Hold => {
                kf.interpolation = project::KeyframeInterpolation::Hold;
                kf.in_tangent = None;
                kf.out_tangent = None;
            }
            _ => {
                kf.interpolation = project::KeyframeInterpolation::Bezier;
                let (out_p, in_p) = match easing {
                    EasingPreset::SineIn => ((0.47, 0.0), (0.745, 0.715)),
                    EasingPreset::SineOut => ((0.39, 0.575), (0.565, 1.0)),
                    EasingPreset::SineInOut => ((0.445, 0.05), (0.55, 0.95)),
                    EasingPreset::QuadIn => ((0.55, 0.085), (0.68, 0.53)),
                    EasingPreset::QuadOut => ((0.25, 0.46), (0.45, 0.94)),
                    EasingPreset::QuadInOut => ((0.455, 0.03), (0.515, 0.955)),
                    EasingPreset::CubicIn => ((0.55, 0.055), (0.675, 0.19)),
                    EasingPreset::CubicOut => ((0.215, 0.61), (0.355, 1.0)),
                    EasingPreset::CubicInOut => ((0.645, 0.045), (0.355, 1.0)),
                    EasingPreset::QuartIn => ((0.895, 0.03), (0.685, 0.22)),
                    EasingPreset::QuartOut => ((0.165, 0.84), (0.44, 1.0)),
                    EasingPreset::QuartInOut => ((0.77, 0.0), (0.175, 1.0)),
                    EasingPreset::QuintIn => ((0.755, 0.05), (0.855, 0.06)),
                    EasingPreset::QuintOut => ((0.23, 1.0), (0.32, 0.0)),
                    EasingPreset::QuintInOut => ((0.86, 0.0), (0.07, 1.0)),
                    EasingPreset::ExpoIn => ((0.95, 0.05), (0.795, 0.035)),
                    EasingPreset::ExpoOut => ((0.19, 1.0), (0.22, 1.0)),
                    EasingPreset::ExpoInOut => ((1.0, 0.0), (0.0, 1.0)),
                    EasingPreset::CircIn => ((0.6, 0.04), (0.98, 0.335)),
                    EasingPreset::CircOut => ((0.075, 0.82), (0.165, 1.0)),
                    EasingPreset::CircInOut => ((0.785, 0.135), (0.15, 0.86)),
                    EasingPreset::BackIn => ((0.6, -0.28), (0.735, 0.045)),
                    EasingPreset::BackOut => ((0.175, 0.885), (0.32, 1.275)),
                    EasingPreset::BackInOut => ((0.68, -0.55), (0.265, 1.55)),
                    EasingPreset::EasyEase => ((0.42, 0.0), (0.58, 1.0)),
                    EasingPreset::AppleDecel => ((0.0, 0.0), (0.2, 1.0)),
                    EasingPreset::Punchy => ((0.8, 0.0), (0.2, 1.0)),
                    _ => ((0.0, 0.0), (1.0, 1.0)) // Unreachable
                };
                kf.out_tangent = Some(project::KeyframeTangent::new(out_p.0, out_p.1));
                kf.in_tangent = Some(project::KeyframeTangent::new(in_p.0, in_p.1));
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationPreset {
    FadeIn, SlideInUp, SlideInDown, SlideInLeft, SlideInRight,
    PopIn, DropIn, WhipIn,
    FadeOut, SlideOutUp, SlideOutDown, SlideOutLeft, SlideOutRight,
    ShrinkOut, DropOut,
    Pulse, Shake, Float, Spin, Flash,
}

impl AnimationPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::FadeIn => "Fade In",
            Self::SlideInUp => "Slide In Up",
            Self::SlideInDown => "Slide In Down",
            Self::SlideInLeft => "Slide In Left",
            Self::SlideInRight => "Slide In Right",
            Self::PopIn => "Pop In",
            Self::DropIn => "Drop In",
            Self::WhipIn => "Whip In",
            Self::FadeOut => "Fade Out",
            Self::SlideOutUp => "Slide Out Up",
            Self::SlideOutDown => "Slide Out Down",
            Self::SlideOutLeft => "Slide Out Left",
            Self::SlideOutRight => "Slide Out Right",
            Self::ShrinkOut => "Shrink Out",
            Self::DropOut => "Drop Out",
            Self::Pulse => "Pulse",
            Self::Shake => "Shake",
            Self::Float => "Float",
            Self::Spin => "Spin",
            Self::Flash => "Flash",
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
            let (eid, pname) = rest.split_once(':')?;
            
            
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
        // Every scalar the Properties panel and timeline can animate, from
        // the same widget declarations both views enumerate — new effect
        // params appear here with no per-effect list to keep in sync.
        // (Color/bool params stay timeline-only: the graph plots f32 curves.)
        for decl in eff.declarations() {
            if !decl.is_scalar() {
                continue;
            }
            if eff.get_param_property(&decl.field).is_none() {
                continue;
            }
            out.push((
                format!("effect:{}:{}", eff.id, decl.field),
                format!("{} · {}", eff.name, decl.label),
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

/// Compact per-key easing for the spline bottom bar (AE key interp row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEase {
    Linear,
    EaseIn,
    EaseOut,
    Hold,
}

impl KeyEase {
    /// Slug for stable test ids (`graph_ease_{slug}_…`).
    pub const fn slug(self) -> &'static str {
        match self {
            KeyEase::Linear => "linear",
            KeyEase::EaseIn => "ease_in",
            KeyEase::EaseOut => "ease_out",
            KeyEase::Hold => "hold",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            KeyEase::Linear => "Linear",
            KeyEase::EaseIn => "Ease In",
            KeyEase::EaseOut => "Ease Out",
            KeyEase::Hold => "Hold",
        }
    }
}

/// Find the keyframe nearest `at_s` (within `tol`) for mutation.
fn find_keyframe_mut<T>(
    kfs: &mut [project::Keyframe<T>],
    at_s: f64,
    tol: f64,
) -> Option<&mut project::Keyframe<T>> {
    kfs.iter_mut().find(|k| (k.time_seconds() - at_s).abs() <= tol)
}

/// Route a graph path to its keyframe track and run `edit` on the key
/// nearest `at_s`. One routing table for the cycle/set paths (same
/// coverage as the old `cycle_graph_key_interp` match).
fn edit_graph_keyframe(
    layer: &mut Layer,
    path: &str,
    at_s: f64,
    tol: f64,
    mut edit: impl FnMut(
        &mut KeyframeInterpolation,
        &mut Option<KeyframeTangent>,
        &mut Option<KeyframeTangent>,
    ),
) -> bool {
    let (base, _) = match split_graph_path(path) {
        Some(v) => v,
        None => return false,
    };
    match base {
        "transform.anchor_point" | "transform.position" | "transform.scale" => {
            let prop = match base {
                "transform.anchor_point" => &mut layer.transform.anchor_point,
                "transform.position" => &mut layer.transform.position,
                _ => &mut layer.transform.scale,
            };
            match find_keyframe_mut(prop.keyframes_mut(), at_s, tol) {
                Some(kf) => {
                    edit(&mut kf.interpolation, &mut kf.in_tangent, &mut kf.out_tangent);
                    true
                }
                None => false,
            }
        }
        _ => with_graph_scalar(layer, path, |p| {
            match find_keyframe_mut(p.keyframes_mut(), at_s, tol) {
                Some(kf) => {
                    edit(&mut kf.interpolation, &mut kf.in_tangent, &mut kf.out_tangent);
                    true
                }
                None => false,
            }
        })
        .unwrap_or(false),
    }
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
        let mut state = Self::blank();
        state.seed_demo_layers();
        state
    }

    /// Create a completely blank EditorState with a clean composition and no layers.
    pub fn blank() -> Self {
        let mut project = Project::new("proj_default", "Motion Studio Project");
        let comp = Composition::hd_1080p_30fps("comp_main", "Untitled Composition", 5.0);
        let clock = PlaybackClock::from_composition(&comp);
        let active_comp_id = "comp_main".to_string();
        let selected_layer_id = None;
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
            tool_path_mode: PathDrawMode::Both,
            tool_path_width: 2.0,
            tool_rotate_step: 15.0,
            preview_fast: false,
            preview_quality: PreviewQuality::Auto,
            project_path: None,
            recent_projects: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            spline_editor_open: false,
            spline_prop_path: "transform.position".to_string(),
            active_mask_edit: None,
            last_m_press_time: None,
            timeline_masks_reveal_all: false,
            timeline_masks_reveal_path: false,
            copied_property_link: None,
            property_link_toast: None,
            layer_clipboard: None,
            asset_clipboard: None,
        }
    }

    /// Seed the active composition with the three animated demo layers used in tests
    /// (Background Solid, Animated Box, Accent Badge). Clears any existing layers first.
    /// Selects "layer_accent" as the active layer after seeding.
    pub fn seed_demo_layers(&mut self) {
        let fps = 30.0;
        let tc0 = TimeCode::from_frames(0, fps);
        let tc150 = TimeCode::from_frames(150, fps);

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
        accent.transform.rotation.add_keyframe(Keyframe::linear(TimeCode::from_frames(0, fps), 0.0));
        accent.transform.rotation.add_keyframe(Keyframe::linear(TimeCode::from_frames(120, fps), 360.0));

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

        if let Some(comp) = self.active_composition_mut() {
            comp.name = "Main Composition".to_string();
            comp.layers.clear();
            comp.add_layer(accent).unwrap();
            comp.add_layer(title_card).unwrap();
            // Background solid last = bottom of the stack (After Effects convention)
            comp.add_layer(bg_solid).unwrap();
        }
        self.selected_layer_id = Some("layer_accent".to_string());
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
    /// Accepts plain numbers, arithmetic expressions ("1920/2", "100+50"),
    /// relative modifiers ("+=10", "-=5", "*2", "/2"), and unit suffixes ("px", "%", "deg").
    /// Returns true when a value was applied.
    pub fn commit_typed_value(&mut self, text: &str) -> bool {
        let Some(prop) = self.value_edit_key.clone() else {
            return false;
        };
        let current = if let Some(rest) = prop.strip_prefix("tl:") {
            if let Some((lid, key)) = rest.split_once(':') {
                self.timeline_current_value(lid, key)
            } else {
                None
            }
        } else {
            self.scrub_current_value(&prop)
        };
        let Some(v) = parse_numeric_expression(text, current) else {
            return false;
        };
        self.checkpoint();
        if let Some(rest) = prop.strip_prefix("tl:") {
            if let Some((lid, key)) = rest.split_once(':') {
                return self.set_timeline_value(lid, key, v);
            }
            return false;
        }
        self.set_scrub_value(&prop, v)
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

    /// Clear all remembered recent project paths.
    pub fn clear_recent_projects(&mut self) {
        self.recent_projects.clear();
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

    /// Effective raster divisor right now: interactive mouse dragging gestures (e.g. gizmo/slider scrub)
    /// use a fast 2x proxy resolution for ultra-responsive manipulation; timeline playback in
    /// Auto drops to half res on non-trivial comps (full quality returns on pause); otherwise
    /// the user's `preview_quality` preference wins (Full = 1, crisp and native).
    pub fn preview_divisor(&self) -> u32 {
        if self.preview_fast {
            2
        } else {
            match self.preview_quality {
                PreviewQuality::Full => 1,
                PreviewQuality::Half => 2,
                PreviewQuality::Quarter => 4,
                PreviewQuality::Auto => {
                    if self.is_playing {
                        let layer_count = self.active_composition().map(|c| c.layers.len()).unwrap_or(1);
                        if layer_count > 2 { 2 } else { 1 }
                    } else {
                        1
                    }
                }
            }
        }
    }

    /// Synchronize the playback clock to the current active composition's
    /// frame rate and duration, preserving the current playback time position.
    pub fn sync_clock_to_active_composition(&mut self) {
        if let Some(comp) = self.active_composition() {
            let pos_seconds = self.clock.position_seconds();
            let was_playing = self.is_playing;
            self.clock = PlaybackClock::from_composition(comp);
            self.clock.seek_seconds(pos_seconds);
            if was_playing {
                self.clock.play();
            }
        }
    }

    /// Seek to continuous `target_time` in seconds, returning true only if the quantized
    /// visual frame changed. Avoids redundant composition re-evaluations during ruler scrubbing.
    pub fn scrub_frame_quantized(&mut self, target_time: f64) -> bool {
        let fps = self.active_composition().map(|c| c.frame_rate).unwrap_or(30.0);
        let target_frame = (target_time * fps).round() as i64;
        let prev_frame = self.clock.current_frame();
        self.seek(target_time);
        target_frame != prev_frame
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
        self.sync_clock_to_active_composition();
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
        let (id, in_pt, out_pt, comp_w, comp_h) = {
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

        let effective_w = if width == 0 { comp_w } else { width };
        let effective_h = if height == 0 { comp_h } else { height };

        let mut layer = Layer::solid(&id, name, color, effective_w, effective_h, in_pt, out_pt);
        layer.transform.position.set_value(Vec2::ZERO);
        layer.transform.anchor_point.set_value(Vec2::new(
            (effective_w / 2) as f32,
            (effective_h / 2) as f32,
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

    /// Import multiple media files at once, registering them in project assets
    /// and adding them to the active composition.
    pub fn import_multiple_media_files(&mut self, paths: &[PathBuf]) -> Result<usize, String> {
        if paths.is_empty() {
            return Ok(0);
        }
        self.checkpoint();
        let mut count = 0;
        for path in paths {
            if self.import_media_file(path.clone()).is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Import all supported media files in a directory.
    pub fn import_media_folder(&mut self, folder: &StdPath) -> Result<usize, String> {
        let entries = std::fs::read_dir(folder).map_err(|e| e.to_string())?;
        let supported = ["png", "jpg", "jpeg", "mp4", "mov", "webm", "wav", "mp3", "ogg"];
        let mut paths = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if supported.iter().any(|&s| s.eq_ignore_ascii_case(ext)) {
                        paths.push(path);
                    }
                }
            }
        }
        paths.sort();
        self.import_multiple_media_files(&paths)
    }

    /// Rasterize one frame of `cid` out of a `Project` into raw RGBA8 bytes at
    /// the requested output size.
    ///
    /// This is the per-frame render callback the export crate drives. It is a
    /// free-standing function taking only a `Project`, so it runs on a worker
    /// thread with no `EditorState`, runtime, or window involved.
    pub fn render_export_frame(
        proj: &Project,
        cid: &str,
        fr: i64,
        w: u32,
        h: u32,
    ) -> Result<Vec<u8>, String> {
        let comp = proj.get_composition(cid).ok_or_else(|| "missing".to_string())?;
        let fps = if comp.frame_rate > 0.0 { comp.frame_rate } else { 30.0 };
        let time = TimeCode::from_frames(fr, fps);
        let graph = SceneGraph::from_project(proj, cid).map_err(|e| format!("{e:?}"))?;
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator
            .evaluate_with_project(&graph, proj, &time)
            .map_err(|e| format!("{e:?}"))?;
        let mut assets = std::collections::HashMap::new();
        let buffer = crate::raster::comp::rasterize_comp(
            &stack,
            comp.width as f32,
            comp.height as f32,
            comp.background_color,
            w,
            h,
            fr as f32 / (fps as f32).max(1.0),
            fr,
            false,
            comp.duration.seconds() as f32,
            &mut assets,
        );
        Ok(buffer.to_rgba8())
    }

    /// Export the current frame of the active composition as a PNG image to the given path.
    pub fn export_frame_as_png(&self, path: &StdPath) -> Result<(), String> {
        let comp = self
            .active_composition()
            .ok_or_else(|| "No active composition to export".to_string())?;
        let comp_w = comp.width;
        let comp_h = comp.height;
        let stack = self
            .evaluate_current_frame()
            .map_err(|e| format!("Failed to evaluate frame: {e}"))?;
        let mut assets = std::collections::HashMap::new();
        let current_frame = self.clock.current_frame();
        let fps = comp.frame_rate.max(1.0) as f32;
        let time_s = current_frame as f32 / fps;
        let duration_s = comp.duration.seconds() as f32;

        let buf = crate::raster::comp::rasterize_comp(
            &stack,
            comp_w as f32,
            comp_h as f32,
            comp.background_color,
            comp_w,
            comp_h,
            time_s,
            current_frame,
            false,
            duration_s,
            &mut assets,
        );

        let rgba_bytes = buf.to_rgba8();
        let img = image::RgbaImage::from_raw(comp_w, comp_h, rgba_bytes)
            .ok_or_else(|| "Failed to construct RGBA image from buffer".to_string())?;
        img.save(path).map_err(|e| format!("Failed to save PNG: {e}"))?;
        Ok(())
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

    /// Drag-and-drop reorder: move an effect to the hovered card's slot
    /// (one undo step). No-op when the target equals the current index.
    pub fn move_effect_to(&mut self, layer_id: &str, effect_id: &str, index: usize) -> Result<(), String> {
        let current = {
            let comp = self
                .active_composition()
                .ok_or_else(|| "No active composition".to_string())?;
            let layer = comp
                .get_layer(layer_id)
                .ok_or_else(|| format!("Layer {layer_id} not found"))?;
            layer
                .effects
                .iter()
                .position(|e| e.id == effect_id)
                .ok_or_else(|| format!("Effect {effect_id} not found"))?
        };
        if current == index {
            return Ok(());
        }
        self.checkpoint();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        layer.move_effect(current, index.min(layer.effects.len().saturating_sub(1)));
        Ok(())
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

    /// Set a mask's combining mode directly (Combobox commit path).
    pub fn set_mask_mode(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        mode: project::MaskMode,
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
        mask.mode = mode;
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
        if let Some(prop) = mask.get_param_property_mut(param_name) {
            let cur = if prop.is_animated() {
                prop.evaluate_at(&current_tc)
            } else {
                prop.value
            };
            let new_val = match param_name.to_lowercase().as_str() {
                "opacity" => (cur + delta).clamp(0.0, 100.0),
                "feather" => (cur + delta).max(0.0),
                "expansion" => (cur + delta).clamp(-500.0, 500.0),
                _ => cur + delta,
            };
            prop.set_value(new_val);
            if prop.is_animated() {
                prop.add_keyframe(Keyframe::new(current_tc, new_val));
            }
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
        self.move_mask_handle_live_internal(layer_id, mask_id, index, is_in, tip, false)
    }

    /// Rewrite one mask tangent handle with Alt breaking (sharp cusp).
    pub fn move_mask_handle_live_break(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        index: usize,
        is_in: bool,
        tip: Vec2,
    ) -> Result<(), String> {
        self.move_mask_handle_live_internal(layer_id, mask_id, index, is_in, tip, true)
    }

    fn move_mask_handle_live_internal(
        &mut self,
        layer_id: &str,
        mask_id: &str,
        index: usize,
        is_in: bool,
        tip: Vec2,
        break_tangent: bool,
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
        if break_tangent {
            node.kind = project::PathPointKind::Corner;
        } else if node.kind == project::PathPointKind::Corner {
            // Dragging handle promotes corner to symmetric Bézier curve
            node.kind = project::PathPointKind::Symmetric;
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

    /// Toggle mask locked status.
    pub fn toggle_mask_lock(&mut self, layer_id: &str, mask_id: &str) -> Result<(), String> {
        let is_locked = {
            let comp = self.active_composition().ok_or_else(|| "No active composition".to_string())?;
            let layer = comp.get_layer(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
            let mask = layer.get_mask(mask_id).ok_or_else(|| format!("Mask {mask_id} not found"))?;
            mask.locked
        };
        self.set_mask_locked(layer_id, mask_id, !is_locked)
    }

    /// Delete a mask from a layer (alias for remove_layer_mask).
    pub fn delete_mask(&mut self, layer_id: &str, mask_id: &str) -> Result<(), String> {
        self.remove_layer_mask(layer_id, mask_id)
    }

    /// Rewrite a mask path from a numeric bounding box (the Mask Shape
    /// dialog: Rectangle / Ellipse + explicit x/y/w/h in layer-local px).
    /// Snapshots a keyframe when the path is animated.
    #[allow(clippy::too_many_arguments)]
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
        self.add_shaped_mask_at(layer_id, kind, None, None)
    }

    /// Add a shaped mask (rectangle/ellipse) with optional canvas center position and size.
    pub fn add_shaped_mask_at(
        &mut self,
        layer_id: &str,
        kind: MaskShapeKind,
        comp_center: Option<Vec2>,
        desired_size: Option<(f32, f32)>,
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
        let (bw, bh) = match desired_size {
            Some((sw, sh)) => (sw.max(8.0), sh.max(8.0)),
            None => (w.clamp(8.0, 400.0), h.clamp(8.0, 300.0)),
        };
        let (x, y) = match comp_center {
            Some(cc) => {
                let local = self.comp_to_layer_local(layer_id, cc).unwrap_or(cc);
                (local.x - bw / 2.0, local.y - bh / 2.0)
            }
            None => ((w - bw) / 2.0, (h - bh) / 2.0),
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
        let path = kind.path(x, y, bw, bh);
        layer.masks.push(Mask::with_path(&id, name, path));
        self.active_mask_edit = Some((layer_id.to_string(), id.clone()));
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
                vec![(current_tc, src.transform.position.value)]
            } else {
                keys.iter().map(|k| (k.time, k.value)).collect()
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
            mask.path.add_keyframe(Keyframe::new(*tc, mask.path.value.clone()));
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
            let dur = comp.duration;
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
                            mask.path.add_keyframe(Keyframe::new(*tc, fp.clone()));
                        }
                    }
                }
                // Guarantee the playhead frame holds.
                if !mask.path.has_keyframe_at(&frames[0]) {
                    mask.path.add_keyframe(Keyframe::new(frames[0], path.clone()));
                }
            } else {
                mask.path.set_animated(true);
                mask.path.add_keyframe(Keyframe::new(frames[0], path.clone()));
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
            let opts = AutoTraceOptions {
                threshold_pct: 20.0,
                min_area_px: 4.0,
                tolerance_px: 0.75,
                ..Default::default()
            };
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
            comp.duration,
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
            mask.path.add_keyframe(Keyframe::new(current_tc, path.clone()));
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

        // Declared range when known (bipolar stock params like Color
        // Balance reach their negatives); legacy zero floor otherwise.
        // Resolved before the property borrow below.
        let range = effect.param_range(param_name);
        if let Some(prop) = effect.get_param_property_mut(param_name) {
            let current = if prop.is_animated() {
                prop.evaluate_at(&current_tc)
            } else {
                prop.value
            };
            let (lo, hi) = range.unwrap_or((0.0, f32::INFINITY));
            let new_val = (current + delta).clamp(lo, hi);
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

        let current_tc = self.clock.timecode();
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
            kc.set_value(key_color);
            if kc.is_animated() {
                kc.add_keyframe(Keyframe::new(current_tc, key_color));
            }
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

        let current_tc = self.clock.timecode();
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
                mb.set_value(c);
                if mb.is_animated() {
                    mb.add_keyframe(Keyframe::new(current_tc, c));
                }
            }
            if let Some(c) = map_white {
                mw.set_value(c);
                if mw.is_animated() {
                    mw.add_keyframe(Keyframe::new(current_tc, c));
                }
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

        let current_tc = self.clock.timecode();
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
            c.set_value(color);
            if c.is_animated() {
                c.add_keyframe(Keyframe::new(current_tc, color));
            }
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
            let next = !monochrome.value;
            monochrome.set_value(next);
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
        let current_tc = self.clock.timecode();
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
                fill.set_value(color);
                if fill.is_animated() {
                    fill.add_keyframe(Keyframe::new(current_tc, color));
                }
                Ok(())
            }
            _ => Err("Not a shape layer".to_string()),
        }
    }

    /// Set a pen-path shape's stroke color (Path shapes only).
    pub fn set_layer_shape_stroke(&mut self, layer_id: &str, color: Color) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Path { stroke, .. },
            } => {
                stroke.set_value(color);
                if stroke.is_animated() {
                    stroke.add_keyframe(Keyframe::new(current_tc, color));
                }
                Ok(())
            }
            _ => Err("Not a path shape layer".to_string()),
        }
    }

    /// Set a pen-path shape's stroke width in px (Path shapes only).
    pub fn set_layer_shape_stroke_width(&mut self, layer_id: &str, width: f32) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Path { stroke_width, .. },
            } => {
                stroke_width.set_value(width.max(0.0));
                if stroke_width.is_animated() {
                    stroke_width.add_keyframe(Keyframe::new(current_tc, width.max(0.0)));
                }
                Ok(())
            }
            _ => Err("Not a path shape layer".to_string()),
        }
    }

    /// Mutable access to a layer's fill-gradient slot by picker key:
    /// `"text_fill"` / `"text_stroke"` (Text), `"shape_fill"` (Shape),
    /// `"solid_color"` (Solid). Errors when the key doesn't fit the source.
    fn fill_gradient_slot_mut<'a>(
        layer: &'a mut Layer,
        key: &str,
    ) -> Result<&'a mut Option<Property<FillGradient>>, String> {
        let lid = layer.id.clone();
        match &mut layer.source {
            LayerSource::Text {
                fill_gradient,
                stroke_gradient,
                ..
            } => match key {
                "text_fill" => Ok(fill_gradient),
                "text_stroke" => Ok(stroke_gradient),
                _ => Err(format!("Fill key {key} is not on Text layer {lid}")),
            },
            LayerSource::Shape {
                shape_type:
                    ShapeType::Rectangle { fill_gradient, .. }
                    | ShapeType::Ellipse { fill_gradient, .. }
                    | ShapeType::Path { fill_gradient, .. },
            } => match key {
                "shape_fill" => Ok(fill_gradient),
                _ => Err(format!("Fill key {key} is not on Shape layer {lid}")),
            },
            LayerSource::Solid { fill_gradient, .. } => match key {
                "solid_color" => Ok(fill_gradient),
                _ => Err(format!("Fill key {key} is not on Solid layer {lid}")),
            },
            _ => Err(format!("Layer {lid} has no gradient fill slot for {key}")),
        }
    }

    /// Read a layer's committed fill gradient (the panel renders from this;
    /// its local maps are only defaults before the first commit).
    pub fn layer_fill_gradient(&self, layer_id: &str, key: &str) -> Option<FillGradient> {
        let comp = self.active_composition()?;
        let layer = comp.get_layer(layer_id)?;
        match (&layer.source, key) {
            (
                LayerSource::Text {
                    fill_gradient, ..
                },
                "text_fill",
            ) => fill_gradient.as_ref().map(|p| p.value.clone()),
            (LayerSource::Text { stroke_gradient, .. }, "text_stroke") => {
                stroke_gradient.as_ref().map(|p| p.value.clone())
            }
            (
                LayerSource::Shape {
                    shape_type:
                        ShapeType::Rectangle { fill_gradient, .. }
                        | ShapeType::Ellipse { fill_gradient, .. }
                        | ShapeType::Path { fill_gradient, .. },
                },
                "shape_fill",
            ) => fill_gradient.as_ref().map(|p| p.value.clone()),
            (LayerSource::Solid { fill_gradient, .. }, "solid_color") => {
                fill_gradient.as_ref().map(|p| p.value.clone())
            }
            _ => None,
        }
    }

    /// Commit a whole fill gradient (one undo step). `None` clears back to
    /// the solid color.
    pub fn set_layer_fill_gradient(
        &mut self,
        layer_id: &str,
        key: &str,
        gradient: Option<FillGradient>,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let slot = Self::fill_gradient_slot_mut(layer, key)?;
        match gradient {
            None => {
                *slot = None;
            }
            Some(g) => match slot {
                Some(prop) => {
                    if prop.is_animated() {
                        prop.add_keyframe(Keyframe::new(current_tc, g.clone()));
                    }
                    prop.set_value(g);
                }
                empty => {
                    *empty = Some(Property::new(key, g));
                }
            },
        }
        Ok(())
    }

    /// Append a stop; returns its sorted index (one undo step).
    pub fn add_fill_gradient_stop(
        &mut self,
        layer_id: &str,
        key: &str,
        offset: f32,
        color: Color,
    ) -> Result<usize, String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let slot = Self::fill_gradient_slot_mut(layer, key)?;
        let prop = slot.get_or_insert_with(|| {
            Property::new(key, FillGradient::two_color(Color::WHITE, Color::BLACK, 90.0))
        });
        let at = prop.value.add_stop(offset, color);
        if prop.is_animated() {
            let v = prop.value.clone();
            prop.add_keyframe(Keyframe::new(current_tc, v));
        }
        Ok(at)
    }

    /// Drag a stop to a new offset (no checkpoint: the drag gesture
    /// checkpoints once on press). Returns the stop's new sorted index.
    pub fn move_fill_gradient_stop(
        &mut self,
        layer_id: &str,
        key: &str,
        index: usize,
        offset: f32,
    ) -> Result<usize, String> {
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let slot = Self::fill_gradient_slot_mut(layer, key)?;
        match slot {
            Some(prop) => {
                let at = prop
                    .value
                    .set_stop_offset(index, offset)
                    .ok_or_else(|| format!("Gradient stop {index} out of range"))?;
                if prop.is_animated() {
                    let v = prop.value.clone();
                    prop.add_keyframe(Keyframe::new(current_tc, v));
                }
                Ok(at)
            }
            None => Err("No gradient on this fill".to_string()),
        }
    }

    /// Recolor one stop (one undo step).
    pub fn set_fill_gradient_stop_color(
        &mut self,
        layer_id: &str,
        key: &str,
        index: usize,
        color: Color,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let slot = Self::fill_gradient_slot_mut(layer, key)?;
        match slot {
            Some(prop) => match prop.value.stops.get_mut(index) {
                Some(stop) => {
                    stop.color = color;
                    if prop.is_animated() {
                        let v = prop.value.clone();
                        prop.add_keyframe(Keyframe::new(current_tc, v));
                    }
                    Ok(())
                }
                None => Err(format!("Gradient stop {index} out of range")),
            },
            None => Err("No gradient on this fill".to_string()),
        }
    }

    /// Delete a stop; refuses to drop below two (one undo step).
    pub fn remove_fill_gradient_stop(
        &mut self,
        layer_id: &str,
        key: &str,
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
        let slot = Self::fill_gradient_slot_mut(layer, key)?;
        match slot {
            Some(prop) => {
                if prop.value.stops.len() <= 2 {
                    return Err("A gradient needs at least two stops".to_string());
                }
                let out = prop
                    .value
                    .remove_stop(index)
                    .map(|_| ())
                    .ok_or_else(|| format!("Gradient stop {index} out of range"));
                if prop.is_animated() {
                    let v = prop.value.clone();
                    prop.add_keyframe(Keyframe::new(current_tc, v));
                }
                out
            }
            None => Err("No gradient on this fill".to_string()),
        }
    }

    /// Set the gradient axis in degrees (one undo step).
    pub fn set_fill_gradient_angle(
        &mut self,
        layer_id: &str,
        key: &str,
        angle: f32,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let slot = Self::fill_gradient_slot_mut(layer, key)?;
        match slot {
            Some(prop) => {
                prop.value.angle = angle;
                if prop.is_animated() {
                    let v = prop.value.clone();
                    prop.add_keyframe(Keyframe::new(current_tc, v));
                }
                Ok(())
            }
            None => Err("No gradient on this fill".to_string()),
        }
    }

    /// Set the gradient projection type (Linear, Radial, Angular).
    pub fn set_fill_gradient_type(
        &mut self,
        layer_id: &str,
        key: &str,
        g_type: GradientType,
    ) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let slot = Self::fill_gradient_slot_mut(layer, key)?;
        match slot {
            Some(prop) => {
                prop.value.gradient_type = g_type;
                if prop.is_animated() {
                    let v = prop.value.clone();
                    prop.add_keyframe(Keyframe::new(current_tc, v));
                }
                Ok(())
            }
            None => Err("No gradient on this fill".to_string()),
        }
    }

    /// Mirror all stop offsets end-for-end (one undo step).
    pub fn reverse_fill_gradient(&mut self, layer_id: &str, key: &str) -> Result<(), String> {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let slot = Self::fill_gradient_slot_mut(layer, key)?;
        match slot {
            Some(prop) => {
                let rev = prop.value.reversed();
                if prop.is_animated() {
                    prop.add_keyframe(Keyframe::new(current_tc, rev.clone()));
                }
                prop.set_value(rev);
                Ok(())
            }
            None => Err("No gradient on this fill".to_string()),
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
        if effect.set_color_value(field, color) || effect.set_stock_color(field, color) {
            if let Some(prop) = effect.get_color_property_mut(field) {
                if prop.is_animated() {
                    prop.add_keyframe(Keyframe::new(current_tc, color));
                }
            }
            Ok(())
        } else {
            Err(format!("Color field {field} not found on effect {effect_id}"))
        }
    }

    /// Set a named enum option by index on an effect (Tiler layout/cell).
    pub fn set_effect_enum(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        field: &str,
        index: usize,
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
        if effect.set_enum_value(field, index) {
            Ok(())
        } else {
            Err(format!("Enum field {field} not found on effect {effect_id}"))
        }
    }

    /// Set a named boolean flag on an effect (Tiler mirror).
    pub fn set_effect_bool(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        field: &str,
        value: bool,
    ) -> Result<(), String> {
        self.checkpoint();
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
        if effect.set_bool_value(field, value) {
            if let Some(prop) = effect.get_bool_property_mut(field) {
                if prop.is_animated() {
                    prop.add_keyframe(Keyframe::new(current_tc, value));
                }
            }
            Ok(())
        } else {
            Err(format!("Bool field {field} not found on effect {effect_id}"))
        }
    }

    /// Set one stock scalar param to an absolute value (live drag: the
    /// grab site checkpoints, mirroring mask handle drags). Used by
    /// viewport gizmos (corner pin) that compute targets, not deltas.
    pub fn move_stock_param_live(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        param_name: &str,
        value: f32,
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
        match effect.get_param_property_mut(param_name) {
            Some(prop) => {
                prop.set_value(value);
                if prop.is_animated() {
                    prop.add_keyframe(Keyframe::new(current_tc, value));
                }
                Ok(())
            }
            None => Err(format!("Parameter {param_name} not found on effect {effect_id}")),
        }
    }

    /// Move one warp lattice pin (live drag: the grab site checkpoints,
    /// mirroring mask handle drags). Empty grids materialize as identity.
    pub fn move_warp_pin_live(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        index: usize,
        dx: f32,
        dy: f32,
    ) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        match &mut effect.effect_type {
            EffectType::Warp { pins, .. } => {
                // Grow on demand (overlay only addresses live grid cells);
                // shrinking keeps tail offsets so regrowing restores them.
                while pins.len() <= index {
                    pins.push(WarpPin::default());
                }
                pins[index] = WarpPin::new(dx, dy);
                Ok(())
            }
            _ => Err(format!("Effect {effect_id} is not a Warp")),
        }
    }

    /// Add a puppet pin at layer-local content coords (double-click).
    /// Pin offsets start identity; keyframe tracks arm on first drag.
    pub fn add_puppet_pin(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        x: f32,
        y: f32,
    ) -> Result<usize, String> {
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
        match &mut effect.effect_type {
            EffectType::Puppet { pins, .. } => {
                pins.push(project::PuppetPin::new(x, y));
                Ok(pins.len() - 1)
            }
            _ => Err(format!("Effect {effect_id} is not a Puppet warp")),
        }
    }

    /// Delete a puppet pin by index (right-click).
    pub fn remove_puppet_pin(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        index: usize,
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
        match &mut effect.effect_type {
            EffectType::Puppet { pins, .. } => {
                if index >= pins.len() {
                    return Err(format!("Puppet pin {index} out of range on effect {effect_id}"));
                }
                pins.remove(index);
                Ok(())
            }
            _ => Err(format!("Effect {effect_id} is not a Puppet warp")),
        }
    }

    /// Move one puppet pin (live drag: the grab site checkpoints, mirroring
    /// mask handle drags). Commits a playhead keyframe when animated, so
    /// pins are keyframable through the standard property tracks.
    pub fn move_puppet_pin_live(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        index: usize,
        dx: f32,
        dy: f32,
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
        match &mut effect.effect_type {
            EffectType::Puppet { pins, .. } => {
                let pin = pins.get_mut(index).ok_or_else(|| {
                    format!("Puppet pin {index} out of range on effect {effect_id}")
                })?;
                pin.dx.set_value(dx);
                if pin.dx.is_animated() {
                    pin.dx.add_keyframe(Keyframe::new(current_tc, dx));
                }
                pin.dy.set_value(dy);
                if pin.dy.is_animated() {
                    pin.dy.add_keyframe(Keyframe::new(current_tc, dy));
                }
                Ok(())
            }
            _ => Err(format!("Effect {effect_id} is not a Puppet warp")),
        }
    }

    /// Read a Gradient Ramp's working stops (explicit stops, else the
    /// endpoint pair) for the shared gradient editor.
    pub fn effect_gradient_stops(
        &self,
        layer_id: &str,
        effect_id: &str,
    ) -> Result<Vec<project::GradientStop>, String> {
        let comp = self
            .active_composition()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let effect = layer
            .get_effect(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        effect
            .effect_type
            .gradient_ramp_stops()
            .ok_or_else(|| format!("Effect {effect_id} has no gradient"))
    }

    /// Replace a Gradient Ramp's explicit stops (one undo step; fewer than
    /// two clears back to the endpoint pair).
    pub fn set_effect_gradient_stops(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        stops: Vec<project::GradientStop>,
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
        if effect.effect_type.set_gradient_ramp_stops(stops) {
            Ok(())
        } else {
            Err(format!("Effect {effect_id} has no gradient"))
        }
    }

    /// Query the gradient ramp type for an effect.
    pub fn effect_gradient_type(
        &self,
        layer_id: &str,
        effect_id: &str,
    ) -> Result<GradientType, String> {
        let comp = self
            .active_composition()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let effect = layer
            .get_effect(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        effect
            .effect_type
            .gradient_ramp_type()
            .ok_or_else(|| format!("Effect {effect_id} has no gradient"))
    }

    /// Set the gradient ramp type for an effect.
    pub fn set_effect_gradient_type(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        g_type: GradientType,
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
        if effect.effect_type.set_gradient_ramp_type(g_type) {
            Ok(())
        } else {
            Err(format!("Effect {effect_id} has no gradient"))
        }
    }

    /// Append a ramp stop; returns its sorted index (one undo step).
    pub fn add_effect_gradient_stop(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        offset: f32,
        color: Color,
    ) -> Result<usize, String> {
        let mut stops = self.effect_gradient_stops(layer_id, effect_id)?;
        let mut grad = FillGradient { stops: std::mem::take(&mut stops), angle: 0.0, gradient_type: GradientType::Linear };
        let at = grad.add_stop(offset, color);
        self.set_effect_gradient_stops(layer_id, effect_id, grad.stops)?;
        Ok(at)
    }

    /// Drag a ramp stop (no checkpoint: the gesture checkpoints on press).
    /// Returns the stop's new sorted index.
    pub fn move_effect_gradient_stop(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        index: usize,
        offset: f32,
    ) -> Result<usize, String> {
        let mut stops = self.effect_gradient_stops(layer_id, effect_id)?;
        let mut grad = FillGradient { stops: std::mem::take(&mut stops), angle: 0.0, gradient_type: GradientType::Linear };
        let at = grad
            .set_stop_offset(index, offset)
            .ok_or_else(|| format!("Gradient stop {index} out of range"))?;
        // Drag commits skip the undo checkpoint (one step per gesture, taken
        // on press), so write the stops back directly.
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;
        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if effect.effect_type.set_gradient_ramp_stops(grad.stops) {
            Ok(at)
        } else {
            Err(format!("Effect {effect_id} has no gradient"))
        }
    }

    /// Recolor one ramp stop (one undo step).
    pub fn set_effect_gradient_stop_color(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        index: usize,
        color: Color,
    ) -> Result<(), String> {
        let mut stops = self.effect_gradient_stops(layer_id, effect_id)?;
        if index >= stops.len() {
            return Err(format!("Gradient stop {index} out of range"));
        }
        stops[index].color = color;
        self.set_effect_gradient_stops(layer_id, effect_id, stops)
    }

    /// Delete a ramp stop; refuses to drop below two (one undo step).
    pub fn remove_effect_gradient_stop(
        &mut self,
        layer_id: &str,
        effect_id: &str,
        index: usize,
    ) -> Result<(), String> {
        let mut stops = self.effect_gradient_stops(layer_id, effect_id)?;
        if stops.len() <= 2 {
            return Err("A gradient needs at least two stops".to_string());
        }
        if index >= stops.len() {
            return Err(format!("Gradient stop {index} out of range"));
        }
        stops.remove(index);
        self.set_effect_gradient_stops(layer_id, effect_id, stops)
    }

    /// Mirror all ramp stops end-for-end (one undo step).
    pub fn reverse_effect_gradient(
        &mut self,
        layer_id: &str,
        effect_id: &str,
    ) -> Result<(), String> {
        let stops = self.effect_gradient_stops(layer_id, effect_id)?;
        let grad = FillGradient { stops, angle: 0.0, gradient_type: GradientType::Linear };
        self.set_effect_gradient_stops(layer_id, effect_id, grad.reversed().stops)
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
                } else if let Some(rest) = key.strip_prefix("mask:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 2 {
                        let _ = self.nudge_mask_param(layer_id, parts[0], parts[1], delta);
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
                if let Some(rest) = key.strip_prefix("fx:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() < 2 {
                        return None;
                    }
                    let prop = layer.get_effect(parts[0])?.get_param_property(parts[1])?;
                    Some(eval_num(prop))
                } else if let Some(rest) = key.strip_prefix("mask:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() < 2 {
                        return None;
                    }
                    let mask = layer.get_mask(parts[0])?;
                    let prop = mask.get_param_property(parts[1])?;
                    Some(eval_num(prop))
                } else {
                    None
                }
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
                if let Some(rest) = key.strip_prefix("fx:") {
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
                } else if let Some(rest) = key.strip_prefix("mask:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() < 2 {
                        return false;
                    }
                    let cur = match self.timeline_current_value(&lid, key) {
                        Some(c) => c,
                        None => return false,
                    };
                    self.nudge_mask_param(&lid, parts[0], parts[1], v - cur)
                        .is_ok()
                } else {
                    false
                }
            }
        }
    }

    /// Toggle stopwatch / animation status for a property path on the specified layer.
    /// Stopwatch toggle for an optional gradient slot: clears keys when
    /// animated, seeds a keyframe on the current value, or inserts a
    /// default gradient + keyframe when empty.
    fn toggle_gradient_animation(slot: &mut Option<Property<FillGradient>>, tc: TimeCode) {
        match slot {
            Some(prop) if prop.is_animated() => prop.clear_keyframes(),
            Some(prop) => {
                let v = prop.value.clone();
                prop.add_keyframe(Keyframe::new(tc, v));
            }
            None => {
                let mut prop = Property::new(
                    "Gradient",
                    FillGradient::two_color(Color::WHITE, Color::BLACK, 90.0),
                );
                prop.add_keyframe(Keyframe::new(tc, prop.value.clone()));
                *slot = Some(prop);
            }
        }
    }

    /// Diamond toggle for an optional gradient slot at the playhead.
    fn toggle_gradient_keyframe(slot: &mut Option<Property<FillGradient>>, tc: TimeCode) {
        let prop = slot.get_or_insert_with(|| {
            Property::new(
                "Gradient",
                FillGradient::two_color(Color::WHITE, Color::BLACK, 90.0),
            )
        });
        let v = if prop.is_animated() {
            prop.evaluate_at(&tc)
        } else {
            prop.value.clone()
        };
        prop.toggle_keyframe(tc, v);
    }

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
            "text.stroke_color" => {
                if let LayerSource::Text { stroke_color, .. } = &mut layer.source {
                    if stroke_color.is_animated() { stroke_color.clear_keyframes(); }
                    else { stroke_color.add_keyframe(Keyframe::new(current_tc, stroke_color.value)); }
                }
            }
            "solid.color" => {
                if let LayerSource::Solid { color, .. } = &mut layer.source {
                    if color.is_animated() { color.clear_keyframes(); }
                    else { color.add_keyframe(Keyframe::new(current_tc, color.value)); }
                }
            }
            "shape.fill" => {
                if let LayerSource::Shape { shape_type } = &mut layer.source {
                    let fill = match shape_type {
                        ShapeType::Rectangle { fill, .. }
                        | ShapeType::Ellipse { fill, .. }
                        | ShapeType::Path { fill, .. } => fill,
                    };
                    if fill.is_animated() { fill.clear_keyframes(); }
                    else { let v = fill.value; fill.add_keyframe(Keyframe::new(current_tc, v)); }
                }
            }
            "solid.gradient" => {
                if let LayerSource::Solid { fill_gradient, .. } = &mut layer.source {
                    Self::toggle_gradient_animation(fill_gradient, current_tc);
                }
            }
            "shape.gradient" => {
                if let LayerSource::Shape { shape_type } = &mut layer.source {
                    let slot = match shape_type {
                        ShapeType::Rectangle { fill_gradient, .. }
                        | ShapeType::Ellipse { fill_gradient, .. }
                        | ShapeType::Path { fill_gradient, .. } => fill_gradient,
                    };
                    Self::toggle_gradient_animation(slot, current_tc);
                }
            }
            "text.fill_gradient" => {
                if let LayerSource::Text { fill_gradient, .. } = &mut layer.source {
                    Self::toggle_gradient_animation(fill_gradient, current_tc);
                }
            }
            "text.stroke_gradient" => {
                if let LayerSource::Text { stroke_gradient, .. } = &mut layer.source {
                    Self::toggle_gradient_animation(stroke_gradient, current_tc);
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
                            } else if let Some(prop) = fx.get_color_property_mut(param_name) {
                                if prop.is_animated() {
                                    prop.clear_keyframes();
                                } else {
                                    let val = prop.value;
                                    prop.add_keyframe(Keyframe::new(current_tc, val));
                                }
                            } else if let Some(prop) = fx.get_bool_property_mut(param_name) {
                                if prop.is_animated() {
                                    prop.clear_keyframes();
                                } else {
                                    let val = prop.value;
                                    prop.add_keyframe(Keyframe::new(current_tc, val));
                                }
                            }
                        }
                    }
                } else if let Some(rest) = prop_path.strip_prefix("mask:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let mask_id = parts[0];
                        let param_name = parts[1];
                        if param_name == "path" {
                            if let Some(mask) = layer.get_mask_mut(mask_id) {
                                if mask.path.is_animated() {
                                    mask.path.clear_keyframes();
                                } else {
                                    let val = mask.path.value.clone();
                                    mask.path.add_keyframe(Keyframe::new(current_tc, val));
                                }
                            }
                        } else if let Some(mask) = layer.get_mask_mut(mask_id) {
                            if let Some(prop) = mask.get_param_property_mut(param_name) {
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

                    None => false,

                }
            }
        }
    }

    /// Delete the keyframe near an absolute time.
    pub fn remove_graph_keyframe(&mut self, layer_id: &str, path: &str, at_s: f64) -> bool {
        self.checkpoint();
        self.remove_graph_keyframe_inner(layer_id, path, at_s)
    }

    /// Delete a marquee selection in one undo step. Returns keys removed.
    pub fn remove_graph_keys(&mut self, keys: &[(String, String, f64)]) -> usize {
        if keys.is_empty() {
            return 0;
        }
        self.checkpoint();
        let mut removed = 0;
        for (layer_id, path, at_s) in keys {
            if self.remove_graph_keyframe_inner(layer_id, path, *at_s) {
                removed += 1;
            }
        }
        removed
    }

    /// Keyframe times for one graph path (marquee hit-testing in lanes).
    /// A Vec2 base without component unions both axes (lane rows show the
    /// union); component paths stay exact.
    pub fn graph_key_times(&self, layer_id: &str, path: &str) -> Vec<f64> {
        let comp = match self.active_composition() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let layer = match comp.get_layer(layer_id) {
            Some(l) => l,
            None => return Vec::new(),
        };
        let (base, comp_sfx) = match split_graph_path(path) {
            Some(v) => v,
            None => return Vec::new(),
        };
        if comp_sfx.is_none()
            && matches!(
                base,
                "transform.anchor_point" | "transform.position" | "transform.scale"
            )
        {
            let mut ts = self.graph_key_times(layer_id, &format!("{base}.x"));
            ts.extend(self.graph_key_times(layer_id, &format!("{base}.y")));
            ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            ts.dedup();
            return ts;
        }
        graph_prop_read(layer, path)
            .map(|(keys, _)| keys.iter().map(|k| k.t).collect())
            .unwrap_or_default()
    }

    fn remove_graph_keyframe_inner(&mut self, layer_id: &str, path: &str, at_s: f64) -> bool {
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
        
        match base {
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
                    None => false,

                }
            }
        }
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
        edit_graph_keyframe(layer, path, at_s, tol, |interp, _, _| {
            *interp = match interp {
                KeyframeInterpolation::Linear => KeyframeInterpolation::Bezier,
                KeyframeInterpolation::Bezier => KeyframeInterpolation::Hold,
                KeyframeInterpolation::Hold => KeyframeInterpolation::Linear,
            };
        })
    }

    /// Set one graph key's interpolation from the spline bottom bar
    /// (Linear / Ease In / Ease Out / Hold apply to the selected key only).
    pub fn set_graph_key_easing(
        &mut self,
        layer_id: &str,
        path: &str,
        at_s: f64,
        ease: KeyEase,
    ) -> bool {
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
        edit_graph_keyframe(layer, path, at_s, tol, |interp, in_tan, out_tan| {
            match ease {
                KeyEase::Linear => {
                    *interp = KeyframeInterpolation::Linear;
                    *out_tan = Some(KeyframeTangent::linear_out());
                    *in_tan = Some(KeyframeTangent::linear_in());
                }
                KeyEase::EaseIn => {
                    *interp = KeyframeInterpolation::Bezier;
                    *out_tan = Some(KeyframeTangent::ease_in_out());
                    *in_tan = Some(KeyframeTangent::ease_in_in());
                }
                KeyEase::EaseOut => {
                    *interp = KeyframeInterpolation::Bezier;
                    *out_tan = Some(KeyframeTangent::ease_out_out());
                    *in_tan = Some(KeyframeTangent::ease_out_in());
                }
                KeyEase::Hold => {
                    *interp = KeyframeInterpolation::Hold;
                    *in_tan = None;
                    *out_tan = None;
                }
            }
        })
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

                    None => false,

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
        let sec = seconds.max(0.0);
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
            let v = if p.is_animated() { p.evaluate_at_seconds(sec) } else { p.value };
            if axis == 0 { v.x } else { v.y }
        };
        let get_f32 = |p: &Property<f32>| {
            if p.is_animated() { p.evaluate_at_seconds(sec) } else { p.value }
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

                    None => false,

                }
            }
        }
    }

    /// Toggle a keyframe at the current playback timecode for a property path on the layer.
    pub fn apply_animation_preset(&mut self, layer_id: &str, preset: AnimationPreset) {
        self.checkpoint();
        let current_tc = self.clock.timecode();
        let t0 = current_tc.seconds();
        
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        let fps = comp.frame_rate;
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return,
        };

        let tc = |s: f64| project::TimeCode::from_seconds(s, fps);
        let ez_f32 = |time_s: f64, val: f32, ease_out: (f32, f32), ease_in: (f32, f32)| {
            let mut kf = project::Keyframe::new(tc(time_s), val);
            kf.interpolation = project::KeyframeInterpolation::Bezier;
            kf.out_tangent = Some(project::KeyframeTangent::new(ease_out.0, ease_out.1));
            kf.in_tangent = Some(project::KeyframeTangent::new(ease_in.0, ease_in.1));
            kf
        };
        let ez_vec2 = |time_s: f64, val: project::Vec2, ease_out: (f32, f32), ease_in: (f32, f32)| {
            let mut kf = project::Keyframe::new(tc(time_s), val);
            kf.interpolation = project::KeyframeInterpolation::Bezier;
            kf.out_tangent = Some(project::KeyframeTangent::new(ease_out.0, ease_out.1));
            kf.in_tangent = Some(project::KeyframeTangent::new(ease_in.0, ease_in.1));
            kf
        };
        let lin_f32 = |time_s: f64, val: f32| {
            let mut kf = project::Keyframe::new(tc(time_s), val);
            kf.interpolation = project::KeyframeInterpolation::Linear;
            kf.out_tangent = Some(project::KeyframeTangent::new(1.0/3.0, 1.0/3.0));
            kf.in_tangent = Some(project::KeyframeTangent::new(2.0/3.0, 2.0/3.0));
            kf
        };
        let lin_vec2 = |time_s: f64, val: project::Vec2| {
            let mut kf = project::Keyframe::new(tc(time_s), val);
            kf.interpolation = project::KeyframeInterpolation::Linear;
            kf.out_tangent = Some(project::KeyframeTangent::new(1.0/3.0, 1.0/3.0));
            kf.in_tangent = Some(project::KeyframeTangent::new(2.0/3.0, 2.0/3.0));
            kf
        };
        let hold_f32 = |time_s: f64, val: f32| {
            let mut kf = project::Keyframe::new(tc(time_s), val);
            kf.interpolation = project::KeyframeInterpolation::Hold;
            kf
        };

        let current_pos = if layer.transform.position.is_animated() { layer.transform.position.evaluate_at(&current_tc) } else { layer.transform.position.value };
        let current_scale = if layer.transform.scale.is_animated() { layer.transform.scale.evaluate_at(&current_tc) } else { layer.transform.scale.value };
        let current_rot = if layer.transform.rotation.is_animated() { layer.transform.rotation.evaluate_at(&current_tc) } else { layer.transform.rotation.value };

        let back_out = ((0.175, 0.885), (0.32, 1.275));
        let back_in = ((0.6, -0.28), (0.735, 0.045));
        let expo_out = ((0.19, 1.0), (0.22, 1.0));
        let expo_in = ((0.95, 0.05), (0.795, 0.035));
        let ease_out = ((0.25, 0.46), (0.45, 0.94)); // QuadOut
        let ease_in = ((0.55, 0.085), (0.68, 0.53)); // QuadIn
        let easy_ease = ((0.42, 0.0), (0.58, 1.0));
        let sine_in_out = ((0.445, 0.05), (0.55, 0.95));

        match preset {
            AnimationPreset::FadeIn => {
                layer.opacity.add_keyframe(ez_f32(t0, 0.0, easy_ease.0, easy_ease.1));
                layer.opacity.add_keyframe(ez_f32(t0 + 0.5, 100.0, easy_ease.0, easy_ease.1));
            }
            AnimationPreset::FadeOut => {
                layer.opacity.add_keyframe(ez_f32(t0, 100.0, ease_in.0, ease_in.1));
                layer.opacity.add_keyframe(ez_f32(t0 + 0.5, 0.0, ease_in.0, ease_in.1));
            }
            AnimationPreset::SlideInUp => {
                layer.transform.position.add_keyframe(ez_vec2(t0, project::Vec2 { x: current_pos.x, y: current_pos.y + 200.0 }, ease_out.0, ease_out.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, current_pos, ease_out.0, ease_out.1));
            }
            AnimationPreset::SlideInDown => {
                layer.transform.position.add_keyframe(ez_vec2(t0, project::Vec2 { x: current_pos.x, y: current_pos.y - 200.0 }, ease_out.0, ease_out.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, current_pos, ease_out.0, ease_out.1));
            }
            AnimationPreset::SlideInLeft => {
                layer.transform.position.add_keyframe(ez_vec2(t0, project::Vec2 { x: current_pos.x + 200.0, y: current_pos.y }, ease_out.0, ease_out.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, current_pos, ease_out.0, ease_out.1));
            }
            AnimationPreset::SlideInRight => {
                layer.transform.position.add_keyframe(ez_vec2(t0, project::Vec2 { x: current_pos.x - 200.0, y: current_pos.y }, ease_out.0, ease_out.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, current_pos, ease_out.0, ease_out.1));
            }
            AnimationPreset::PopIn => {
                layer.transform.scale.add_keyframe(ez_vec2(t0, project::Vec2 { x: 0.0, y: 0.0 }, back_out.0, back_out.1));
                layer.transform.scale.add_keyframe(ez_vec2(t0 + 0.4, project::Vec2 { x: 100.0, y: 100.0 }, back_out.0, back_out.1));
            }
            AnimationPreset::DropIn => {
                layer.transform.position.add_keyframe(ez_vec2(t0, project::Vec2 { x: current_pos.x, y: current_pos.y - 300.0 }, back_out.0, back_out.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.6, current_pos, back_out.0, back_out.1));
            }
            AnimationPreset::WhipIn => {
                layer.transform.position.add_keyframe(ez_vec2(t0, project::Vec2 { x: current_pos.x + 400.0, y: current_pos.y }, expo_out.0, expo_out.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.35, current_pos, expo_out.0, expo_out.1));
            }
            AnimationPreset::SlideOutUp => {
                layer.transform.position.add_keyframe(ez_vec2(t0, current_pos, expo_in.0, expo_in.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, project::Vec2 { x: current_pos.x, y: current_pos.y - 200.0 }, expo_in.0, expo_in.1));
            }
            AnimationPreset::SlideOutDown => {
                layer.transform.position.add_keyframe(ez_vec2(t0, current_pos, expo_in.0, expo_in.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, project::Vec2 { x: current_pos.x, y: current_pos.y + 200.0 }, expo_in.0, expo_in.1));
            }
            AnimationPreset::SlideOutLeft => {
                layer.transform.position.add_keyframe(ez_vec2(t0, current_pos, expo_in.0, expo_in.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, project::Vec2 { x: current_pos.x - 200.0, y: current_pos.y }, expo_in.0, expo_in.1));
            }
            AnimationPreset::SlideOutRight => {
                layer.transform.position.add_keyframe(ez_vec2(t0, current_pos, expo_in.0, expo_in.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, project::Vec2 { x: current_pos.x + 200.0, y: current_pos.y }, expo_in.0, expo_in.1));
            }
            AnimationPreset::ShrinkOut => {
                layer.transform.scale.add_keyframe(ez_vec2(t0, current_scale, back_in.0, back_in.1));
                layer.transform.scale.add_keyframe(ez_vec2(t0 + 0.4, project::Vec2 { x: 0.0, y: 0.0 }, back_in.0, back_in.1));
            }
            AnimationPreset::DropOut => {
                layer.transform.position.add_keyframe(ez_vec2(t0, current_pos, expo_in.0, expo_in.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, project::Vec2 { x: current_pos.x, y: current_pos.y + 300.0 }, expo_in.0, expo_in.1));
            }
            AnimationPreset::Pulse => {
                layer.transform.scale.add_keyframe(ez_vec2(t0, current_scale, sine_in_out.0, sine_in_out.1));
                layer.transform.scale.add_keyframe(ez_vec2(t0 + 0.25, project::Vec2 { x: current_scale.x * 1.1, y: current_scale.y * 1.1 }, sine_in_out.0, sine_in_out.1));
                layer.transform.scale.add_keyframe(ez_vec2(t0 + 0.5, current_scale, sine_in_out.0, sine_in_out.1));
            }
            AnimationPreset::Shake => {
                let steps = [0.0, -20.0, 20.0, -15.0, 15.0, -8.0, 8.0, 0.0];
                let dt = 0.5 / 7.0;
                for (i, &offset) in steps.iter().enumerate() {
                    layer.transform.position.add_keyframe(lin_vec2(t0 + (i as f64) * dt, project::Vec2 { x: current_pos.x + offset, y: current_pos.y }));
                }
            }
            AnimationPreset::Float => {
                layer.transform.position.add_keyframe(ez_vec2(t0, current_pos, sine_in_out.0, sine_in_out.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 0.5, project::Vec2 { x: current_pos.x, y: current_pos.y - 15.0 }, sine_in_out.0, sine_in_out.1));
                layer.transform.position.add_keyframe(ez_vec2(t0 + 1.0, current_pos, sine_in_out.0, sine_in_out.1));
            }
            AnimationPreset::Spin => {
                layer.transform.rotation.add_keyframe(lin_f32(t0, current_rot));
                layer.transform.rotation.add_keyframe(lin_f32(t0 + 0.5, current_rot + 360.0));
            }
            AnimationPreset::Flash => {
                let dt = 0.4 / 4.0;
                for i in 0..5 {
                    let val = if i % 2 == 0 { 100.0 } else { 0.0 };
                    layer.opacity.add_keyframe(hold_f32(t0 + (i as f64) * dt, val));
                }
            }
        }
    }

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
            "text.stroke_color" => {
                if let LayerSource::Text { ref mut stroke_color, .. } = layer.source {
                    let val = if stroke_color.is_animated() {
                        stroke_color.evaluate_at(&current_tc)
                    } else {
                        stroke_color.value
                    };
                    stroke_color.toggle_keyframe(current_tc, val);
                }
            }
            "solid.color" => {
                if let LayerSource::Solid { ref mut color, .. } = layer.source {
                    let val = if color.is_animated() {
                        color.evaluate_at(&current_tc)
                    } else {
                        color.value
                    };
                    color.toggle_keyframe(current_tc, val);
                }
            }
            "shape.fill" => {
                if let LayerSource::Shape { ref mut shape_type } = layer.source {
                    let fill = match shape_type {
                        ShapeType::Rectangle { ref mut fill, .. }
                        | ShapeType::Ellipse { ref mut fill, .. }
                        | ShapeType::Path { ref mut fill, .. } => fill,
                    };
                    let val = if fill.is_animated() {
                        fill.evaluate_at(&current_tc)
                    } else {
                        fill.value
                    };
                    fill.toggle_keyframe(current_tc, val);
                }
            }
            "solid.gradient" => {
                if let LayerSource::Solid { ref mut fill_gradient, .. } = layer.source {
                    Self::toggle_gradient_keyframe(fill_gradient, current_tc);
                }
            }
            "shape.gradient" => {
                if let LayerSource::Shape { ref mut shape_type } = layer.source {
                    let slot = match shape_type {
                        ShapeType::Rectangle { ref mut fill_gradient, .. }
                        | ShapeType::Ellipse { ref mut fill_gradient, .. }
                        | ShapeType::Path { ref mut fill_gradient, .. } => fill_gradient,
                    };
                    Self::toggle_gradient_keyframe(slot, current_tc);
                }
            }
            "text.fill_gradient" => {
                if let LayerSource::Text { ref mut fill_gradient, .. } = layer.source {
                    Self::toggle_gradient_keyframe(fill_gradient, current_tc);
                }
            }
            "text.stroke_gradient" => {
                if let LayerSource::Text { ref mut stroke_gradient, .. } = layer.source {
                    Self::toggle_gradient_keyframe(stroke_gradient, current_tc);
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
                            } else if let Some(prop) = fx.get_color_property_mut(param_name) {
                                let val = if prop.is_animated() {
                                    prop.evaluate_at(&current_tc)
                                } else {
                                    prop.value
                                };
                                prop.toggle_keyframe(current_tc, val);
                            } else if let Some(prop) = fx.get_bool_property_mut(param_name) {
                                let val = if prop.is_animated() {
                                    prop.evaluate_at(&current_tc)
                                } else {
                                    prop.value
                                };
                                prop.toggle_keyframe(current_tc, val);
                            }
                        }
                    }
                } else if let Some(rest) = prop_path.strip_prefix("mask:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let mask_id = parts[0];
                        let param_name = parts[1];
                        if param_name == "path" {
                            if let Some(mask) = layer.get_mask_mut(mask_id) {
                                let val = if mask.path.is_animated() {
                                    mask.path.evaluate_at(&current_tc)
                                } else {
                                    mask.path.value.clone()
                                };
                                mask.path.toggle_keyframe(current_tc, val);
                            }
                        } else if let Some(mask) = layer.get_mask_mut(mask_id) {
                            if let Some(prop) = mask.get_param_property_mut(param_name) {
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

    /// Retrieve the modifier graph attached to a property path on a layer.
    pub fn get_layer_modifier_graph(&self, layer_id: &str, prop_path: &str) -> Option<project::ModifierGraph> {
        let comp = self.active_composition()?;
        let layer = comp.get_layer(layer_id)?;
        if let Some(mg) = layer.get_modifier_graph(prop_path) {
            return Some(mg.clone());
        }
        let alias = match prop_path {
            "anchor_x" => "transform.anchor_point.x",
            "anchor_y" => "transform.anchor_point.y",
            "pos_x" => "transform.position.x",
            "pos_y" => "transform.position.y",
            "scale_x" => "transform.scale.x",
            "scale_y" => "transform.scale.y",
            "scale_u" => "transform.scale.x",
            "rotation" => "transform.rotation",
            "transform.anchor_point.x" => "anchor_x",
            "transform.anchor_point.y" => "anchor_y",
            "transform.position.x" => "pos_x",
            "transform.position.y" => "pos_y",
            "transform.scale.x" => "scale_x",
            "transform.scale.y" => "scale_y",
            "transform.rotation" => "rotation",
            _ => return None,
        };
        layer.get_modifier_graph(alias).cloned()
    }

    /// Set or update the modifier graph for a property path on a layer.
    pub fn set_layer_modifier_graph(&mut self, layer_id: &str, prop_path: &str, graph: project::ModifierGraph) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.set_modifier_graph(prop_path, graph);
            }
        }
    }

    /// Remove the modifier graph for a property path on a layer.
    pub fn remove_layer_modifier_graph(&mut self, layer_id: &str, prop_path: &str) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.remove_modifier_graph(prop_path);
            }
        }
    }

    /// Ensure a modifier graph exists for a property (creating default passthrough if missing) and return it.
    pub fn ensure_layer_modifier_graph(&mut self, layer_id: &str, prop_path: &str) -> project::ModifierGraph {
        if let Some(existing) = self.get_layer_modifier_graph(layer_id, prop_path) {
            existing
        } else {
            let default_graph = project::ModifierGraph::default_passthrough();
            self.set_layer_modifier_graph(layer_id, prop_path, default_graph.clone());
            default_graph
        }
    }

    /// Read the current un-modified base value for a property path on a layer.
    pub fn get_layer_property_base_value(&self, layer_id: &str, prop_path: &str) -> f32 {
        let tc = self.clock.timecode();
        let comp = match self.active_composition() {
            Some(c) => c,
            None => return 0.0,
        };
        let layer = match comp.get_layer(layer_id) {
            Some(l) => l,
            None => return 0.0,
        };
        match prop_path {
            "transform.anchor_point.x" | "anchor_x" => layer.transform.anchor_point.evaluate_at(&tc).x,
            "transform.anchor_point.y" | "anchor_y" => layer.transform.anchor_point.evaluate_at(&tc).y,
            "transform.position.x" | "pos_x" => layer.transform.position.evaluate_at(&tc).x,
            "transform.position.y" | "pos_y" => layer.transform.position.evaluate_at(&tc).y,
            "transform.scale.x" | "scale_x" => layer.transform.scale.evaluate_at(&tc).x,
            "transform.scale.y" | "scale_y" => layer.transform.scale.evaluate_at(&tc).y,
            "transform.rotation" | "rotation" => layer.transform.rotation.evaluate_at(&tc),
            "opacity" => layer.opacity.evaluate_at(&tc),
            _ => {
                // Effect scalars share the canonical `effect:{id}:{param}`
                // path everywhere (graph, links, compositor).
                if let Some(rest) = prop_path.strip_prefix("effect:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        if let Some(fx) = layer.get_effect(parts[0]) {
                            if let Some(prop) = fx.get_param_property(parts[1]) {
                                return prop.evaluate_at(&tc);
                            }
                            if let project::EffectType::ShaderLab { params, values, .. } = &fx.effect_type {
                                if let Some(p) = params.iter().find(|p| p.name.eq_ignore_ascii_case(parts[1])) {
                                    let v = values.get(&p.name).cloned().unwrap_or_else(|| p.default.clone());
                                    match v {
                                        project::ShaderParamValue::Float(f) => return f,
                                        project::ShaderParamValue::Int(i) => return i as f32,
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
                0.0
            }
        }
    }

    /// Read the layer's current normalized progression factor ($0.0 \to 1.0$) at the current playhead.
    pub fn get_layer_progression_factor(&self, layer_id: &str) -> f32 {
        let tc = self.clock.timecode();
        self.active_composition()
            .and_then(|c| c.get_layer(layer_id))
            .map(|l| l.progression_factor(&tc))
            .unwrap_or(0.0)
    }

    /// Reset an animatable property to its default value, clearing its keyframes and removing any modifier graph.
    pub fn reset_layer_property(&mut self, layer_id: &str, prop_path: &str) {
        self.checkpoint();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return,
        };

        // Remove any modifier graph on this property
        layer.remove_modifier_graph(prop_path);
        // Remove any property link on this property and its aliases
        let _ = layer.remove_property_link(prop_path);
        match prop_path {
            "anchor_x" => { let _ = layer.remove_property_link("transform.anchor_point.x"); }
            "anchor_y" => { let _ = layer.remove_property_link("transform.anchor_point.y"); }
            "pos_x" => { let _ = layer.remove_property_link("transform.position.x"); }
            "pos_y" => { let _ = layer.remove_property_link("transform.position.y"); }
            "scale_x" => { let _ = layer.remove_property_link("transform.scale.x"); }
            "scale_y" => { let _ = layer.remove_property_link("transform.scale.y"); }
            "rotation" => { let _ = layer.remove_property_link("transform.rotation"); }
            "transform.anchor_point.x" => { let _ = layer.remove_property_link("anchor_x"); }
            "transform.anchor_point.y" => { let _ = layer.remove_property_link("anchor_y"); }
            "transform.position.x" => { let _ = layer.remove_property_link("pos_x"); }
            "transform.position.y" => { let _ = layer.remove_property_link("pos_y"); }
            "transform.scale.x" => { let _ = layer.remove_property_link("scale_x"); }
            "transform.scale.y" => { let _ = layer.remove_property_link("scale_y"); }
            "transform.rotation" => { let _ = layer.remove_property_link("rotation"); }
            _ => {}
        }

        match prop_path {
            "transform.anchor_point" => {
                layer.transform.anchor_point.reset();
                layer.transform.anchor_point.clear_keyframes();
            }
            "transform.anchor_point.x" | "anchor_x" => {
                let def_x = layer.transform.anchor_point.default_value().x;
                layer.transform.anchor_point.value_mut().x = def_x;
                layer.transform.anchor_point.clear_keyframes();
            }
            "transform.anchor_point.y" | "anchor_y" => {
                let def_y = layer.transform.anchor_point.default_value().y;
                layer.transform.anchor_point.value_mut().y = def_y;
                layer.transform.anchor_point.clear_keyframes();
            }
            "transform.position" => {
                layer.transform.position.reset();
                layer.transform.position.clear_keyframes();
            }
            "transform.position.x" | "pos_x" => {
                let def_x = layer.transform.position.default_value().x;
                layer.transform.position.value_mut().x = def_x;
                layer.transform.position.clear_keyframes();
            }
            "transform.position.y" | "pos_y" => {
                let def_y = layer.transform.position.default_value().y;
                layer.transform.position.value_mut().y = def_y;
                layer.transform.position.clear_keyframes();
            }
            "transform.scale" => {
                layer.transform.scale.reset();
                layer.transform.scale.clear_keyframes();
            }
            "transform.scale.x" | "scale_x" => {
                let def_x = layer.transform.scale.default_value().x;
                layer.transform.scale.value_mut().x = def_x;
                layer.transform.scale.clear_keyframes();
            }
            "transform.scale.y" | "scale_y" => {
                let def_y = layer.transform.scale.default_value().y;
                layer.transform.scale.value_mut().y = def_y;
                layer.transform.scale.clear_keyframes();
            }
            "transform.rotation" | "rotation" => {
                layer.transform.rotation.reset();
                layer.transform.rotation.clear_keyframes();
            }
            "opacity" => {
                layer.opacity.reset();
                layer.opacity.clear_keyframes();
            }
            "text.font_size" | "font_size" => {
                if let LayerSource::Text { ref mut font_size, .. } = layer.source {
                    font_size.reset();
                    font_size.clear_keyframes();
                }
            }
            "text.tracking" | "tracking" => {
                if let LayerSource::Text { ref mut tracking, .. } = layer.source {
                    tracking.reset();
                    tracking.clear_keyframes();
                }
            }
            "text.leading" | "leading" => {
                if let LayerSource::Text { ref mut leading, .. } = layer.source {
                    leading.reset();
                    leading.clear_keyframes();
                }
            }
            "text.stroke_width" | "stroke_width" => {
                if let LayerSource::Text { ref mut stroke_width, .. } = layer.source {
                    stroke_width.reset();
                    stroke_width.clear_keyframes();
                }
            }
            "text.baseline_shift" | "baseline_shift" => {
                if let LayerSource::Text { ref mut baseline_shift, .. } = layer.source {
                    baseline_shift.reset();
                    baseline_shift.clear_keyframes();
                }
            }
            "text.box_width" | "box_width" => {
                if let LayerSource::Text { ref mut box_width, .. } = layer.source {
                    box_width.reset();
                    box_width.clear_keyframes();
                }
            }
            "text.fill_color" | "fill_color" => {
                if let LayerSource::Text { ref mut fill_color, .. } = layer.source {
                    fill_color.reset();
                    fill_color.clear_keyframes();
                }
            }
            "text.stroke_color" => {
                if let LayerSource::Text { ref mut stroke_color, .. } = layer.source {
                    stroke_color.reset();
                    stroke_color.clear_keyframes();
                }
            }
            "solid.color" => {
                if let LayerSource::Solid { ref mut color, .. } = layer.source {
                    color.reset();
                    color.clear_keyframes();
                }
            }
            "shape.fill" => {
                if let LayerSource::Shape { ref mut shape_type } = layer.source {
                    let fill = match shape_type {
                        ShapeType::Rectangle { ref mut fill, .. }
                        | ShapeType::Ellipse { ref mut fill, .. }
                        | ShapeType::Path { ref mut fill, .. } => fill,
                    };
                    fill.reset();
                    fill.clear_keyframes();
                }
            }
            "solid.gradient" => {
                if let LayerSource::Solid { ref mut fill_gradient, .. } = layer.source {
                    *fill_gradient = None;
                }
            }
            "shape.gradient" => {
                if let LayerSource::Shape { ref mut shape_type } = layer.source {
                    let slot = match shape_type {
                        ShapeType::Rectangle { ref mut fill_gradient, .. }
                        | ShapeType::Ellipse { ref mut fill_gradient, .. }
                        | ShapeType::Path { ref mut fill_gradient, .. } => fill_gradient,
                    };
                    *slot = None;
                }
            }
            "text.fill_gradient" => {
                if let LayerSource::Text { ref mut fill_gradient, .. } = layer.source {
                    *fill_gradient = None;
                }
            }
            "text.stroke_gradient" => {
                if let LayerSource::Text { ref mut stroke_gradient, .. } = layer.source {
                    *stroke_gradient = None;
                }
            }
            "shape.rect_width" | "rect_width" | "width" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref mut width, .. } } = layer.source {
                    width.reset();
                    width.clear_keyframes();
                }
            }
            "shape.rect_height" | "rect_height" | "height" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref mut height, .. } } = layer.source {
                    height.reset();
                    height.clear_keyframes();
                }
            }
            "shape.corner_radius" | "corner_radius" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref mut corner_radius, .. } } = layer.source {
                    corner_radius.reset();
                    corner_radius.clear_keyframes();
                }
            }
            "shape.ellipse_rx" | "radius_x" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref mut radius_x, .. } } = layer.source {
                    radius_x.reset();
                    radius_x.clear_keyframes();
                }
            }
            "shape.ellipse_ry" | "radius_y" => {
                if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref mut radius_y, .. } } = layer.source {
                    radius_y.reset();
                    radius_y.clear_keyframes();
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
                                prop.reset();
                                prop.clear_keyframes();
                            } else if let Some(prop) = fx.get_color_property_mut(param_name) {
                                prop.reset();
                                prop.clear_keyframes();
                            } else if let Some(prop) = fx.get_bool_property_mut(param_name) {
                                prop.reset();
                                prop.clear_keyframes();
                            }
                        }
                    }
                } else if let Some(rest) = prop_path.strip_prefix("mask:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let mask_id = parts[0];
                        let param_name = parts[1];
                        if let Some(mask) = layer.masks.iter_mut().find(|m| m.id == mask_id) {
                            if let Some(prop) = mask.get_param_property_mut(param_name) {
                                prop.reset();
                                prop.clear_keyframes();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Copy a property link `(layer_id, prop_path)` to the clipboard.
    pub fn copy_property_link(&mut self, layer_id: &str, prop_path: &str) {
        let layer_name = self
            .active_composition()
            .and_then(|c| c.get_layer(layer_id))
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "Layer".to_string());
        self.copied_property_link = Some((layer_id.to_string(), prop_path.to_string()));
        self.property_link_toast = Some(format!("Copied with Property Links: {} · {}", layer_name, prop_path));
    }

    /// Paste the copied property link to target property on target layer.
    /// Sets value, sets property link, and replicates modifier graph if present. Returns true if pasted.
    pub fn paste_property_link(&mut self, target_layer_id: &str, target_prop_path: &str) -> bool {
        let (src_lid, src_prop) = match &self.copied_property_link {
            Some(pair) => pair.clone(),
            None => return false,
        };

        let src_val = self.get_layer_property_live_value(&src_lid, &src_prop);
        let src_graph = self.get_layer_modifier_graph(&src_lid, &src_prop);

        self.checkpoint();

        if let Some(g) = src_graph {
            self.set_layer_modifier_graph(target_layer_id, target_prop_path, g);
        }

        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return false,
        };
        let layer = match comp.get_layer_mut(target_layer_id) {
            Some(l) => l,
            None => return false,
        };

        // Bind the property link on the target layer
        layer.set_property_link(target_prop_path, project::PropertyLink::new(&src_lid, &src_prop));

        match target_prop_path {
            "transform.anchor_point.x" | "anchor_x" => {
                layer.transform.anchor_point.value_mut().x = src_val;
            }
            "transform.anchor_point.y" | "anchor_y" => {
                layer.transform.anchor_point.value_mut().y = src_val;
            }
            "transform.anchor_point" => {
                layer.transform.anchor_point.set_value(Vec2::new(src_val, src_val));
            }
            "transform.position.x" | "pos_x" => {
                layer.transform.position.value_mut().x = src_val;
            }
            "transform.position.y" | "pos_y" => {
                layer.transform.position.value_mut().y = src_val;
            }
            "transform.position" => {
                layer.transform.position.set_value(Vec2::new(src_val, src_val));
            }
            "transform.scale.x" | "scale_x" => {
                layer.transform.scale.value_mut().x = src_val;
            }
            "transform.scale.y" | "scale_y" => {
                layer.transform.scale.value_mut().y = src_val;
            }
            "transform.scale" => {
                layer.transform.scale.set_value(Vec2::new(src_val, src_val));
            }
            "transform.rotation" | "rotation" => {
                layer.transform.rotation.set_value(src_val);
            }
            "opacity" => {
                layer.opacity.set_value(src_val);
            }
            "text.font_size" | "font_size" => {
                if let LayerSource::Text { ref mut font_size, .. } = layer.source {
                    font_size.set_value(src_val);
                }
            }
            "text.tracking" | "tracking" => {
                if let LayerSource::Text { ref mut tracking, .. } = layer.source {
                    tracking.set_value(src_val);
                }
            }
            "text.leading" | "leading" => {
                if let LayerSource::Text { ref mut leading, .. } = layer.source {
                    leading.set_value(src_val);
                }
            }
            "text.stroke_width" | "stroke_width" => {
                if let LayerSource::Text { ref mut stroke_width, .. } = layer.source {
                    stroke_width.set_value(src_val);
                }
            }
            "text.baseline_shift" | "baseline_shift" => {
                if let LayerSource::Text { ref mut baseline_shift, .. } = layer.source {
                    baseline_shift.set_value(src_val);
                }
            }
            "text.box_width" | "box_width" => {
                if let LayerSource::Text { ref mut box_width, .. } = layer.source {
                    box_width.set_value(src_val);
                }
            }
            _ => {
                if let Some(rest) = target_prop_path.strip_prefix("effect:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let fx_id = parts[0];
                        let param_name = parts[1];
                        if let Some(fx) = layer.get_effect_mut(fx_id) {
                            if let Some(prop) = fx.get_param_property_mut(param_name) {
                                prop.set_value(src_val);
                            }
                        }
                    }
                } else if let Some(rest) = target_prop_path.strip_prefix("mask:") {
                    let parts: Vec<&str> = rest.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        let mask_id = parts[0];
                        let param_name = parts[1];
                        if let Some(mask) = layer.masks.iter_mut().find(|m| m.id == mask_id) {
                            if let Some(prop) = mask.get_param_property_mut(param_name) {
                                prop.set_value(src_val);
                            }
                        }
                    }
                }
            }
        }
        true
    }

    /// Remove the property link on a given layer's property path.
    pub fn remove_property_link(&mut self, layer_id: &str, prop_path: &str) {
        self.checkpoint();
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                let _ = layer.remove_property_link(prop_path);
                match prop_path {
                    "anchor_x" => { let _ = layer.remove_property_link("transform.anchor_point.x"); }
                    "anchor_y" => { let _ = layer.remove_property_link("transform.anchor_point.y"); }
                    "pos_x" => { let _ = layer.remove_property_link("transform.position.x"); }
                    "pos_y" => { let _ = layer.remove_property_link("transform.position.y"); }
                    "scale_x" => { let _ = layer.remove_property_link("transform.scale.x"); }
                    "scale_y" => { let _ = layer.remove_property_link("transform.scale.y"); }
                    "rotation" => { let _ = layer.remove_property_link("transform.rotation"); }
                    "transform.anchor_point.x" => { let _ = layer.remove_property_link("anchor_x"); }
                    "transform.anchor_point.y" => { let _ = layer.remove_property_link("anchor_y"); }
                    "transform.position.x" => { let _ = layer.remove_property_link("pos_x"); }
                    "transform.position.y" => { let _ = layer.remove_property_link("pos_y"); }
                    "transform.scale.x" => { let _ = layer.remove_property_link("scale_x"); }
                    "transform.scale.y" => { let _ = layer.remove_property_link("scale_y"); }
                    "transform.rotation" => { let _ = layer.remove_property_link("rotation"); }
                    _ => {}
                }
            }
        }
    }

    /// Check if a layer's property is currently driven by a PropertyLink.
    pub fn is_layer_property_linked(&self, layer_id: &str, prop_path: &str) -> bool {
        let comp = match self.active_composition() {
            Some(c) => c,
            None => return false,
        };
        let layer = match comp.get_layer(layer_id) {
            Some(l) => l,
            None => return false,
        };
        if layer.is_property_linked(prop_path) {
            return true;
        }
        match prop_path {
            "anchor_x" => layer.is_property_linked("transform.anchor_point.x"),
            "anchor_y" => layer.is_property_linked("transform.anchor_point.y"),
            "pos_x" => layer.is_property_linked("transform.position.x") || layer.is_property_linked("transform.position"),
            "pos_y" => layer.is_property_linked("transform.position.y") || layer.is_property_linked("transform.position"),
            "scale_x" => layer.is_property_linked("transform.scale.x") || layer.is_property_linked("transform.scale"),
            "scale_y" => layer.is_property_linked("transform.scale.y") || layer.is_property_linked("transform.scale"),
            "scale_u" => layer.is_property_linked("transform.scale.x") || layer.is_property_linked("transform.scale"),
            "rotation" => layer.is_property_linked("transform.rotation"),
            "transform.anchor_point.x" => layer.is_property_linked("anchor_x"),
            "transform.anchor_point.y" => layer.is_property_linked("anchor_y"),
            "transform.position.x" => layer.is_property_linked("pos_x") || layer.is_property_linked("transform.position"),
            "transform.position.y" => layer.is_property_linked("pos_y") || layer.is_property_linked("transform.position"),
            "transform.scale.x" => layer.is_property_linked("scale_x") || layer.is_property_linked("transform.scale"),
            "transform.scale.y" => layer.is_property_linked("scale_y") || layer.is_property_linked("transform.scale"),
            "transform.rotation" => layer.is_property_linked("rotation"),
            _ => false,
        }
    }

    /// Retrieve the property link attached to a layer's property, checking aliases.
    pub fn get_layer_property_link(&self, layer_id: &str, prop_path: &str) -> Option<project::PropertyLink> {
        let comp = self.active_composition()?;
        let layer = comp.get_layer(layer_id)?;
        if let Some(link) = layer.get_property_link(prop_path) {
            return Some(link.clone());
        }
        let alias = match prop_path {
            "anchor_x" => "transform.anchor_point.x",
            "anchor_y" => "transform.anchor_point.y",
            "pos_x" => "transform.position.x",
            "pos_y" => "transform.position.y",
            "scale_x" => "transform.scale.x",
            "scale_y" => "transform.scale.y",
            "scale_u" => "transform.scale.x",
            "rotation" => "transform.rotation",
            "transform.anchor_point.x" => "anchor_x",
            "transform.anchor_point.y" => "anchor_y",
            "transform.position.x" => "pos_x",
            "transform.position.y" => "pos_y",
            "transform.scale.x" => "scale_x",
            "transform.scale.y" => "scale_y",
            "transform.rotation" => "rotation",
            _ => return None,
        };
        layer.get_property_link(alias).cloned()
    }

    /// Read the live value of a layer's property, resolving property links recursively (with cycle detection) if linked.
    pub fn get_layer_property_live_value(&self, layer_id: &str, prop_path: &str) -> f32 {
        let mut visited = std::collections::HashSet::new();
        self.resolve_layer_property_live_value_recursive(layer_id, prop_path, &mut visited)
    }

    fn resolve_layer_property_live_value_recursive(
        &self,
        layer_id: &str,
        prop_path: &str,
        visited: &mut std::collections::HashSet<(String, String)>,
    ) -> f32 {
        let key = (layer_id.to_string(), prop_path.to_string());
        if !visited.insert(key.clone()) {
            return self.get_layer_property_base_value(layer_id, prop_path);
        }

        let base = if let Some(link) = self.get_layer_property_link(layer_id, prop_path) {
            self.resolve_layer_property_live_value_recursive(
                &link.driver_layer_id,
                &link.driver_prop_path,
                visited,
            )
        } else {
            self.get_layer_property_base_value(layer_id, prop_path)
        };

        let result = if let Some(mg) = self.get_layer_modifier_graph(layer_id, prop_path) {
            let factor = self.get_layer_progression_factor(layer_id);
            let visited_cell = std::cell::RefCell::new(visited.clone());
            let resolver = |drv_layer: &str, drv_prop: &str| -> f32 {
                self.resolve_layer_property_live_value_recursive(
                    drv_layer,
                    drv_prop,
                    &mut visited_cell.borrow_mut(),
                )
            };
            mg.evaluate_with_resolver(base, factor, &resolver)
        } else {
            base
        };
        visited.remove(&key);
        result
    }

    /// Delete all keyframes across all animatable properties of a layer.
    pub fn delete_all_keyframes_on_layer(&mut self, layer_id: &str) {
        self.checkpoint();
        let comp = match self.active_composition_mut() {
            Some(c) => c,
            None => return,
        };
        let layer = match comp.get_layer_mut(layer_id) {
            Some(l) => l,
            None => return,
        };
        layer.transform.position.clear_keyframes();
        layer.transform.anchor_point.clear_keyframes();
        layer.transform.scale.clear_keyframes();
        layer.transform.rotation.clear_keyframes();
        layer.opacity.clear_keyframes();
        match &mut layer.source {
            LayerSource::Text {
                text,
                font_size,
                tracking,
                leading,
                stroke_width,
                baseline_shift,
                box_width,
                box_height,
                fill_color,
                ..
            } => {
                text.clear_keyframes();
                font_size.clear_keyframes();
                tracking.clear_keyframes();
                leading.clear_keyframes();
                stroke_width.clear_keyframes();
                baseline_shift.clear_keyframes();
                box_width.clear_keyframes();
                box_height.clear_keyframes();
                fill_color.clear_keyframes();
            }
            LayerSource::Shape { shape_type, .. } => match shape_type {
                project::ShapeType::Rectangle {
                    width,
                    height,
                    corner_radius,
                    ..
                } => {
                    width.clear_keyframes();
                    height.clear_keyframes();
                    corner_radius.clear_keyframes();
                }
                project::ShapeType::Ellipse {
                    radius_x,
                    radius_y,
                    ..
                } => {
                    radius_x.clear_keyframes();
                    radius_y.clear_keyframes();
                }
                _ => {}
            },
            _ => {}
        }
        for effect in &mut layer.effects {
            for decl in effect.declarations() {
                if let Some(prop) = effect.get_param_property_mut(&decl.field) {
                    prop.clear_keyframes();
                }
            }
        }
        for mask in &mut layer.masks {
            mask.path.clear_keyframes();
            mask.opacity.clear_keyframes();
            mask.feather.clear_keyframes();
            mask.expansion.clear_keyframes();
        }
    }

    /// Delete all keyframes on any layer in the active composition referencing this asset.
    pub fn delete_all_keyframes_for_asset(&mut self, asset_id: &str) {
        let matching_layer_ids: Vec<String> = if let Some(comp) = self.active_composition() {
            comp.layers.iter().filter_map(|l| {
                match &l.source {
                    LayerSource::Image { asset_id: aid } | LayerSource::Video { asset_id: aid, .. } => {
                        if aid == asset_id || l.name == asset_id || l.id == asset_id {
                            Some(l.id.clone())
                        } else {
                            None
                        }
                    }
                    _ => {
                        if l.name == asset_id || l.id == asset_id {
                            Some(l.id.clone())
                        } else {
                            None
                        }
                    }
                }
            }).collect()
        } else {
            Vec::new()
        };

        for lid in matching_layer_ids {
            self.delete_all_keyframes_on_layer(&lid);
        }
    }

    /// Delete / remove a layer from the active composition by id.
    pub fn delete_layer(&mut self, layer_id: &str) -> Result<(), String> {
        self.remove_layer_by_id(layer_id)
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
                "text.stroke_color" => {
                    if let LayerSource::Text { ref stroke_color, .. } = layer.source {
                        stroke_color.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "solid.color" => {
                    if let LayerSource::Solid { ref color, .. } = layer.source {
                        color.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.fill" => {
                    if let LayerSource::Shape { ref shape_type } = layer.source {
                        let fill = match shape_type {
                            ShapeType::Rectangle { ref fill, .. }
                            | ShapeType::Ellipse { ref fill, .. }
                            | ShapeType::Path { ref fill, .. } => fill,
                        };
                        fill.previous_keyframe_time(&current_tc)
                    } else { None }
                }
                "solid.gradient" => {
                    if let LayerSource::Solid { ref fill_gradient, .. } = layer.source {
                        fill_gradient.as_ref().and_then(|p| p.previous_keyframe_time(&current_tc))
                    } else { None }
                }
                "shape.gradient" => {
                    if let LayerSource::Shape { ref shape_type } = layer.source {
                        let slot = match shape_type {
                            ShapeType::Rectangle { ref fill_gradient, .. }
                            | ShapeType::Ellipse { ref fill_gradient, .. }
                            | ShapeType::Path { ref fill_gradient, .. } => fill_gradient,
                        };
                        slot.as_ref().and_then(|p| p.previous_keyframe_time(&current_tc))
                    } else { None }
                }
                "text.fill_gradient" => {
                    if let LayerSource::Text { ref fill_gradient, .. } = layer.source {
                        fill_gradient.as_ref().and_then(|p| p.previous_keyframe_time(&current_tc))
                    } else { None }
                }
                "text.stroke_gradient" => {
                    if let LayerSource::Text { ref stroke_gradient, .. } = layer.source {
                        stroke_gradient.as_ref().and_then(|p| p.previous_keyframe_time(&current_tc))
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
                            layer.get_effect(fx_id).and_then(|fx| {
                                if let Some(prop) = fx.get_param_property(param_name) {
                                    prop.previous_keyframe_time(&current_tc)
                                } else if let Some(prop) = fx.get_color_property(param_name) {
                                    prop.previous_keyframe_time(&current_tc)
                                } else if let Some(prop) = fx.get_bool_property(param_name) {
                                    prop.previous_keyframe_time(&current_tc)
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        }
                    } else if let Some(rest) = prop_path.strip_prefix("mask:") {
                        let parts: Vec<&str> = rest.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let mask_id = parts[0];
                            let param_name = parts[1];
                            if param_name == "path" {
                                layer.get_mask(mask_id)
                                    .and_then(|m| m.path.previous_keyframe_time(&current_tc))
                            } else {
                                layer.get_mask(mask_id)
                                    .and_then(|m| m.get_param_property(param_name))
                                    .and_then(|prop| prop.previous_keyframe_time(&current_tc))
                            }
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
                "text.stroke_color" => {
                    if let LayerSource::Text { ref stroke_color, .. } = layer.source {
                        stroke_color.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "solid.color" => {
                    if let LayerSource::Solid { ref color, .. } = layer.source {
                        color.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "shape.fill" => {
                    if let LayerSource::Shape { ref shape_type } = layer.source {
                        let fill = match shape_type {
                            ShapeType::Rectangle { ref fill, .. }
                            | ShapeType::Ellipse { ref fill, .. }
                            | ShapeType::Path { ref fill, .. } => fill,
                        };
                        fill.next_keyframe_time(&current_tc)
                    } else { None }
                }
                "solid.gradient" => {
                    if let LayerSource::Solid { ref fill_gradient, .. } = layer.source {
                        fill_gradient.as_ref().and_then(|p| p.next_keyframe_time(&current_tc))
                    } else { None }
                }
                "shape.gradient" => {
                    if let LayerSource::Shape { ref shape_type } = layer.source {
                        let slot = match shape_type {
                            ShapeType::Rectangle { ref fill_gradient, .. }
                            | ShapeType::Ellipse { ref fill_gradient, .. }
                            | ShapeType::Path { ref fill_gradient, .. } => fill_gradient,
                        };
                        slot.as_ref().and_then(|p| p.next_keyframe_time(&current_tc))
                    } else { None }
                }
                "text.fill_gradient" => {
                    if let LayerSource::Text { ref fill_gradient, .. } = layer.source {
                        fill_gradient.as_ref().and_then(|p| p.next_keyframe_time(&current_tc))
                    } else { None }
                }
                "text.stroke_gradient" => {
                    if let LayerSource::Text { ref stroke_gradient, .. } = layer.source {
                        stroke_gradient.as_ref().and_then(|p| p.next_keyframe_time(&current_tc))
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
                            layer.get_effect(fx_id).and_then(|fx| {
                                if let Some(prop) = fx.get_param_property(param_name) {
                                    prop.next_keyframe_time(&current_tc)
                                } else if let Some(prop) = fx.get_color_property(param_name) {
                                    prop.next_keyframe_time(&current_tc)
                                } else if let Some(prop) = fx.get_bool_property(param_name) {
                                    prop.next_keyframe_time(&current_tc)
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        }
                    } else if let Some(rest) = prop_path.strip_prefix("mask:") {
                        let parts: Vec<&str> = rest.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let mask_id = parts[0];
                            let param_name = parts[1];
                            if param_name == "path" {
                                layer.get_mask(mask_id)
                                    .and_then(|m| m.path.next_keyframe_time(&current_tc))
                            } else {
                                layer.get_mask(mask_id)
                                    .and_then(|m| m.get_param_property(param_name))
                                    .and_then(|prop| prop.next_keyframe_time(&current_tc))
                            }
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

    /// Select active editing tool.
    pub fn select_tool(&mut self, tool: EditorTool) {
        self.active_tool = tool;
    }

    /// Handle 'M' / 'MM' key shortcut (After Effects parity):
    /// - Pressing 'M' reveals the Mask Path property for the selected layer.
    /// - Pressing 'MM' (double-tap within 350ms) reveals ALL mask properties (Path, Feather, Opacity, Expansion).
    ///
    /// Returns `(revealed, is_all)`.
    pub fn handle_m_shortcut(&mut self) -> (bool, bool) {
        let now = std::time::Instant::now();
        let is_double = if let Some(last) = self.last_m_press_time {
            now.duration_since(last) < std::time::Duration::from_millis(350)
        } else {
            false
        };
        self.last_m_press_time = Some(now);
        if is_double {
            self.timeline_masks_reveal_all = true;
            (true, true)
        } else {
            self.timeline_masks_reveal_path = !self.timeline_masks_reveal_path;
            (true, false)
        }
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
            comp.set_frame_rate(fps);
            let frames = (duration_secs * comp.frame_rate).round() as i64;
            comp.duration = project::TimeCode::from_frames(frames, comp.frame_rate);
        }
        self.sync_clock_to_active_composition();
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
        self.sync_clock_to_active_composition();
        id
    }

    /// Switch active composition by ID.
    pub fn set_active_composition(&mut self, comp_id: &str) {
        if self.project.compositions.iter().any(|c| c.id == comp_id) {
            self.active_comp_id = comp_id.to_string();
            self.selected_layer_id = None;
            self.sync_clock_to_active_composition();
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
                self.sync_clock_to_active_composition();
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
            world *= local;
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
                fill: Property::new("Fill", fill),
                fill_gradient: None,
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
                fill: Property::new("Fill", fill),
                fill_gradient: None,
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
    ///
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
        let (fill, stroke_width) = match self.tool_path_mode {
            PathDrawMode::Fill => (fill, 0.0),
            PathDrawMode::Stroke => (Color::TRANSPARENT, self.tool_path_width),
            PathDrawMode::Both => (fill, self.tool_path_width),
        };
        let layer = Layer::shape(
            &layer_id,
            "Path",
            ShapeType::Path {
                path_data: format!("M {:.1} {:.1}", point.x, point.y),
                fill: Property::new("Fill", fill),
                fill_gradient: None,
                stroke: Property::new("Stroke", self.tool_shape_fill),
                stroke_width: Property::new("Stroke Width", stroke_width),
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
        let layer = self
            .active_composition()
            .ok_or_else(|| "No active composition".to_string())?
            .get_layer(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?
            .clone();
        self.insert_layer_copy(layer, layer_id)
    }

    /// Copy a layer snapshot to the layer clipboard (no mutation).
    pub fn copy_layer(&mut self, layer_id: &str) -> Result<(), String> {
        let layer = self
            .active_composition()
            .ok_or_else(|| "No active composition".to_string())?
            .get_layer(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?
            .clone();
        self.layer_clipboard = Some(layer);
        Ok(())
    }

    /// Paste the copied layer into the active composition (fresh id, top).
    pub fn paste_copied_layer(&mut self) -> Result<String, String> {
        self.checkpoint();
        let layer = self
            .layer_clipboard
            .clone()
            .ok_or_else(|| "Layer clipboard is empty".to_string())?;
        let base_id = layer.id.clone();
        self.insert_layer_copy(layer, &base_id)
    }

    /// Copy an asset id to the asset clipboard (no mutation).
    pub fn copy_asset(&mut self, asset_id: &str) -> Result<(), String> {
        if self.project.get_asset(asset_id).is_none() {
            return Err(format!("Asset {asset_id} not found"));
        }
        self.asset_clipboard = Some(asset_id.to_string());
        Ok(())
    }

    /// Paste the copied asset as a new layer in the active composition.
    pub fn paste_copied_asset(&mut self) -> Result<String, String> {
        let aid = self
            .asset_clipboard
            .clone()
            .ok_or_else(|| "Asset clipboard is empty".to_string())?;
        self.add_asset_layer(&aid)
    }

    /// Insert a layer snapshot with a fresh id + "Copy" name at the top of
    /// the active composition. Shared by duplicate and clipboard paste so
    /// both stay consistent (callers checkpoint first).
    fn insert_layer_copy(&mut self, mut layer: Layer, base_id: &str) -> Result<String, String> {
        let comp = self
            .active_composition()
            .ok_or_else(|| "No active composition".to_string())?;

        let new_id = {
            let mut counter = comp.layers.len() + 1;
            let mut id = format!("{base_id}_copy_{counter}");
            while comp.get_layer(&id).is_some() {
                counter += 1;
                id = format!("{base_id}_copy_{counter}");
            }
            id
        };

        layer.id = new_id.clone();
        layer.name = format!("{} Copy", layer.name);

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .insert_layer(0, layer)
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
        let current_tc = self.clock.timecode();
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Solid { color: c, .. } => {
                c.set_value(color);
                if c.is_animated() {
                    c.add_keyframe(Keyframe::new(current_tc, color));
                }
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
        let current_tc = self.clock.timecode();
        let comp = self.active_composition_mut().ok_or_else(|| "No active composition".to_string())?;
        let layer = comp.get_layer_mut(layer_id).ok_or_else(|| format!("Layer {layer_id} not found"))?;
        match &mut layer.source {
            LayerSource::Text { stroke_color, .. } => {
                stroke_color.set_value(color);
                if stroke_color.is_animated() {
                    stroke_color.add_keyframe(Keyframe::new(current_tc, color));
                }
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

/// Parse a numeric string or arithmetic expression into a final float value.
/// Supports:
/// - Plain numbers (e.g. "120", "-45.5")
/// - Unit suffixes (e.g. "75%", "100px", "45deg", "45°", "2s", "10f")
/// - Thousand commas (e.g. "1,920")
/// - Arithmetic expressions (e.g. "1920/2", "100 + 50", "50 * 2.5", "(100 + 20) * 3")
/// - Relative modifiers against `current` value (e.g. "+=50", "-=20", "*=2", "/=2", "*2", "/2")
pub fn parse_numeric_expression(text: &str, current: Option<f32>) -> Option<f32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Check for relative modifiers first
    if let Some(rest) = trimmed.strip_prefix("+=") {
        let delta = parse_numeric_expression(rest, None)?;
        return Some(current.unwrap_or(0.0) + delta);
    }
    if let Some(rest) = trimmed.strip_prefix("-=") {
        let delta = parse_numeric_expression(rest, None)?;
        return Some(current.unwrap_or(0.0) - delta);
    }
    if let Some(rest) = trimmed.strip_prefix("*=") {
        let factor = parse_numeric_expression(rest, None)?;
        return Some(current.unwrap_or(1.0) * factor);
    }
    if let Some(rest) = trimmed.strip_prefix("/=") {
        let divisor = parse_numeric_expression(rest, None)?;
        if divisor.abs() < 1e-9 {
            return None;
        }
        return Some(current.unwrap_or(0.0) / divisor);
    }
    if let Some(rest) = trimmed.strip_prefix('*') {
        let factor = parse_numeric_expression(rest, None)?;
        return Some(current.unwrap_or(1.0) * factor);
    }
    if let Some(rest) = trimmed.strip_prefix('/') {
        let divisor = parse_numeric_expression(rest, None)?;
        if divisor.abs() < 1e-9 {
            return None;
        }
        return Some(current.unwrap_or(0.0) / divisor);
    }

    // Clean common units and punctuation
    let mut s = trimmed.to_lowercase().replace(',', "");
    let suffixes = ["px", "pt", "deg", "°", "rad", "fps", "frames", "frame", "sec", "s", "f", "%"];
    for suffix in suffixes {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.trim_end().to_string();
            break;
        }
    }

    // Try evaluating as arithmetic expression
    if let Some(v) = evaluate_math_expression(&s) {
        return Some(v);
    }

    // Fallback: take leading numeric characters only if text does not contain operator symbols
    // (to prevent invalid expressions like "100 / 0" from being truncated to 100)
    if !s.contains('/') && !s.contains('*') && !s.contains('(') && !s.contains(')') {
        let numeric: String = s
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+' || *c == 'e' || *c == 'E')
            .collect();
        return numeric.parse::<f32>().ok();
    }

    None
}

#[derive(Debug, PartialEq, Clone)]
enum MathToken {
    Num(f32),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

fn tokenize_math(input: &str) -> Option<Vec<MathToken>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '+' => {
                tokens.push(MathToken::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(MathToken::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(MathToken::Star);
                i += 1;
            }
            '/' => {
                tokens.push(MathToken::Slash);
                i += 1;
            }
            '(' => {
                tokens.push(MathToken::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(MathToken::RParen);
                i += 1;
            }
            '0'..='9' | '.' => {
                let start = i;
                let mut dot_seen = c == '.';
                i += 1;
                while i < chars.len() {
                    let next_c = chars[i];
                    if next_c.is_ascii_digit() {
                        i += 1;
                    } else if next_c == '.' && !dot_seen {
                        dot_seen = true;
                        i += 1;
                    } else if (next_c == 'e' || next_c == 'E') && i + 1 < chars.len() {
                        i += 1;
                        if chars[i] == '+' || chars[i] == '-' {
                            i += 1;
                        }
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                        break;
                    } else {
                        break;
                    }
                }
                let num_str: String = chars[start..i].iter().collect();
                let num = num_str.parse::<f32>().ok()?;
                tokens.push(MathToken::Num(num));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

struct ExprParser<'a> {
    tokens: &'a [MathToken],
    pos: usize,
}

impl<'a> ExprParser<'a> {
    fn new(tokens: &'a [MathToken]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&MathToken> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<&MathToken> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_expression(&mut self) -> Option<f32> {
        let mut left = self.parse_term()?;
        while let Some(op) = self.peek() {
            match op {
                MathToken::Plus => {
                    self.next();
                    let right = self.parse_term()?;
                    left += right;
                }
                MathToken::Minus => {
                    self.next();
                    let right = self.parse_term()?;
                    left -= right;
                }
                _ => break,
            }
        }
        Some(left)
    }

    fn parse_term(&mut self) -> Option<f32> {
        let mut left = self.parse_factor()?;
        while let Some(op) = self.peek() {
            match op {
                MathToken::Star => {
                    self.next();
                    let right = self.parse_factor()?;
                    left *= right;
                }
                MathToken::Slash => {
                    self.next();
                    let right = self.parse_factor()?;
                    if right.abs() < 1e-9 {
                        return None;
                    }
                    left /= right;
                }
                _ => break,
            }
        }
        Some(left)
    }

    fn parse_factor(&mut self) -> Option<f32> {
        match self.peek() {
            Some(MathToken::Minus) => {
                self.next();
                let val = self.parse_factor()?;
                Some(-val)
            }
            Some(MathToken::Plus) => {
                self.next();
                self.parse_factor()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Option<f32> {
        match self.next()? {
            MathToken::Num(n) => Some(*n),
            MathToken::LParen => {
                let val = self.parse_expression()?;
                if self.next() != Some(&MathToken::RParen) {
                    return None;
                }
                Some(val)
            }
            _ => None,
        }
    }
}

fn evaluate_math_expression(input: &str) -> Option<f32> {
    let tokens = tokenize_math(input)?;
    if tokens.is_empty() {
        return None;
    }
    let mut parser = ExprParser::new(&tokens);
    let result = parser.parse_expression()?;
    if parser.pos == tokens.len() {
        Some(result)
    } else {
        None
    }
}
