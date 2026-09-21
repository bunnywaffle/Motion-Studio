use gpui_kit::assets::IconName;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::base::{h_flex, v_flex, StyledExt, TestSupportExt};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use std::collections::HashSet;
use std::collections::HashMap;

use crate::state::{EditorState, EditorTool};
use project::shader::{presets as shader_presets, ShaderParamValue};
use project::{BlendMode, Color, EffectType, LayerSource, ShapeType, TimeCode, TrackMatteMode, Vec2};

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

fn menu_button<F>(label: &'static str, cx: &App, on_click: F) -> impl IntoElement
where
    F: Fn(&mut App) + 'static,
{
    div()
        .cursor_pointer()
        .px_2()
        .py_1()
        .rounded_sm()
        .text_xs()
        .hover(|st| st.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            on_click(cx);
        })
        .child(label)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectContextMenuTarget {
    BinBackground,
    Asset(String),
    Solid(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectFilterType {
    All,
    Compositions,
    Footage,
    Solids,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectSortMode {
    Name,
    Type,
}

impl ProjectSortMode {
    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Type => "Type",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Name => Self::Type,
            Self::Type => Self::Name,
        }
    }
}

pub struct ProjectPanel {
    focus_handle: FocusHandle,
    state: Entity<EditorState>,
    _subscription: Subscription,
    pub context_menu: Option<ProjectContextMenuTarget>,
    pub filter: ProjectFilterType,
    pub sort_mode: ProjectSortMode,
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
            context_menu: None,
            filter: ProjectFilterType::All,
            sort_mode: ProjectSortMode::Name,
        }
    }

    pub fn standalone(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new(state, cx)
    }

    pub fn open_context_menu(&mut self, target: ProjectContextMenuTarget) {
        self.context_menu = Some(target);
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
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
        let txt_state = self.state.clone();
        let adj_state = self.state.clone();
        let del_state = self.state.clone();
        let import_state = self.state.clone();

        let active_filter = self.filter;
        let solids_count = comp_opt
            .map(|c| c.layers.iter().filter(|l| matches!(&l.source, LayerSource::Solid { .. })).count())
            .unwrap_or(0);
        let assets_count = state.project.assets.len();
        let comps_count = if comp_opt.is_some() { 1 } else { 0 };
        let total_count = comps_count + assets_count + solids_count;

        let p_self = cx.entity().clone();
        let make_pill = |filter: ProjectFilterType, label: &'static str, count: usize| {
            let is_active = active_filter == filter;
            let p = p_self.clone();
            div()
                .cursor_pointer()
                .px_2()
                .py_0p5()
                .rounded_full()
                .text_xs()
                .font_medium()
                .flex()
                .items_center()
                .gap_1()
                .bg(if is_active { cx.theme().accent } else { cx.theme().muted })
                .text_color(if is_active { cx.theme().accent_foreground } else { cx.theme().muted_foreground })
                .hover(|s| s.opacity(0.85))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    p.update(cx, |this, cx| {
                        this.filter = filter;
                        cx.notify();
                    });
                })
                .child(label)
                .child(
                    div()
                        .px_1p5()
                        .rounded_full()
                        .text_xs()
                        .bg(if is_active { cx.theme().primary } else { cx.theme().secondary })
                        .text_color(if is_active { cx.theme().primary_foreground } else { cx.theme().foreground })
                        .child(format!("{count}")),
                )
        };

        let p_sort = p_self.clone();
        let sort_label = self.sort_mode.label();
        let filter_row = h_flex()
            .px_3()
            .py_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .gap_1p5()
            .items_center()
            .child(make_pill(ProjectFilterType::All, "All", total_count))
            .child(make_pill(ProjectFilterType::Compositions, "Comps", comps_count))
            .child(make_pill(ProjectFilterType::Footage, "Media", assets_count))
            .child(make_pill(ProjectFilterType::Solids, "Solids", solids_count))
            .child(
                div()
                    .cursor_pointer()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .text_xs()
                    .bg(cx.theme().secondary)
                    .text_color(cx.theme().muted_foreground)
                    .hover(|s| s.opacity(0.85))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_sort.update(cx, |this, cx| {
                            this.sort_mode = this.sort_mode.next();
                            cx.notify();
                        });
                    })
                    .child(format!("Sort: {sort_label}")),
            );

        let mut bin_items: Vec<AnyElement> = Vec::new();

        if let Some(comp) = comp_opt {
            // --- Section 1: Compositions ---
            if active_filter == ProjectFilterType::All || active_filter == ProjectFilterType::Compositions {
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
                        )
                        .into_any_element(),
                );

                // Active comp card
                bin_items.push(
                    h_flex()
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .bg(cx.theme().muted)
                        .text_xs()
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .w(px(24.))
                                        .h(px(24.))
                                        .rounded_sm()
                                        .bg(cx.theme().accent)
                                        .text_color(cx.theme().accent_foreground)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon_box(IconName::Film)),
                                )
                                .child(
                                    v_flex()
                                        .child(div().font_semibold().child(comp.name.clone()))
                                        .child(
                                            h_flex()
                                                .gap_1p5()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("{}x{}", comp.width, comp.height))
                                                .child("•")
                                                .child(format!("{:.0}fps", comp.frame_rate))
                                                .child("•")
                                                .child(format!("{}", comp.duration)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .bg(cx.theme().primary)
                                .text_color(cx.theme().primary_foreground)
                                .font_medium()
                                .child("Active"),
                        )
                        .into_any_element(),
                );
            }

            // --- Section 2: Project Media & Bins ---
            if active_filter == ProjectFilterType::All || active_filter == ProjectFilterType::Footage {
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
                        )
                        .into_any_element(),
                );

                if state.project.assets.is_empty() {
                    bin_items.push(
                        div()
                            .px_3()
                            .py_3()
                            .border_1()
                            .border_dashed()
                            .border_color(cx.theme().border)
                            .rounded_md()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(div().child(icon_box(IconName::FolderOpen)))
                            .child(div().child("No external media imported yet."))
                            .child(div().text_color(cx.theme().muted_foreground).child("Click 'Import Media' or '+ Sample' to add assets."))
                            .into_any_element(),
                    );
                } else {
                    let mut ordered: Vec<&project::Asset> =
                        state.project.assets.iter().collect();
                    match self.sort_mode {
                        ProjectSortMode::Name => ordered.sort_by(|a, b| {
                            a.name.to_lowercase().cmp(&b.name.to_lowercase())
                        }),
                        ProjectSortMode::Type => ordered.sort_by(|a, b| {
                            format!("{:?}", a.asset_type)
                                .cmp(&format!("{:?}", b.asset_type))
                                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                        }),
                    }
                    for asset in ordered {
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
                        let asset_id_add = asset.id.clone();
                        let s_del = self.state.clone();
                        let s_add = self.state.clone();
                        let p_asset_rclick = cx.entity().clone();
                        let aid_rclick = asset.id.clone();

                        bin_items.push(
                            h_flex()
                                .id(SharedString::from(format!("project_asset_item_{}", asset.id)))
                                .test_support()
                                .px_2()
                                .py_1p5()
                                .rounded_md()
                                .text_xs()
                                .items_center()
                                .justify_between()
                                .bg(cx.theme().secondary)
                                .border_1()
                                .border_color(cx.theme().border)
                                .text_color(cx.theme().foreground)
                                .hover(|s| s.bg(cx.theme().muted))
                                .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                    p_asset_rclick.update(cx, |this, cx| {
                                        this.open_context_menu(ProjectContextMenuTarget::Asset(aid_rclick.clone()));
                                        cx.notify();
                                    });
                                })
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .child({
                                            let thumb_path =
                                                match &asset.asset_type {
                                                    project::AssetType::Image => {
                                                        Some(asset.path.clone())
                                                    }
                                                    _ => None,
                                                };
                                            match thumb_path {
                                                Some(p) => gpui::img(p)
                                                    .w(px(28.))
                                                    .h(px(28.))
                                                    .rounded_sm()
                                                    .into_any_element(),
                                                None => div()
                                                    .w(px(28.))
                                                    .h(px(28.))
                                                    .rounded_sm()
                                                    .bg(cx.theme().muted)
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon_box(icon))
                                                    .into_any_element(),
                                            }
                                        })
                                        .child(
                                            v_flex()
                                                .child(div().font_semibold().child(display_name))
                                                .child(div().text_color(cx.theme().muted_foreground).child(type_str)),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .items_center()
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(cx.theme().primary)
                                                .text_color(cx.theme().primary_foreground)
                                                .hover(|s| s.opacity(0.85))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    let aid = asset_id_add.clone();
                                                    s_add.update(cx, |s, cx| {
                                                        let _ = s.add_asset_layer(&aid);
                                                        cx.notify();
                                                    });
                                                })
                                                .child("+ Comp"),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .p_1()
                                                .rounded_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    s_del.update(cx, |s, cx| {
                                                        let _ = s.delete_asset(&asset_id_del);
                                                        cx.notify();
                                                    });
                                                })
                                                .child(icon_box(IconName::Trash)),
                                        ),
                                )
                                .into_any_element(),
                        );
                    }
                }
            }

            // --- Section 3: Project Solids & Footage Bin ---
            if active_filter == ProjectFilterType::All || active_filter == ProjectFilterType::Solids {
                let mut solids: Vec<_> = comp
                    .layers
                    .iter()
                    .filter(|l| matches!(&l.source, LayerSource::Solid { .. }))
                    .collect();
                if self.sort_mode == ProjectSortMode::Name {
                    solids.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                }

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
                        )
                        .into_any_element(),
                );

                if solids.is_empty() {
                    bin_items.push(
                        div()
                            .px_2()
                            .py_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("No solid footage items. Click '+ Solid' to create one.")
                            .into_any_element(),
                    );
                } else {
                    for solid in solids {
                        let (w, h, col) = match &solid.source {
                            LayerSource::Solid { width, height, color } => (*width, *height, *color),
                            _ => (1920, 1080, Color::WHITE),
                        };

                        let p_solid_rclick = cx.entity().clone();
                        let sid_rclick = solid.id.clone();
                        let row = h_flex()
                            .id(SharedString::from(format!("project_solid_item_{}", solid.id)))
                            .test_support()
                            .px_2()
                            .py_1p5()
                            .rounded_md()
                            .text_xs()
                            .items_center()
                            .justify_between()
                            .bg(cx.theme().secondary)
                            .border_1()
                            .border_color(cx.theme().border)
                            .text_color(cx.theme().foreground)
                            .hover(|s| s.bg(cx.theme().muted))
                            .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                p_solid_rclick.update(cx, |this, cx| {
                                    this.open_context_menu(ProjectContextMenuTarget::Solid(sid_rclick.clone()));
                                    cx.notify();
                                });
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .w(px(20.))
                                            .h(px(20.))
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .bg(Rgba { r: col.r, g: col.g, b: col.b, a: col.a }),
                                    )
                                    .child(
                                        v_flex()
                                            .child(div().font_semibold().child(format!("{} (Footage)", solid.name)))
                                            .child(div().text_color(cx.theme().muted_foreground).child(format!("{}x{}", w, h))),
                                    ),
                            );
                        bin_items.push(row.into_any_element());
                    }
                }
            }
        }

        let sample_state = self.state.clone();

            let p_bin_rclick = cx.entity().clone();
            let mut root = div()
                .id("project_panel")
                .test_support()
                .track_focus(&self.focus_handle)
                .size_full()
                .flex()
                .flex_col()
                .relative()
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
                                        .id("add_text_button")
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
                                            txt_state.update(cx, |s, cx| {
                                                let _ = s.add_text_layer("New Text", None);
                                                cx.notify();
                                            });
                                        })
                                        .child(icon_box(IconName::Type))
                                        .child("Text"),
                                )
                                .child(
                                    div()
                                        .id("add_adjustment_button")
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
                                            adj_state.update(cx, |s, cx| {
                                                let _ = s.add_adjustment_layer(None);
                                                cx.notify();
                                            });
                                        })
                                        .child(icon_box(IconName::SlidersHorizontal))
                                        .child("Adj Layer"),
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
                // Filter bar
                .child(filter_row)
                // Organized Bins and Assets List
                .child(
                    v_flex()
                        .id("project_assets")
                        .test_support()
                        .flex_1()
                        .overflow_y_scroll()
                        .px_2()
                        .py_1()
                        .gap_1()
                        .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                            p_bin_rclick.update(cx, |this, cx| {
                                this.open_context_menu(ProjectContextMenuTarget::BinBackground);
                                cx.notify();
                            });
                        })
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
                );

            if let Some(ref target) = self.context_menu {
                let p_close = cx.entity().clone();
                let s_menu = self.state.clone();
                let mut menu_items = v_flex().gap_0p5().p_1();

                match target {
                    ProjectContextMenuTarget::BinBackground => {
                        let s_new = s_menu.clone();
                        let p_new = p_close.clone();
                        menu_items = menu_items
                            .child(
                                div()
                                    .id("proj_ctx_new_solid")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_xs()
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_new.update(cx, |s, cx| {
                                            let _ = s.add_solid_layer("New Solid", Color::from_rgba_u8(245, 158, 11, 255), 400, 400);
                                            cx.notify();
                                        });
                                        p_new.update(cx, |this, cx| {
                                            this.close_context_menu();
                                            cx.notify();
                                        });
                                    })
                                    .child("New Solid Layer"),
                            )
                            .child({
                                let s_txt = s_menu.clone();
                                let p_txt = p_close.clone();
                                div()
                                    .id("proj_ctx_new_text")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_xs()
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_txt.update(cx, |s, cx| {
                                            let _ = s.add_text_layer("New Text Layer", None);
                                            cx.notify();
                                        });
                                        p_txt.update(cx, |this, cx| {
                                            this.close_context_menu();
                                            cx.notify();
                                        });
                                    })
                                    .child("New Text Layer")
                            })
                            .child({
                                let s_adj = s_menu.clone();
                                let p_adj = p_close.clone();
                                div()
                                    .id("proj_ctx_new_adjustment")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_xs()
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_adj.update(cx, |s, cx| {
                                            let _ = s.add_adjustment_layer(None);
                                            cx.notify();
                                        });
                                        p_adj.update(cx, |this, cx| {
                                            this.close_context_menu();
                                            cx.notify();
                                        });
                                    })
                                    .child("New Adjustment Layer")
                            })
                            .child({
                                let s_smp = s_menu.clone();
                                let p_smp = p_close.clone();
                                div()
                                    .id("proj_ctx_import_sample")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_xs()
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_smp.update(cx, |s, cx| {
                                            let _ = s.import_sample_image();
                                            cx.notify();
                                        });
                                        p_smp.update(cx, |this, cx| {
                                            this.close_context_menu();
                                            cx.notify();
                                        });
                                    })
                                    .child("Import Sample Footage")
                            });
                    }
                    ProjectContextMenuTarget::Asset(asset_id) => {
                        let aid = asset_id.clone();
                        let s_add = s_menu.clone();
                        let p_add = p_close.clone();
                        let aid2 = asset_id.clone();
                        let s_del = s_menu.clone();
                        let p_del = p_close.clone();
                        menu_items = menu_items
                            .child(
                                div()
                                    .id("proj_ctx_add_asset")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_xs()
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_add.update(cx, |s, cx| {
                                            let _ = s.add_asset_layer(&aid);
                                            cx.notify();
                                        });
                                        p_add.update(cx, |this, cx| {
                                            this.close_context_menu();
                                            cx.notify();
                                        });
                                    })
                                    .child("Add to Composition"),
                            )
                            .child(
                                div()
                                    .id("proj_ctx_del_asset")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_xs()
                                    .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_del.update(cx, |s, cx| {
                                            let _ = s.delete_asset(&aid2);
                                            cx.notify();
                                        });
                                        p_del.update(cx, |this, cx| {
                                            this.close_context_menu();
                                            cx.notify();
                                        });
                                    })
                                    .child("Delete Asset"),
                            );
                    }
                    ProjectContextMenuTarget::Solid(solid_id) => {
                        let sid = solid_id.clone();
                        let s_del = s_menu.clone();
                        let p_del = p_close.clone();
                        menu_items = menu_items
                            .child(
                                div()
                                    .id("proj_ctx_del_solid")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_xs()
                                    .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        s_del.update(cx, |s, cx| {
                                            let _ = s.remove_layer_by_id(&sid);
                                            cx.notify();
                                        });
                                        p_del.update(cx, |this, cx| {
                                            this.close_context_menu();
                                            cx.notify();
                                        });
                                    })
                                    .child("Delete Solid"),
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

                let overlay = div()
                    .id("project_context_menu")
                    .test_support()
                    .absolute()
                    .top(px(40.))
                    .left(px(20.))
                    .w(px(180.))
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .shadow_lg()
                    .child(menu_items);

                root = root.child(overlay);
            }

            root
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewerContextMenuTarget {
    Canvas,
    Layer(String),
}

