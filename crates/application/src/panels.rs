use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex, StyledExt, TestSupportExt};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use crate::state::EditorState;
use project::{Color, EffectType, LayerSource};

fn icon_box(icon: IconName) -> Div {
    div().w(px(14.)).h(px(14.)).flex().items_center().justify_center().child(icon)
}

fn step_button<F>(label: &'static str, cx: &App, on_click: F) -> impl IntoElement
where
    F: Fn(&mut App) + 'static,
{
    div()
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(cx.theme().muted)
        .hover(|s| s.bg(cx.theme().accent))
        .text_color(cx.theme().foreground)
        .text_xs()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            on_click(cx);
        })
        .child(label)
}

fn step_button_with_id<F>(
    id: impl Into<ElementId>,
    label: &'static str,
    cx: &App,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut App) + 'static,
{
    div()
        .id(id)
        .test_support()
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(cx.theme().muted)
        .hover(|s| s.bg(cx.theme().accent))
        .text_color(cx.theme().foreground)
        .text_xs()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            on_click(cx);
        })
        .child(label)
}

// --- 1. Project Panel ---

pub struct ProjectPanel {
    focus_handle: FocusHandle,
    state: Entity<EditorState>,
    _subscription: Subscription,
}

impl ProjectPanel {
    pub fn new(state: Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&state, |_this, _state, cx| {
            cx.notify();
        });
        Self {
            focus_handle: cx.focus_handle(),
            state,
            _subscription,
        }
    }

    pub fn standalone(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new(state, cx)
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn state(&self) -> &Entity<EditorState> {
        &self.state
    }
}

impl EventEmitter<PanelEvent> for ProjectPanel {}

impl Focusable for ProjectPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ProjectPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let comp_opt = state.active_composition();
        let add_state = self.state.clone();

        let asset_items: Vec<Div> = match comp_opt {
            Some(comp) => {
                let mut items = Vec::new();

                // 1. Active Composition item
                items.push(
                    h_flex()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .text_xs()
                        .items_center()
                        .child(
                            h_flex()
                                .w(px(110.))
                                .gap_1()
                                .items_center()
                                .font_semibold()
                                .child(icon_box(IconName::Folder))
                                .child(comp.name.clone()),
                        )
                        .child(div().w(px(70.)).child("Composition"))
                        .child(div().w(px(70.)).child(format!("{}x{}", comp.width, comp.height)))
                        .child(div().flex_1().child(format!("{}", comp.duration))),
                );

                // 2. Imported project assets (Image, Video, Audio, etc.)
                for asset in &state.project.assets {
                    let (type_str, icon) = match &asset.asset_type {
                        project::AssetType::Image => ("Image", IconName::Image),
                        project::AssetType::Video => ("Video", IconName::Film),
                        project::AssetType::Audio => ("Audio", IconName::Music),
                        project::AssetType::Vector => ("Vector", IconName::Folder),
                        project::AssetType::Font => ("Font", IconName::Type),
                        project::AssetType::Other(_) => ("Media", IconName::Layers),
                    };

                    let path_str = asset.path.to_string_lossy().to_string();
                    let display_name = asset
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&asset.name)
                        .to_string();

                    items.push(
                        h_flex()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .items_center()
                            .text_color(cx.theme().foreground)
                            .hover(|s| s.bg(cx.theme().muted))
                            .child(
                                h_flex()
                                    .w(px(110.))
                                    .gap_1()
                                    .items_center()
                                    .child(icon_box(icon))
                                    .child(display_name),
                            )
                            .child(div().w(px(70.)).child(type_str))
                            .child(div().w(px(70.)).child("-"))
                            .child(div().flex_1().child(path_str)),
                    );
                }

                // 3. Layers within active composition
                for layer in &comp.layers {
                    let is_selected = state.selected_layer_id.as_deref() == Some(&layer.id);
                    let type_str = match &layer.source {
                        LayerSource::Solid { .. } => "Solid",
                        LayerSource::Image { .. } => "Image",
                        LayerSource::Video { .. } => "Video",
                        LayerSource::Text { .. } => "Text",
                        LayerSource::Shape { .. } => "Shape",
                        LayerSource::NestedComposition { .. } => "Pre-comp",
                        _ => "Procedural",
                    };
                    let res_str = match &layer.source {
                        LayerSource::Solid { width, height, .. } => format!("{width}x{height}"),
                        _ => "-".to_string(),
                    };

                    let sel_state = self.state.clone();
                    let lid = layer.id.clone();
                    let icon_elem = match &layer.source {
                        LayerSource::Solid { .. } => icon_box(IconName::Layers),
                        LayerSource::Image { .. } => icon_box(IconName::Image),
                        LayerSource::Video { .. } => icon_box(IconName::Film),
                        LayerSource::Text { .. } => icon_box(IconName::Type),
                        LayerSource::Shape { .. } => icon_box(IconName::Sparkles),
                        LayerSource::NestedComposition { .. } => icon_box(IconName::Folder),
                        _ => icon_box(IconName::Layers),
                    };

                    let mut row = h_flex()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .text_xs()
                        .items_center()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            sel_state.update(cx, |s, cx| {
                                s.select_layer(Some(lid.clone()));
                                cx.notify();
                            });
                        })
                        .child(
                            h_flex()
                                .w(px(110.))
                                .gap_1()
                                .items_center()
                                .child(icon_elem)
                                .child(layer.name.clone()),
                        )
                        .child(div().w(px(70.)).child(type_str))
                        .child(div().w(px(70.)).child(res_str))
                        .child(div().flex_1().child(format!("{} - {}", layer.in_point, layer.out_point)));

                    if is_selected {
                        row = row
                            .bg(cx.theme().accent)
                            .text_color(cx.theme().accent_foreground);
                    } else {
                        row = row
                            .text_color(cx.theme().foreground)
                            .hover(|s| s.bg(cx.theme().muted));
                    }
                    items.push(row);
                }

                items
            }
            None => Vec::new(),
        };

        let import_state = self.state.clone();

        div()
            .id("project_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Header / search bar & actions
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .text_xs()
                            .child("Search..."),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .id("add_solid_button")
                                    .test_support()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .hover(|s| s.bg(cx.theme().accent))
                                    .text_color(cx.theme().foreground)
                                    .text_xs()
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        add_state.update(cx, |s, cx| {
                                            let color = Color::from_rgba_u8(245, 158, 11, 255);
                                            let _ = s.add_solid_layer("New Solid", color, 400, 400);
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::Plus))
                                    .child("Solid"),
                            )
                            .child(
                                div()
                                    .id("import_media_button")
                                    .test_support()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .hover(|s| s.bg(cx.theme().accent))
                                    .text_color(cx.theme().foreground)
                                    .text_xs()
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        let file = rfd::FileDialog::new()
                                            .add_filter("Media Files", &["png", "jpg", "jpeg", "mp4", "mov", "webm"])
                                            .pick_file();
                                        if let Some(path) = file {
                                            import_state.update(cx, |s, cx| {
                                                let _ = s.import_media_file(path);
                                                cx.notify();
                                            });
                                        }
                                    })
                                    .child(icon_box(IconName::FolderOpen))
                                    .child("Import Media..."),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .text_color(cx.theme().foreground)
                                    .text_xs()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(icon_box(IconName::Folder))
                                    .child("Comp"),
                            ),
                    ),
            )
            // Column headers
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(110.)).child("Name"))
                    .child(div().w(px(70.)).child("Type"))
                    .child(div().w(px(70.)).child("Resolution"))
                    .child(div().flex_1().child("Duration")),
            )
            // Asset media list
            .child(
                v_flex()
                    .id("project_assets")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .px_1()
                    .py_1()
                    .children(asset_items),
            )
            // Footer
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(match comp_opt {
                        Some(comp) => format!(
                            "{} items • {} selected • {:.2} fps",
                            comp.layers.len() + 1,
                            if state.selected_layer_id.is_some() { 1 } else { 0 },
                            comp.frame_rate
                        ),
                        None => "0 items".to_string(),
                    }),
            )
    }
}

