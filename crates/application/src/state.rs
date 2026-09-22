use compositor::{EvaluatedStack, LayerStackEvaluator, SceneGraph};
use gpui_kit::component::input::InputState;
use gpui_kit::{Entity, Subscription};
use project::{
    Asset, BlendMode, Color, Composition, Effect, EffectType, Keyframe, KeyframeTangent, Layer,
    LayerSource, PlaybackClock, Project, Property, ShapeType, TimeCode, TrackMatteMode, Vec2,
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
    /// Scrub key currently open for After Effects-style keyboard entry.
    pub value_edit_key: Option<String>,
    /// Live single-line editor for `value_edit_key` (created on demand).
    pub value_editor: Option<Entity<InputState>>,
    pub value_editor_sub: Option<Subscription>,
    evaluator: LayerStackEvaluator,
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
            value_edit_key: None,
            value_editor: None,
            value_editor_sub: None,
            evaluator: LayerStackEvaluator::new(),
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

    /// Validate and apply new Shader Lab source on the selected layer.
    /// Success swaps in the source (UI regenerates); failure keeps the
    /// last-good source running and records the error for the panel.
    pub fn apply_shader_source(
        &mut self,
        effect_id: &str,
        source: &str,
    ) -> Result<(), String> {
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
    pub fn end_value_edit_state(&mut self) -> bool {
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

    /// Toggle play/pause transport state.
    pub fn toggle_playback(&mut self) {
        self.is_playing = !self.is_playing;
        if self.is_playing {
            self.clock.play();
        } else {
            self.clock.pause();
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
        let sel_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.move_layer_up(&sel_id)
    }

    /// Move the currently selected layer down in the stack (away from index 0, lower visually).
    pub fn move_selected_layer_down(&mut self) -> Result<(), String> {
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
        let selected_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.toggle_layer_effect_enabled(&selected_id, effect_id)
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
                                shape_type: ShapeType::Rectangle { width, height, corner_radius },
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
                                shape_type: ShapeType::Ellipse { radius_x, radius_y },
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
    pub fn toggle_layer_keyframe_at_current_time(&mut self, layer_id: &str, prop_path: &str) {
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

    /// Set blend mode on the specified layer.
    pub fn set_layer_blend_mode(&mut self, layer_id: &str, mode: BlendMode) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.blend_mode = mode;
            }
        }
    }

    /// Set track matte mode and optional target matte layer ID on the specified layer.
    pub fn set_layer_track_matte(&mut self, layer_id: &str, mode: TrackMatteMode, target_id: Option<String>) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.set_matte(mode, target_id);
            }
        }
    }

    /// Set parent layer ID on the specified layer.
    pub fn set_layer_parent(&mut self, layer_id: &str, parent_id: Option<String>) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.set_parent(parent_id);
            }
        }
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
        if let Some(id) = self.selected_layer_id.clone() {
            self.toggle_layer_visibility(&id);
        }
    }

    /// Toggle solo state of the currently selected layer.
    pub fn toggle_selected_layer_solo(&mut self) {
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
        let mut layer = Layer::text(
            &layer_id,
            if text.is_empty() { "Text Layer" } else { text },
            text,
            Self::default_font_family(),
            48.0,
            Color::WHITE,
            in_pt,
            out_pt,
        );

        let target_pos = pos.unwrap_or(Vec2::ZERO);
        layer.transform.position.set_value(target_pos);
        // Center the text block on the spawn point: anchor at half of the
        // estimated block size so position (0, 0) lands it in the viewport
        // center like every other new layer (mirrors the viewer estimate).
        let est_w = (text.chars().count().max(1) as f32 * 48.0 * 0.6 + 40.0).max(100.0);
        let est_h: f32 = (48.0f32 * 1.4 + 20.0).max(40.0);
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
        let mut layer = Layer::shape(
            &layer_id,
            "Rectangle Shape",
            ShapeType::Rectangle {
                width: Property::new("Width", width),
                height: Property::new("Height", height),
                corner_radius: Property::new("Corner Radius", 0.0),
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
        let mut layer = Layer::shape(
            &layer_id,
            "Ellipse Shape",
            ShapeType::Ellipse {
                radius_x: Property::new("Radius X", radius_x),
                radius_y: Property::new("Radius Y", radius_y),
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

    /// Add a vector path point using the Pen tool. If the currently selected layer is a Path shape,
    /// appends the vertex; otherwise creates a new vector Path layer starting at `point`.
    pub fn add_pen_point(&mut self, point: Vec2) -> Result<String, String> {
        // Check if selected layer is a Path shape
        let sel_id = self.selected_layer_id.clone();
        if let Some(id) = sel_id {
            if let Some(comp) = self.active_composition_mut() {
                if let Some(layer) = comp.get_layer_mut(&id) {
                    if let LayerSource::Shape { shape_type: ShapeType::Path { path_data } } = &mut layer.source {
                        path_data.push_str(&format!(" L {:.1} {:.1}", point.x, point.y));
                        return Ok(id);
                    }
                }
            }
        }

        // Otherwise, create a new Path layer
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
        let layer = Layer::shape(
            &layer_id,
            "Pen Path",
            ShapeType::Path {
                path_data: format!("M {:.1} {:.1}", point.x, point.y),
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

    /// Duplicate the specified layer in the active composition.
    pub fn duplicate_layer(&mut self, layer_id: &str) -> Result<String, String> {
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
        let sel_id = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| "No layer selected".to_string())?;
        self.duplicate_layer(&sel_id)
    }

    /// Reset transform properties of the specified layer to defaults.
    pub fn reset_layer_transform(&mut self, layer_id: &str) {
        if let Some(comp) = self.active_composition_mut() {
            if let Some(layer) = comp.get_layer_mut(layer_id) {
                layer.transform = project::Transform::default();
                layer.transform.position.set_value(Vec2::ZERO);
                layer.opacity.set_value(100.0);
            }
        }
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

    /// Set dimensions and corner radius on a Rectangle shape layer.
    pub fn set_layer_rect_dimensions(
        &mut self,
        layer_id: &str,
        width: f32,
        height: f32,
        corner_radius: f32,
    ) -> Result<(), String> {
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Rectangle { width: w, height: h, corner_radius: cr },
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
                shape_type: ShapeType::Rectangle { width: w, height: h, corner_radius: cr },
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
        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(layer_id)
            .ok_or_else(|| format!("Layer {layer_id} not found"))?;

        match &mut layer.source {
            LayerSource::Shape {
                shape_type: ShapeType::Ellipse { radius_x: rx, radius_y: ry },
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
                shape_type: ShapeType::Ellipse { radius_x: rx, radius_y: ry },
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