pub struct CompositionViewerPanel {
    focus_handle: FocusHandle,
    state: Entity<EditorState>,
    _subscription: Subscription,
    pub context_menu: Option<ViewerContextMenuTarget>,
    pub is_dragging_canvas: bool,
    pub last_canvas_mouse: Option<(f32, f32)>,
    /// Remembered rectangle/ellipse variant for the grouped Shape tool.
    pub shape_variant: EditorTool,
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
            is_dragging_canvas: false,
            last_canvas_mouse: None,
            shape_variant: EditorTool::ShapeRect,
        }
    }

    pub fn open_context_menu(&mut self, target: ViewerContextMenuTarget) {
        self.context_menu = Some(target);
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
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
                let mut full_frame_backdrop = Color::rgba(
                    bg_color.r, bg_color.g, bg_color.b, bg_color.a,
                );
                let mut rendered_regions: Vec<(f32, f32, f32, f32, Color)> = Vec::new();

                for layer in stack.render_layers() {
                    let is_text = matches!(&layer.source, LayerSource::Text { .. });
                    let is_adjustment = matches!(&layer.source, LayerSource::Adjustment);

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
                        LayerSource::Text { font_size, fill_color, text, .. } => {
                            let len = text.value.chars().count().max(1) as f32;
                            let fs = font_size.value;
                            let estimated_w = (len * fs * 0.6 + 40.0).max(100.0);
                            let estimated_h = (fs * 1.4 + 20.0).max(40.0);
                            (estimated_w, estimated_h, fill_color.value, None)
                        }
                        LayerSource::Adjustment => {
                            (comp_w, comp_h, Color::TRANSPARENT, None)
                        }
                        _ => (400.0, 300.0, Color::WHITE, None),
                    };

                    let bbox = layer.world_bounds(base_w, base_h);
                    let l_x = (bbox.min.x + comp_w / 2.0) * scale_x;
                    let l_y = (bbox.min.y + comp_h / 2.0) * scale_y;
                    let l_w = ((bbox.max.x - bbox.min.x) * scale_x).max(2.0);
                    let l_h = ((bbox.max.y - bbox.min.y) * scale_y).max(2.0);
                    let is_selected = state.selected_layer_id.as_deref() == Some(&layer.id);

                    // Sample backdrop for this layer from the composition background
                    // and all intersecting underlying layers rendered so far.
                    let mut sampled_backdrop = full_frame_backdrop;
                    for (rx1, ry1, rx2, ry2, rcol) in &rendered_regions {
                        if bbox.min.x < *rx2 && bbox.max.x > *rx1 && bbox.min.y < *ry2 && bbox.max.y > *ry1 {
                            sampled_backdrop = BlendMode::Normal.composite(sampled_backdrop, *rcol);
                        }
                    }

                    // Process visual effects. After Effects adjustment semantics:
                    // every active adjustment layer stacked *above* this layer
                    // folds its effects into the composite below it, so apply
                    // those here (spatial blur accumulates into `blur_rad`).
                    let mut processed_col = if is_adjustment {
                        layer.processed_color(sampled_backdrop)
                    } else {
                        layer.processed_color(col)
                    };
                    let mut adjustment_blur = 0.0f32;
                    if !is_adjustment {
                        for adj_fx in stack.adjustment_effects_applying_to(&layer.id) {
                            if let Some(r) = adj_fx.effect_type.blur_radius() {
                                adjustment_blur += r;
                            } else {
                                processed_col = adj_fx.process_color(processed_col);
                            }
                        }
                    }

                    let eff_opacity = layer.effective_opacity.clamp(0.0, 1.0);
                    let source_color = Color::rgba(
                        processed_col.r,
                        processed_col.g,
                        processed_col.b,
                        processed_col.a * eff_opacity,
                    );

                    // Determine canvas rendering color avoiding double-blend:
                    // For Normal mode: straight alpha rendering with GPU rasterizer.
                    // For non-Normal modes (Multiply, Screen, Add, Overlay, etc.):
                    // composite source_color mathematically over sampled_backdrop.
                    let (canvas_bg, recorded_color) = if is_text {
                        (
                            Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
                            BlendMode::Normal.composite(sampled_backdrop, source_color),
                        )
                    } else if is_adjustment {
                        let blended = layer.blend_mode.composite(sampled_backdrop, processed_col);
                        (
                            Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 },
                            blended,
                        )
                    } else if layer.blend_mode == BlendMode::Normal {
                        (
                            Rgba { r: processed_col.r, g: processed_col.g, b: processed_col.b, a: source_color.a },
                            BlendMode::Normal.composite(sampled_backdrop, source_color),
                        )
                    } else {
                        let blended = layer.blend_mode.composite(sampled_backdrop, source_color);
                        (
                            Rgba { r: blended.r, g: blended.g, b: blended.b, a: (blended.a * eff_opacity).clamp(0.0, 1.0) },
                            blended,
                        )
                    };

                    rendered_regions.push((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y, recorded_color));

                    let covers_canvas = bbox.min.x <= -comp_w / 2.0
                        && bbox.min.y <= -comp_h / 2.0
                        && bbox.max.x >= comp_w / 2.0
                        && bbox.max.y >= comp_h / 2.0;
                    if covers_canvas {
                        full_frame_backdrop = recorded_color;
                    }

                    // Render Drop Shadow if present
                    for eff in &layer.effects {
                        if eff.enabled {
                            if let compositor::EvaluatedEffectType::DropShadow { distance, angle, opacity, color, .. } = &eff.effect_type {
                                let rad = angle.to_radians();
                                let sx = l_x + distance * rad.cos() * scale_x;
                                let sy = l_y + distance * rad.sin() * scale_y;
                                let op = (opacity / 100.0).clamp(0.0, 1.0) * eff_opacity;
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

                    // Render Gaussian Blur diffusion when present (own effects plus
                    // any adjustment-layer blur folded in above). This is a
                    // multi-tap preview approximation — true per-pixel diffusion
                    // runs in `renderer::blur` (CPU) / `BLUR_WGSL` (GPU).
                    // Adjustment layers draw no pixels themselves (outline only).
                    let mut blur_rad = adjustment_blur;
                    for eff in &layer.effects {
                        if eff.enabled {
                            if let compositor::EvaluatedEffectType::GaussianBlur { radius } = &eff.effect_type {
                                blur_rad += *radius;
                            }
                        }
                    }
                    if blur_rad > 0.0 && !is_adjustment {
                        // Compass taps soften edges like a real blur kernel.
                        let tap_dist = (blur_rad * 0.35).clamp(1.5, 14.0);
                        let tap_alpha = (processed_col.a * eff_opacity * 0.10).clamp(0.01, 0.25);
                        let taps = [
                            (-tap_dist, 0.0),
                            (tap_dist, 0.0),
                            (0.0, -tap_dist),
                            (0.0, tap_dist),
                            (-tap_dist, -tap_dist),
                            (tap_dist, -tap_dist),
                            (-tap_dist, tap_dist),
                            (tap_dist, tap_dist),
                        ];
                        for (ox, oy) in taps {
                            elements.push(
                                div()
                                    .absolute()
                                    .left(px(l_x + ox))
                                    .top(px(l_y + oy))
                                    .w(px(l_w))
                                    .h(px(l_h))
                                    .bg(Rgba {
                                        r: processed_col.r,
                                        g: processed_col.g,
                                        b: processed_col.b,
                                        a: tap_alpha,
                                    })
                                    .into_any_element(),
                            );
                        }
                        let steps = 4;
                        let max_expand = (blur_rad * 0.4).clamp(4.0, 28.0);
                        for i in 1..=steps {
                            let factor = i as f32 / steps as f32;
                            let exp = max_expand * factor;
                            let weight = (-2.0 * factor * factor).exp();
                            let step_alpha = (processed_col.a * eff_opacity * 0.12 * weight).clamp(0.01, 0.4);
                            elements.push(
                                div()
                                    .absolute()
                                    .left(px(l_x - exp))
                                    .top(px(l_y - exp))
                                    .w(px(l_w + exp * 2.0))
                                    .h(px(l_h + exp * 2.0))
                                    .rounded_lg()
                                    .bg(Rgba {
                                        r: processed_col.r,
                                        g: processed_col.g,
                                        b: processed_col.b,
                                        a: step_alpha,
                                    })
                                    .into_any_element(),
                            );
                        }
                    }

                    let p_drag_layer = cx.entity().clone();
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
                        .bg(canvas_bg)
                        .overflow_hidden()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                            let curr_x = event.position.x / px(1.0);
                            let curr_y = event.position.y / px(1.0);
                            p_drag_layer.update(cx, |this, _cx| {
                                this.is_dragging_canvas = true;
                                this.last_canvas_mouse = Some((curr_x, curr_y));
                            });
                            sel_state.update(cx, |s, cx| {
                                s.select_layer(Some(lid.clone()));
                                cx.notify();
                            });
                        })
                        .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                            p_menu.update(cx, |this, cx| {
                                this.open_context_menu(ViewerContextMenuTarget::Layer(lid_menu.clone()));
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
                                .opacity(eff_opacity),
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
                    } else if let LayerSource::Text { text, font_family, font_size, .. } = &layer.source {
                        let text_val = text.value.clone();
                        let fs_scaled = (font_size.value * scale_y).max(8.0);
                        // Resolve against installed system fonts so text never
                        // breaks on machines missing the stored family.
                        let ff = crate::state::resolve_font_family(font_family);
                        let text_color = Rgba {
                            r: processed_col.r,
                            g: processed_col.g,
                            b: processed_col.b,
                            a: source_color.a,
                        };
                        layer_el = layer_el
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(fs_scaled))
                                    .text_color(text_color)
                                    .font_family(SharedString::from(ff))
                                    .child(text_val),
                            );
                    } else if is_adjustment {
                        // After Effects behavior: an adjustment layer draws no
                        // pixels of its own — its effects were already folded
                        // into every layer beneath it above. Render only the
                        // dashed extent outline plus a status label.
                        let adj_fx_count = layer.effects.iter().filter(|e| e.enabled).count();
                        let adj_label = if adj_fx_count == 0 {
                            format!("Adj: {} (no FX — affects below)", layer.name)
                        } else {
                            format!(
                                "Adj: {} ({} FX → {} layer{} below)",
                                layer.name,
                                adj_fx_count,
                                stack
                                    .render_list
                                    .iter()
                                    .position(|id| id == &layer.id)
                                    .unwrap_or(0),
                                if stack
                                    .render_list
                                    .iter()
                                    .position(|id| id == &layer.id)
                                    .unwrap_or(0)
                                    == 1
                                {
                                    ""
                                } else {
                                    "s"
                                },
                            )
                        };
                        layer_el = layer_el.child(
                            div()
                                .size_full()
                                .border_1()
                                .border_dashed()
                                .border_color(if is_selected { rgb(0x3b82f6) } else { Rgba { r: 0.8, g: 0.8, b: 0.8, a: 0.15 } })
                                .flex()
                                .items_end()
                                .justify_start()
                                .p_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(Rgba { r: 0.1, g: 0.1, b: 0.1, a: 0.6 })
                                        .text_color(if is_selected { rgb(0x93c5fd) } else { Rgba { r: 1.0, g: 1.0, b: 1.0, a: 0.4 } })
                                        .child(adj_label),
                                ),
                        );
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
        let active_tool = state.active_tool;
        let s_side = self.state.clone();
        let tool_btn = |tool: EditorTool, icon: IconName, label: &'static str, cx: &App| {
            let is_active = active_tool == tool;
            let s_click = s_side.clone();
            div()
                .id(SharedString::from(format!("side_tool_btn_{label}")))
                .test_support()
                .cursor_pointer()
                .w(px(28.))
                .h(px(28.))
                .rounded_sm()
                .flex()
                .items_center()
                .justify_center()
                .bg(if is_active { cx.theme().primary } else { cx.theme().muted })
                .text_color(if is_active { cx.theme().primary_foreground } else { cx.theme().foreground })
                .hover(|s| if !is_active { s.bg(cx.theme().accent) } else { s })
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    s_click.update(cx, |s, cx| {
                        s.set_tool(tool);
                        cx.notify();
                    });
                })
                .child(icon_box(icon))
        };

        let s_add = self.state.clone();
        let panel_entity = cx.entity().clone();
        // Grouped Shape tool: shows the active variant (rectangle/ellipse).
        // Click selects it; clicking again toggles the variant (same as `Q`).
        let shown_shape = match active_tool {
            EditorTool::ShapeRect | EditorTool::ShapeEllipse => active_tool,
            _ => self.shape_variant,
        };
        let (shape_icon, shape_label) = match shown_shape {
            EditorTool::ShapeEllipse => (IconName::Circle, "ellipse"),
            _ => (IconName::Square, "rect"),
        };
        let shape_is_active = matches!(active_tool, EditorTool::ShapeRect | EditorTool::ShapeEllipse);

        let side_toolbar = v_flex()
            .w(px(36.))
            .h_full()
            .py_2()
            .gap_1p5()
            .items_center()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(tool_btn(EditorTool::Move, IconName::Move, "move", cx))
            .child(tool_btn(EditorTool::Hand, IconName::Hand, "hand", cx))
            .child(tool_btn(EditorTool::Rotate, IconName::RotateCw, "rotate", cx))
            .child(tool_btn(EditorTool::Pen, IconName::Pen, "pen", cx))
            .child(tool_btn(EditorTool::Text, IconName::Type, "text", cx))
            .child({
                let p_shape = panel_entity.clone();
                div()
                    .id(SharedString::from(format!("side_tool_btn_{shape_label}")))
                    .test_support()
                    .cursor_pointer()
                    .w(px(28.))
                    .h(px(28.))
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(if shape_is_active { cx.theme().primary } else { cx.theme().muted })
                    .text_color(if shape_is_active { cx.theme().primary_foreground } else { cx.theme().foreground })
                    .hover(|s| if !shape_is_active { s.bg(cx.theme().accent) } else { s })
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_shape.update(cx, |this, cx| {
                            // Clicking the grouped tool selects it; clicking
                            // again toggles rectangle/ellipse (same as `Q`).
                            let next = match this.state.read(cx).active_tool {
                                EditorTool::ShapeRect => EditorTool::ShapeEllipse,
                                EditorTool::ShapeEllipse => EditorTool::ShapeRect,
                                _ => this.shape_variant,
                            };
                            this.shape_variant = next;
                            this.state.update(cx, |s, cx| {
                                s.set_tool(next);
                                cx.notify();
                            });
                            cx.notify();
                        });
                    })
                    .child(icon_box(shape_icon))
            })
            .child(div().w(px(20.)).h(px(1.)).bg(cx.theme().border).my_1())
            // Single contextual action: creates a layer of the active tool
            // type at the viewport center (click the canvas to place freely).
            .child(
                div()
                    .id("quick_add_center_button")
                    .test_support()
                    .cursor_pointer()
                    .w(px(28.))
                    .h(px(28.))
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(cx.theme().muted)
                    .hover(|s| s.bg(cx.theme().accent))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        s_add.update(cx, |s, cx| {
                            match s.active_tool {
                                EditorTool::Text => {
                                    let _ = s.add_text_layer("New Text", None);
                                }
                                EditorTool::ShapeRect => {
                                    let _ = s.add_rectangle_shape_layer(400.0, 300.0, None);
                                }
                                EditorTool::ShapeEllipse => {
                                    let _ = s.add_ellipse_shape_layer(150.0, 150.0, None);
                                }
                                EditorTool::Pen => {
                                    let _ = s.add_pen_point(project::Vec2::ZERO);
                                }
                                _ => {
                                    let _ = s.add_solid_layer(
                                        "New Solid",
                                        project::Color::from_rgba_u8(59, 130, 246, 255),
                                        400,
                                        400,
                                    );
                                }
                            }
                            cx.notify();
                        });
                    })
                    .child(icon_box(IconName::Plus))
            );

        div()
            .id("composition_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if !event.dragging() {
                    if this.is_dragging_canvas {
                        this.is_dragging_canvas = false;
                        this.last_canvas_mouse = None;
                    }
                    return;
                }
                if let (true, Some((last_x, last_y))) = (this.is_dragging_canvas, this.last_canvas_mouse) {
                    let curr_x = event.position.x / px(1.0);
                    let curr_y = event.position.y / px(1.0);
                    let dx = curr_x - last_x;
                    let dy = curr_y - last_y;
                    let active_tool = this.state.read(cx).active_tool;
                    if active_tool == EditorTool::Move {
                        let scale_factor_x = 1920.0 / 512.0;
                        let scale_factor_y = 1080.0 / 288.0;
                        let s = this.state.clone();
                        s.update(cx, |s, cx| {
                            s.nudge_position(dx * scale_factor_x, dy * scale_factor_y);
                            cx.notify();
                        });
                        this.last_canvas_mouse = Some((curr_x, curr_y));
                    } else if active_tool == EditorTool::Rotate {
                        let s = this.state.clone();
                        s.update(cx, |s, cx| {
                            s.nudge_rotation(dx * 0.5);
                            cx.notify();
                        });
                        this.last_canvas_mouse = Some((curr_x, curr_y));
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.is_dragging_canvas = false;
                this.last_canvas_mouse = None;
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.is_dragging_canvas = false;
                this.last_canvas_mouse = None;
            }))
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
            // Composition Canvas area with vertical side toolbar
            .child(
                h_flex()
                    .id("composition_viewer")
                    .test_support()
                    .flex_1()
                    .size_full()
                    .child(side_toolbar)
                    .child(
                        v_flex()
                            .flex_1()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .p_4()
                            .overflow_hidden()
                            .child({
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
                                    .on_mouse_down(MouseButton::Left, {
                                        let p_drag = cx.entity().clone();
                                        let s_tool = self.state.clone();
                                        move |event, _window, cx| {
                                            let curr_x = event.position.x / px(1.0);
                                            let curr_y = event.position.y / px(1.0);
                                            p_drag.update(cx, |this, _cx| {
                                                this.is_dragging_canvas = true;
                                                this.last_canvas_mouse = Some((curr_x, curr_y));
                                            });
                                            let active_tool = s_tool.read(cx).active_tool;
                                            let comp_x = (curr_x - canvas_w / 2.0) * (comp_w / canvas_w);
                                            let comp_y = (curr_y - canvas_h / 2.0) * (comp_h / canvas_h);
                                            match active_tool {
                                                EditorTool::Text => {
                                                    s_tool.update(cx, |s, cx| {
                                                        let _ = s.add_text_layer("New Text Layer", Some(Vec2::new(comp_x, comp_y)));
                                                        cx.notify();
                                                    });
                                                }
                                                EditorTool::ShapeRect => {
                                                    s_tool.update(cx, |s, cx| {
                                                        let _ = s.add_rectangle_shape_layer(300.0, 200.0, Some(Vec2::new(comp_x, comp_y)));
                                                        cx.notify();
                                                    });
                                                }
                                                EditorTool::ShapeEllipse => {
                                                    s_tool.update(cx, |s, cx| {
                                                        let _ = s.add_ellipse_shape_layer(150.0, 150.0, Some(Vec2::new(comp_x, comp_y)));
                                                        cx.notify();
                                                    });
                                                }
                                                EditorTool::Pen => {
                                                    s_tool.update(cx, |s, cx| {
                                                        let _ = s.add_pen_point(Vec2::new(comp_x, comp_y));
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
                                        }
                                    })
                                    .children(rendered_layers);

                                let p_canvas_rclick = cx.entity().clone();
                                canvas_frame = canvas_frame.on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                    p_canvas_rclick.update(cx, |this, cx| {
                                        this.open_context_menu(ViewerContextMenuTarget::Canvas);
                                        cx.notify();
                                    });
                                });

                                if let Some(ref target) = self.context_menu {
                                    let s_menu = self.state.clone();
                                    let p_close = cx.entity().clone();

                                    let mut menu_items = v_flex().gap_0p5();

                                    match target {
                                        ViewerContextMenuTarget::Canvas => {
                                            menu_items = menu_items
                                                .child(
                                                    div()
                                                        .font_bold()
                                                        .text_xs()
                                                        .px_2()
                                                        .py_1()
                                                        .border_b_1()
                                                        .border_color(cx.theme().border)
                                                        .child("Composition Canvas"),
                                                )
                                                .child(menu_button("New Text Layer", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.add_text_layer("New Text Layer", None); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }))
                                                .child(menu_button("New Rectangle Shape", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.add_rectangle_shape_layer(300.0, 200.0, None); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }))
                                                .child(menu_button("New Ellipse Shape", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.add_ellipse_shape_layer(150.0, 150.0, None); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }))
                                                .child(menu_button("New Solid Layer", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.add_solid_layer("New Solid", Color::from_rgba_u8(245, 158, 11, 255), 400, 400); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }))
                                                .child(menu_button("New Adjustment Layer", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.add_adjustment_layer(None); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }));
                                        }
                                        ViewerContextMenuTarget::Layer(lid) => {
                                            let lid_del = lid.clone();
                                            let lid_dup = lid.clone();
                                            let lid_rst = lid.clone();

                                            menu_items = menu_items
                                                .child(
                                                    div()
                                                        .font_bold()
                                                        .text_xs()
                                                        .px_2()
                                                        .py_1()
                                                        .border_b_1()
                                                        .border_color(cx.theme().border)
                                                        .child(format!("Layer: {lid}")),
                                                )
                                                .child(menu_button("Reset Transform", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { s.reset_layer_transform(&lid_rst); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }))
                                                .child(menu_button("Duplicate Layer", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.duplicate_layer(&lid_dup); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }))
                                                .child(menu_button("Bring Forward", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.move_selected_layer_up(); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }))
                                                .child(menu_button("Send Backward", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.move_selected_layer_down(); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }))
                                                .child(menu_button("Delete Layer", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { let _ = s.remove_layer_by_id(&lid_del); cx.notify(); });
                                                        p.update(cx, |this, cx| { this.close_context_menu(); cx.notify(); });
                                                    }
                                                }));
                                        }
                                    }

                                    canvas_frame = canvas_frame.child(
                                        div()
                                            .id("viewer_context_menu")
                                            .test_support()
                                            .absolute()
                                            .top(px(10.))
                                            .left(px(10.))
                                            .w(px(180.))
                                            .p_1()
                                            .bg(cx.theme().popover)
                                            .text_color(cx.theme().popover_foreground)
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .rounded_md()
                                            .shadow_lg()
                                            .child(menu_items),
                                    );
                                }

                                canvas_frame
                            })
                    )
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
    /// True once the current scrub drag has moved (distinguishes scrub-drag
    /// from click-to-type on a value field).
    pub scrub_moved: bool,
    /// Collapsible section states (true = expanded, false = collapsed)
    pub source_expanded: bool,
    pub transform_expanded: bool,
    pub switches_expanded: bool,
    pub effects_expanded: bool,
    /// Effect ID with the Shader Lab source editor open (`None` = closed).
    pub shader_editor_open: Option<String>,
    /// Live Shader Lab source editor (single open editor; re-created when
    /// the opened effect or its source hash changes so Apply refreshes it).
    pub shader_editor: Option<Entity<TextareaState>>,
    pub shader_editor_key: Option<(String, u64)>,
}