impl BasePanel for ProjectPanel {
    fn panel_name(&self) -> &'static str {
        "project"
    }
}

impl Panel for ProjectPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Project".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Project"
    }
}

// --- 2. Composition Viewer Panel ---

pub type CompositionPanel = CompositionViewerPanel;

pub struct CompositionViewerPanel {
    focus_handle: FocusHandle,
    state: Entity<EditorState>,
    _subscription: Subscription,
}

impl CompositionViewerPanel {
    pub fn new(state: Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&state, |_this, _state, cx| {
            cx.notify();
        });
        Self {
            focus_handle: cx.focus_handle(),
            state,
            _subscription,
        }
    }

    pub fn standalone(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new(state, cx)
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn state(&self) -> &Entity<EditorState> {
        &self.state
    }
}

impl EventEmitter<PanelEvent> for CompositionViewerPanel {}

impl Focusable for CompositionViewerPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CompositionViewerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let comp_opt = state.active_composition();
        let eval_stack = state.evaluate_current_frame().ok();

        let comp_name = comp_opt.map(|c| c.name.clone()).unwrap_or_else(|| "No Comp".to_string());
        let comp_res = comp_opt.map(|c| format!("{} x {} (1.00)", c.width, c.height)).unwrap_or_default();
        let comp_fps = comp_opt.map(|c| format!("{:.2} fps", c.frame_rate)).unwrap_or_default();
        let current_tc = format!("{}", state.clock.timecode());
        let current_frame = state.clock.current_frame();

        let bg_color = comp_opt
            .map(|c| Rgba { r: c.background_color.r, g: c.background_color.g, b: c.background_color.b, a: c.background_color.a })
            .unwrap_or(Rgba { r: 0.07, g: 0.07, b: 0.08, a: 1.0 });

        // Canvas viewport dimensions
        let canvas_w = 512.0f32;
        let canvas_h = 288.0f32;
        let comp_w = comp_opt.map(|c| c.width as f32).unwrap_or(1920.0);
        let comp_h = comp_opt.map(|c| c.height as f32).unwrap_or(1080.0);
        let scale_x = canvas_w / comp_w;
        let scale_y = canvas_h / comp_h;

        // Render evaluated layers in painter's composite order
        let rendered_layers: Vec<AnyElement> = match eval_stack.as_ref() {
            Some(stack) => stack
                .render_layers()
                .into_iter()
                .map(|layer| {
                    let (base_w, base_h, col) = match &layer.source {
                        LayerSource::Solid {
                            width,
                            height,
                            color,
                        } => (*width as f32, *height as f32, *color),
                        _ => (400.0, 300.0, Color::WHITE),
                    };

                    let bbox = layer.world_bounds(base_w, base_h);
                    let l_x = bbox.min.x * scale_x;
                    let l_y = bbox.min.y * scale_y;
                    let l_w = ((bbox.max.x - bbox.min.x) * scale_x).max(2.0);
                    let l_h = ((bbox.max.y - bbox.min.y) * scale_y).max(2.0);
                    let is_selected = state.selected_layer_id.as_deref() == Some(&layer.id);

                    let sel_state = self.state.clone();
                    let lid = layer.id.clone();

                    let mut layer_el = div()
                        .id(ElementId::Name(format!("canvas_layer_{}", layer.id).into()))
                        .test_support()
                        .absolute()
                        .left(px(l_x))
                        .top(px(l_y))
                        .w(px(l_w))
                        .h(px(l_h))
                        .bg(Rgba {
                            r: col.r,
                            g: col.g,
                            b: col.b,
                            a: col.a * layer.effective_opacity.clamp(0.0, 1.0),
                        })
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            sel_state.update(cx, |s, cx| {
                                s.select_layer(Some(lid.clone()));
                                cx.notify();
                            });
                        });

                    if is_selected {
                        layer_el = layer_el
                            .border_2()
                            .border_color(rgb(0x3b82f6)); // accent selection border
                    }

                    layer_el.into_any_element()
                })
                .collect(),
            None => Vec::new(),
        };

        div()
            .id("composition_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Viewport header / controls
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(div().font_bold().child(comp_name))
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(comp_res),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(comp_fps),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .child("100% (Fit)"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .child("Full Res"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .child("Active Camera"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .child("RGB"),
                            ),
                    ),
            )
            // Composition Canvas area
            .child(
                v_flex()
                    .id("composition_viewer")
                    .test_support()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .p_4()
                    .overflow_hidden()
                    .child(
                        // 16:9 canvas frame
                        div()
                            .w(px(canvas_w))
                            .h(px(canvas_h))
                            .border_2()
                            .border_color(cx.theme().border)
                            .bg(bg_color)
                            .rounded_sm()
                            .relative()
                            .overflow_hidden()
                            .children(rendered_layers),
                    ),
            )
            // Status bar
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .justify_between()
                    .child(div().child(format!("Time: {} (Frame {})", current_tc, current_frame)))
                    .child(div().child("Scroll to Zoom • Space to Play/Pause")),
            )
    }
}

