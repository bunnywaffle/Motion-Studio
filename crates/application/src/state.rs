use compositor::{EvaluatedStack, LayerStackEvaluator, SceneGraph};
use project::{
    Color, Composition, Keyframe, KeyframeTangent, Layer, PlaybackClock, Project, TimeCode, Vec2,
};
use std::time::Duration;

/// Central application editor state managing the active project, playback clock,
/// layer selection, and composition evaluation.
pub struct EditorState {
    pub project: Project,
    pub active_comp_id: String,
    pub clock: PlaybackClock,
    pub selected_layer_id: Option<String>,
    pub is_playing: bool,
    evaluator: LayerStackEvaluator,
}

impl EditorState {
    /// Create a new EditorState pre-seeded with a starter composition and animated demo layers.
    pub fn new() -> Self {
        let mut project = Project::new("proj_default", "Motion Studio Project");
        let fps = 30.0;
        let duration_secs = 5.0;
        let mut comp = Composition::hd_1080p_30fps("comp_main", "Main Composition", duration_secs);

        let tc0 = TimeCode::from_frames(0, fps);
        let tc150 = TimeCode::from_frames(150, fps);

        // Layer 1: Dark background canvas solid
        let bg_solid = Layer::solid(
            "layer_bg",
            "Background Solid",
            Color::from_hex("#121316").unwrap_or(Color::BLACK),
            1920,
            1080,
            tc0,
            tc150,
        );

        // Layer 2: Animated accent solid with Position and Rotation keyframes
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
        accent.transform.position.set_value(Vec2::new(960.0, 540.0));

        // Position animation: horizontal sway
        accent.transform.position.add_keyframe(Keyframe::bezier(
            TimeCode::from_frames(0, fps),
            Vec2::new(760.0, 540.0),
            None,
            Some(KeyframeTangent::ease_in_out_out()),
        ));
        accent.transform.position.add_keyframe(Keyframe::bezier(
            TimeCode::from_frames(60, fps),
            Vec2::new(1160.0, 540.0),
            Some(KeyframeTangent::ease_in_out_in()),
            Some(KeyframeTangent::ease_in_out_out()),
        ));
        accent.transform.position.add_keyframe(Keyframe::bezier(
            TimeCode::from_frames(120, fps),
            Vec2::new(760.0, 540.0),
            Some(KeyframeTangent::ease_in_out_in()),
            None,
        ));

        // Rotation animation: continuous spin
        accent.transform.rotation.add_keyframe(Keyframe::linear(TimeCode::from_frames(0, fps), 0.0));
        accent.transform.rotation.add_keyframe(Keyframe::linear(TimeCode::from_frames(120, fps), 360.0));

        // Layer 3: Title badge with opacity fade-in
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
        title_card.transform.position.set_value(Vec2::new(960.0, 780.0));
        title_card.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(15, fps), 0.0));
        title_card.opacity.add_keyframe(Keyframe::linear(TimeCode::from_frames(45, fps), 100.0));

        comp.add_layer(bg_solid).unwrap();
        comp.add_layer(accent).unwrap();
        comp.add_layer(title_card).unwrap();

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
            evaluator: LayerStackEvaluator::new(),
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
    pub fn add_solid_layer(
        &mut self,
        name: &str,
        color: Color,
        width: u32,
        height: u32,
    ) -> Result<String, String> {
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

        let mut layer = Layer::solid(&id, name, color, width, height, in_pt, out_pt);
        layer.transform.position.set_value(Vec2::new(
            (comp_w / 2) as f32,
            (comp_h / 2) as f32,
        ));
        layer.transform.anchor_point.set_value(Vec2::new(
            (width / 2) as f32,
            (height / 2) as f32,
        ));

        let comp_mut = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        comp_mut
            .add_layer(layer)
            .map_err(|e| format!("Failed to add layer: {e:?}"))?;

        self.selected_layer_id = Some(id.clone());
        Ok(id)
    }

    /// Nudge position of the selected layer by `(dx, dy)`.
    pub fn nudge_position(&mut self, dx: f32, dy: f32) {
        let delta = Vec2::new(dx, dy);
        if let Some(layer) = self.selected_layer_mut() {
            let current = layer.transform.position.value;
            layer.transform.position.set_value(current + delta);
            for kf in layer.transform.position.keyframes_mut() {
                kf.value += delta;
            }
        }
    }

    /// Nudge anchor point of the selected layer by `(dx, dy)`.
    pub fn nudge_anchor(&mut self, dx: f32, dy: f32) {
        let delta = Vec2::new(dx, dy);
        if let Some(layer) = self.selected_layer_mut() {
            let current = layer.transform.anchor_point.value;
            layer.transform.anchor_point.set_value(current + delta);
            for kf in layer.transform.anchor_point.keyframes_mut() {
                kf.value += delta;
            }
        }
    }

    /// Nudge scale of the selected layer by `(dx, dy)` percent.
    pub fn nudge_scale(&mut self, dx: f32, dy: f32) {
        let delta = Vec2::new(dx, dy);
        if let Some(layer) = self.selected_layer_mut() {
            let current = layer.transform.scale.value;
            layer.transform.scale.set_value(current + delta);
            for kf in layer.transform.scale.keyframes_mut() {
                kf.value += delta;
            }
        }
    }

    /// Nudge rotation of the selected layer by `ddeg` degrees.
    pub fn nudge_rotation(&mut self, ddeg: f32) {
        if let Some(layer) = self.selected_layer_mut() {
            let current = layer.transform.rotation.value;
            layer.transform.rotation.set_value(current + ddeg);
            for kf in layer.transform.rotation.keyframes_mut() {
                kf.value += ddeg;
            }
        }
    }

    /// Nudge opacity of the selected layer by `dop` percent.
    pub fn nudge_opacity(&mut self, dop: f32) {
        if let Some(layer) = self.selected_layer_mut() {
            let current = layer.opacity.value;
            let new_op = (current + dop).clamp(0.0, 100.0);
            layer.opacity.set_value(new_op);
            for kf in layer.opacity.keyframes_mut() {
                kf.value = (kf.value + dop).clamp(0.0, 100.0);
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
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}