struct TextInspectorInputs {
    text: Entity<InputState>,
    font_family: Entity<InputState>,
    font_size: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

struct InspectorColorPicker {
    state: Entity<ColorPickerState>,
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
            scrub_prop: None,
            scrub_last_x: None,
            scrub_moved: false,
            source_expanded: true,
            transform_expanded: true,
            switches_expanded: false,
            effects_expanded: true,
            shader_editor_open: None,
            shader_editor: None,
            shader_editor_key: None,
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
                "scale_u" => s.nudge_scale(dx * 0.5, 0.0),
                "rotation" => s.nudge_rotation(dx * 0.5),
                "opacity" => s.nudge_opacity(dx * 0.5),
                "solid_w" => {
                    if let Some(l) = s.selected_layer_mut() {
                        if let LayerSource::Solid { width, .. } = &mut l.source {
                            *width = (*width as f32 + dx * 2.0).max(1.0) as u32;
                        }
                    }
                }
                "solid_h" => {
                    if let Some(l) = s.selected_layer_mut() {
                        if let LayerSource::Solid { height, .. } = &mut l.source {
                            *height = (*height as f32 + dx * 2.0).max(1.0) as u32;
                        }
                    }
                }
                "font_size" => {
                    if let Some(lid) = s.selected_layer_id.clone() {
                        let _ = s.nudge_layer_font_size(&lid, dx * 0.5);
                    }
                }
                "rect_w" => {
                    if let Some(lid) = s.selected_layer_id.clone() {
                        let _ = s.nudge_layer_rect_dimensions(&lid, dx * 2.0, 0.0, 0.0);
                    }
                }
                "rect_h" => {
                    if let Some(lid) = s.selected_layer_id.clone() {
                        let _ = s.nudge_layer_rect_dimensions(&lid, 0.0, dx * 2.0, 0.0);
                    }
                }
                "rect_cr" => {
                    if let Some(lid) = s.selected_layer_id.clone() {
                        let _ = s.nudge_layer_rect_dimensions(&lid, 0.0, 0.0, dx * 0.5);
                    }
                }
                "ellipse_rx" => {
                    if let Some(lid) = s.selected_layer_id.clone() {
                        let _ = s.nudge_layer_ellipse_radii(&lid, dx * 1.0, 0.0);
                    }
                }
                "ellipse_ry" => {
                    if let Some(lid) = s.selected_layer_id.clone() {
                        let _ = s.nudge_layer_ellipse_radii(&lid, 0.0, dx * 1.0);
                    }
                }
                other => {
                    if let Some(rest) = other.strip_prefix("slc:") {
                        // Shader Lab vector/color component: slc:<eff>:<name>:<idx>
                        let parts: Vec<&str> = rest.split(':').collect();
                        if parts.len() >= 3 {
                            if let (Some(eff), Some(name), Ok(idx)) =
                                (parts.first(), parts.get(1), parts.get(2).unwrap_or(&"0").parse::<usize>())
                            {
                                let _ = s.nudge_shaderlab_component(eff, name, idx, dx * 0.25);
                            }
                        }
                    } else if let Some(rest) = other.strip_prefix("sl:") {
                        // Shader Lab scalar: sl:<eff>:<name>
                        let parts: Vec<&str> = rest.split(':').collect();
                        if parts.len() >= 2 {
                            let _ = s.nudge_shaderlab_param(parts[0], parts[1], dx);
                        }
                    } else if let Some(rest) = other.strip_prefix("fx:") {
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

    /// Open keyboard entry for a scrub value (After Effects-style click-type).
    /// Creates a prefilled single-line editor, subscribes for commit (typing,
    /// Enter) and dismiss (focus loss), and focuses it. Edit-session state
    /// lives on `EditorState` (freely readable during render).
    pub fn begin_value_edit(&mut self, prop: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.end_value_edit(cx);
        let initial = self
            .state
            .read(cx)
            .scrub_current_value(prop)
            .map(|v| {
                if (v - v.round()).abs() < 1e-4 {
                    format!("{}", v.round() as i64)
                } else {
                    format!("{v:.2}")
                }
            })
            .unwrap_or_default();
        // Prefill inside the entity constructor, where the context type is
        // already `Context<InputState>` as `set_value` requires.
        let editor = cx.new(|cx| {
            let mut st = InputState::new(window, cx);
            st.set_value(initial, window, cx);
            st
        });
        let st = self.state.clone();
        let sub = cx.subscribe(&editor, move |_: &mut Self, input: Entity<InputState>, event: &InputEvent, cx| {
            match event {
                InputEvent::Change => {
                    let text = input.read(cx).value().trim().to_string();
                    st.update(cx, |s, cx| {
                        if s.commit_typed_value(&text) {
                            cx.notify();
                        }
                    });
                }
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    st.update(cx, |s, cx| {
                        if s.end_value_edit_state() {
                            cx.notify();
                        }
                    });
                }
                _ => {}
            }
        });
        let handle = editor.read(cx).focus_handle(cx);
        self.state.update(cx, |s, _| {
            s.value_edit_key = Some(prop.to_string());
            s.value_editor = Some(editor);
            s.value_editor_sub = Some(sub);
        });
        window.focus(&handle, cx);
        cx.notify();
    }

    /// Close keyboard entry without further changes.
    pub fn end_value_edit(&mut self, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| {
            if s.end_value_edit_state() {
                cx.notify();
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn scrub_field<FMinus, FPlus>(
    id: impl Into<ElementId>,
    prop_key: String,
    label: String,
    _minus_id: Option<ElementId>,
    _plus_id: Option<ElementId>,
    state: &Entity<EditorState>,
    panel_entity: &Entity<PropertiesPanel>,
    cx: &App,
    _on_minus: FMinus,
    _on_plus: FPlus,
) -> Div
where
    FMinus: Fn(&mut App) + 'static,
    FPlus: Fn(&mut App) + 'static,
{
    let panel_down = panel_entity.clone();
    let state_scroll = state.clone();
    let prop_for_wheel = prop_key.clone();
    let prop_for_edit = prop_key.clone();

    // After Effects-style keyboard entry: when this field is the open edit
    // target, render the live single-line editor instead of the value label.
    // (Drag-scrub and mouse-wheel still work on the label.)
    let edit_id = id.into();
    // Edit-session state lives on EditorState so render can read it freely
    // (reading the panel itself here would re-borrow it mid-render).
    let edit_state = state.read(cx);
    let editor_opt = edit_state.value_editor.clone();
    let is_editing = edit_state.value_edit_key.as_deref() == Some(prop_key.as_str());
    let value_child: AnyElement = match (is_editing, editor_opt) {
        (true, Some(editor)) => Input::new(&editor)
            .id(edit_id.clone())
            .w_full()
            .into_any_element(),
        _ => div()
            .id(edit_id)
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
                let p = prop_for_edit.clone();
                panel_down.update(cx, |this, _| {
                    this.scrub_prop = Some(p);
                    this.scrub_last_x = Some(curr_x);
                    this.scrub_moved = false;
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
                            "scale_u" => s.nudge_scale(step * 0.5, 0.0),
                            "rotation" => s.nudge_rotation(step * 0.5),
                            "opacity" => s.nudge_opacity(step * 0.5),
                            other => {
                                if let Some(rest) = other.strip_prefix("slc:") {
                                    let parts: Vec<&str> = rest.split(':').collect();
                                    if parts.len() >= 3 {
                                        if let Ok(idx) = parts[2].parse::<usize>() {
                                            let _ = s.nudge_shaderlab_component(parts[0], parts[1], idx, step * 0.25);
                                        }
                                    }
                                } else if let Some(rest) = other.strip_prefix("sl:") {
                                    let parts: Vec<&str> = rest.split(':').collect();
                                    if parts.len() >= 2 {
                                        let _ = s.nudge_shaderlab_param(parts[0], parts[1], step);
                                    }
                                } else if let Some(rest) = other.strip_prefix("fx:") {
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
            .child(label)
            .into_any_element(),
    };

    h_flex()
        .gap_1()
        .items_center()
        .flex_1()
        .child(value_child)
}

/// Keyframe controls widget for the Properties Panel inspector:
/// - Stopwatch toggle (enables/disables keyframing)
/// - When animated:
///   - ◂ Jump to previous keyframe
///   - ◆ Add or remove keyframe at the current playhead timecode
///   - ▸ Jump to next keyframe
fn property_keyframe_controls(
    state: &Entity<EditorState>,
    layer_id: &str,
    prop_path: &'static str,
    animated: bool,
    cx: &App,
) -> AnyElement {
    let editor = state.clone();
    let layer_id_str = layer_id.to_string();
    let (has_kf_at_playhead, has_prev_kf, has_next_kf) = {
        let s = state.read(cx);
        let current_tc = s.current_timecode();
        if let Some(comp) = s.active_composition() {
            if let Some(layer) = comp.get_layer(layer_id) {
                match prop_path {
                    "transform.anchor_point" => (
                        layer.transform.anchor_point.has_keyframe_at(&current_tc),
                        layer.transform.anchor_point.previous_keyframe_time(&current_tc).is_some(),
                        layer.transform.anchor_point.next_keyframe_time(&current_tc).is_some(),
                    ),
                    "transform.position" => (
                        layer.transform.position.has_keyframe_at(&current_tc),
                        layer.transform.position.previous_keyframe_time(&current_tc).is_some(),
                        layer.transform.position.next_keyframe_time(&current_tc).is_some(),
                    ),
                    "transform.scale" => (
                        layer.transform.scale.has_keyframe_at(&current_tc),
                        layer.transform.scale.previous_keyframe_time(&current_tc).is_some(),
                        layer.transform.scale.next_keyframe_time(&current_tc).is_some(),
                    ),
                    "transform.rotation" => (
                        layer.transform.rotation.has_keyframe_at(&current_tc),
                        layer.transform.rotation.previous_keyframe_time(&current_tc).is_some(),
                        layer.transform.rotation.next_keyframe_time(&current_tc).is_some(),
                    ),
                    "opacity" => (
                        layer.opacity.has_keyframe_at(&current_tc),
                        layer.opacity.previous_keyframe_time(&current_tc).is_some(),
                        layer.opacity.next_keyframe_time(&current_tc).is_some(),
                    ),
                    "text.font_size" => {
                        if let LayerSource::Text { ref font_size, .. } = layer.source {
                            (
                                font_size.has_keyframe_at(&current_tc),
                                font_size.previous_keyframe_time(&current_tc).is_some(),
                                font_size.next_keyframe_time(&current_tc).is_some(),
                            )
                        } else {
                            (false, false, false)
                        }
                    }
                    "text.fill_color" => {
                        if let LayerSource::Text { ref fill_color, .. } = layer.source {
                            (
                                fill_color.has_keyframe_at(&current_tc),
                                fill_color.previous_keyframe_time(&current_tc).is_some(),
                                fill_color.next_keyframe_time(&current_tc).is_some(),
                            )
                        } else {
                            (false, false, false)
                        }
                    }
                    "shape.rect_width" => {
                        if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref width, .. } } = layer.source {
                            (
                                width.has_keyframe_at(&current_tc),
                                width.previous_keyframe_time(&current_tc).is_some(),
                                width.next_keyframe_time(&current_tc).is_some(),
                            )
                        } else { (false, false, false) }
                    }
                    "shape.rect_height" => {
                        if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref height, .. } } = layer.source {
                            (
                                height.has_keyframe_at(&current_tc),
                                height.previous_keyframe_time(&current_tc).is_some(),
                                height.next_keyframe_time(&current_tc).is_some(),
                            )
                        } else { (false, false, false) }
                    }
                    "shape.corner_radius" => {
                        if let LayerSource::Shape { shape_type: project::ShapeType::Rectangle { ref corner_radius, .. } } = layer.source {
                            (
                                corner_radius.has_keyframe_at(&current_tc),
                                corner_radius.previous_keyframe_time(&current_tc).is_some(),
                                corner_radius.next_keyframe_time(&current_tc).is_some(),
                            )
                        } else { (false, false, false) }
                    }
                    "shape.ellipse_rx" => {
                        if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref radius_x, .. } } = layer.source {
                            (
                                radius_x.has_keyframe_at(&current_tc),
                                radius_x.previous_keyframe_time(&current_tc).is_some(),
                                radius_x.next_keyframe_time(&current_tc).is_some(),
                            )
                        } else { (false, false, false) }
                    }
                    "shape.ellipse_ry" => {
                        if let LayerSource::Shape { shape_type: project::ShapeType::Ellipse { ref radius_y, .. } } = layer.source {
                            (
                                radius_y.has_keyframe_at(&current_tc),
                                radius_y.previous_keyframe_time(&current_tc).is_some(),
                                radius_y.next_keyframe_time(&current_tc).is_some(),
                            )
                        } else { (false, false, false) }
                    }
                    _ => {
                        if let Some(rest) = prop_path.strip_prefix("effect:") {
                            let parts: Vec<&str> = rest.splitn(2, ':').collect();
                            if parts.len() == 2 {
                                let fx_id = parts[0];
                                let param_name = parts[1];
                                if let Some(fx) = layer.get_effect(fx_id) {
                                    if let Some(prop) = fx.get_param_property(param_name) {
                                        (
                                            prop.has_keyframe_at(&current_tc),
                                            prop.previous_keyframe_time(&current_tc).is_some(),
                                            prop.next_keyframe_time(&current_tc).is_some(),
                                        )
                                    } else { (false, false, false) }
                                } else { (false, false, false) }
                            } else { (false, false, false) }
                        } else { (false, false, false) }
                    }
                }
            } else { (false, false, false) }
        } else { (false, false, false) }
    };

    let s_toggle = editor.clone();
    let s_prev = editor.clone();
    let s_kf = editor.clone();
    let s_next = editor.clone();
    let lid1 = layer_id_str.clone();
    let lid2 = layer_id_str.clone();
    let lid3 = layer_id_str.clone();
    let lid4 = layer_id_str.clone();

    let stopwatch_btn = div()
        .id(SharedString::from(format!("property_stopwatch_{layer_id}_{prop_path}")))
        .test_support()
        .cursor_pointer()
        .p_0p5()
        .rounded_sm()
        .text_color(if animated { cx.theme().primary } else { cx.theme().muted_foreground })
        .hover(|s| s.bg(cx.theme().muted))
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            s_toggle.update(cx, |s, cx| {
                s.toggle_layer_property_animation(&lid1, prop_path);
                cx.notify();
            });
        })
        .child(icon_box(IconName::Timer));



    h_flex()
        .gap_0p5()
        .items_center()
        .child(stopwatch_btn)
        .child(
            div()
                .cursor_pointer()
                .px_0p5()
                .text_xs()
                .text_color(if has_prev_kf { cx.theme().foreground } else { cx.theme().muted_foreground.opacity(0.3) })
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
                .id(SharedString::from(format!("property_kf_{layer_id}_{prop_path}")))
                .test_support()
                .cursor_pointer()
                .px_0p5()
                .text_xs()
                .text_color(if has_kf_at_playhead { rgb(0xf59e0b).into() } else { cx.theme().muted_foreground })
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
                .text_color(if has_next_kf { cx.theme().foreground } else { cx.theme().muted_foreground.opacity(0.3) })
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
        .into_any_element()
}

fn property_stopwatch(
    state: &Entity<EditorState>,
    layer_id: &str,
    property: &'static str,
    animated: bool,
    cx: &App,
) -> AnyElement {
    property_keyframe_controls(state, layer_id, property, animated, cx)
}

fn effect_param_keyframe_controls(
    state: &Entity<EditorState>,
    layer_id: &str,
    eff_id: &str,
    param_name: &'static str,
    animated: bool,
    cx: &App,
) -> AnyElement {
    let editor = state.clone();
    let layer_id_str = layer_id.to_string();
    let prop_path = format!("effect:{eff_id}:{param_name}");
    let (has_kf_at_playhead, has_prev_kf, has_next_kf) = {
        let s = state.read(cx);
        let current_tc = s.current_timecode();
        if let Some(comp) = s.active_composition() {
            if let Some(layer) = comp.get_layer(layer_id) {
                if let Some(fx) = layer.get_effect(eff_id) {
                    if let Some(prop) = fx.get_param_property(param_name) {
                        (
                            prop.has_keyframe_at(&current_tc),
                            prop.previous_keyframe_time(&current_tc).is_some(),
                            prop.next_keyframe_time(&current_tc).is_some(),
                        )
                    } else { (false, false, false) }
                } else { (false, false, false) }
            } else { (false, false, false) }
        } else { (false, false, false) }
    };

    let s_toggle = editor.clone();
    let s_prev = editor.clone();
    let s_kf = editor.clone();
    let s_next = editor.clone();
    let lid1 = layer_id_str.clone();
    let lid2 = layer_id_str.clone();
    let lid3 = layer_id_str.clone();
    let lid4 = layer_id_str.clone();
    let p1 = prop_path.clone();
    let p2 = prop_path.clone();
    let p3 = prop_path.clone();
    let p4 = prop_path.clone();

    let stopwatch_btn = div()
        .id(SharedString::from(format!("property_stopwatch_{layer_id}_{eff_id}_{param_name}")))
        .test_support()
        .cursor_pointer()
        .p_0p5()
        .rounded_sm()
        .text_color(if animated { cx.theme().primary } else { cx.theme().muted_foreground })
        .hover(|s| s.bg(cx.theme().muted))
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            s_toggle.update(cx, |s, cx| {
                s.toggle_layer_property_animation(&lid1, &p1);
                cx.notify();
            });
        })
        .child(icon_box(IconName::Timer));



