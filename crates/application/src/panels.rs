use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex, StyledExt, TestSupportExt};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use std::collections::HashSet;

use crate::state::{EditorState, EditorTool};
use project::{BlendMode, Color, EffectType, LayerSource, TimeCode, TrackMatteMode, Vec2};

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
        let del_state = self.state.clone();
        let import_state = self.state.clone();

        let mut bin_items: Vec<Div> = Vec::new();

        if let Some(comp) = comp_opt {
            // --- Section 1: Compositions ---
            bin_items.push(
                h_flex()
                    .px_2()
                    .py_1()
                    .mt_1()
                    .bg(cx.theme().secondary)
                    .rounded_sm()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().foreground)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(icon_box(IconName::Folder))
                            .child("COMPOSITIONS"),
                    )
                    .child(
                        div()
                            .px_1p5()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .child("1"),
                    ),
            );

            // Active comp row
            bin_items.push(
                h_flex()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .text_xs()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .font_semibold()
                            .child(icon_box(IconName::Film))
                            .child(comp.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}x{}", comp.width, comp.height))
                            .child(format!("{:.0}fps", comp.frame_rate))
                            .child(format!("{}", comp.duration)),
                    ),
            );

            // --- Section 2: Project Media & Bins ---
            bin_items.push(
                h_flex()
                    .px_2()
                    .py_1()
                    .mt_2()
                    .bg(cx.theme().secondary)
                    .rounded_sm()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().foreground)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(icon_box(IconName::FolderOpen))
                            .child("PROJECT MEDIA"),
                    )
                    .child(
                        div()
                            .px_1p5()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}", state.project.assets.len())),
                    ),
            );

            if state.project.assets.is_empty() {
                bin_items.push(
                    div()
                        .px_2()
                        .py_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("No external media. Click 'Import Media' to add PNG/JPG or video."),
                );
            } else {
                for asset in &state.project.assets {
                    let (type_str, icon) = match &asset.asset_type {
                        project::AssetType::Image => ("PNG/JPG", IconName::Image),
                        project::AssetType::Video => ("VIDEO", IconName::Film),
                        project::AssetType::Audio => ("AUDIO", IconName::Music),
                        project::AssetType::Vector => ("SVG", IconName::Folder),
                        project::AssetType::Font => ("FONT", IconName::Type),
                        project::AssetType::Other(_) => ("MEDIA", IconName::Layers),
                    };

                    let display_name = asset
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&asset.name)
                        .to_string();

                    let asset_id_del = asset.id.clone();
                    let s_del = self.state.clone();

                    bin_items.push(
                        h_flex()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .items_center()
                            .justify_between()
                            .text_color(cx.theme().foreground)
                            .hover(|s| s.bg(cx.theme().muted))
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(icon_box(icon))
                                    .child(div().font_medium().child(display_name)),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(cx.theme().secondary)
                                            .text_color(cx.theme().muted_foreground)
                                            .child(type_str),
                                    )
                                    .child(
                                        div()
                                            .cursor_pointer()
                                            .text_color(cx.theme().muted_foreground)
                                            .hover(|s| s.text_color(rgb(0xef4444)))
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_del.update(cx, |s, cx| {
                                                    let _ = s.delete_asset(&asset_id_del);
                                                    cx.notify();
                                                });
                                            })
                                            .child(icon_box(IconName::Trash)),
                                    ),
                            ),
                    );
                }
            }

            // --- Section 3: Project Solids & Footage Bin ---
            let solids: Vec<_> = comp
                .layers
                .iter()
                .filter(|l| matches!(&l.source, LayerSource::Solid { .. }))
                .collect();

            bin_items.push(
                h_flex()
                    .px_2()
                    .py_1()
                    .mt_2()
                    .bg(cx.theme().secondary)
                    .rounded_sm()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().foreground)
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(icon_box(IconName::Folder))
                            .child("SOLIDS BIN"),
                    )
                    .child(
                        div()
                            .px_1p5()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}", solids.len())),
                    ),
            );

            if solids.is_empty() {
                bin_items.push(
                    div()
                        .px_2()
                        .py_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("No solid footage items. Click '+ Solid' to create one."),
                );
            } else {
                for solid in solids {
                    let (w, h, col) = match &solid.source {
                        LayerSource::Solid { width, height, color } => (*width, *height, *color),
                        _ => (1920, 1080, Color::WHITE),
                    };

                    let row = h_flex()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .text_xs()
                        .items_center()
                        .justify_between()
                        .text_color(cx.theme().foreground)
                        .hover(|s| s.bg(cx.theme().muted))
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    div()
                                        .w(px(10.))
                                        .h(px(10.))
                                        .rounded_sm()
                                        .bg(Rgba { r: col.r, g: col.g, b: col.b, a: col.a }),
                                )
                                .child(div().font_medium().child(format!("{} (Footage)", solid.name))),
                        )
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{}x{}", w, h)),
                        );
                    bin_items.push(row);
                }
            }
        }

        let sample_state = self.state.clone();

        div()
            .id("project_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Header / Actions toolbar
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_1p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1()
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
                                        let s_import = import_state.clone();
                                        cx.spawn(|cx: &mut AsyncApp| {
                                            let cx = cx.clone();
                                            async move {
                                                let (tx, rx) = std::sync::mpsc::channel();
                                                let _ = std::thread::Builder::new()
                                                    .name("file-dialog-worker".to_string())
                                                    .stack_size(8 * 1024 * 1024)
                                                    .spawn(move || {
                                                        let file = rfd::FileDialog::new()
                                                            .add_filter("Media Files", &["png", "jpg", "jpeg", "mp4", "mov", "webm"])
                                                            .pick_file();
                                                        let _ = tx.send(file);
                                                    });
                                                if let Ok(Some(path)) = rx.recv() {
                                                    cx.update(|cx| {
                                                        s_import.update(cx, |s, cx| {
                                                            let _ = s.import_media_file(path);
                                                            cx.notify();
                                                        });
                                                    });
                                                }
                                            }
                                        }).detach();
                                    })
                                    .child(icon_box(IconName::FolderOpen))
                                    .child("Import Media..."),
                            )
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
                                    .id("sample_media_button")
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
                                        sample_state.update(cx, |s, cx| {
                                            let _ = s.import_sample_image();
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::Image))
                                    .child("Sample"),
                            )
                            .child(
                                div()
                                    .id("delete_asset_button")
                                    .test_support()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                                    .text_color(cx.theme().foreground)
                                    .text_xs()
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        del_state.update(cx, |s, cx| {
                                            let _ = s.delete_selected_layer();
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::Trash))
                                    .child("Delete"),
                            ),
                    ),
            )
            // Organized Bins and Assets List
            .child(
                v_flex()
                    .id("project_assets")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .children(bin_items),
            )
            // Footer summary
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
                            "1 Comp • {} Assets • {} Layers • {:.2} fps",
                            state.project.assets.len(),
                            comp.layers.len(),
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
    pub context_menu: Option<String>,
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
            context_menu: None,
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
            Some(stack) => {
                let mut elements = Vec::new();
                for layer in stack.render_layers() {
                    let (base_w, base_h, col, image_path) = match &layer.source {
                        LayerSource::Solid {
                            width,
                            height,
                            color,
                        } => (*width as f32, *height as f32, *color, None),
                        LayerSource::Image { asset_id } => {
                            let (w, h, p) = if let Some(asset) = state.project.get_asset(asset_id) {
                                let (dim_w, dim_h) = image::image_dimensions(&asset.path)
                                    .unwrap_or((1920, 1080));
                                (dim_w as f32, dim_h as f32, Some(asset.path.clone()))
                            } else {
                                (400.0, 300.0, None)
                            };
                            (w, h, Color::WHITE, p)
                        }
                        LayerSource::Video { asset_id, .. } => {
                            let p = state.project.get_asset(asset_id).map(|a| a.path.clone());
                            (1920.0, 1080.0, Color::WHITE, p)
                        }
                        _ => (400.0, 300.0, Color::WHITE, None),
                    };

                    // Process visual effects (Invert, Tint, Brightness/Contrast, GlslShader)
                    let processed_col = layer.processed_color(col);

                    let bbox = layer.world_bounds(base_w, base_h);
                    let l_x = bbox.min.x * scale_x;
                    let l_y = bbox.min.y * scale_y;
                    let l_w = ((bbox.max.x - bbox.min.x) * scale_x).max(2.0);
                    let l_h = ((bbox.max.y - bbox.min.y) * scale_y).max(2.0);
                    let is_selected = state.selected_layer_id.as_deref() == Some(&layer.id);

                    // Render Drop Shadow if present
                    for eff in &layer.effects {
                        if eff.enabled {
                            if let compositor::EvaluatedEffectType::DropShadow { distance, angle, opacity, color, .. } = &eff.effect_type {
                                let rad = angle.to_radians();
                                let sx = l_x + distance * rad.cos() * scale_x;
                                let sy = l_y + distance * rad.sin() * scale_y;
                                let op = (opacity / 100.0).clamp(0.0, 1.0) * layer.effective_opacity.clamp(0.0, 1.0);
                                elements.push(
                                    div()
                                        .absolute()
                                        .left(px(sx))
                                        .top(px(sy))
                                        .w(px(l_w))
                                        .h(px(l_h))
                                        .bg(Rgba {
                                            r: color.r,
                                            g: color.g,
                                            b: color.b,
                                            a: color.a * op * 0.75,
                                        })
                                        .rounded_sm()
                                        .into_any_element(),
                                );
                                break;
                            }
                        }
                    }

                    // Render Gaussian Blur aura if present
                    let mut blur_rad = 0.0f32;
                    for eff in &layer.effects {
                        if eff.enabled {
                            if let compositor::EvaluatedEffectType::GaussianBlur { radius } = &eff.effect_type {
                                blur_rad += *radius;
                            }
                        }
                    }
                    if blur_rad > 0.0 {
                        let expand = (blur_rad * 0.25).clamp(2.0, 16.0);
                        elements.push(
                            div()
                                .absolute()
                                .left(px(l_x - expand))
                                .top(px(l_y - expand))
                                .w(px(l_w + expand * 2.0))
                                .h(px(l_h + expand * 2.0))
                                .rounded_md()
                                .bg(Rgba {
                                    r: processed_col.r,
                                    g: processed_col.g,
                                    b: processed_col.b,
                                    a: (processed_col.a * layer.effective_opacity.clamp(0.0, 1.0) * 0.35).clamp(0.0, 1.0),
                                })
                                .into_any_element(),
                        );
                    }

                    let sel_state = self.state.clone();
                    let lid = layer.id.clone();
                    let is_video = matches!(&layer.source, LayerSource::Video { .. });

                    let p_menu = cx.entity().clone();
                    let lid_menu = layer.id.clone();
                    let mut layer_el = div()
                        .id(ElementId::Name(format!("canvas_layer_{}", layer.id).into()))
                        .test_support()
                        .absolute()
                        .left(px(l_x))
                        .top(px(l_y))
                        .w(px(l_w))
                        .h(px(l_h))
                        .bg(Rgba {
                            r: processed_col.r,
                            g: processed_col.g,
                            b: processed_col.b,
                            a: processed_col.a * layer.effective_opacity.clamp(0.0, 1.0),
                        })
                        .overflow_hidden()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            sel_state.update(cx, |s, cx| {
                                s.select_layer(Some(lid.clone()));
                                cx.notify();
                            });
                        })
                        .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                            p_menu.update(cx, |this, cx| {
                                this.context_menu = Some(lid_menu.clone());
                                cx.notify();
                            });
                        });

                    if is_video {
                        layer_el = layer_el.child(
                            div()
                                .size_full()
                                .bg(rgb(0x0f172a))
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .child(
                                    div()
                                        .w(px(24.))
                                        .h(px(24.))
                                        .rounded_full()
                                        .bg(cx.theme().primary)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(cx.theme().primary_foreground)
                                        .child(icon_box(IconName::Film)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(rgb(0xffffff))
                                        .child(layer.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x94a3b8))
                                        .child("1920x1080 • Video Footage"),
                                ),
                        );
                    } else if let Some(ref img_path) = image_path {
                        layer_el = layer_el.child(
                            gpui::img(img_path.clone())
                                .size_full()
                                .opacity(layer.effective_opacity.clamp(0.0, 1.0)),
                        );

                        // Image effect overlays
                        for eff in &layer.effects {
                            if eff.enabled {
                                match &eff.effect_type {
                                    compositor::EvaluatedEffectType::Tint { map_white, amount, .. } => {
                                        let alpha = (*amount / 100.0).clamp(0.0, 0.75);
                                        layer_el = layer_el.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .size_full()
                                                .bg(Rgba { r: map_white.r, g: map_white.g, b: map_white.b, a: alpha }),
                                        );
                                    }
                                    compositor::EvaluatedEffectType::Invert { amount } => {
                                        let alpha = (*amount / 100.0).clamp(0.0, 0.6);
                                        layer_el = layer_el.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .size_full()
                                                .bg(Rgba { r: 1.0, g: 1.0, b: 1.0, a: alpha }),
                                        );
                                    }
                                    compositor::EvaluatedEffectType::GlslShader { param1, param2, .. } => {
                                        let pulse = ((current_frame as f32 * param1 * 0.1).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
                                        let alpha = (*param2 / 100.0 * 0.4 * pulse).clamp(0.0, 0.7);
                                        layer_el = layer_el.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .size_full()
                                                .bg(Rgba { r: 0.2 + 0.6 * pulse, g: 0.3, b: 0.9, a: alpha }),
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    if is_selected {
                        layer_el = layer_el
                            .border_2()
                            .border_color(rgb(0x3b82f6)); // accent selection border
                    }

                    elements.push(layer_el.into_any_element());
                }
                elements
            }
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
                    .child({
                        let active_tool = state.active_tool;
                        let s_tool = self.state.clone();
                        let mut canvas_frame = div()
                            .id("canvas_viewport_frame")
                            .test_support()
                            .w(px(canvas_w))
                            .h(px(canvas_h))
                            .border_2()
                            .border_color(cx.theme().border)
                            .bg(bg_color)
                            .rounded_sm()
                            .relative()
                            .overflow_hidden()
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                match active_tool {
                                    EditorTool::Text => {
                                        s_tool.update(cx, |s, cx| {
                                            let _ = s.add_text_layer("New Text Layer", None);
                                            cx.notify();
                                        });
                                    }
                                    EditorTool::ShapeRect => {
                                        s_tool.update(cx, |s, cx| {
                                            let _ = s.add_rectangle_shape_layer(300.0, 200.0, None);
                                            cx.notify();
                                        });
                                    }
                                    EditorTool::ShapeEllipse => {
                                        s_tool.update(cx, |s, cx| {
                                            let _ = s.add_ellipse_shape_layer(150.0, 150.0, None);
                                            cx.notify();
                                        });
                                    }
                                    EditorTool::Pen => {
                                        s_tool.update(cx, |s, cx| {
                                            let _ = s.add_pen_point(Vec2::new(100.0, 100.0));
                                            cx.notify();
                                        });
                                    }
                                    EditorTool::Rotate => {
                                        s_tool.update(cx, |s, cx| {
                                            s.nudge_rotation(15.0);
                                            cx.notify();
                                        });
                                    }
                                    _ => {}
                                }
                            })
                            .children(rendered_layers);

                        if let Some(ref menu_lid) = self.context_menu {
                            let s_menu = self.state.clone();
                            let p_close = cx.entity().clone();
                            let target_lid = menu_lid.clone();

                            let s1 = s_menu.clone();
                            let p1 = p_close.clone();
                            let t1 = target_lid.clone();

                            let s2 = s_menu.clone();
                            let p2 = p_close.clone();
                            let t2 = target_lid.clone();

                            let s3 = s_menu.clone();
                            let p3 = p_close.clone();
                            let t3 = target_lid.clone();

                            let s4 = s_menu.clone();
                            let p4 = p_close.clone();
                            let t4 = target_lid.clone();

                            let p5 = p_close.clone();

                            let canvas_ctx_overlay = div()
                                .id("canvas_context_menu")
                                .test_support()
                                .absolute()
                                .top(px(40.))
                                .left(px(100.))
                                .w(px(200.))
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded_md()
                                .shadow_lg()
                                .p_1()
                                .child(
                                    v_flex()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .font_bold()
                                                .text_xs()
                                                .px_2()
                                                .py_1()
                                                .border_b_1()
                                                .border_color(cx.theme().border)
                                                .child("Layer Actions"),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .text_xs()
                                                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    s1.update(cx, |s, cx| {
                                                        let _ = s.duplicate_layer(&t1);
                                                        cx.notify();
                                                    });
                                                    p1.update(cx, |this, cx| {
                                                        this.context_menu = None;
                                                        cx.notify();
                                                    });
                                                })
                                                .child("Duplicate Layer"),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .text_xs()
                                                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    s2.update(cx, |s, cx| {
                                                        s.reset_layer_transform(&t2);
                                                        cx.notify();
                                                    });
                                                    p2.update(cx, |this, cx| {
                                                        this.context_menu = None;
                                                        cx.notify();
                                                    });
                                                })
                                                .child("Reset Transform"),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .text_xs()
                                                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    s3.update(cx, |s, cx| {
                                                        s.add_keyframe_to_all_transforms_at_playhead(&t3);
                                                        cx.notify();
                                                    });
                                                    p3.update(cx, |this, cx| {
                                                        this.context_menu = None;
                                                        cx.notify();
                                                    });
                                                })
                                                .child("Keyframe Transform at CTI"),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .text_xs()
                                                .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    s4.update(cx, |s, cx| {
                                                        let _ = s.remove_layer_by_id(&t4);
                                                        cx.notify();
                                                    });
                                                    p4.update(cx, |this, cx| {
                                                        this.context_menu = None;
                                                        cx.notify();
                                                    });
                                                })
                                                .child("Delete Layer"),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .hover(|s| s.bg(cx.theme().muted))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    p5.update(cx, |this, cx| {
                                                        this.context_menu = None;
                                                        cx.notify();
                                                    });
                                                })
                                                .child("Cancel"),
                                        ),
                                );
                            canvas_frame = canvas_frame.child(canvas_ctx_overlay);
                        }

                        canvas_frame
                    }),
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
    pub scrub_prop: Option<String>,
    pub scrub_last_x: Option<f32>,
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
            scrub_prop: None,
            scrub_last_x: None,
        }
    }

    pub fn standalone(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new(state, cx)
    }

    pub fn apply_scrub_delta(&mut self, prop: &str, dx: f32, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| {
            match prop {
                "anchor_x" => s.nudge_anchor(dx * 1.0, 0.0),
                "anchor_y" => s.nudge_anchor(0.0, dx * 1.0),
                "pos_x" => s.nudge_position(dx * 1.0, 0.0),
                "pos_y" => s.nudge_position(0.0, dx * 1.0),
                "scale_x" => s.nudge_scale(dx * 0.5, 0.0),
                "scale_y" => s.nudge_scale(0.0, dx * 0.5),
                "rotation" => s.nudge_rotation(dx * 0.5),
                "opacity" => s.nudge_opacity(dx * 0.5),
                other => {
                    if let Some(rest) = other.strip_prefix("fx:") {
                        let parts: Vec<&str> = rest.split(':').collect();
                        if parts.len() >= 2 {
                            let eff_id = parts[0];
                            let param = parts[1];
                            let mult = parts.get(2).and_then(|m| m.parse::<f32>().ok()).unwrap_or(50.0) / 100.0;
                            let _ = s.nudge_effect_param(eff_id, param, dx * mult);
                        }
                    }
                }
            }
            cx.notify();
        });
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub fn state(&self) -> &Entity<EditorState> {
        &self.state
    }
}