impl BasePanel for CompositionViewerPanel {
    fn panel_name(&self) -> &'static str {
        "composition"
    }
}

impl Panel for CompositionViewerPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Composition".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Composition"
    }
}

// --- 3. Properties Panel ---

pub struct PropertiesPanel {
    focus_handle: FocusHandle,
    state: Entity<EditorState>,
    _subscription: Subscription,
}

impl PropertiesPanel {
    pub fn new(state: Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&state, |_this, _state, cx| {
            cx.notify();
        });
        Self {
            focus_handle: cx.focus_handle(),
            state,
            _subscription,
        }
    }

    pub fn standalone(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new(state, cx)
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn state(&self) -> &Entity<EditorState> {
        &self.state
    }
}

impl EventEmitter<PanelEvent> for PropertiesPanel {}

impl Focusable for PropertiesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PropertiesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let selected_layer = state.selected_layer();
        let eval_stack = state.evaluate_current_frame().ok();

        let (header_title, layer_type_title) = match selected_layer {
            Some(l) => {
                let type_str = match &l.source {
                    LayerSource::Solid { .. } => "Solid Layer",
                    LayerSource::Image { .. } => "Image Layer",
                    LayerSource::Video { .. } => "Video Layer",
                    LayerSource::Text { .. } => "Text Layer",
                    LayerSource::Shape { .. } => "Shape Layer",
                    LayerSource::NestedComposition { .. } => "Pre-comp Layer",
                    _ => "2D Layer",
                };
                (format!("Selected: {}", l.name), type_str.to_string())
            }
            None => ("No Layer Selected".to_string(), "-".to_string()),
        };

        div()
            .id("properties_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Header
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(icon_box(IconName::Layers))
                            .child(div().font_semibold().text_xs().child(header_title)),
                    )
                    .child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_xs()
                            .child(layer_type_title),
                    ),
            )
            // Inspector fields
            .child(
                v_flex()
                    .id("properties_inspector")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .p_3()
                    .gap_3()
                    .children(if let Some(layer) = selected_layer {
                        let eval_layer = eval_stack.as_ref().and_then(|s| s.get_layer(&layer.id));
                        let anchor = eval_layer.map(|l| l.transform.anchor_point).unwrap_or(layer.transform.anchor_point.value);
                        let pos = eval_layer.map(|l| l.transform.position).unwrap_or(layer.transform.position.value);
                        let sc = eval_layer.map(|l| l.transform.scale).unwrap_or(layer.transform.scale.value);
                        let rot = eval_layer.map(|l| l.transform.rotation).unwrap_or(layer.transform.rotation.value);
                        let current_tc = state.clock.timecode();
                        let op = layer.opacity.evaluate_at(&current_tc).clamp(0.0, 100.0);

                        let s_anchor_mx = self.state.clone();
                        let s_anchor_px = self.state.clone();
                        let s_anchor_my = self.state.clone();
                        let s_anchor_py = self.state.clone();

                        let s_pos_mx = self.state.clone();
                        let s_pos_px = self.state.clone();
                        let s_pos_my = self.state.clone();
                        let s_pos_py = self.state.clone();

                        let s_scale_mx = self.state.clone();
                        let s_scale_px = self.state.clone();
                        let s_scale_my = self.state.clone();
                        let s_scale_py = self.state.clone();

                        let s_rot_m = self.state.clone();
                        let s_rot_p = self.state.clone();

                        let s_op_m = self.state.clone();
                        let s_op_p = self.state.clone();

                        let s_vis = self.state.clone();
                        let s_solo = self.state.clone();

                        let effects_list = if layer.effects.is_empty() {
                            div()
                                .id("no_effects_applied")
                                .test_support()
                                .p_2()
                                .rounded_sm()
                                .bg(cx.theme().secondary)
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("No effects applied. Click an effect in the Effects tab to apply.")
                        } else {
                            let mut fx_col = v_flex().id("applied_effects_list").test_support().gap_2();
                            for effect in &layer.effects {
                                let eff_id = effect.id.clone();
                                let eff_id_toggle = effect.id.clone();
                                let eff_id_del = effect.id.clone();
                                let s_toggle = self.state.clone();
                                let s_del = self.state.clone();

                                let mut effect_box = v_flex()
                                    .id(SharedString::from(format!("applied_effect_{}", effect.id)))
                                    .test_support()
                                    .p_2()
                                    .rounded_sm()
                                    .bg(cx.theme().secondary)
                                    .gap_1p5();

                                let header_row = h_flex()
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .child(
                                        h_flex()
                                            .gap_1p5()
                                            .items_center()
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!("effect_toggle_{}", effect.id)))
                                                    .test_support()
                                                    .cursor_pointer()
                                                    .w(px(14.))
                                                    .h(px(14.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_color(if effect.enabled {
                                                        cx.theme().foreground
                                                    } else {
                                                        cx.theme().muted_foreground
                                                    })
                                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                        s_toggle.update(cx, |s, cx| {
                                                            let _ = s.toggle_effect_enabled(&eff_id_toggle);
                                                            cx.notify();
                                                        });
                                                    })
                                                    .child(if effect.enabled { icon_box(IconName::Eye) } else { icon_box(IconName::EyeOff) }),
                                            )
                                            .child(
                                                div()
                                                    .font_semibold()
                                                    .text_color(if effect.enabled {
                                                        cx.theme().foreground
                                                    } else {
                                                        cx.theme().muted_foreground
                                                    })
                                                    .child(effect.name.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("effect_delete_{}", effect.id)))
                                            .test_support()
                                            .cursor_pointer()
                                            .w(px(14.))
                                            .h(px(14.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_color(cx.theme().muted_foreground)
                                            .hover(|s| s.text_color(rgb(0xef4444)))
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_del.update(cx, |s, cx| {
                                                    let _ = s.remove_effect_from_selected_layer(&eff_id_del);
                                                    cx.notify();
                                                });
                                            })
                                            .child(icon_box(IconName::Trash)),
                                    );

                                effect_box = effect_box.child(header_row);

                                match &effect.effect_type {
                                    EffectType::GaussianBlur { radius } => {
                                        let r = radius.value;
                                        let s_m = self.state.clone();
                                        let s_p = self.state.clone();
                                        let id_m = eff_id.clone();
                                        let id_p = eff_id.clone();
                                        effect_box = effect_box.child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Radius"))
                                                .child(
                                                    h_flex()
                                                        .gap_1()
                                                        .items_center()
                                                        .child(step_button_with_id(
                                                            SharedString::from(format!("param_radius_minus_{}", eff_id)),
                                                            "-",
                                                            cx,
                                                            move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "radius", -5.0); cx.notify(); }),
                                                        ))
                                                        .child(div().id(SharedString::from(format!("param_radius_{}", eff_id))).test_support().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.1} px", r)))
                                                        .child(step_button_with_id(
                                                            SharedString::from(format!("param_radius_plus_{}", eff_id)),
                                                            "+",
                                                            cx,
                                                            move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "radius", 5.0); cx.notify(); }),
                                                        )),
                                                ),
                                        );
                                    }
                                    EffectType::BrightnessContrast { brightness, contrast } => {
                                        let b = brightness.value;
                                        let c = contrast.value;
                                        let s_bm = self.state.clone();
                                        let s_bp = self.state.clone();
                                        let s_cm = self.state.clone();
                                        let s_cp = self.state.clone();
                                        let id_bm = eff_id.clone();
                                        let id_bp = eff_id.clone();
                                        let id_cm = eff_id.clone();
                                        let id_cp = eff_id.clone();
                                        effect_box = effect_box
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Brightness"))
                                                    .child(
                                                        h_flex()
                                                            .gap_1()
                                                            .items_center()
                                                            .child(step_button("-", cx, move |cx| s_bm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_bm, "brightness", -5.0); cx.notify(); })))
                                                            .child(div().id(SharedString::from(format!("param_brightness_{}", eff_id))).test_support().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.1}", b)))
                                                            .child(step_button("+", cx, move |cx| s_bp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_bp, "brightness", 5.0); cx.notify(); }))),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Contrast"))
                                                    .child(
                                                        h_flex()
                                                            .gap_1()
                                                            .items_center()
                                                            .child(step_button("-", cx, move |cx| s_cm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_cm, "contrast", -5.0); cx.notify(); })))
                                                            .child(div().id(SharedString::from(format!("param_contrast_{}", eff_id))).test_support().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.1}", c)))
                                                            .child(step_button("+", cx, move |cx| s_cp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_cp, "contrast", 5.0); cx.notify(); }))),
                                                    ),
                                            );
                                    }
                                    EffectType::Tint { amount, .. } => {
                                        let a = amount.value;
                                        let s_m = self.state.clone();
                                        let s_p = self.state.clone();
                                        let id_m = eff_id.clone();
                                        let id_p = eff_id.clone();
                                        effect_box = effect_box.child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Amount"))
                                                .child(
                                                    h_flex()
                                                        .gap_1()
                                                        .items_center()
                                                        .child(step_button("-", cx, move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "amount", -10.0); cx.notify(); })))
                                                        .child(div().id(SharedString::from(format!("param_amount_{}", eff_id))).test_support().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.0} %", a)))
                                                        .child(step_button("+", cx, move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "amount", 10.0); cx.notify(); }))),
                                                ),
                                        );
                                    }
                                    EffectType::Invert { amount } => {
                                        let a = amount.value;
                                        let s_m = self.state.clone();
                                        let s_p = self.state.clone();
                                        let id_m = eff_id.clone();
                                        let id_p = eff_id.clone();
                                        effect_box = effect_box.child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Amount"))
                                                .child(
                                                    h_flex()
                                                        .gap_1()
                                                        .items_center()
                                                        .child(step_button("-", cx, move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "amount", -10.0); cx.notify(); })))
                                                        .child(div().id(SharedString::from(format!("param_amount_{}", eff_id))).test_support().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.0} %", a)))
                                                        .child(step_button("+", cx, move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "amount", 10.0); cx.notify(); }))),
                                                ),
                                        );
                                    }
                                    EffectType::DropShadow { distance, softness, opacity, .. } => {
                                        let d = distance.value;
                                        let s_val = softness.value;
                                        let o = opacity.value;
                                        let s_dm = self.state.clone();
                                        let s_dp = self.state.clone();
                                        let s_sm = self.state.clone();
                                        let s_sp = self.state.clone();
                                        let s_om = self.state.clone();
                                        let s_op = self.state.clone();
                                        let id_dm = eff_id.clone();
                                        let id_dp = eff_id.clone();
                                        let id_sm = eff_id.clone();
                                        let id_sp = eff_id.clone();
                                        let id_om = eff_id.clone();
                                        let id_op = eff_id.clone();
                                        effect_box = effect_box
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Distance"))
                                                    .child(
                                                        h_flex()
                                                            .gap_1()
                                                            .items_center()
                                                            .child(step_button("-", cx, move |cx| s_dm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_dm, "distance", -2.0); cx.notify(); })))
                                                            .child(div().id(SharedString::from(format!("param_distance_{}", eff_id))).test_support().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.1} px", d)))
                                                            .child(step_button("+", cx, move |cx| s_dp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_dp, "distance", 2.0); cx.notify(); }))),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Softness"))
                                                    .child(
                                                        h_flex()
                                                            .gap_1()
                                                            .items_center()
                                                            .child(step_button("-", cx, move |cx| s_sm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_sm, "softness", -2.0); cx.notify(); })))
                                                            .child(div().id(SharedString::from(format!("param_softness_{}", eff_id))).test_support().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.1} px", s_val)))
                                                            .child(step_button("+", cx, move |cx| s_sp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_sp, "softness", 2.0); cx.notify(); }))),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Opacity"))
                                                    .child(
                                                        h_flex()
                                                            .gap_1()
                                                            .items_center()
                                                            .child(step_button("-", cx, move |cx| s_om.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_om, "opacity", -10.0); cx.notify(); })))
                                                            .child(div().id(SharedString::from(format!("param_opacity_{}", eff_id))).test_support().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.0} %", o)))
                                                            .child(step_button("+", cx, move |cx| s_op.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_op, "opacity", 10.0); cx.notify(); }))),
                                                    ),
                                            );
                                    }
                                }

                                fx_col = fx_col.child(effect_box);
                            }
                            fx_col
                        };

                        vec![
                            // Transform Section Header
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .font_semibold()
                                .text_xs()
                                .text_color(cx.theme().foreground)
                                .child(icon_box(IconName::Move))
                                .child("Transform"),

                            // Anchor Point (X, Y)
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .w(px(70.))
                                        .gap_1()
                                        .items_center()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(icon_box(IconName::Move))
                                        .child("Anchor Pt"),
                                )
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .items_center()
                                                .child(step_button("-", cx, move |cx| s_anchor_mx.update(cx, |s, cx| { s.nudge_anchor(-10.0, 0.0); cx.notify(); })))
                                                .child(div().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("X: {:.1}", anchor.x)))
                                                .child(step_button("+", cx, move |cx| s_anchor_px.update(cx, |s, cx| { s.nudge_anchor(10.0, 0.0); cx.notify(); }))),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .items_center()
                                                .child(step_button("-", cx, move |cx| s_anchor_my.update(cx, |s, cx| { s.nudge_anchor(0.0, -10.0); cx.notify(); })))
                                                .child(div().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("Y: {:.1}", anchor.y)))
                                                .child(step_button("+", cx, move |cx| s_anchor_py.update(cx, |s, cx| { s.nudge_anchor(0.0, 10.0); cx.notify(); }))),
                                        ),
                                ),

                            // Position (X, Y)
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .w(px(70.))
                                        .gap_1()
                                        .items_center()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(icon_box(IconName::Move))
                                        .child("Position"),
                                )
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .items_center()
                                                .child(step_button("-", cx, move |cx| s_pos_mx.update(cx, |s, cx| { s.nudge_position(-10.0, 0.0); cx.notify(); })))
                                                .child(div().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("X: {:.1}", pos.x)))
                                                .child(step_button("+", cx, move |cx| s_pos_px.update(cx, |s, cx| { s.nudge_position(10.0, 0.0); cx.notify(); }))),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .items_center()
                                                .child(step_button("-", cx, move |cx| s_pos_my.update(cx, |s, cx| { s.nudge_position(0.0, -10.0); cx.notify(); })))
                                                .child(div().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("Y: {:.1}", pos.y)))
                                                .child(step_button("+", cx, move |cx| s_pos_py.update(cx, |s, cx| { s.nudge_position(0.0, 10.0); cx.notify(); }))),
                                        ),
                                ),

                            // Scale (X, Y)
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .w(px(70.))
                                        .gap_1()
                                        .items_center()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(icon_box(IconName::Maximize2))
                                        .child("Scale"),
                                )
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .items_center()
                                                .child(step_button("-", cx, move |cx| s_scale_mx.update(cx, |s, cx| { s.nudge_scale(-10.0, 0.0); cx.notify(); })))
                                                .child(div().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.1} %", sc.x)))
                                                .child(step_button("+", cx, move |cx| s_scale_px.update(cx, |s, cx| { s.nudge_scale(10.0, 0.0); cx.notify(); }))),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .items_center()
                                                .child(step_button("-", cx, move |cx| s_scale_my.update(cx, |s, cx| { s.nudge_scale(0.0, -10.0); cx.notify(); })))
                                                .child(div().px_2().py_0p5().bg(cx.theme().muted).rounded_sm().child(format!("{:.1} %", sc.y)))
                                                .child(step_button("+", cx, move |cx| s_scale_py.update(cx, |s, cx| { s.nudge_scale(0.0, 10.0); cx.notify(); }))),
                                        ),
                                ),

                            // Rotation
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .w(px(70.))
                                        .gap_1()
                                        .items_center()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(icon_box(IconName::RotateCw))
                                        .child("Rotation"),
                                )
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(step_button("-", cx, move |cx| s_rot_m.update(cx, |s, cx| { s.nudge_rotation(-15.0); cx.notify(); })))
                                        .child(
                                            div()
                                                .px_2()
                                                .py_0p5()
                                                .bg(cx.theme().muted)
                                                .rounded_sm()
                                                .child(format!("{:.1}°", rot)),
                                        )
                                        .child(step_button("+", cx, move |cx| s_rot_p.update(cx, |s, cx| { s.nudge_rotation(15.0); cx.notify(); }))),
                                ),

                            // Opacity
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .w(px(70.))
                                        .gap_1()
                                        .items_center()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(icon_box(IconName::Sun))
                                        .child("Opacity"),
                                )
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(step_button("-", cx, move |cx| s_op_m.update(cx, |s, cx| { s.nudge_opacity(-10.0); cx.notify(); })))
                                        .child(
                                            div()
                                                .px_2()
                                                .py_0p5()
                                                .bg(cx.theme().muted)
                                                .rounded_sm()
                                                .child(format!("{:.1} %", op)),
                                        )
                                        .child(step_button("+", cx, move |cx| s_op_p.update(cx, |s, cx| { s.nudge_opacity(10.0); cx.notify(); }))),
                                ),

                            // Switches & Modes Section Header
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .font_semibold()
                                .text_xs()
                                .text_color(cx.theme().foreground)
                                .child(icon_box(IconName::Eye))
                                .child("Switches & Modes"),

                            // Switch buttons
                            h_flex()
                                .gap_2()
                                .text_xs()
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .bg(if layer.visible { cx.theme().secondary } else { cx.theme().muted })
                                        .text_color(cx.theme().foreground)
                                        .rounded_sm()
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            s_vis.update(cx, |s, cx| {
                                                s.toggle_selected_layer_visibility();
                                                cx.notify();
                                            });
                                        })
                                        .child(icon_box(if layer.visible { IconName::Eye } else { IconName::EyeOff }))
                                        .child(if layer.visible { "Visible" } else { "Hidden" }),
                                )
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .bg(if layer.is_solo() { cx.theme().accent } else { cx.theme().muted })
                                        .text_color(if layer.is_solo() { cx.theme().accent_foreground } else { cx.theme().foreground })
                                        .rounded_sm()
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            s_solo.update(cx, |s, cx| {
                                                s.toggle_selected_layer_solo();
                                                cx.notify();
                                            });
                                        })
                                        .child(icon_box(IconName::Sparkles))
                                        .child("Solo"),
                                ),

                            // Effects Section Header
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .font_semibold()
                                .text_xs()
                                .text_color(cx.theme().foreground)
                                .child(icon_box(IconName::SlidersHorizontal))
                                .child(format!("Effects ({})", layer.effects.len())),

                            // Applied Effects List
                            div().child(effects_list),
                        ]
                    } else {
                        vec![
                            div()
                                .p_4()
                                .text_center()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("No layer selected. Select a layer from the timeline or project panel to inspect its properties.")
                        ]
                    }),
            )
    }
}