    h_flex()
        .gap_0p5()
        .items_center()
        .child(stopwatch_btn)
        .child(
            div()
                .cursor_pointer()
                .px_0p5()
                .text_xs()
                .text_color(if has_prev_kf { cx.theme().foreground } else { cx.theme().muted_foreground.opacity(0.3) })
                .hover(|s| s.bg(cx.theme().muted))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    if has_prev_kf {
                        s_prev.update(cx, |s, cx| {
                            s.seek_previous_keyframe(&lid2, &p2);
                            cx.notify();
                        });
                    }
                })
                .child("◂"),
        )
        .child(
            div()
                .id(SharedString::from(format!("property_kf_{layer_id}_{eff_id}_{param_name}")))
                .test_support()
                .cursor_pointer()
                .px_0p5()
                .text_xs()
                .text_color(if has_kf_at_playhead { rgb(0xf59e0b).into() } else { cx.theme().muted_foreground })
                .hover(|s| s.text_color(rgb(0xffffff)))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    s_kf.update(cx, |s, cx| {
                        s.toggle_layer_keyframe_at_current_time(&lid3, &p3);
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
                .text_color(if has_next_kf { cx.theme().foreground } else { cx.theme().muted_foreground.opacity(0.3) })
                .hover(|s| s.bg(cx.theme().muted))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    if has_next_kf {
                        s_next.update(cx, |s, cx| {
                            s.seek_next_keyframe(&lid4, &p4);
                            cx.notify();
                        });
                    }
                })
                .child("▸"),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn render_applied_effects(
    state: &Entity<EditorState>,
    layer: &project::Layer,
    panel_entity: &Entity<PropertiesPanel>,
    chroma_color_picker: &Entity<ColorPickerState>,
    tint_black_color_picker: &Entity<ColorPickerState>,
    tint_white_color_picker: &Entity<ColorPickerState>,
    shadow_color_picker: &Entity<ColorPickerState>,
    shader_editor_open: Option<String>,
    shader_editor: Option<Entity<TextareaState>>,
    cx: &App,
) -> AnyElement {
    if layer.effects.is_empty() {
        div()
            .id("no_effects_applied")
            .test_support()
            .p_2()
            .rounded_sm()
            .bg(cx.theme().secondary)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("No effects applied. Click an effect in the Effects tab to apply.")
            .into_any_element()
    } else {
        let mut fx_col = v_flex().id("applied_effects_list").test_support().gap_2();
        for effect in &layer.effects {
            let eff_id = effect.id.clone();
            let eff_id_toggle = effect.id.clone();
            let eff_id_del = effect.id.clone();
            let s_toggle = state.clone();
            let s_del = state.clone();

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
                    let s_m = state.clone();
                    let s_p = state.clone();
                    let id_m = eff_id.clone();
                    let id_p = eff_id.clone();
                    effect_box = effect_box.child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "radius", radius.is_animated(), cx))
                                    .child(div().text_color(cx.theme().muted_foreground).child("Radius")),
                            )
                            .child(scrub_field(
                                SharedString::from(format!("param_radius_{}", eff_id)),
                                format!("fx:{}:radius:100", eff_id),
                                format!("{:.1} px", r),
                                Some(ElementId::from(SharedString::from(format!("param_radius_minus_{}", eff_id)))),
                                Some(ElementId::from(SharedString::from(format!("param_radius_plus_{}", eff_id)))),
                                state,
                                panel_entity,
                                cx,
                                move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "radius", -5.0); cx.notify(); }),
                                move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "radius", 5.0); cx.notify(); }),
                            )),
                    );
                }
                EffectType::BrightnessContrast { brightness, contrast } => {
                    let b = brightness.value;
                    let c = contrast.value;
                    let s_bm = state.clone();
                    let s_bp = state.clone();
                    let s_cm = state.clone();
                    let s_cp = state.clone();
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
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "brightness", brightness.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Brightness")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_brightness_{}", eff_id)),
                                    format!("fx:{}:brightness:100", eff_id),
                                    format!("{:.1}", b),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
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
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "contrast", contrast.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Contrast")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_contrast_{}", eff_id)),
                                    format!("fx:{}:contrast:100", eff_id),
                                    format!("{:.1}", c),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
                                    cx,
                                    move |cx| s_cm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_cm, "contrast", -5.0); cx.notify(); }),
                                    move |cx| s_cp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_cp, "contrast", 5.0); cx.notify(); }),
                                )),
                        );
                }
                EffectType::Tint { map_black, map_white, amount } => {
                    let a = amount.value;
                    let mb = *map_black;
                    let mw = *map_white;
                    let mb_hex = format!("#{:02X}{:02X}{:02X}", (mb.r * 255.0) as u8, (mb.g * 255.0) as u8, (mb.b * 255.0) as u8);
                    let mw_hex = format!("#{:02X}{:02X}{:02X}", (mw.r * 255.0) as u8, (mw.g * 255.0) as u8, (mw.b * 255.0) as u8);
                    let s_m = state.clone();
                    let s_p = state.clone();
                    let s_tb = state.clone();
                    let s_tw = state.clone();
                    let id_m = eff_id.clone();
                    let id_p = eff_id.clone();
                    let id_tb = eff_id.clone();
                    let id_tw = eff_id.clone();

                    let black_presets = [
                        ("#000000", Color::from_hex("#000000").unwrap()),
                        ("#1A1A2E", Color::from_hex("#1A1A2E").unwrap()),
                        ("#1B262C", Color::from_hex("#1B262C").unwrap()),
                        ("#2C061F", Color::from_hex("#2C061F").unwrap()),
                    ];
                    let mut black_swatches = h_flex().gap_1().items_center();
                    for (hex_str, col_val) in black_presets {
                        let s_p = s_tb.clone();
                        let id_p = id_tb.clone();
                        black_swatches = black_swatches.child(
                            div()
                                .id(SharedString::from(format!("tint_black_preset_{hex_str}_{id_p}")))
                                .test_support()
                                .cursor_pointer()
                                .w(px(14.))
                                .h(px(14.))
                                .rounded_sm()
                                .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                                .border_1()
                                .border_color(if hex_str == mb_hex { cx.theme().primary } else { cx.theme().border })
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_p.update(cx, |s, cx| {
                                        let _ = s.set_tint_colors(&id_p, Some(col_val), None);
                                        cx.notify();
                                    });
                                })
                        );
                    }

                    let white_presets = [
                        ("#FFFFFF", Color::WHITE),
                        ("#F59E0B", Color::from_hex("#F59E0B").unwrap()),
                        ("#38BDF8", Color::from_hex("#38BDF8").unwrap()),
                        ("#EF4444", Color::from_hex("#EF4444").unwrap()),
                    ];
                    let mut white_swatches = h_flex().gap_1().items_center();
                    for (hex_str, col_val) in white_presets {
                        let s_p = s_tw.clone();
                        let id_p = id_tw.clone();
                        white_swatches = white_swatches.child(
                            div()
                                .id(SharedString::from(format!("tint_white_preset_{hex_str}_{id_p}")))
                                .test_support()
                                .cursor_pointer()
                                .w(px(14.))
                                .h(px(14.))
                                .rounded_sm()
                                .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                                .border_1()
                                .border_color(if hex_str == mw_hex { cx.theme().primary } else { cx.theme().border })
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_p.update(cx, |s, cx| {
                                        let _ = s.set_tint_colors(&id_p, None, Some(col_val));
                                        cx.notify();
                                    });
                                })
                        );
                    }

                    effect_box = effect_box
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .items_center()
                                        .child(div().text_color(cx.theme().muted_foreground).child("Map Black"))
                                        .child(
                                            div()
                                                .id("tint_black_swatch")
                                                .test_support()
                                                .w(px(20.))
                                                .h(px(14.))
                                                .rounded_sm()
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .bg(Rgba { r: mb.r, g: mb.g, b: mb.b, a: mb.a }),
                                        )
                                        .child(div().text_xs().text_color(cx.theme().foreground).child(mb_hex))
                                        .child(
                                            div()
                                                .id("tint_black_color_wheel")
                                                .test_support()
                                                .child(ColorPicker::new(tint_black_color_picker).label("Black"))
                                        ),
                                )
                                .child(black_swatches),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .items_center()
                                        .child(div().text_color(cx.theme().muted_foreground).child("Map White"))
                                        .child(
                                            div()
                                                .id("tint_white_swatch")
                                                .test_support()
                                                .w(px(20.))
                                                .h(px(14.))
                                                .rounded_sm()
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .bg(Rgba { r: mw.r, g: mw.g, b: mw.b, a: mw.a }),
                                        )
                                        .child(div().text_xs().text_color(cx.theme().foreground).child(mw_hex))
                                        .child(
                                            div()
                                                .id("tint_white_color_wheel")
                                                .test_support()
                                                .child(ColorPicker::new(tint_white_color_picker).label("White"))
                                        ),
                                )
                                .child(white_swatches),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "amount", amount.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Amount")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_amount_{}", eff_id)),
                                    format!("fx:{}:amount:100", eff_id),
                                    format!("{:.0} %", a),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
                                    cx,
                                    move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "amount", -10.0); cx.notify(); }),
                                    move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "amount", 10.0); cx.notify(); }),
                                )),
                        );
                }
                EffectType::Invert { amount } => {
                    let a = amount.value;
                    let s_m = state.clone();
                    let s_p = state.clone();
                    let id_m = eff_id.clone();
                    let id_p = eff_id.clone();
                    effect_box = effect_box.child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "amount", amount.is_animated(), cx))
                                    .child(div().text_color(cx.theme().muted_foreground).child("Amount")),
                            )
                            .child(scrub_field(
                                SharedString::from(format!("param_amount_{}", eff_id)),
                                format!("fx:{}:amount:100", eff_id),
                                format!("{:.0} %", a),
                                None,
                                None,
                                state,
                                panel_entity,
                                cx,
                                move |cx| s_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_m, "amount", -10.0); cx.notify(); }),
                                move |cx| s_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_p, "amount", 10.0); cx.notify(); }),
                            )),
                    );
                }
                EffectType::DropShadow { distance, angle, softness, opacity, color } => {
                    let d = distance.value;
                    let ang = angle.value;
                    let s_val = softness.value;
                    let o = opacity.value;
                    let sc = *color;
                    let sc_hex = format!("#{:02X}{:02X}{:02X}", (sc.r * 255.0) as u8, (sc.g * 255.0) as u8, (sc.b * 255.0) as u8);
                    let s_dm = state.clone();
                    let s_dp = state.clone();
                    let s_ang_m = state.clone();
                    let s_ang_p = state.clone();
                    let s_sm = state.clone();
                    let s_sp = state.clone();
                    let s_om = state.clone();
                    let s_op = state.clone();
                    let s_sc = state.clone();
                    let id_dm = eff_id.clone();
                    let id_dp = eff_id.clone();
                    let id_ang_m = eff_id.clone();
                    let id_ang_p = eff_id.clone();
                    let id_sm = eff_id.clone();
                    let id_sp = eff_id.clone();
                    let id_om = eff_id.clone();
                    let id_op = eff_id.clone();
                    let id_sc = eff_id.clone();

                    let shadow_presets = [
                        ("#000000", Color::from_hex("#000000").unwrap()),
                        ("#1E293B", Color::from_hex("#1E293B").unwrap()),
                        ("#0F172A", Color::from_hex("#0F172A").unwrap()),
                        ("#450A0A", Color::from_hex("#450A0A").unwrap()),
                        ("#1E1B4B", Color::from_hex("#1E1B4B").unwrap()),
                    ];
                    let mut shadow_swatches = h_flex().gap_1().items_center();
                    for (hex_str, col_val) in shadow_presets {
                        let s_p = s_sc.clone();
                        let id_p = id_sc.clone();
                        shadow_swatches = shadow_swatches.child(
                            div()
                                .id(SharedString::from(format!("shadow_preset_{hex_str}_{id_p}")))
                                .test_support()
                                .cursor_pointer()
                                .w(px(14.))
                                .h(px(14.))
                                .rounded_sm()
                                .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                                .border_1()
                                .border_color(if hex_str == sc_hex { cx.theme().primary } else { cx.theme().border })
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_p.update(cx, |s, cx| {
                                        let _ = s.set_drop_shadow_color(&id_p, col_val);
                                        cx.notify();
                                    });
                                })
                        );
                    }

                    effect_box = effect_box
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .items_center()
                                        .child(div().text_color(cx.theme().muted_foreground).child("Color"))
                                        .child(
                                            div()
                                                .id("shadow_color_swatch")
                                                .test_support()
                                                .w(px(20.))
                                                .h(px(14.))
                                                .rounded_sm()
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .bg(Rgba { r: sc.r, g: sc.g, b: sc.b, a: sc.a }),
                                        )
                                        .child(div().text_xs().text_color(cx.theme().foreground).child(sc_hex))
                                        .child(
                                            div()
                                                .id("shadow_color_wheel")
                                                .test_support()
                                                .child(ColorPicker::new(shadow_color_picker).label("Color"))
                                        ),
                                )
                                .child(shadow_swatches),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "distance", distance.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Distance")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_distance_{}", eff_id)),
                                    format!("fx:{}:distance:50", eff_id),
                                    format!("{:.1} px", d),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
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
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "angle", angle.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Angle")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_angle_{}", eff_id)),
                                    format!("fx:{}:angle:360", eff_id),
                                    format!("{:.1}°", ang),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
                                    cx,
                                    move |cx| s_ang_m.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_ang_m, "angle", -15.0); cx.notify(); }),
                                    move |cx| s_ang_p.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_ang_p, "angle", 15.0); cx.notify(); }),
                                )),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "softness", softness.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Softness")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_softness_{}", eff_id)),
                                    format!("fx:{}:softness:50", eff_id),
                                    format!("{:.1} px", s_val),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
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
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "opacity", opacity.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Opacity")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_opacity_{}", eff_id)),
                                    format!("fx:{}:opacity:100", eff_id),
                                    format!("{:.0} %", o),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
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
                    let s_p1m = state.clone();
                    let s_p1p = state.clone();
                    let s_p2m = state.clone();
                    let s_p2p = state.clone();
                    let s_p3m = state.clone();
                    let s_p3p = state.clone();
                    let s_p4m = state.clone();
                    let s_p4p = state.clone();
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
                        let s_preset = state.clone();
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
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "param1", param1.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("P1 (Speed)")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_p1_{}", eff_id)),
                                    format!("fx:{}:param1:10", eff_id),
                                    format!("{:.2}", p1),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
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
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "param2", param2.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("P2 (Intensity)")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_p2_{}", eff_id)),
                                    format!("fx:{}:param2:100", eff_id),
                                    format!("{:.1}", p2),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
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
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "param3", param3.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("P3 (Scale)")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_p3_{}", eff_id)),
                                    format!("fx:{}:param3:10", eff_id),
                                    format!("{:.2}", p3),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
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
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "param4", param4.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("P4 (Opacity)")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_p4_{}", eff_id)),
                                    format!("fx:{}:param4:100", eff_id),
                                    format!("{:.1}", p4),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
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
                                        .id("glsl_code_preview")
                                        .test_support()
                                        .p_1p5()
                                        .rounded_sm()
                                        .bg(cx.theme().muted)
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .overflow_y_scroll()
                                        .max_h(px(160.))
                                        .child(code_preview),
                                ),
                        );
                }
                EffectType::ShaderLab { source: _, params, values, compile_error } => {
                    // Resolved live values (overrides win over defaults).
                    let resolved: HashMap<&str, &ShaderParamValue> = params
                        .iter()
                        .map(|p| (p.name.as_str(), values.get(&p.name).unwrap_or(&p.default)))
                        .collect();

                    // Status: last-good source always runs; errors show here.
                    let status_row: AnyElement = match compile_error {
                        Some(err) => v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(rgb(0xef4444))
                                    .child("● Apply failed — previous shader still active"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0xef4444))
                                    .child(err.clone()),
                            )
                            .into_any_element(),
                        None => div()
                            .text_xs()
                            .text_color(rgb(0x22c55e))
                            .child(format!(
                                "● Ready — {} param{} · GPU validated",
                                params.len(),
                                if params.len() == 1 { "" } else { "s" }
                            ))
                            .into_any_element(),
                    };
                    effect_box = effect_box.child(status_row);

                    // Auto-generated parameter UI (adding/removing a uniform
                    // in the source changes this list after Apply).
                    let mut last_group: Option<&str> = None;
                    for param in params {
                        if param.group.as_deref() != last_group {
                            last_group = param.group.as_deref();
                            if let Some(g) = last_group {
                                effect_box = effect_box.child(
                                    div()
                                        .mt_1()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(cx.theme().foreground)
                                        .child(g.to_string()),
                                );
                            }
                        }
                        let pname = param.name.clone();
                        let plabel = param.label.clone();
                        let eff_key = eff_id.clone();
                        match &param.param_type {
                            project::shader::ShaderParamType::Float
                            | project::shader::ShaderParamType::Angle
                            | project::shader::ShaderParamType::Int => {
                                let cur = match resolved.get(pname.as_str()) {
                                    Some(ShaderParamValue::Int(v)) => format!("{v}"),
                                    Some(v) => v.display(),
                                    None => String::new(),
                                };
                                let unit = if matches!(param.param_type, project::shader::ShaderParamType::Angle) {
                                    "°"
                                } else {
                                    ""
                                };
                                effect_box = effect_box.child(
                                    h_flex()
                                        .items_center()
                                        .justify_between()
                                        .text_xs()
                                        .child(
                                            div()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("{plabel}{unit}")),
                                        )
                                        .child(scrub_field(
                                            SharedString::from(format!("shader_param_{eff_key}_{pname}")),
                                            format!("sl:{eff_key}:{pname}"),
                                            cur,
                                            None,
                                            None,
                                            state,
                                            panel_entity,
                                            cx,
                                            move |_| {},
                                            move |_| {},
                                        )),
                                );
                            }
                            project::shader::ShaderParamType::Bool => {
                                let on = matches!(
                                    resolved.get(pname.as_str()),
                                    Some(ShaderParamValue::Bool(true))
                                );
                                let s_t = state.clone();
                                let eid = eff_key.clone();
                                let pn = pname.clone();
                                effect_box = effect_box.child(
                                    h_flex()
                                        .items_center()
                                        .justify_between()
                                        .text_xs()
                                        .child(
                                            div()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(plabel),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(if on { cx.theme().primary } else { cx.theme().muted })
                                                .text_color(if on {
                                                    cx.theme().primary_foreground
                                                } else {
                                                    cx.theme().foreground
                                                })
                                                .hover(|s| s.opacity(0.85))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    s_t.update(cx, |s, cx| {
                                                        let _ = s.set_shaderlab_param(&eid, &pn, if on { 0.0 } else { 1.0 });
                                                        cx.notify();
                                                    });
                                                })
                                                .child(if on { "On" } else { "Off" }),
                                        ),
                                );
                            }
                            project::shader::ShaderParamType::Enum { options } => {
                                let idx = match resolved.get(pname.as_str()) {
                                    Some(ShaderParamValue::Int(v)) => (*v).clamp(0, options.len().saturating_sub(1) as i32) as usize,
                                    _ => 0,
                                };
                                let cur_label = options.get(idx).cloned().unwrap_or_else(|| format!("{idx}"));
                                let s_c = state.clone();
                                let eid = eff_key.clone();
                                let pn = pname.clone();
                                let n_opts = options.len().max(1) as f32;
                                effect_box = effect_box.child(
                                    h_flex()
                                        .items_center()
                                        .justify_between()
                                        .text_xs()
                                        .child(
                                            div()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(plabel),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(cx.theme().muted)
                                                .text_color(cx.theme().foreground)
                                                .hover(|s| s.bg(cx.theme().accent))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    s_c.update(cx, |s, cx| {
                                                        let next = (idx as f32 + 1.0) % n_opts;
                                                        let _ = s.set_shaderlab_param(&eid, &pn, next);
                                                        cx.notify();
                                                    });
                                                })
                                                .child(format!("◂ {cur_label} ▸")),
                                        ),
                                );
                            }
                            project::shader::ShaderParamType::Vec2
                            | project::shader::ShaderParamType::Vec3
                            | project::shader::ShaderParamType::Vec4
                            | project::shader::ShaderParamType::Color => {
                                let (count, tags): (usize, &[&str]) = match param.param_type {
                                    project::shader::ShaderParamType::Vec2 => (2, &["X", "Y"]),
                                    project::shader::ShaderParamType::Vec3 => (3, &["X", "Y", "Z"]),
                                    project::shader::ShaderParamType::Vec4 => (4, &["X", "Y", "Z", "W"]),
                                    _ => (4, &["R", "G", "B", "A"]),
                                };
                                effect_box = effect_box.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(plabel),
                                );
                                for i in 0..count {
                                    let comp_val = match resolved.get(pname.as_str()) {
                                        Some(ShaderParamValue::Vec2(a)) => a.get(i).copied().unwrap_or(0.0),
                                        Some(ShaderParamValue::Vec3(a)) => a.get(i).copied().unwrap_or(0.0),
                                        Some(ShaderParamValue::Vec4(a)) => a.get(i).copied().unwrap_or(0.0),
                                        Some(ShaderParamValue::Color(c)) => {
                                            [c.r, c.g, c.b, c.a].get(i).copied().unwrap_or(0.0)
                                        }
                                        Some(ShaderParamValue::Float(v)) => *v,
                                        _ => 0.0,
                                    };
                                    effect_box = effect_box.child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .text_xs()
                                            .child(
                                                div()
                                                    .w(px(52.))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(format!("  {}", tags[i])),
                                            )
                                            .child(scrub_field(
                                                SharedString::from(format!(
                                                    "shader_param_{eff_key}_{pname}_{i}"
                                                )),
                                                format!("slc:{eff_key}:{pname}:{i}"),
                                                format!("{comp_val:.3}"),
                                                None,
                                                None,
                                                state,
                                                panel_entity,
                                                cx,
                                                move |_| {},
                                                move |_| {},
                                            )),
                                    );
                                }
                            }
                        }
                    }

                    // Edit Shader toggle.
                    let p_ed = panel_entity.clone();
                    let eid_ed = eff_id.clone();
                    let editor_open = shader_editor_open.as_deref() == Some(eff_id.as_str());
                    effect_box = effect_box.child(
                        div()
                            .id(SharedString::from(format!("shader_edit_toggle_{eff_id}")))
                            .test_support()
                            .cursor_pointer()
                            .mt_1()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .font_medium()
                            .bg(if editor_open { cx.theme().primary } else { cx.theme().muted })
                            .text_color(if editor_open {
                                cx.theme().primary_foreground
                            } else {
                                cx.theme().foreground
                            })
                            .hover(|s| s.opacity(0.9))
                            .text_center()
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                p_ed.update(cx, |this, cx| {
                                    if this.shader_editor_open.as_deref() == Some(eid_ed.as_str()) {
                                        this.shader_editor_open = None;
                                    } else {
                                        this.shader_editor_open = Some(eid_ed.clone());
                                    }
                                    cx.notify();
                                });
                            })
                            .child(if editor_open { "Close Shader Editor" } else { "Edit Shader" }),
                    );

                    if editor_open {
                        if let Some(ed) = shader_editor.as_ref() {
                            let s_apply = state.clone();
                            let eid_apply = eff_id.clone();
                            let ed_apply = ed.clone();
                            let s_export = state.clone();
                            let eid_export = eff_id.clone();
                            let s_import = state.clone();
                            let eid_import = eff_id.clone();
                            effect_box = effect_box
                                .child(
                                    div()
                                        .id(SharedString::from(format!("shader_editor_box_{eff_id}")))
                                        .test_support()
                                        .mt_1()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .rounded_sm()
                                        .child(
                                            Textarea::new(ed)
                                                .h(px(220.))
                                                .bordered(true),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .mt_1()
                                        .gap_1p5()
                                        .items_center()
                                        .child(
                                            div()
                                                .id(SharedString::from(format!("shader_apply_{eff_id}")))
                                                .test_support()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .text_xs()
                                                .font_semibold()
                                                .bg(cx.theme().primary)
                                                .text_color(cx.theme().primary_foreground)
                                                .hover(|s| s.opacity(0.9))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    let src = ed_apply.read(cx).value().to_string();
                                                    s_apply.update(cx, |s, cx| {
                                                        let _ = s.apply_shader_source(&eid_apply, &src);
                                                        cx.notify();
                                                    });
                                                })
                                                .child("Apply"),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .text_xs()
                                                .bg(cx.theme().muted)
                                                .text_color(cx.theme().foreground)
                                                .hover(|s| s.bg(cx.theme().accent))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    let s_exp = s_export.clone();
                                                    let eid_exp = eid_export.clone();
                                                    let src_now = s_exp.read(cx)
                                                        .selected_layer()
                                                        .and_then(|l| l.get_effect(&eid_exp))
                                                        .and_then(|e| e.shader_source().map(str::to_string))
                                                        .unwrap_or_default();
                                                    cx.spawn(|cx: &mut AsyncApp| {
                                                        let cx = cx.clone();
                                                        async move {
                                                            let (tx, rx) = std::sync::mpsc::channel();
                                                            let _ = std::thread::Builder::new()
                                                                .name("shader-export-worker".to_string())
                                                                .stack_size(8 * 1024 * 1024)
                                                                .spawn(move || {
                                                                    let file = rfd::FileDialog::new()
                                                                        .add_filter("GLSL Shader", &["glsl"])
                                                                        .set_file_name("effect.glsl")
                                                                        .save_file();
                                                                    let _ = tx.send(file);
                                                                });
                                                            if let Ok(Some(path)) = rx.recv() {
                                                                let _ = std::fs::write(&path, &src_now);
                                                                let _ = cx;
                                                            }
                                                        }
                                                    }).detach();
                                                })
                                                .child("Export"),
                                        )
                                        .child(
                                            div()
                                                .cursor_pointer()
                                                .px_2()
                                                .py_1()
                                                .rounded_sm()
                                                .text_xs()
                                                .bg(cx.theme().muted)
                                                .text_color(cx.theme().foreground)
                                                .hover(|s| s.bg(cx.theme().accent))
                                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                    let s_imp = s_import.clone();
                                                    let eid_imp = eid_import.clone();
                                                    cx.spawn(|cx: &mut AsyncApp| {
                                                        let cx = cx.clone();
                                                        async move {
                                                            let (tx, rx) = std::sync::mpsc::channel();
                                                            let _ = std::thread::Builder::new()
                                                                .name("shader-import-worker".to_string())
                                                                .stack_size(8 * 1024 * 1024)
                                                                .spawn(move || {
                                                                    let file = rfd::FileDialog::new()
                                                                        .add_filter("GLSL Shader", &["glsl", "txt"])
                                                                        .pick_file();
                                                                    let _ = tx.send(file);
                                                                });
                                                            if let Ok(Some(path)) = rx.recv() {
                                                                if let Ok(src) = std::fs::read_to_string(&path) {
                                                                    cx.update(|cx| {
                                                                        s_imp.update(cx, |s, cx| {
                                                                            let _ = s.apply_shader_source(&eid_imp, &src);
                                                                            cx.notify();
                                                                        });
                                                                    });
                                                                }
                                                            }
                                                        }
                                                    }).detach();
                                                })
                                                .child("Import"),
                                        ),
                                );
                            // Preset snippets (validate on the way in).
                            let mut preset_row = h_flex().mt_1().gap_1p5().items_center().flex_wrap();
                            for (pname, psrc) in [
                                ("Grade", shader_presets::GRADE),
                                ("Vignette", shader_presets::VIGNETTE),
                                ("Scanlines", shader_presets::SCANLINES),
                                ("Duotone", shader_presets::DUOTONE),
                            ] {
                                let s_pre = state.clone();
                                let eid_pre = eff_id.clone();
                                let psrc = psrc.to_string();
                                preset_row = preset_row.child(
                                    div()
                                        .cursor_pointer()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(cx.theme().muted)
                                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                        .text_xs()
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            let c = psrc.clone();
                                            s_pre.update(cx, |s, cx| {
                                                let _ = s.apply_shader_source(&eid_pre, &c);
                                                cx.notify();
                                            });
                                        })
                                        .child(pname),
                                );
                            }
                            effect_box = effect_box.child(preset_row);
                        }
                    }
                }
                EffectType::DisplacementMap { max_horizontal, max_vertical } => {
                    let mh = max_horizontal.value;
                    let mv = max_vertical.value;
                    let s_hm = state.clone();
                    let s_hp = state.clone();
                    let s_vm = state.clone();
                    let s_vp = state.clone();
                    let id_hm = eff_id.clone();
                    let id_hp = eff_id.clone();
                    let id_vm = eff_id.clone();
                    let id_vp = eff_id.clone();
                    effect_box = effect_box
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "max_horizontal", max_horizontal.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Max Horizontal")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_maxh_{}", eff_id)),
                                    format!("fx:{}:max_horizontal:100", eff_id),
                                    format!("{:.1} px", mh),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
                                    cx,
                                    move |cx| s_hm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_hm, "max_horizontal", -5.0); cx.notify(); }),
                                    move |cx| s_hp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_hp, "max_horizontal", 5.0); cx.notify(); }),
                                )),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "max_vertical", max_vertical.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Max Vertical")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_maxv_{}", eff_id)),
                                    format!("fx:{}:max_vertical:100", eff_id),
                                    format!("{:.1} px", mv),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
                                    cx,
                                    move |cx| s_vm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_vm, "max_vertical", -5.0); cx.notify(); }),
                                    move |cx| s_vp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_vp, "max_vertical", 5.0); cx.notify(); }),
                                )),
                        );
                }
                EffectType::ChromaKey { key_color, tolerance, feather } => {
                    let tol = tolerance.value;
                    let fth = feather.value;
                    let s_tm = state.clone();
                    let s_tp = state.clone();
                    let s_fm = state.clone();
                    let s_fp = state.clone();
                    let s_ck = state.clone();
                    let id_tm = eff_id.clone();
                    let id_tp = eff_id.clone();
                    let id_fm = eff_id.clone();
                    let id_fp = eff_id.clone();
                    let id_ck = eff_id.clone();
                    let kc = *key_color;
                    let ck_hex = format!("#{:02X}{:02X}{:02X}", (kc.r * 255.0) as u8, (kc.g * 255.0) as u8, (kc.b * 255.0) as u8);

                    let chroma_presets = [
                        ("#00FF00", Color::from_hex("#00FF00").unwrap()),
                        ("#0000FF", Color::from_hex("#0000FF").unwrap()),
                        ("#00BFFF", Color::from_hex("#00BFFF").unwrap()),
                        ("#FF00FF", Color::from_hex("#FF00FF").unwrap()),
                        ("#000000", Color::from_hex("#000000").unwrap()),
                        ("#FFFFFF", Color::WHITE),
                    ];
                    let mut ck_swatches = h_flex().gap_1().items_center();
                    for (hex_str, col_val) in chroma_presets {
                        let s_p = s_ck.clone();
                        let id_p = id_ck.clone();
                        ck_swatches = ck_swatches.child(
                            div()
                                .id(SharedString::from(format!("chroma_preset_{hex_str}_{id_p}")))
                                .test_support()
                                .cursor_pointer()
                                .w(px(14.))
                                .h(px(14.))
                                .rounded_sm()
                                .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                                .border_1()
                                .border_color(if hex_str == ck_hex { cx.theme().primary } else { cx.theme().border })
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_p.update(cx, |s, cx| {
                                        let _ = s.set_chroma_key_color(&id_p, col_val);
                                        cx.notify();
                                    });
                                })
                        );
                    }

                    effect_box = effect_box
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .items_center()
                                        .child(div().text_color(cx.theme().muted_foreground).child("Key Color"))
                                        .child(
                                            div()
                                                .id("chroma_color_swatch")
                                                .test_support()
                                                .w(px(20.))
                                                .h(px(14.))
                                                .rounded_sm()
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .bg(Rgba { r: kc.r, g: kc.g, b: kc.b, a: kc.a }),
                                        )
                                        .child(
                                            div()
                                                .id("chroma_color_hex")
                                                .test_support()
                                                .text_xs()
                                                .text_color(cx.theme().foreground)
                                                .child(ck_hex)
                                        )
                                        .child(
                                            div()
                                                .id("chroma_color_wheel")
                                                .test_support()
                                                .child(ColorPicker::new(chroma_color_picker).label("Pick"))
                                        ),
                                )
                                .child(ck_swatches),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "tolerance", tolerance.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Tolerance")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_tol_{}", eff_id)),
                                    format!("fx:{}:tolerance:100", eff_id),
                                    format!("{:.1}", tol),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
                                    cx,
                                    move |cx| s_tm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_tm, "tolerance", -5.0); cx.notify(); }),
                                    move |cx| s_tp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_tp, "tolerance", 5.0); cx.notify(); }),
                                )),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "feather", feather.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Feather")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_fth_{}", eff_id)),
                                    format!("fx:{}:feather:100", eff_id),
                                    format!("{:.1}", fth),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
                                    cx,
                                    move |cx| s_fm.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_fm, "feather", -2.0); cx.notify(); }),
                                    move |cx| s_fp.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_fp, "feather", 2.0); cx.notify(); }),
                                )),
                        );
                }
                EffectType::NoiseGenerator { amount, monochrome } => {
                    let amt = amount.value;
                    let mono = *monochrome;
                    let s_am = state.clone();
                    let s_ap = state.clone();
                    let s_mono = state.clone();
                    let id_am = eff_id.clone();
                    let id_ap = eff_id.clone();
                    let id_mono = eff_id.clone();
                    effect_box = effect_box
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(effect_param_keyframe_controls(state, &layer.id, &eff_id, "amount", amount.is_animated(), cx))
                                        .child(div().text_color(cx.theme().muted_foreground).child("Amount")),
                                )
                                .child(scrub_field(
                                    SharedString::from(format!("param_noise_amt_{}", eff_id)),
                                    format!("fx:{}:amount:100", eff_id),
                                    format!("{:.1}%", amt),
                                    None,
                                    None,
                                    state,
                                    panel_entity,
                                    cx,
                                    move |cx| s_am.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_am, "amount", -5.0); cx.notify(); }),
                                    move |cx| s_ap.update(cx, |s, cx| { let _ = s.nudge_effect_param(&id_ap, "amount", 5.0); cx.notify(); }),
                                )),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .py_0p5()
                                .child(div().text_color(cx.theme().muted_foreground).child("Monochrome"))
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(if mono { cx.theme().accent } else { cx.theme().muted })
                                        .text_color(if mono { cx.theme().accent_foreground } else { cx.theme().muted_foreground })
                                        .child(if mono { "ON" } else { "OFF" })
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            s_mono.update(cx, |s, cx| {
                                                let _ = s.toggle_noise_monochrome(&id_mono);
                                                cx.notify();
                                            });
                                        }),
                                ),
                        );
                }
            }

            fx_col = fx_col.child(effect_box);
        }
        fx_col.into_any_element()
    }
}