#[allow(clippy::too_many_arguments)]
fn scrub_field<FMinus, FPlus>(
    id: impl Into<ElementId>,
    prop_key: String,
    label: String,
    minus_id: Option<ElementId>,
    plus_id: Option<ElementId>,
    state: &Entity<EditorState>,
    panel_entity: &Entity<PropertiesPanel>,
    cx: &App,
    on_minus: FMinus,
    on_plus: FPlus,
) -> Div
where
    FMinus: Fn(&mut App) + 'static,
    FPlus: Fn(&mut App) + 'static,
{
    let panel_down = panel_entity.clone();
    let state_scroll = state.clone();
    let prop_for_wheel = prop_key.clone();

    let btn_minus: AnyElement = if let Some(mid) = minus_id {
        step_button_with_id(mid, "-", cx, on_minus).into_any_element()
    } else {
        step_button("-", cx, on_minus).into_any_element()
    };

    let btn_plus: AnyElement = if let Some(pid) = plus_id {
        step_button_with_id(pid, "+", cx, on_plus).into_any_element()
    } else {
        step_button("+", cx, on_plus).into_any_element()
    };

    h_flex()
        .gap_1()
        .items_center()
        .child(btn_minus)
        .child(
            div()
                .id(id)
                .test_support()
                .px_2()
                .py_0p5()
                .bg(cx.theme().muted)
                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                .border_1()
                .border_color(cx.theme().border)
                .rounded_sm()
                .cursor_col_resize()
                .text_xs()
                .font_medium()
                .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                    let curr_x = event.position.x / px(1.0);
                    let p = prop_key.clone();
                    panel_down.update(cx, |this, _| {
                        this.scrub_prop = Some(p);
                        this.scrub_last_x = Some(curr_x);
                    });
                })
                .on_scroll_wheel(move |event, _window, cx| {
                    let dy = match event.delta {
                        ScrollDelta::Pixels(p) => p.y / px(1.0),
                        ScrollDelta::Lines(l) => l.y * 5.0,
                    };
                    if dy != 0.0 {
                        let step = if dy > 0.0 { 1.0 } else { -1.0 };
                        let pk = prop_for_wheel.clone();
                        state_scroll.update(cx, |s, cx| {
                            match pk.as_str() {
                                "anchor_x" => s.nudge_anchor(step, 0.0),
                                "anchor_y" => s.nudge_anchor(0.0, step),
                                "pos_x" => s.nudge_position(step, 0.0),
                                "pos_y" => s.nudge_position(0.0, step),
                                "scale_x" => s.nudge_scale(step * 0.5, 0.0),
                                "scale_y" => s.nudge_scale(0.0, step * 0.5),
                                "rotation" => s.nudge_rotation(step * 0.5),
                                "opacity" => s.nudge_opacity(step * 0.5),
                                other => {
                                    if let Some(rest) = other.strip_prefix("fx:") {
                                        let parts: Vec<&str> = rest.split(':').collect();
                                        if parts.len() >= 2 {
                                            let _ = s.nudge_effect_param(parts[0], parts[1], step * 2.0);
                                        }
                                    }
                                }
                            }
                            cx.notify();
                        });
                    }
                })
                .child(label),
        )
        .child(btn_plus)
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

        let panel_entity = cx.entity();
        let s_up_prop = self.state.clone();
        let s_down_prop = self.state.clone();
        let s_del_prop = self.state.clone();

        let mut header_actions = h_flex()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .text_xs()
                    .child(layer_type_title),
            );

        if selected_layer.is_some() {
            header_actions = header_actions
                .child(
                    div()
                        .cursor_pointer()
                        .p_0p5()
                        .rounded_sm()
                        .hover(|s| s.bg(cx.theme().muted))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_up_prop.update(cx, |s, cx| {
                                let _ = s.move_selected_layer_up();
                                cx.notify();
                            });
                        })
                        .child(icon_box(IconName::ChevronUp)),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .p_0p5()
                        .rounded_sm()
                        .hover(|s| s.bg(cx.theme().muted))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_down_prop.update(cx, |s, cx| {
                                let _ = s.move_selected_layer_down();
                                cx.notify();
                            });
                        })
                        .child(icon_box(IconName::ChevronDown)),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .p_0p5()
                        .rounded_sm()
                        .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_del_prop.update(cx, |s, cx| {
                                let _ = s.delete_selected_layer();
                                cx.notify();
                            });
                        })
                        .child(icon_box(IconName::Trash)),
                );
        }

        div()
            .id("properties_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if !event.dragging() {
                    if this.scrub_prop.is_some() {
                        this.scrub_prop = None;
                        this.scrub_last_x = None;
                    }
                    return;
                }
                if let (Some(prop), Some(last_x)) = (this.scrub_prop.clone(), this.scrub_last_x) {
                    let curr_x = event.position.x / px(1.0);
                    let dx = curr_x - last_x;
                    if dx.abs() >= 1.0 {
                        this.apply_scrub_delta(&prop, dx, cx);
                        this.scrub_last_x = Some(curr_x);
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.scrub_prop = None;
                this.scrub_last_x = None;
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.scrub_prop = None;
                this.scrub_last_x = None;
            }))
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
                    .child(header_actions),
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
                                                .child(scrub_field(
                                                    SharedString::from(format!("param_radius_{}", eff_id)),
                                                    format!("fx:{}:radius:100", eff_id),
                                                    format!("{:.1} px", r),
                                                    Some(ElementId::from(SharedString::from(format!("param_radius_minus_{}", eff_id)))),
                                                    Some(ElementId::from(SharedString::from(format!("param_radius_plus_{}", eff_id)))),
                                                    &self.state,
                                                    &panel_entity,
                                                    cx,
                                                    move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "radius", -5.0); cx.notify(); }),
                                                    move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "radius", 5.0); cx.notify(); }),
                                                )),
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
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_brightness_{}", eff_id)),
                                                        format!("fx:{}:brightness:100", eff_id),
                                                        format!("{:.1}", b),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_bm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_bm, "brightness", -5.0); cx.notify(); }),
                                                        move |cx| s_bp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_bp, "brightness", 5.0); cx.notify(); }),
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Contrast"))
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_contrast_{}", eff_id)),
                                                        format!("fx:{}:contrast:100", eff_id),
                                                        format!("{:.1}", c),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_cm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_cm, "contrast", -5.0); cx.notify(); }),
                                                        move |cx| s_cp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_cp, "contrast", 5.0); cx.notify(); }),
                                                    )),
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
                                                .child(scrub_field(
                                                    SharedString::from(format!("param_amount_{}", eff_id)),
                                                    format!("fx:{}:amount:100", eff_id),
                                                    format!("{:.0} %", a),
                                                    None,
                                                    None,
                                                    &self.state,
                                                    &panel_entity,
                                                    cx,
                                                    move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "amount", -10.0); cx.notify(); }),
                                                    move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "amount", 10.0); cx.notify(); }),
                                                )),
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
                                                .child(scrub_field(
                                                    SharedString::from(format!("param_amount_{}", eff_id)),
                                                    format!("fx:{}:amount:100", eff_id),
                                                    format!("{:.0} %", a),
                                                    None,
                                                    None,
                                                    &self.state,
                                                    &panel_entity,
                                                    cx,
                                                    move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "amount", -10.0); cx.notify(); }),
                                                    move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "amount", 10.0); cx.notify(); }),
                                                )),
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
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_distance_{}", eff_id)),
                                                        format!("fx:{}:distance:50", eff_id),
                                                        format!("{:.1} px", d),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_dm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_dm, "distance", -2.0); cx.notify(); }),
                                                        move |cx| s_dp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_dp, "distance", 2.0); cx.notify(); }),
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Softness"))
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_softness_{}", eff_id)),
                                                        format!("fx:{}:softness:50", eff_id),
                                                        format!("{:.1} px", s_val),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_sm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_sm, "softness", -2.0); cx.notify(); }),
                                                        move |cx| s_sp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_sp, "softness", 2.0); cx.notify(); }),
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Opacity"))
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_opacity_{}", eff_id)),
                                                        format!("fx:{}:opacity:100", eff_id),
                                                        format!("{:.0} %", o),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_om.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_om, "opacity", -10.0); cx.notify(); }),
                                                        move |cx| s_op.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_op, "opacity", 10.0); cx.notify(); }),
                                                    )),
                                            );
                                    }
                                    EffectType::GlslShader { param1, param2, param3, param4, code } => {
                                        let p1 = param1.value;
                                        let p2 = param2.value;
                                        let p3 = param3.value;
                                        let p4 = param4.value;
                                        let s_p1m = self.state.clone();
                                        let s_p1p = self.state.clone();
                                        let s_p2m = self.state.clone();
                                        let s_p2p = self.state.clone();
                                        let s_p3m = self.state.clone();
                                        let s_p3p = self.state.clone();
                                        let s_p4m = self.state.clone();
                                        let s_p4p = self.state.clone();
                                        let id_p1m = eff_id.clone();
                                        let id_p1p = eff_id.clone();
                                        let id_p2m = eff_id.clone();
                                        let id_p2p = eff_id.clone();
                                        let id_p3m = eff_id.clone();
                                        let id_p3p = eff_id.clone();
                                        let id_p4m = eff_id.clone();
                                        let id_p4p = eff_id.clone();
                                        let code_preview = if code.len() > 60 {
                                            format!("{}...", &code[..60])
                                        } else {
                                            code.clone()
                                        };

                                        let mut presets_bar = h_flex().gap_1().items_center().flex_wrap();
                                        for (p_name, p_code) in EditorState::GLSL_PRESETS {
                                            let s_preset = self.state.clone();
                                            let eff_id_preset = eff_id.clone();
                                            let p_name_str = *p_name;
                                            let p_code_str = (*p_code).to_string();
                                            presets_bar = presets_bar.child(
                                                div()
                                                    .cursor_pointer()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(cx.theme().muted)
                                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                                    .text_xs()
                                                    .child(p_name_str)
                                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                        let c = p_code_str.clone();
                                                        let id = eff_id_preset.clone();
                                                        s_preset.update(cx, |s, cx| {
                                                            let _ = s.set_glsl_code(&id, c);
                                                            cx.notify();
                                                        });
                                                    }),
                                            );
                                        }

                                        effect_box = effect_box
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("P1 (Speed)"))
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_p1_{}", eff_id)),
                                                        format!("fx:{}:param1:10", eff_id),
                                                        format!("{:.2}", p1),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_p1m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p1m, "param1", -0.5); cx.notify(); }),
                                                        move |cx| s_p1p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p1p, "param1", 0.5); cx.notify(); }),
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("P2 (Intensity)"))
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_p2_{}", eff_id)),
                                                        format!("fx:{}:param2:100", eff_id),
                                                        format!("{:.1}", p2),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_p2m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p2m, "param2", -5.0); cx.notify(); }),
                                                        move |cx| s_p2p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p2p, "param2", 5.0); cx.notify(); }),
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("P3 (Scale)"))
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_p3_{}", eff_id)),
                                                        format!("fx:{}:param3:10", eff_id),
                                                        format!("{:.2}", p3),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_p3m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p3m, "param3", -0.5); cx.notify(); }),
                                                        move |cx| s_p3p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p3p, "param3", 0.5); cx.notify(); }),
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("P4 (Opacity)"))
                                                    .child(scrub_field(
                                                        SharedString::from(format!("param_p4_{}", eff_id)),
                                                        format!("fx:{}:param4:100", eff_id),
                                                        format!("{:.1}", p4),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |cx| s_p4m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p4m, "param4", -5.0); cx.notify(); }),
                                                        move |cx| s_p4p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p4p, "param4", 5.0); cx.notify(); }),
                                                    )),
                                            )
                                            .child(
                                                v_flex()
                                                    .gap_1()
                                                    .mt_1()
                                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Presets:"))
                                                    .child(presets_bar)
                                                    .child(
                                                        div()
                                                            .p_1p5()
                                                            .rounded_sm()
                                                            .bg(cx.theme().muted)
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .overflow_hidden()
                                                            .child(code_preview),
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
                                        .child(scrub_field(
                                            "prop_anchor_x",
                                            "anchor_x".to_string(),
                                            format!("X: {:.1}", anchor.x),
                                            None,
                                            None,
                                            &self.state,
                                            &panel_entity,
                                            cx,
                                            move |cx| s_anchor_mx.update(cx, |s, cx| { s.nudge_anchor(-10.0, 0.0); cx.notify(); }),
                                            move |cx| s_anchor_px.update(cx, |s, cx| { s.nudge_anchor(10.0, 0.0); cx.notify(); }),
                                        ))
                                        .child(scrub_field(
                                            "prop_anchor_y",
                                            "anchor_y".to_string(),
                                            format!("Y: {:.1}", anchor.y),
                                            None,
                                            None,
                                            &self.state,
                                            &panel_entity,
                                            cx,
                                            move |cx| s_anchor_my.update(cx, |s, cx| { s.nudge_anchor(0.0, -10.0); cx.notify(); }),
                                            move |cx| s_anchor_py.update(cx, |s, cx| { s.nudge_anchor(0.0, 10.0); cx.notify(); }),
                                        )),
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
                                        .child(scrub_field(
                                            "prop_pos_x",
                                            "pos_x".to_string(),
                                            format!("X: {:.1}", pos.x),
                                            None,
                                            None,
                                            &self.state,
                                            &panel_entity,
                                            cx,
                                            move |cx| s_pos_mx.update(cx, |s, cx| { s.nudge_position(-10.0, 0.0); cx.notify(); }),
                                            move |cx| s_pos_px.update(cx, |s, cx| { s.nudge_position(10.0, 0.0); cx.notify(); }),
                                        ))
                                        .child(scrub_field(
                                            "prop_pos_y",
                                            "pos_y".to_string(),
                                            format!("Y: {:.1}", pos.y),
                                            None,
                                            None,
                                            &self.state,
                                            &panel_entity,
                                            cx,
                                            move |cx| s_pos_my.update(cx, |s, cx| { s.nudge_position(0.0, -10.0); cx.notify(); }),
                                            move |cx| s_pos_py.update(cx, |s, cx| { s.nudge_position(0.0, 10.0); cx.notify(); }),
                                        )),
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
                                        .child(scrub_field(
                                            "prop_scale_x",
                                            "scale_x".to_string(),
                                            format!("{:.1} %", sc.x),
                                            None,
                                            None,
                                            &self.state,
                                            &panel_entity,
                                            cx,
                                            move |cx| s_scale_mx.update(cx, |s, cx| { s.nudge_scale(-10.0, 0.0); cx.notify(); }),
                                            move |cx| s_scale_px.update(cx, |s, cx| { s.nudge_scale(10.0, 0.0); cx.notify(); }),
                                        ))
                                        .child(scrub_field(
                                            "prop_scale_y",
                                            "scale_y".to_string(),
                                            format!("{:.1} %", sc.y),
                                            None,
                                            None,
                                            &self.state,
                                            &panel_entity,
                                            cx,
                                            move |cx| s_scale_my.update(cx, |s, cx| { s.nudge_scale(0.0, -10.0); cx.notify(); }),
                                            move |cx| s_scale_py.update(cx, |s, cx| { s.nudge_scale(0.0, 10.0); cx.notify(); }),
                                        )),
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
                                .child(scrub_field(
                                    "prop_rotation",
                                    "rotation".to_string(),
                                    format!("{:.1}°", rot),
                                    None,
                                    None,
                                    &self.state,
                                    &panel_entity,
                                    cx,
                                    move |cx| s_rot_m.update(cx, |s, cx| { s.nudge_rotation(-15.0); cx.notify(); }),
                                    move |cx| s_rot_p.update(cx, |s, cx| { s.nudge_rotation(15.0); cx.notify(); }),
                                )),

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
                                .child(scrub_field(
                                    "prop_opacity",
                                    "opacity".to_string(),
                                    format!("{:.1} %", op),
                                    None,
                                    None,
                                    &self.state,
                                    &panel_entity,
                                    cx,
                                    move |cx| s_op_m.update(cx, |s, cx| { s.nudge_opacity(-10.0); cx.notify(); }),
                                    move |cx| s_op_p.update(cx, |s, cx| { s.nudge_opacity(10.0); cx.notify(); }),
                                )),

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
                    .child(effect_item_row("linear_wipe", "Linear Wipe", EffectType::invert(50.0), &self.state, cx))
                    // Category 6: Custom Shaders
                    .child(category_header("▼ Custom Shaders (GLSL/WGSL)", IconName::Code, cx))
                    .child(effect_item_row("custom_glsl", "Custom GLSL Shader", EffectType::glsl_shader(project::Effect::default_glsl_code(), 1.0, 50.0, 1.0, 100.0), &self.state, cx)),
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

pub const BLEND_MODE_GROUPS: &[(&str, &[BlendMode])] = &[
    ("Normal", &[BlendMode::Normal, BlendMode::Dissolve]),
    ("Darken", &[BlendMode::Darken, BlendMode::Multiply, BlendMode::ColorBurn]),
    ("Lighten", &[BlendMode::Lighten, BlendMode::Screen, BlendMode::ColorDodge, BlendMode::Add]),
    ("Contrast", &[BlendMode::Overlay, BlendMode::SoftLight, BlendMode::HardLight]),
    ("Inversion", &[BlendMode::Difference, BlendMode::Exclusion, BlendMode::Subtract]),
    ("Component", &[BlendMode::Hue, BlendMode::Saturation, BlendMode::Color, BlendMode::Luminosity]),
];



fn next_matte_mode(mode: TrackMatteMode) -> TrackMatteMode {
    match mode {
        TrackMatteMode::None => TrackMatteMode::Alpha,
        TrackMatteMode::Alpha => TrackMatteMode::AlphaInverted,
        TrackMatteMode::AlphaInverted => TrackMatteMode::Luma,
        TrackMatteMode::Luma => TrackMatteMode::LumaInverted,
        TrackMatteMode::LumaInverted => TrackMatteMode::None,
    }
}

#[allow(clippy::too_many_arguments)]
fn timeline_stopwatch_nav(
    state: &Entity<EditorState>,
    layer_id: &str,
    prop_path: &'static str,
    is_animated: bool,
    has_kf_at_playhead: bool,
    has_prev_kf: bool,
    has_next_kf: bool,
    cx: &App,
) -> Div {
    let s_toggle = state.clone();
    let s_prev = state.clone();
    let s_kf = state.clone();
    let s_next = state.clone();
    let lid1 = layer_id.to_string();
    let lid2 = layer_id.to_string();
    let lid3 = layer_id.to_string();
    let lid4 = layer_id.to_string();

    let stopwatch_btn = div()
        .cursor_pointer()
        .p_0p5()
        .rounded_sm()
        .hover(|s| s.bg(cx.theme().muted))
        .text_color(if is_animated {
            rgb(0x38bdf8).into()
        } else {
            cx.theme().muted_foreground
        })
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            s_toggle.update(cx, |s, cx| {
                s.toggle_layer_property_animation(&lid1, prop_path);
                cx.notify();
            });
        })
        .child(icon_box(IconName::Timer));

    let nav = if is_animated {
        h_flex()
            .gap_0p5()
            .items_center()
            .child(
                div()
                    .cursor_pointer()
                    .px_0p5()
                    .text_xs()
                    .text_color(if has_prev_kf {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground.opacity(0.3)
                    })
                    .hover(|s| s.bg(cx.theme().muted))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        if has_prev_kf {
                            s_prev.update(cx, |s, cx| {
                                s.seek_previous_keyframe(&lid2, prop_path);
                                cx.notify();
                            });
                        }
                    })
                    .child("◂"),
            )
            .child(
                div()
                    .cursor_pointer()
                    .px_0p5()
                    .text_xs()
                    .text_color(if has_kf_at_playhead {
                        rgb(0xf59e0b).into()
                    } else {
                        cx.theme().muted_foreground
                    })
                    .hover(|s| s.text_color(rgb(0xffffff)))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        s_kf.update(cx, |s, cx| {
                            s.toggle_layer_keyframe_at_current_time(&lid3, prop_path);
                            cx.notify();
                        });
                    })
                    .child("◆"),
            )
            .child(
                div()
                    .cursor_pointer()
                    .px_0p5()
                    .text_xs()
                    .text_color(if has_next_kf {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground.opacity(0.3)
                    })
                    .hover(|s| s.bg(cx.theme().muted))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        if has_next_kf {
                            s_next.update(cx, |s, cx| {
                                s.seek_next_keyframe(&lid4, prop_path);
                                cx.notify();
                            });
                        }
                    })
                    .child("▸"),
            )
    } else {
        h_flex().w(px(28.))
    };

    h_flex()
        .gap_1()
        .items_center()
        .child(stopwatch_btn)
        .child(nav)
}