impl BasePanel for PropertiesPanel {
    fn panel_name(&self) -> &'static str {
        "properties"
    }
}

impl Panel for PropertiesPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Properties".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Properties"
    }
}

// --- 4. Effects Panel ---

pub struct EffectsPanel {
    focus_handle: FocusHandle,
    state: Option<Entity<EditorState>>,
    _subscription: Option<Subscription>,
}

impl EffectsPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            state: None,
            _subscription: None,
        }
    }

    pub fn new_with_state(state: Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&state, |_this, _state, cx| {
            cx.notify();
        });
        Self {
            focus_handle: cx.focus_handle(),
            state: Some(state),
            _subscription: Some(_subscription),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }
}

impl EventEmitter<PanelEvent> for EffectsPanel {}

impl Focusable for EffectsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn effect_item_row(
    id_str: &'static str,
    title: &'static str,
    effect_type: EffectType,
    state: &Option<Entity<EditorState>>,
    cx: &App,
) -> impl IntoElement {
    let mut row = h_flex()
        .id(SharedString::from(format!("effect_item_{id_str}")))
        .test_support()
        .px_3()
        .py_1()
        .rounded_sm()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .hover(|s| s.bg(cx.theme().muted));

    if let Some(state) = state {
        let state = state.clone();
        let et = effect_type.clone();
        row = row.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            state.update(cx, |s, cx| {
                let _ = s.add_effect_to_selected_layer(et.clone());
                cx.notify();
            });
        });
    }

    row.child(
        h_flex()
            .gap_1p5()
            .items_center()
            .child(icon_box(IconName::Sparkles))
            .child(div().text_xs().text_color(cx.theme().foreground).child(title)),
    )
    .child(
        h_flex()
            .gap_1()
            .items_center()
            .text_xs()
            .text_color(cx.theme().primary)
            .child(icon_box(IconName::Plus))
            .child("Add"),
    )
}