impl EventEmitter<PanelEvent> for PropertiesPanel {}

impl Focusable for PropertiesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PropertiesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor_for_inputs = self.state.clone();
        let text_inputs = window.use_keyed_state("properties_text_inspector_inputs", cx, move |window, cx| {
            let text = cx.new(|cx| InputState::new(window, cx));
            let font_family = cx.new(|cx| InputState::new(window, cx));
            let font_size = cx.new(|cx| InputState::new(window, cx));
            let mut subscriptions = Vec::new();
            for (input, field) in [(text.clone(), "text"), (font_family.clone(), "font"), (font_size.clone(), "size")] {
                let editor = editor_for_inputs.clone();
                subscriptions.push(cx.subscribe(&input, move |_, input, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::Change) { return; }
                    let value = input.read(cx).value();
                    editor.update(cx, |state, cx| {
                        let Some(layer_id) = state.selected_layer_id.clone() else { return; };
                        let result = match field {
                            "text" => state.set_layer_text(&layer_id, &value),
                            "font" => state.set_layer_font_family(&layer_id, &value),
                            "size" => value.parse::<f32>().map_err(|_| "Font size must be a number".to_string()).and_then(|size| state.set_layer_font_size(&layer_id, size)),
                            _ => Ok(()),
                        };
                        if result.is_ok() { cx.notify(); }
                    });
                }));
            }
            TextInspectorInputs { text, font_family, font_size, _subscriptions: subscriptions }
        });
        let editor_for_color = self.state.clone();
        let inspector_color = window.use_keyed_state("properties_color_picker", cx, move |window, cx| {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx));
            let editor = editor_for_color.clone();
            let subscription = cx.subscribe(&picker, move |_, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(hsla)) = event else { return; };
                let rgba: Rgba = (*hsla).into();
                let color = Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a);
                editor.update(cx, |state, _cx| {
                    let Some(layer_id) = state.selected_layer_id.clone() else { return; };
                    let _ = match state.selected_layer().map(|layer| &layer.source) {
                        Some(LayerSource::Text { .. }) => state.set_layer_text_color(&layer_id, color),
                        Some(LayerSource::Solid { .. }) => state.set_layer_solid_color(&layer_id, color),
                        _ => Ok(()),
                    };
                });
            });
            InspectorColorPicker { state: picker, _subscription: subscription }
        });

        let editor_for_chroma = self.state.clone();
        let chroma_color_picker = window.use_keyed_state("properties_chroma_color_picker", cx, move |window, cx| {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx));
            let editor = editor_for_chroma.clone();
            let subscription = cx.subscribe(&picker, move |_, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(hsla)) = event else { return; };
                let rgba: Rgba = (*hsla).into();
                let color = Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a);
                editor.update(cx, |state, cx| {
                    let eff_id = state.selected_layer().and_then(|layer| {
                        layer.effects.iter().find(|e| matches!(e.effect_type, EffectType::ChromaKey { .. })).map(|e| e.id.clone())
                    });
                    if let Some(id) = eff_id {
                        let _ = state.set_chroma_key_color(&id, color);
                        cx.notify();
                    }
                });
            });
            InspectorColorPicker { state: picker, _subscription: subscription }
        });

        let editor_for_tb = self.state.clone();
        let tint_black_color_picker = window.use_keyed_state("properties_tint_black_color_picker", cx, move |window, cx| {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx));
            let editor = editor_for_tb.clone();
            let subscription = cx.subscribe(&picker, move |_, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(hsla)) = event else { return; };
                let rgba: Rgba = (*hsla).into();
                let color = Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a);
                editor.update(cx, |state, cx| {
                    let eff_id = state.selected_layer().and_then(|layer| {
                        layer.effects.iter().find(|e| matches!(e.effect_type, EffectType::Tint { .. })).map(|e| e.id.clone())
                    });
                    if let Some(id) = eff_id {
                        let _ = state.set_tint_colors(&id, Some(color), None);
                        cx.notify();
                    }
                });
            });
            InspectorColorPicker { state: picker, _subscription: subscription }
        });

        let editor_for_tw = self.state.clone();
        let tint_white_color_picker = window.use_keyed_state("properties_tint_white_color_picker", cx, move |window, cx| {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx));
            let editor = editor_for_tw.clone();
            let subscription = cx.subscribe(&picker, move |_, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(hsla)) = event else { return; };
                let rgba: Rgba = (*hsla).into();
                let color = Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a);
                editor.update(cx, |state, cx| {
                    let eff_id = state.selected_layer().and_then(|layer| {
                        layer.effects.iter().find(|e| matches!(e.effect_type, EffectType::Tint { .. })).map(|e| e.id.clone())
                    });
                    if let Some(id) = eff_id {
                        let _ = state.set_tint_colors(&id, None, Some(color));
                        cx.notify();
                    }
                });
            });
            InspectorColorPicker { state: picker, _subscription: subscription }
        });

        let editor_for_shadow = self.state.clone();
        let shadow_color_picker = window.use_keyed_state("properties_shadow_color_picker", cx, move |window, cx| {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx));
            let editor = editor_for_shadow.clone();
            let subscription = cx.subscribe(&picker, move |_, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(hsla)) = event else { return; };
                let rgba: Rgba = (*hsla).into();
                let color = Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a);
                editor.update(cx, |state, cx| {
                    let eff_id = state.selected_layer().and_then(|layer| {
                        layer.effects.iter().find(|e| matches!(e.effect_type, EffectType::DropShadow { .. })).map(|e| e.id.clone())
                    });
                    if let Some(id) = eff_id {
                        let _ = state.set_drop_shadow_color(&id, color);
                        cx.notify();
                    }
                });
            });
            InspectorColorPicker { state: picker, _subscription: subscription }
        });

        // Sync inspector color, effect colors, and text inputs with current selected layer before reading state
        let (layer_info, effect_colors, is_open) = {
            let state = self.state.read(cx);
            let layer_info = state.selected_layer().map(|l| (l.id.clone(), l.source.clone()));
            let mut chroma_col = None;
            let mut tint_b = None;
            let mut tint_w = None;
            let mut shadow_col = None;
            if let Some(layer) = state.selected_layer() {
                for eff in &layer.effects {
                    match &eff.effect_type {
                        EffectType::ChromaKey { key_color, .. } => {
                            chroma_col = Some(*key_color);
                        }
                        EffectType::Tint { map_black, map_white, .. } => {
                            tint_b = Some(*map_black);
                            tint_w = Some(*map_white);
                        }
                        EffectType::DropShadow { color, .. } => {
                            shadow_col = Some(*color);
                        }
                        _ => {}
                    }
                }
            }
            let is_open = inspector_color.read(cx).state.read(cx).is_open();
            let chroma_open = chroma_color_picker.read(cx).state.read(cx).is_open();
            let tb_open = tint_black_color_picker.read(cx).state.read(cx).is_open();
            let tw_open = tint_white_color_picker.read(cx).state.read(cx).is_open();
            let shadow_open = shadow_color_picker.read(cx).state.read(cx).is_open();
            (
                layer_info,
                (chroma_col, tint_b, tint_w, shadow_col, chroma_open, tb_open, tw_open, shadow_open),
                is_open,
            )
        };

        // Sync the single Shader Lab source editor slot. The key embeds the
        // opened effect id + source hash so Apply/presets/imports refresh the
        // draft while typing never loses it.
        {
            let (open_id, open_src) = {
                let st = self.state.read(cx);
                let open = self.shader_editor_open.clone();
                let src = open.as_ref().and_then(|id| {
                    st.selected_layer()
                        .and_then(|l| l.get_effect(id))
                        .and_then(|e| e.shader_source().map(str::to_string))
                });
                (open, src)
            };
            let want_key: Option<(String, u64)> = open_id
                .zip(open_src.clone())
                .map(|(id, src)| (id, renderer::shader_lab::hash_source(&src)));
            if self.shader_editor_key != want_key {
                self.shader_editor_key = want_key;
                self.shader_editor = open_src.map(|src| {
                    cx.new(|cx| {
                        let mut st = TextareaState::new(window, cx);
                        st.set_value(src, window, cx);
                        st
                    })
                });
            }
        }

        if let Some((_lid, ref src)) = layer_info {
            match src {
                LayerSource::Text { text, font_family, font_size, fill_color } => {
                    let text_in = text_inputs.read(cx).text.clone();
                    let font_in = text_inputs.read(cx).font_family.clone();
                    let size_in = text_inputs.read(cx).font_size.clone();

                    let target_text = text.value.clone();
                    let target_font = font_family.clone();
                    let target_size = format!("{:.0}", font_size.value);

                    text_in.update(cx, |inp, cx| {
                        if inp.value() != target_text.as_str() {
                            inp.set_value(target_text.as_str(), window, cx);
                        }
                    });
                    font_in.update(cx, |inp, cx| {
                        if inp.value() != target_font.as_str() {
                            inp.set_value(target_font.as_str(), window, cx);
                        }
                    });
                    size_in.update(cx, |inp, cx| {
                        if inp.value() != target_size.as_str() {
                            inp.set_value(target_size.as_str(), window, cx);
                        }
                    });

                    if !is_open {
                        let c = fill_color.value;
                        let hsla: Hsla = Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into();
                        let picker_ent = inspector_color.read(cx).state.clone();
                        picker_ent.update(cx, |p, cx| {
                            p.set_value(hsla, window, cx);
                        });
                    }
                }
                LayerSource::Solid { color, .. } => {
                    if !is_open {
                        let c = *color;
                        let hsla: Hsla = Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into();
                        let picker_ent = inspector_color.read(cx).state.clone();
                        picker_ent.update(cx, |p, cx| {
                            p.set_value(hsla, window, cx);
                        });
                    }
                }
                _ => {}
            }
        }

        let (chroma_col, tint_b, tint_w, shadow_col, chroma_open, tb_open, tw_open, shadow_open) = effect_colors;
        if let Some(c) = chroma_col {
            if !chroma_open {
                let hsla: Hsla = Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into();
                let picker_ent = chroma_color_picker.read(cx).state.clone();
                picker_ent.update(cx, |p, cx| {
                    p.set_value(hsla, window, cx);
                });
            }
        }
        if let Some(c) = tint_b {
            if !tb_open {
                let hsla: Hsla = Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into();
                let picker_ent = tint_black_color_picker.read(cx).state.clone();
                picker_ent.update(cx, |p, cx| {
                    p.set_value(hsla, window, cx);
                });
            }
        }
        if let Some(c) = tint_w {
            if !tw_open {
                let hsla: Hsla = Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into();
                let picker_ent = tint_white_color_picker.read(cx).state.clone();
                picker_ent.update(cx, |p, cx| {
                    p.set_value(hsla, window, cx);
                });
            }
        }
        if let Some(c) = shadow_col {
            if !shadow_open {
                let hsla: Hsla = Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into();
                let picker_ent = shadow_color_picker.read(cx).state.clone();
                picker_ent.update(cx, |p, cx| {
                    p.set_value(hsla, window, cx);
                });
            }
        }

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
                    LayerSource::Adjustment => "Adjustment Layer",
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
                        this.scrub_moved = false;
                    }
                    return;
                }
                if let (Some(prop), Some(last_x)) = (this.scrub_prop.clone(), this.scrub_last_x) {
                    let curr_x = event.position.x / px(1.0);
                    let dx = curr_x - last_x;
                    if dx.abs() >= 1.0 {
                        this.apply_scrub_delta(&prop, dx, cx);
                        this.scrub_last_x = Some(curr_x);
                        this.scrub_moved = true;
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _event, window, cx| {
                // Click (no drag movement) on a value field opens keyboard
                // entry After Effects-style; a real drag just ends the scrub.
                if let Some(prop) = this.scrub_prop.take() {
                    this.scrub_last_x = None;
                    let was_drag = std::mem::replace(&mut this.scrub_moved, false);
                    if !was_drag {
                        this.begin_value_edit(&prop, window, cx);
                    }
                }
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.scrub_prop = None;
                this.scrub_last_x = None;
                this.scrub_moved = false;
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
                    .overflow_y_scroll()
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

                        let effects_list = render_applied_effects(
                            &self.state,
                            layer,
                            &panel_entity,
                            &chroma_color_picker.read(cx).state,
                            &tint_black_color_picker.read(cx).state,
                            &tint_white_color_picker.read(cx).state,
                            &shadow_color_picker.read(cx).state,
                            self.shader_editor_open.clone(),
                            self.shader_editor.clone(),
                            cx,
                        );

                        let mut props_items: Vec<AnyElement> = Vec::new();

                        // --- Source-Specific Properties Section ---
                        match &layer.source {
                            LayerSource::Solid { color, width, height } => {
                                let c = *color;
                                let w = *width;
                                let h = *height;
                                let lid_c = layer.id.clone();
                                let s_swatch = self.state.clone();
                                let s_rgb = self.state.clone();

                                let hex_code = format!("#{:02X}{:02X}{:02X}", (c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8);

                                let presets = [
                                    ("#EF4444", Color::from_hex("#EF4444").unwrap()),
                                    ("#10B981", Color::from_hex("#10B981").unwrap()),
                                    ("#3B82F6", Color::from_hex("#3B82F6").unwrap()),
                                    ("#F59E0B", Color::from_hex("#F59E0B").unwrap()),
                                    ("#8B5CF6", Color::from_hex("#8B5CF6").unwrap()),
                                    ("#06B6D4", Color::from_hex("#06B6D4").unwrap()),
                                    ("#FFFFFF", Color::WHITE),
                                    ("#121316", Color::from_hex("#121316").unwrap()),
                                ];

                                let mut palette_row = h_flex().gap_1().items_center();
                                for (hex_str, col_val) in presets {
                                    let s_p = s_swatch.clone();
                                    let lid_p = lid_c.clone();
                                    palette_row = palette_row.child(
                                        div()
                                            .id(SharedString::from(format!("solid_color_preset_{hex_str}")))
                                            .test_support()
                                            .cursor_pointer()
                                            .w(px(14.))
                                            .h(px(14.))
                                            .rounded_sm()
                                            .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                                            .border_1()
                                            .border_color(if hex_str == hex_code { cx.theme().primary } else { cx.theme().border })
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_p.update(cx, |s, cx| {
                                                    let _ = s.set_layer_solid_color(&lid_p, col_val);
                                                    cx.notify();
                                                });
                                            })
                                    );
                                }

                                let s_rm = s_rgb.clone();
                                let s_rp = s_rgb.clone();
                                let s_gm = s_rgb.clone();
                                let s_gp = s_rgb.clone();
                                let s_bm = s_rgb.clone();
                                let s_bp = s_rgb.clone();
                                let lid_r1 = lid_c.clone();
                                let lid_r2 = lid_c.clone();
                                let lid_g1 = lid_c.clone();
                                let lid_g2 = lid_c.clone();
                                let lid_b1 = lid_c.clone();
                                let lid_b2 = lid_c.clone();

                                props_items.push(
                                    v_flex()
                                        .id("solid_properties_section")
                                        .test_support()
                                        .gap_2()
                                        .p_2()
                                        .rounded_sm()
                                        .bg(cx.theme().secondary)
                                        .child(
                                            h_flex()
                                                .gap_1p5()
                                                .items_center()
                                                .font_semibold()
                                                .text_xs()
                                                .text_color(cx.theme().foreground)
                                                .child(icon_box(IconName::Square))
                                                .child(format!("Solid Source ({}x{})", w, h)),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(
                                                    h_flex()
                                                        .gap_2()
                                                        .items_center()
                                                        .child(
                                                            div()
                                                                .id("solid_color_swatch")
                                                                .test_support()
                                                                .w(px(28.))
                                                                .h(px(20.))
                                                                .rounded_sm()
                                                                .bg(Rgba { r: c.r, g: c.g, b: c.b, a: 1.0 })
                                                                .border_1()
                                                                .border_color(cx.theme().border),
                                                        )
                                                        .child(
                                                            div()
                                                                .id("solid_color_hex")
                                                                .test_support()
                                                                .font_semibold()
                                                                .child(hex_code),
                                                        )
                                                        .child(div().id("solid_color_wheel").test_support().child(ColorPicker::new(&inspector_color.read(cx).state).label("Color")))
                                                )
                                                .child(
                                                    h_flex()
                                                        .gap_1()
                                                        .items_center()
                                                        .child(step_button_with_id("solid_r_minus", "R-", cx, move |cx| s_rm.update(cx, |s, cx| { let _ = s.nudge_layer_solid_color(&lid_r1, -0.1, 0.0, 0.0); cx.notify(); })))
                                                        .child(step_button_with_id("solid_r_plus", "R+", cx, move |cx| s_rp.update(cx, |s, cx| { let _ = s.nudge_layer_solid_color(&lid_r2, 0.1, 0.0, 0.0); cx.notify(); })))
                                                        .child(step_button_with_id("solid_g_minus", "G-", cx, move |cx| s_gm.update(cx, |s, cx| { let _ = s.nudge_layer_solid_color(&lid_g1, 0.0, -0.1, 0.0); cx.notify(); })))
                                                        .child(step_button_with_id("solid_g_plus", "G+", cx, move |cx| s_gp.update(cx, |s, cx| { let _ = s.nudge_layer_solid_color(&lid_g2, 0.0, 0.1, 0.0); cx.notify(); })))
                                                        .child(step_button_with_id("solid_b_minus", "B-", cx, move |cx| s_bm.update(cx, |s, cx| { let _ = s.nudge_layer_solid_color(&lid_b1, 0.0, 0.0, -0.1); cx.notify(); })))
                                                        .child(step_button_with_id("solid_b_plus", "B+", cx, move |cx| s_bp.update(cx, |s, cx| { let _ = s.nudge_layer_solid_color(&lid_b2, 0.0, 0.0, 0.1); cx.notify(); }))),
                                                ),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Presets"))
                                                .child(palette_row),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Dimensions"))
                                                .child(
                                                    h_flex()
                                                        .gap_1()
                                                        .items_center()
                                                        .child(scrub_field(
                                                            "solid_dim_w",
                                                            "solid_w".to_string(),
                                                            format!("{w} px"),
                                                            None,
                                                            None,
                                                            &self.state,
                                                            &panel_entity,
                                                            cx,
                                                            move |_| {},
                                                            move |_| {},
                                                        ))
                                                        .child(scrub_field(
                                                            "solid_dim_h",
                                                            "solid_h".to_string(),
                                                            format!("{h} px"),
                                                            None,
                                                            None,
                                                            &self.state,
                                                            &panel_entity,
                                                            cx,
                                                            move |_| {},
                                                            move |_| {},
                                                        )),
                                                ),
                                        )
                                        .into_any_element(),
                                );
                            }
                            LayerSource::Text { text, font_family, font_size, fill_color } => {
                                let lid_t = layer.id.clone();
                                let s_text = self.state.clone();
                                let s_fs = self.state.clone();
                                let s_col = self.state.clone();

                                let cur_text = text.value.clone();
                                let cur_fs = font_size.value;
                                let cur_fam = font_family.clone();
                                let cur_col = fill_color.value;

                                let inputs = text_inputs.read(cx);

                                let presets = ["Title Text", "Motion Studio", "Subheading", "After Effects"];
                                let mut text_presets = h_flex().gap_1().items_center().flex_wrap();
                                for p_str in presets {
                                    let s_p = s_text.clone();
                                    let lid_p = lid_t.clone();
                                    let target_str = p_str.to_string();
                                    text_presets = text_presets.child(
                                        div()
                                            .id(SharedString::from(format!("text_preset_{p_str}")))
                                            .test_support()
                                            .cursor_pointer()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                            .text_xs()
                                            .child(p_str)
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                let t = target_str.clone();
                                                s_p.update(cx, |s, cx| {
                                                    let _ = s.set_layer_text(&lid_p, &t);
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }

                                let s_fsm = s_fs.clone();
                                let s_fsp = s_fs.clone();
                                let lid_fs1 = lid_t.clone();
                                let lid_fs2 = lid_t.clone();

                                let mut fs_buttons = h_flex().gap_1().items_center();
                                fs_buttons = fs_buttons
                                    .child(step_button_with_id("font_size_minus", "-4", cx, move |cx| s_fsm.update(cx, |s, cx| { let _ = s.nudge_layer_font_size(&lid_fs1, -4.0); cx.notify(); })))
                                    .child(step_button_with_id("font_size_plus", "+4", cx, move |cx| s_fsp.update(cx, |s, cx| { let _ = s.nudge_layer_font_size(&lid_fs2, 4.0); cx.notify(); })));
                                for sz in [24.0f32, 36.0, 48.0, 72.0] {
                                    let s_sz = s_fs.clone();
                                    let lid_sz = lid_t.clone();
                                    fs_buttons = fs_buttons.child(
                                        div()
                                            .cursor_pointer()
                                            .px_1()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(if (cur_fs - sz).abs() < 1.0 { cx.theme().primary } else { cx.theme().muted })
                                            .text_color(if (cur_fs - sz).abs() < 1.0 { cx.theme().primary_foreground } else { cx.theme().foreground })
                                            .text_xs()
                                            .child(format!("{:.0}", sz))
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_sz.update(cx, |s, cx| {
                                                    let _ = s.set_layer_font_size(&lid_sz, sz);
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }

                                let col_presets = [
                                    ("#FFFFFF", Color::WHITE),
                                    ("#F59E0B", Color::from_hex("#F59E0B").unwrap()),
                                    ("#38BDF8", Color::from_hex("#38BDF8").unwrap()),
                                    ("#EF4444", Color::from_hex("#EF4444").unwrap()),
                                    ("#10B981", Color::from_hex("#10B981").unwrap()),
                                ];
                                let mut text_col_row = h_flex().gap_1().items_center();
                                for (hex_str, col_val) in col_presets {
                                    let s_cp = s_col.clone();
                                    let lid_cp = lid_t.clone();
                                    text_col_row = text_col_row.child(
                                        div()
                                            .id(SharedString::from(format!("text_color_preset_{hex_str}")))
                                            .test_support()
                                            .cursor_pointer()
                                            .w(px(14.))
                                            .h(px(14.))
                                            .rounded_sm()
                                            .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_cp.update(cx, |s, cx| {
                                                    let _ = s.set_layer_text_color(&lid_cp, col_val);
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }

                                let sys_fonts = EditorState::available_system_fonts();
                                let mut font_presets_row = h_flex().gap_1().items_center().flex_wrap();
                                for fam in sys_fonts.iter().take(12) {
                                    let s_fam = s_text.clone();
                                    let lid_fam = lid_t.clone();
                                    let is_sel = cur_fam.eq_ignore_ascii_case(fam);
                                    let fam_str = fam.clone();
                                    font_presets_row = font_presets_row.child(
                                        div()
                                            .id(SharedString::from(format!("font_preset_{fam}")))
                                            .test_support()
                                            .cursor_pointer()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(if is_sel { cx.theme().primary } else { cx.theme().muted })
                                            .text_color(if is_sel { cx.theme().primary_foreground } else { cx.theme().foreground })
                                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                            .text_xs()
                                            .child(fam.clone())
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                let f = fam_str.clone();
                                                s_fam.update(cx, |s, cx| {
                                                    let _ = s.set_layer_font_family(&lid_fam, &f);
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }

                                props_items.push(
                                    v_flex()
                                        .id("text_properties_section")
                                        .test_support()
                                        .gap_2()
                                        .p_2()
                                        .rounded_sm()
                                        .bg(cx.theme().secondary)
                                        .child(
                                            h_flex()
                                                .gap_1p5()
                                                .items_center()
                                                .font_semibold()
                                                .text_xs()
                                                .text_color(cx.theme().foreground)
                                                .child(icon_box(IconName::Type))
                                                .child(property_stopwatch(&self.state, &layer.id, "text.source", text.is_animated(), cx))
                                                .child(property_stopwatch(&self.state, &layer.id, "text.font_size", font_size.is_animated(), cx))
                                                .child(property_stopwatch(&self.state, &layer.id, "text.fill_color", fill_color.is_animated(), cx))
                                                .child(format!("Text Layer (\"{}\")", cur_text)),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Text"))
                                                .child(Input::new(&inputs.text).id("text_content_input").w(px(220.))),
                                        )
                                        .child(text_presets)
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(Input::new(&inputs.font_family).id("text_font_input").w(px(130.)))
                                                .child(Input::new(&inputs.font_size).id("text_size_input").w(px(60.)))
                                                .child(fs_buttons),
                                        )
                                        .child(font_presets_row)
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(
                                                    h_flex()
                                                        .gap_1p5()
                                                        .items_center()
                                                        .child(div().text_color(cx.theme().muted_foreground).child("Color"))
                                                        .child(div().w(px(14.)).h(px(14.)).rounded_sm().bg(Rgba { r: cur_col.r, g: cur_col.g, b: cur_col.b, a: 1.0 }).border_1().border_color(cx.theme().border))
                                                        .child(div().id("text_color_wheel").test_support().child(ColorPicker::new(&inspector_color.read(cx).state).label("Fill")))
                                                )
                                                .child(text_col_row),
                                        )
                                        .into_any_element(),
                                );
                            }
                            LayerSource::Shape { shape_type } => {
                                let lid_sh = layer.id.clone();
                                match shape_type {
                                    ShapeType::Rectangle { width, height, corner_radius } => {
                                        let w = width.value;
                                        let h = height.value;
                                        let cr = corner_radius.value;
                                        let s_sh = self.state.clone();
                                        let lid_w1 = lid_sh.clone();
                                        let lid_w2 = lid_sh.clone();
                                        let lid_h1 = lid_sh.clone();
                                        let lid_h2 = lid_sh.clone();
                                        let lid_cr1 = lid_sh.clone();
                                        let lid_cr2 = lid_sh.clone();
                                        let s_wm = s_sh.clone();
                                        let s_wp = s_sh.clone();
                                        let s_hm = s_sh.clone();
                                        let s_hp = s_sh.clone();
                                        let s_crm = s_sh.clone();
                                        let s_crp = s_sh.clone();

                                        props_items.push(
                                            v_flex()
                                                .id("shape_properties_section")
                                                .test_support()
                                                .gap_2()
                                                .p_2()
                                                .rounded_sm()
                                                .bg(cx.theme().secondary)
                                                .child(
                                                    h_flex()
                                                        .gap_1p5()
                                                        .items_center()
                                                        .font_semibold()
                                                        .text_xs()
                                                        .text_color(cx.theme().foreground)
                                                        .child(icon_box(IconName::Square))
                                                        .child("Rectangle Geometry"),
                                                )
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child(format!("Width: {:.0}px", w)))
                                                        .child(
                                                            h_flex()
                                                                .gap_1()
                                                                .items_center()
                                                                .child(step_button_with_id("rect_w_minus", "-20", cx, move |cx| s_wm.update(cx, |s, cx| { let _ = s.nudge_layer_rect_dimensions(&lid_w1, -20.0, 0.0, 0.0); cx.notify(); })))
                                                                .child(step_button_with_id("rect_w_plus", "+20", cx, move |cx| s_wp.update(cx, |s, cx| { let _ = s.nudge_layer_rect_dimensions(&lid_w2, 20.0, 0.0, 0.0); cx.notify(); }))),
                                                        ),
                                                )
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child(format!("Height: {:.0}px", h)))
                                                        .child(
                                                            h_flex()
                                                                .gap_1()
                                                                .items_center()
                                                                .child(step_button_with_id("rect_h_minus", "-20", cx, move |cx| s_hm.update(cx, |s, cx| { let _ = s.nudge_layer_rect_dimensions(&lid_h1, 0.0, -20.0, 0.0); cx.notify(); })))
                                                                .child(step_button_with_id("rect_h_plus", "+20", cx, move |cx| s_hp.update(cx, |s, cx| { let _ = s.nudge_layer_rect_dimensions(&lid_h2, 0.0, 20.0, 0.0); cx.notify(); }))),
                                                        ),
                                                )
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child(format!("Corner Radius: {:.0}px", cr)))
                                                        .child(
                                                            h_flex()
                                                                .gap_1()
                                                                .items_center()
                                                                .child(step_button_with_id("rect_cr_minus", "-5", cx, move |cx| s_crm.update(cx, |s, cx| { let _ = s.nudge_layer_rect_dimensions(&lid_cr1, 0.0, 0.0, -5.0); cx.notify(); })))
                                                                .child(step_button_with_id("rect_cr_plus", "+5", cx, move |cx| s_crp.update(cx, |s, cx| { let _ = s.nudge_layer_rect_dimensions(&lid_cr2, 0.0, 0.0, 5.0); cx.notify(); }))),
                                                        ),
                                                )
                                                .into_any_element(),
                                        );
                                    }
                                    ShapeType::Ellipse { radius_x, radius_y } => {
                                        let rx = radius_x.value;
                                        let ry = radius_y.value;
                                        let s_sh = self.state.clone();
                                        let lid_rx1 = lid_sh.clone();
                                        let lid_rx2 = lid_sh.clone();
                                        let lid_ry1 = lid_sh.clone();
                                        let lid_ry2 = lid_sh.clone();
                                        let s_rxm = s_sh.clone();
                                        let s_rxp = s_sh.clone();
                                        let s_rym = s_sh.clone();
                                        let s_ryp = s_sh.clone();

                                        props_items.push(
                                            v_flex()
                                                .id("shape_properties_section")
                                                .test_support()
                                                .gap_2()
                                                .p_2()
                                                .rounded_sm()
                                                .bg(cx.theme().secondary)
                                                .child(
                                                    h_flex()
                                                        .gap_1p5()
                                                        .items_center()
                                                        .font_semibold()
                                                        .text_xs()
                                                        .text_color(cx.theme().foreground)
                                                        .child(icon_box(IconName::Circle))
                                                        .child("Ellipse Geometry"),
                                                )
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child(format!("Radius X: {:.0}px", rx)))
                                                        .child(
                                                            h_flex()
                                                                .gap_1()
                                                                .items_center()
                                                                .child(step_button_with_id("ellipse_rx_minus", "-10", cx, move |cx| s_rxm.update(cx, |s, cx| { let _ = s.nudge_layer_ellipse_radii(&lid_rx1, -10.0, 0.0); cx.notify(); })))
                                                                .child(step_button_with_id("ellipse_rx_plus", "+10", cx, move |cx| s_rxp.update(cx, |s, cx| { let _ = s.nudge_layer_ellipse_radii(&lid_rx2, 10.0, 0.0); cx.notify(); }))),
                                                        ),
                                                )
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child(format!("Radius Y: {:.0}px", ry)))
                                                        .child(
                                                            h_flex()
                                                                .gap_1()
                                                                .items_center()
                                                                .child(step_button_with_id("ellipse_ry_minus", "-10", cx, move |cx| s_rym.update(cx, |s, cx| { let _ = s.nudge_layer_ellipse_radii(&lid_ry1, 0.0, -10.0); cx.notify(); })))
                                                                .child(step_button_with_id("ellipse_ry_plus", "+10", cx, move |cx| s_ryp.update(cx, |s, cx| { let _ = s.nudge_layer_ellipse_radii(&lid_ry2, 0.0, 10.0); cx.notify(); }))),
                                                        ),
                                                )
                                                .into_any_element(),
                                        );
                                    }
                                    ShapeType::Path { path_data } => {
                                        props_items.push(
                                            v_flex()
                                                .id("shape_properties_section")
                                                .test_support()
                                                .gap_2()
                                                .p_2()
                                                .rounded_sm()
                                                .bg(cx.theme().secondary)
                                                .child(
                                                    h_flex()
                                                        .gap_1p5()
                                                        .items_center()
                                                        .font_semibold()
                                                        .text_xs()
                                                        .text_color(cx.theme().foreground)
                                                        .child(icon_box(IconName::Pen))
                                                        .child("Vector Pen Path"),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(format!("Path: {}", if path_data.len() > 40 { format!("{}...", &path_data[..40]) } else { path_data.clone() })),
                                                )
                                                .into_any_element(),
                                        );
                                    }
                                }
                            }
                            LayerSource::Image { asset_id } => {
                                let asset = state.project.get_asset(asset_id);
                                let asset_name = asset.map(|a| a.name.clone()).unwrap_or_else(|| asset_id.clone());
                                let path_str = asset.map(|a| a.path.to_string_lossy().to_string()).unwrap_or_default();
                                props_items.push(
                                    v_flex()
                                        .id("media_properties_section")
                                        .test_support()
                                        .gap_1p5()
                                        .p_2()
                                        .rounded_sm()
                                        .bg(cx.theme().secondary)
                                        .child(
                                            h_flex()
                                                .gap_1p5()
                                                .items_center()
                                                .font_semibold()
                                                .text_xs()
                                                .text_color(cx.theme().foreground)
                                                .child(icon_box(IconName::Film))
                                                .child(format!("Image: {asset_name}")),
                                        )
                                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(path_str))
                                        .into_any_element(),
                                );
                            }
                            LayerSource::Video { asset_id, media_start } => {
                                let asset = state.project.get_asset(asset_id);
                                let asset_name = asset.map(|a| a.name.clone()).unwrap_or_else(|| asset_id.clone());
                                let path_str = asset.map(|a| a.path.to_string_lossy().to_string()).unwrap_or_default();
                                props_items.push(
                                    v_flex()
                                        .id("media_properties_section")
                                        .test_support()
                                        .gap_1p5()
                                        .p_2()
                                        .rounded_sm()
                                        .bg(cx.theme().secondary)
                                        .child(
                                            h_flex()
                                                .gap_1p5()
                                                .items_center()
                                                .font_semibold()
                                                .text_xs()
                                                .text_color(cx.theme().foreground)
                                                .child(icon_box(IconName::Film))
                                                .child(format!("Video: {asset_name}")),
                                        )
                                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("Start: {} • File: {}", media_start, path_str)))
                                        .into_any_element(),
                                );
                            }
                            LayerSource::Adjustment => {
                                props_items.push(
                                    v_flex()
                                        .id("adjustment_properties_section")
                                        .test_support()
                                        .gap_1p5()
                                        .p_2()
                                        .rounded_sm()
                                        .bg(cx.theme().secondary)
                                        .child(
                                            h_flex()
                                                .gap_1p5()
                                                .items_center()
                                                .font_semibold()
                                                .text_xs()
                                                .text_color(cx.theme().foreground)
                                                .child(icon_box(IconName::SlidersHorizontal))
                                                .child("Adjustment Layer"),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Applies effects and blend modes to all underlying layers."),
                                        )
                                        .into_any_element(),
                                );
                            }
                            _ => {}
                        }

                        // --- Transform Card ---
                        let s_reset = self.state.clone();
                        let lid_reset = layer.id.clone();
                        let transform_expanded = self.transform_expanded;
                        let mut transform_card = v_flex()
                            .p_2p5()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .gap_2()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        h_flex()
                                            .id("transform_card_header")
                                            .gap_1p5()
                                            .items_center()
                                            .font_semibold()
                                            .text_xs()
                                            .text_color(cx.theme().foreground)
                                            .cursor_pointer()
                                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
                                                this.transform_expanded = !this.transform_expanded;
                                                cx.notify();
                                            }))
                                            .child(icon_box(if transform_expanded { IconName::ChevronDown } else { IconName::ChevronRight }))
                                            .child("Transform"),
                                    )
                                    .child(
                                        step_button("Reset", cx, move |cx| {
                                            s_reset.update(cx, |s, cx| {
                                                s.reset_layer_transform(&lid_reset);
                                                cx.notify();
                                            });
                                        })
                                    ),
                            );
                        if transform_expanded {
                            transform_card = transform_card
                            // Anchor Point (X, Y)
                            .child(
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
                                            .child(property_stopwatch(&self.state, &layer.id, "transform.anchor_point", layer.transform.anchor_point.is_animated(), cx))
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
                            )
                            // Position (X, Y)
                            .child(
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
                                            .child(property_stopwatch(&self.state, &layer.id, "transform.position", layer.transform.position.is_animated(), cx))
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
                            )
                            // Scale: uniform single value by default, X/Y when unlinked
                            .child(
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
                                            .child(property_stopwatch(&self.state, &layer.id, "transform.scale", layer.transform.scale.is_animated(), cx))
                                            .child("Scale"),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .items_center()
                                            .child({
                                                let s_link = self.state.clone();
                                                let lid_link = layer.id.clone();
                                                let linked = layer.transform.scale_uniform;
                                                div()
                                                    .id(SharedString::from(format!("scale_link_{}", layer.id)))
                                                    .test_support()
                                                    .cursor_pointer()
                                                    .p_0p5()
                                                    .rounded_sm()
                                                    .text_color(if linked { cx.theme().primary } else { cx.theme().muted_foreground })
                                                    .hover(|s| s.bg(cx.theme().muted))
                                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                        s_link.update(cx, |s, cx| {
                                                            s.toggle_layer_scale_link(&lid_link);
                                                            cx.notify();
                                                        });
                                                    })
                                                    .child(icon_box(if linked { IconName::Link } else { IconName::Unlink }))
                                            })
                                            .child(if layer.transform.scale_uniform {
                                                scrub_field(
                                                    "prop_scale_u",
                                                    "scale_u".to_string(),
                                                    format!("{:.1} %", sc.x),
                                                    None,
                                                    None,
                                                    &self.state,
                                                    &panel_entity,
                                                    cx,
                                                    move |cx| s_scale_mx.update(cx, |s, cx| { s.nudge_scale(-10.0, 0.0); cx.notify(); }),
                                                    move |cx| s_scale_px.update(cx, |s, cx| { s.nudge_scale(10.0, 0.0); cx.notify(); }),
                                                ).into_any_element()
                                            } else {
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
                                                    ))
                                                    .into_any_element()
                                            }),
                                    ),
                            )
                            // Rotation
                            .child(
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
                                            .child(property_stopwatch(&self.state, &layer.id, "transform.rotation", layer.transform.rotation.is_animated(), cx))
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
                            )
                            // Opacity
                            .child(
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
                                            .child(property_stopwatch(&self.state, &layer.id, "opacity", layer.opacity.is_animated(), cx))
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
                            );
                        }

                        // --- Switches & Modes Card ---
                        let switches_card = v_flex()
                            .p_2p5()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .font_semibold()
                                    .text_xs()
                                    .text_color(cx.theme().foreground)
                                    .child(icon_box(IconName::Eye))
                                    .child("Switches & Modes"),
                            )
                            .child(
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
                            );

                        // --- Effects Card ---
                        let effects_card = v_flex()
                            .p_2p5()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .font_semibold()
                                    .text_xs()
                                    .text_color(cx.theme().foreground)
                                    .child(icon_box(IconName::SlidersHorizontal))
                                    .child(format!("Applied Effects ({})", layer.effects.len())),
                            )
                            .child(div().child(effects_list));

                        props_items.push(transform_card.into_any_element());
                        props_items.push(switches_card.into_any_element());
                        props_items.push(effects_card.into_any_element());

                        props_items
                    } else {
                        vec![
                            div()
                                .p_4()
                                .text_center()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("No layer selected. Select a layer from the timeline or project panel to inspect its properties.")
                                .into_any_element(),
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
                    .overflow_y_scroll()
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
                    .child(effect_item_row("shader_lab", "Shader Lab", EffectType::shader_lab(shader_presets::GRADE), &self.state, cx))
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

    let nav = h_flex()
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
                .id(SharedString::from(format!("timeline_kf_{layer_id}_{prop_path}")))
                .test_support()
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
                        // After Effects behavior: the diamond always works.
                        // When the stopwatch is off, this records the first
                        // keyframe and enables animation for the property.
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
        );

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
    EmptyTrackArea,
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
    pub is_scrubbing_ruler: bool,
    /// Drag state for layer strip interactions (After Effects-style)
    pub drag_action: Option<TimelineDragAction>,
    pub drag_last_x: f32,
}