fn timeline_keyframe_lane(
    keyframe_times: &[f64],
    total_duration_secs: f64,
    current_time_secs: f64,
    fps: f64,
    playhead_percent: f32,
    state: &Entity<EditorState>,
    cx: &App,
) -> Div {
    let mut lane = div()
        .flex_1()
        .h(px(24.))
        .relative()
        .border_b_1()
        .border_color(cx.theme().border.opacity(0.3));

    lane = lane.child(
        div()
            .absolute()
            .top(px(11.))
            .left_0()
            .right_0()
            .h(px(1.))
            .bg(cx.theme().border.opacity(0.15)),
    );

    for &t in keyframe_times {
        let percent = (t / total_duration_secs.max(0.001) * 100.0).clamp(0.0, 100.0) as f32;
        let is_at_playhead = (t - current_time_secs).abs() < (0.5 / fps);
        let s_seek = state.clone();
        let target_tc = TimeCode::from_seconds(t, fps);

        lane = lane.child(
            div()
                .absolute()
                .top(px(4.))
                .left(relative(percent / 100.0))
                .ml(px(-6.))
                .w(px(12.))
                .h(px(14.))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_bold()
                .text_color(if is_at_playhead {
                    rgb(0xf59e0b)
                } else {
                    rgb(0x38bdf8)
                })
                .hover(|s| s.text_color(rgb(0xffffff)))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    s_seek.update(cx, |s, cx| {
                        s.clock.seek(target_tc);
                        cx.notify();
                    });
                })
                .child("◆"),
        );
    }

    lane = lane.child(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(1.))
            .bg(rgb(0xef4444))
            .left(relative(playhead_percent / 100.0)),
    );

    lane
}

