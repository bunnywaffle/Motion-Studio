use compositor::{EvaluatedStack, LayerStackEvaluator, SceneGraph};
use project::{
    Asset, Color, Composition, Effect, EffectType, Keyframe, KeyframeTangent, Layer, PlaybackClock,
    Project, TimeCode, Vec2,
};
use std::path::PathBuf;
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

    /// Import a media file from disk (image or video), register it in project assets,
    /// and add a new centered layer to the active composition.
    pub fn import_media_file(&mut self, path: PathBuf) -> Result<String, String> {
        let (comp_w, comp_h, frame_rate, duration) = {
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
            .map_err(|e| format!("Failed to add media layer: {e:?}"))?;

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

    /// Remove an effect by ID from the currently selected layer.
    pub fn remove_effect_from_selected_layer(&mut self, effect_id: &str) -> Result<(), String> {
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

        layer
            .remove_effect(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        Ok(())
    }

    /// Toggle enabled state of an effect on the currently selected layer.
    pub fn toggle_effect_enabled(&mut self, effect_id: &str) -> Result<(), String> {
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
        effect.toggle_enabled();
        Ok(())
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

        let comp = self
            .active_composition_mut()
            .ok_or_else(|| "No active composition".to_string())?;
        let layer = comp
            .get_layer_mut(&selected_id)
            .ok_or_else(|| format!("Layer {selected_id} not found"))?;

        let effect = layer
            .get_effect_mut(effect_id)
            .ok_or_else(|| format!("Effect {effect_id} not found on layer"))?;
        if effect.nudge_param(param_name, delta) {
            Ok(())
        } else {
            Err(format!(
                "Parameter {param_name} not found on effect {effect_id}"
            ))
        }
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