/// Describes what kind of drag the user is performing on the timeline layer strip.
#[derive(Clone, Debug)]
pub enum TimelineDragAction {
    /// Dragging the body of a layer strip (slip/move both in+out)
    SlipLayer {
        layer_id: String,
        initial_mouse_x: f32,
        initial_in_frame: i64,
        initial_out_frame: i64,
    },
    /// Dragging the left trim handle (change in-point)
    TrimIn {
        layer_id: String,
        initial_mouse_x: f32,
        initial_in_frame: i64,
        initial_out_frame: i64,
    },
    /// Dragging the right trim handle (change out-point)
    TrimOut {
        layer_id: String,
        initial_mouse_x: f32,
        initial_in_frame: i64,
        initial_out_frame: i64,
    },
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
            is_scrubbing_ruler: false,
            drag_action: None,
            drag_last_x: 0.0,
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
                let p_layer_rclick = panel_entity.clone();
                let lid_rclick = layer.id.clone();

                // Drag-start clones for trim-in, body-slip, trim-out
                let p_drag_tin = panel_entity.clone();
                let lid_drag_tin = layer.id.clone();
                let p_drag_slip = panel_entity.clone();
                let lid_drag_slip = layer.id.clone();
                let p_drag_tout = panel_entity.clone();
                let lid_drag_tout = layer.id.clone();
                let layer_in_frame = layer.in_point.frames();
                let layer_out_frame = layer.out_point.frames();