fn timeline_stepper<FM, FP>(
    label: &'static str,
    val_str: String,
    on_minus: FM,
    on_plus: FP,
    cx: &App,
) -> Div
where
    FM: Fn(&mut App) + 'static,
    FP: Fn(&mut App) + 'static,
{
    h_flex()
        .gap_1()
        .items_center()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .cursor_pointer()
                .px_1()
                .rounded_sm()
                .bg(cx.theme().muted)
                .hover(|s| s.bg(cx.theme().accent))
                .text_xs()
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    on_minus(cx);
                })
                .child("-"),
        )
        .child(
            div()
                .px_1p5()
                .py_0p5()
                .rounded_sm()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .font_medium()
                .text_color(cx.theme().foreground)
                .child(val_str),
        )
        .child(
            div()
                .cursor_pointer()
                .px_1()
                .rounded_sm()
                .bg(cx.theme().muted)
                .hover(|s| s.bg(cx.theme().accent))
                .text_xs()
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    on_plus(cx);
                })
                .child("+"),
        )
}

#[derive(Clone, Debug)]
pub enum ContextMenuTarget {
    Layer(String),
    Effect { layer_id: String, effect_id: String },
    Property { layer_id: String, prop_path: &'static str },
}

#[derive(Clone, Debug)]
pub struct ContextMenuState {
    pub target: ContextMenuTarget,
}

pub struct TimelinePanel {
    focus_handle: FocusHandle,
    state: Entity<EditorState>,
    _subscription: Subscription,
    expanded_layers: HashSet<String>,
    expanded_groups: HashSet<String>,
    pub active_blend_dropdown: Option<String>,
    pub context_menu: Option<ContextMenuState>,
}

impl TimelinePanel {
    pub fn new(state: Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        let _subscription = cx.observe(&state, |_this, _state, cx| {
            cx.notify();
        });
        let expanded_layers = HashSet::new();
        let expanded_groups = HashSet::new();
        Self {
            focus_handle: cx.focus_handle(),
            state,
            _subscription,
            expanded_layers,
            expanded_groups,
            active_blend_dropdown: None,
            context_menu: None,
        }
    }