fn category_header(title: &'static str, icon: IconName, cx: &App) -> Div {
    h_flex()
        .px_2()
        .py_1()
        .gap_1p5()
        .items_center()
        .font_semibold()
        .text_xs()
        .text_color(cx.theme().foreground)
        .child(icon_box(icon))
        .child(title)
}

impl Render for EffectsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("effects_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Search / Filter
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(
                        h_flex()
                            .flex_1()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .text_xs()
                            .gap_1p5()
                            .items_center()
                            .child(icon_box(IconName::FolderOpen))
                            .child("Search Effects & Presets..."),
                    ),
            )
            // Effects Category List
            .child(
                v_flex()
                    .id("effects_categories")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .p_2()
                    .gap_1()
                    // Category 1: Blur & Sharpen
                    .child(category_header("▼ Blur & Sharpen", IconName::SlidersHorizontal, cx))
                    .child(effect_item_row("gaussian_blur", "Gaussian Blur", EffectType::gaussian_blur(10.0), &self.state, cx))
                    .child(effect_item_row("fast_box_blur", "Fast Box Blur", EffectType::gaussian_blur(5.0), &self.state, cx))
                    .child(effect_item_row("directional_blur", "Directional Blur", EffectType::gaussian_blur(15.0), &self.state, cx))
                    .child(effect_item_row("sharpen", "Sharpen", EffectType::brightness_contrast(0.0, 25.0), &self.state, cx))
                    // Category 2: Color Correction
                    .child(category_header("▼ Color Correction", IconName::Palette, cx))
                    .child(effect_item_row("brightness_contrast", "Brightness & Contrast", EffectType::brightness_contrast(15.0, 10.0), &self.state, cx))
                    .child(effect_item_row("tint", "Tint", EffectType::tint(Color::BLACK, Color::WHITE, 100.0), &self.state, cx))
                    .child(effect_item_row("invert", "Invert", EffectType::invert(100.0), &self.state, cx))
                    .child(effect_item_row("color_balance", "Color Balance (HLS)", EffectType::tint(Color::rgb(0.1, 0.0, 0.0), Color::rgb(1.0, 0.9, 0.8), 50.0), &self.state, cx))
                    .child(effect_item_row("lumetri_color", "Lumetri Color", EffectType::brightness_contrast(5.0, 15.0), &self.state, cx))
                    // Category 3: Distort & Perspective
                    .child(category_header("▼ Distort & Perspective", IconName::WandSparkles, cx))
                    .child(effect_item_row("drop_shadow", "Drop Shadow", EffectType::drop_shadow(8.0, 45.0, 10.0, 75.0, Color::BLACK), &self.state, cx))
                    .child(effect_item_row("transform", "Transform", EffectType::drop_shadow(0.0, 0.0, 0.0, 100.0, Color::BLACK), &self.state, cx))
                    // Category 4: Generate & Stylize
                    .child(category_header("▼ Generate & Stylize", IconName::Sparkles, cx))
                    .child(effect_item_row("fill", "Fill", EffectType::tint(Color::rgb(0.2, 0.4, 0.8), Color::rgb(0.2, 0.4, 0.8), 100.0), &self.state, cx))
                    .child(effect_item_row("gradient_ramp", "Gradient Ramp", EffectType::tint(Color::BLACK, Color::rgb(0.9, 0.3, 0.1), 75.0), &self.state, cx))
                    // Category 5: Transition
                    .child(category_header("▼ Transition", IconName::RotateCw, cx))
                    .child(effect_item_row("linear_wipe", "Linear Wipe", EffectType::invert(50.0), &self.state, cx)),
            )
            // Footer
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .gap_1p5()
                    .items_center()
                    .child(icon_box(IconName::Sparkles))
                    .child("13 real built-in effects available • Click to apply"),
            )
    }
}