                let track_col = div()
                    .flex_1()
                    .h(px(26.))
                    .relative()
                    .child(
                        // Layer span bar
                        h_flex()
                            .id(SharedString::from(format!("track_span_{}", layer.id)))
                            .test_support()
                            .absolute()
                            .top(px(3.))
                            .bottom(px(3.))
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
                            .items_center()
                            .justify_between()
                            .child(
                                // Left Trim Handle (In-point) — drag to trim
                                div()
                                    .id(SharedString::from(format!("track_trim_in_{}", layer.id)))
                                    .test_support()
                                    .w(px(12.))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_col_resize()
                                    .bg(rgba(0x00000033))
                                    .hover(|s| s.bg(rgba(0xffffff44)))
                                    .text_color(rgb(0xffffff))
                                    .text_xs()
                                    .font_bold()
                                    .child("[")
                                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                        let mx = event.position.x / px(1.0);
                                        p_drag_tin.update(cx, |this, _cx| {
                                            this.drag_action = Some(TimelineDragAction::TrimIn {
                                                layer_id: lid_drag_tin.clone(),
                                                initial_mouse_x: mx,
                                                initial_in_frame: layer_in_frame,
                                                initial_out_frame: layer_out_frame,
                                            });
                                            this.drag_last_x = mx;
                                        });
                                    }),
                            )
                            .child(
                                // Center Body — drag to slip/move the layer strip
                                h_flex()
                                    .id(SharedString::from(format!("track_slip_{}", layer.id)))
                                    .test_support()
                                    .flex_1()
                                    .h_full()
                                    .px_1()
                                    .items_center()
                                    .justify_center()
                                    .cursor_grab()
                                    .overflow_hidden()
                                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                        let mx = event.position.x / px(1.0);
                                        // Select the layer and start dragging
                                        span_state.update(cx, |s, cx| {
                                            s.select_layer(Some(lid_span.clone()));
                                            cx.notify();
                                        });
                                        p_drag_slip.update(cx, |this, _cx| {
                                            this.drag_action = Some(TimelineDragAction::SlipLayer {
                                                layer_id: lid_drag_slip.clone(),
                                                initial_mouse_x: mx,
                                                initial_in_frame: layer_in_frame,
                                                initial_out_frame: layer_out_frame,
                                            });
                                            this.drag_last_x = mx;
                                        });
                                    })
                                    .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                                        p_layer_rclick.update(cx, |this, cx| {
                                            this.open_context_menu(ContextMenuTarget::Layer(lid_rclick.clone()));
                                            cx.notify();
                                        });
                                    })
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(0xffffff))
                                            .truncate()
                                            .child(format!("{} [{} - {}]", layer.name, layer.in_point, layer.out_point)),
                                    ),
                            )
                            .child(
                                // Right Trim Handle (Out-point) — drag to trim
                                div()
                                    .id(SharedString::from(format!("track_trim_out_{}", layer.id)))
                                    .test_support()
                                    .w(px(12.))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_col_resize()
                                    .bg(rgba(0x00000033))
                                    .hover(|s| s.bg(rgba(0xffffff44)))
                                    .text_color(rgb(0xffffff))
                                    .text_xs()
                                    .font_bold()
                                    .child("]")
                                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                        let mx = event.position.x / px(1.0);
                                        p_drag_tout.update(cx, |this, _cx| {
                                            this.drag_action = Some(TimelineDragAction::TrimOut {
                                                layer_id: lid_drag_tout.clone(),
                                                initial_mouse_x: mx,
                                                initial_in_frame: layer_in_frame,
                                                initial_out_frame: layer_out_frame,
                                            });
                                            this.drag_last_x = mx;
                                        });
                                    }),
                            ),
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
                                        EffectType::DisplacementMap { max_horizontal, max_vertical } => {
                                            param_entries.push(("max_horizontal", "Max Horizontal", max_horizontal.evaluate_at(&current_tc), 5.0));
                                            param_entries.push(("max_vertical", "Max Vertical", max_vertical.evaluate_at(&current_tc), 5.0));
                                        }
                                        EffectType::ChromaKey { tolerance, feather, .. } => {
                                            param_entries.push(("tolerance", "Tolerance", tolerance.evaluate_at(&current_tc), 5.0));
                                            param_entries.push(("feather", "Feather", feather.evaluate_at(&current_tc), 2.0));
                                        }
                                        EffectType::NoiseGenerator { amount, .. } => {
                                            param_entries.push(("amount", "Amount", amount.evaluate_at(&current_tc), 5.0));
                                        }
                                        // Shader Lab parameters are edited in the Properties
                                        // panel (dynamic uniforms have no static keyframe paths).
                                        EffectType::ShaderLab { .. } => {}
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
                                            "max_horizontal" => "effect:max_horizontal",
                                            "max_vertical" => "effect:max_vertical",
                                            "tolerance" => "effect:tolerance",
                                            "feather" => "effect:feather",
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

        let p_root_up = panel_entity.clone();
        let p_root_up_out = panel_entity.clone();
        let p_root_move = panel_entity.clone();
        let s_root_move = self.state.clone();

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
            .on_mouse_move(move |event, window, cx| {
                let action = p_root_move.read(cx).drag_action.clone();
                if let Some(action) = action {
                    let track_width = (window.bounds().size.width / px(1.0) - 380.0).max(200.0);
                    let pixels_per_frame = (track_width / total_frames.max(1) as f32).max(0.5);
                    let cur_x = event.position.x / px(1.0);
                    match action {
                        TimelineDragAction::SlipLayer { layer_id, initial_mouse_x, initial_in_frame, initial_out_frame } => {
                            let delta_px = cur_x - initial_mouse_x;
                            let delta_frames = (delta_px / pixels_per_frame).round() as i64;
                            let dur = initial_out_frame - initial_in_frame;
                            s_root_move.update(cx, |s, cx| {
                                if let Some(comp) = s.active_composition_mut() {
                                    let comp_dur = comp.duration.frames();
                                    let fps = comp.frame_rate;
                                    let mut target_in = initial_in_frame + delta_frames;
                                    if target_in < 0 {
                                        target_in = 0;
                                    }
                                    if target_in + dur > comp_dur {
                                        target_in = (comp_dur - dur).max(0);
                                    }
                                    if let Some(layer) = comp.get_layer_mut(&layer_id) {
                                        layer.in_point = TimeCode::from_frames(target_in, fps);
                                        layer.out_point = TimeCode::from_frames(target_in + dur, fps);
                                    }
                                }
                                cx.notify();
                            });
                        }
                        TimelineDragAction::TrimIn { layer_id, initial_mouse_x, initial_in_frame, initial_out_frame } => {
                            let delta_px = cur_x - initial_mouse_x;
                            let delta_frames = (delta_px / pixels_per_frame).round() as i64;
                            let max_in = initial_out_frame - 1;
                            let target_in = (initial_in_frame + delta_frames).max(0).min(max_in);
                            s_root_move.update(cx, |s, cx| {
                                if let Some(comp) = s.active_composition_mut() {
                                    let fps = comp.frame_rate;
                                    if let Some(layer) = comp.get_layer_mut(&layer_id) {
                                        layer.in_point = TimeCode::from_frames(target_in, fps);
                                    }
                                }
                                cx.notify();
                            });
                        }
                        TimelineDragAction::TrimOut { layer_id, initial_mouse_x, initial_in_frame, initial_out_frame } => {
                            let delta_px = cur_x - initial_mouse_x;
                            let delta_frames = (delta_px / pixels_per_frame).round() as i64;
                            let min_out = initial_in_frame + 1;
                            s_root_move.update(cx, |s, cx| {
                                if let Some(comp) = s.active_composition_mut() {
                                    let comp_dur = comp.duration.frames();
                                    let fps = comp.frame_rate;
                                    let target_out = (initial_out_frame + delta_frames).max(min_out).min(comp_dur);
                                    if let Some(layer) = comp.get_layer_mut(&layer_id) {
                                        layer.out_point = TimeCode::from_frames(target_out, fps);
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }
                }
            })
            .on_mouse_up(MouseButton::Left, move |_event, _window, cx| {
                p_root_up.update(cx, |this, cx| {
                    this.is_scrubbing_ruler = false;
                    this.drag_action = None;
                    cx.notify();
                });
            })
            .on_mouse_up_out(MouseButton::Left, move |_event, _window, cx| {
                p_root_up_out.update(cx, |this, cx| {
                    this.is_scrubbing_ruler = false;
                    this.drag_action = None;
                    cx.notify();
                });
            })
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
                        let p_ruler_down = panel_entity.clone();
                        let mut ruler_track = div()
                            .id("ruler_track")
                            .test_support()
                            .flex_1()
                            .relative()
                            .h_full()
                            .cursor_col_resize()
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                p_ruler_down.update(cx, |this, _cx| {
                                    this.is_scrubbing_ruler = true;
                                });
                            });

                        // 200 interactive scrub slices across the timeline ruler track
                        for slice_idx in 0..200 {
                            let scrub_pct = (slice_idx as f64) / 200.0;
                            let s_scrub = self.state.clone();
                            let s_scrub_move = self.state.clone();
                            let p_slice_down = panel_entity.clone();
                            let p_slice_move = panel_entity.clone();
                            let target_time = scrub_pct * total_duration_secs;
                            ruler_track = ruler_track.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(relative(scrub_pct as f32))
                                    .w(relative(1.0 / 200.0))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        p_slice_down.update(cx, |this, _cx| {
                                            this.is_scrubbing_ruler = true;
                                        });
                                        s_scrub.update(cx, |s, cx| {
                                            s.seek(target_time);
                                            cx.notify();
                                        });
                                    })
                                    .on_mouse_move(move |event, _window, cx| {
                                        if event.dragging() || p_slice_move.read(cx).is_scrubbing_ruler {
                                            s_scrub_move.update(cx, |s, cx| {
                                                s.seek(target_time);
                                                cx.notify();
                                            });
                                        }
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
            .child({
                let p_tl_ctx = panel_entity.clone();
                v_flex()
                    .id("timeline")
                    .test_support()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(timeline_rows)
                    .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                        p_tl_ctx.update(cx, |this, cx| {
                            this.open_context_menu(ContextMenuTarget::EmptyTrackArea);
                            cx.notify();
                        });
                    })
            });

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

                    let s_tin = s_menu.clone();
                    let p_tin = p_close.clone();
                    let t_tin = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_tin.update(cx, |s, cx| {
                                    let current_tc = s.clock.timecode();
                                    let _ = s.trim_layer_in_point(&t_tin, current_tc);
                                    cx.notify();
                                });
                                p_tin.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Set In Point to Current Time ([)"),
                    );

                    let s_tout = s_menu.clone();
                    let p_tout = p_close.clone();
                    let t_tout = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_tout.update(cx, |s, cx| {
                                    let current_tc = s.clock.timecode();
                                    let _ = s.trim_layer_out_point(&t_tout, current_tc);
                                    cx.notify();
                                });
                                p_tout.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Set Out Point to Current Time (])"),
                    );

                    let s_full = s_menu.clone();
                    let p_full = p_close.clone();
                    let t_full = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_full.update(cx, |s, cx| {
                                    let _ = s.reset_layer_duration_to_comp(&t_full);
                                    cx.notify();
                                });
                                p_full.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Reset Duration to Full Comp"),
                    );

                    let s_spf = s_menu.clone();
                    let p_spf = p_close.clone();
                    let t_spf = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_spf.update(cx, |s, cx| {
                                    let _ = s.slip_layer(&t_spf, 5);
                                    cx.notify();
                                });
                                p_spf.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Slip Forward 5 Frames"),
                    );

                    let s_spb = s_menu.clone();
                    let p_spb = p_close.clone();
                    let t_spb = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_spb.update(cx, |s, cx| {
                                    let _ = s.slip_layer(&t_spb, -5);
                                    cx.notify();
                                });
                                p_spb.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Slip Backward 5 Frames"),
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

                    let s_adj = s_menu.clone();
                    let p_adj = p_close.clone();
                    menu_items = menu_items.child(
                        div()
                            .id("timeline_ctx_new_adjustment")
                            .test_support()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_adj.update(cx, |s, cx| {
                                    let _ = s.add_adjustment_layer(None);
                                    cx.notify();
                                });
                                p_adj.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("New Adjustment Layer"),
                    );

                    let s_sol = s_menu.clone();
                    let p_sol = p_close.clone();
                    menu_items = menu_items.child(
                        div()
                            .id("timeline_ctx_new_solid")
                            .test_support()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_sol.update(cx, |s, cx| {
                                    let _ = s.add_solid_layer("New Solid", Color::from_rgba_u8(245, 158, 11, 255), 400, 400);
                                    cx.notify();
                                });
                                p_sol.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("New Solid Layer"),
                    );

                    let s_txt = s_menu.clone();
                    let p_txt = p_close.clone();
                    menu_items = menu_items.child(
                        div()
                            .id("timeline_ctx_new_text")
                            .test_support()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_txt.update(cx, |s, cx| {
                                    let _ = s.add_text_layer("New Text Layer", None);
                                    cx.notify();
                                });
                                p_txt.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("New Text Layer"),
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
                ContextMenuTarget::EmptyTrackArea => {
                    let s_adj = s_menu.clone();
                    let p_adj = p_close.clone();
                    menu_items = menu_items.child(
                        div()
                            .id("timeline_ctx_new_adjustment")
                            .test_support()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_adj.update(cx, |s, cx| {
                                    let _ = s.add_adjustment_layer(None);
                                    cx.notify();
                                });
                                p_adj.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("New Adjustment Layer"),
                    );

                    let s_sol = s_menu.clone();
                    let p_sol = p_close.clone();
                    menu_items = menu_items.child(
                        div()
                            .id("timeline_ctx_new_solid")
                            .test_support()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_sol.update(cx, |s, cx| {
                                    let _ = s.add_solid_layer("New Solid", Color::from_rgba_u8(245, 158, 11, 255), 400, 400);
                                    cx.notify();
                                });
                                p_sol.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("New Solid Layer"),
                    );

                    let s_txt = s_menu.clone();
                    let p_txt = p_close.clone();
                    menu_items = menu_items.child(
                        div()
                            .id("timeline_ctx_new_text")
                            .test_support()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_txt.update(cx, |s, cx| {
                                    let _ = s.add_text_layer("New Text Layer", None);
                                    cx.notify();
                                });
                                p_txt.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("New Text Layer"),
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