    pub fn open_blend_dropdown(&mut self, layer_id: String) {
        self.active_blend_dropdown = Some(layer_id);
    }

    pub fn close_blend_dropdown(&mut self) {
        self.active_blend_dropdown = None;
    }

    pub fn open_context_menu(&mut self, target: ContextMenuTarget) {
        self.context_menu = Some(ContextMenuState { target });
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
    }

    pub fn standalone(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new(state, cx)
    }

    pub fn is_layer_expanded(&self, id: &str) -> bool {
        self.expanded_layers.contains(id)
    }

    pub fn toggle_layer_expanded(&mut self, id: &str) {
        if self.expanded_layers.contains(id) {
            self.expanded_layers.remove(id);
        } else {
            self.expanded_layers.insert(id.to_string());
        }
    }

    pub fn is_group_expanded(&self, key: &str) -> bool {
        self.expanded_groups.contains(key)
    }

    pub fn toggle_group_expanded(&mut self, key: &str) {
        if self.expanded_groups.contains(key) {
            self.expanded_groups.remove(key);
        } else {
            self.expanded_groups.insert(key.to_string());
        }
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
        let fps = comp_opt.map(|c| c.frame_rate).unwrap_or(30.0);
        let total_duration_secs = comp_opt.map(|c| c.duration_seconds()).unwrap_or(5.0);
        let current_time_secs = state.clock.position_seconds();

        let s_start = self.state.clone();
        let s_step_prev = self.state.clone();
        let s_play = self.state.clone();
        let s_step_next = self.state.clone();
        let s_end = self.state.clone();
        let s_up_header = self.state.clone();
        let s_down_header = self.state.clone();
        let s_del_header = self.state.clone();

        let in_str = "00:00:00:00";
        let out_str = comp_opt.map(|c| format!("{}", c.duration)).unwrap_or_else(|| "00:00:05:00".to_string());

        let playhead_percent = (current_frame as f32 / total_frames.max(1) as f32 * 100.0).clamp(0.0, 100.0);
        let panel_entity = cx.entity().clone();

        let mut timeline_rows: Vec<Div> = Vec::new();

        if let Some(comp) = comp_opt {
            for (idx, layer) in comp.layers.iter().enumerate() {
                let is_selected = state.selected_layer_id.as_deref() == Some(&layer.id);
                let label_color = layer.label_color(idx);
                let is_layer_exp = self.expanded_layers.contains(&layer.id);

                let sel_state = self.state.clone();
                let vis_state = self.state.clone();
                let solo_state = self.state.clone();
                let lock_state = self.state.clone();
                let matte_state = self.state.clone();
                let parent_state = self.state.clone();
                let s_up = self.state.clone();
                let s_down = self.state.clone();
                let s_del = self.state.clone();

                let lid = layer.id.clone();
                let lid_vis = layer.id.clone();
                let lid_solo = layer.id.clone();
                let lid_lock = layer.id.clone();
                let lid_matte = layer.id.clone();
                let lid_parent = layer.id.clone();
                let lid_up = layer.id.clone();
                let lid_down = layer.id.clone();
                let lid_del = layer.id.clone();
                let current_matte = layer.matte_mode;
                let current_parent = layer.parent_id.clone();

                let p_twirl = panel_entity.clone();
                let lid_twirl = layer.id.clone();

                let in_ratio = (layer.in_point.frames() as f32 / total_frames.max(1) as f32).clamp(0.0, 1.0);
                let out_ratio = (layer.out_point.frames() as f32 / total_frames.max(1) as f32).clamp(0.0, 1.0);
                let span_w = ((out_ratio - in_ratio) * 100.0).max(5.0);
                let span_left = in_ratio * 100.0;

                // --- 1. Main Layer Row ---
                let mut left_col = h_flex()
                    .w(px(380.))
                    .h(px(26.))
                    .px_2()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .cursor_pointer();

                if is_selected {
                    left_col = left_col
                        .bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground);
                } else {
                    left_col = left_col
                        .bg(cx.theme().background)
                        .text_color(cx.theme().foreground)
                        .hover(|s| s.bg(cx.theme().muted));
                }

                let p_layer_ctx = panel_entity.clone();
                let lid_layer_ctx = layer.id.clone();
                let left_col = left_col
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        sel_state.update(cx, |s, cx| {
                            s.select_layer(Some(lid.clone()));
                            cx.notify();
                        });
                    })
                    .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                        p_layer_ctx.update(cx, |this, cx| {
                            this.open_context_menu(ContextMenuTarget::Layer(lid_layer_ctx.clone()));
                            cx.notify();
                        });
                    })
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            // Twirl arrow
                            .child(
                                div()
                                    .cursor_pointer()
                                    .w(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|s| s.text_color(rgb(0x38bdf8)))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        p_twirl.update(cx, |this, cx| {
                                            this.toggle_layer_expanded(&lid_twirl);
                                            cx.notify();
                                        });
                                    })
                                    .child(if is_layer_exp { "▾" } else { "▸" }),
                            )
                            // Layer index
                            .child(div().w(px(14.)).text_color(cx.theme().muted_foreground).child(format!("{}", idx + 1)))
                            // Visibility (Eye)
                            .child(
                                div()
                                    .cursor_pointer()
                                    .text_color(if layer.visible { cx.theme().foreground } else { cx.theme().muted_foreground })
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        vis_state.update(cx, |s, cx| {
                                            s.toggle_layer_visibility(&lid_vis);
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(if layer.visible { IconName::Eye } else { IconName::EyeOff })),
                            )
                            // Lock
                            .child(
                                div()
                                    .cursor_pointer()
                                    .text_color(if layer.locked { rgb(0xf59e0b).into() } else { cx.theme().muted_foreground.opacity(0.5) })
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        lock_state.update(cx, |s, cx| {
                                            s.toggle_layer_lock(&lid_lock);
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::Lock)),
                            )
                            // Solo
                            .child(
                                div()
                                    .cursor_pointer()
                                    .px_1()
                                    .rounded_sm()
                                    .font_bold()
                                    .text_xs()
                                    .bg(if layer.is_solo() { rgb(0xf59e0b).into() } else { cx.theme().secondary })
                                    .text_color(if layer.is_solo() { rgb(0x000000).into() } else { cx.theme().muted_foreground })
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        solo_state.update(cx, |s, cx| {
                                            s.toggle_layer_solo(&lid_solo);
                                            cx.notify();
                                        });
                                    })
                                    .child("S"),
                            )
                            // AE Color Tag Swatch
                            .child(
                                div()
                                    .w(px(10.))
                                    .h(px(10.))
                                    .rounded_sm()
                                    .bg(Rgba { r: label_color.r, g: label_color.g, b: label_color.b, a: label_color.a }),
                            )
                            // Reorder arrows
                            .child(
                                div()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(cx.theme().muted))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_up.update(cx, |s, cx| {
                                            let _ = s.move_layer_up(&lid_up);
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::ChevronUp)),
                            )
                            .child(
                                div()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(cx.theme().muted))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_down.update(cx, |s, cx| {
                                            let _ = s.move_layer_down(&lid_down);
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::ChevronDown)),
                            )
                            // Layer Name
                            .child(
                                div()
                                    .max_w(px(110.))
                                    .truncate()
                                    .font_semibold()
                                    .child(layer.name.clone()),
                            ),
                    )
                    // Right controls: Mode, Matte, Parent, Delete
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            // Blend Mode
                            .child({
                                let p_blend = panel_entity.clone();
                                let lid_bm = layer.id.clone();
                                div()
                                    .cursor_pointer()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().secondary)
                                    .text_color(cx.theme().muted_foreground)
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .text_xs()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        p_blend.update(cx, |this, cx| {
                                            if this.active_blend_dropdown.as_deref() == Some(&lid_bm) {
                                                this.close_blend_dropdown();
                                            } else {
                                                this.open_blend_dropdown(lid_bm.clone());
                                            }
                                            cx.notify();
                                        });
                                    })
                                    .child(layer.blend_mode.as_str())
                            })
                            // Track Matte
                            .child(
                                div()
                                    .cursor_pointer()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().secondary)
                                    .text_color(cx.theme().muted_foreground)
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .text_xs()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        let next = next_matte_mode(current_matte);
                                        matte_state.update(cx, |s, cx| {
                                            s.set_layer_track_matte(&lid_matte, next, None);
                                            cx.notify();
                                        });
                                    })
                                    .child(match layer.matte_mode {
                                        TrackMatteMode::None => "None",
                                        TrackMatteMode::Alpha => "Alpha",
                                        TrackMatteMode::AlphaInverted => "Inv Alpha",
                                        TrackMatteMode::Luma => "Luma",
                                        TrackMatteMode::LumaInverted => "Inv Luma",
                                    }),
                            )
                            // Parent & Link
                            .child(
                                div()
                                    .cursor_pointer()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(cx.theme().secondary)
                                    .text_color(cx.theme().muted_foreground)
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .text_xs()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        parent_state.update(cx, |s, cx| {
                                            let next_p = if current_parent.is_some() { None } else { Some("layer_bg".to_string()) };
                                            s.set_layer_parent(&lid_parent, next_p);
                                            cx.notify();
                                        });
                                    })
                                    .child(layer.parent_id.clone().unwrap_or_else(|| "None".to_string())),
                            )
                            // Delete Layer
                            .child(
                                div()
                                    .cursor_pointer()
                                    .p_0p5()
                                    .rounded_sm()
                                    .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_del.update(cx, |s, cx| {
                                            let _ = s.remove_layer_by_id(&lid_del);
                                            cx.notify();
                                        });
                                    })
                                    .child(icon_box(IconName::Trash)),
                            ),
                    );

                let span_state = self.state.clone();
                let lid_span = layer.id.clone();

                let track_col = div()
                    .flex_1()
                    .h(px(26.))
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
                            .bg(Rgba {
                                r: label_color.r,
                                g: label_color.g,
                                b: label_color.b,
                                a: if is_selected { 0.95 } else { 0.75 },
                            })
                            .border_1()
                            .border_color(Rgba { r: label_color.r, g: label_color.g, b: label_color.b, a: 1.0 })
                            .opacity(if layer.visible { 1.0 } else { 0.35 })
                            .px_2()
                            .text_xs()
                            .text_color(rgb(0xffffff))
                            .child(format!("{} [{} - {}]", layer.name, layer.in_point, layer.out_point)),
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

                let main_row = h_flex()
                    .h(px(26.))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .items_center()
                    .child(left_col)
                    .child(track_col);

                timeline_rows.push(main_row);

                // --- 2. Twirled-Down Hierarchy ---
                if is_layer_exp {
                    // Group: Transform
                    let trans_key = format!("{}:transform", layer.id);
                    let is_trans_exp = self.expanded_groups.contains(&trans_key);
                    let p_trans = panel_entity.clone();
                    let tkey_click = trans_key.clone();

                    let trans_header_left = h_flex()
                        .w(px(380.))
                        .h(px(24.))
                        .pl_6()
                        .pr_2()
                        .border_r_1()
                        .border_color(cx.theme().border)
                        .items_center()
                        .gap_1p5()
                        .bg(cx.theme().secondary.opacity(0.4))
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().foreground)
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            p_trans.update(cx, |this, cx| {
                                this.toggle_group_expanded(&tkey_click);
                                cx.notify();
                            });
                        })
                        .child(div().w(px(10.)).child(if is_trans_exp { "▾" } else { "▸" }))
                        .child(icon_box(IconName::Move))
                        .child("Transform");

                    let trans_header_lane = div()
                        .flex_1()
                        .h(px(24.))
                        .relative()
                        .bg(cx.theme().secondary.opacity(0.2))
                        .border_b_1()
                        .border_color(cx.theme().border.opacity(0.2))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(rgb(0xef4444))
                                .left(relative(playhead_percent / 100.0)),
                        );

                    timeline_rows.push(
                        h_flex()
                            .h(px(24.))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .child(trans_header_left)
                            .child(trans_header_lane),
                    );

                    if is_trans_exp {
                        // 1. Anchor Point
                        let ap = layer.transform.anchor_point.evaluate_at(&current_tc);
                        let ap_anim = layer.transform.anchor_point.is_animated();
                        let ap_has_kf = layer.transform.anchor_point.has_keyframe_at(&current_tc);
                        let ap_prev = layer.transform.anchor_point.previous_keyframe_time(&current_tc).is_some();
                        let ap_next = layer.transform.anchor_point.next_keyframe_time(&current_tc).is_some();
                        let ap_times: Vec<f64> = layer.transform.anchor_point.keyframes().iter().map(|k| k.time_seconds()).collect();
                        let s_ap_mx = self.state.clone();
                        let s_ap_px = self.state.clone();
                        let s_ap_my = self.state.clone();
                        let s_ap_py = self.state.clone();
                        let lid_ap1 = layer.id.clone();
                        let lid_ap2 = layer.id.clone();
                        let lid_ap3 = layer.id.clone();
                        let lid_ap4 = layer.id.clone();

                        let p_prop_ap = panel_entity.clone();
                        let lid_prop_ap = layer.id.clone();
                        let ap_left = h_flex()
                            .w(px(380.))
                            .h(px(24.))
                            .pl(px(32.))
                            .pr_2()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                p_prop_ap.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_ap.clone(),
                                        prop_path: "transform.anchor_point",
                                    });
                                    cx.notify();
                                });
                            })
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(timeline_stopwatch_nav(&self.state, &layer.id, "transform.anchor_point", ap_anim, ap_has_kf, ap_prev, ap_next, cx))
                                    .child(div().w(px(80.)).text_color(cx.theme().foreground).child("Anchor Point")),
                            )
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .child(timeline_stepper("X", format!("{:.0}", ap.x), move |cx| s_ap_mx.update(cx, |s, cx| { s.nudge_layer_anchor(&lid_ap1, -10.0, 0.0); cx.notify(); }), move |cx| s_ap_px.update(cx, |s, cx| { s.nudge_layer_anchor(&lid_ap2, 10.0, 0.0); cx.notify(); }), cx))
                                    .child(timeline_stepper("Y", format!("{:.0}", ap.y), move |cx| s_ap_my.update(cx, |s, cx| { s.nudge_layer_anchor(&lid_ap3, 0.0, -10.0); cx.notify(); }), move |cx| s_ap_py.update(cx, |s, cx| { s.nudge_layer_anchor(&lid_ap4, 0.0, 10.0); cx.notify(); }), cx)),
                            );
                        let ap_lane = timeline_keyframe_lane(&ap_times, total_duration_secs, current_time_secs, fps, playhead_percent, &self.state, cx);
                        timeline_rows.push(h_flex().h(px(24.)).items_center().child(ap_left).child(ap_lane));

                        // 2. Position
                        let pos = layer.transform.position.evaluate_at(&current_tc);
                        let pos_anim = layer.transform.position.is_animated();
                        let pos_has_kf = layer.transform.position.has_keyframe_at(&current_tc);
                        let pos_prev = layer.transform.position.previous_keyframe_time(&current_tc).is_some();
                        let pos_next = layer.transform.position.next_keyframe_time(&current_tc).is_some();
                        let pos_times: Vec<f64> = layer.transform.position.keyframes().iter().map(|k| k.time_seconds()).collect();
                        let s_pos_mx = self.state.clone();
                        let s_pos_px = self.state.clone();
                        let s_pos_my = self.state.clone();
                        let s_pos_py = self.state.clone();
                        let lid_pos1 = layer.id.clone();
                        let lid_pos2 = layer.id.clone();
                        let lid_pos3 = layer.id.clone();
                        let lid_pos4 = layer.id.clone();

                        let p_prop_pos = panel_entity.clone();
                        let lid_prop_pos = layer.id.clone();
                        let pos_left = h_flex()
                            .w(px(380.))
                            .h(px(24.))
                            .pl(px(32.))
                            .pr_2()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                p_prop_pos.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_pos.clone(),
                                        prop_path: "transform.position",
                                    });
                                    cx.notify();
                                });
                            })
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(timeline_stopwatch_nav(&self.state, &layer.id, "transform.position", pos_anim, pos_has_kf, pos_prev, pos_next, cx))
                                    .child(div().w(px(80.)).text_color(cx.theme().foreground).child("Position")),
                            )
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .child(timeline_stepper("X", format!("{:.0}", pos.x), move |cx| s_pos_mx.update(cx, |s, cx| { s.nudge_layer_position(&lid_pos1, -10.0, 0.0); cx.notify(); }), move |cx| s_pos_px.update(cx, |s, cx| { s.nudge_layer_position(&lid_pos2, 10.0, 0.0); cx.notify(); }), cx))
                                    .child(timeline_stepper("Y", format!("{:.0}", pos.y), move |cx| s_pos_my.update(cx, |s, cx| { s.nudge_layer_position(&lid_pos3, 0.0, -10.0); cx.notify(); }), move |cx| s_pos_py.update(cx, |s, cx| { s.nudge_layer_position(&lid_pos4, 0.0, 10.0); cx.notify(); }), cx)),
                            );
                        let pos_lane = timeline_keyframe_lane(&pos_times, total_duration_secs, current_time_secs, fps, playhead_percent, &self.state, cx);
                        timeline_rows.push(h_flex().h(px(24.)).items_center().child(pos_left).child(pos_lane));

                        // 3. Scale
                        let sc = layer.transform.scale.evaluate_at(&current_tc);
                        let sc_anim = layer.transform.scale.is_animated();
                        let sc_has_kf = layer.transform.scale.has_keyframe_at(&current_tc);
                        let sc_prev = layer.transform.scale.previous_keyframe_time(&current_tc).is_some();
                        let sc_next = layer.transform.scale.next_keyframe_time(&current_tc).is_some();
                        let sc_times: Vec<f64> = layer.transform.scale.keyframes().iter().map(|k| k.time_seconds()).collect();
                        let s_sc_mx = self.state.clone();
                        let s_sc_px = self.state.clone();
                        let s_sc_my = self.state.clone();
                        let s_sc_py = self.state.clone();
                        let lid_sc1 = layer.id.clone();
                        let lid_sc2 = layer.id.clone();
                        let lid_sc3 = layer.id.clone();
                        let lid_sc4 = layer.id.clone();

                        let p_prop_sc = panel_entity.clone();
                        let lid_prop_sc = layer.id.clone();
                        let sc_left = h_flex()
                            .w(px(380.))
                            .h(px(24.))
                            .pl(px(32.))
                            .pr_2()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                p_prop_sc.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_sc.clone(),
                                        prop_path: "transform.scale",
                                    });
                                    cx.notify();
                                });
                            })
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(timeline_stopwatch_nav(&self.state, &layer.id, "transform.scale", sc_anim, sc_has_kf, sc_prev, sc_next, cx))
                                    .child(div().w(px(80.)).text_color(cx.theme().foreground).child("Scale")),
                            )
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .child(timeline_stepper("X", format!("{:.0}%", sc.x), move |cx| s_sc_mx.update(cx, |s, cx| { s.nudge_layer_scale(&lid_sc1, -10.0, 0.0); cx.notify(); }), move |cx| s_sc_px.update(cx, |s, cx| { s.nudge_layer_scale(&lid_sc2, 10.0, 0.0); cx.notify(); }), cx))
                                    .child(timeline_stepper("Y", format!("{:.0}%", sc.y), move |cx| s_sc_my.update(cx, |s, cx| { s.nudge_layer_scale(&lid_sc3, 0.0, -10.0); cx.notify(); }), move |cx| s_sc_py.update(cx, |s, cx| { s.nudge_layer_scale(&lid_sc4, 0.0, 10.0); cx.notify(); }), cx)),
                            );
                        let sc_lane = timeline_keyframe_lane(&sc_times, total_duration_secs, current_time_secs, fps, playhead_percent, &self.state, cx);
                        timeline_rows.push(h_flex().h(px(24.)).items_center().child(sc_left).child(sc_lane));

                        // 4. Rotation
                        let rot = layer.transform.rotation.evaluate_at(&current_tc);
                        let rot_anim = layer.transform.rotation.is_animated();
                        let rot_has_kf = layer.transform.rotation.has_keyframe_at(&current_tc);
                        let rot_prev = layer.transform.rotation.previous_keyframe_time(&current_tc).is_some();
                        let rot_next = layer.transform.rotation.next_keyframe_time(&current_tc).is_some();
                        let rot_times: Vec<f64> = layer.transform.rotation.keyframes().iter().map(|k| k.time_seconds()).collect();
                        let s_rot_m = self.state.clone();
                        let s_rot_p = self.state.clone();
                        let lid_rot1 = layer.id.clone();
                        let lid_rot2 = layer.id.clone();

                        let p_prop_rot = panel_entity.clone();
                        let lid_prop_rot = layer.id.clone();
                        let rot_left = h_flex()
                            .w(px(380.))
                            .h(px(24.))
                            .pl(px(32.))
                            .pr_2()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                p_prop_rot.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_rot.clone(),
                                        prop_path: "transform.rotation",
                                    });
                                    cx.notify();
                                });
                            })
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(timeline_stopwatch_nav(&self.state, &layer.id, "transform.rotation", rot_anim, rot_has_kf, rot_prev, rot_next, cx))
                                    .child(div().w(px(80.)).text_color(cx.theme().foreground).child("Rotation")),
                            )
                            .child(
                                timeline_stepper("Angle", format!("{:.1}°", rot), move |cx| s_rot_m.update(cx, |s, cx| { s.nudge_layer_rotation(&lid_rot1, -15.0); cx.notify(); }), move |cx| s_rot_p.update(cx, |s, cx| { s.nudge_layer_rotation(&lid_rot2, 15.0); cx.notify(); }), cx),
                            );
                        let rot_lane = timeline_keyframe_lane(&rot_times, total_duration_secs, current_time_secs, fps, playhead_percent, &self.state, cx);
                        timeline_rows.push(h_flex().h(px(24.)).items_center().child(rot_left).child(rot_lane));

                        // 5. Opacity
                        let op = layer.opacity.evaluate_at(&current_tc);
                        let op_anim = layer.opacity.is_animated();
                        let op_has_kf = layer.opacity.has_keyframe_at(&current_tc);
                        let op_prev = layer.opacity.previous_keyframe_time(&current_tc).is_some();
                        let op_next = layer.opacity.next_keyframe_time(&current_tc).is_some();
                        let op_times: Vec<f64> = layer.opacity.keyframes().iter().map(|k| k.time_seconds()).collect();
                        let s_op_m = self.state.clone();
                        let s_op_p = self.state.clone();
                        let lid_op1 = layer.id.clone();
                        let lid_op2 = layer.id.clone();

                        let p_prop_op = panel_entity.clone();
                        let lid_prop_op = layer.id.clone();
                        let op_left = h_flex()
                            .w(px(380.))
                            .h(px(24.))
                            .pl(px(32.))
                            .pr_2()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                p_prop_op.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_op.clone(),
                                        prop_path: "opacity",
                                    });
                                    cx.notify();
                                });
                            })
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(timeline_stopwatch_nav(&self.state, &layer.id, "opacity", op_anim, op_has_kf, op_prev, op_next, cx))
                                    .child(div().w(px(80.)).text_color(cx.theme().foreground).child("Opacity")),
                            )
                            .child(
                                timeline_stepper("Op", format!("{:.0}%", op), move |cx| s_op_m.update(cx, |s, cx| { s.nudge_layer_opacity(&lid_op1, -10.0); cx.notify(); }), move |cx| s_op_p.update(cx, |s, cx| { s.nudge_layer_opacity(&lid_op2, 10.0); cx.notify(); }), cx),
                            );
                        let op_lane = timeline_keyframe_lane(&op_times, total_duration_secs, current_time_secs, fps, playhead_percent, &self.state, cx);
                        timeline_rows.push(h_flex().h(px(24.)).items_center().child(op_left).child(op_lane));
                    }

                    // Group: Effects
                    if !layer.effects.is_empty() {
                        let fx_grp_key = format!("{}:effects", layer.id);
                        let is_fx_grp_exp = self.expanded_groups.contains(&fx_grp_key);
                        let p_fx_grp = panel_entity.clone();
                        let fx_grp_click = fx_grp_key.clone();

                        let fx_header_left = h_flex()
                            .w(px(380.))
                            .h(px(24.))
                            .pl_6()
                            .pr_2()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .items_center()
                            .justify_between()
                            .bg(cx.theme().secondary.opacity(0.4))
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().foreground)
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                p_fx_grp.update(cx, |this, cx| {
                                    this.toggle_group_expanded(&fx_grp_click);
                                    cx.notify();
                                });
                            })
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(div().w(px(10.)).child(if is_fx_grp_exp { "▾" } else { "▸" }))
                                    .child(icon_box(IconName::SlidersHorizontal))
                                    .child(format!("Effects ({})", layer.effects.len())),
                            );

                        let fx_header_lane = div()
                            .flex_1()
                            .h(px(24.))
                            .relative()
                            .bg(cx.theme().secondary.opacity(0.2))
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.2))
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(1.))
                                    .bg(rgb(0xef4444))
                                    .left(relative(playhead_percent / 100.0)),
                            );

                        timeline_rows.push(
                            h_flex()
                                .h(px(24.))
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .items_center()
                                .child(fx_header_left)
                                .child(fx_header_lane),
                        );

                        if is_fx_grp_exp {
                            for effect in &layer.effects {
                                let fx_item_key = format!("{}:effect:{}", layer.id, effect.id);
                                let is_fx_item_exp = self.expanded_groups.contains(&fx_item_key);
                                let p_fx_item = panel_entity.clone();
                                let fx_item_click = fx_item_key.clone();

                                let s_fx_toggle = self.state.clone();
                                let s_fx_del = self.state.clone();
                                let lid_fx1 = layer.id.clone();
                                let lid_fx2 = layer.id.clone();
                                let eid1 = effect.id.clone();
                                let eid2 = effect.id.clone();

                                let fx_item_left = h_flex()
                                    .w(px(380.))
                                    .h(px(24.))
                                    .pl(px(32.))
                                    .pr_2()
                                    .border_r_1()
                                    .border_color(cx.theme().border)
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .bg(cx.theme().muted.opacity(0.3))
                                    .child(
                                        h_flex()
                                            .gap_1p5()
                                            .items_center()
                                            .child(
                                                div()
                                                    .cursor_pointer()
                                                    .w(px(10.))
                                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                        p_fx_item.update(cx, |this, cx| {
                                                            this.toggle_group_expanded(&fx_item_click);
                                                            cx.notify();
                                                        });
                                                    })
                                                    .child(if is_fx_item_exp { "▾" } else { "▸" }),
                                            )
                                            .child(
                                                div()
                                                    .cursor_pointer()
                                                    .text_color(if effect.enabled { cx.theme().foreground } else { cx.theme().muted_foreground })
                                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                        s_fx_toggle.update(cx, |s, cx| {
                                                            let _ = s.toggle_layer_effect_enabled(&lid_fx1, &eid1);
                                                            cx.notify();
                                                        });
                                                    })
                                                    .child(icon_box(if effect.enabled { IconName::Eye } else { IconName::EyeOff })),
                                            )
                                            .child(div().font_medium().child(effect.name.clone())),
                                    )
                                    .child(
                                        // Trash Delete Effect button
                                        div()
                                            .cursor_pointer()
                                            .p_0p5()
                                            .rounded_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_fx_del.update(cx, |s, cx| {
                                                    let _ = s.remove_layer_effect(&lid_fx2, &eid2);
                                                    cx.notify();
                                                });
                                            })
                                            .child(icon_box(IconName::Trash)),
                                    );

                                let p_fx_menu = panel_entity.clone();
                                let lid_fx_menu = layer.id.clone();
                                let eid_fx_menu = effect.id.clone();
                                let fx_item_left = fx_item_left
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                        p_fx_menu.update(cx, |this, cx| {
                                            this.open_context_menu(ContextMenuTarget::Effect {
                                                layer_id: lid_fx_menu.clone(),
                                                effect_id: eid_fx_menu.clone(),
                                            });
                                            cx.notify();
                                        });
                                    });

                                let fx_item_lane = div()
                                    .flex_1()
                                    .h(px(24.))
                                    .relative()
                                    .border_b_1()
                                    .border_color(cx.theme().border.opacity(0.2))
                                    .child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .bottom_0()
                                            .w(px(1.))
                                            .bg(rgb(0xef4444))
                                            .left(relative(playhead_percent / 100.0)),
                                    );

                                timeline_rows.push(
                                    h_flex()
                                        .h(px(24.))
                                        .border_b_1()
                                        .border_color(cx.theme().border)
                                        .items_center()
                                        .child(fx_item_left)
                                        .child(fx_item_lane),
                                );

                                if is_fx_item_exp {
                                    // Render animatable parameters for this effect
                                    let mut param_entries: Vec<(&'static str, &'static str, f32, f32)> = Vec::new();
                                    match &effect.effect_type {
                                        EffectType::GaussianBlur { radius } => {
                                            param_entries.push(("radius", "Blur Radius", radius.evaluate_at(&current_tc), 2.0));
                                        }
                                        EffectType::BrightnessContrast { brightness, contrast } => {
                                            param_entries.push(("brightness", "Brightness", brightness.evaluate_at(&current_tc), 5.0));
                                            param_entries.push(("contrast", "Contrast", contrast.evaluate_at(&current_tc), 5.0));
                                        }
                                        EffectType::Tint { amount, .. } => {
                                            param_entries.push(("amount", "Amount", amount.evaluate_at(&current_tc), 5.0));
                                        }
                                        EffectType::Invert { amount } => {
                                            param_entries.push(("amount", "Amount", amount.evaluate_at(&current_tc), 5.0));
                                        }
                                        EffectType::DropShadow { distance, softness, opacity, .. } => {
                                            param_entries.push(("distance", "Distance", distance.evaluate_at(&current_tc), 2.0));
                                            param_entries.push(("softness", "Softness", softness.evaluate_at(&current_tc), 2.0));
                                            param_entries.push(("opacity", "Opacity", opacity.evaluate_at(&current_tc), 5.0));
                                        }
                                        EffectType::GlslShader { param1, param2, param3, param4, .. } => {
                                            param_entries.push(("param1", "Param 1 (Speed)", param1.evaluate_at(&current_tc), 0.2));
                                            param_entries.push(("param2", "Param 2 (Boost)", param2.evaluate_at(&current_tc), 5.0));
                                            param_entries.push(("param3", "Param 3 (Scale)", param3.evaluate_at(&current_tc), 0.2));
                                            param_entries.push(("param4", "Param 4 (Blend)", param4.evaluate_at(&current_tc), 5.0));
                                        }
                                    }

                                    for (p_slug, p_label, p_val, p_step) in param_entries {
                                        let prop_path_static: &'static str = match p_slug {
                                            "radius" => "effect:radius",
                                            "brightness" => "effect:brightness",
                                            "contrast" => "effect:contrast",
                                            "amount" => "effect:amount",
                                            "distance" => "effect:distance",
                                            "softness" => "effect:softness",
                                            "opacity" => "effect:opacity",
                                            "param1" => "effect:param1",
                                            "param2" => "effect:param2",
                                            "param3" => "effect:param3",
                                            "param4" => "effect:param4",
                                            _ => "effect:param",
                                        };

                                        let prop_ref = effect.get_param_property(p_slug);
                                        let is_anim = prop_ref.map(|p| p.is_animated()).unwrap_or(false);
                                        let has_kf = prop_ref.map(|p| p.has_keyframe_at(&current_tc)).unwrap_or(false);
                                        let prev_kf = prop_ref.and_then(|p| p.previous_keyframe_time(&current_tc)).is_some();
                                        let next_kf = prop_ref.and_then(|p| p.next_keyframe_time(&current_tc)).is_some();
                                        let kf_times: Vec<f64> = prop_ref.map(|p| p.keyframes().iter().map(|k| k.time_seconds()).collect()).unwrap_or_default();

                                        let s_pm = self.state.clone();
                                        let s_pp = self.state.clone();
                                        let lid_p1 = layer.id.clone();
                                        let lid_p2 = layer.id.clone();
                                        let eid_p1 = effect.id.clone();
                                        let eid_p2 = effect.id.clone();

                                        let param_left = h_flex()
                                            .w(px(380.))
                                            .h(px(24.))
                                            .pl(px(44.))
                                            .pr_2()
                                            .border_r_1()
                                            .border_color(cx.theme().border)
                                            .items_center()
                                            .justify_between()
                                            .text_xs()
                                            .child(
                                                h_flex()
                                                    .gap_1()
                                                    .items_center()
                                                    .child(timeline_stopwatch_nav(&self.state, &layer.id, prop_path_static, is_anim, has_kf, prev_kf, next_kf, cx))
                                                    .child(div().w(px(100.)).truncate().text_color(cx.theme().foreground).child(p_label)),
                                            )
                                            .child(
                                                timeline_stepper("Val", format!("{:.1}", p_val), move |cx| s_pm.update(cx, |s, cx| { let _ = s.nudge_layer_effect_param(&lid_p1, &eid_p1, p_slug, -p_step); cx.notify(); }), move |cx| s_pp.update(cx, |s, cx| { let _ = s.nudge_layer_effect_param(&lid_p2, &eid_p2, p_slug, p_step); cx.notify(); }), cx),
                                            );

                                        let param_lane = timeline_keyframe_lane(&kf_times, total_duration_secs, current_time_secs, fps, playhead_percent, &self.state, cx);

                                        timeline_rows.push(
                                            h_flex()
                                                .h(px(24.))
                                                .border_b_1()
                                                .border_color(cx.theme().border)
                                                .items_center()
                                                .child(param_left)
                                                .child(param_lane),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut root = div()
            .id("timeline_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
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
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        div()
                                            .cursor_pointer()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .hover(|s| s.bg(cx.theme().accent))
                                            .text_xs()
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_up_header.update(cx, |s, cx| {
                                                    let _ = s.move_selected_layer_up();
                                                    cx.notify();
                                                });
                                            })
                                            .child(
                                                h_flex()
                                                    .gap_0p5()
                                                    .items_center()
                                                    .child(icon_box(IconName::ChevronUp))
                                                    .child("Up"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .cursor_pointer()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .hover(|s| s.bg(cx.theme().accent))
                                            .text_xs()
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_down_header.update(cx, |s, cx| {
                                                    let _ = s.move_selected_layer_down();
                                                    cx.notify();
                                                });
                                            })
                                            .child(
                                                h_flex()
                                                    .gap_0p5()
                                                    .items_center()
                                                    .child(icon_box(IconName::ChevronDown))
                                                    .child("Down"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .cursor_pointer()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                                            .text_xs()
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_del_header.update(cx, |s, cx| {
                                                    let _ = s.delete_selected_layer();
                                                    cx.notify();
                                                });
                                            })
                                            .child(
                                                h_flex()
                                                    .gap_0p5()
                                                    .items_center()
                                                    .child(icon_box(IconName::Trash))
                                                    .child("Delete"),
                                            ),
                                    ),
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
                            .w(px(380.))
                            .px_3()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child("Layer Name / Switches / Properties"),
                    )
                    .child({
                        let mut ruler_track = div()
                            .id("ruler_track")
                            .test_support()
                            .flex_1()
                            .relative()
                            .h_full()
                            .cursor_col_resize();

                        // 50 interactive scrub slices across the timeline ruler track
                        for slice_idx in 0..50 {
                            let scrub_pct = (slice_idx as f64) / 50.0;
                            let s_scrub = self.state.clone();
                            let target_time = scrub_pct * total_duration_secs;
                            ruler_track = ruler_track.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(relative(scrub_pct as f32))
                                    .w(relative(1.0 / 50.0))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_scrub.update(cx, |s, cx| {
                                            s.seek(target_time);
                                            cx.notify();
                                        });
                                    }),
                            );
                        }

                        ruler_track = ruler_track
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
                            );

                        ruler_track
                    }),
            )
            // Tracks area
            .child(
                v_flex()
                    .id("timeline")
                    .test_support()
                    .flex_1()
                    .overflow_hidden()
                    .children(timeline_rows),
            );

        // Blend Mode Dropdown Overlay
        if let Some(ref target_lid) = self.active_blend_dropdown {
            let s_bm = self.state.clone();
            let p_close = panel_entity.clone();
            let target_lid_str = target_lid.clone();

            let mut cat_columns = h_flex().gap_2().p_2();

            for (cat_name, modes) in BLEND_MODE_GROUPS {
                let mut col = v_flex().gap_0p5().w(px(95.));
                col = col.child(
                    div()
                        .font_semibold()
                        .text_xs()
                        .text_color(cx.theme().primary)
                        .px_1()
                        .py_0p5()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(*cat_name),
                );

                for &bm in *modes {
                    let s_item = s_bm.clone();
                    let p_close_item = p_close.clone();
                    let target_lid_item = target_lid_str.clone();

                    col = col.child(
                        div()
                            .cursor_pointer()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_item.update(cx, |s, cx| {
                                    s.set_layer_blend_mode(&target_lid_item, bm);
                                    cx.notify();
                                });
                                p_close_item.update(cx, |this, cx| {
                                    this.close_blend_dropdown();
                                    cx.notify();
                                });
                            })
                            .child(bm.as_str()),
                    );
                }
                cat_columns = cat_columns.child(col);
            }

            let p_close_bg = p_close.clone();
            let blend_dropdown_overlay = div()
                .id("blend_mode_dropdown")
                .test_support()
                .absolute()
                .top(px(40.))
                .left(px(180.))
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .rounded_md()
                .shadow_lg()
                .child(
                    v_flex()
                        .child(
                            h_flex()
                                .justify_between()
                                .items_center()
                                .px_2()
                                .py_1()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().secondary)
                                .child(div().font_bold().text_xs().child("Blend Modes"))
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_xs()
                                        .hover(|s| s.text_color(rgb(0xef4444)))
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            p_close_bg.update(cx, |this, cx| {
                                                this.close_blend_dropdown();
                                                cx.notify();
                                            });
                                        })
                                        .child("✕"),
                                ),
                        )
                        .child(cat_columns),
                );
            root = root.child(blend_dropdown_overlay);
        }

        // Context Menu Overlay
        if let Some(ref ctx_menu) = self.context_menu {
            let p_close = panel_entity.clone();
            let s_menu = self.state.clone();

            let mut menu_items = v_flex().gap_0p5().p_1();

            match &ctx_menu.target {
                ContextMenuTarget::Layer(lid) => {
                    let target_lid = lid.clone();
                    let s1 = s_menu.clone();
                    let p1 = p_close.clone();
                    let t1 = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s1.update(cx, |s, cx| {
                                    let _ = s.duplicate_layer(&t1);
                                    cx.notify();
                                });
                                p1.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Duplicate Layer"),
                    );

                    let s2 = s_menu.clone();
                    let p2 = p_close.clone();
                    let t2 = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s2.update(cx, |s, cx| {
                                    s.reset_layer_transform(&t2);
                                    cx.notify();
                                });
                                p2.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Reset Transform"),
                    );

                    let s3 = s_menu.clone();
                    let p3 = p_close.clone();
                    let t3 = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s3.update(cx, |s, cx| {
                                    s.add_keyframe_to_all_transforms_at_playhead(&t3);
                                    cx.notify();
                                });
                                p3.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Add Keyframe to Transforms at CTI"),
                    );

                    let s4 = s_menu.clone();
                    let p4 = p_close.clone();
                    let t4 = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s4.update(cx, |s, cx| {
                                    let _ = s.move_layer_up(&t4);
                                    cx.notify();
                                });
                                p4.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Move Up"),
                    );

                    let s5 = s_menu.clone();
                    let p5 = p_close.clone();
                    let t5 = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s5.update(cx, |s, cx| {
                                    let _ = s.move_layer_down(&t5);
                                    cx.notify();
                                });
                                p5.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Move Down"),
                    );

                    let s6 = s_menu.clone();
                    let p6 = p_close.clone();
                    let t6 = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s6.update(cx, |s, cx| {
                                    let _ = s.remove_layer_by_id(&t6);
                                    cx.notify();
                                });
                                p6.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Delete Layer"),
                    );
                }
                ContextMenuTarget::Effect { layer_id, effect_id } => {
                    let lid = layer_id.clone();
                    let eid = effect_id.clone();
                    let s1 = s_menu.clone();
                    let p1 = p_close.clone();
                    let l1 = lid.clone();
                    let e1 = eid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s1.update(cx, |s, cx| {
                                    let _ = s.duplicate_layer_effect(&l1, &e1);
                                    cx.notify();
                                });
                                p1.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Duplicate Effect"),
                    );

                    let s2 = s_menu.clone();
                    let p2 = p_close.clone();
                    let l2 = lid.clone();
                    let e2 = eid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s2.update(cx, |s, cx| {
                                    let _ = s.toggle_layer_effect_enabled(&l2, &e2);
                                    cx.notify();
                                });
                                p2.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Toggle Enabled"),
                    );

                    let s3 = s_menu.clone();
                    let p3 = p_close.clone();
                    let l3 = lid.clone();
                    let e3 = eid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s3.update(cx, |s, cx| {
                                    let _ = s.remove_layer_effect(&l3, &e3);
                                    cx.notify();
                                });
                                p3.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Delete Effect"),
                    );
                }
                ContextMenuTarget::Property { layer_id, prop_path } => {
                    let lid = layer_id.clone();
                    let path = *prop_path;
                    let s1 = s_menu.clone();
                    let p1 = p_close.clone();
                    let l1 = lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s1.update(cx, |s, cx| {
                                    s.toggle_layer_property_keyframe_at_playhead(&l1, path);
                                    cx.notify();
                                });
                                p1.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Add/Remove Keyframe at CTI"),
                    );

                    let s2 = s_menu.clone();
                    let p2 = p_close.clone();
                    let l2 = lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s2.update(cx, |s, cx| {
                                    s.toggle_layer_property_animation(&l2, path);
                                    cx.notify();
                                });
                                p2.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Toggle Stopwatch Animation"),
                    );
                }
            }

            let p_cancel = p_close.clone();
            menu_items = menu_items.child(
                div()
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|s| s.bg(cx.theme().muted))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_cancel.update(cx, |this, cx| {
                            this.close_context_menu();
                            cx.notify();
                        });
                    })
                    .child("Cancel"),
            );

            let context_menu_overlay = div()
                .id("timeline_context_menu")
                .test_support()
                .absolute()
                .top(px(40.))
                .left(px(120.))
                .w(px(220.))
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .rounded_md()
                .shadow_lg()
                .child(menu_items);
            root = root.child(context_menu_overlay);
        }

        root
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