impl BasePanel for EffectsPanel {
    fn panel_name(&self) -> &'static str {
        "effects"
    }
}

impl Panel for EffectsPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Effects".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Effects & Presets"
    }
}

// --- 5. Timeline Panel ---

pub struct TimelinePanel {
    focus_handle: FocusHandle,
    state: Entity<EditorState>,
    _subscription: Subscription,
}

impl TimelinePanel {
    pub fn new(state: Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&state, |_this, _state, cx| {
            cx.notify();
        });
        Self {
            focus_handle: cx.focus_handle(),
            state,
            _subscription,
        }
    }

    pub fn standalone(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new(state, cx)
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn state(&self) -> &Entity<EditorState> {
        &self.state
    }
}

impl EventEmitter<PanelEvent> for TimelinePanel {}

impl Focusable for TimelinePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TimelinePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let comp_opt = state.active_composition();
        let current_tc = state.clock.timecode();
        let current_frame = state.clock.current_frame();
        let total_frames = comp_opt.map(|c| c.duration.frames()).unwrap_or(150);
        let is_playing = state.is_playing;

        let s_start = self.state.clone();
        let s_step_prev = self.state.clone();
        let s_play = self.state.clone();
        let s_step_next = self.state.clone();
        let s_end = self.state.clone();

        let in_str = "00:00:00:00";
        let out_str = comp_opt.map(|c| format!("{}", c.duration)).unwrap_or_else(|| "00:00:05:00".to_string());

        let playhead_percent = (current_frame as f32 / total_frames.max(1) as f32 * 100.0).clamp(0.0, 100.0);

        let track_rows: Vec<Div> = match comp_opt {
            Some(comp) => comp
                .layers
                .iter()
                .enumerate()
                .map(|(idx, layer)| {
                    let is_selected = state.selected_layer_id.as_deref() == Some(&layer.id);
                    let sel_state = self.state.clone();
                    let vis_state = self.state.clone();
                    let solo_state = self.state.clone();
                    let lid = layer.id.clone();
                    let lid_vis = layer.id.clone();
                    let lid_solo = layer.id.clone();

                    let in_ratio = (layer.in_point.frames() as f32 / total_frames.max(1) as f32).clamp(0.0, 1.0);
                    let out_ratio = (layer.out_point.frames() as f32 / total_frames.max(1) as f32).clamp(0.0, 1.0);
                    let span_w = ((out_ratio - in_ratio) * 100.0).max(5.0);
                    let span_left = in_ratio * 100.0;

                    let row = h_flex()
                        .h(px(26.))
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .items_center();

                    let mut left_col = h_flex()
                        .w(px(240.))
                        .px_2()
                        .border_r_1()
                        .border_color(cx.theme().border)
                        .items_center()
                        .justify_between()
                        .text_xs()
                        .cursor_pointer()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(div().w(px(14.)).child(format!("{}", idx + 1)))
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_color(if layer.visible {
                                            cx.theme().foreground
                                        } else {
                                            cx.theme().muted_foreground
                                        })
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            vis_state.update(cx, |s, cx| {
                                                s.toggle_layer_visibility(&lid_vis);
                                                cx.notify();
                                            });
                                        })
                                        .child(icon_box(if layer.visible {
                                            IconName::Eye
                                        } else {
                                            IconName::EyeOff
                                        })),
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_color(if layer.is_solo() {
                                            rgb(0xf59e0b).into()
                                        } else {
                                            cx.theme().muted_foreground
                                        })
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            solo_state.update(cx, |s, cx| {
                                                s.toggle_layer_solo(&lid_solo);
                                                cx.notify();
                                            });
                                        })
                                        .child(icon_box(IconName::Sparkles)),
                                )
                                .child(div().font_semibold().child(layer.name.clone())),
                        )
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("Normal"),
                        );

                    if is_selected {
                        left_col = left_col
                            .bg(cx.theme().accent)
                            .text_color(cx.theme().accent_foreground);
                    } else {
                        left_col = left_col.hover(|s| s.bg(cx.theme().muted));
                    }

                    left_col = left_col.on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        sel_state.update(cx, |s, cx| {
                            s.select_layer(Some(lid.clone()));
                            cx.notify();
                        });
                    });

                    let span_state = self.state.clone();
                    let lid_span = layer.id.clone();

                    let track_col = div()
                        .flex_1()
                        .h_full()
                        .relative()
                        .child(
                            // Layer span bar
                            div()
                                .id(SharedString::from(format!("track_span_{}", layer.id)))
                                .test_support()
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    span_state.update(cx, |s, cx| {
                                        s.select_layer(Some(lid_span.clone()));
                                        cx.notify();
                                    });
                                })
                                .absolute()
                                .top(px(4.))
                                .bottom(px(4.))
                                .left(relative(span_left / 100.0))
                                .w(relative(span_w / 100.0))
                                .rounded_sm()
                                .bg(if is_selected {
                                    cx.theme().accent
                                } else {
                                    cx.theme().primary
                                })
                                .opacity(if layer.visible { 0.85 } else { 0.35 })
                                .px_2()
                                .text_xs()
                                .text_color(cx.theme().primary_foreground)
                                .child(format!(
                                    "{} [{} - {}]",
                                    layer.name, layer.in_point, layer.out_point
                                )),
                        )
                        .child(
                            // Playhead line across track
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(rgb(0xef4444))
                                .left(relative(playhead_percent / 100.0)),
                        );

                    row.child(left_col).child(track_col)
                })
                .collect(),
            None => Vec::new(),
        };

        div()
            .id("timeline_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Header / Timecode & Transport
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .items_center()
                    .justify_between()
                    // Timecode display
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("timecode_display")
                                    .test_support()
                                    .px_2()
                                    .py_0p5()
                                    .bg(cx.theme().muted)
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .font_bold()
                                    .text_sm()
                                    .text_color(cx.theme().primary)
                                    .child(format!("{current_tc}")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("Frame {current_frame} / {total_frames}")),
                            ),
                    )
                    // Transport controls
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .id("transport_start")
                                    .test_support()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .hover(|s| s.bg(cx.theme().accent))
                                    .cursor_pointer()
                                    .text_xs()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_start.update(cx, |s, cx| {
                                            s.jump_to_start();
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::SkipBack)),
                            )
                            .child(
                                div()
                                    .id("transport_prev")
                                    .test_support()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .hover(|s| s.bg(cx.theme().accent))
                                    .cursor_pointer()
                                    .text_xs()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_step_prev.update(cx, |s, cx| {
                                            s.step_backward();
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::StepBack)),
                            )
                            .child(
                                div()
                                    .id("transport_play")
                                    .test_support()
                                    .px_3()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().primary)
                                    .hover(|s| s.opacity(0.9))
                                    .cursor_pointer()
                                    .text_color(cx.theme().primary_foreground)
                                    .text_xs()
                                    .font_bold()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_play.update(cx, |s, cx| {
                                            s.toggle_playback();
                                            cx.notify();
                                        });
                                    })
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .items_center()
                                            .child(icon_box(if is_playing {
                                                IconName::Pause
                                            } else {
                                                IconName::Play
                                            }))
                                            .child(if is_playing { "Pause" } else { "Play" }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("transport_next")
                                    .test_support()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .hover(|s| s.bg(cx.theme().accent))
                                    .cursor_pointer()
                                    .text_xs()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_step_next.update(cx, |s, cx| {
                                            s.step_forward();
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::StepForward)),
                            )
                            .child(
                                div()
                                    .id("transport_end")
                                    .test_support()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .hover(|s| s.bg(cx.theme().accent))
                                    .cursor_pointer()
                                    .text_xs()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_end.update(cx, |s, cx| {
                                            s.jump_to_end();
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::SkipForward)),
                            )
                            .child(
                                h_flex()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().secondary)
                                    .text_xs()
                                    .gap_1()
                                    .items_center()
                                    .child(icon_box(IconName::Repeat))
                                    .child("Loop"),
                            ),
                    )
                    // Duration & In/Out
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(div().child(format!("In: {in_str}")))
                            .child(div().child(format!("Out: {out_str}"))),
                    ),
            )
            // Time Ruler
            .child(
                h_flex()
                    .h(px(22.))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .w(px(240.))
                            .px_3()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child("Layer Name / Switches"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .relative()
                            .h_full()
                            .child(
                                h_flex()
                                    .size_full()
                                    .justify_between()
                                    .px_3()
                                    .items_center()
                                    .child(div().child("00:00s"))
                                    .child(div().child("00:01s"))
                                    .child(div().child("00:02s"))
                                    .child(div().child("00:03s"))
                                    .child(div().child("00:04s"))
                                    .child(div().child("00:05s")),
                            )
                            .child(
                                // Playhead marker on ruler
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(2.))
                                    .bg(rgb(0xef4444))
                                    .left(relative(playhead_percent / 100.0)),
                            ),
                    ),
            )
            // Tracks area
            .child(
                v_flex()
                    .id("timeline")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .children(track_rows),
            )
    }
}

impl BasePanel for TimelinePanel {
    fn panel_name(&self) -> &'static str {
        "timeline"
    }
}

impl Panel for TimelinePanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Timeline".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "Timeline"
    }
}

// --- App Panels Container ---

#[derive(Clone)]
pub struct AppPanels {
    pub project: Entity<ProjectPanel>,
    pub composition: Entity<CompositionViewerPanel>,
    pub viewer: Entity<CompositionViewerPanel>,
    pub properties: Entity<PropertiesPanel>,
    pub effects: Entity<EffectsPanel>,
    pub timeline: Entity<TimelinePanel>,
}

impl AppPanels {
    pub fn new(state: Entity<EditorState>, cx: &mut App) -> Self {
        let composition = cx.new(|cx| CompositionViewerPanel::new(state.clone(), cx));
        Self {
            project: cx.new(|cx| ProjectPanel::new(state.clone(), cx)),
            composition: composition.clone(),
            viewer: composition,
            properties: cx.new(|cx| PropertiesPanel::new(state.clone(), cx)),
            effects: cx.new(|cx| EffectsPanel::new_with_state(state.clone(), cx)),
            timeline: cx.new(|cx| TimelinePanel::new(state, cx)),
        }
    }
}
