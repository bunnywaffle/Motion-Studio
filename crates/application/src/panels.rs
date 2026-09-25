use gpui_kit::assets::IconName;
use gpui_kit::base::IndexPath;
use gpui_kit::component::button::Button;
#[allow(unused_imports)]
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::combobox::{Combobox, ComboboxEvent, ComboboxState};
use gpui_kit::component::Selectable;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::base::{h_flex, v_flex, ElementExt as _, Positioner, StyledExt, TestSupportExt};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use std::collections::HashSet;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::state::{EditorState, EditorTool, EasingPreset, GraphSeries};
use project::shader::{presets as shader_presets, ShaderParamValue};
use project::{BlendMode, Color, EffectType, LayerSource, ShapeType, TimeCode, TrackMatteMode, Vec2};

fn icon_box(icon: IconName) -> Div {
    div().w(px(14.)).h(px(14.)).flex().items_center().justify_center().child(icon)
}

/// After Effects workspace palette (dark flat chrome sampled from the AE
/// reference mockup). Used for panel chrome so every surface matches even
/// though the base kit theme stays untouched.
pub mod ae {
    use super::*;

    /// Deepest app background (timeline lanes, canvas letterbox).
    pub fn bg() -> Rgba {
        rgb(0x1b1b1b)
    }
    /// Raised panel surfaces (cards, headers, side rails).
    pub fn panel() -> Rgba {
        rgb(0x232323)
    }
    /// Controls / pills / wells.
    pub fn control() -> Rgba {
        rgb(0x2e2e2e)
    }
    /// Hover highlight.
    pub fn hover() -> Rgba {
        rgb(0x3a3a3a)
    }
    /// Hairline borders.
    pub fn border() -> Rgba {
        rgb(0x101010)
    }
    /// Primary text.
    pub fn text() -> Rgba {
        rgb(0xd7d7d7)
    }
    /// Secondary / header text.
    pub fn dim() -> Rgba {
        rgb(0x9a9a9a)
    }
    /// AE selection blue (active tools, spans, toggles).
    pub fn accent() -> Rgba {
        rgb(0x2f7cf6)
    }
    /// Timeline layer span blue.
    pub fn span() -> Rgba {
        rgb(0x2b6cb0)
    }
    /// Parent badge / success green.
    pub fn green() -> Rgba {
        rgb(0x2f9e44)
    }
    /// Timecode readout blue.
    pub fn timecode() -> Rgba {
        rgb(0x4da3ff)
    }
    /// Keyframe diamond amber.
    pub fn amber() -> Rgba {
        rgb(0xf5a623)
    }

    /// Small-caps dim section header: `▾ TITLE ......... count`.
    pub fn section_header(title: &str, count: Option<usize>) -> Div {
        let mut row = h_flex()
            .px_2()
            .py_1()
            .mt_1()
            .items_center()
            .gap_1p5()
            .text_xs()
            .font_semibold()
            .text_color(dim());
        row = row.child(div().child("▾")).child(title.to_string());
        if let Some(n) = count {
            row = row.child(
                div()
                    .px_1p5()
                    .rounded_sm()
                    .bg(control())
                    .text_color(dim())
                    .child(format!("{n}")),
            );
        }
        row
    }
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

/// Viewport gizmo handle dot, centered on canvas-space `(x, y)`.
/// Note: no `.test_support()` wrapper here — it would hide the concrete
/// `Stateful<Div>` type that callers extend with children and handlers.
fn gizmo_dot(id: String, x: f32, y: f32, size: f32, fill: Rgba, border: Rgba, round: bool) -> Stateful<Div> {
    let mut d = div()
        .id(SharedString::from(id))
        .absolute()
        .left(px(x - size / 2.0))
        .top(px(y - size / 2.0))
        .w(px(size))
        .h(px(size))
        .bg(fill)
        .border_1()
        .border_color(border)
        .cursor_pointer();
    d = if round { d.rounded_full() } else { d.rounded_sm() };
    d
}

/// Window px -> composition px for gizmo drag starts (measured frame
/// origin + uniform fit; no hardcoded factors, no missing offsets).
fn gizmo_to_comp(
    mx: f32,
    my: f32,
    frame: Option<(f32, f32)>,
    fit: f32,
    cw: f32,
    ch: f32,
) -> (f32, f32) {
    let (fox, foy) = frame.unwrap_or((0.0, 0.0));
    (
        (mx - fox) / fit - cw / 2.0,
        (my - foy) / fit - ch / 2.0,
    )
}

/// Frame origin with a centering fallback for pre-measure frames: the
/// wrap centers content, so origin = wrap origin + (wrap - canvas) / 2.
fn frame_origin_or_center(
    measured: Option<(f32, f32)>,
    viewport_px: Option<(f32, f32)>,
    viewport_origin: Option<(f32, f32)>,
    canvas_px: Option<(f32, f32)>,
) -> Option<(f32, f32)> {
    if measured.is_some() {
        return measured;
    }
    match (viewport_px, viewport_origin, canvas_px) {
        (Some((vw, vh)), Some((ox, oy)), Some((cw, ch))) => {
            Some((ox + (vw - cw) / 2.0, oy + (vh - ch) / 2.0))
        }
        _ => None,
    }
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
    /// Window-space cursor position where the context menu was requested.
    /// Rendered through `deferred` + `Positioner` so the menu opens exactly
    /// under the mouse instead of at a fixed corner.
    pub menu_pos: Option<Point<Pixels>>,
    pub filter: ProjectFilterType,
    pub sort_mode: ProjectSortMode,
    /// "New Composition" dialog state (After Effects-style).
    pub show_new_comp: bool,
    pub nc_w: u32,
    pub nc_h: u32,
    pub nc_fps: f64,
    pub nc_dur: f64,
    /// 0 = Black, 1 = White, 2 = Transparent.
    pub nc_bg: u8,
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
            menu_pos: None,
            filter: ProjectFilterType::All,
            sort_mode: ProjectSortMode::Name,
            show_new_comp: false,
            nc_w: 1920,
            nc_h: 1080,
            nc_fps: 30.0,
            nc_dur: 10.0,
            nc_bg: 0,
        }
    }

    pub fn standalone(cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new(state, cx)
    }

    pub fn open_context_menu(&mut self, target: ProjectContextMenuTarget, pos: Point<Pixels>) {
        self.context_menu = Some(target);
        self.menu_pos = Some(pos);
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
        self.menu_pos = None;
    }

    /// After Effects-style "New Composition" dialog: size presets (aspect
    /// ratio), frame rate, duration, and background color. The composition
    /// becomes active on Create.
    fn render_new_comp_dialog(&self, panel: &Entity<ProjectPanel>, cx: &App) -> impl IntoElement {
        let sizes: &[(u32, u32)] = &[(1920, 1080), (1280, 720), (1080, 1080), (1080, 1920), (3840, 2160)];
        let fps_opts: &[f64] = &[24.0, 25.0, 30.0, 60.0];
        let dur_opts: &[f64] = &[5.0, 10.0, 30.0];
        let bg_opts: &[(u8, &str)] = &[(0, "Black"), (1, "White"), (2, "Transparent")];

        let mut size_row = h_flex().gap_1().items_center().flex_wrap();
        for (w, h) in sizes {
            let (w, h) = (*w, *h);
            let sel = self.nc_w == w && self.nc_h == h;
            let p_pick = panel.clone();
            size_row = size_row.child(
                div()
                    .id(SharedString::from(format!("nc_size_{w}x{h}")))
                    .test_support()
                    .cursor_pointer()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(if sel { cx.theme().primary } else { cx.theme().muted })
                    .text_color(if sel { cx.theme().primary_foreground } else { cx.theme().foreground })
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .text_xs()
                    .child(format!("{w}×{h}"))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_pick.update(cx, |this, cx| {
                            this.nc_w = w;
                            this.nc_h = h;
                            cx.notify();
                        });
                    }),
            );
        }

        let mut fps_row = h_flex().gap_1().items_center().flex_wrap();
        for f in fps_opts {
            let f = *f;
            let sel = (self.nc_fps - f).abs() < 1e-6;
            let p_pick = panel.clone();
            fps_row = fps_row.child(
                div()
                    .id(SharedString::from(format!("nc_fps_{f}")))
                    .test_support()
                    .cursor_pointer()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(if sel { cx.theme().primary } else { cx.theme().muted })
                    .text_color(if sel { cx.theme().primary_foreground } else { cx.theme().foreground })
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .text_xs()
                    .child(format!("{f:.0} fps"))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_pick.update(cx, |this, cx| {
                            this.nc_fps = f;
                            cx.notify();
                        });
                    }),
            );
        }

        let mut dur_row = h_flex().gap_1().items_center().flex_wrap();
        for d in dur_opts {
            let d = *d;
            let sel = (self.nc_dur - d).abs() < 1e-6;
            let p_pick = panel.clone();
            dur_row = dur_row.child(
                div()
                    .id(SharedString::from(format!("nc_dur_{d}")))
                    .test_support()
                    .cursor_pointer()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(if sel { cx.theme().primary } else { cx.theme().muted })
                    .text_color(if sel { cx.theme().primary_foreground } else { cx.theme().foreground })
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .text_xs()
                    .child(format!("{d:.0}s"))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_pick.update(cx, |this, cx| {
                            this.nc_dur = d;
                            cx.notify();
                        });
                    }),
            );
        }

        let mut bg_row = h_flex().gap_1().items_center().flex_wrap();
        for (b, label) in bg_opts {
            let (b, label) = (*b, *label);
            let sel = self.nc_bg == b;
            let p_pick = panel.clone();
            bg_row = bg_row.child(
                div()
                    .id(SharedString::from(format!("nc_bg_{b}")))
                    .test_support()
                    .cursor_pointer()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(if sel { cx.theme().primary } else { cx.theme().muted })
                    .text_color(if sel { cx.theme().primary_foreground } else { cx.theme().foreground })
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .text_xs()
                    .child(label)
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_pick.update(cx, |this, cx| {
                            this.nc_bg = b;
                            cx.notify();
                        });
                    }),
            );
        }

        let bg_name = match self.nc_bg {
            1 => "White",
            2 => "Transparent",
            _ => "Black",
        };
        let summary = format!(
            "{}×{} • {:.2} fps • {:.0}s • {bg_name}",
            self.nc_w, self.nc_h, self.nc_fps, self.nc_dur
        );

        let p_create = panel.clone();
        let s_create = self.state.clone();
        let (cw, ch, cfps, cdur, cbg) = (self.nc_w, self.nc_h, self.nc_fps, self.nc_dur, self.nc_bg);
        let p_cancel = panel.clone();

        div()
            .id("new_comp_dialog")
            .test_support()
            .absolute()
            .top(px(48.))
            .left(px(16.))
            .right(px(16.))
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .shadow_lg()
            .p_3()
            .child(
                v_flex()
                    .gap_2()
                    .child(div().font_semibold().text_sm().child("New Composition"))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Frame Size (aspect ratio)"))
                            .child(size_row),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Frame Rate"))
                            .child(fps_row),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Duration"))
                            .child(dur_row),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_color(cx.theme().muted_foreground).text_xs().child("Background"))
                            .child(bg_row),
                    )
                    .child(div().text_xs().text_color(cx.theme().primary).child(summary))
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(
                                div()
                                    .id("nc_create")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(cx.theme().primary)
                                    .text_color(cx.theme().primary_foreground)
                                    .text_xs()
                                    .font_semibold()
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        let bg = match cbg {
                                            1 => Color::WHITE,
                                            2 => Color::TRANSPARENT,
                                            _ => Color::BLACK,
                                        };
                                        s_create.update(cx, |s, cx| {
                                            let _ = s.add_composition("", cw, ch, cfps, cdur, bg);
                                            cx.notify();
                                        });
                                        p_create.update(cx, |this, cx| {
                                            this.show_new_comp = false;
                                            cx.notify();
                                        });
                                    })
                                    .child("Create"),
                            )
                            .child(
                                div()
                                    .id("nc_cancel")
                                    .test_support()
                                    .cursor_pointer()
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(cx.theme().muted)
                                    .text_color(cx.theme().muted_foreground)
                                    .text_xs()
                                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        p_cancel.update(cx, |this, cx| {
                                            this.show_new_comp = false;
                                            cx.notify();
                                        });
                                    })
                                    .child("Cancel"),
                            ),
                    ),
            )
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
                bin_items.push(ae::section_header("COMPOSITIONS", Some(1)).into_any_element());

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
                    ae::section_header("PROJECT MEDIA", Some(state.project.assets.len()))
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
                                .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                    let pos = event.position;
                                    p_asset_rclick.update(cx, |this, cx| {
                                        this.open_context_menu(ProjectContextMenuTarget::Asset(aid_rclick.clone()), pos);
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
                            .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                let pos = event.position;
                                p_solid_rclick.update(cx, |this, cx| {
                                    this.open_context_menu(ProjectContextMenuTarget::Solid(sid_rclick.clone()), pos);
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
                .overflow_hidden()
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
                        .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                            let pos = event.position;
                            p_bin_rclick.update(cx, |this, cx| {
                                this.open_context_menu(ProjectContextMenuTarget::BinBackground, pos);
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
                        let p_comp = p_close.clone();
                        menu_items = menu_items.child(
                            div()
                                .id("proj_ctx_new_composition")
                                .test_support()
                                .cursor_pointer()
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .text_xs()
                                .font_semibold()
                                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    p_comp.update(cx, |this, cx| {
                                        this.show_new_comp = true;
                                        this.close_context_menu();
                                        cx.notify();
                                    });
                                })
                                .child("New Composition..."),
                        );
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

                // Cursor-anchored: opens exactly under the mouse (viewport
                // clamped), not at a fixed corner.
                let menu_pos = self.menu_pos.unwrap_or(point(px(20.), px(40.)));
                let menu_box = div()
                    .id("project_context_menu")
                    .test_support()
                    .w(px(200.))
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .shadow_lg()
                    .child(menu_items);
                let overlay = deferred(
                    Positioner::corner(Anchor::TopLeft, menu_pos)
                        .margin(px(8.))
                        .occlude()
                        .child(menu_box),
                );

                root = root.child(overlay);
            }

            if self.show_new_comp {
                let panel_self = cx.entity().clone();
                root = root.child(self.render_new_comp_dialog(&panel_self, cx));
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
    /// Window-space cursor position for the context menu (cursor-anchored).
    pub menu_pos: Option<Point<Pixels>>,
    pub is_dragging_canvas: bool,
    pub last_canvas_mouse: Option<(f32, f32)>,
    /// True when the current left-press began on a layer body (set by the
    /// layer shell, read by the canvas frame): only then may the Move /
    /// Rotate tools start a canvas drag. Prevents empty-space clicks from
    /// nudging the selection.
    pub down_on_layer: bool,
    /// Window-space position of an empty-canvas press (click = deselect).
    pub empty_down: Option<(f32, f32)>,
    /// Remembered rectangle/ellipse variant for the grouped Shape tool.
    pub shape_variant: EditorTool,
    /// Measured canvas-wrap size in window px (responsive fit).
    pub viewport_px: Option<(f32, f32)>,
    /// Measured canvas-wrap origin in window px (for cursor mapping).
    pub viewport_origin: Option<(f32, f32)>,
    /// Measured canvas-frame origin in window px (exact picking origin).
    pub frame_origin: Option<(f32, f32)>,
    /// Active transform-gizmo drag (rotate / scale / anchor handles).
    pub gizmo_drag: Option<ViewerGizmoDrag>,
    /// Active mask node/handle drag (viewport Path Editor).
    pub mask_drag: Option<MaskDrag>,
    /// Selected mask node index for handle display (viewport Path Editor).
    pub mask_edit_point: Option<usize>,
    /// True once the current mask drag moved (click without drag cycles
    /// the node kind instead).
    pub mask_down_moved: bool,
    /// Per-layer CPU raster cache (layer id -> last raster).
    pub raster_cache: HashMap<String, crate::raster::RasterEntry>,
    /// Decoded gpui render images by (layer id, raster key). Pointer
    /// keys are unsafe here (freed Arcs reuse addresses and would serve
    /// stale frames); the content key cannot collide without the raster
    /// itself colliding.
    pub img_cache: HashMap<(String, u64), std::sync::Arc<gpui::RenderImage>>,
    /// Decoded image asset cache (asset id -> RGBA).
    pub asset_cache: HashMap<String, Arc<image::RgbaImage>>,
    /// Decoded image dimensions cache (asset id -> w/h). `image_dimensions`
    /// hits the disk, so without this every viewport render re-reads every
    /// image file — visible as gizmo/effect lag.
    pub img_dims: HashMap<String, (u32, u32)>,
    /// Last fitted canvas size, for cursor mapping before measure.
    pub canvas_px: Option<(f32, f32)>,
    /// Last viewer build time in ms (status bar readout).
    pub last_frame_ms: f32,
    /// Evaluated layer boxes, topmost-first, for deterministic viewport
    /// picking (independent of sibling hit-test order).
    pub pick_boxes: Vec<PickBox>,
}

/// Evaluated world-space AABB of one layer for viewport picking (comp px).
#[derive(Clone, Debug)]
pub struct PickBox {
    pub id: String,
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

/// What part of a mask node is being dragged in the Path Editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskDragKind {
    Point,
    InHandle,
    OutHandle,
}

/// Active mask node/handle drag in the viewport Path Editor.
#[derive(Clone, Debug)]
pub struct MaskDrag {
    pub layer_id: String,
    pub mask_id: String,
    pub index: usize,
    pub kind: MaskDragKind,
}

/// Viewport transform-gizmo drag state (After Effects-style direct
/// manipulation: move the body, drag corners/edges to scale, the top
/// handle to rotate about the pivot, the diamond to move the pivot).
#[derive(Clone, Debug)]
pub enum ViewerGizmoDrag {    Rotate {
        layer_id: String,
        start_rot: f32,
        start_angle: f32,
    },
    Scale {
        layer_id: String,
        uniform: bool,
        use_x: bool,
        use_y: bool,
        start_scale: Vec2,
        start_local: Vec2,
        anchor_local: Vec2,
    },
    Anchor {
        layer_id: String,
        start_local: Vec2,
    },
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
            menu_pos: None,
            is_dragging_canvas: false,
            last_canvas_mouse: None,
            down_on_layer: false,
            empty_down: None,
            shape_variant: EditorTool::ShapeRect,
            viewport_px: None,
            viewport_origin: None,
            frame_origin: None,
            gizmo_drag: None,
            mask_drag: None,
            mask_edit_point: None,
            mask_down_moved: false,
            raster_cache: HashMap::new(),
            img_cache: HashMap::new(),
            asset_cache: HashMap::new(),
            img_dims: HashMap::new(),
            canvas_px: None,
            last_frame_ms: 0.0,
            pick_boxes: Vec::new(),
        }
    }

    pub fn open_context_menu(&mut self, target: ViewerContextMenuTarget, pos: Point<Pixels>) {
        self.context_menu = Some(target);
        self.menu_pos = Some(pos);
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
        self.menu_pos = None;
    }

    /// Topmost layer id whose evaluated box contains comp-space `(x, y)`.
    /// `pick_boxes` is stored topmost-first, so the first hit wins —
    /// deterministic regardless of sibling hit-test order.
    pub fn pick_top_at(&self, x: f32, y: f32) -> Option<String> {
        self.pick_boxes
            .iter()
            .find(|b| x >= b.min_x && x <= b.max_x && y >= b.min_y && y <= b.max_y)
            .map(|b| b.id.clone())
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
        let render_t0 = std::time::Instant::now();
        let state = self.state.read(cx);
        let comp_opt = state.active_composition();
        let eval_stack = state.evaluate_current_frame().ok();

        let comp_name = comp_opt.map(|c| c.name.clone()).unwrap_or_else(|| "No Comp".to_string());
        let comp_res = comp_opt.map(|c| format!("{} x {} (1.00)", c.width, c.height)).unwrap_or_default();
        let comp_fps = comp_opt.map(|c| format!("{:.2} fps", c.frame_rate)).unwrap_or_default();
        let _current_tc = format!("{}", state.clock.timecode());
        let current_frame = state.clock.current_frame();

        let bg_color = comp_opt
            .map(|c| Rgba { r: c.background_color.r, g: c.background_color.g, b: c.background_color.b, a: c.background_color.a })
            .unwrap_or(Rgba { r: 0.07, g: 0.07, b: 0.08, a: 1.0 });

        // Responsive canvas: fit the composition into the measured wrap
        // box with a uniform scale (aspect preserved — no stretch on
        // portrait/square comps), with letterboxing. Falls back to the
        // classic 512x288 fit before the first measurement lands.
        let comp_w = comp_opt.map(|c| c.width as f32).unwrap_or(1920.0);
        let comp_h = comp_opt.map(|c| c.height as f32).unwrap_or(1080.0);
        let (avail_w, avail_h) = self.viewport_px.unwrap_or((512.0, 288.0));
        let fit = ((avail_w - 32.0) / comp_w)
            .min((avail_h - 32.0) / comp_h)
            .clamp(0.05, 4.0);
        let canvas_w = (comp_w * fit).max(2.0);
        let canvas_h = (comp_h * fit).max(2.0);
        // Uniform canvas scale (comp px -> canvas px).
        let scale_x = fit;
        let scale_y = fit;
        self.canvas_px = Some((canvas_w, canvas_h));
        let frame_org = frame_origin_or_center(
            self.frame_origin,
            self.viewport_px,
            self.viewport_origin,
            self.canvas_px,
        );

        // Render evaluated layers in painter's composite order
        let (rendered_layers, gizmo_els): (Vec<AnyElement>, Vec<AnyElement>) = match eval_stack.as_ref() {
            Some(stack) => {
                let mut elements = Vec::new();
                // Transform-gizmo overlays live in canvas space (NOT inside
                // layer shells, whose local origin would misplace them).
                let mut gizmo_els: Vec<AnyElement> = Vec::new();
                let mut full_frame_backdrop = Color::rgba(
                    bg_color.r, bg_color.g, bg_color.b, bg_color.a,
                );
                let mut rendered_regions: Vec<(f32, f32, f32, f32, Color)> = Vec::new();
                let mut pick_list: Vec<PickBox> = Vec::new();

                for layer in stack.render_layers() {
                    let is_adjustment = matches!(&layer.source, LayerSource::Adjustment);

                    // Base content dims mirror the rasterizer estimate so
                    // pivots, bounds, and pixels stay consistent.
                    let (base_w, base_h) = match &layer.source {
                        LayerSource::Solid { width, height, .. } => {
                            (*width as f32, *height as f32)
                        }
                        LayerSource::Image { asset_id } => {
                            if let Some(asset) = state.project.get_asset(asset_id) {
                                // Cached: image_dimensions hits the disk, and
                                // this runs for every image layer per render.
                                let (dim_w, dim_h) = match self.img_dims.get(asset_id) {
                                    Some(&d) => d,
                                    None => {
                                        let d = image::image_dimensions(&asset.path)
                                            .unwrap_or((1920, 1080));
                                        self.img_dims.insert(asset_id.clone(), d);
                                        d
                                    }
                                };
                                (dim_w as f32, dim_h as f32)
                            } else {
                                (400.0, 300.0)
                            }
                        }
                        LayerSource::Video { .. } => (1920.0, 1080.0),
                        LayerSource::Text { font_size, text, text_path, .. } => {
                            if let Some(path) = text_path {
                                if let Some((mn, mx)) = path.bounds() {
                                    let pad = font_size.value * 1.5 + 16.0;
                                    let x0 = mn.x.min(0.0) - pad;
                                    let y0 = mn.y.min(0.0) - pad;
                                    let x1 = mx.x.max(0.0) + pad;
                                    let y1 = mx.y.max(0.0) + pad;
                                    ((x1 - x0).max(64.0), (y1 - y0).max(64.0))
                                } else {
                                    (400.0, 100.0)
                                }
                            } else {
                                let len = text.value.chars().count().max(1) as f32;
                                let fs = font_size.value;
                                let estimated_w = (len * fs * 0.6 + 40.0).max(100.0);
                                let estimated_h = (fs * 1.4 + 20.0).max(40.0);
                                (estimated_w, estimated_h)
                            }
                        }
                        LayerSource::Shape { shape_type } => match shape_type {
                            ShapeType::Rectangle { width, height, .. } => {
                                (width.value, height.value)
                            }
                            ShapeType::Ellipse { radius_x, radius_y, .. } => {
                                (radius_x.value * 2.0, radius_y.value * 2.0)
                            }
                            ShapeType::Path { path_data, .. } => {
                                // Frame-aware size (see `Path::frame`): pen
                                // paths live in arbitrary local coords.
                                match project::Path::from_svg(path_data).frame(8.0) {
                                    Some((_, size)) => (size.x, size.y),
                                    None => (400.0, 300.0),
                                }
                            }
                        },
                        LayerSource::Adjustment => (comp_w, comp_h),
                        _ => (400.0, 300.0),
                    };

                    // World box from the layer's local content box. Pen/path
                    // shapes use their raster frame (arbitrary local coords),
                    // everything else spans (0, 0, base).
                    let bbox = match &layer.source {
                        LayerSource::Shape {
                            shape_type: ShapeType::Path { path_data, .. },
                        } => match project::Path::from_svg(path_data).frame(8.0) {
                            Some((origin, size)) => layer.local_to_world_bbox(
                                &compositor::BoundingBox2D::from_origin_size(origin, size),
                            ),
                            None => layer.world_bounds(base_w, base_h),
                        },
                        _ => layer.world_bounds(base_w, base_h),
                    };
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

                    // CPU raster viewport: each layer becomes an AABB-sized
                    // true-color pixmap (rotation, spatial effects, and text
                    // all resolve per-pixel), shown through `gpui::img`.
                    // Adjustment layers render nothing themselves; their
                    // effects fold into the layers beneath via the shared
                    // adjustment stack inside the rasterizer.
                    let comp_fps = comp_opt.map(|c| c.frame_rate as f32).unwrap_or(30.0);
                    let time_s = current_frame as f32 / comp_fps.max(1.0);
                    let duration_s = comp_opt.map(|c| c.duration_seconds() as f32).unwrap_or(0.0);
                    // Gestures preview fast (probe wash); release restores
                    // full per-pixel quality via the cache key below.
                    let playing_now = state.is_playing || state.preview_fast;
                    // Raster output size = AABB box, capped for speed (the
                    // img child stretches to the shell on cap). Halved
                    // during gestures/playback and per the View > Preview
                    // Quality preference when idle. The cache key covers
                    // size + quality flag, so previews never poison
                    // full-quality entries.
                    let qdiv = state.preview_divisor().max(1);
                    let (rw, rh) = (
                        ((l_w / qdiv as f32).ceil().max(1.0) as u32).min(1024),
                        ((l_h / qdiv as f32).ceil().max(1.0) as u32).min(1024),
                    );
                    // Decode image assets once into the shared cache.
                    if let LayerSource::Image { asset_id } = &layer.source {
                        if let Some(asset) = state.project.get_asset(asset_id) {
                            let path = asset.path.clone();
                            crate::raster::decoded_asset(&mut self.asset_cache, asset_id, &path);
                        }
                    }
                    let cache_key = crate::raster::layer_cache_key(
                        layer,
                        current_frame,
                        rw,
                        rh,
                        playing_now,
                    );
                    // Exotic blend modes sample the backdrop average under
                    // the box, which moves with translation — the
                    // translation-stable key would go stale, so those
                    // layers always re-rasterize.
                    let cacheable = layer.blend_mode == BlendMode::Normal;
                    let entry = match self.raster_cache.get(&layer.id) {
                        Some(e) if cacheable && e.key == cache_key && e.w == rw && e.h == rh => e.clone(),
                        _ => {
                            let (buf, avg, empty) = crate::raster::rasterize_layer(
                                layer,
                                base_w,
                                base_h,
                                rw,
                                rh,
                                comp_w,
                                comp_h,
                                sampled_backdrop,
                                time_s,
                                current_frame,
                                playing_now,
                                duration_s,
                                &self.asset_cache,
                            );
                            let bgra = std::sync::Arc::new(buf.to_bgra8());
                            let e = crate::raster::RasterEntry {
                                key: cache_key,
                                bgra,
                                w: rw,
                                h: rh,
                                avg,
                                empty,
                            };
                            self.raster_cache.insert(layer.id.clone(), e.clone());
                            // Bound memory: BGRA payloads are ~4MB at full
                            // res, so the cap is tighter than the PNG days.
                            if self.raster_cache.len() > 48 {
                                self.raster_cache.clear();
                                self.img_cache.clear();
                                self.raster_cache.insert(layer.id.clone(), e.clone());
                            }
                            e
                        }
                    };
                    let recorded_color = if entry.empty {
                        sampled_backdrop
                    } else {
                        entry.avg
                    };

                    rendered_regions.push((bbox.min.x, bbox.min.y, bbox.max.x, bbox.max.y, recorded_color));
                    pick_list.push(PickBox {
                        id: layer.id.clone(),
                        min_x: bbox.min.x,
                        min_y: bbox.min.y,
                        max_x: bbox.max.x,
                        max_y: bbox.max.y,
                    });


                    let covers_canvas = bbox.min.x <= -comp_w / 2.0
                        && bbox.min.y <= -comp_h / 2.0
                        && bbox.max.x >= comp_w / 2.0
                        && bbox.max.y >= comp_h / 2.0;
                    if covers_canvas {
                        full_frame_backdrop = recorded_color;
                    }


                    let p_drag_layer = cx.entity().clone();
                    let sel_state = self.state.clone();
                    let lid = layer.id.clone();

                    let p_menu = cx.entity().clone();
                    let lid_menu = layer.id.clone();
                    // Transparent hit shell: the CPU raster draws the pixels
                    // (child img); the shell keeps AABB position, selection,
                    // and mouse interaction.
                    let mut layer_el = div()
                        .id(ElementId::Name(format!("canvas_layer_{}", layer.id).into()))
                        .test_support()
                        .absolute()
                        .left(px(l_x))
                        .top(px(l_y))
                        .w(px(l_w))
                        .h(px(l_h))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            // Mark press-on-layer FIRST (bubbles to the
                            // canvas frame, which gates drag start on it).
                            // Drag state itself starts there so empty-space
                            // presses never move the selection.
                            p_drag_layer.update(cx, |this, _cx| {
                                this.down_on_layer = true;
                                this.empty_down = None;
                            });
                            sel_state.update(cx, |s, cx| {
                                s.select_layer(Some(lid.clone()));
                                cx.notify();
                            });
                        })
                        .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                            let pos = event.position;
                            p_menu.update(cx, |this, cx| {
                                this.open_context_menu(ViewerContextMenuTarget::Layer(lid_menu.clone()), pos);
                                cx.notify();
                            });
                        });

                    // Raster pixels (CPU compositor output for this layer).
                    // Decoded render images are cached per unique payload
                    // and built straight from BGRA bytes: no PNG encode,
                    // no content hashing, no async decode pop-in — the
                    // synchronous `Render` path presents the same tick.
                    if !entry.empty {
                        let img_key = (layer.id.clone(), cache_key);
                        let img = match self.img_cache.get(&img_key) {
                            Some(im) => im.clone(),
                            None => {
                                let frame = image::Frame::new(
                                    image::RgbaImage::from_raw(entry.w, entry.h, (*entry.bgra).clone())
                                        .unwrap_or_else(|| image::RgbaImage::new(entry.w, entry.h)),
                                );
                                let im = std::sync::Arc::new(gpui::RenderImage::new(vec![frame]));
                                self.img_cache.insert(img_key, im.clone());
                                if self.img_cache.len() > 48 {
                                    self.img_cache.clear();
                                }
                                im
                            }
                        };
                        layer_el = layer_el.child(
                            gpui::img(img).w(px(l_w)).h(px(l_h)),
                        );
                    }

                    if is_adjustment {
                        // After Effects behavior: an adjustment layer draws no
                        // pixels of its own — its effects post-process the
                        // composite beneath it (see the rasterizer). Render
                        // only the dashed extent outline plus a status label.
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

                    // Transform gizmo for the selected layer (After Effects
                    // style): corner/edge scale handles, a rotate handle
                    // above the top edge (rotates about the pivot), and an
                    // amber pivot diamond (drag to move the anchor point
                    // without moving pixels). All math runs in composition
                    // px through the evaluated world matrix, so rotation,
                    // parenting, and off-center anchors stay exact.
                    if is_selected && !is_adjustment && layer.is_visible {
                        let giz_panel = cx.entity().clone();
                        let giz_state = self.state.clone();
                        let giz_lid = layer.id.clone();
                        let giz_fit = scale_x;
                        let giz_cw = comp_w;
                        let giz_ch = comp_h;
                        let giz_frame = frame_org;
                        let link_uniform = state
                            .active_composition()
                            .and_then(|c| c.get_layer(&layer.id))
                            .map(|l| l.transform.scale_uniform)
                            .unwrap_or(true);
                        let w2c = |p: Vec2| {
                            let w = layer.local_to_world_point(p);
                            (
                                (w.x + giz_cw / 2.0) * giz_fit,
                                (w.y + giz_ch / 2.0) * giz_fit,
                            )
                        };
                        // Window px -> composition px for drag starts.
                        let mid = |a: (f32, f32), b: (f32, f32)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                        let c00 = w2c(Vec2::new(0.0, 0.0));
                        let c10 = w2c(Vec2::new(base_w, 0.0));
                        let c11 = w2c(Vec2::new(base_w, base_h));
                        let c01 = w2c(Vec2::new(0.0, base_h));
                        let anchor_l = layer.transform.anchor_point;
                        let anchor_c = w2c(anchor_l);
                        let accent = rgb(0x3b82f6);
                        let white = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

                        // Window px -> composition px for drag starts (fresh
                        // copies per handle: every mouse handler owns its own).
                        // Snapshot the evaluated layer for drag-start math.
                        // Rc-shared: every handle captures it without moves.
                        let snap_layer: Rc<
                            dyn Fn(&mut App) -> Option<compositor::EvaluatedLayer>,
                        > = Rc::new({
                            let giz_state = giz_state.clone();
                            let giz_lid = giz_lid.clone();
                            move |cx: &mut App| {
                                giz_state
                                    .read(cx)
                                    .evaluate_current_frame()
                                    .ok()
                                    .and_then(|stack| stack.get_layer(&giz_lid).cloned())
                            }
                        });

                        // --- Scale handles: 4 corners (both axes) + 4 edges.
                        let corners = [("nw", c00), ("ne", c10), ("se", c11), ("sw", c01)];
                        for (tag, pos) in corners {
                            let p_h = giz_panel.clone();
                            let s_h = giz_state.clone();
                            let lid_h = giz_lid.clone();
                            let snap_h = snap_layer.clone();
                            let uni = link_uniform;
                            let (h_frame, h_fit, h_cw, h_ch) =
                                (giz_frame, giz_fit, giz_cw, giz_ch);
                            gizmo_els.push(
                                gizmo_dot(
                                    format!("gizmo_scale_{tag}_{}", giz_lid),
                                    pos.0,
                                    pos.1,
                                    12.0,
                                    white,
                                    accent,
                                    false,
                                )
                                .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                    let (mx, my) = (event.position.x / px(1.0), event.position.y / px(1.0));
                                    s_h.update(cx, |s, cx| {
                                        // Gizmo gesture start: checkpoint before select+drag.
                                        s.checkpoint();
                                        s.select_layer(Some(lid_h.clone()));
                                        s.preview_fast = true;
                                        cx.notify();
                                    });
                                    let (cmx, cmy) = gizmo_to_comp(mx, my, h_frame, h_fit, h_cw, h_ch);
                                    if let Some(lay) = snap_h(cx) {
                                        if let Some(loc) = lay.world_to_local_point(Vec2::new(cmx, cmy)) {
                                            let a = lay.transform.anchor_point;
                                            let sc = lay.transform.scale;
                                            p_h.update(cx, |this, cx| {
                                                this.gizmo_drag = Some(ViewerGizmoDrag::Scale {
                                                    layer_id: lid_h.clone(),
                                                    uniform: uni,
                                                    use_x: true,
                                                    use_y: true,
                                                    start_scale: sc,
                                                    start_local: loc,
                                                    anchor_local: a,
                                                });
                                                cx.notify();
                                            });
                                        }
                                    }
                                }).into_any_element());
                        }
                        let edges = [
                            ("n", mid(c00, c10), false, true),
                            ("s", mid(c11, c01), false, true),
                            ("w", mid(c00, c01), true, false),
                            ("e", mid(c10, c11), true, false),
                        ];
                        for (tag, pos, ux, uy) in edges {
                            let p_h = giz_panel.clone();
                            let s_h = giz_state.clone();
                            let lid_h = giz_lid.clone();
                            let snap_h = snap_layer.clone();
                            let uni = link_uniform;
                            let (h_frame, h_fit, h_cw, h_ch) =
                                (giz_frame, giz_fit, giz_cw, giz_ch);
                            gizmo_els.push(
                                gizmo_dot(
                                    format!("gizmo_scale_{tag}_{}", giz_lid),
                                    pos.0,
                                    pos.1,
                                    10.0,
                                    white,
                                    accent,
                                    true,
                                )
                                .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                    let (mx, my) = (event.position.x / px(1.0), event.position.y / px(1.0));
                                    s_h.update(cx, |s, cx| {
                                        // Gizmo gesture start: checkpoint before select+drag.
                                        s.checkpoint();
                                        s.select_layer(Some(lid_h.clone()));
                                        s.preview_fast = true;
                                        cx.notify();
                                    });
                                    let (cmx, cmy) = gizmo_to_comp(mx, my, h_frame, h_fit, h_cw, h_ch);
                                    if let Some(lay) = snap_h(cx) {
                                        if let Some(loc) = lay.world_to_local_point(Vec2::new(cmx, cmy)) {
                                            let a = lay.transform.anchor_point;
                                            let sc = lay.transform.scale;
                                            p_h.update(cx, |this, cx| {
                                                this.gizmo_drag = Some(ViewerGizmoDrag::Scale {
                                                    layer_id: lid_h.clone(),
                                                    uniform: uni && !(ux ^ uy),
                                                    use_x: ux,
                                                    use_y: uy,
                                                    start_scale: sc,
                                                    start_local: loc,
                                                    anchor_local: a,
                                                });
                                                cx.notify();
                                            });
                                        }
                                    }
                                }).into_any_element());
                        }

                        // --- Rotate handle above the top edge.
                        {
                            let top_mid = mid(c00, c10);
                            let center = mid(c00, c11);
                            let mut tx = c10.0 - c00.0;
                            let mut ty = c10.1 - c00.1;
                            let len = (tx * tx + ty * ty).sqrt().max(1.0);
                            tx /= len;
                            ty /= len;
                            let mut nx = -ty;
                            let mut ny = tx;
                            if nx * (top_mid.0 - center.0) + ny * (top_mid.1 - center.1) < 0.0 {
                                nx = -nx;
                                ny = -ny;
                            }
                            let rp = (top_mid.0 + nx * 26.0, top_mid.1 + ny * 26.0);
                            let p_h = giz_panel.clone();
                            let s_h = giz_state.clone();
                            let lid_h = giz_lid.clone();
                            let snap_h = snap_layer.clone();
                            let (h_frame, h_fit, h_cw, h_ch) =
                                (giz_frame, giz_fit, giz_cw, giz_ch);
                            gizmo_els.push(
                                gizmo_dot(
                                    format!("gizmo_rotate_{}", giz_lid),
                                    rp.0,
                                    rp.1,
                                    14.0,
                                    accent,
                                    white,
                                    true,
                                )
                                .child(icon_box(IconName::RotateCw))
                                .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                    let (mx, my) = (event.position.x / px(1.0), event.position.y / px(1.0));
                                    s_h.update(cx, |s, cx| {
                                        // Gizmo gesture start: checkpoint before select+drag.
                                        s.checkpoint();
                                        s.select_layer(Some(lid_h.clone()));
                                        s.preview_fast = true;
                                        cx.notify();
                                    });
                                    let (cmx, cmy) = gizmo_to_comp(mx, my, h_frame, h_fit, h_cw, h_ch);
                                    if let Some(lay) = snap_h(cx) {
                                        let a = lay.transform.anchor_point;
                                        let aw = lay.local_to_world_point(a);
                                        let ang = (cmy - aw.y).atan2(cmx - aw.x);
                                        let rot = lay.transform.rotation;
                                        p_h.update(cx, |this, cx| {
                                            this.gizmo_drag = Some(ViewerGizmoDrag::Rotate {
                                                layer_id: lid_h.clone(),
                                                start_rot: rot,
                                                start_angle: ang,
                                            });
                                            cx.notify();
                                        });
                                    }
                                }).into_any_element());
                        }

                        // --- Pivot diamond (amber): drag to move the anchor.
                        {
                            let p_h = giz_panel.clone();
                            let s_h = giz_state.clone();
                            let lid_h = giz_lid.clone();
                            let snap_h = snap_layer.clone();
                            let (h_frame, h_fit, h_cw, h_ch) =
                                (giz_frame, giz_fit, giz_cw, giz_ch);
                            gizmo_els.push(
                                gizmo_dot(
                                    format!("gizmo_anchor_{}", giz_lid),
                                    anchor_c.0,
                                    anchor_c.1,
                                    12.0,
                                    Rgba { r: 0.96, g: 0.62, b: 0.04, a: 1.0 },
                                    white,
                                    true,
                                )
                                .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                    let (mx, my) = (event.position.x / px(1.0), event.position.y / px(1.0));
                                    s_h.update(cx, |s, cx| {
                                        // Gizmo gesture start: checkpoint before select+drag.
                                        s.checkpoint();
                                        s.select_layer(Some(lid_h.clone()));
                                        s.preview_fast = true;
                                        cx.notify();
                                    });
                                    let (cmx, cmy) = gizmo_to_comp(mx, my, h_frame, h_fit, h_cw, h_ch);
                                    if let Some(lay) = snap_h(cx) {
                                        if let Some(loc) = lay.world_to_local_point(Vec2::new(cmx, cmy)) {
                                            p_h.update(cx, |this, cx| {
                                                this.gizmo_drag = Some(ViewerGizmoDrag::Anchor {
                                                    layer_id: lid_h.clone(),
                                                    start_local: loc,
                                                });
                                                cx.notify();
                                            });
                                        }
                                    }
                                }).into_any_element());
                        }

                        // --- Mask Path Editor overlay: sampled curves for
                        // every mask, draggable nodes + tangent handles for
                        // the active edit target. Shared Path model drives
                        // masks, pen shapes, motion and text paths alike.
                        {
                            let edit_target: Option<(String, String)> =
                                giz_state.read(cx).active_mask_edit.clone();
                            for mask in &layer.masks {
                                let mid = mask.id.clone();
                                let is_active = edit_target
                                    == Some((giz_lid.clone(), mid.clone()));
                                let full =
                                    layer.world_matrix() * mask.transform.local_matrix;
                                let m2c = |p: Vec2| {
                                    let w = full.transform_point(p);
                                    (
                                        (w.x + giz_cw / 2.0) * giz_fit,
                                        (w.y + giz_ch / 2.0) * giz_fit,
                                    )
                                };
                                let dim = if mask.enabled { 1.0 } else { 0.3 };
                                let curve_col = if is_active {
                                    Rgba { r: 1.0, g: 0.85, b: 0.25, a: 0.95 * dim }
                                } else {
                                    Rgba { r: 1.0, g: 0.85, b: 0.25, a: 0.45 * dim }
                                };
                                // Sampled curve dots (bounded count).
                                let flat = mask.path.flatten(0.75);
                                let step = (flat.len() / 120).max(1);
                                for p in flat.iter().step_by(step) {
                                    let (cxp, cyp) = m2c(*p);
                                    gizmo_els.push(
                                        div()
                                            .absolute()
                                            .left(px(cxp - 1.5))
                                            .top(px(cyp - 1.5))
                                            .w(px(3.))
                                            .h(px(3.))
                                            .rounded_full()
                                            .bg(curve_col)
                                            .into_any_element(),
                                    );
                                }
                                // Nodes (+ handles for the selected node).
                                for (idx, node) in mask.path.points.iter().enumerate() {
                                    let (nx, ny) = m2c(node.pos);
                                    // NOTE: read panel state from `self`
                                    // directly — `cx.read()` on our own
                                    // entity panics inside render.
                                    let is_sel = is_active
                                        && self.mask_edit_point == Some(idx);
                                    let p_h = giz_panel.clone();
                                    let s_h = giz_state.clone();
                                    let lid_h = giz_lid.clone();
                                    let mid_h = mid.clone();
                                    let s_r = giz_state.clone();
                                    let lid_r = giz_lid.clone();
                                    let mid_r = mid.clone();
                                    let (h_frame, h_fit, h_cw, h_ch) =
                                        (giz_frame, giz_fit, giz_cw, giz_ch);
                                    let dot_col = if is_sel {
                                        Rgba { r: 0.96, g: 0.62, b: 0.04, a: 1.0 }
                                    } else {
                                        Rgba { r: 1.0, g: 1.0, b: 1.0, a: 0.9 * dim + 0.1 }
                                    };
                                    gizmo_els.push(
                                        gizmo_dot(
                                            format!("mask_node_{}_{}", mid, idx),
                                            nx,
                                            ny,
                                            if is_sel { 11.0 } else { 9.0 },
                                            dot_col,
                                            accent,
                                            true,
                                        )
                                        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                            let (mx, my) = (event.position.x / px(1.0), event.position.y / px(1.0));
                                            s_h.update(cx, |s, cx| {
                                                s.checkpoint();
                                                s.select_layer(Some(lid_h.clone()));
                                                s.set_active_mask_edit(Some((lid_h.clone(), mid_h.clone())));
                                                s.preview_fast = true;
                                                cx.notify();
                                            });
                                            let _ = gizmo_to_comp(mx, my, h_frame, h_fit, h_cw, h_ch);
                                            p_h.update(cx, |this, cx| {
                                                this.mask_edit_point = Some(idx);
                                                this.mask_down_moved = false;
                                                this.mask_drag = Some(MaskDrag {
                                                    layer_id: lid_h.clone(),
                                                    mask_id: mid_h.clone(),
                                                    index: idx,
                                                    kind: MaskDragKind::Point,
                                                });
                                                cx.notify();
                                            });
                                        })
                                        .on_mouse_down(MouseButton::Right, {
                                            move |_event, _window, cx| {
                                                s_r.update(cx, |s, cx| {
                                                    let _ = s.delete_mask_point(&lid_r, &mid_r, idx);
                                                    cx.notify();
                                                });
                                            }
                                        })
                                        .into_any_element(),
                                    );
                                    // Tangent handles for the selected node.
                                    if is_sel {
                                        for (is_in, tip) in [(true, node.in_abs()), (false, node.out_abs())] {
                                            let (hx, hy) = m2c(tip);
                                            let p_hh = giz_panel.clone();
                                            let s_hh = giz_state.clone();
                                            let lid_hh = giz_lid.clone();
                                            let mid_hh = mid.clone();
                                            let (hh_frame, hh_fit, hh_cw, hh_ch) =
                                                (giz_frame, giz_fit, giz_cw, giz_ch);
                                            let _ = (hh_frame, hh_fit, hh_cw, hh_ch);
                                            gizmo_els.push(
                                                gizmo_dot(
                                                    format!(
                                                        "mask_handle_{}_{}_{}",
                                                        mid,
                                                        idx,
                                                        if is_in { "in" } else { "out" }
                                                    ),
                                                    hx,
                                                    hy,
                                                    7.0,
                                                    Rgba { r: 0.35, g: 0.85, b: 1.0, a: 1.0 },
                                                    white,
                                                    true,
                                                )
                                                .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                                    let (mx, my) = (event.position.x / px(1.0), event.position.y / px(1.0));
                                                    s_hh.update(cx, |s, cx| {
                                                        s.checkpoint();
                                                        s.select_layer(Some(lid_hh.clone()));
                                                        s.set_active_mask_edit(Some((lid_hh.clone(), mid_hh.clone())));
                                                        s.preview_fast = true;
                                                        cx.notify();
                                                    });
                                                    let _ = gizmo_to_comp(mx, my, hh_frame, hh_fit, hh_cw, hh_ch);
                                                    p_hh.update(cx, |this, cx| {
                                                        this.mask_edit_point = Some(idx);
                                                        this.mask_down_moved = true;
                                                        this.mask_drag = Some(MaskDrag {
                                                            layer_id: lid_hh.clone(),
                                                            mask_id: mid_hh.clone(),
                                                            index: idx,
                                                            kind: if is_in {
                                                                MaskDragKind::InHandle
                                                            } else {
                                                                MaskDragKind::OutHandle
                                                            },
                                                        });
                                                        cx.notify();
                                                    });
                                                })
                                                .into_any_element(),
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // --- Pen/shape path overlay: sampled spline curves
                        // (+ nodes) for the selected layer's own paths, so
                        // pen work is always visible, not just masks.
                        {
                            let mut pen_paths: Vec<project::Path> = Vec::new();
                            match &layer.source {
                                LayerSource::Shape {
                                    shape_type: ShapeType::Path { path_data, .. },
                                } => {
                                    pen_paths.push(project::Path::from_svg(path_data));
                                }
                                LayerSource::Text { text_path: Some(tp), .. } => {
                                    pen_paths.push(tp.clone());
                                }
                                _ => {}
                            }
                            // Evaluated mask paths already draw above; pen
                            // paths draw here in blueprint blue.
                            let pen_col = Rgba { r: 0.4, g: 0.8, b: 1.0, a: 0.95 };
                            let mut pen_els = div()
                                .id("pen_curve_overlay")
                                .test_support()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .bottom_0();
                            for path in &pen_paths {
                                let flat = path.flatten(0.75);
                                let step = (flat.len() / 120).max(1);
                                for p in flat.iter().step_by(step) {
                                    let w = layer.local_to_world_point(*p);
                                    let cxp = (w.x + giz_cw / 2.0) * giz_fit;
                                    let cyp = (w.y + giz_ch / 2.0) * giz_fit;
                                    pen_els = pen_els.child(
                                        div()
                                            .absolute()
                                            .left(px(cxp - 1.5))
                                            .top(px(cyp - 1.5))
                                            .w(px(3.))
                                            .h(px(3.))
                                            .rounded_full()
                                            .bg(pen_col),
                                    );
                                }
                                for node in path.points.iter() {
                                    let w = layer.local_to_world_point(node.pos);
                                    let cxp = (w.x + giz_cw / 2.0) * giz_fit;
                                    let cyp = (w.y + giz_ch / 2.0) * giz_fit;
                                    pen_els = pen_els.child(
                                        gizmo_dot(
                                            format!("pen_node_{}_{}_{}", giz_lid, cxp.round() as i32, cyp.round() as i32),
                                            cxp,
                                            cyp,
                                            8.0,
                                            Rgba { r: 0.4, g: 0.8, b: 1.0, a: 1.0 },
                                            white,
                                            true,
                                        ),
                                    );
                                }
                            }
                            // Empty containers still hit-test, so only mount
                            // when there is actually a curve to show.
                            if !pen_paths.is_empty() {
                                gizmo_els.push(pen_els.into_any_element());
                            }
                        }
                    }

                    elements.push(layer_el.into_any_element());
                }
                // render_layers() walks bottom-to-top; picking needs
                // topmost-first.
                self.pick_boxes = pick_list.into_iter().rev().collect();
                (elements, gizmo_els)
            }
            None => {
                self.pick_boxes = Vec::new();
                (Vec::new(), Vec::new())
            }
        };
        let active_tool = state.active_tool;
        let s_side = self.state.clone();
        // Semantic tool buttons (kit Button: focus, keyboard, tooltip and
        // cursor contracts included; toggled marks the active tool).
        let tool_btn = |tool: EditorTool, icon: IconName, label: &'static str, tip: &'static str, _cx: &App| {
            let is_active = active_tool == tool;
            let s_click = s_side.clone();
            div()
                .id(SharedString::from(format!("side_tool_btn_{label}")))
                .test_support()
                .child(
                    Button::new(SharedString::from(format!("side_tool_btn_{label}_btn")))
                        .compact()
                        .toggled(is_active)
                        .tooltip(tip)
                        .child(icon_box(icon))
                        .on_click(move |_, _, cx| {
                            s_click.update(cx, |s, cx| {
                                s.set_tool(tool);
                                cx.notify();
                            });
                        }),
                )
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
            .id("viewer_tool_strip")
            .test_support()
            .w(px(36.))
            .h_full()
            .py_2()
            .gap_1p5()
            .items_center()
            .border_r_1()
            .border_color(ae::border())
            .bg(ae::panel())
            .child(tool_btn(EditorTool::Move, IconName::Move, "move", "Move Tool (V)", cx))
            .child(tool_btn(EditorTool::Hand, IconName::Hand, "hand", "Hand Tool (H)", cx))
            .child(tool_btn(EditorTool::Rotate, IconName::RotateCw, "rotate", "Rotate Tool (W)", cx))
            .child(tool_btn(EditorTool::Pen, IconName::Pen, "pen", "Pen Tool (G)", cx))
            .child(tool_btn(EditorTool::Text, IconName::Type, "text", "Text Tool (T)", cx))
            .child({
                let p_shape = panel_entity.clone();
                div()
                    .id(SharedString::from(format!("side_tool_btn_{shape_label}")))
                    .test_support()
                    .child(
                        Button::new(SharedString::from(format!("side_tool_btn_{shape_label}_btn")))
                            .compact()
                            .toggled(shape_is_active)
                            .tooltip("Shape Tool (Q) — click again to toggle rectangle/ellipse")
                            .child(icon_box(shape_icon))
                            .on_click(move |_, _, cx| {
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
                            }),
                    )
            })
            .child(div().w(px(20.)).h(px(1.)).bg(ae::border()).my_1())
            // Single contextual action: creates a layer of the active tool
            // type at the viewport center (click the canvas to place freely).
            .child(
                div()
                    .id("quick_add_center_button")
                    .test_support()
                    .child(
                        Button::new("quick_add_center_btn")
                            .compact()
                            .tooltip("Add layer of the active tool at viewport center")
                            .child(icon_box(IconName::Plus))
                            .on_click(move |_, _, cx| {
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
                                        s.tool_solid_color,
                                        400,
                                        400,
                                    );
                                }
                            }
                            cx.notify();
                        });
                    })
                )
            );

        div()
            .id("composition_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
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
                let curr_x = event.position.x / px(1.0);
                let curr_y = event.position.y / px(1.0);
                // Path Editor mask drags win over everything (nodes map
                // through the cheap matrix path, no scene eval per move).
                if let Some(mdrag) = this.mask_drag.clone() {
                    let frame_org = frame_origin_or_center(
                        this.frame_origin,
                        this.viewport_px,
                        this.viewport_origin,
                        this.canvas_px,
                    );
                    let (fox, foy) = frame_org.unwrap_or((0.0, 0.0));
                    let (cw, ch) = {
                        let s = this.state.read(cx);
                        match s.active_composition() {
                            Some(c) => (c.width as f32, c.height as f32),
                            None => (1920.0, 1080.0),
                        }
                    };
                    let fit_here = this
                        .viewport_px
                        .map(|(vw, vh)| {
                            ((vw - 32.0) / cw).min((vh - 32.0) / ch).clamp(0.05, 4.0)
                        })
                        .unwrap_or(1.0);
                    let cmx = (curr_x - fox) / fit_here - cw / 2.0;
                    let cmy = (curr_y - foy) / fit_here - ch / 2.0;
                    let st = this.state.clone();
                    let local = st.read(cx).comp_to_layer_local(&mdrag.layer_id, Vec2::new(cmx, cmy));
                    if let Some(loc) = local {
                        let ok = match mdrag.kind {
                            MaskDragKind::Point => st
                                .update(cx, |s, cx| {
                                    let r = s.move_mask_point_live(
                                        &mdrag.layer_id,
                                        &mdrag.mask_id,
                                        mdrag.index,
                                        loc,
                                    );
                                    cx.notify();
                                    r.is_ok()
                                }),
                            MaskDragKind::InHandle => st.update(cx, |s, cx| {
                                let r = s.move_mask_handle_live(
                                    &mdrag.layer_id,
                                    &mdrag.mask_id,
                                    mdrag.index,
                                    true,
                                    loc,
                                );
                                cx.notify();
                                r.is_ok()
                            }),
                            MaskDragKind::OutHandle => st.update(cx, |s, cx| {
                                let r = s.move_mask_handle_live(
                                    &mdrag.layer_id,
                                    &mdrag.mask_id,
                                    mdrag.index,
                                    false,
                                    loc,
                                );
                                cx.notify();
                                r.is_ok()
                            }),
                        };
                        if ok {
                            this.mask_down_moved = true;
                        }
                    }
                    return;
                }
                // Transform-gizmo drags win over canvas drags.
                if let Some(drag) = this.gizmo_drag.clone() {
                    // Window px -> composition px via the measured frame.
                    let frame_org = frame_origin_or_center(
                        this.frame_origin,
                        this.viewport_px,
                        this.viewport_origin,
                        this.canvas_px,
                    );
                    let (fox, foy) = frame_org.unwrap_or((0.0, 0.0));
                    // Re-derive the uniform fit from the live composition so
                    // stale renders never skew a drag.
                    let (cw, ch, vfit) = {
                        let s = this.state.read(cx);
                        match s.active_composition() {
                            Some(c) => (c.width as f32, c.height as f32, 1.0),
                            None => (1920.0, 1080.0, 1.0),
                        }
                    };
                    let fit_here = this
                        .viewport_px
                        .map(|(vw, vh)| {
                            ((vw - 32.0) / cw).min((vh - 32.0) / ch).clamp(0.05, 4.0)
                        })
                        .unwrap_or(vfit);
                    let cmx = (curr_x - fox) / fit_here - cw / 2.0;
                    let cmy = (curr_y - foy) / fit_here - ch / 2.0;
                    let st = this.state.clone();
                    match drag {
                        ViewerGizmoDrag::Rotate { layer_id, start_rot, start_angle } => {
                            // Cached drag frame: anchor world from the cheap
                            // matrix path, no full scene evaluation per move.
                            let anchor_world = st.read(cx).layer_drag_frame(&layer_id).map(|(world, anchor)| {
                                world.transform_point(anchor)
                            });
                            if let Some(aw) = anchor_world {
                                let ang = (cmy - aw.y).atan2(cmx - aw.x);
                                let delta_deg = (ang - start_angle).to_degrees();
                                st.update(cx, |s, cx| {
                                    s.set_layer_rotation(&layer_id, start_rot + delta_deg);
                                    cx.notify();
                                });
                            }
                        }
                        ViewerGizmoDrag::Scale { layer_id, uniform, use_x, use_y, start_scale, start_local, anchor_local } => {
                            let local = st.read(cx).layer_drag_frame(&layer_id).and_then(|(world, _)| {
                                world.transform_point_inverse(Vec2::new(cmx, cmy))
                            });
                            if let Some(loc) = local {
                                let denom_x = (start_local.x - anchor_local.x).abs().max(1.0);
                                let denom_y = (start_local.y - anchor_local.y).abs().max(1.0);
                                let mut rx = (loc.x - anchor_local.x) / denom_x;
                                let mut ry = (loc.y - anchor_local.y) / denom_y;
                                // Dragging across the pivot flips sign; keep
                                // magnitude motion smooth by sign of start side.
                                if (start_local.x - anchor_local.x) < 0.0 {
                                    rx = -rx;
                                }
                                if (start_local.y - anchor_local.y) < 0.0 {
                                    ry = -ry;
                                }
                                let (mut nx, mut ny) = (start_scale.x * rx.max(0.01), start_scale.y * ry.max(0.01));
                                if uniform {
                                    let r = ((rx.max(0.01) + ry.max(0.01)) / 2.0).max(0.01);
                                    nx = start_scale.x * r;
                                    ny = start_scale.y * r;
                                }
                                if !use_x {
                                    nx = start_scale.x;
                                }
                                if !use_y {
                                    ny = start_scale.y;
                                }
                                st.update(cx, |s, cx| {
                                    s.set_layer_scale(&layer_id, nx, ny);
                                    cx.notify();
                                });
                            }
                        }
                        ViewerGizmoDrag::Anchor { layer_id, start_local } => {
                            let local = st.read(cx).layer_drag_frame(&layer_id).and_then(|(world, _)| {
                                world.transform_point_inverse(Vec2::new(cmx, cmy))
                            });
                            if let Some(loc) = local {
                                let d = Vec2::new(loc.x - start_local.x, loc.y - start_local.y);
                                st.update(cx, |s, cx| {
                                    s.move_layer_anchor(&layer_id, d);
                                    cx.notify();
                                });
                            }
                        }
                    }
                    return;
                }
                if let (true, Some((last_x, last_y))) = (this.is_dragging_canvas, this.last_canvas_mouse) {
                    let dx = curr_x - last_x;
                    let dy = curr_y - last_y;
                    let active_tool = this.state.read(cx).active_tool;
                    // Composition-aware factors (uniform fit scale): layers
                    // track the cursor 1:1 at any comp size or zoom.
                    let (cw2, ch2) = this
                        .state
                        .read(cx)
                        .active_composition()
                        .map(|c| (c.width as f32, c.height as f32))
                        .unwrap_or((1920.0, 1080.0));
                    let fit_here = this
                        .viewport_px
                        .map(|(vw, vh)| {
                            ((vw - 32.0) / cw2).min((vh - 32.0) / ch2).clamp(0.05, 4.0)
                        })
                        .unwrap_or(1.0);
                    if active_tool == EditorTool::Move {
                        let s = this.state.clone();
                        s.update(cx, |s, cx| {
                            s.nudge_position(dx / fit_here, dy / fit_here);
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
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.is_dragging_canvas = false;
                this.last_canvas_mouse = None;
                this.gizmo_drag = None;
                this.down_on_layer = false;
                // Mask node click without drag cycles the node kind
                // (Corner → Smooth → Symmetric → Auto).
                if let Some(mdrag) = this.mask_drag.take() {
                    let moved = std::mem::replace(&mut this.mask_down_moved, false);
                    if !moved && mdrag.kind == MaskDragKind::Point {
                        let s = this.state.clone();
                        s.update(cx, |s, cx| {
                            let _ = s.cycle_mask_point_kind(
                                &mdrag.layer_id,
                                &mdrag.mask_id,
                                mdrag.index,
                            );
                            cx.notify();
                        });
                    }
                }
                // Empty-canvas click with the Move tool deselects (the Move
                // tool never drags from empty space).
                if this.empty_down.take().is_some() {
                    let s = this.state.clone();
                    s.update(cx, |s, cx| {
                        if s.active_tool == EditorTool::Move {
                            s.select_layer(None);
                        }
                        s.preview_fast = false;
                        cx.notify();
                    });
                } else {
                    let s = this.state.clone();
                    s.update(cx, |s, cx| {
                        s.preview_fast = false;
                        cx.notify();
                    });
                }
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.is_dragging_canvas = false;
                this.last_canvas_mouse = None;
                this.gizmo_drag = None;
                this.mask_drag = None;
                this.mask_down_moved = false;
                this.down_on_layer = false;
                this.empty_down = None;
                let s = this.state.clone();
                s.update(cx, |s, cx| {
                    s.preview_fast = false;
                    cx.notify();
                });
            }))
            // Viewport header / controls (AE comp viewer chrome).
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(ae::border())
                    .bg(ae::panel())
                    .items_center()
                    .justify_between()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(ae::text())
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .min_w_0()
                            .flex_1()
                            .child(div().font_bold().truncate().child(format!("{comp_name} / Active Camera")))
                            .child(
                                div()
                                    .text_color(ae::dim())
                                    .flex_none()
                                    .child(comp_res),
                            )
                            .child(
                                div()
                                    .text_color(ae::dim())
                                    .flex_none()
                                    .child(comp_fps),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .flex_none()
                            .text_color(ae::dim())
                            .child(div().child("100% (Fit)"))
                            .child(div().child("Full Res (1:1)"))
                            .child(div().child("RGB Channel"))
                            .child(div().font_bold().child("+")),
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
                            .id("canvas_viewport_wrap")
                            .test_support()
                            .flex_1()
                            .size_full()
                            .bg(ae::bg())
                            .items_center()
                            .justify_center()
                            .p_4()
                            .overflow_hidden()
                            .on_prepaint({
                                let p_measure = cx.entity().clone();
                                move |bounds, _window, cx| {
                                    let w = bounds.size.width / px(1.0);
                                    let h = bounds.size.height / px(1.0);
                                    let ox = bounds.origin.x / px(1.0);
                                    let oy = bounds.origin.y / px(1.0);
                                    p_measure.update(cx, |this, cx| {
                                        let changed = this.viewport_px != Some((w, h))
                                            || this.viewport_origin != Some((ox, oy));
                                        this.viewport_px = Some((w, h));
                                        this.viewport_origin = Some((ox, oy));
                                        if changed {
                                            cx.notify();
                                        }
                                    });
                                }
                            })
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
                                    .on_prepaint({
                                        let p_frame = cx.entity().clone();
                                        move |bounds, _window, cx| {
                                            let ox = bounds.origin.x / px(1.0);
                                            let oy = bounds.origin.y / px(1.0);
                                            p_frame.update(cx, |this, cx| {
                                                if this.frame_origin != Some((ox, oy)) {
                                                    this.frame_origin = Some((ox, oy));
                                                    cx.notify();
                                                }
                                            });
                                        }
                                    })
                                    .on_mouse_down(MouseButton::Left, {
                                        let p_drag = cx.entity().clone();
                                        let s_tool = self.state.clone();
                                        // Captured fit + frame origin map window px to comp px
                                        // exactly (no hardcoded 1920/512 factors, no missing
                                        // panel offset): click-to-place and drags stay correct
                                        // for any composition size and any window layout.
                                        let fit_pick = fit;
                                        let comp_pw = comp_w;
                                        let comp_ph = comp_h;
                                        let frame_org = frame_org;
                                        move |event, _window, cx| {
                                            // Gizmo handles set their own drag first (they
                                            // bubble through here); never start a canvas op.
                                            if p_drag.read(cx).gizmo_drag.is_some() {
                                                p_drag.update(cx, |this, cx| {
                                                    this.down_on_layer = false;
                                                    this.empty_down = None;
                                                    cx.notify();
                                                });
                                                return;
                                            }
                                            let curr_x = event.position.x / px(1.0);
                                            let curr_y = event.position.y / px(1.0);
                                            // Move/Rotate drags start ONLY on a layer body
                                            // (flagged by the shell handler above). Empty
                                            // presses just record for click-deselect.
                                            let mut on_layer = p_drag.read(cx).down_on_layer;
                                            let active_tool = s_tool.read(cx).active_tool;
                                            let (fox, foy) = frame_org.unwrap_or((curr_x - canvas_w / 2.0, curr_y - canvas_h / 2.0));
                                            let comp_x = (curr_x - fox) / fit_pick - comp_pw / 2.0;
                                            let comp_y = (curr_y - foy) / fit_pick - comp_ph / 2.0;
                                            // Deterministic topmost pick: sibling
                                            // hit-test order has sent presses to
                                            // covered layers (e.g. background)
                                            // instead of the visible top one.
                                            // The pick wins for the Move tool.
                                            if active_tool == EditorTool::Move {
                                                if let Some(picked) = p_drag.read(cx).pick_top_at(comp_x, comp_y) {
                                                    on_layer = true;
                                                    s_tool.update(cx, |s, cx| {
                                                        s.select_layer(Some(picked));
                                                        cx.notify();
                                                    });
                                                }
                                            }
                                            p_drag.update(cx, |this, cx| {
                                                this.down_on_layer = false;
                                                if on_layer {
                                                    this.is_dragging_canvas = true;
                                                    this.last_canvas_mouse = Some((curr_x, curr_y));
                                                    this.empty_down = None;
                                                } else {
                                                    this.is_dragging_canvas = false;
                                                    this.last_canvas_mouse = None;
                                                    this.empty_down = Some((curr_x, curr_y));
                                                }
                                                cx.notify();
                                            });
                                            s_tool.update(cx, |s, cx| {
                                                // Gesture start: one undo step per drag/create.
                                                s.checkpoint();
                                                if on_layer {
                                                    s.preview_fast = true;
                                                }
                                                cx.notify();
                                            });
                                            match active_tool {
                                                EditorTool::Text => {
                                                    // Select the topmost text
                                                    // layer under the cursor
                                                    // for editing; only empty
                                                    // space creates a layer.
                                                    let pick = p_drag.read(cx).pick_top_at(comp_x, comp_y);
                                                    s_tool.update(cx, |s, cx| {
                                                        let _ = s.text_press_at(Vec2::new(comp_x, comp_y), pick);
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
                                                    // Explicit topmost routing
                                                    // (mask target, path/text
                                                    // under cursor, else new):
                                                    // never depends on stale
                                                    // selection.
                                                    let pick = p_drag.read(cx).pick_top_at(comp_x, comp_y);
                                                    s_tool.update(cx, |s, cx| {
                                                        let _ = s.pen_press_at(Vec2::new(comp_x, comp_y), pick);
                                                        cx.notify();
                                                    });
                                                }
                                                EditorTool::Rotate => {
                                                    s_tool.update(cx, |s, cx| {
                                                        let step = s.tool_rotate_step;
                                                        s.nudge_rotation(step);
                                                        cx.notify();
                                                    });
                                                }
                                                _ => {}
                                            }
                                        }
                                    })
                                    .children(rendered_layers)
                                    .children(gizmo_els);

                                let p_canvas_rclick = cx.entity().clone();
                                canvas_frame = canvas_frame.on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                    let pos = event.position;
                                    p_canvas_rclick.update(cx, |this, cx| {
                                        this.open_context_menu(ViewerContextMenuTarget::Canvas, pos);
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
                                            let lid_piv = lid.clone();

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
                                                .child(menu_button("Center Pivot", cx, {
                                                    let s = s_menu.clone();
                                                    let p = p_close.clone();
                                                    move |cx| {
                                                        s.update(cx, |s, cx| { s.reset_layer_anchor_center(&lid_piv); cx.notify(); });
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

                                    // Cursor-anchored via deferred Positioner (viewport
                                    // clamped), not a fixed corner.
                                    let viewer_menu_pos = self.menu_pos.unwrap_or(point(px(10.), px(10.)));
                                    let viewer_menu_box = div()
                                        .id("viewer_context_menu")
                                        .test_support()
                                        .w(px(200.))
                                        .p_1()
                                        .bg(cx.theme().popover)
                                        .text_color(cx.theme().popover_foreground)
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .rounded_md()
                                        .shadow_lg()
                                        .child(menu_items);
                                    canvas_frame = canvas_frame.child(deferred(
                                        Positioner::corner(Anchor::TopLeft, viewer_menu_pos)
                                            .margin(px(8.))
                                            .occlude()
                                            .child(viewer_menu_box),
                                    ));
                                }

                                canvas_frame
                            })
                    )
            )
            // Status bar (AE comp viewer footer chrome).
            .child({
                self.last_frame_ms = render_t0.elapsed().as_secs_f32() * 1000.0;
                let ms = self.last_frame_ms;
                let layer_count = comp_opt.map(|c| c.layers.len()).unwrap_or(0);
                let solids_count = comp_opt
                    .map(|c| c.layers.iter().filter(|l| matches!(&l.source, LayerSource::Solid { .. })).count())
                    .unwrap_or(0);
                let audio_count = comp_opt
                    .map(|c| c.layers.iter().filter(|l| matches!(&l.source, LayerSource::Video { .. })).count())
                    .unwrap_or(0);
                let quality = state.preview_quality.label();
                let sel_name = state
                    .selected_layer_id
                    .as_ref()
                    .and_then(|id| comp_opt.and_then(|c| c.get_layer(id)))
                    .map(|l| l.name.clone())
                    .unwrap_or_else(|| "None".to_string());
                let fps_label = comp_opt.map(|c| format!("{:.2}", c.frame_rate)).unwrap_or_else(|| "—.——".to_string());
                // Pen target hint so routing never surprises.
                let pen_hint = if state.active_tool == EditorTool::Pen {
                    match state.active_mask_edit.clone() {
                        Some((lid, mid)) => {
                            let name = state
                                .active_composition()
                                .and_then(|c| c.get_layer(&lid))
                                .and_then(|l| l.get_mask(&mid))
                                .map(|m| m.name.clone())
                                .unwrap_or(mid);
                            format!("Pen → mask '{name}' (Esc exits) · ")
                        }
                        None => String::new(),
                    }
                } else {
                    String::new()
                };
                h_flex()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(ae::border())
                    .bg(ae::panel())
                    .text_xs()
                    .text_color(ae::dim())
                    .justify_between()
                    .child(div().child(format!("Comp {layer_count} Solids {solids_count} Audio {audio_count}")))
                    .child(div().child(format!(
                        "{pen_hint}Layer: {sel_name}  FPS: {fps_label} (Realtime)"
                    )))
                    .child(div().child(format!(
                        "{quality} · {ms:.1} ms · GPU Acceleration: ACTIVE"
                    )))
            })
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
    pub masks_expanded: bool,
    pub tools_expanded: bool,
    /// AE text inspector twirl-downs (`true` = collapsed).
    /// Keys: `character`, `paragraph`, `stroke`, `path`.
    pub text_collapsed: HashSet<&'static str>,
    /// Collapsed applied-effect cards by effect id (empty = all expanded).
    pub fx_collapsed: HashSet<String>,
    /// Collapsed Shader Lab parameter groups (`effect_id:group`).
    pub fx_group_collapsed: HashSet<String>,
    /// Effect ID with the Shader Lab source editor open (`None` = closed).
    pub shader_editor_open: Option<String>,
    /// Live Shader Lab source editor (single open editor; re-created when
    /// the opened effect or its source hash changes so Apply refreshes it).
    pub shader_editor: Option<Entity<TextareaState>>,
    pub shader_editor_key: Option<(String, u64)>,
    /// Mask Shape dialog target `(layer_id, mask_id)` (`None` = closed).
    pub mask_shape_open: Option<(String, String)>,
    /// Live numeric editors for the Mask Shape dialog (created on open).
    pub mask_shape_editors: Option<MaskShapeEditors>,
    /// Mask being renamed + its live editor (`None` = not renaming).
    pub mask_rename_editor: Option<MaskRenameEditor>,
    /// Auto-trace options disclosure in the Masks card.
    pub trace_open: bool,
    /// Pending Auto-trace options (mirrors the AE dialog; Run applies).
    pub trace_opts: project::AutoTraceOptions,
    /// Last auto-trace error shown under the Trace block.
    pub trace_error: Option<String>,
    /// Armed shape-clipboard source layer for cross-layer Shape→Mask.
    pub shape_clipboard: Option<String>,
    /// Retained Select states per ShaderLab enum param
    /// (`effect_id`, `param`, selected index).
    pub select_states: HashMap<(String, String, usize), Entity<SelectState<SearchableVec<String>>>>,
    /// Confirm subscriptions for the retained Select states.
    pub select_subs: HashMap<(String, String, usize), Subscription>,
    /// Linked vector widgets (`effect_id:param` present = linked).
    pub vec_link: HashSet<String>,
    /// Selected gradient-editor stop per effect id.
    pub gradient_stop: HashMap<String, usize>,
    /// Active 3-mode color picker key: e.g. "text_fill", "text_stroke", "solid_color" (None = closed).
    pub active_color_picker: Option<String>,
    /// Color mode per key: "none" | "color" | "gradient"
    pub color_picker_mode: HashMap<String, String>,
    /// Gradient angle per key in degrees.
    pub color_picker_gradient_angle: HashMap<String, f32>,
    /// Active gradient stop index (0 or 1) per key.
    pub color_picker_gradient_stop: HashMap<String, usize>,
    /// Gradient stop colors (stop0, stop1) per key.
    pub color_picker_gradient_colors: HashMap<String, (Color, Color)>,
    /// Retained Combobox states (e.g. "text_font_family", "text_font_style").
    pub combobox_states: HashMap<String, Entity<ComboboxState<SearchableVec<String>>>>,
    /// Subscriptions for retained Combobox states.
    pub combobox_subs: HashMap<String, Subscription>,
}

/// Live rename editor for one mask (Enter commits, blur/Esc cancels).
pub struct MaskRenameEditor {
    pub target: (String, String),
    pub editor: Entity<InputState>,
    pub _sub: Subscription,
}

/// Live numeric editors for the Mask Shape dialog (X/Y/W/H + kind pill).
/// Texts are cached on every keystroke (Change events) so Apply never has
/// to read entities from inside event handlers.
pub struct MaskShapeEditors {
    pub target: (String, String),
    pub kind: project::MaskShapeKind,
    pub x: Entity<InputState>,
    pub y: Entity<InputState>,
    pub w: Entity<InputState>,
    pub h: Entity<InputState>,
    pub x_text: String,
    pub y_text: String,
    pub w_text: String,
    pub h_text: String,
    pub _subs: Vec<Subscription>,
}

/// Cloneable render snapshot of the open Mask Shape dialog (the live
/// struct holds Subscriptions, so the section gets entities only).
#[derive(Clone)]
pub struct MaskShapeView {
    pub target: (String, String),
    pub kind: project::MaskShapeKind,
    pub x: Entity<InputState>,
    pub y: Entity<InputState>,
    pub w: Entity<InputState>,
    pub h: Entity<InputState>,
}

/// Cloneable render snapshot of the active mask rename editor.
#[derive(Clone)]
pub struct MaskRenameView {
    pub target: (String, String),
    pub editor: Entity<InputState>,
}

/// Render snapshot for the automatic effect widgets (the panel is
/// borrowed by render, so link/stop state crosses by clone).
#[derive(Clone, Default)]
pub struct PropUi {
    /// Linked vector widgets (`effect_id:param` present = linked).
    pub vec_link: HashSet<String>,
    /// Selected gradient-editor stop per effect id.
    pub gradient_stop: HashMap<String, usize>,
}

struct TextInspectorInputs {
    text: Entity<InputState>,
    font_family: Entity<InputState>,
    font_size: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

pub(crate) struct InspectorColorPicker {
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
            masks_expanded: true,
            tools_expanded: false,
            text_collapsed: HashSet::new(),
            fx_collapsed: HashSet::new(),
            fx_group_collapsed: HashSet::new(),
            shader_editor_open: None,
            shader_editor: None,
            shader_editor_key: None,
            mask_shape_open: None,
            mask_shape_editors: None,
            mask_rename_editor: None,
            trace_open: false,
            trace_opts: project::AutoTraceOptions::default(),
            trace_error: None,
            shape_clipboard: None,
            select_states: HashMap::new(),
            select_subs: HashMap::new(),
            vec_link: HashSet::new(),
            gradient_stop: HashMap::new(),
            active_color_picker: None,
            color_picker_mode: HashMap::new(),
            color_picker_gradient_angle: HashMap::new(),
            color_picker_gradient_stop: HashMap::new(),
            color_picker_gradient_colors: HashMap::new(),
            combobox_states: HashMap::new(),
            combobox_subs: HashMap::new(),
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
                "font_weight" | "text_weight" => {
                    if let Some(lid) = s.selected_layer_id.clone() {
                        let cur = match s.selected_layer().map(|l| &l.source) {
                            Some(LayerSource::Text { weight, .. }) => *weight as f32,
                            _ => 400.0,
                        };
                        let new_w = (cur + dx * 10.0).clamp(100.0, 900.0).round() as u16;
                        let _ = s.set_layer_font_weight(&lid, new_w);
                    }
                }
                other => {
                    if let Some((key_head, rest)) = other.split_once(':') {
                        if key_head.starts_with("text_") && rest.starts_with(|c: char| c.is_ascii_digit()) {
                            // text_scalar key: `text_tracking:<mult100>`.
                            if let Some(lid) = s.selected_layer_id.clone() {
                                let field = match key_head {
                                    "text_tracking" => "tracking",
                                    "text_leading" => "leading",
                                    "text_stroke_w" => "stroke_width",
                                    "text_baseline" => "baseline_shift",
                                    "text_box_h" => "box_height",
                                    _ => "box_width",
                                };
                                let cur = s.scrub_current_value(key_head).unwrap_or(0.0);
                                let mult = rest.parse::<f32>().unwrap_or(50.0) / 100.0;
                                let _ = s.set_layer_text_scalar(&lid, field, cur + dx * mult);
                            }
                            return;
                        }
                    }
                    if let Some(rest) = other.strip_prefix("slcl:") {
                        // Linked vector scrub: slcl:<eff>:<name> (all
                        // components together; see the vector widget).
                        let parts: Vec<&str> = rest.split(':').collect();
                        if parts.len() >= 2 {
                            if let (Some(eff), Some(name)) = (parts.first(), parts.get(1)) {
                                let _ = s.nudge_shaderlab_linked(eff, name, dx * 0.25);
                            }
                        }
                    } else if let Some(rest) = other.strip_prefix("slc:") {
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
                    } else if let Some(rest) = other.strip_prefix("mask:") {
                        // mask:<layer>:<mask>:<param>[:<mult100>]
                        let parts: Vec<&str> = rest.split(':').collect();
                        if parts.len() >= 3 {
                            let mult = parts.get(3).and_then(|m| m.parse::<f32>().ok()).unwrap_or(50.0) / 100.0;
                            let _ = s.nudge_mask_param(parts[0], parts[1], parts[2], dx * mult);
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
        // Typed entry is one undo step: checkpoint before editing.
        self.state.update(cx, |s, cx| {
            s.checkpoint();
            cx.notify();
        });
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

    /// Open the Mask Shape dialog for one mask (numeric Rectangle/Ellipse
    /// bounding box, After Effects: Layer > Mask > Mask Shape). Editors are
    /// prefilled from the mask's current bounds.
    pub fn open_mask_shape_dialog(
        &mut self,
        lid: &str,
        mid: &str,
        kind: project::MaskShapeKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (x, y, w, h) = self
            .state
            .read(cx)
            .active_composition()
            .and_then(|c| c.get_layer(lid))
            .and_then(|l| l.get_mask(mid))
            .and_then(|m| m.path.value.bounds())
            .map(|(mn, mx)| (mn.x, mn.y, mx.x - mn.x, mx.y - mn.y))
            .unwrap_or((0.0, 0.0, 200.0, 200.0));
        let fmt = |v: f32| {
            if (v - v.round()).abs() < 1e-4 {
                format!("{}", v.round() as i64)
            } else {
                format!("{v:.1}")
            }
        };
        let mk = |slot: u8, v: String, window: &mut Window, cx: &mut Context<Self>| -> (Entity<InputState>, Subscription) {
            let editor = cx.new(|cx| {
                let mut st = InputState::new(window, cx);
                st.set_value(v, window, cx);
                st
            });
            let sub = cx.subscribe(
                &editor,
                move |this: &mut Self, input: Entity<InputState>, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let text = input.read(cx).value().to_string();
                        if let Some(ed) = this.mask_shape_editors.as_mut() {
                            match slot {
                                0 => ed.x_text = text,
                                1 => ed.y_text = text,
                                2 => ed.w_text = text,
                                _ => ed.h_text = text,
                            }
                        }
                        cx.notify();
                    }
                },
            );
            (editor, sub)
        };
        let (x, y, w, h) = (fmt(x), fmt(y), fmt(w), fmt(h));
        let (xe, xs) = mk(0, x.clone(), window, cx);
        let (ye, ys) = mk(1, y.clone(), window, cx);
        let (we, ws) = mk(2, w.clone(), window, cx);
        let (he, hs) = mk(3, h.clone(), window, cx);
        self.mask_shape_editors = Some(MaskShapeEditors {
            target: (lid.to_string(), mid.to_string()),
            kind,
            x: xe,
            y: ye,
            w: we,
            h: he,
            x_text: x,
            y_text: y,
            w_text: w,
            h_text: h,
            _subs: vec![xs, ys, ws, hs],
        });
        if let Some(ed) = self.mask_shape_editors.as_ref().map(|e| e.x.clone()) {
            let handle = ed.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    /// Begin renaming one mask (Enter commits, blur cancels).
    pub fn begin_mask_rename(
        &mut self,
        lid: &str,
        mid: &str,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = cx.new(|cx| {
            let mut st = InputState::new(window, cx);
            st.set_value(initial, window, cx);
            st
        });
        let st = self.state.clone();
        let (lid_s, mid_s) = (lid.to_string(), mid.to_string());
        let sub = cx.subscribe(
            &editor,
            move |this: &mut Self, input: Entity<InputState>, event: &InputEvent, cx| {
                match event {
                    InputEvent::PressEnter { .. } => {
                        let text = input.read(cx).value().trim().to_string();
                        st.update(cx, |s, cx| {
                            let _ = s.rename_mask(&lid_s, &mid_s, &text);
                            cx.notify();
                        });
                        this.mask_rename_editor = None;
                        cx.notify();
                    }
                    InputEvent::Blur => {
                        this.mask_rename_editor = None;
                        cx.notify();
                    }
                    _ => {}
                }
            },
        );
        self.mask_rename_editor = Some(MaskRenameEditor {
            target: (lid.to_string(), mid.to_string()),
            editor: editor.clone(),
            _sub: sub,
        });
        let handle = editor.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn scrub_field<FMinus, FPlus>(
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
    let state_fast = state.clone();
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
                // Scrubbing previews fast; release restores quality.
                state_fast.update(cx, |s, cx| {
                    s.checkpoint();
                    s.preview_fast = true;
                    cx.notify();
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
                        // Discrete wheel step = one undoable nudge.
                        s.checkpoint();
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
                                } else if let Some(rest) = other.strip_prefix("mask:") {
                                    // mask:<layer>:<mask>:<param>
                                    let parts: Vec<&str> = rest.split(':').collect();
                                    if parts.len() >= 3 {
                                        let _ = s.nudge_mask_param(parts[0], parts[1], parts[2], step);
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

/// After Effects-style blue numeric scrubbable value label with drag-scrubbing, scroll-wheel nudging, and click-to-type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ae_blue_scrub_field<FMinus, FPlus>(
    id: impl Into<ElementId>,
    prop_key: String,
    label: String,
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
    let state_fast = state.clone();
    let prop_for_wheel = prop_key.clone();
    let prop_for_edit = prop_key.clone();

    let edit_id = id.into();
    let edit_state = state.read(cx);
    let editor_opt = edit_state.value_editor.clone();
    let is_editing = edit_state.value_edit_key.as_deref() == Some(prop_key.as_str());

    let value_child: AnyElement = match (is_editing, editor_opt) {
        (true, Some(editor)) => Input::new(&editor)
            .id(edit_id.clone())
            .w(px(70.))
            .into_any_element(),
        _ => div()
            .id(edit_id)
            .test_support()
            .px_1p5()
            .py_0p5()
            .text_color(rgb(0x3b82f6))
            .hover(|s| s.text_color(rgb(0x60a5fa)).bg(rgba(0x3b82f620)))
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
                state_fast.update(cx, |s, cx| {
                    s.checkpoint();
                    s.preview_fast = true;
                    cx.notify();
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
                        s.checkpoint();
                        if pk == "font_size" {
                            if let Some(lid) = s.selected_layer_id.clone() {
                                let _ = s.nudge_layer_font_size(&lid, step);
                            }
                        } else if pk == "font_weight" || pk == "text_weight" {
                            if let Some(lid) = s.selected_layer_id.clone() {
                                let cur = match s.selected_layer().map(|l| &l.source) {
                                    Some(LayerSource::Text { weight, .. }) => *weight as f32,
                                    _ => 400.0,
                                };
                                let _ = s.set_layer_font_weight(&lid, (cur + step * 50.0).clamp(100.0, 900.0).round() as u16);
                            }
                        } else if let Some((head, _)) = pk.split_once(':') {
                            if let Some(lid) = s.selected_layer_id.clone() {
                                let field = match head {
                                    "text_tracking" => "tracking",
                                    "text_leading" => "leading",
                                    "text_stroke_w" => "stroke_width",
                                    "text_baseline" => "baseline_shift",
                                    "text_box_h" => "box_height",
                                    _ => "box_width",
                                };
                                let cur = s.scrub_current_value(head).unwrap_or(0.0);
                                let _ = s.set_layer_text_scalar(&lid, field, cur + step);
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
        .items_center()
        .child(value_child)
}

/// Swatch supporting 3 modes: None (white box with red diagonal slash), Solid Color, or Gradient.
pub(crate) fn render_color_swatch<F>(
    id: impl Into<ElementId>,
    color: Color,
    mode: &str,
    grad_colors: (Color, Color),
    is_active: bool,
    cx: &App,
    on_click: F,
) -> AnyElement
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let swatch_id = id.into();
    let border_col: Hsla = if is_active {
        cx.theme().primary
    } else {
        cx.theme().border
    };

    let inner: AnyElement = match mode {
        "none" => {
            let mut slash = div()
                .relative()
                .w(px(32.))
                .h(px(20.))
                .rounded_sm()
                .bg(rgb(0xffffff))
                .border_1()
                .border_color(border_col)
                .overflow_hidden();
            for i in 0..14 {
                let x = i as f32 * 2.3;
                let y = i as f32 * 1.45;
                slash = slash.child(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(3.5))
                        .h(px(2.5))
                        .bg(rgb(0xef4444)),
                );
            }
            slash.into_any_element()
        }
        "gradient" => {
            let (ca, cb) = grad_colors;
            let mut bar = h_flex()
                .w(px(32.))
                .h(px(20.))
                .rounded_sm()
                .overflow_hidden()
                .border_1()
                .border_color(border_col);
            for i in 0..16 {
                let t = i as f32 / 15.0;
                bar = bar.child(
                    div().flex_1().h_full().bg(Rgba {
                        r: ca.r + (cb.r - ca.r) * t,
                        g: ca.g + (cb.g - ca.g) * t,
                        b: ca.b + (cb.b - ca.b) * t,
                        a: 1.0,
                    }),
                );
            }
            bar.into_any_element()
        }
        _ => {
            div()
                .w(px(32.))
                .h(px(20.))
                .rounded_sm()
                .bg(Rgba {
                    r: color.r,
                    g: color.g,
                    b: color.b,
                    a: if color.a <= 0.0 { 1.0 } else { color.a },
                })
                .border_1()
                .border_color(border_col)
                .into_any_element()
        }
    };

    div()
        .id(swatch_id)
        .test_support()
        .cursor_pointer()
        .on_click(on_click)
        .child(inner)
        .into_any_element()
}

/// Three-mode color picker dialog / popover: None, Color, Gradient.
pub(crate) fn render_three_mode_color_picker(
    key: &str,
    current_color: Color,
    panel_self: &PropertiesPanel,
    panel_entity: &Entity<PropertiesPanel>,
    state: &Entity<EditorState>,
    inspector_color: &Entity<InspectorColorPicker>,
    cx: &App,
) -> Div {
    let mode = panel_self.color_picker_mode.get(key).map(|s| s.as_str()).unwrap_or_else(|| {
        if current_color.a <= 0.0 {
            "none"
        } else {
            "color"
        }
    });
    let grad_colors = panel_self.color_picker_gradient_colors.get(key).copied().unwrap_or((Color::WHITE, Color::BLACK));
    let grad_stop = panel_self.color_picker_gradient_stop.get(key).copied().unwrap_or(0);
    let grad_angle = panel_self.color_picker_gradient_angle.get(key).copied().unwrap_or(0.0);

    let k_none = key.to_string();
    let k_col = key.to_string();
    let k_grad = key.to_string();
    let k_close = key.to_string();

    let p_none = panel_entity.clone();
    let s_none = state.clone();
    let p_col = panel_entity.clone();
    let s_col = state.clone();
    let p_grad = panel_entity.clone();
    let s_grad = state.clone();
    let p_close = panel_entity.clone();

    // Mode tabs: None | Color | Gradient
    let mode_tabs = h_flex()
        .gap_1()
        .items_center()
        .justify_between()
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .id(SharedString::from(format!("{key}_tab_none")))
                        .test_support()
                        .child(
                            Button::new(SharedString::from(format!("{key}_btn_none")))
                                .compact()
                                .selected(mode == "none")
                                .child("None")
                                .on_click(move |_, _, cx| {
                                    let k = k_none.clone();
                                    p_none.update(cx, |this, _| {
                                        this.color_picker_mode.insert(k.clone(), "none".to_string());
                                    });
                                    s_none.update(cx, |s, cx| {
                                        if let Some(lid) = s.selected_layer_id.clone() {
                                            match k.as_str() {
                                                "text_fill" => { let _ = s.set_layer_text_color(&lid, Color::TRANSPARENT); }
                                                "text_stroke" => { let _ = s.set_layer_text_scalar(&lid, "stroke_width", 0.0); }
                                                "solid_color" => { let _ = s.set_layer_solid_color(&lid, Color::TRANSPARENT); }
                                                "shape_fill" => { let _ = s.set_layer_shape_fill(&lid, Color::TRANSPARENT); }
                                                _ => {}
                                            }
                                            cx.notify();
                                        }
                                    });
                                }),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{key}_tab_color")))
                        .test_support()
                        .child(
                            Button::new(SharedString::from(format!("{key}_btn_color")))
                                .compact()
                                .selected(mode == "color")
                                .child("Color")
                                .on_click(move |_, _, cx| {
                                    let k = k_col.clone();
                                    p_col.update(cx, |this, _| {
                                        this.color_picker_mode.insert(k.clone(), "color".to_string());
                                    });
                                    s_col.update(cx, |s, cx| {
                                        if let Some(lid) = s.selected_layer_id.clone() {
                                            match k.as_str() {
                                                "text_fill" => { let _ = s.set_layer_text_color(&lid, Color::WHITE); }
                                                "text_stroke" => {
                                                    let _ = s.set_layer_text_scalar(&lid, "stroke_width", 2.0);
                                                    let _ = s.set_layer_stroke_color(&lid, Color::WHITE);
                                                }
                                                "solid_color" => { let _ = s.set_layer_solid_color(&lid, Color::WHITE); }
                                                "shape_fill" => { let _ = s.set_layer_shape_fill(&lid, Color::WHITE); }
                                                _ => {}
                                            }
                                            cx.notify();
                                        }
                                    });
                                }),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{key}_tab_gradient")))
                        .test_support()
                        .child(
                            Button::new(SharedString::from(format!("{key}_btn_gradient")))
                                .compact()
                                .selected(mode == "gradient")
                                .child("Gradient")
                                .on_click(move |_, _, cx| {
                                    let k = k_grad.clone();
                                    p_grad.update(cx, |this, _| {
                                        this.color_picker_mode.insert(k.clone(), "gradient".to_string());
                                    });
                                    s_grad.update(cx, |s, cx| {
                                        if let Some(lid) = s.selected_layer_id.clone() {
                                            match k.as_str() {
                                                "text_fill" => { let _ = s.set_layer_text_color(&lid, Color::from_hex("#3B82F6").unwrap()); }
                                                "text_stroke" => {
                                                    let _ = s.set_layer_text_scalar(&lid, "stroke_width", 2.0);
                                                    let _ = s.set_layer_stroke_color(&lid, Color::from_hex("#3B82F6").unwrap());
                                                }
                                                _ => {}
                                            }
                                            cx.notify();
                                        }
                                    });
                                }),
                        ),
                ),
        )
        .child(
            div()
                .cursor_pointer()
                .px_1p5()
                .py_0p5()
                .rounded_sm()
                .hover(|s| s.bg(cx.theme().muted))
                .child("✕")
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    p_close.update(cx, |this, cx| {
                        if this.active_color_picker.as_deref() == Some(k_close.as_str()) {
                            this.active_color_picker = None;
                        }
                        cx.notify();
                    });
                }),
        );

    let content = match mode {
        "none" => {
            v_flex()
                .gap_2()
                .p_2()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("No Color / Transparent mode active"),
                )
                .into_any_element()
        }
        "gradient" => {
            let (ca, cb) = grad_colors;
            let mut bar = h_flex().flex_1().h(px(18.)).rounded_sm().overflow_hidden();
            for i in 0..24 {
                let t = i as f32 / 23.0;
                bar = bar.child(
                    div().flex_1().h_full().bg(Rgba {
                        r: ca.r + (cb.r - ca.r) * t,
                        g: ca.g + (cb.g - ca.g) * t,
                        b: ca.b + (cb.b - ca.b) * t,
                        a: 1.0,
                    }),
                );
            }

            let p_s0 = panel_entity.clone();
            let p_s1 = panel_entity.clone();
            let k_s0 = key.to_string();
            let k_s1 = key.to_string();

            let mut presets_row = h_flex().gap_1().items_center().flex_wrap();
            for (p_name, c1, c2) in [
                ("Sunset", Color::from_hex("#F59E0B").unwrap(), Color::from_hex("#EC4899").unwrap()),
                ("Ocean", Color::from_hex("#3B82F6").unwrap(), Color::from_hex("#10B981").unwrap()),
                ("Neon", Color::from_hex("#8B5CF6").unwrap(), Color::from_hex("#EC4899").unwrap()),
                ("Mono", Color::WHITE, Color::BLACK),
            ] {
                let p_pr = panel_entity.clone();
                let s_pr = state.clone();
                let k_pr = key.to_string();
                presets_row = presets_row.child(
                    Button::new(SharedString::from(format!("{key}_grad_{p_name}")))
                        .compact()
                        .child(p_name)
                        .on_click(move |_, _, cx| {
                            let k = k_pr.clone();
                            p_pr.update(cx, |this, _| {
                                this.color_picker_gradient_colors.insert(k.clone(), (c1, c2));
                            });
                            s_pr.update(cx, |s, cx| {
                                if let Some(lid) = s.selected_layer_id.clone() {
                                    if k == "text_fill" {
                                        let _ = s.set_layer_text_color(&lid, c1);
                                    } else if k == "text_stroke" {
                                        let _ = s.set_layer_stroke_color(&lid, c1);
                                    }
                                    cx.notify();
                                }
                            });
                        }),
                );
            }

            v_flex()
                .gap_2()
                .p_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(bar)
                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("{grad_angle:.0}°"))),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .cursor_pointer()
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .border_1()
                                .border_color(if grad_stop == 0 { cx.theme().primary } else { cx.theme().border })
                                .child("Stop 1")
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    p_s0.update(cx, |this, cx| {
                                        this.color_picker_gradient_stop.insert(k_s0.clone(), 0);
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            div()
                                .cursor_pointer()
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .border_1()
                                .border_color(if grad_stop == 1 { cx.theme().primary } else { cx.theme().border })
                                .child("Stop 2")
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    p_s1.update(cx, |this, cx| {
                                        this.color_picker_gradient_stop.insert(k_s1.clone(), 1);
                                        cx.notify();
                                    });
                                }),
                        ),
                )
                .child(presets_row)
                .into_any_element()
        }
        _ => {
            // Solid Color mode: GPUI Kit ColorPicker + Swatch Palette + Hex Code
            let hex_str = format!(
                "#{:02X}{:02X}{:02X}",
                (current_color.r * 255.0).round() as u8,
                (current_color.g * 255.0).round() as u8,
                (current_color.b * 255.0).round() as u8
            );

            let mut palette = h_flex().gap_1p5().items_center().flex_wrap();
            for hex in ["#FFFFFF", "#000000", "#EF4444", "#F59E0B", "#10B981", "#3B82F6", "#8B5CF6", "#EC4899"] {
                let col = Color::from_hex(hex).unwrap();
                let s_p = state.clone();
                let k_p = key.to_string();
                let sel = (current_color.r - col.r).abs() < 0.02
                    && (current_color.g - col.g).abs() < 0.02
                    && (current_color.b - col.b).abs() < 0.02;
                palette = palette.child(
                    div()
                        .id(SharedString::from(format!("{key}_palette_{hex}")))
                        .test_support()
                        .cursor_pointer()
                        .w(px(16.))
                        .h(px(16.))
                        .rounded_sm()
                        .bg(Rgba { r: col.r, g: col.g, b: col.b, a: 1.0 })
                        .border_1()
                        .border_color(if sel { cx.theme().primary } else { cx.theme().border })
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            let k = k_p.clone();
                            s_p.update(cx, |s, cx| {
                                if let Some(lid) = s.selected_layer_id.clone() {
                                    match k.as_str() {
                                        "text_fill" => { let _ = s.set_layer_text_color(&lid, col); }
                                        "text_stroke" => { let _ = s.set_layer_stroke_color(&lid, col); }
                                        "solid_color" => { let _ = s.set_layer_solid_color(&lid, col); }
                                        "shape_fill" => { let _ = s.set_layer_shape_fill(&lid, col); }
                                        _ => {}
                                    }
                                    cx.notify();
                                }
                            });
                        }),
                );
            }

            v_flex()
                .gap_2()
                .p_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(ColorPicker::new(&inspector_color.read(cx).state).label("Pick Color"))
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .bg(cx.theme().muted)
                                .text_xs()
                                .child(hex_str),
                        ),
                )
                .child(palette)
                .into_any_element()
        }
    };

    v_flex()
        .gap_1p5()
        .p_2()
        .rounded_md()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().border)
        .child(mode_tabs)
        .child(content)
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
                    "text.tracking" | "text.leading" | "text.stroke_width"
                    | "text.baseline_shift" | "text.box_width" => {
                        let prop = if let LayerSource::Text {
                            ref tracking,
                            ref leading,
                            ref stroke_width,
                            ref baseline_shift,
                            ref box_width,
                            ..
                        } = layer.source
                        {
                            match prop_path {
                                "text.tracking" => Some(tracking),
                                "text.leading" => Some(leading),
                                "text.stroke_width" => Some(stroke_width),
                                "text.baseline_shift" => Some(baseline_shift),
                                _ => Some(box_width),
                            }
                        } else {
                            None
                        };
                        match prop {
                            Some(p) => (
                                p.has_keyframe_at(&current_tc),
                                p.previous_keyframe_time(&current_tc).is_some(),
                                p.next_keyframe_time(&current_tc).is_some(),
                            ),
                            None => (false, false, false),
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

pub(crate) fn effect_param_keyframe_controls(
    state: &Entity<EditorState>,
    layer_id: &str,
    eff_id: &str,
    param_name: &str,
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

/// Compact color-swatch row for effect color fields (`color_a` / `color_b`
/// / `color`): preset swatches + current hex. Used by checker, gradient,
/// and outline arms (no extra ColorPicker plumbing).
/// Color field names of one effect for wheel-picker creation (legacy
/// color slots + stock generator slots). Mirrors the fx_swatch_row call
/// sites so every color param gets a wheel, not just presets.
fn fx_color_fields(effect: &project::Effect) -> Vec<&'static str> {
    match &effect.effect_type {
        EffectType::Tint { .. } => vec!["map_black", "map_white"],
        EffectType::DropShadow { .. } => vec!["color"],
        EffectType::ChromaKey { .. } => vec!["key_color"],
        EffectType::Checkerboard { .. } => vec!["color_a", "color_b"],
        EffectType::GradientRamp { .. } => vec!["color_a", "color_b"],
        EffectType::TextOutline { .. } => vec!["color"],
        _ => effect.color_slots(),
    }
}

pub(crate) fn fx_swatch_row(
    state: &Entity<EditorState>,
    layer_id: &str,
    eff_id: &str,
    field: &str,
    label: &str,
    current: Color,
    wheel: Option<&Entity<InspectorColorPicker>>,
    cx: &App,
) -> AnyElement {
    let field_owned = field.to_string();
    let mut row = h_flex().gap_1().items_center();
    for (hex_str, col_val) in [
        ("#FFFFFF", Color::WHITE),
        ("#121316", Color::from_hex("#121316").unwrap()),
        ("#EF4444", Color::from_hex("#EF4444").unwrap()),
        ("#10B981", Color::from_hex("#10B981").unwrap()),
        ("#3B82F6", Color::from_hex("#3B82F6").unwrap()),
        ("#F59E0B", Color::from_hex("#F59E0B").unwrap()),
        ("#00FF00", Color::from_hex("#00FF00").unwrap()),
        ("#0000FF", Color::from_hex("#0000FF").unwrap()),
    ] {
        let s_p = state.clone();
        let lid_p = layer_id.to_string();
        let eid_p = eff_id.to_string();
        let fld = field_owned.clone();
        let is_sel = (current.r - col_val.r).abs() < 0.01
            && (current.g - col_val.g).abs() < 0.01
            && (current.b - col_val.b).abs() < 0.01;
        row = row.child(
            div()
                .id(SharedString::from(format!("fx_{field}_{hex_str}_{eff_id}")))
                .test_support()
                .cursor_pointer()
                .w(px(14.))
                .h(px(14.))
                .rounded_sm()
                .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                .border_1()
                .border_color(if is_sel { cx.theme().primary } else { cx.theme().border })
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    s_p.update(cx, |s, cx| {
                        let _ = s.set_effect_color(&lid_p, &eid_p, &fld, col_val);
                        cx.notify();
                    });
                }),
        );
    }
    let cur_hex = format!(
        "#{:02X}{:02X}{:02X}",
        (current.r * 255.0) as u8,
        (current.g * 255.0) as u8,
        (current.b * 255.0) as u8
    );
    // Full color wheel for arbitrary custom colors (same control as the
    // solid/text inspectors). The keyed picker state is created by the
    // Properties render before the state read-guard (see fx wheels).
    let wheel_el: AnyElement = match wheel {
        Some(picker) => fx_wheel_el(field, eff_id, picker, cx),
        None => div().into_any_element(),
    };
    h_flex()
        .items_center()
        .justify_between()
        .text_xs()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(div().text_color(cx.theme().muted_foreground).child(label.to_string()))
                .child(
                    div()
                        .w(px(20.))
                        .h(px(14.))
                        .rounded_sm()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(Rgba { r: current.r, g: current.g, b: current.b, a: 1.0 }),
                )
                .child(div().text_color(cx.theme().foreground).child(cur_hex))
                .child(wheel_el),
        )
        .child(row)
        .into_any_element()
}

/// Effect color wheel element (shared by swatch rows and the gradient
/// editor): full color wheel with the stable `fx_wheel_btn_` id.
pub(crate) fn fx_wheel_el(
    field: &str,
    eff_id: &str,
    picker: &Entity<InspectorColorPicker>,
    cx: &App,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("fx_wheel_btn_{field}_{eff_id}")))
        .test_support()
        .child(ColorPicker::new(&picker.read(cx).state).label("Pick"))
        .into_any_element()
}

/// Toolbar tool settings (Properties > Tool Settings): defaults that new
/// layers are created with, so tools behave consistently across the app.
/// Pure preset chips/swatches — no steppers anywhere.
fn render_tool_settings(state: &Entity<EditorState>, cx: &App) -> AnyElement {
    let (tool, font_size, text_color, shape_fill, solid_color, rotate_step) = {
        let s = state.read(cx);
        (
            s.active_tool,
            s.tool_font_size,
            s.tool_text_color,
            s.tool_shape_fill,
            s.tool_solid_color,
            s.tool_rotate_step,
        )
    };
    let (tool_name, tool_icon) = match tool {
        EditorTool::Move => ("Move", IconName::Move),
        EditorTool::Hand => ("Hand", IconName::Hand),
        EditorTool::Rotate => ("Rotate", IconName::RotateCw),
        EditorTool::Pen => ("Pen", IconName::Pen),
        EditorTool::Text => ("Text", IconName::Type),
        EditorTool::ShapeRect => ("Rectangle", IconName::Square),
        EditorTool::ShapeEllipse => ("Ellipse", IconName::Circle),
    };
    // Option chip (selectable value).
    let chip = |id: String, label: String, selected: bool, cx: &App| {
        div()
            .id(SharedString::from(id))
            .test_support()
            .cursor_pointer()
            .px_1p5()
            .py_0p5()
            .rounded_sm()
            .bg(if selected { cx.theme().primary } else { cx.theme().muted })
            .text_color(if selected {
                cx.theme().primary_foreground
            } else {
                cx.theme().foreground
            })
            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
            .text_xs()
            .child(label)
    };
    // Color swatch (current value ring + presets live with the caller).
    let swatch = |id: String, col: Color, selected: bool, cx: &App| {
        div()
            .id(SharedString::from(id))
            .test_support()
            .cursor_pointer()
            .w(px(14.))
            .h(px(14.))
            .rounded_sm()
            .bg(Rgba { r: col.r, g: col.g, b: col.b, a: 1.0 })
            .border_1()
            .border_color(if selected { cx.theme().primary } else { cx.theme().border })
    };
    let palette: [(&str, Color); 8] = [
        ("#FFFFFF", Color::WHITE),
        ("#121316", Color::from_hex("#121316").unwrap()),
        ("#EF4444", Color::from_hex("#EF4444").unwrap()),
        ("#10B981", Color::from_hex("#10B981").unwrap()),
        ("#3B82F6", Color::from_hex("#3B82F6").unwrap()),
        ("#8B5CF6", Color::from_hex("#8B5CF6").unwrap()),
        ("#F59E0B", Color::from_hex("#F59E0B").unwrap()),
        ("#06B6D4", Color::from_hex("#06B6D4").unwrap()),
    ];
    let same_color = |a: Color, b: Color| {
        (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
    };

    let mut body = v_flex().gap_2();
    match tool {
        EditorTool::Text => {
            let mut sizes = h_flex().gap_1().items_center().flex_wrap();
            for sz in [24.0f32, 36.0, 48.0, 72.0, 96.0] {
                let s_sz = state.clone();
                let sel = (font_size - sz).abs() < 0.5;
                sizes = sizes.child(
                    chip(format!("tool_font_size_{sz:.0}"), format!("{sz:.0}"), sel, cx)
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_sz.update(cx, |s, cx| {
                                s.tool_font_size = sz;
                                cx.notify();
                            });
                        }),
                );
            }
            let mut cols = h_flex().gap_1().items_center().flex_wrap();
            for (hex_str, col_val) in palette {
                let s_c = state.clone();
                let sel = same_color(text_color, col_val);
                cols = cols.child(
                    swatch(format!("tool_text_color_{hex_str}"), col_val, sel, cx)
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_c.update(cx, |s, cx| {
                                s.tool_text_color = col_val;
                                cx.notify();
                            });
                        }),
                );
            }
            body = body
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Default size for new text"))
                .child(sizes)
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Default color for new text"))
                .child(cols)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Font family follows your system default; pick any installed family in the Text section below."),
                );
        }
        EditorTool::ShapeRect | EditorTool::ShapeEllipse => {
            let mut cols = h_flex().gap_1().items_center().flex_wrap();
            for (hex_str, col_val) in palette {
                let s_c = state.clone();
                let sel = same_color(shape_fill, col_val);
                cols = cols.child(
                    swatch(format!("tool_shape_fill_{hex_str}"), col_val, sel, cx)
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_c.update(cx, |s, cx| {
                                s.tool_shape_fill = col_val;
                                cx.notify();
                            });
                        }),
                );
            }
            body = body
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Default fill for new shapes"))
                .child(cols)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Click the canvas to place at the cursor, or + in the toolbar for viewport center."),
                );
        }
        EditorTool::Rotate => {
            let mut steps = h_flex().gap_1().items_center().flex_wrap();
            for st in [1.0f32, 5.0, 15.0, 45.0] {
                let s_s = state.clone();
                let sel = (rotate_step - st).abs() < 1e-4;
                steps = steps.child(
                    chip(format!("tool_rotate_step_{st:.0}"), format!("{st:.0}°"), sel, cx)
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_s.update(cx, |s, cx| {
                                s.tool_rotate_step = st;
                                cx.notify();
                            });
                        }),
                );
            }
            body = body
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Degrees per canvas click"))
                .child(steps)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Drag the blue handle above a selected layer for free rotation about its pivot."),
                );
        }
        EditorTool::Move => {
            let mut cols = h_flex().gap_1().items_center().flex_wrap();
            for (hex_str, col_val) in palette {
                let s_c = state.clone();
                let sel = same_color(solid_color, col_val);
                cols = cols.child(
                    swatch(format!("tool_solid_color_{hex_str}"), col_val, sel, cx)
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_c.update(cx, |s, cx| {
                                s.tool_solid_color = col_val;
                                cx.notify();
                            });
                        }),
                );
            }
            body = body
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Default color for quick-add solids"))
                .child(cols)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Drag layer bodies to move • corners/edges scale • top handle rotates • amber diamond moves the pivot."),
                );
        }
        _ => {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(match tool {
                        EditorTool::Hand => "Drag the canvas to pan the view.",
                        EditorTool::Pen => "Click the canvas to drop path points; switch tools to finish.",
                        _ => "Canvas tool active.",
                    }),
            );
        }
    }

    v_flex()
        .id("tool_settings_section")
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
                .child(icon_box(tool_icon))
                .child(format!("Tool Settings ({tool_name})")),
        )
        .child(body)
        .into_any_element()
}

/// Collapsible inspector section card (Properties accordion) built on the
/// gpui-kit `Collapsible` primitive: header goes via `.child`, body via
/// `.content`, controlled with `.open`. Only the header always renders.
fn prop_section<F>(
    id: &'static str,
    title: String,
    icon: IconName,
    expanded: bool,
    on_toggle: F,
    _cx: &App,
    body: Option<AnyElement>,
) -> AnyElement
where
    F: Fn(&mut App) + 'static,
{
    let header_el = h_flex()
        .id(SharedString::from(format!("props_section_{id}")))
        .test_support()
        .gap_1p5()
        .items_center()
        .font_semibold()
        .text_xs()
        .text_color(ae::dim())
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            on_toggle(cx);
        })
        .child(div().w(px(10.)).child(if expanded { "▾" } else { "▸" }))
        .child(icon_box(icon))
        .child(title.to_uppercase())
        .into_any_element();
    let chrome = v_flex()
        .p_2p5()
        .rounded_sm()
        .border_b_1()
        .border_color(ae::border())
        .bg(ae::panel())
        .gap_2()
        .child(header_el)
        .into_any_element();
    let content = body.filter(|_| expanded).unwrap_or_else(|| div().into_any_element());
    Collapsible::new()
        .open(expanded)
        .child(chrome)
        .content(content)
        .into_any_element()
}

/// Nested twirl-down group inside an open card (applied-effect parameter
/// groups, AE Character / Paragraph groups, Shader Lab groups). The caret
/// pill is the only toggle hit-target so sibling controls (eye, trash,
/// presets) never collapse the group. Body mounts only while `open`.
fn nested_group<F>(
    toggle_id: SharedString,
    title: AnyElement,
    open: bool,
    on_toggle: F,
    content: AnyElement,
) -> AnyElement
where
    F: Fn(&mut App) + 'static,
{
    let header = h_flex()
        .gap_1p5()
        .items_center()
        .text_xs()
        .child(
            div()
                .id(toggle_id)
                .test_support()
                .cursor_pointer()
                .px_1()
                .text_color(ae::dim())
                .hover(|s| s.text_color(ae::text()))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    on_toggle(cx);
                })
                .child(if open { "▾" } else { "▸" }),
        )
        .child(title);
    let body = if open { content } else { div().into_any_element() };
    Collapsible::new()
        .open(open)
        .child(header.into_any_element())
        .content(body)
        .into_any_element()
}

/// Compact scalar row for mask params: stopwatch, label, drag/wheel/type
/// scrub field. Param keys route through the `mask:` scrub prefix.
#[allow(clippy::too_many_arguments)]
fn mask_param_row(
    state: &Entity<EditorState>,
    panel_entity: &Entity<PropertiesPanel>,
    layer_id: &str,
    mask_id: &str,
    param_name: &'static str,
    label: &str,
    display: String,
    step: f32,
    cx: &App,
) -> AnyElement {
    let animated = state
        .read(cx)
        .active_composition()
        .and_then(|c| c.get_layer(layer_id))
        .and_then(|l| l.get_mask(mask_id))
        .and_then(|m| m.get_param_property(param_name))
        .map(|p| p.is_animated())
        .unwrap_or(false);
    let s_t = state.clone();
    let lid_t = layer_id.to_string();
    let mid_t = mask_id.to_string();
    let mult = (step * 100.0).round().max(1.0) as i64;
    h_flex()
        .items_center()
        .justify_between()
        .text_xs()
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .cursor_pointer()
                        .p_0p5()
                        .rounded_sm()
                        .hover(|s| s.bg(cx.theme().muted))
                        .text_color(if animated {
                            cx.theme().primary
                        } else {
                            cx.theme().muted_foreground
                        })
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_t.update(cx, |s, cx| {
                                let _ = s.toggle_mask_param_animation(&lid_t, &mid_t, param_name);
                                cx.notify();
                            });
                        })
                        .child(icon_box(IconName::Timer)),
                )
                .child(div().text_color(cx.theme().muted_foreground).child(label.to_string())),
        )
        .child(scrub_field(
            SharedString::from(format!("param_mask_{param_name}_{mask_id}")),
            format!("mask:{layer_id}:{mask_id}:{param_name}:{mult}"),
            display,
            None,
            None,
            state,
            panel_entity,
            cx,
            move |_| {},
            move |_| {},
        ))
        .into_any_element()
}

/// Compact scalar row for a keyframable text property: stopwatch (toggles
/// and navigates keyframes), label, and an AE-style scrub/click-type field.
#[allow(clippy::too_many_arguments, dead_code)]
fn text_param_row(
    state: &Entity<EditorState>,
    panel_entity: &Entity<PropertiesPanel>,
    layer_id: &str,
    prop_path: &'static str,
    scrub_key: &'static str,
    label: &str,
    display: String,
    step: f32,
    animated: bool,
    cx: &App,
) -> AnyElement {
    let mult = (step * 100.0).round().max(1.0) as i64;
    h_flex()
        .items_center()
        .justify_between()
        .text_xs()
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(property_stopwatch(state, layer_id, prop_path, animated, cx))
                .child(div().text_color(cx.theme().muted_foreground).child(label.to_string())),
        )
        .child(scrub_field(
            SharedString::from(format!("text_param_{scrub_key}")),
            format!("{scrub_key}:{mult}"),
            display,
            None,
            None,
            state,
            panel_entity,
            cx,
            move |_| {},
            move |_| {},
        ))
        .into_any_element()
}

/// Masks card body for the Properties panel: creation toolbar (AE parity:
/// numeric shapes, full-layer, shape/motion/text/trace sources), per-mask
/// enable, lock, rename, combine mode, invert, edit target, path keyframe
/// nav, scalar rows, shape dialog, delete.
fn render_masks_section(
    state: &Entity<EditorState>,
    layer: &project::Layer,
    panel_entity: &Entity<PropertiesPanel>,
    shape_view: Option<MaskShapeView>,
    rename_view: Option<MaskRenameView>,
    trace_open: bool,
    trace_opts: &project::AutoTraceOptions,
    trace_error: Option<String>,
    shape_clipboard: Option<String>,
    cx: &App,
) -> AnyElement {
    let lid = layer.id.clone();
    let mut col = v_flex().gap_2();
    if layer.masks.is_empty() {
        col = col.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("No masks. Add one, then edit its nodes in the viewport Path Editor."),
        );
    }
    let is_shape_layer = matches!(&layer.source, project::LayerSource::Shape { .. });
    let is_text_layer = matches!(&layer.source, project::LayerSource::Text { .. });

    // --- Creation toolbar (AE parity: Layer > Mask creation methods) ---
    {
        let s_mask = state.clone();
        let s_rect = state.clone();
        let s_ellipse = state.clone();
        let s_full = state.clone();
        let lid_mask = lid.clone();
        let lid_rect = lid.clone();
        let lid_ellipse = lid.clone();
        let lid_full = lid.clone();
        let p_rect = panel_entity.clone();
        let p_ellipse = panel_entity.clone();
        col = col.child(
            h_flex()
                .gap_1()
                .items_center()
                .text_xs()
                .child(
                    div()
                        .id("mask_add_button")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_mask.update(cx, |s, cx| {
                                if let Ok(mid) = s.add_mask_to_layer(&lid_mask) {
                                    s.set_active_mask_edit(Some((lid_mask.clone(), mid)));
                                }
                                cx.notify();
                            });
                        })
                        .child("+ Mask"),
                )
                .child(
                    div()
                        .id("mask_add_rect")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                            let mid = s_rect.update(cx, |s, cx| {
                                let mid = s.add_shaped_mask(&lid_rect, project::MaskShapeKind::Rectangle).ok();
                                if let Some(ref m) = mid {
                                    s.set_active_mask_edit(Some((lid_rect.clone(), m.clone())));
                                }
                                cx.notify();
                                mid
                            });
                            if let Some(m) = mid {
                                p_rect.update(cx, |this, cx| {
                                    this.open_mask_shape_dialog(&lid_rect, &m, project::MaskShapeKind::Rectangle, window, cx);
                                });
                            }
                        })
                        .child("+ Rect"),
                )
                .child(
                    div()
                        .id("mask_add_ellipse")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                            let mid = s_ellipse.update(cx, |s, cx| {
                                let mid = s.add_shaped_mask(&lid_ellipse, project::MaskShapeKind::Ellipse).ok();
                                if let Some(ref m) = mid {
                                    s.set_active_mask_edit(Some((lid_ellipse.clone(), m.clone())));
                                }
                                cx.notify();
                                mid
                            });
                            if let Some(m) = mid {
                                p_ellipse.update(cx, |this, cx| {
                                    this.open_mask_shape_dialog(&lid_ellipse, &m, project::MaskShapeKind::Ellipse, window, cx);
                                });
                            }
                        })
                        .child("+ Ellipse"),
                )
                .child(
                    div()
                        .id("mask_add_full")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_full.update(cx, |s, cx| {
                                if let Ok(mid) = s.add_layer_sized_mask(&lid_full) {
                                    s.set_active_mask_edit(Some((lid_full.clone(), mid)));
                                }
                                cx.notify();
                            });
                        })
                        .child("+ Full"),
                ),
        );
    }
    // --- Source toolbar: shape / motion / text / trace (AE parity) ---
    {
        let s_paste = state.clone();
        let s_motion = state.clone();
        let s_text = state.clone();
        let lid_paste = lid.clone();
        let lid_motion = lid.clone();
        let lid_text = lid.clone();
        let lid_copy = lid.clone();
        let p_trace = panel_entity.clone();
        let p_copy = panel_entity.clone();
        let clipboard_src = shape_clipboard.clone();
        let mut row = h_flex().gap_1().items_center().text_xs();
        // Cross-layer shape copy: arm on a shape layer, paste anywhere.
        if is_shape_layer {
            let armed = clipboard_src.as_deref() == Some(lid_copy.as_str());
            row = row.child(
                div()
                    .id("mask_copy_shape")
                    .test_support()
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(if armed { cx.theme().primary } else { cx.theme().muted })
                    .text_color(if armed { cx.theme().primary_foreground } else { cx.theme().foreground })
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_copy.update(cx, |this, cx| {
                            this.shape_clipboard = if armed { None } else { Some(lid_copy.clone()) };
                            cx.notify();
                        });
                    })
                    .child(if armed { "Shape Armed (Clear)" } else { "Copy Shape" }),
            );
        }
        if let Some(src) = clipboard_src {
            if src != lid {
                row = row.child(
                    div()
                        .id("mask_paste_shape")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_paste.update(cx, |s, cx| {
                                if let Ok(mid) = s.shape_path_to_mask(&src, &lid_paste) {
                                    s.set_active_mask_edit(Some((lid_paste.clone(), mid)));
                                }
                                cx.notify();
                            });
                        })
                        .child("Paste Shape→Mask"),
                );
            }
        }
        row = row
            .child(
                div()
                    .id("mask_from_motion")
                    .test_support()
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        s_motion.update(cx, |s, cx| {
                            if let Ok(mid) = s.motion_path_to_mask(&lid_motion, &lid_motion) {
                                s.set_active_mask_edit(Some((lid_motion.clone(), mid)));
                            }
                            cx.notify();
                        });
                    })
                    .child("Motion→Mask"),
            )
            .child(
                div()
                    .id("mask_trace_toggle")
                    .test_support()
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(if trace_open { cx.theme().primary } else { cx.theme().muted })
                    .text_color(if trace_open { cx.theme().primary_foreground } else { cx.theme().foreground })
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        p_trace.update(cx, |this, cx| {
                            this.trace_open = !this.trace_open;
                            cx.notify();
                        });
                    })
                    .child("Trace…"),
            );
        if is_text_layer {
            row = row.child(
                div()
                    .id("mask_from_text")
                    .test_support()
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                        s_text.update(cx, |s, cx| {
                            let _ = s.create_masks_from_text(&lid_text);
                            cx.notify();
                        });
                    })
                    .child("Text→Masks"),
            );
        }
        col = col.child(row);
    }
    // --- Auto-trace block (AE Layer > Auto-trace dialog parity) ---
    if trace_open {
        let p_chan = panel_entity.clone();
        let p_thr_m = panel_entity.clone();
        let p_thr_p = panel_entity.clone();
        let p_tol_m = panel_entity.clone();
        let p_tol_p = panel_entity.clone();
        let p_area_m = panel_entity.clone();
        let p_area_p = panel_entity.clone();
        let p_rnd_m = panel_entity.clone();
        let p_rnd_p = panel_entity.clone();
        let p_inv = panel_entity.clone();
        let p_new = panel_entity.clone();
        let p_range = panel_entity.clone();
        let p_run = panel_entity.clone();
        let s_run = state.clone();
        let lid_run = lid.clone();
        let opts_run = trace_opts.clone();
        // Small -/value/+ stepper (mutates panel.trace_opts directly).
        fn trace_stepper(
            _id_prefix: &str,
            label: &str,
            value: String,
            minus: Entity<PropertiesPanel>,
            plus: Entity<PropertiesPanel>,
            apply: fn(&mut project::AutoTraceOptions, f32),
            step: f32,
            cx: &App,
        ) -> AnyElement {
            let m = minus.clone();
            let p = plus.clone();
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .w(px(78.))
                        .text_color(cx.theme().muted_foreground)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .px_1p5()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent))
                        .text_xs()
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            m.update(cx, |this, cx| {
                                apply(&mut this.trace_opts, -step);
                                cx.notify();
                            });
                        })
                        .child("−"),
                )
                .child(
                    div()
                        .text_xs()
                        .min_w(px(52.))
                        .items_center()
                        .justify_center()
                        .flex()
                        .child(value),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .px_1p5()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent))
                        .text_xs()
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            p.update(cx, |this, cx| {
                                apply(&mut this.trace_opts, step);
                                cx.notify();
                            });
                        })
                        .child("+"),
                )
                .into_any_element()
        }
        let mut tcol = v_flex()
            .id("mask_trace_block")
            .test_support()
            .p_2()
            .gap_1()
            .rounded_sm()
            .bg(cx.theme().secondary);
        tcol = tcol.child(
            h_flex()
                .gap_1()
                .items_center()
                .text_xs()
                .child(
                    div()
                        .text_xs()
                        .w(px(78.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Channel"),
                )
                .child(
                    div()
                        .id("mask_trace_channel")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            p_chan.update(cx, |this, cx| {
                                this.trace_opts.channel = this.trace_opts.channel.cycle();
                                cx.notify();
                            });
                        })
                        .child(trace_opts.channel.label().to_string()),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            p_range.update(cx, |this, cx| {
                                this.trace_opts.range = match this.trace_opts.range {
                                    project::TraceRange::CurrentFrame => project::TraceRange::WorkArea,
                                    project::TraceRange::WorkArea => project::TraceRange::CurrentFrame,
                                };
                                cx.notify();
                            });
                        })
                        .child(trace_opts.range.label().to_string()),
                ),
        );
        tcol = tcol
            .child(trace_stepper(
                "thr",
                "Threshold",
                format!("{:.0}%", trace_opts.threshold_pct),
                p_thr_m,
                p_thr_p,
                |o, d| o.threshold_pct = (o.threshold_pct + d).clamp(0.0, 100.0),
                5.0,
                cx,
            ))
            .child(trace_stepper(
                "tol",
                "Tolerance",
                format!("{:.1} px", trace_opts.tolerance_px),
                p_tol_m,
                p_tol_p,
                |o, d| o.tolerance_px = (o.tolerance_px + d).max(0.0),
                0.5,
                cx,
            ))
            .child(trace_stepper(
                "area",
                "Min Area",
                format!("{:.0} px", trace_opts.min_area_px),
                p_area_m,
                p_area_p,
                |o, d| o.min_area_px = (o.min_area_px + d).max(0.0),
                4.0,
                cx,
            ))
            .child(trace_stepper(
                "rnd",
                "Roundness",
                format!("{:.0}%", trace_opts.corner_roundness),
                p_rnd_m,
                p_rnd_p,
                |o, d| o.corner_roundness = (o.corner_roundness + d).clamp(0.0, 100.0),
                10.0,
                cx,
            ));
        tcol = tcol.child(
            h_flex()
                .gap_1()
                .items_center()
                .text_xs()
                .child(
                    div()
                        .cursor_pointer()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(if trace_opts.invert { cx.theme().primary } else { cx.theme().muted })
                        .text_color(if trace_opts.invert { cx.theme().primary_foreground } else { cx.theme().foreground })
                        .hover(|s| s.bg(cx.theme().accent))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            p_inv.update(cx, |this, cx| {
                                this.trace_opts.invert = !this.trace_opts.invert;
                                cx.notify();
                            });
                        })
                        .child(if trace_opts.invert { "Inverted" } else { "Invert" }),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(if trace_opts.apply_to_new_layer { cx.theme().primary } else { cx.theme().muted })
                        .text_color(if trace_opts.apply_to_new_layer { cx.theme().primary_foreground } else { cx.theme().foreground })
                        .hover(|s| s.bg(cx.theme().accent))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            p_new.update(cx, |this, cx| {
                                this.trace_opts.apply_to_new_layer = !this.trace_opts.apply_to_new_layer;
                                cx.notify();
                            });
                        })
                        .child(if trace_opts.apply_to_new_layer { "New Layer: On" } else { "New Layer: Off" }),
                )
                .child(
                    div()
                        .id("mask_trace_run")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(cx.theme().primary)
                        .text_color(cx.theme().primary_foreground)
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            let res = s_run.update(cx, |s, cx| {
                                let r = s.auto_trace_masks(&lid_run, &opts_run);
                                cx.notify();
                                r
                            });
                            match res {
                                Ok(ids) => {
                                    if let Some(first) = ids.first() {
                                        let lid_a = lid_run.clone();
                                        let mid_a = first.clone();
                                        s_run.update(cx, |s, cx| {
                                            s.set_active_mask_edit(Some((lid_a, mid_a)));
                                            cx.notify();
                                        });
                                    }
                                    p_run.update(cx, |this, cx| {
                                        this.trace_error = None;
                                        cx.notify();
                                    });
                                }
                                Err(e) => {
                                    p_run.update(cx, |this, cx| {
                                        this.trace_error = Some(e);
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .child("Run Trace"),
                ),
        );
        if let Some(err) = trace_error {
            tcol = tcol.child(
                div()
                    .text_xs()
                    .text_color(rgb(0xef4444))
                    .child(err),
            );
        }
        col = col.child(tcol);
    }
    for mask in &layer.masks {
        let mid = mask.id.clone();
        let is_editing = state.read(cx).active_mask_edit
            == Some((lid.clone(), mid.clone()));
        let s_toggle = state.clone();
        let s_mode = state.clone();
        let s_inv = state.clone();
        let s_del = state.clone();
        let s_edit = state.clone();
        let s_add = state.clone();
        let s_k = state.clone();
        let s_p = state.clone();
        let s_n = state.clone();
        let s_lock = state.clone();
        let lid_t = lid.clone();
        let mid_t = mid.clone();
        let lid_m = lid.clone();
        let mid_m = mid.clone();
        let lid_i = lid.clone();
        let mid_i = mid.clone();
        let lid_d = lid.clone();
        let mid_d = mid.clone();
        let lid_e = lid.clone();
        let mid_e = mid.clone();
        let lid_k = lid.clone();
        let mid_k = mid.clone();
        let lid_p = lid.clone();
        let mid_p = mid.clone();
        let lid_n = lid.clone();
        let mid_n = mid.clone();
        let lid_c = lid.clone();
        let mid_c = mid.clone();
        let lid_l = lid.clone();
        let mid_l = mid.clone();
        let locked_now = mask.locked;
        let lid_r = lid.clone();
        let mid_r = mid.clone();
        let p_r = panel_entity.clone();
        let mask_name_now = mask.name.clone();
        let renaming_here = rename_view.as_ref().map(|v| v.target.clone()) == Some((lid.clone(), mid.clone()));

        let current_tc = state.read(cx).current_timecode();
        let path_kf = mask.path.has_keyframe_at(&current_tc);
        let path_prev = mask.path.previous_keyframe_time(&current_tc).is_some();
        let path_next = mask.path.next_keyframe_time(&current_tc).is_some();
        let node_count = mask.path.value.points.len();
        let is_closed = mask.path.value.closed;

        let mut box_el = v_flex()
            .id(SharedString::from(format!("mask_box_{mid}")))
            .test_support()
            .p_2()
            .rounded_sm()
            .bg(cx.theme().secondary)
            .gap_1p5();

        // Header: enable, name, mode pill, invert, edit, delete.
        box_el = box_el.child(
            h_flex()
                .items_center()
                .justify_between()
                .text_xs()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(
                            div()
                                .id(SharedString::from(format!("mask_toggle_{mid}")))
                                .test_support()
                                .cursor_pointer()
                                .w(px(14.))
                                .h(px(14.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(if mask.enabled {
                                    cx.theme().foreground
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_toggle.update(cx, |s, cx| {
                                        let _ = s.toggle_mask_enabled(&lid_t, &mid_t);
                                        cx.notify();
                                    });
                                })
                                .child(if mask.enabled { icon_box(IconName::Eye) } else { icon_box(IconName::EyeOff) }),
                        )
                        .child(match rename_view.clone() {
                            Some(v) if v.target == (lid.clone(), mid.clone()) => {
                                Input::new(&v.editor).w(px(110.)).into_any_element()
                            }
                            _ => div()
                                .font_semibold()
                                .text_color(if mask.enabled {
                                    cx.theme().foreground
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .child(format!("{} · {} pts{}{}", mask.name, node_count, if is_closed { " · closed" } else { "" }, if mask.locked { " · locked" } else { "" }))
                                .into_any_element(),
                        })
                        .child(
                            div()
                                .id(SharedString::from(format!("mask_rename_{mid}")))
                                .test_support()
                                .cursor_pointer()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(cx.theme().muted)
                                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                .text_xs()
                                .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                                    p_r.update(cx, |this, cx| {
                                        this.begin_mask_rename(&lid_r, &mid_r, mask_name_now.clone(), window, cx);
                                    });
                                })
                                .child(if renaming_here { "Renaming…" } else { "Rename" }),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            div()
                                .id(SharedString::from(format!("mask_mode_{mid}")))
                                .test_support()
                                .cursor_pointer()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(cx.theme().muted)
                                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_mode.update(cx, |s, cx| {
                                        let _ = s.cycle_mask_mode(&lid_m, &mid_m);
                                        cx.notify();
                                    });
                                })
                                .child(mask.mode.label()),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("mask_lock_{mid}")))
                                .test_support()
                                .cursor_pointer()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(if mask.locked { cx.theme().primary } else { cx.theme().muted })
                                .text_color(if mask.locked { cx.theme().primary_foreground } else { cx.theme().foreground })
                                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_lock.update(cx, |s, cx| {
                                        let _ = s.set_mask_locked(&lid_l, &mid_l, !locked_now);
                                        cx.notify();
                                    });
                                })
                                .child(if mask.locked { "Locked" } else { "Lock" }),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("mask_delete_{mid}")))
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
                                        let _ = s.remove_layer_mask(&lid_d, &mid_d);
                                        cx.notify();
                                    });
                                })
                                .child(icon_box(IconName::Trash)),
                        ),
                ),
        );

        // Second row: invert, path open/close, edit target, path keys.
        box_el = box_el.child(
            h_flex()
                .gap_1()
                .items_center()
                .text_xs()
                .child(
                    div()
                        .cursor_pointer()
                        .px_1p5()
                        .py_0p5()
                        .rounded_sm()
                        .bg(if mask.invert { cx.theme().primary } else { cx.theme().muted })
                        .text_color(if mask.invert { cx.theme().primary_foreground } else { cx.theme().foreground })
                        .hover(|s| s.bg(cx.theme().accent))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_inv.update(cx, |s, cx| {
                                let _ = s.toggle_mask_invert(&lid_i, &mid_i);
                                cx.notify();
                            });
                        })
                        .child(if mask.invert { "Inverted" } else { "Invert" }),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .px_1p5()
                        .py_0p5()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .hover(|s| s.bg(cx.theme().accent))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_add.update(cx, |s, cx| {
                                let cur = s
                                    .active_composition()
                                    .and_then(|c| c.get_layer(&lid_c))
                                    .and_then(|l| l.get_mask(&mid_c))
                                    .map(|m| m.path.value.closed)
                                    .unwrap_or(false);
                                let _ = s.set_mask_closed(&lid_c, &mid_c, !cur);
                                cx.notify();
                            });
                        })
                        .child(if is_closed { "Open Path" } else { "Close Path" }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("mask_edit_{mid}")))
                        .test_support()
                        .cursor_pointer()
                        .px_1p5()
                        .py_0p5()
                        .rounded_sm()
                        .bg(if is_editing { cx.theme().primary } else { cx.theme().muted })
                        .text_color(if is_editing { cx.theme().primary_foreground } else { cx.theme().foreground })
                        .hover(|s| s.bg(cx.theme().accent))
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_edit.update(cx, |s, cx| {
                                        // Entering edit mode also selects the
                                        // layer so the overlay is visible.
                                        s.select_layer(Some(lid_e.clone()));
                                        if s.active_mask_edit == Some((lid_e.clone(), mid_e.clone())) {
                                            s.set_active_mask_edit(None);
                                        } else {
                                            s.set_active_mask_edit(Some((lid_e.clone(), mid_e.clone())));
                                        }
                                        cx.notify();
                                    });
                                })
                        .child(if is_editing { "Editing…" } else { "Edit Nodes" }),
                )
                .child({
                    let s_shape = state.clone();
                    let p_shape = panel_entity.clone();
                    let lid_s = lid.clone();
                    let mid_s = mid.clone();
                    let dialog_open = shape_view.as_ref().map(|v| v.target.clone()) == Some((lid.clone(), mid.clone()));
                    div()
                        .id(SharedString::from(format!("mask_shape_{mid}")))
                        .test_support()
                        .cursor_pointer()
                        .px_1p5()
                        .py_0p5()
                        .rounded_sm()
                        .bg(if dialog_open { cx.theme().primary } else { cx.theme().muted })
                        .text_color(if dialog_open { cx.theme().primary_foreground } else { cx.theme().foreground })
                        .hover(|s| s.bg(cx.theme().accent))
                        .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                            if dialog_open {
                                p_shape.update(cx, |this, cx| {
                                    this.mask_shape_editors = None;
                                    cx.notify();
                                });
                            } else {
                                // Guess the primitive from the current path.
                                let kind = s_shape.update(cx, |s, _| {
                                    s.active_composition()
                                        .and_then(|c| c.get_layer(&lid_s))
                                        .and_then(|l| l.get_mask(&mid_s))
                                        .map(|m| {
                                            let pts = &m.path.value.points;
                                            if pts.len() == 4
                                                && pts.iter().all(|p| {
                                                    p.kind == project::PathPointKind::Smooth
                                                })
                                            {
                                                project::MaskShapeKind::Ellipse
                                            } else {
                                                project::MaskShapeKind::Rectangle
                                            }
                                        })
                                        .unwrap_or(project::MaskShapeKind::Rectangle)
                                });
                                p_shape.update(cx, |this, cx| {
                                    this.open_mask_shape_dialog(&lid_s, &mid_s, kind, window, cx);
                                });
                            }
                        })
                        .child("Shape…")
                })
                .child(
                    h_flex()
                        .gap_0p5()
                        .items_center()
                        .child(
                            div()
                                .cursor_pointer()
                                .px_0p5()
                                .text_color(if path_prev { cx.theme().foreground } else { cx.theme().muted_foreground.opacity(0.3) })
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    if path_prev {
                                        s_k.update(cx, |s, cx| {
                                            let _ = s.seek_mask_path_keyframe(&lid_k, &mid_k, -1);
                                            cx.notify();
                                        });
                                    }
                                })
                                .child("◂"),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("mask_pathkf_{mid}")))
                                .test_support()
                                .cursor_pointer()
                                .px_0p5()
                                .text_color(if path_kf { rgb(0xf59e0b).into() } else { cx.theme().muted_foreground })
                                .hover(|s| s.text_color(rgb(0xffffff)))
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    s_p.update(cx, |s, cx| {
                                        let _ = s.toggle_mask_path_keyframe_at_current_time(&lid_p, &mid_p);
                                        cx.notify();
                                    });
                                })
                                .child("◆"),
                        )
                        .child(
                            div()
                                .cursor_pointer()
                                .px_0p5()
                                .text_color(if path_next { cx.theme().foreground } else { cx.theme().muted_foreground.opacity(0.3) })
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    if path_next {
                                        s_n.update(cx, |s, cx| {
                                            let _ = s.seek_mask_path_keyframe(&lid_n, &mid_n, 1);
                                            cx.notify();
                                        });
                                    }
                                })
                                .child("▸"),
                        ),
                ),
        );

        // Scalar rows: opacity / feather / expansion.
        box_el = box_el
            .child(mask_param_row(state, panel_entity, &lid, &mid, "opacity", "Opacity", format!("{:.0}%", mask.opacity.value), 1.0, cx))
            .child(mask_param_row(state, panel_entity, &lid, &mid, "feather", "Feather", format!("{:.1} px", mask.feather.value), 0.5, cx))
            .child(mask_param_row(state, panel_entity, &lid, &mid, "expansion", "Expansion", format!("{:+.1} px", mask.expansion.value), 0.5, cx));

        // Mask Shape dialog (numeric bounding box, AE parity).
        if let Some(view) = shape_view.clone() {
            if view.target == (lid.clone(), mid.clone()) {
                let p_apply = panel_entity.clone();
                let p_kind = panel_entity.clone();
                let p_close = panel_entity.clone();
                let lid_a = lid.clone();
                let mid_a = mid.clone();
                let field = |label: &str, ed: &Entity<InputState>| {
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .w(px(14.))
                                .text_color(cx.theme().muted_foreground)
                                .child(label.to_string()),
                        )
                        .child(Input::new(ed).w(px(64.)))
                };
                box_el = box_el.child(
                    v_flex()
                        .id(SharedString::from(format!("mask_shape_dialog_{mid}")))
                        .test_support()
                        .p_2()
                        .gap_1()
                        .rounded_sm()
                        .bg(cx.theme().muted)
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_xs()
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(cx.theme().secondary)
                                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            p_kind.update(cx, |this, cx| {
                                                if let Some(ed) = this.mask_shape_editors.as_mut() {
                                                    ed.kind = ed.kind.cycle();
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .child(view.kind.label().to_string()),
                                )
                                .child(field("X", &view.x))
                                .child(field("Y", &view.y)),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_xs()
                                .child(field("W", &view.w))
                                .child(field("H", &view.h)),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .text_xs()
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(cx.theme().primary)
                                        .text_color(cx.theme().primary_foreground)
                                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            p_apply.update(cx, |this, cx| {
                                                if let Some(ed) = this.mask_shape_editors.as_ref() {
                                                    let kind = ed.kind;
                                                    let parse = |t: &str, fb: f32| {
                                                        t.trim().parse::<f32>().unwrap_or(fb)
                                                    };
                                                    // Fall back to the live bounds per field.
                                                    let (bx, by, bw, bh) = this
                                                        .state
                                                        .read(cx)
                                                        .active_composition()
                                                        .and_then(|c| c.get_layer(&lid_a))
                                                        .and_then(|l| l.get_mask(&mid_a))
                                                        .and_then(|m| m.path.value.bounds())
                                                        .map(|(mn, mx)| (mn.x, mn.y, mx.x - mn.x, mx.y - mn.y))
                                                        .unwrap_or((0.0, 0.0, 200.0, 200.0));
                                                    let (x, y, w, h) = (
                                                        parse(&ed.x_text, bx),
                                                        parse(&ed.y_text, by),
                                                        parse(&ed.w_text, bw),
                                                        parse(&ed.h_text, bh),
                                                    );
                                                    this.state.update(cx, |s, cx| {
                                                        let _ = s.set_mask_shape_numeric(&lid_a, &mid_a, kind, x, y, w, h);
                                                        cx.notify();
                                                    });
                                                    this.mask_shape_editors = None;
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .child("Apply"),
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(cx.theme().secondary)
                                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            p_close.update(cx, |this, cx| {
                                                this.mask_shape_editors = None;
                                                cx.notify();
                                            });
                                        })
                                        .child("Close"),
                                ),
                        ),
                );
            }
        }

        col = col.child(box_el);
    }

    col.into_any_element()
}

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
    wheels: &HashMap<(String, String), Entity<InspectorColorPicker>>,
    ui: &PropUi,
    selects: &HashMap<(String, String), Entity<SelectState<SearchableVec<String>>>>,
    collapsed: &HashSet<String>,
    group_collapsed: &HashSet<String>,
    cx: &App,
) -> AnyElement {    if layer.effects.is_empty() {
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
            let eff_id_disc = effect.id.clone();
            let s_toggle = state.clone();
            let s_del = state.clone();
            let p_disc = panel_entity.clone();
            // Nested-collapsible state: expanded by default so every
            // `param_*` / `shader_*` test id stays visible until collapsed.
            let fx_open = !collapsed.contains(&eff_id);

            // Parameter body (nested inside the collapsible card below).
            let mut effect_box = v_flex().gap_1p5();

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
                                .id(SharedString::from(format!("effect_disclosure_{}", effect.id)))
                                .test_support()
                                .cursor_pointer()
                                .w(px(10.))
                                .text_color(cx.theme().muted_foreground)
                                .hover(|s| s.text_color(ae::text()))
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    let key = eff_id_disc.clone();
                                    p_disc.update(cx, |this, cx| {
                                        if !this.fx_collapsed.remove(key.as_str()) {
                                            this.fx_collapsed.insert(key);
                                        }
                                        cx.notify();
                                    });
                                })
                                .child(if fx_open { "▾" } else { "▸" }),
                        )
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

            match &effect.effect_type {
                // Fully declarative arms: every parameter renders from its
                // declaration (no per-effect UI). Custom arms below keep
                // only their bespoke parts (pickers, editors, gradients).
                EffectType::GaussianBlur { .. } | EffectType::BrightnessContrast { .. } => {
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                }
                EffectType::Tint { map_black, map_white, .. } => {
                    let mb = *map_black;
                    let mw = *map_white;
                    let mb_hex = format!("#{:02X}{:02X}{:02X}", (mb.r * 255.0) as u8, (mb.g * 255.0) as u8, (mb.b * 255.0) as u8);
                    let mw_hex = format!("#{:02X}{:02X}{:02X}", (mw.r * 255.0) as u8, (mw.g * 255.0) as u8, (mw.b * 255.0) as u8);
                    let s_tb = state.clone();
                    let s_tw = state.clone();
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
                        );
                    // Amount rides the declaration (scalar widget); the
                    // bespoke rows above keep the fixed Black/White pickers.
                    for decl in effect.declarations() {
                        if decl.is_scalar() {
                            effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                        }
                    }
                }
                EffectType::Invert { .. } => {
                    // Fully declarative: the amount row renders from its
                    // declaration (scalar widget).
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                }
                EffectType::DropShadow { color, .. } => {
                    let sc = *color;
                    let sc_hex = format!("#{:02X}{:02X}{:02X}", (sc.r * 255.0) as u8, (sc.g * 255.0) as u8, (sc.b * 255.0) as u8);
                    let s_sc = state.clone();
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
                        );
                    // Distance / Angle / Softness / Opacity ride their
                    // declarations (scalar widgets); the bespoke Color row
                    // above keeps the fixed shadow picker.
                    for decl in effect.declarations() {
                        if decl.is_scalar() {
                            effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                        }
                    }
                }
                EffectType::GlslShader { code, .. } => {
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

                    // P1..P4 ride their declarations (scalar widgets); the
                    // code editor below stays bespoke.
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                    effect_box = effect_box
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

                    // Auto-generated parameter UI, grouped into nested
                    // collapsibles (AE-style twirl-downs). Params without a
                    // declared group render flat; grouped params collapse.
                    let mut groups: Vec<(Option<String>, Vec<AnyElement>)> = Vec::new();
                    for param in params {
                        let gname = param.group.clone();
                        if groups.last().map(|(n, _)| n != &gname).unwrap_or(true) {
                            groups.push((gname, Vec::new()));
                        }
                        let slot = match groups.last_mut() {
                            Some(s) => s,
                            None => continue,
                        };
                        let pname = param.name.clone();
                        let plabel = param.label.clone();
                        let eff_key = eff_id.clone();
                        let row: AnyElement = match param.param_type.widget_kind() {
                            project::widget::WidgetKind::Slider
                            | project::widget::WidgetKind::Integer
                            | project::widget::WidgetKind::Angle => {
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
                                crate::widgets::widget_scalar(
                                    state,
                                    panel_entity,
                                    &layer.id,
                                    &eff_key,
                                    &pname,
                                    &format!("{plabel}{unit}"),
                                    cur,
                                    param.step.unwrap_or(0.05),
                                    100.0,
                                    false,
                                    crate::widgets::ScalarCommit::ShaderLab,
                                    cx,
                                )
                            }
                            project::widget::WidgetKind::Checkbox => {
                                let on = matches!(
                                    resolved.get(pname.as_str()),
                                    Some(ShaderParamValue::Bool(true))
                                );
                                let s_t = state.clone();
                                let eid = eff_key.clone();
                                let pn = pname.clone();
                                crate::widgets::widget_bool(
                                    &plabel,
                                    on,
                                    format!("shader_bool_{eff_key}_{pname}"),
                                    &format!("Toggle {plabel}"),
                                    move |next: bool, cx: &mut App| {
                                        let (eid, pn) = (eid.clone(), pn.clone());
                                        s_t.update(cx, |s, cx| {
                                            let _ = s.set_shaderlab_param(&eid, &pn, if next { 1.0 } else { 0.0 });
                                            cx.notify();
                                        });
                                    },
                                    cx,
                                )
                            }
                            project::widget::WidgetKind::Dropdown => {
                                crate::widgets::widget_dropdown(
                                    &eff_key,
                                    &pname,
                                    &plabel,
                                    selects.get(&(eff_key.clone(), pname.clone())),
                                    cx,
                                )
                            }
                            project::widget::WidgetKind::Vec2
                            | project::widget::WidgetKind::Vec3
                            | project::widget::WidgetKind::Vec4
                            | project::widget::WidgetKind::Color => {
                                let (count, tags): (usize, &[&str]) = match &param.param_type {
                                    project::shader::ShaderParamType::Vec2 => (2, &["X", "Y"]),
                                    project::shader::ShaderParamType::Vec3 => (3, &["X", "Y", "Z"]),
                                    project::shader::ShaderParamType::Vec4 => (4, &["X", "Y", "Z", "W"]),
                                    _ => (4, &["R", "G", "B", "A"]),
                                };
                                let mut values = vec![0.0f32; count];
                                for i in 0..count {
                                    values[i] = match resolved.get(pname.as_str()) {
                                        Some(ShaderParamValue::Vec2(a)) => a.get(i).copied().unwrap_or(0.0),
                                        Some(ShaderParamValue::Vec3(a)) => a.get(i).copied().unwrap_or(0.0),
                                        Some(ShaderParamValue::Vec4(a)) => a.get(i).copied().unwrap_or(0.0),
                                        Some(ShaderParamValue::Color(c)) => {
                                            [c.r, c.g, c.b, c.a].get(i).copied().unwrap_or(0.0)
                                        }
                                        Some(ShaderParamValue::Float(v)) => *v,
                                        _ => 0.0,
                                    };
                                }
                                crate::widgets::widget_vec(
                                    state,
                                    panel_entity,
                                    &eff_key,
                                    &pname,
                                    &plabel,
                                    tags,
                                    &values,
                                    ui.vec_link.contains(&format!("{eff_key}:{pname}")),
                                    cx,
                                )
                            }
                            // Slider-family fallthrough is handled above;
                            // unmapped kinds render nothing.
                            _ => div().into_any_element(),
                        };
                        slot.1.push(row);
                    }

                    // Emit groups: declared groups become nested collapsibles,
                    // ungrouped params stay flat.
                    for (gname, rows) in groups {
                        let rows_body = v_flex().gap_1p5().children(rows).into_any_element();
                        match gname {
                            None => effect_box = effect_box.child(rows_body),
                            Some(g) => {
                                let gkey = format!("{eff_id}:{g}");
                                let gopen = !group_collapsed.contains(&gkey);
                                let p_grp = panel_entity.clone();
                                let gkey_t = gkey.clone();
                                let gtitle = div()
                                    .font_semibold()
                                    .text_color(cx.theme().foreground)
                                    .child(g.clone())
                                    .into_any_element();
                                effect_box = effect_box.child(nested_group(
                                    SharedString::from(format!("shader_group_{gkey}")),
                                    gtitle,
                                    gopen,
                                    move |cx| {
                                        let key = gkey_t.clone();
                                        p_grp.update(cx, |this, cx| {
                                            if !this.fx_group_collapsed.remove(key.as_str()) {
                                                this.fx_group_collapsed.insert(key);
                                            }
                                            cx.notify();
                                        });
                                    },
                                    rows_body,
                                ));
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
                EffectType::DisplacementMap { .. } => {
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                }
                EffectType::ChromaKey { key_color, .. } => {
                    let s_ck = state.clone();
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
                        );
                    // Tolerance / Feather ride their declarations (scalar
                    // widgets); the bespoke Key Color row above keeps the
                    // fixed chroma picker.
                    for decl in effect.declarations() {
                        if decl.is_scalar() {
                            effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                        }
                    }
                }
                EffectType::LumaKey { .. } => {
                    // Fully declarative: both rows render from declarations.
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                }
                EffectType::NoiseGenerator { .. } => {
                    // Amount + monochrome checkbox ride their declarations.
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                }
                EffectType::Checkerboard { .. } => {
                    // Swatches + size ride their declarations.
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                }
                EffectType::GradientRamp { .. } => {
                    // Two-stop gradient editor over the Start/End colors
                    // (stop bar, wheel, reverse, presets) + angle dial row.
                    let decls = effect.declarations();
                    let find = |f: &str| decls.iter().find(|d| d.field == f).cloned();
                    if let (Some(da), Some(db)) = (find("color_a"), find("color_b")) {
                        let (ca, cb) = match (&da.value, &db.value) {
                            (project::PropValue::Color(a), project::PropValue::Color(b)) => (*a, *b),
                            _ => (Color::BLACK, Color::WHITE),
                        };
                        effect_box = effect_box.child(crate::widgets::widget_gradient(
                            state,
                            panel_entity,
                            &layer.id,
                            &eff_id,
                            ("color_a", da.label.as_str(), ca),
                            ("color_b", db.label.as_str(), cb),
                            ui.gradient_stop.get(&eff_id).copied().unwrap_or(0),
                            wheels,
                            cx,
                        ));
                    }
                    if let Some(angle) = find("angle") {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &angle, wheels, cx));
                    }
                }
                EffectType::Perspective { .. }
                | EffectType::TextOutline { .. }
                | EffectType::TextBevel { .. }
                | EffectType::Bloom { .. }
                | EffectType::Tiler { .. }
                | EffectType::Warp { .. }
                | EffectType::Exposure { .. }
                | EffectType::Vibrance { .. }
                | EffectType::Levels { .. }
                | EffectType::HueSaturation { .. }
                | EffectType::Sharpen { .. }
                | EffectType::Vignette { .. } => {
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                }
                // Modular stock plug-ins render from the same declarations
                // as every other effect (scalars + color slots).
                EffectType::Stock { .. } => {
                    for decl in effect.declarations() {
                        effect_box = effect_box.child(crate::widgets::widget_for_decl(state, panel_entity, &layer.id, &eff_id, &decl, wheels, cx));
                    }
                }
            }

            // AE-style nested collapsible: the header (disclosure, eye,
            // name, trash) always mounts so the card keeps its test id and
            // actions; the parameter body mounts only while expanded.
            let params_body = if fx_open {
                effect_box.into_any_element()
            } else {
                div().into_any_element()
            };
            fx_col = fx_col.child(
                v_flex()
                    .id(SharedString::from(format!("applied_effect_{}", effect.id)))
                    .test_support()
                    .p_2()
                    .rounded_sm()
                    .bg(cx.theme().secondary)
                    .gap_1p5()
                    .child(
                        Collapsible::new()
                            .open(fx_open)
                            .child(header_row)
                            .content(params_body),
                    ),
            );
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
                LayerSource::Text { text, font_family, font_size, fill_color, .. } => {
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

        // Dynamic color wheels, one per effect color field on the selected
        // layer. Created here (before the state read-guard) with the same
        // keyed pattern as the fixed chroma/tint/shadow pickers above, so
        // every color param gets a real wheel, not just presets.
        let fx_color_targets: Vec<(String, &'static str)> = {
            let s = self.state.read(cx);
            match s
                .active_composition()
                .zip(s.selected_layer_id.clone())
                .and_then(|(c, lid)| c.get_layer(&lid))
            {
                Some(layer) => layer
                    .effects
                    .iter()
                    .flat_map(|e| {
                        fx_color_fields(e)
                            .into_iter()
                            .map(move |f| (e.id.clone(), f))
                    })
                    .collect(),
                None => Vec::new(),
            }
        };
        let fx_editor = self.state.clone();
        let mut fx_wheels: HashMap<(String, String), Entity<InspectorColorPicker>> =
            HashMap::new();
        for (eid, field) in fx_color_targets {
            let editor = fx_editor.clone();
            let eid_w = eid.clone();
            let picker = window.use_keyed_state(
                SharedString::from(format!("fx_wheel_{eid}_{field}")),
                cx,
                move |window, cx| {
                    let picker = cx.new(|cx| ColorPickerState::new(window, cx));
                    let editor = editor.clone();
                    let subscription = cx.subscribe(
                        &picker,
                        move |_, _, event: &ColorPickerEvent, cx| {
                            let ColorPickerEvent::Change(Some(hsla)) = event else {
                                return;
                            };
                            let rgba: Rgba = (*hsla).into();
                            let color = Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a);
                            editor.update(cx, |state, cx| {
                                if let Some(lid) = state.selected_layer_id.clone() {
                                    let present = state
                                        .active_composition()
                                        .and_then(|c| c.get_layer(&lid))
                                        .and_then(|l| l.get_effect(&eid_w))
                                        .is_some();
                                    if present {
                                        let _ = state.set_effect_color(&lid, &eid_w, field, color);
                                        cx.notify();
                                    }
                                }
                            });
                        },
                    );
                    InspectorColorPicker { state: picker, _subscription: subscription }
                },
            );
            fx_wheels.insert((eid, field.to_string()), picker);
        }

        // Enum Select states, one per ShaderLab enum param on the selected
        // layer (kit Select: keyboard nav, search, dismissal and a11y come
        // free). Keyed by (effect, param, index) so external changes (undo)
        // resync by construction; Confirm commits through the shader
        // setter. States + subscriptions live on the panel — never rebuilt
        // in render — with stale keys pruned every frame.
        let fx_enum_targets: Vec<(String, String, Vec<String>, usize)> = {
            let s = self.state.read(cx);
            match s
                .active_composition()
                .zip(s.selected_layer_id.clone())
                .and_then(|(c, lid)| c.get_layer(&lid))
            {
                Some(layer) => layer
                    .effects
                    .iter()
                    .flat_map(|e| {
                        let eid = e.id.clone();
                        match &e.effect_type {
                            EffectType::ShaderLab { params, values, .. } => params
                                .iter()
                                .filter_map(|p| {
                                    let project::shader::ShaderParamType::Enum { options } =
                                        &p.param_type
                                    else {
                                        return None;
                                    };
                                    let idx = match values.get(&p.name) {
                                        Some(project::ShaderParamValue::Int(v)) => (*v).clamp(
                                            0,
                                            options.len().saturating_sub(1) as i32,
                                        )
                                            as usize,
                                        _ => 0,
                                    };
                                    Some((eid.clone(), p.name.clone(), options.clone(), idx))
                                })
                                .collect::<Vec<_>>(),
                            _ => Vec::new(),
                        }
                    })
                    .collect(),
                None => Vec::new(),
            }
        };
        let mut fx_selects: HashMap<
            (String, String),
            Entity<SelectState<SearchableVec<String>>>,
        > = HashMap::new();
        {
            let live: HashSet<(String, String, usize)> = fx_enum_targets
                .iter()
                .map(|(e, p, _, i)| (e.clone(), p.clone(), *i))
                .collect();
            self.select_states.retain(|k, _| live.contains(k));
            self.select_subs.retain(|k, _| live.contains(k));
            for (eid, pname, options, idx) in &fx_enum_targets {
                let key = (eid.clone(), pname.clone(), *idx);
                if !self.select_states.contains_key(&key) {
                    let delegate = SearchableVec::new(options.clone());
                    let st = cx.new(|cx| {
                        SelectState::new(delegate, Some(IndexPath::new(*idx)), window, cx)
                    });
                    let editor = self.state.clone();
                    let (eid_s, pname_s, opts_s) =
                        (eid.clone(), pname.clone(), options.clone());
                    let sub = cx.subscribe(
                        &st,
                        move |_, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                            let SelectEvent::Confirm(value) = event;
                            let Some(v) = value else {
                                return;
                            };
                            let i = opts_s.iter().position(|o| o == v).unwrap_or(0) as f32;
                            editor.update(cx, |state, cx| {
                                let _ = state.set_shaderlab_param(&eid_s, &pname_s, i);
                                cx.notify();
                            });
                        },
                    );
                    self.select_states.insert(key.clone(), st);
                    self.select_subs.insert(key, sub);
                }
                if let Some(st) = self.select_states.get(&(eid.clone(), pname.clone(), *idx)) {
                    fx_selects.insert((eid.clone(), pname.clone()), st.clone());
                }
            }
        }

        // Retain and sync Combobox states for Font Family and Font Style
        {
            let (is_text, cur_fam, cur_w) = {
                let st = self.state.read(cx);
                match st.selected_layer().map(|l| &l.source) {
                    Some(LayerSource::Text { font_family, weight, .. }) => (true, font_family.clone(), *weight),
                    _ => (false, "Inter".to_string(), 400u16),
                }
            };
            if is_text {
                if !self.combobox_states.contains_key("text_font_family") {
                    let sys_fonts = EditorState::available_system_fonts().to_vec();
                    let sel_idx = sys_fonts.iter().position(|f| f == &cur_fam).unwrap_or(0);
                    let delegate = SearchableVec::new(sys_fonts);
                    let cb = cx.new(|cx| {
                        ComboboxState::new(delegate, vec![IndexPath::new(sel_idx)], window, cx)
                    });
                    let s_f = self.state.clone();
                    let sub = cx.subscribe(&cb, move |_, _, event: &ComboboxEvent<SearchableVec<String>>, cx| {
                        let vals = match event {
                            ComboboxEvent::Confirm(v) => v,
                            ComboboxEvent::Change(v) => v,
                        };
                        if let Some(font_name) = vals.first() {
                            s_f.update(cx, |s, cx| {
                                if let Some(lid) = s.selected_layer_id.clone() {
                                    let _ = s.set_layer_font_family(&lid, font_name);
                                    cx.notify();
                                }
                            });
                        }
                    });
                    self.combobox_states.insert("text_font_family".to_string(), cb);
                    self.combobox_subs.insert("text_font_family".to_string(), sub);
                }

                if !self.combobox_states.contains_key("text_font_style") {
                    let style_options = vec![
                        "Regular".to_string(),
                        "Medium".to_string(),
                        "SemiBold".to_string(),
                        "Bold".to_string(),
                        "Black".to_string(),
                    ];
                    let sel_style_idx = match cur_w {
                        w if w < 450 => 0,
                        w if w < 550 => 1,
                        w if w < 650 => 2,
                        w if w < 800 => 3,
                        _ => 4,
                    };
                    let delegate = SearchableVec::new(style_options);
                    let cb = cx.new(|cx| {
                        ComboboxState::new(delegate, vec![IndexPath::new(sel_style_idx)], window, cx)
                    });
                    let s_w = self.state.clone();
                    let sub = cx.subscribe(&cb, move |_, _, event: &ComboboxEvent<SearchableVec<String>>, cx| {
                        let vals = match event {
                            ComboboxEvent::Confirm(v) => v,
                            ComboboxEvent::Change(v) => v,
                        };
                        if let Some(style_name) = vals.first() {
                            let w = match style_name.as_str() {
                                "Regular" => 400,
                                "Medium" => 500,
                                "SemiBold" => 600,
                                "Bold" => 700,
                                "Black" => 900,
                                _ => 400,
                            };
                            s_w.update(cx, |s, cx| {
                                if let Some(lid) = s.selected_layer_id.clone() {
                                    let _ = s.set_layer_font_weight(&lid, w);
                                    cx.notify();
                                }
                            });
                        }
                    });
                    self.combobox_states.insert("text_font_style".to_string(), cb);
                    self.combobox_subs.insert("text_font_style".to_string(), sub);
                }
            } else {
                self.combobox_states.clear();
                self.combobox_subs.clear();
            }
        }

        let state = self.state.read(cx);
        let selected_layer = state.selected_layer();

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
                let idx = state
                    .active_composition()
                    .and_then(|c| c.layers.iter().position(|x| x.id == l.id))
                    .map(|i| i + 1)
                    .unwrap_or(1);
                (format!("{} (Layer {})", l.name, idx), type_str.to_string())
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
                let s = this.state.clone();
                s.update(cx, |s, cx| {
                    s.preview_fast = false;
                    cx.notify();
                });
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.scrub_prop = None;
                this.scrub_last_x = None;
                this.scrub_moved = false;
                let s = this.state.clone();
                s.update(cx, |s, cx| {
                    s.preview_fast = false;
                    cx.notify();
                });
            }))
            // Header (AE inspector chrome: layer dot + name + type).
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(ae::border())
                    .bg(ae::panel())
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(icon_box(IconName::Layers))
                            .child(div().font_semibold().text_xs().text_color(ae::text()).child(header_title)),
                    )
                    .child(header_actions),
            )
            // Inspector fields (scrolls both ways: narrow docks never clip).
            .child(
                v_flex()
                    .id("properties_inspector")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .overflow_x_scroll()
                    .p_3()
                    .gap_3()
                    .children(if let Some(layer) = selected_layer {
                        // Local transform values at the playhead (cheap
                        // property reads). The full scene eval is the
                        // viewer's job — doing it here too doubled
                        // evaluation cost on every frame.
                        let current_tc = state.clock.timecode();
                        let (anchor, pos, sc, rot) = layer.transform.evaluate_at(&current_tc);
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

                        let chroma_picker_state = chroma_color_picker.read(cx).state.clone();
                        let tint_black_picker_state = tint_black_color_picker.read(cx).state.clone();
                        let tint_white_picker_state = tint_white_color_picker.read(cx).state.clone();
                        let shadow_picker_state = shadow_color_picker.read(cx).state.clone();
                        let effects_list = render_applied_effects(
                            &self.state,
                            layer,
                            &panel_entity,
                            &chroma_picker_state,
                            &tint_black_picker_state,
                            &tint_white_picker_state,
                            &shadow_picker_state,
                            self.shader_editor_open.clone(),
                            self.shader_editor.clone(),
                            &fx_wheels,
                            &PropUi {
                                vec_link: self.vec_link.clone(),
                                gradient_stop: self.gradient_stop.clone(),
                            },
                            &fx_selects,
                            &self.fx_collapsed,
                            &self.fx_group_collapsed,
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

                                let solid_mode = self.color_picker_mode.get("solid_color").map(|s| s.as_str()).unwrap_or(if c.a <= 0.0 { "none" } else { "color" });
                                let solid_grad = self.color_picker_gradient_colors.get("solid_color").copied().unwrap_or((Color::WHITE, Color::BLACK));
                                let is_solid_active = self.active_color_picker.as_deref() == Some("solid_color");
                                let p_solid_picker = panel_entity.clone();
                                let p_src = panel_entity.clone();

                                let solid_dialog: Option<AnyElement> = if is_solid_active {
                                    Some(render_three_mode_color_picker("solid_color", c, self, &panel_entity, &self.state, &inspector_color, cx).into_any_element())
                                } else {
                                    None
                                };

                                let solid_body = v_flex()
                                    .id("solid_properties_section")
                                    .test_support()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .text_xs()
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(div().text_color(ae::dim()).child("Color:"))
                                                    .child(
                                                        render_color_swatch(
                                                            "solid_color_swatch",
                                                            c,
                                                            solid_mode,
                                                            solid_grad,
                                                            is_solid_active,
                                                            cx,
                                                            move |_event, _window, cx| {
                                                                p_solid_picker.update(cx, |this, cx| {
                                                                    this.active_color_picker = if this.active_color_picker.as_deref() == Some("solid_color") {
                                                                        None
                                                                    } else {
                                                                        Some("solid_color".to_string())
                                                                    };
                                                                    cx.notify();
                                                                });
                                                            },
                                                        )
                                                    )
                                                    .child(
                                                        div()
                                                            .id("solid_color_hex")
                                                            .test_support()
                                                            .font_semibold()
                                                            .text_color(ae::text())
                                                            .child(hex_code),
                                                    )
                                                    .child(div().id("solid_color_wheel").test_support().child(ColorPicker::new(&inspector_color.read(cx).state).label("Color")))
                                            )
                                    )
                                    .children(solid_dialog)
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .text_xs()
                                            .child(div().text_color(ae::dim()).child("Native Size:"))
                                            .child(
                                                div()
                                                    .font_medium()
                                                    .text_color(ae::text())
                                                    .child(format!("{w} × {h} px")),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .text_xs()
                                            .child(div().text_color(ae::dim()).child("Presets:"))
                                            .child(palette_row),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .text_xs()
                                            .child(div().text_color(ae::dim()).child("Dimensions:"))
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
                                    );

                                props_items.push(prop_section(
                                    "solid_source",
                                    format!("Solid Source ({}x{})", w, h),
                                    IconName::Square,
                                    self.source_expanded,
                                    move |cx| {
                                        p_src.update(cx, |this, cx| {
                                            this.source_expanded = !this.source_expanded;
                                            cx.notify();
                                        });
                                    },
                                    cx,
                                    Some(solid_body.into_any_element()),
                                ));
                            }
                            LayerSource::Text {
                                text,
                                font_family,
                                font_size,
                                fill_color,
                                weight,
                                italic,
                                tracking,
                                leading,
                                align,
                                all_caps,
                                stroke_width,
                                stroke_color,
                                baseline_shift,
                                box_width,
                                box_height,
                                underline,
                                small_caps,
                                superscript,
                                subscript,
                                stroke_position,
                                paint_order,
                                vertical_align,
                                text_path,
                                ..
                            } => {
                                let lid_t = layer.id.clone();
                                let s_text = self.state.clone();
                                let s_fs = self.state.clone();
                                let _s_col = self.state.clone();
                                let s_typo = self.state.clone();

                                let cur_text = text.value.clone();
                                let cur_fs = font_size.value;
                                let _cur_fam = font_family.clone();
                                let cur_col = fill_color.value;
                                let cur_weight = *weight;
                                let cur_italic = *italic;
                                let cur_tracking = tracking.value;
                                let cur_leading = leading.value;
                                let cur_align = *align;
                                let cur_caps = *all_caps;
                                let cur_stroke_w = stroke_width.value;
                                let cur_stroke = *stroke_color;
                                let _cur_baseline = baseline_shift.value;
                                let cur_box = box_width.value;
                                let cur_box_h = box_height.value;
                                let cur_underline = *underline;
                                let cur_small_caps = *small_caps;
                                let cur_superscript = *superscript;
                                let cur_subscript = *subscript;
                                let cur_stroke_pos = stroke_position.clone();
                                let cur_paint_order = paint_order.clone();
                                let cur_vert_align = vertical_align.clone();

                                let inputs = text_inputs.read(cx);

                                // 1. Source Text Presets
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
                                            .child(
                                                Button::new(SharedString::from(format!("text_preset_btn_{p_str}")))
                                                    .compact()
                                                    .child(p_str)
                                                    .on_click(move |_, _, cx| {
                                                        let t = target_str.clone();
                                                        s_p.update(cx, |s, cx| {
                                                            let _ = s.set_layer_text(&lid_p, &t);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    );
                                }

                                // 2. Font Group Collapsible
                                let (font_open, fill_open, para_open) = (
                                    !self.text_collapsed.contains("font") && !self.text_collapsed.contains("character"),
                                    !self.text_collapsed.contains("fill_stroke") && !self.text_collapsed.contains("stroke"),
                                    !self.text_collapsed.contains("paragraph"),
                                );

                                // Comboboxes for Font Family and Font Style
                                let font_family_cb = if let Some(cb) = self.combobox_states.get("text_font_family") {
                                    div().id("text_font_combobox").test_support().w_full().child(Combobox::new(cb)).into_any_element()
                                } else {
                                    Input::new(&inputs.font_family).id("text_font_input").w_full().into_any_element()
                                };

                                let font_style_cb = if let Some(cb) = self.combobox_states.get("text_font_style") {
                                    div().id("text_style_combobox").test_support().w_full().child(Combobox::new(cb)).into_any_element()
                                } else {
                                    div().text_xs().text_color(cx.theme().muted_foreground).child("Regular").into_any_element()
                                };

                                // Style Row: 7 buttons (B, I, U, AB, AA, x², x₂)
                                let s_b = s_typo.clone();
                                let lid_b = lid_t.clone();
                                let s_it = s_typo.clone();
                                let lid_it = lid_t.clone();
                                let s_u = s_typo.clone();
                                let lid_u = lid_t.clone();
                                let s_sc = s_typo.clone();
                                let lid_sc = lid_t.clone();
                                let s_cp = s_typo.clone();
                                let lid_cp = lid_t.clone();
                                let s_sup = s_typo.clone();
                                let lid_sup = lid_t.clone();
                                let s_sub = s_typo.clone();
                                let lid_sub = lid_t.clone();

                                let style_row = h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        div()
                                            .id("text_weight_700")
                                            .test_support()
                                            .child(
                                                Button::new("text_weight_btn_700")
                                                    .compact()
                                                    .selected(cur_weight >= 700)
                                                    .child("B")
                                                    .on_click(move |_, _, cx| {
                                                        let new_w = if cur_weight >= 700 { 400 } else { 700 };
                                                        s_b.update(cx, |s, cx| {
                                                            let _ = s.set_layer_font_weight(&lid_b, new_w);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("text_italic_toggle")
                                            .test_support()
                                            .child(
                                                Button::new("text_italic_btn")
                                                    .compact()
                                                    .selected(cur_italic)
                                                    .child("I")
                                                    .on_click(move |_, _, cx| {
                                                        s_it.update(cx, |s, cx| {
                                                            let _ = s.set_layer_italic(&lid_it, !cur_italic);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("text_underline_toggle")
                                            .test_support()
                                            .child(
                                                Button::new("text_underline_btn")
                                                    .compact()
                                                    .selected(cur_underline)
                                                    .child("U")
                                                    .on_click(move |_, _, cx| {
                                                        s_u.update(cx, |s, cx| {
                                                            let _ = s.set_layer_underline(&lid_u, !cur_underline);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("text_small_caps_toggle")
                                            .test_support()
                                            .child(
                                                Button::new("text_small_caps_btn")
                                                    .compact()
                                                    .selected(cur_small_caps)
                                                    .child("AB")
                                                    .on_click(move |_, _, cx| {
                                                        s_sc.update(cx, |s, cx| {
                                                            let _ = s.set_layer_small_caps(&lid_sc, !cur_small_caps);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("text_caps_toggle")
                                            .test_support()
                                            .child(
                                                Button::new("text_caps_btn")
                                                    .compact()
                                                    .selected(cur_caps)
                                                    .child("AA")
                                                    .on_click(move |_, _, cx| {
                                                        s_cp.update(cx, |s, cx| {
                                                            let _ = s.set_layer_caps(&lid_cp, !cur_caps);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("text_superscript_toggle")
                                            .test_support()
                                            .child(
                                                Button::new("text_superscript_btn")
                                                    .compact()
                                                    .selected(cur_superscript)
                                                    .child("x²")
                                                    .on_click(move |_, _, cx| {
                                                        s_sup.update(cx, |s, cx| {
                                                            let _ = s.set_layer_superscript(&lid_sup, !cur_superscript);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("text_subscript_toggle")
                                            .test_support()
                                            .child(
                                                Button::new("text_subscript_btn")
                                                    .compact()
                                                    .selected(cur_subscript)
                                                    .child("x₂")
                                                    .on_click(move |_, _, cx| {
                                                        s_sub.update(cx, |s, cx| {
                                                            let _ = s.set_layer_subscript(&lid_sub, !cur_subscript);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    );

                                // 2x2 Numeric Parameter Grid:
                                // Row 1: T (Size) | ↕ (Leading)
                                // Row 2: ↔ (Tracking) | ≡ (Box Width %)
                                let s_fs48 = s_fs.clone();
                                let lid_fs48 = lid_t.clone();
                                let numeric_grid = v_flex()
                                    .gap_1p5()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                h_flex()
                                                    .gap_1p5()
                                                    .items_center()
                                                    .child(div().font_bold().text_xs().text_color(cx.theme().muted_foreground).child("T"))
                                                    .child(ae_blue_scrub_field(
                                                        "text_font_size_scrub",
                                                        "font_size".to_string(),
                                                        format!("{cur_fs:.1}"),
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    ))
                                                    .child(
                                                        div()
                                                            .id("text_font_size_48")
                                                            .test_support()
                                                            .child(
                                                                Button::new("text_fs_btn_48")
                                                                    .compact()
                                                                    .selected((cur_fs - 48.0).abs() < 1.0)
                                                                    .child("48")
                                                                    .on_click(move |_, _, cx| {
                                                                        s_fs48.update(cx, |s, cx| {
                                                                            let _ = s.set_layer_font_size(&lid_fs48, 48.0);
                                                                            cx.notify();
                                                                        });
                                                                    }),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_1p5()
                                                    .items_center()
                                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("↕"))
                                                    .child(ae_blue_scrub_field(
                                                        "text_leading_scrub",
                                                        "text_leading:50".to_string(),
                                                        format!("{:.1}", if cur_leading <= 0.0 { 1.2 } else { cur_leading / cur_fs.max(1.0) }),
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                h_flex()
                                                    .gap_1p5()
                                                    .items_center()
                                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("↔"))
                                                    .child(ae_blue_scrub_field(
                                                        "text_tracking_scrub",
                                                        "text_tracking:50".to_string(),
                                                        format!("{cur_tracking:.1}"),
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_1p5()
                                                    .items_center()
                                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("≡"))
                                                    .child(ae_blue_scrub_field(
                                                        "text_box_w_scrub_grid",
                                                        "text_box_w:100".to_string(),
                                                        format!("{:.1}%", if cur_box <= 0.0 { 100.0 } else { cur_box / 6.0 }),
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            ),
                                    );

                                let p_font = panel_entity.clone();
                                let font_group = nested_group(
                                    SharedString::from("text_group_character"),
                                    div().font_semibold().text_color(cx.theme().foreground).child("FONT").into_any_element(),
                                    font_open,
                                    move |cx| {
                                        p_font.update(cx, |this, cx| {
                                            let is_collapsed = this.text_collapsed.contains("font") || this.text_collapsed.contains("character");
                                            if is_collapsed {
                                                this.text_collapsed.remove("font");
                                                this.text_collapsed.remove("character");
                                            } else {
                                                this.text_collapsed.insert("font");
                                                this.text_collapsed.insert("character");
                                            }
                                            cx.notify();
                                        });
                                    },
                                    v_flex()
                                        .gap_2()
                                        .child(font_family_cb)
                                        .child(font_style_cb)
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Weight"))
                                                .child(ae_blue_scrub_field(
                                                    "text_font_weight_scrub",
                                                    "font_weight".to_string(),
                                                    format!("{:.1}", cur_weight as f32),
                                                    &self.state,
                                                    &panel_entity,
                                                    cx,
                                                    move |_| {},
                                                    move |_| {},
                                                )),
                                        )
                                        .child(style_row)
                                        .child(numeric_grid)
                                        .into_any_element(),
                                );

                                // 3. Fill & Stroke Group Collapsible
                                let fill_mode = self.color_picker_mode.get("text_fill").map(|s| s.as_str()).unwrap_or(if cur_col.a <= 0.0 { "none" } else { "color" });
                                let fill_grad = self.color_picker_gradient_colors.get("text_fill").copied().unwrap_or((Color::WHITE, Color::BLACK));
                                let is_fill_active = self.active_color_picker.as_deref() == Some("text_fill");

                                let stroke_mode = self.color_picker_mode.get("text_stroke").map(|s| s.as_str()).unwrap_or(if cur_stroke_w <= 0.0 || cur_stroke.a <= 0.0 { "none" } else { "color" });
                                let stroke_grad = self.color_picker_gradient_colors.get("text_stroke").copied().unwrap_or((Color::WHITE, Color::BLACK));
                                let is_stroke_active = self.active_color_picker.as_deref() == Some("text_stroke");

                                let p_swatch_fill = panel_entity.clone();
                                let p_swatch_stroke = panel_entity.clone();

                                let swatches_row = h_flex()
                                    .gap_3()
                                    .items_center()
                                    .child(
                                        render_color_swatch(
                                            "text_fill_swatch",
                                            cur_col,
                                            fill_mode,
                                            fill_grad,
                                            is_fill_active,
                                            cx,
                                            move |_event, _window, cx| {
                                                p_swatch_fill.update(cx, |this, cx| {
                                                    this.active_color_picker = if this.active_color_picker.as_deref() == Some("text_fill") {
                                                        None
                                                    } else {
                                                        Some("text_fill".to_string())
                                                    };
                                                    cx.notify();
                                                });
                                            },
                                        )
                                    )
                                    .child(
                                        render_color_swatch(
                                            "text_stroke_swatch",
                                            cur_stroke,
                                            stroke_mode,
                                            stroke_grad,
                                            is_stroke_active,
                                            cx,
                                            move |_event, _window, cx| {
                                                p_swatch_stroke.update(cx, |this, cx| {
                                                    this.active_color_picker = if this.active_color_picker.as_deref() == Some("text_stroke") {
                                                        None
                                                    } else {
                                                        Some("text_stroke".to_string())
                                                    };
                                                    cx.notify();
                                                });
                                            },
                                        )
                                    );

                                let color_dialog: Option<AnyElement> = if is_fill_active {
                                    Some(render_three_mode_color_picker("text_fill", cur_col, self, &panel_entity, &self.state, &inspector_color, cx).into_any_element())
                                } else if is_stroke_active {
                                    Some(render_three_mode_color_picker("text_stroke", cur_stroke, self, &panel_entity, &self.state, &inspector_color, cx).into_any_element())
                                } else {
                                    None
                                };

                                let s_st2 = s_typo.clone();
                                let lid_st2 = lid_t.clone();

                                let p_fill_stroke = panel_entity.clone();
                                let fill_stroke_group = nested_group(
                                    SharedString::from("text_group_fill_stroke"),
                                    div().font_semibold().text_color(cx.theme().foreground).child("FILL & STROKE").into_any_element(),
                                    fill_open,
                                    move |cx| {
                                        p_fill_stroke.update(cx, |this, cx| {
                                            if !this.text_collapsed.remove("fill_stroke") {
                                                this.text_collapsed.insert("fill_stroke");
                                            }
                                            cx.notify();
                                        });
                                    },
                                    v_flex()
                                        .gap_2()
                                        .child(swatches_row)
                                        .children(color_dialog)
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Stroke Width"))
                                                .child(
                                                    h_flex()
                                                        .gap_2()
                                                        .items_center()
                                                        .child(ae_blue_scrub_field(
                                                            "text_stroke_w_scrub",
                                                            "text_stroke_w:50".to_string(),
                                                            format!("{cur_stroke_w:.1}"),
                                                            &self.state,
                                                            &panel_entity,
                                                            cx,
                                                            move |_| {},
                                                            move |_| {},
                                                        ))
                                                        .child(
                                                            div()
                                                                .id("text_stroke_2")
                                                                .test_support()
                                                                .child(
                                                                    Button::new("text_stroke_btn_2")
                                                                        .compact()
                                                                        .selected((cur_stroke_w - 2.0).abs() < 0.1)
                                                                        .child("2px")
                                                                        .on_click(move |_, _, cx| {
                                                                            s_st2.update(cx, |s, cx| {
                                                                                let _ = s.set_layer_text_scalar(&lid_st2, "stroke_width", 2.0);
                                                                                cx.notify();
                                                                            });
                                                                        }),
                                                                ),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Position"))
                                                .child(
                                                    h_flex()
                                                        .gap_1()
                                                        .items_center()
                                                        .child({
                                                            let s_pos = s_typo.clone();
                                                            let lid_pos = lid_t.clone();
                                                            Button::new("text_pos_center")
                                                                .compact()
                                                                .selected(cur_stroke_pos == "Center" || cur_stroke_pos.is_empty())
                                                                .child("Center")
                                                                .on_click(move |_, _, cx| {
                                                                    s_pos.update(cx, |s, cx| {
                                                                        let _ = s.set_layer_stroke_position(&lid_pos, "Center");
                                                                        cx.notify();
                                                                    });
                                                                })
                                                        })
                                                        .child({
                                                            let s_pos = s_typo.clone();
                                                            let lid_pos = lid_t.clone();
                                                            Button::new("text_pos_inside")
                                                                .compact()
                                                                .selected(cur_stroke_pos == "Inside")
                                                                .child("Inside")
                                                                .on_click(move |_, _, cx| {
                                                                    s_pos.update(cx, |s, cx| {
                                                                        let _ = s.set_layer_stroke_position(&lid_pos, "Inside");
                                                                        cx.notify();
                                                                    });
                                                                })
                                                        })
                                                        .child({
                                                            let s_pos = s_typo.clone();
                                                            let lid_pos = lid_t.clone();
                                                            Button::new("text_pos_outside")
                                                                .compact()
                                                                .selected(cur_stroke_pos == "Outside")
                                                                .child("Outside")
                                                                .on_click(move |_, _, cx| {
                                                                    s_pos.update(cx, |s, cx| {
                                                                        let _ = s.set_layer_stroke_position(&lid_pos, "Outside");
                                                                        cx.notify();
                                                                    });
                                                                })
                                                        }),
                                                ),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .justify_between()
                                                .text_xs()
                                                .child(div().text_color(cx.theme().muted_foreground).child("Paint Order"))
                                                .child(
                                                    h_flex()
                                                        .gap_1()
                                                        .items_center()
                                                        .child({
                                                            let s_po = s_typo.clone();
                                                            let lid_po = lid_t.clone();
                                                            Button::new("text_po_fos")
                                                                .compact()
                                                                .selected(cur_paint_order == "Fill over Stroke" || cur_paint_order.is_empty())
                                                                .child("Fill over Stroke")
                                                                .on_click(move |_, _, cx| {
                                                                    s_po.update(cx, |s, cx| {
                                                                        let _ = s.set_layer_paint_order(&lid_po, "Fill over Stroke");
                                                                        cx.notify();
                                                                    });
                                                                })
                                                        })
                                                        .child({
                                                            let s_po = s_typo.clone();
                                                            let lid_po = lid_t.clone();
                                                            Button::new("text_po_sof")
                                                                .compact()
                                                                .selected(cur_paint_order == "Stroke over Fill")
                                                                .child("Stroke over Fill")
                                                                .on_click(move |_, _, cx| {
                                                                    s_po.update(cx, |s, cx| {
                                                                        let _ = s.set_layer_paint_order(&lid_po, "Stroke over Fill");
                                                                        cx.notify();
                                                                    });
                                                                })
                                                        }),
                                                ),
                                        )
                                        .into_any_element(),
                                );

                                // 4. Paragraph Group Collapsible
                                let align_presets = [
                                    (project::TextAlign::Left, "Left", "L"),
                                    (project::TextAlign::Center, "Center", "C"),
                                    (project::TextAlign::Right, "Right", "R"),
                                    (project::TextAlign::JustifyLeft, "JustifyLeft", "JL"),
                                    (project::TextAlign::JustifyCenter, "JustifyCenter", "JC"),
                                    (project::TextAlign::JustifyRight, "JustifyRight", "JR"),
                                    (project::TextAlign::JustifyAll, "JustifyAll", "JA"),
                                ];
                                let mut align_row = h_flex().gap_1().items_center().flex_wrap();
                                for (a, id_tag, label) in align_presets {
                                    let s_a = s_typo.clone();
                                    let lid_a = lid_t.clone();
                                    let sel = cur_align == a;
                                    align_row = align_row.child(
                                        div()
                                            .id(SharedString::from(format!("text_align_{id_tag}")))
                                            .test_support()
                                            .child(
                                                Button::new(SharedString::from(format!("text_align_btn_{id_tag}")))
                                                    .compact()
                                                    .selected(sel)
                                                    .child(label)
                                                    .on_click(move |_, _, cx| {
                                                        s_a.update(cx, |s, cx| {
                                                            let _ = s.set_layer_text_align(&lid_a, a);
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    );
                                }

                                let vert_presets = [
                                    ("top", "Top"),
                                    ("center", "Center"),
                                    ("bottom", "Bottom"),
                                ];
                                let mut vert_row = h_flex().gap_1().items_center().flex_wrap();
                                for (v_code, v_label) in vert_presets {
                                    let s_v = s_typo.clone();
                                    let lid_v = lid_t.clone();
                                    let sel = cur_vert_align.eq_ignore_ascii_case(v_code);
                                    let v_str = v_code.to_string();
                                    vert_row = vert_row.child(
                                        Button::new(SharedString::from(format!("text_vert_{v_code}")))
                                            .compact()
                                            .selected(sel)
                                            .child(v_label)
                                            .on_click(move |_, _, cx| {
                                                let vs = v_str.clone();
                                                s_v.update(cx, |s, cx| {
                                                    let _ = s.set_layer_vertical_align(&lid_v, &vs);
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }

                                let p_para = panel_entity.clone();
                                let para_group = nested_group(
                                    SharedString::from("text_group_paragraph"),
                                    div().font_semibold().text_color(cx.theme().foreground).child("PARAGRAPH").into_any_element(),
                                    para_open,
                                    move |cx| {
                                        p_para.update(cx, |this, cx| {
                                            if !this.text_collapsed.remove("paragraph") {
                                                this.text_collapsed.insert("paragraph");
                                            }
                                            cx.notify();
                                        });
                                    },
                                    v_flex()
                                        .gap_2()
                                        .child(
                                            v_flex()
                                                .gap_1()
                                                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Horizontal Alignment"))
                                                .child(align_row),
                                        )
                                        .child(
                                            v_flex()
                                                .gap_1()
                                                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Vertical Alignment"))
                                                .child(vert_row),
                                        )
                                        .into_any_element(),
                                );

                                // 5. Path Group Collapsible
                                let p_path = panel_entity.clone();
                                let path_group = nested_group(
                                    SharedString::from("text_group_path"),
                                    div().font_semibold().text_color(cx.theme().foreground).child("PATH").into_any_element(),
                                    !self.text_collapsed.contains("path"),
                                    move |cx| {
                                        p_path.update(cx, |this, cx| {
                                            if !this.text_collapsed.remove("path") {
                                                this.text_collapsed.insert("path");
                                            }
                                            cx.notify();
                                        });
                                    },
                                    {
                                        let s_tp = self.state.clone();
                                        let lid_tp = lid_t.clone();
                                        let has_path = text_path.is_some();
                                        let count = text_path.as_ref().map(|p| p.points.len()).unwrap_or(0);
                                        h_flex()
                                            .gap_1()
                                            .items_center()
                                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                                                if has_path {
                                                    format!("Text path: {count} pts (Pen adds points)")
                                                } else {
                                                    "Text path: none (select + Pen clicks to draw)".to_string()
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .id("text_path_clear")
                                                    .test_support()
                                                    .child(
                                                        Button::new("text_clear_path_btn")
                                                            .compact()
                                                            .child("Clear")
                                                            .on_click(move |_, _, cx| {
                                                                s_tp.update(cx, |s, cx| {
                                                                    let _ = s.clear_text_path(&lid_tp);
                                                                    cx.notify();
                                                                });
                                                            }),
                                                    ),
                                            )
                                            .into_any_element()
                                    },
                                );

                                let p_src = panel_entity.clone();
                                let text_body = v_flex()
                                    .id("text_properties_section")
                                    .test_support()
                                    .gap_3()
                                    // Header: red icon + Text
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(div().w(px(10.)).h(px(10.)).rounded_xs().bg(rgb(0xef4444)))
                                            .child(div().font_bold().text_sm().text_color(cx.theme().foreground).child("Text")),
                                    )
                                    // Source Text
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(
                                                h_flex()
                                                    .gap_1p5()
                                                    .items_center()
                                                    .child(property_stopwatch(&self.state, &layer.id, "text.source", text.is_animated(), cx))
                                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Source Text")),
                                            )
                                            .child(Input::new(&inputs.text).id("text_content_input").w_full())
                                            .child(text_presets),
                                    )
                                    // Box Size
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .text_xs()
                                            .child(
                                                h_flex()
                                                    .gap_1p5()
                                                    .items_center()
                                                    .child(property_stopwatch(&self.state, &layer.id, "text.box_width", box_width.is_animated(), cx))
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Box Size")),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(ae_blue_scrub_field(
                                                        "text_box_w_scrub",
                                                        "text_box_w:100".to_string(),
                                                        format!("{:.1}", if cur_box <= 0.0 { 600.0 } else { cur_box }),
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    ))
                                                    .child(ae_blue_scrub_field(
                                                        "text_box_h_scrub",
                                                        "text_box_h:100".to_string(),
                                                        format!("{:.1}", if cur_box_h <= 0.0 { 200.0 } else { cur_box_h }),
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            ),
                                    )
                                    // FONT group
                                    .child(font_group)
                                    // FILL & STROKE group
                                    .child(fill_stroke_group)
                                    // PARAGRAPH group
                                    .child(para_group)
                                    // PATH group
                                    .child(path_group);

                                let text_section = prop_section(
                                    "text_source",
                                    format!("Typography & Text (\"{}\")", cur_text),
                                    IconName::Type,
                                    self.source_expanded,
                                    move |cx| {
                                        p_src.update(cx, |this, cx| {
                                            this.source_expanded = !this.source_expanded;
                                            cx.notify();
                                        });
                                    },
                                    cx,
                                    Some(text_body.into_any_element()),
                                );
                                props_items.push(text_section);
                            }
                            LayerSource::Shape { shape_type } => {
                                match shape_type {
                                    ShapeType::Rectangle { width, height, corner_radius, fill } => {
                                        let w = width.value;
                                        let h = height.value;
                                        let cr = corner_radius.value;
                                        let fill_col = *fill;
                                        let s_fill = self.state.clone();
                                        let lid_fill = layer.id.clone();
                                        let mut fill_row = h_flex().gap_1().items_center();
                                        for (hex_str, col_val) in [
                                            ("#FFFFFF", Color::WHITE),
                                            ("#121316", Color::from_hex("#121316").unwrap()),
                                            ("#EF4444", Color::from_hex("#EF4444").unwrap()),
                                            ("#10B981", Color::from_hex("#10B981").unwrap()),
                                            ("#3B82F6", Color::from_hex("#3B82F6").unwrap()),
                                            ("#F59E0B", Color::from_hex("#F59E0B").unwrap()),
                                        ] {
                                            let s_p = s_fill.clone();
                                            let lid_p = lid_fill.clone();
                                            let is_sel = (fill_col.r - col_val.r).abs() < 0.01
                                                && (fill_col.g - col_val.g).abs() < 0.01
                                                && (fill_col.b - col_val.b).abs() < 0.01;
                                            fill_row = fill_row.child(
                                                div()
                                                    .id(SharedString::from(format!("shape_fill_{hex_str}")))
                                                    .test_support()
                                                    .cursor_pointer()
                                                    .w(px(14.))
                                                    .h(px(14.))
                                                    .rounded_sm()
                                                    .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                                                    .border_1()
                                                    .border_color(if is_sel { cx.theme().primary } else { cx.theme().border })
                                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                        s_p.update(cx, |s, cx| {
                                                            let _ = s.set_layer_shape_fill(&lid_p, col_val);
                                                            cx.notify();
                                                        });
                                                    }),
                                            );
                                        }
                                        let shape_mode = self.color_picker_mode.get("shape_fill").map(|s| s.as_str()).unwrap_or(if fill_col.a <= 0.0 { "none" } else { "color" });
                                        let shape_grad = self.color_picker_gradient_colors.get("shape_fill").copied().unwrap_or((Color::WHITE, Color::BLACK));
                                        let is_shape_active = self.active_color_picker.as_deref() == Some("shape_fill");
                                        let p_shape_picker = panel_entity.clone();

                                        let shape_dialog: Option<AnyElement> = if is_shape_active {
                                            Some(render_three_mode_color_picker("shape_fill", fill_col, self, &panel_entity, &self.state, &inspector_color, cx).into_any_element())
                                        } else {
                                            None
                                        };

                                        let rect_body = v_flex()
                                            .id("shape_properties_section")
                                            .test_support()
                                            .gap_2()
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Fill"))
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(
                                                                render_color_swatch(
                                                                    "shape_fill_swatch",
                                                                    fill_col,
                                                                    shape_mode,
                                                                    shape_grad,
                                                                    is_shape_active,
                                                                    cx,
                                                                    move |_event, _window, cx| {
                                                                        p_shape_picker.update(cx, |this, cx| {
                                                                            this.active_color_picker = if this.active_color_picker.as_deref() == Some("shape_fill") {
                                                                                None
                                                                            } else {
                                                                                Some("shape_fill".to_string())
                                                                            };
                                                                            cx.notify();
                                                                        });
                                                                    },
                                                                )
                                                            )
                                                            .child(fill_row),
                                                    ),
                                            )
                                            .children(shape_dialog)
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Width"))
                                                    .child(scrub_field(
                                                        "rect_w_field",
                                                        "rect_w".to_string(),
                                                        format!("{w:.0} px"),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Height"))
                                                    .child(scrub_field(
                                                        "rect_h_field",
                                                        "rect_h".to_string(),
                                                        format!("{h:.0} px"),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Corner Radius"))
                                                    .child(scrub_field(
                                                        "rect_cr_field",
                                                        "rect_cr".to_string(),
                                                        format!("{cr:.0} px"),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            );

                                        let p_src = panel_entity.clone();
                                        let rect_section = prop_section(
                                            "shape_source",
                                            "Rectangle Shape Geometry".to_string(),
                                            IconName::Square,
                                            self.source_expanded,
                                            move |cx| {
                                                p_src.update(cx, |this, cx| {
                                                    this.source_expanded = !this.source_expanded;
                                                    cx.notify();
                                                });
                                            },
                                            cx,
                                            Some(rect_body.into_any_element()),
                                        );
                                        props_items.push(rect_section);
                                    }
                                    ShapeType::Ellipse { radius_x, radius_y, fill } => {
                                        let rx = radius_x.value;
                                        let ry = radius_y.value;
                                        let fill_col = *fill;
                                        let s_fill = self.state.clone();
                                        let lid_fill = layer.id.clone();
                                        let mut fill_row = h_flex().gap_1().items_center();
                                        for (hex_str, col_val) in [
                                            ("#FFFFFF", Color::WHITE),
                                            ("#121316", Color::from_hex("#121316").unwrap()),
                                            ("#EF4444", Color::from_hex("#EF4444").unwrap()),
                                            ("#10B981", Color::from_hex("#10B981").unwrap()),
                                            ("#3B82F6", Color::from_hex("#3B82F6").unwrap()),
                                            ("#F59E0B", Color::from_hex("#F59E0B").unwrap()),
                                        ] {
                                            let s_p = s_fill.clone();
                                            let lid_p = lid_fill.clone();
                                            let is_sel = (fill_col.r - col_val.r).abs() < 0.01
                                                && (fill_col.g - col_val.g).abs() < 0.01
                                                && (fill_col.b - col_val.b).abs() < 0.01;
                                            fill_row = fill_row.child(
                                                div()
                                                    .id(SharedString::from(format!("shape_fill_{hex_str}")))
                                                    .test_support()
                                                    .cursor_pointer()
                                                    .w(px(14.))
                                                    .h(px(14.))
                                                    .rounded_sm()
                                                    .bg(Rgba { r: col_val.r, g: col_val.g, b: col_val.b, a: 1.0 })
                                                    .border_1()
                                                    .border_color(if is_sel { cx.theme().primary } else { cx.theme().border })
                                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                        s_p.update(cx, |s, cx| {
                                                            let _ = s.set_layer_shape_fill(&lid_p, col_val);
                                                            cx.notify();
                                                        });
                                                    }),
                                            );
                                        }
                                        let shape_mode = self.color_picker_mode.get("shape_fill").map(|s| s.as_str()).unwrap_or(if fill_col.a <= 0.0 { "none" } else { "color" });
                                        let shape_grad = self.color_picker_gradient_colors.get("shape_fill").copied().unwrap_or((Color::WHITE, Color::BLACK));
                                        let is_shape_active = self.active_color_picker.as_deref() == Some("shape_fill");
                                        let p_shape_picker = panel_entity.clone();

                                        let shape_dialog: Option<AnyElement> = if is_shape_active {
                                            Some(render_three_mode_color_picker("shape_fill", fill_col, self, &panel_entity, &self.state, &inspector_color, cx).into_any_element())
                                        } else {
                                            None
                                        };

                                        let ellipse_body = v_flex()
                                            .id("shape_properties_section")
                                            .test_support()
                                            .gap_2()
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Fill"))
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(
                                                                render_color_swatch(
                                                                    "ellipse_fill_swatch",
                                                                    fill_col,
                                                                    shape_mode,
                                                                    shape_grad,
                                                                    is_shape_active,
                                                                    cx,
                                                                    move |_event, _window, cx| {
                                                                        p_shape_picker.update(cx, |this, cx| {
                                                                            this.active_color_picker = if this.active_color_picker.as_deref() == Some("shape_fill") {
                                                                                None
                                                                            } else {
                                                                                Some("shape_fill".to_string())
                                                                            };
                                                                            cx.notify();
                                                                        });
                                                                    },
                                                                )
                                                            )
                                                            .child(fill_row),
                                                    ),
                                            )
                                            .children(shape_dialog)
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Radius X"))
                                                    .child(scrub_field(
                                                        "ellipse_rx_field",
                                                        "ellipse_rx".to_string(),
                                                        format!("{rx:.0} px"),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .child(div().text_color(cx.theme().muted_foreground).child("Radius Y"))
                                                    .child(scrub_field(
                                                        "ellipse_ry_field",
                                                        "ellipse_ry".to_string(),
                                                        format!("{ry:.0} px"),
                                                        None,
                                                        None,
                                                        &self.state,
                                                        &panel_entity,
                                                        cx,
                                                        move |_| {},
                                                        move |_| {},
                                                    )),
                                            );

                                        let p_src = panel_entity.clone();
                                        let ellipse_section = prop_section(
                                            "shape_source",
                                            "Ellipse Shape Geometry".to_string(),
                                            IconName::Circle,
                                            self.source_expanded,
                                            move |cx| {
                                                p_src.update(cx, |this, cx| {
                                                    this.source_expanded = !this.source_expanded;
                                                    cx.notify();
                                                });
                                            },
                                            cx,
                                            Some(ellipse_body.into_any_element()),
                                        );
                                        props_items.push(ellipse_section);
                                    }
                                    ShapeType::Path { path_data, .. } => {
                                        let path_body = v_flex()
                                            .id("shape_properties_section")
                                            .test_support()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(format!("Path: {}", if path_data.len() > 40 { format!("{}...", &path_data[..40]) } else { path_data.clone() })),
                                            );

                                        let p_src = panel_entity.clone();
                                        let path_section = prop_section(
                                            "shape_source",
                                            "Vector Pen Path".to_string(),
                                            IconName::Pen,
                                            self.source_expanded,
                                            move |cx| {
                                                p_src.update(cx, |this, cx| {
                                                    this.source_expanded = !this.source_expanded;
                                                    cx.notify();
                                                });
                                            },
                                            cx,
                                            Some(path_body.into_any_element()),
                                        );
                                        props_items.push(path_section);
                                    }
                                }
                            }
                            LayerSource::Image { asset_id } => {
                                let asset = state.project.get_asset(asset_id);
                                let asset_name = asset.map(|a| a.name.clone()).unwrap_or_else(|| asset_id.clone());
                                let path_str = asset.map(|a| a.path.to_string_lossy().to_string()).unwrap_or_default();
                                let media_body = v_flex()
                                    .id("media_properties_section")
                                    .test_support()
                                    .gap_1p5()
                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(path_str));

                                let p_src = panel_entity.clone();
                                let media_section = prop_section(
                                    "media_source",
                                    format!("Image: {asset_name}"),
                                    IconName::Film,
                                    self.source_expanded,
                                    move |cx| {
                                        p_src.update(cx, |this, cx| {
                                            this.source_expanded = !this.source_expanded;
                                            cx.notify();
                                        });
                                    },
                                    cx,
                                    Some(media_body.into_any_element()),
                                );
                                props_items.push(media_section);
                            }
                            LayerSource::Video { asset_id, media_start } => {
                                let asset = state.project.get_asset(asset_id);
                                let asset_name = asset.map(|a| a.name.clone()).unwrap_or_else(|| asset_id.clone());
                                let path_str = asset.map(|a| a.path.to_string_lossy().to_string()).unwrap_or_default();
                                let media_body = v_flex()
                                    .id("media_properties_section")
                                    .test_support()
                                    .gap_1p5()
                                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("Start: {} • File: {}", media_start, path_str)));

                                let p_src = panel_entity.clone();
                                let media_section = prop_section(
                                    "media_source",
                                    format!("Video: {asset_name}"),
                                    IconName::Film,
                                    self.source_expanded,
                                    move |cx| {
                                        p_src.update(cx, |this, cx| {
                                            this.source_expanded = !this.source_expanded;
                                            cx.notify();
                                        });
                                    },
                                    cx,
                                    Some(media_body.into_any_element()),
                                );
                                props_items.push(media_section);
                            }
                            LayerSource::Adjustment => {
                                let adj_body = v_flex()
                                    .id("adjustment_properties_section")
                                    .test_support()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Applies effects and blend modes to all underlying layers."),
                                    );

                                let p_src = panel_entity.clone();
                                let adj_section = prop_section(
                                    "adjustment_source",
                                    "Adjustment Layer".to_string(),
                                    IconName::SlidersHorizontal,
                                    self.source_expanded,
                                    move |cx| {
                                        p_src.update(cx, |this, cx| {
                                            this.source_expanded = !this.source_expanded;
                                            cx.notify();
                                        });
                                    },
                                    cx,
                                    Some(adj_body.into_any_element()),
                                );
                                props_items.push(adj_section);
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
                        let p_switches = panel_entity.clone();
                        let s_blend = self.state.clone();
                        let s_matte = self.state.clone();
                        let s_lock = self.state.clone();
                        let lid_blend = layer.id.clone();
                        let lid_matte = layer.id.clone();
                        let lid_lock = layer.id.clone();
                        let cur_bm = layer.blend_mode;
                        let cur_matte = layer.matte_mode;
                        let is_locked = layer.locked;

                        let switches_body = v_flex()
                            .id("switches_properties_section")
                            .test_support()
                            .gap_2()
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
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .bg(if is_locked { rgb(0xf59e0b).into() } else { cx.theme().muted })
                                            .text_color(if is_locked { rgb(0x000000).into() } else { cx.theme().muted_foreground })
                                            .rounded_sm()
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                s_lock.update(cx, |s, cx| {
                                                    s.toggle_layer_lock(&lid_lock);
                                                    cx.notify();
                                                });
                                            })
                                            .child(icon_box(IconName::Lock))
                                            .child(if is_locked { "Locked" } else { "Lock" }),
                                    ),
                            )
                            // Blend Mode Row
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .child(div().text_color(cx.theme().muted_foreground).child("Blend Mode"))
                                    .child(
                                        div()
                                            .id("props_blend_mode_button")
                                            .test_support()
                                            .cursor_pointer()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .text_color(cx.theme().primary)
                                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                            .child(cur_bm.as_str())
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                let next_bm = match cur_bm {
                                                    project::BlendMode::Normal => project::BlendMode::Multiply,
                                                    project::BlendMode::Multiply => project::BlendMode::Screen,
                                                    project::BlendMode::Screen => project::BlendMode::Overlay,
                                                    project::BlendMode::Overlay => project::BlendMode::Add,
                                                    _ => project::BlendMode::Normal,
                                                };
                                                s_blend.update(cx, |s, cx| {
                                                    s.set_layer_blend_mode(&lid_blend, next_bm);
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            )
                            // Track Matte Row
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .text_xs()
                                    .child(div().text_color(cx.theme().muted_foreground).child("Track Matte"))
                                    .child(
                                        div()
                                            .id("props_track_matte_button")
                                            .test_support()
                                            .cursor_pointer()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(cx.theme().muted)
                                            .text_color(cx.theme().primary)
                                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                            .child(match cur_matte {
                                                project::TrackMatteMode::None => "No Matte",
                                                project::TrackMatteMode::Alpha => "Alpha Matte",
                                                project::TrackMatteMode::AlphaInverted => "Alpha Invert",
                                                project::TrackMatteMode::Luma => "Luma Matte",
                                                project::TrackMatteMode::LumaInverted => "Luma Invert",
                                            })
                                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                let next_matte = match cur_matte {
                                                    project::TrackMatteMode::None => project::TrackMatteMode::Alpha,
                                                    project::TrackMatteMode::Alpha => project::TrackMatteMode::AlphaInverted,
                                                    project::TrackMatteMode::AlphaInverted => project::TrackMatteMode::Luma,
                                                    project::TrackMatteMode::Luma => project::TrackMatteMode::LumaInverted,
                                                    project::TrackMatteMode::LumaInverted => project::TrackMatteMode::None,
                                                };
                                                s_matte.update(cx, |s, cx| {
                                                    s.set_layer_track_matte(&lid_matte, next_matte, None);
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            );

                        let switches_card = prop_section(
                            "switches",
                            "Switches & Compositing".to_string(),
                            IconName::Eye,
                            self.switches_expanded,
                            move |cx| {
                                p_switches.update(cx, |this, cx| {
                                    this.switches_expanded = !this.switches_expanded;
                                    cx.notify();
                                });
                            },
                            cx,
                            Some(switches_body.into_any_element()),
                        );

                        // --- Effects Card ---
                        let p_effects = panel_entity.clone();
                        let effects_card = prop_section(
                            "effects",
                            format!("Applied Effects ({})", layer.effects.len()),
                            IconName::SlidersHorizontal,
                            self.effects_expanded,
                            move |cx| {
                                p_effects.update(cx, |this, cx| {
                                    this.effects_expanded = !this.effects_expanded;
                                    cx.notify();
                                });
                            },
                            cx,
                            Some(effects_list),
                        );

                        props_items.push(transform_card.into_any_element());
                        props_items.push(switches_card);
                        props_items.push(effects_card);
                        // --- Masks Card (first-class vector masks) ---
                        {
                            let p_masks = panel_entity.clone();
                            let shape_view = self.mask_shape_editors.as_ref().map(|e| MaskShapeView {
                                target: e.target.clone(),
                                kind: e.kind,
                                x: e.x.clone(),
                                y: e.y.clone(),
                                w: e.w.clone(),
                                h: e.h.clone(),
                            });
                            let rename_view = self.mask_rename_editor.as_ref().map(|e| MaskRenameView {
                                target: e.target.clone(),
                                editor: e.editor.clone(),
                            });
                            let masks_list = render_masks_section(
                                &self.state,
                                layer,
                                &panel_entity,
                                shape_view,
                                rename_view,
                                self.trace_open,
                                &self.trace_opts,
                                self.trace_error.clone(),
                                self.shape_clipboard.clone(),
                                cx,
                            );
                            let masks_card = prop_section(
                                "masks",
                                format!("Masks ({})", layer.masks.len()),
                                IconName::Scissors,
                                self.masks_expanded,
                                move |cx| {
                                    p_masks.update(cx, |this, cx| {
                                        this.masks_expanded = !this.masks_expanded;
                                        cx.notify();
                                    });
                                },
                                cx,
                                Some(masks_list),
                            );
                            props_items.push(masks_card);
                        }

                        props_items
                    } else {
                        vec![
                            div()
                                .id("properties_empty_placeholder")
                                .test_support()
                                .p_4()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_center()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(icon_box(IconName::Layers))
                                .child("No layer selected")
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground.opacity(0.7))
                                        .child("Select a layer from the timeline or project panel to inspect its properties.")
                                )
                                .into_any_element(),
                            render_tool_settings(&self.state, cx),
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
    /// Collapsed effect categories (all collapsed by default — click a
    /// header to expand, After Effects-style accordion).
    collapsed: HashSet<&'static str>,
}

impl EffectsPanel {
    /// Category keys in display order (all start collapsed).
    const CATEGORIES: &[&'static str] = &[
        "blur", "color", "distort", "generate", "transition", "keying", "text", "custom",
    ];

    fn all_collapsed() -> HashSet<&'static str> {
        Self::CATEGORIES.iter().copied().collect()
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            state: None,
            _subscription: None,
            collapsed: Self::all_collapsed(),
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
            collapsed: Self::all_collapsed(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Expand an effect category (accordion + test helper).
    pub fn expand_category(&mut self, key: &'static str) {
        self.collapsed.remove(key);
    }

    /// True when the category is collapsed (all are by default).
    pub fn is_collapsed(&self, key: &str) -> bool {
        self.collapsed.contains(key)
    }
}

impl EventEmitter<PanelEvent> for EffectsPanel {}

impl Focusable for EffectsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn effect_item_row(
    id_str: &str,
    title: &str,
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
            .child(div().text_xs().text_color(cx.theme().foreground).child(title.to_string())),
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

/// Collapsible category header (After Effects-style accordion). Clicking
/// toggles the category; categories start collapsed.
fn category_header(
    key: &'static str,
    title: &'static str,
    icon: IconName,
    expanded: bool,
    panel: &Entity<EffectsPanel>,
    cx: &App,
) -> impl IntoElement {
    let p_toggle = panel.clone();
    h_flex()
        .id(SharedString::from(format!("effect_category_{key}")))
        .test_support()
        .px_2()
        .py_1()
        .gap_1p5()
        .items_center()
        .font_semibold()
        .text_xs()
        .text_color(cx.theme().foreground)
        .cursor_pointer()
        .rounded_sm()
        .hover(|s| s.bg(cx.theme().muted))
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            p_toggle.update(cx, |this, cx| {
                if this.collapsed.contains(key) {
                    this.collapsed.remove(key);
                } else {
                    this.collapsed.insert(key);
                }
                cx.notify();
            });
        })
        .child(icon_box(icon))
        .child(if expanded { "▼" } else { "▶" })
        .child(title)
}

/// Stable accordion key per OFX category (matches the test hooks).
fn ofx_category_key(cat: project::OfxCategory) -> &'static str {
    match cat {
        project::OfxCategory::Blur => "blur",
        project::OfxCategory::Color => "color",
        project::OfxCategory::Light => "light",
        project::OfxCategory::Key => "keying",
        project::OfxCategory::Distort => "distort",
        project::OfxCategory::Stylize => "stylize",
        project::OfxCategory::Noise => "noise",
        project::OfxCategory::Generate => "generate",
        project::OfxCategory::Spatial => "spatial",
        project::OfxCategory::Cleanup => "cleanup",
        project::OfxCategory::Text => "text",
        project::OfxCategory::Custom => "custom",
    }
}

/// Accordion icon per OFX category.
fn ofx_category_icon(cat: project::OfxCategory) -> IconName {
    match cat {
        project::OfxCategory::Blur => IconName::SlidersHorizontal,
        project::OfxCategory::Color => IconName::Palette,
        project::OfxCategory::Light => IconName::Sun,
        project::OfxCategory::Key => IconName::Scissors,
        project::OfxCategory::Distort => IconName::WandSparkles,
        project::OfxCategory::Stylize => IconName::Sparkles,
        project::OfxCategory::Noise => IconName::Film,
        project::OfxCategory::Generate => IconName::Plus,
        project::OfxCategory::Spatial => IconName::Move,
        project::OfxCategory::Cleanup => IconName::Circle,
        project::OfxCategory::Text => IconName::Type,
        project::OfxCategory::Custom => IconName::Code,
    }
}

/// Default-constructed template for a registry plug-in id: legacy ctors
/// for the hand-rolled suite, descriptor-built stock for everything else.
fn effect_template_for(plugin_id: &str) -> Option<EffectType> {
    if let Some(plugin) = project::stock_from_id(plugin_id) {
        return Some(EffectType::Stock {
            plugin,
            params: EffectType::stock_params(plugin),
            colors: EffectType::stock_colors(plugin),
        });
    }
    Some(match plugin_id {
        "net.sf.openfx.blur" => EffectType::gaussian_blur(10.0),
        "net.sf.openfx.sharpen" => EffectType::sharpen(50.0, 2.0),
        "net.sf.openfx.brightness_contrast" => EffectType::brightness_contrast(15.0, 10.0),
        "net.sf.openfx.levels" => EffectType::levels(0.0, 255.0, 1.0, 0.0, 255.0),
        "net.sf.openfx.hue_saturation" => EffectType::hue_saturation(0.0, 0.0, 0.0),
        "net.sf.openfx.tint" => EffectType::tint(Color::BLACK, Color::WHITE, 100.0),
        "net.sf.openfx.invert" => EffectType::invert(100.0),
        "net.sf.openfx.exposure" => EffectType::exposure(0.0),
        "net.sf.openfx.vibrance" => EffectType::vibrance(30.0),
        "net.sf.openfx.chroma_key" => {
            EffectType::chroma_key(Color::from_hex("#00FF00").unwrap(), 30.0, 10.0)
        }
        "net.sf.openfx.luma_key" => EffectType::luma_key(20.0, 10.0),
        "net.sf.openfx.drop_shadow" => {
            EffectType::drop_shadow(8.0, 45.0, 10.0, 75.0, Color::BLACK)
        }
        "net.sf.openfx.displacement" => EffectType::displacement(50.0, 50.0),
        "net.sf.openfx.perspective" => EffectType::perspective(0.0, 0.0),
        "net.sf.openfx.tiler" => EffectType::tiler(2.0, 2.0),
        "net.sf.openfx.warp" => EffectType::warp(30.0, 1.0),
        "net.sf.openfx.bloom" => EffectType::bloom(40.0, 10.0),
        "net.sf.openfx.vignette" => EffectType::vignette(50.0, 50.0),
        "net.sf.openfx.noise" => EffectType::noise_generator(25.0, true),
        "net.sf.openfx.checkerboard" => {
            EffectType::checkerboard(32.0, Color::BLACK, Color::WHITE)
        }
        "net.sf.openfx.gradient_ramp" => {
            EffectType::gradient_ramp(Color::BLACK, Color::rgb(0.9, 0.3, 0.1), 90.0)
        }
        "net.sf.openfx.text_outline" => EffectType::text_outline(3.0, Color::BLACK),
        "net.sf.openfx.text_bevel" => EffectType::text_bevel(60.0, 30.0),
        "net.sf.openfx.custom.glsl" => EffectType::glsl_shader(
            project::Effect::default_glsl_code(),
            1.0,
            50.0,
            1.0,
            100.0,
        ),
        "net.sf.openfx.custom.shader_lab" => EffectType::shader_lab(shader_presets::GRADE),
        _ => return None,
    })
}

impl Render for EffectsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.entity().clone();
        let is_open = |key: &str| !self.collapsed.contains(key);

        // Registry-driven accordion: every row comes from the OFX suites
        // (`project::ofx`), so new plug-ins appear with zero panel code.
        // Each row creates exactly the plug-in it names.
        let mut cats = v_flex()
            .id("effects_categories")
            .test_support()
            .flex_1()
            .overflow_y_scroll()
            .p_2()
            .gap_1();

        for cat in [
            project::OfxCategory::Blur,
            project::OfxCategory::Color,
            project::OfxCategory::Light,
            project::OfxCategory::Key,
            project::OfxCategory::Distort,
            project::OfxCategory::Stylize,
            project::OfxCategory::Noise,
            project::OfxCategory::Generate,
            project::OfxCategory::Spatial,
            project::OfxCategory::Cleanup,
            project::OfxCategory::Text,
            project::OfxCategory::Custom,
        ] {
            let key = ofx_category_key(cat);
            cats = cats.child(category_header(
                key,
                cat.label(),
                ofx_category_icon(cat),
                is_open(key),
                &panel,
                cx,
            ));
            if is_open(key) {
                for desc in project::ofx_in_category(cat) {
                    if let Some(template) = effect_template_for(desc.id) {
                        let slug = desc.id.rsplit('.').next().unwrap_or(desc.id);
                        cats = cats.child(effect_item_row(slug, desc.label, template, &self.state, cx));
                    }
                }
            }
        }

        div()
            .id("effects_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
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
            // Effects Category List (collapsible accordion)
            .child(cats)
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
                    .child("31 real built-in effects available • Click a category to expand • Click to apply"),
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
    prop_path: &str,
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
    // Owned paths: effect rows pass `effect:<fx_id>:<param>` (the timeline
    // used to pass bare `effect:<param>` with no id, so effect stopwatches,
    // diamonds, and prev/next nav silently no-op'd).
    let p_toggle = prop_path.to_string();
    let p_prev = prop_path.to_string();
    let p_kf = prop_path.to_string();
    let p_next = prop_path.to_string();
    let id_path = prop_path.to_string();

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
                s.toggle_layer_property_animation(&lid1, &p_toggle);
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
                            s.seek_previous_keyframe(&lid2, &p_prev);
                            cx.notify();
                        });
                    }
                })
                .child("◂"),
        )
        .child(
            div()
                .id(SharedString::from(format!("timeline_kf_{layer_id}_{id_path}")))
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
                        s.toggle_layer_keyframe_at_current_time(&lid3, &p_kf);
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
                            s.seek_next_keyframe(&lid4, &p_next);
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

fn timeline_scrub(
    id: impl Into<ElementId>,
    label: &'static str,
    val_str: String,
    layer_id: String,
    value_key: String,
    drag_factor: f32,
    wheel_step: f32,
    state: &Entity<EditorState>,
    panel_entity: &Entity<TimelinePanel>,
    cx: &App,
) -> Div {
    // After Effects-style value pill: drag horizontally to scrub,
    // mouse-wheel for fine steps, click (no drag) for keyboard entry.
    // There are no +/- buttons anywhere on timeline values.
    let edit_key = format!("tl:{}:{}", layer_id, value_key);
    let edit_state = state.read(cx);
    let editor_opt = edit_state.value_editor.clone();
    let is_editing = edit_state.value_edit_key.as_deref() == Some(edit_key.as_str());
    let edit_id = id.into();
    let value_child: AnyElement = match (is_editing, editor_opt) {
        (true, Some(editor)) => Input::new(&editor)
            .id(edit_id.clone())
            .w_full()
            .into_any_element(),
        _ => {
            let panel_down = panel_entity.clone();
            let state_wheel = state.clone();
            let state_fast = state.clone();
            let lid_down = layer_id.clone();
            let key_down = value_key.clone();
            let lid_wheel = layer_id.clone();
            let key_wheel = value_key.clone();
            div()
                .id(edit_id)
                .test_support()
                .px_1p5()
                .py_0p5()
                .rounded_sm()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .font_medium()
                .text_color(cx.theme().foreground)
                .cursor_col_resize()
                .hover(|s| {
                    s.bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                })
                .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                    let curr_x = event.position.x / px(1.0);
                    panel_down.update(cx, |this, _| {
                        this.scrub_layer = Some(lid_down.clone());
                        this.scrub_key = Some(key_down.clone());
                        this.scrub_last_x = Some(curr_x);
                        this.scrub_moved = false;
                        this.scrub_factor = drag_factor;
                    });
                    // Scrubbing previews fast; release restores quality.
                    state_fast.update(cx, |s, cx| {
                        s.checkpoint();
                        s.preview_fast = true;
                        cx.notify();
                    });
                })
                .on_scroll_wheel(move |event, _window, cx| {
                    let dy = match event.delta {
                        ScrollDelta::Pixels(p) => p.y / px(1.0),
                        ScrollDelta::Lines(l) => l.y * 5.0,
                    };
                    if dy != 0.0 {
                        let step = if dy > 0.0 { wheel_step } else { -wheel_step };
                        let lid = lid_wheel.clone();
                        let key = key_wheel.clone();
                        state_wheel.update(cx, |s, cx| {
                            // Discrete wheel step = one undoable nudge.
                            s.checkpoint();
                            s.nudge_timeline_value(&lid, &key, step);
                            cx.notify();
                        });
                    }
                })
                .child(val_str)
                .into_any_element()
        }
    };

    h_flex()
        .gap_1()
        .items_center()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(value_child)
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
    /// Window-space cursor position (cursor-anchored menu).
    pub pos: Point<Pixels>,
}

pub struct TimelinePanel {
    focus_handle: FocusHandle,
    state: Entity<EditorState>,
    _subscription: Subscription,
    expanded_layers: HashSet<String>,
    expanded_groups: HashSet<String>,
    pub active_blend_dropdown: Option<String>,
    /// Layer id whose parent picker is open (timeline Parent pill).
    pub active_parent_dropdown: Option<String>,
    pub context_menu: Option<ContextMenuState>,
    pub is_scrubbing_ruler: bool,
    /// Drag state for layer strip interactions (After Effects-style)
    pub drag_action: Option<TimelineDragAction>,
    pub drag_last_x: f32,
    /// After Effects-style value scrub: which timeline row is being dragged.
    pub scrub_layer: Option<String>,
    pub scrub_key: Option<String>,
    pub scrub_last_x: Option<f32>,
    pub scrub_moved: bool,
    pub scrub_factor: f32,
    /// Active spline/graph keyframe drag (time + value).
    pub graph_drag: Option<GraphKeyDrag>,
    /// Active tangent-handle drag (Bezier curve editing).
    pub graph_tan_drag: Option<GraphTangentDrag>,
    /// Graph Editor value axis (Value / Speed tabs).
    pub graph_tab: GraphTab,
    /// Isolate: plot only the focused series.
    pub graph_isolate: bool,
    /// Grid + keyframe diamond visibility (AE Grid / Keys toggles).
    pub graph_show_grid: bool,
    pub graph_show_keys: bool,
    /// Hidden series (`layer_id:path`) via the legend eye.
    pub graph_hidden: HashSet<String>,
    /// Explicit graph viewport (None = auto-fit full range).
    pub graph_view: Option<GraphViewRect>,
    /// Timeline layer reorder drag: (layer id, from-index).
    pub reorder_drag: Option<(String, usize)>,
    /// Hovered row index as drop target while reordering.
    pub reorder_hover: Option<usize>,
    /// Window-y where the reorder press started (threshold gate).
    pub reorder_start_y: f32,
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

/// Active drag of a single spline/graph keyframe (After Effects Graph Editor).
#[derive(Clone, Debug)]
pub struct GraphKeyDrag {
    pub layer_id: String,
    pub path: String,
    /// Key time at drag start / last committed position (seconds).
    pub at_s: f64,
    pub last_x: f32,
    pub last_y: f32,
    pub v_min: f32,
    pub v_max: f32,
    pub duration: f64,
    /// Visible time span (seconds) for px→dt scaling (zoom-aware).
    pub span: f64,
    /// True once the pointer actually moved (a press without movement is a
    /// click: the mousedown checkpoint is undone so clicks leave no undo).
    pub moved: bool,
}

/// Active drag of one graph tangent handle (Bezier curve editing).
#[derive(Clone, Debug)]
pub struct GraphTangentDrag {
    pub layer_id: String,
    pub path: String,
    pub at_s: f64,
    pub is_in: bool,
    pub hx: f32,
    pub hy: f32,
    pub seg_t: f64,
    pub seg_v: f32,
    pub last_x: f32,
    pub last_y: f32,
    pub span: f64,
    pub vspan: f32,
    pub moved: bool,
}

/// Graph Editor value axis mode (AE Value Graph / Speed Graph tabs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GraphTab {
    #[default]
    Value,
    Speed,
}

/// Render snapshot of the TimelinePanel's graph chrome (the panel itself
/// is already borrowed by render, so values cross by clone).
#[derive(Clone)]
pub struct GraphUi {
    pub tab: GraphTab,
    pub isolate: bool,
    pub show_grid: bool,
    pub show_keys: bool,
    pub hidden: HashSet<String>,
    pub view: Option<GraphViewRect>,
    pub drag: Option<GraphKeyDrag>,
}

/// Explicit graph viewport (AE Fit View / Fit Sel). `None` = auto-fit the
/// full composition range + padded value range.
#[derive(Clone, Copy, Debug)]
pub struct GraphViewRect {
    pub t0: f64,
    pub t1: f64,
    pub v0: f32,
    pub v1: f32,
}

/// Graph legend series row as a value-like component (guide:
/// RenderOnce when all inputs come from the caller and no state is
/// retained between frames). Eye, focus, live value and key toggle.
#[derive(IntoElement)]
struct GraphLegendRow {
    state: Entity<EditorState>,
    panel: Entity<TimelinePanel>,
    layer_id: String,
    path: String,
    label: String,
    color: Rgba,
    value_text: String,
    veiled: bool,
    focused: bool,
    key_here: bool,
}

impl RenderOnce for GraphLegendRow {
    fn render(self, _: &mut Window, _cx: &mut App) -> impl IntoElement {
        let s_focus = self.state.clone();
        let s_key = self.state.clone();
        let p_eye = self.panel.clone();
        let path_c = self.path.clone();
        let path_k = self.path.clone();
        let lid_k = self.layer_id.clone();
        let eye_key = format!("{}:{}", self.layer_id, self.path);
        h_flex()
            .pl_5()
            .pr_1()
            .py_0p5()
            .gap_1p5()
            .items_center()
            .rounded_sm()
            .bg(if self.focused { ae::control() } else { rgb(0x00000000) })
            .child(
                div()
                    .cursor_pointer()
                    .text_color(if self.veiled { ae::dim() } else { ae::text() })
                    .text_xs()
                    .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                        p_eye.update(cx, |this, cx| {
                            this.toggle_graph_series(&eye_key);
                            cx.notify();
                        });
                    })
                    .child(if self.veiled { "○" } else { "◉" }),
            )
            .child(div().w(px(8.)).h(px(2.)).rounded_sm().bg(self.color))
            .child(
                div()
                    .cursor_pointer()
                    .flex_1()
                    .truncate()
                    .text_xs()
                    .text_color(if self.veiled { ae::dim() } else { ae::text() })
                    .hover(|s| s.text_color(rgb(0xffffff)))
                    .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                        let p = path_c.clone();
                        s_focus.update(cx, |s, cx| {
                            s.set_spline_prop_path(&p);
                            cx.notify();
                        });
                    })
                    .child(self.label),
            )
            .child(
                div()
                    .text_xs()
                    .font_medium()
                    .text_color(self.color)
                    .child(self.value_text),
            )
            .child(
                div()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(if self.key_here { ae::amber() } else { ae::dim() })
                    .hover(|s| s.text_color(rgb(0xffffff)))
                    .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                        s_key.update(cx, |s, cx| {
                            s.toggle_layer_keyframe_at_current_time(&lid_k, &path_k);
                            cx.notify();
                        });
                    })
                    .child(if self.key_here { "◆" } else { "◇" }),
            )
    }
}

/// Display unit for a graph path (AE value readouts).
fn graph_unit(path: &str) -> &'static str {    let base = match path.rsplit_once('.') {
        Some((b, c)) if c == "x" || c == "y" => b,
        _ => path,
    };
    match base {
        "transform.anchor_point" | "transform.position" => "px",
        "transform.scale" | "opacity" => "%",
        "transform.rotation" => "°",
        "text.font_size" | "shape.rect_width" | "shape.rect_height" | "shape.corner_radius"
        | "shape.ellipse_rx" | "shape.ellipse_ry" => "px",
        _ => "",
    }
}

/// Graph Editor plot height (shared by layout, tooltip, and drag math).
const GRAPH_PLOT_H: f32 = 150.0;
///
/// Shows one normalized curve per animated scalar property of the selected
/// layer (`graph_series`), sampled via `evaluate_graph_param`. Keyframes are
/// draggable diamonds (time = x, value = y); right-click cycles
/// Linear -> Bezier -> Hold. Background slices seek the playhead.
fn render_graph_view(
    state: &Entity<EditorState>,
    panel_entity: &Entity<TimelinePanel>,
    selected_layer_id: Option<String>,
    gui: &GraphUi,
    cx: &App,
) -> AnyElement {
    let (comp_duration, current_time, fps, spline_prop) = {
        let s = state.read(cx);
        let comp = s.active_composition();
        (
            comp.map(|c| c.duration_seconds()).unwrap_or(5.0),
            s.clock.position_seconds(),
            comp.map(|c| c.frame_rate).unwrap_or(30.0),
            s.spline_prop_path.clone(),
        )
    };
    let duration = comp_duration.max(0.01);

    // Panel chrome state (tabs, isolate, grid/keys, hidden series, view).
    let tab = gui.tab;
    let isolate = gui.isolate;
    let show_grid = gui.show_grid;
    let show_keys = gui.show_keys;
    let hidden = gui.hidden.clone();
    let view_opt = gui.view;
    let gdrag = gui.drag.clone();

    // Header legend data: focused-series matching (exact path, or the
    // `.x`/`.y` pair when focus names a Vec2 base like
    // `transform.position`).
    let lid_opt = selected_layer_id.clone();
    let series: Vec<GraphSeries> = match &lid_opt {
        Some(lid) => state.read(cx).graph_series(lid),
        None => Vec::new(),
    };
    let is_focused = |path: &str| -> bool {
        path == spline_prop
            || (!spline_prop.is_empty()
                && !spline_prop.ends_with(".x")
                && !spline_prop.ends_with(".y")
                && (path == format!("{spline_prop}.x") || path == format!("{spline_prop}.y")))
    };
    let series_key = |lid: &str, path: &str| format!("{lid}:{path}");
    let is_visible = |lid: &str, path: &str| -> bool {
        !hidden.contains(&series_key(lid, path)) && (!isolate || is_focused(path))
    };
    let visible_idx: Vec<usize> = series
        .iter()
        .enumerate()
        .filter(|(_, se)| {
            lid_opt
                .as_deref()
                .map(|lid| is_visible(lid, &se.path))
                .unwrap_or(false)
        })
        .map(|(i, _)| i)
        .collect();
    let focus_idx: Option<usize> = visible_idx
        .iter()
        .find(|&&i| is_focused(&series[i].path))
        .copied()
        .or(visible_idx.first().copied());

    // Readouts for the focused series at the playhead.
    let (focus_label, focus_unit, focus_val, focus_speed) = {
        let s = state.read(cx);
        match (lid_opt.as_deref(), focus_idx) {
            (Some(lid), Some(i)) => {
                let se = &series[i];
                let v = s
                    .evaluate_graph_param(lid, &se.path, current_time)
                    .unwrap_or(0.0);
                let h = (0.5 / fps.max(1.0)).max(1e-4);
                let v0 = s.evaluate_graph_param(lid, &se.path, (current_time - h).max(0.0)).unwrap_or(v);
                let v1 = s.evaluate_graph_param(lid, &se.path, (current_time + h).min(duration)).unwrap_or(v);
                let dt = ((current_time + h).min(duration) - (current_time - h).max(0.0)).max(1e-6);
                (
                    se.label.clone(),
                    graph_unit(&se.path).to_string(),
                    v,
                    ((v1 - v0) as f64 / dt).abs() as f32,
                )
            }
            _ => ("—".to_string(), String::new(), 0.0, 0.0),
        }
    };

    // --- AE header bar: legend count, isolate, tabs, fit, readout, grid ---
    let p_iso = panel_entity.clone();
    let p_tab_v = panel_entity.clone();
    let p_tab_s = panel_entity.clone();
    let p_fit = panel_entity.clone();
    let p_fitsel = panel_entity.clone();
    let p_grid = panel_entity.clone();
    let p_keys = panel_entity.clone();
    let s_fitsel = state.clone();
    let lid_fitsel = lid_opt.clone();
    let focus_fitsel = spline_prop.clone();
    let tab_btn = |id: &'static str, label: &str, on: bool| {
        div()
            .id(id)
            .test_support()
            .cursor_pointer()
            .px_2()
            .py_0p5()
            .rounded_sm()
            .bg(if on { ae::accent() } else { ae::control() })
            .text_color(if on { rgb(0xffffff) } else { ae::text() })
            .hover(|s| s.bg(ae::hover()))
            .text_xs()
            .child(label.to_string())
    };
    let header = h_flex()
        .px_2()
        .py_1()
        .gap_2()
        .items_center()
        .flex_wrap()
        .border_b_1()
        .border_color(ae::border())
        .bg(ae::panel())
        .text_xs()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(div().font_semibold().text_color(ae::text()).child("Graphed Properties"))
                .child(
                    div()
                        .px_1p5()
                        .rounded_sm()
                        .bg(ae::control())
                        .text_color(ae::dim())
                        .child(format!("{} Active", visible_idx.len())),
                ),
        )
        .child(
            div()
                .id("graph_isolate_toggle")
                .test_support()
                .cursor_pointer()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(if isolate { ae::accent() } else { ae::control() })
                .text_color(if isolate { rgb(0xffffff) } else { ae::text() })
                .hover(|s| s.bg(ae::hover()))
                .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                    p_iso.update(cx, |this, cx| {
                        this.graph_isolate = !this.graph_isolate;
                        cx.notify();
                    });
                })
                .child(if isolate { "Isolate" } else { "Animated" }),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    tab_btn("graph_tab_value", "Value Graph", tab == GraphTab::Value)
                        .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                            p_tab_v.update(cx, |this, cx| {
                                this.graph_tab = GraphTab::Value;
                                cx.notify();
                            });
                        }),
                )
                .child(
                    tab_btn("graph_tab_speed", "Speed Graph", tab == GraphTab::Speed)
                        .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                            p_tab_s.update(cx, |this, cx| {
                                this.graph_tab = GraphTab::Speed;
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            div()
                .id("graph_fit_view")
                .test_support()
                .cursor_pointer()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(ae::control())
                .text_color(ae::text())
                .hover(|s| s.bg(ae::hover()))
                .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                    p_fit.update(cx, |this, cx| {
                        this.reset_graph_view();
                        cx.notify();
                    });
                })
                .child("Fit View [F]"),
        )
        .child(
            div()
                .id("graph_fit_sel")
                .test_support()
                .cursor_pointer()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(ae::control())
                .text_color(ae::text())
                .hover(|s| s.bg(ae::hover()))
                .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                    // Fit Sel: frame the focused series' keys (AE parity).
                    let rect = s_fitsel.read(cx).graph_series(&lid_fitsel.clone().unwrap_or_default())
                        .into_iter()
                        .filter(|se| {
                            focus_fitsel.is_empty()
                                || se.path == focus_fitsel
                                || se.path == format!("{}.x", focus_fitsel)
                                || se.path == format!("{}.y", focus_fitsel)
                        })
                        .flat_map(|se| se.keys.into_iter().map(|k| (k.t as f32, k.v)))
                        .fold(None::<(f32, f32, f32, f32)>, |acc, (t, v)| {
                            Some(match acc {
                                None => (t, t, v, v),
                                Some((a, b, c, d)) => (a.min(t), b.max(t), c.min(v), d.max(v)),
                            })
                        })
                        .map(|(a, b, c, d)| {
                            let tp = ((b - a) * 0.1).max(0.25);
                            let vp = ((d - c) * 0.15).max(0.5);
                            GraphViewRect { t0: (a - tp).max(0.0) as f64, t1: (b + tp) as f64, v0: c - vp, v1: d + vp }
                        });
                    p_fitsel.update(cx, |this, cx| {
                        this.graph_view = rect;
                        cx.notify();
                    });
                })
                .child("Fit Sel"),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(div().text_color(ae::dim()).child("X:"))
                .child(
                    div()
                        .id("graph_readout")
                        .test_support()
                        .font_medium()
                        .text_color(ae::timecode())
                        .child(format!("{focus_label}  Value: {focus_val:.1} {focus_unit}  Speed: {focus_speed:.1} {focus_unit}/s")),
                ),
        )
        .child(
            div()
                .id("graph_toggle_grid")
                .test_support()
                .cursor_pointer()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(if show_grid { ae::accent() } else { ae::control() })
                .text_color(if show_grid { rgb(0xffffff) } else { ae::text() })
                .hover(|s| s.bg(ae::hover()))
                .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                    p_grid.update(cx, |this, cx| {
                        this.graph_show_grid = !this.graph_show_grid;
                        cx.notify();
                    });
                })
                .child("Grid"),
        )
        .child(
            div()
                .id("graph_toggle_keys")
                .test_support()
                .cursor_pointer()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(if show_keys { ae::accent() } else { ae::control() })
                .text_color(if show_keys { rgb(0xffffff) } else { ae::text() })
                .hover(|s| s.bg(ae::hover()))
                .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                    p_keys.update(cx, |this, cx| {
                        this.graph_show_keys = !this.graph_show_keys;
                        cx.notify();
                    });
                })
                .child("Keys"),
        );

    // Easing preset bar (applies to focused prop, else all series).
    let mut easing_row = h_flex().gap_1().items_center();
    {
        let presets = [
            (EasingPreset::Linear, "Linear"),
            (EasingPreset::EaseIn, "Ease In"),
            (EasingPreset::EaseOut, "Ease Out"),
            (EasingPreset::EasyEase, "Easy Ease"),
            (EasingPreset::Hold, "Hold"),
        ];
        for (preset, label) in presets {
            let s_e = state.clone();
            let lid_e = lid_opt.clone();
            let focus_e = spline_prop.clone();
            let series_paths: Vec<String> = series.iter().map(|se| se.path.clone()).collect();
            easing_row = easing_row.child(
                div()
                    .cursor_pointer()
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .bg(ae::control())
                    .hover(|s| s.bg(ae::hover()))
                    .text_color(ae::text())
                    .text_xs()
                    .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                        if let Some(ref lid) = lid_e {
                            let paths = series_paths.clone();
                            let focus = focus_e.clone();
                            s_e.update(cx, |s, cx| {
                                s.checkpoint();
                                if !focus.is_empty() && paths.iter().any(|p| p == &focus) {
                                    s.set_layer_property_easing(lid, &focus, preset);
                                } else {
                                    for p in &paths {
                                        s.set_layer_property_easing(lid, p, preset);
                                    }
                                }
                                cx.notify();
                            });
                        }
                    })
                    .child(label),
            );
        }
    }

    // --- Legend column: layer blocks with per-series eye, value, key ---
    // (AE Graphed Properties pane). One block per layer carrying curves;
    // the selected layer always gets a block.
    let legend = {
        let s = state.read(cx);
        let half_frame = 0.5 / fps.max(1.0);
        let layers: Vec<(String, String, bool)> = match s.active_composition() {
            Some(comp) => comp
                .layers
                .iter()
                .map(|l| {
                    (
                        l.id.clone(),
                        l.name.clone(),
                        Some(&l.id) == lid_opt.as_ref(),
                    )
                })
                .collect(),
            None => Vec::new(),
        };
        let mut col = v_flex()
            .id("graph_legend")
            .test_support()
            .w(px(210.))
            .flex_none()
            .gap_0p5()
            .py_1()
            .pr_2()
            .border_r_1()
            .border_color(ae::border())
            .overflow_y_scroll();
        for (llid, lname, is_sel) in layers {
            let lseries = s.graph_series(&llid);
            if lseries.is_empty() && !is_sel {
                continue;
            }
            let mut block = v_flex().gap_0p5().py_1();
            block = block.child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_xs()
                    .font_semibold()
                    .text_color(if is_sel { ae::text() } else { ae::dim() })
                    .child(div().child("▾"))
                    .child(div().truncate().child(lname)),
            );
            if lseries.is_empty() {
                block = block.child(
                    div()
                        .pl_5()
                        .text_xs()
                        .text_color(ae::dim())
                        .child("No Curves"),
                );
            }
            for se in &lseries {
                let key = series_key(&llid, &se.path);
                let veiled = hidden.contains(&key);
                let focused_here = is_sel && is_focused(&se.path);
                let v_now = s
                    .evaluate_graph_param(&llid, &se.path, current_time)
                    .unwrap_or(0.0);
                let unit = graph_unit(&se.path);
                let kf_here = se.keys.iter().any(|k| (k.t - current_time).abs() <= half_frame);
                let scol = Rgba { r: se.color.0, g: se.color.1, b: se.color.2, a: 1.0 };
                block = block.child(GraphLegendRow {
                    state: state.clone(),
                    panel: panel_entity.clone(),
                    layer_id: llid.clone(),
                    path: se.path.clone(),
                    label: se.label.clone(),
                    color: scol,
                    value_text: format!("{v_now:.1} {unit}"),
                    veiled,
                    focused: focused_here,
                    key_here: kf_here,
                });
            }
            col = col.child(block);
        }
        col.into_any_element()
    };

    // Empty states.
    if lid_opt.is_none() {
        return v_flex()
            .flex_1()
            .gap_0()
            .child(header)
            .child(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(ae::dim())
                    .child("Select a layer to edit its splines."),
            )
            .into_any_element();
    }
    if series.is_empty() {
        let s_add = state.clone();
        let lid_add = lid_opt.clone().unwrap_or_default();
        let focus_add = spline_prop.clone();
        return v_flex()
            .flex_1()
            .gap_0()
            .child(header)
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(ae::dim())
                            .child("No keys yet on this layer."),
                    )
                    .child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(ae::accent())
                            .text_color(rgb(0xffffff))
                            .text_xs()
                            .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                                let lid = lid_add.clone();
                                let prop = if focus_add.is_empty() {
                                    "opacity".to_string()
                                } else {
                                    focus_add.clone()
                                };
                                s_add.update(cx, |s, cx| {
                                    s.toggle_layer_keyframe_at_current_time(&lid, &prop);
                                    cx.notify();
                                });
                            })
                            .child("◆ Add key at playhead"),
                    ),
            )
            .into_any_element();
    }

    // View rect: stored (Fit Sel) or auto-fit full range + padded values.
    // Range series = focused visible series when any is focused, else all
    // visible ones (Isolate/hidden respected).
    let range_idx: Vec<usize> = {
        let focused: Vec<usize> = visible_idx.iter().copied().filter(|&i| is_focused(&series[i].path)).collect();
        if focused.is_empty() { visible_idx.clone() } else { focused }
    };
    const SAMPLES: usize = 120;
    // Sampled values per visible series, across the FULL duration (view
    // mapping applied at draw time so Fit Sel never resamples).
    let mut values_per_series: Vec<Vec<f32>> = Vec::new();
    {
        let s = state.read(cx);
        let lid = lid_opt.as_deref().unwrap_or("");
        for &si in &visible_idx {
            let se = &series[si];
            let mut vals = Vec::with_capacity(SAMPLES);
            for i in 0..SAMPLES {
                let t = i as f64 / (SAMPLES - 1) as f64 * duration;
                vals.push(s.evaluate_graph_param(lid, &se.path, t).unwrap_or(0.0));
            }
            values_per_series.push(vals);
        }
    }
    // Auto value range from the range series (samples + key values).
    let (mut a_min, mut a_max) = (f32::MAX, f32::MIN);
    for &si in &range_idx {
        if let Some(pos) = visible_idx.iter().position(|&i| i == si) {
            for &v in &values_per_series[pos] {
                a_min = a_min.min(v);
                a_max = a_max.max(v);
            }
        }
        for k in &series[si].keys {
            a_min = a_min.min(k.v);
            a_max = a_max.max(k.v);
        }
    }
    if !(a_min < a_max) {
        a_min -= 1.0;
        a_max += 1.0;
    }
    let vpad = ((a_max - a_min) * 0.12).max(0.001);
    let auto_view = GraphViewRect { t0: 0.0, t1: duration, v0: a_min - vpad, v1: a_max + vpad };
    let view = view_opt.unwrap_or(auto_view);
    let tspan = (view.t1 - view.t0).max(1e-5);
    let vspan = (view.v1 - view.v0).max(1e-5);
    let x_of = |t: f64| -> f32 { ((t - view.t0) / tspan).clamp(0.0, 1.0) as f32 };
    let y_of = |v: f32| -> f32 { ((v - view.v0) / vspan).clamp(0.0, 1.0) };
    // Speed curves (units/s, central differences over the same samples).
    let speed_per_series: Vec<Vec<f32>> = values_per_series
        .iter()
        .map(|vals| {
            let dt = duration / (SAMPLES - 1) as f64;
            (0..SAMPLES)
                .map(|i| {
                    let (a, b) = if i == 0 {
                        (vals[0], vals[1])
                    } else if i + 1 == SAMPLES {
                        (vals[SAMPLES - 2], vals[SAMPLES - 1])
                    } else {
                        (vals[i - 1], vals[i + 1])
                    };
                    let steps = if i == 0 || i + 1 == SAMPLES { 1.0 } else { 2.0 };
                    ((b - a) as f64 / (dt * steps)).abs() as f32
                })
                .collect()
        })
        .collect();
    // Speed range for the Speed tab (shared across visible series).
    let (s_min, s_max) = {
        let (mut a, mut b) = (f32::MAX, f32::MIN);
        for vals in &speed_per_series {
            for &v in vals {
                a = a.min(v);
                b = b.max(v);
            }
        }
        if !(a < b) {
            a -= 1.0;
            b += 1.0;
        }
        let p = ((b - a) * 0.12).max(0.001);
        (a - p, b + p)
    };
    let sspan = (s_max - s_min).max(1e-5);
    let sy_of = |v: f32| -> f32 { ((v - s_min) / sspan).clamp(0.0, 1.0) };

    let playhead_x = x_of(current_time);
    let lid_graph = lid_opt.clone().unwrap_or_default();
    let half_frame2 = 0.5 / fps.max(1.0);

    // Speed of one series at one time (central difference, units/s).
    let series_speed_at = |lid: &str, path: &str, t: f64| -> f32 {
        let s = state.read(cx);
        let h = half_frame2.max(1e-4);
        let v0 = s.evaluate_graph_param(lid, path, (t - h).max(0.0)).unwrap_or(0.0);
        let v1 = s.evaluate_graph_param(lid, path, (t + h).min(duration)).unwrap_or(v0);
        let dt = ((t + h).min(duration) - (t - h).max(0.0)).max(1e-6);
        ((v1 - v0) as f64 / dt).abs() as f32
    };

    // Tooltip anchor: dragged key wins, else the focused series' key at
    // the playhead (AE pins the readout to the selected key).
    let tooltip: Option<(String, String, f32, f32, f32)> = {
        let pick = |lid: &str, path: &str, t: f64| -> Option<(String, String, f32, f32, f32)> {
            let se = series.iter().find(|se| se.path == path)?;
            let k = se.keys.iter().min_by(|a, b| {
                (a.t - t).abs().partial_cmp(&(b.t - t).abs()).unwrap_or(std::cmp::Ordering::Equal)
            })?;
            let unit = graph_unit(path);
            let tc = TimeCode::from_seconds(k.t.max(0.0), fps);
            let spd = series_speed_at(lid, path, k.t);
            Some((
                format!("{}: {:.1} {unit} @ {tc}", se.label, k.v),
                format!("Speed: {spd:.1} {unit}/s"),
                x_of(k.t),
                y_of(k.v),
                spd,
            ))
        };
        if let Some(gd) = gdrag.clone() {
            pick(&gd.layer_id, &gd.path, gd.at_s)
        } else if let (Some(lid), Some(i)) = (lid_opt.as_deref(), focus_idx) {
            let se = &series[i];
            if se.keys.iter().any(|k| (k.t - current_time).abs() <= half_frame2) {
                pick(lid, &se.path, current_time)
            } else {
                None
            }
        } else {
            None
        }
    };

    // Bottom-bar key position for the focused series.
    let key_info: String = match (lid_opt.as_deref(), focus_idx) {
        (Some(_), Some(i)) => {
            let se = &series[i];
            if se.keys.is_empty() {
                String::new()
            } else {
                let n = se.keys.iter().filter(|k| k.t <= current_time + half_frame2).count().max(1);
                let interp = se
                    .keys
                    .iter()
                    .min_by(|a, b| {
                        (a.t - current_time).abs().partial_cmp(&(b.t - current_time).abs()).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|k| match k.interp {
                        project::KeyframeInterpolation::Linear => "Linear",
                        project::KeyframeInterpolation::Bezier => "Bezier",
                        project::KeyframeInterpolation::Hold => "Hold",
                    })
                    .unwrap_or("");
                format!("Keyframe {n} of {} · {interp}", se.keys.len())
            }
        }
        _ => String::new(),
    };

    // Graph canvas: fixed height plot area (AE graph pane).
    let mut plot = div().flex_1().h(px(GRAPH_PLOT_H)).relative().bg(rgb(0x141414));

    // Grid: horizontal quarters + per-second verticals across the VIEW.
    if show_grid {
        for frac in [0.25f32, 0.5, 0.75] {
            plot = plot.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(frac))
                    .h(px(1.))
                    .bg(rgb(0x2e2e2e)),
            );
        }
        let span_secs = (view.t1 - view.t0).max(0.5);
        let step = ((span_secs / 12.0).ceil().max(1.0)) as i64;
        let mut s = view.t0.ceil() as i64;
        if s < 1 {
            s = step;
        }
        while (s as f64) < view.t1 {
            let f = ((s as f64 - view.t0) / tspan) as f32;
            plot = plot.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(f))
                    .w(px(1.))
                    .bg(rgb(0x262626)),
            );
            s += step;
        }
    }

    // Background seek slices (below curves/keys so keys stay clickable).
    // Mapped across the VIEW so Fit Sel seeks precisely.
    for slice_idx in 0..120 {
        let frac = slice_idx as f64 / 120.0;
        let s_seek = state.clone();
        let target = view.t0 + frac * tspan;
        plot = plot.child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(frac as f32))
                .w(relative(1.0 / 120.0))
                .cursor_col_resize()
                .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                    s_seek.update(cx, |s, cx| {
                        s.seek(target);
                        cx.notify();
                    });
                }),
        );
    }

    // Curves as dense dots (120 per series reads as a line).
    let speed_tab = tab == GraphTab::Speed;
    for (vi, &si) in visible_idx.iter().enumerate() {
        let se = &series[si];
        let col = Rgba { r: se.color.0, g: se.color.1, b: se.color.2, a: 0.95 };
        let dimmed = !is_focused(&se.path);
        let vals = if speed_tab { &speed_per_series[vi] } else { &values_per_series[vi] };
        for (i, v) in vals.iter().enumerate() {
            let t = i as f64 / (SAMPLES - 1) as f64 * duration;
            let (x, y) = if speed_tab {
                (x_of(t), sy_of(*v))
            } else {
                (x_of(t), y_of(*v))
            };
            plot = plot.child(
                div()
                    .absolute()
                    .left(relative(x))
                    .top(relative(1.0 - y))
                    .w(px(2.))
                    .h(px(2.))
                    .ml(px(-1.))
                    .mt(px(-1.))
                    .rounded_full()
                    .bg(col)
                    .opacity(if dimmed { 0.3 } else { 1.0 }),
            );
        }
    }

    // Playhead (red line + square handle, AE style).
    plot = plot
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(1.))
                .bg(rgb(0xef4444))
                .left(relative(playhead_x)),
        )
        .child(
            div()
                .absolute()
                .top(px(0.))
                .left(relative(playhead_x))
                .ml(px(-4.))
                .w(px(9.))
                .h(px(9.))
                .rounded_sm()
                .bg(rgb(0xef4444)),
        );

    // Keyframes: click-drag moves time + value (Value tab), right-click
    // cycles interpolation. Selected keys (dragged or at playhead) render
    // white. The Speed tab is view-only.
    if show_keys {
        for &si in &visible_idx {
            let se = &series[si];
            let focused_here = is_focused(&se.path);
            for k in &se.keys {
                if k.t < view.t0 - half_frame2 || k.t > view.t1 + half_frame2 {
                    continue;
                }
                let x = x_of(k.t);
                let dragging_this = gdrag
                    .as_ref()
                    .map(|gd| gd.layer_id == lid_graph && gd.path == se.path && (gd.at_s - k.t).abs() <= half_frame2)
                    .unwrap_or(false);
                let at_playhead = (k.t - current_time).abs() <= half_frame2;
                let selected = dragging_this || at_playhead;
                let (glyph, glyph_col): (&str, Rgba) = if selected {
                    ("◆", rgb(0xffffff))
                } else {
                    match k.interp {
                        project::KeyframeInterpolation::Linear => ("◆", rgb(0x38bdf8)),
                        project::KeyframeInterpolation::Bezier => ("●", rgb(0x4ade80)),
                        project::KeyframeInterpolation::Hold => ("■", rgb(0xf59e0b)),
                    }
                };
                let y = if speed_tab {
                    sy_of(series_speed_at(&lid_graph, &se.path, k.t))
                } else {
                    y_of(k.v)
                };
                if speed_tab {
                    plot = plot.child(
                        div()
                            .absolute()
                            .left(relative(x))
                            .top(relative(1.0 - y))
                            .ml(px(-8.))
                            .mt(px(-9.))
                            .w(px(16.))
                            .h(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .font_bold()
                            .text_color(glyph_col)
                            .opacity(if focused_here { 1.0 } else { 0.45 })
                            .child(glyph),
                    );
                    continue;
                }
                let p_down = panel_entity.clone();
                let s_down_ck = state.clone();
                let s_cycle = state.clone();
                let lid_k = lid_graph.clone();
                let path_k = se.path.clone();
                let at_s = k.t;
                let lid_r = lid_graph.clone();
                let path_r = se.path.clone();
                plot = plot.child(
                    div()
                        .id(SharedString::from(format!(
                            "graph_key_{}_{}_{}",
                            lid_graph,
                            se.path.replace(['.', ':'], "_"),
                            (k.t * 1000.0).round() as i64
                        )))
                        .test_support()
                        .absolute()
                        .left(relative(x))
                        .top(relative(1.0 - y))
                        .ml(px(-8.))
                        .mt(px(-9.))
                        .w(px(16.))
                        .h(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .font_bold()
                        .cursor_pointer()
                        .text_color(glyph_col)
                        .opacity(if focused_here { 1.0 } else { 0.45 })
                        .hover(|s| s.text_color(rgb(0xffffff)))
                        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                            let cxp = event.position.x / px(1.0);
                            let cyp = event.position.y / px(1.0);
                            s_down_ck.update(cx, |s, cx| {
                                s.checkpoint();
                                s.preview_fast = true;
                                cx.notify();
                            });
                            p_down.update(cx, |this, cx| {
                                this.graph_drag = Some(GraphKeyDrag {
                                    layer_id: lid_k.clone(),
                                    path: path_k.clone(),
                                    at_s,
                                    last_x: cxp,
                                    last_y: cyp,
                                    v_min: view.v0,
                                    v_max: view.v1,
                                    duration,
                                    span: tspan,
                                    moved: false,
                                });
                                cx.notify();
                            });
                        })
                        .on_mouse_down(MouseButton::Right, move |_event, _window, cx| {
                            let lid = lid_r.clone();
                            let path = path_r.clone();
                            s_cycle.update(cx, |s, cx| {
                                s.checkpoint();
                                s.cycle_graph_key_interp(&lid, &path, at_s);
                                cx.notify();
                            });
                        })
                        .child(glyph),
                );
            }
        }
    }

    // Bezier tangent handles for the focused series (Value tab): drag to
    // reshape the curve (Auto/linear defaults show until touched).
    if show_keys && !speed_tab {
        if let Some(i) = focus_idx {
            let se = &series[i];
            for (ki, k) in se.keys.iter().enumerate() {
                if k.interp != project::KeyframeInterpolation::Bezier {
                    continue;
                }
                if k.t < view.t0 - half_frame2 || k.t > view.t1 + half_frame2 {
                    continue;
                }
                let kx = x_of(k.t);
                let ky = y_of(k.v);
                // (neighbor, is_in, default tangent)
                let mut handles: Vec<(usize, bool, (f32, f32))> = Vec::new();
                if ki > 0 {
                    let p = &se.keys[ki - 1];
                    if k.t - p.t > 1e-6 {
                        handles.push((ki - 1, true, k.in_tan.unwrap_or((0.67, 0.67))));
                    }
                }
                if ki + 1 < se.keys.len() {
                    let n = &se.keys[ki + 1];
                    if n.t - k.t > 1e-6 {
                        handles.push((ki + 1, false, k.out_tan.unwrap_or((0.33, 0.33))));
                    }
                }
                for (ni, is_in, (hx, hy)) in handles {
                    let n = &se.keys[ni];
                    let seg_t = (n.t - k.t).abs().max(1e-6);
                    let seg_v = n.v - k.v;
                    let (ht, hv) = if is_in {
                        (k.t - hx as f64 * seg_t, k.v - hy * seg_v)
                    } else {
                        (k.t + hx as f64 * seg_t, k.v + hy * seg_v)
                    };
                    let (hx_r, hy_r) = (x_of(ht), y_of(hv));
                    // Dotted leader key → handle.
                    for s in 1..7 {
                        let f = s as f32 / 7.0;
                        plot = plot.child(
                            div()
                                .absolute()
                                .left(relative(kx + (hx_r - kx) * f))
                                .top(relative(1.0 - (ky + (hy_r - ky) * f)))
                                .w(px(2.))
                                .h(px(2.))
                                .ml(px(-1.))
                                .mt(px(-1.))
                                .rounded_full()
                                .bg(ae::amber())
                                .opacity(0.5),
                        );
                    }
                    let p_tan = panel_entity.clone();
                    let s_tan = state.clone();
                    let lid_t = lid_graph.clone();
                    let path_t = se.path.clone();
                    let at_s = k.t;
                    let side = if is_in { "in" } else { "out" };
                    plot = plot.child(
                        div()
                            .id(SharedString::from(format!(
                                "graph_tan_{}_{}_{}_{}",
                                lid_graph,
                                se.path.replace(['.', ':'], "_"),
                                (k.t * 1000.0).round() as i64,
                                side
                            )))
                            .test_support()
                            .absolute()
                            .left(relative(hx_r))
                            .top(relative(1.0 - hy_r))
                            .ml(px(-5.))
                            .mt(px(-5.))
                            .w(px(10.))
                            .h(px(10.))
                            .rounded_sm()
                            .border_1()
                            .border_color(ae::amber())
                            .bg(rgb(0x141414))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                let cxp = event.position.x / px(1.0);
                                let cyp = event.position.y / px(1.0);
                                s_tan.update(cx, |s, cx| {
                                    s.checkpoint();
                                    s.preview_fast = true;
                                    cx.notify();
                                });
                                p_tan.update(cx, |this, cx| {
                                    this.graph_tan_drag = Some(GraphTangentDrag {
                                        layer_id: lid_t.clone(),
                                        path: path_t.clone(),
                                        at_s,
                                        is_in,
                                        hx,
                                        hy,
                                        seg_t,
                                        seg_v,
                                        last_x: cxp,
                                        last_y: cyp,
                                        span: tspan,
                                        vspan,
                                        moved: false,
                                    });
                                    cx.notify();
                                });
                            })
                            .child(""),
                    );
                }
            }
        }
    }

    // Tooltip pinned to the selected key (value + speed + influence row).
    if let Some((line1, line2, tx, ty, _spd)) = tooltip.clone() {
        let ty_px = (1.0 - ty) * GRAPH_PLOT_H - 52.0;
        plot = plot.child(
            div()
                .id("graph_tooltip")
                .test_support()
                .absolute()
                .left(relative(tx.clamp(0.0, 0.72)))
                .top(px(ty_px.max(2.0)))
                .px_2()
                .py_1()
                .rounded_sm()
                .bg(rgb(0x0a0a0a))
                .border_1()
                .border_color(ae::amber())
                .text_xs()
                .child(
                    div()
                        .font_medium()
                        .text_color(ae::timecode())
                        .child(line1),
                )
                .child(
                    div()
                        .text_color(ae::amber())
                        .child(line2),
                ),
        );
    }

    // Value axis gutter (5 ticks with the focused unit, AE style).
    let axis_unit = focus_idx.map(|i| graph_unit(&series[i].path)).unwrap_or("");
    let (axis_lo, axis_hi) = if speed_tab { (s_min, s_max) } else { (view.v0, view.v1) };
    let mut axis = v_flex()
        .w(px(64.))
        .h(px(GRAPH_PLOT_H))
        .flex_none()
        .justify_between()
        .py_1()
        .pr_2()
        .text_xs()
        .text_color(ae::dim());
    for r in [1.0f32, 0.75, 0.5, 0.25, 0.0] {
        let v = axis_lo + (axis_hi - axis_lo) * r;
        axis = axis.child(div().child(format!("{v:.0} {axis_unit}")));
    }

    // Time ruler across the view (AE `00:01s (30f)` ticks).
    let mut ruler = div()
        .id("graph_ruler")
        .test_support()
        .w_full()
        .h(px(18.))
        .flex_none()
        .relative()
        .text_xs()
        .text_color(ae::dim());
    {
        let span_secs = view.t1 - view.t0;
        let step = ((span_secs / 8.0).ceil().max(1.0)) as i64;
        let mut s = (view.t0.ceil() as i64).max(0);
        if s == 0 {
            s = 0;
        }
        while (s as f64) <= view.t1 + 1e-6 {
            let f = ((s as f64 - view.t0) / tspan) as f32;
            let tc = TimeCode::from_seconds(s.max(0) as f64, fps);
            let fr = (s.max(0) as f64 * fps).round() as i64;
            ruler = ruler.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(f.clamp(0.0, 0.92)))
                    .child(format!("{tc} ({fr}f)")),
            );
            s += step;
            if step <= 0 {
                break;
            }
        }
    }

    // Quick actions for the focused key under the playhead.
    let s_add2 = state.clone();
    let lid_add2 = lid_graph.clone();
    let focus_add2 = spline_prop.clone();
    let focus_label = if focus_add2.is_empty() { "opacity".to_string() } else { focus_add2.clone() };

    v_flex()
        .id("spline_graph_scroll")
        .test_support()
        .flex_1()
        .overflow_y_scroll()
        .bg(ae::bg())
        .text_color(ae::text())
        .child(header)
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .child(legend)
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .px_2()
                        .py_1()
                        .gap_1()
                        .child(ruler)
                        .child(h_flex().gap_0().child(axis).child(plot))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .flex_wrap()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(ae::dim())
                                        .child("Ease:"),
                                )
                                .child(easing_row)
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(ae::control())
                                        .hover(|s| s.bg(ae::hover()))
                                        .text_xs()
                                        .text_color(ae::text())
                                        .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                                            let lid = lid_add2.clone();
                                            let prop = focus_add2.clone();
                                            let prop = if prop.is_empty() { "opacity".to_string() } else { prop };
                                            s_add2.update(cx, |s, cx| {
                                                s.toggle_layer_keyframe_at_current_time(&lid, &prop);
                                                cx.notify();
                                            });
                                        })
                                        .child(format!("◆ Key {focus_label} @ playhead")),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(ae::amber())
                                        .child(key_info),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(ae::dim())
                                        .child(if speed_tab {
                                            "Speed Graph is view-only — switch to Value Graph to drag keys"
                                        } else {
                                            "Click-drag diamonds to move time + value · right-click cycles interp"
                                        }),
                                ),
                        ),
                ),
        )
        .into_any_element()
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
            active_parent_dropdown: None,
            context_menu: None,
            is_scrubbing_ruler: false,
            drag_action: None,
            drag_last_x: 0.0,
            scrub_layer: None,
            scrub_key: None,
            scrub_last_x: None,
            scrub_moved: false,
            scrub_factor: 1.0,
            graph_drag: None,
            graph_tan_drag: None,
            graph_tab: GraphTab::Value,
            graph_isolate: false,
            graph_show_grid: true,
            graph_show_keys: true,
            graph_hidden: HashSet::new(),
            graph_view: None,
            reorder_drag: None,
            reorder_hover: None,
            reorder_start_y: 0.0,
        }
    }

    pub fn open_blend_dropdown(&mut self, layer_id: String) {
        self.active_blend_dropdown = Some(layer_id);
        self.active_parent_dropdown = None;
    }

    pub fn close_blend_dropdown(&mut self) {
        self.active_blend_dropdown = None;
    }

    pub fn open_parent_dropdown(&mut self, layer_id: String) {
        self.active_parent_dropdown = Some(layer_id);
        self.active_blend_dropdown = None;
    }

    pub fn close_parent_dropdown(&mut self) {
        self.active_parent_dropdown = None;
    }

    pub fn open_context_menu(&mut self, target: ContextMenuTarget, pos: Point<Pixels>) {
        self.context_menu = Some(ContextMenuState { target, pos });
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
    }

    /// Open keyboard entry for a timeline value row (`tl:<layer>:<key>`).
    /// Mirrors `PropertiesPanel::begin_value_edit` with layer-targeted keys.
    pub fn begin_timeline_value_edit(
        &mut self,
        layer_id: &str,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Typed entry is one undo step: checkpoint before editing.
        self.state.update(cx, |s, cx| {
            s.checkpoint();
            cx.notify();
        });
        let prop = format!("tl:{layer_id}:{key}");
        let initial = self
            .state
            .read(cx)
            .timeline_current_value(layer_id, key)
            .map(|v| {
                if (v - v.round()).abs() < 1e-4 {
                    format!("{}", v.round() as i64)
                } else {
                    format!("{v:.2}")
                }
            })
            .unwrap_or_default();
        let editor = cx.new(|cx| {
            let mut st = InputState::new(window, cx);
            st.set_value(initial, window, cx);
            st
        });
        let st = self.state.clone();
        let sub = cx.subscribe(
            &editor,
            move |_: &mut Self, input: Entity<InputState>, event: &InputEvent, cx| {
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
            },
        );
        let handle = editor.read(cx).focus_handle(cx);
        self.state.update(cx, |s, _| {
            s.value_edit_key = Some(prop);
            s.value_editor = Some(editor);
            s.value_editor_sub = Some(sub);
        });
        window.focus(&handle, cx);
        cx.notify();
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

    /// Reset the graph viewport to auto-fit (AE Fit View [F]).
    pub fn reset_graph_view(&mut self) {
        self.graph_view = None;
    }

    /// Toggle a legend series' visibility (AE per-series eye).
    pub fn toggle_graph_series(&mut self, key: &str) {
        if self.graph_hidden.contains(key) {
            self.graph_hidden.remove(key);
        } else {
            self.graph_hidden.insert(key.to_string());
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

                let lid = layer.id.clone();
                let lid_vis = layer.id.clone();
                let lid_solo = layer.id.clone();
                let lid_lock = layer.id.clone();
                let lid_matte = layer.id.clone();
                let lid_parent = layer.id.clone();
                let current_matte = layer.matte_mode;

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
                        .bg(ae::span())
                        .text_color(rgb(0xffffff));
                } else {
                    left_col = left_col
                        .bg(cx.theme().background)
                        .text_color(cx.theme().foreground)
                        .hover(|s| s.bg(cx.theme().muted));
                }

                let p_layer_ctx = panel_entity.clone();
                let lid_layer_ctx = layer.id.clone();
                let p_reorder = panel_entity.clone();
                let p_reorder_hover = panel_entity.clone();
                let lid_reorder = layer.id.clone();
                let row_idx = idx;
                // Drop indicator: accent line above the hovered row.
                if self.reorder_hover == Some(idx) && self.reorder_drag.as_ref().map(|(id, _)| id != &layer.id).unwrap_or(false) {
                    left_col = left_col
                        .border_t_2()
                        .border_color(rgb(0x38bdf8));
                }
                let left_col = left_col
                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                        let start_y = event.position.y / px(1.0);
                        sel_state.update(cx, |s, cx| {
                            s.select_layer(Some(lid.clone()));
                            cx.notify();
                        });
                        // Arm a potential reorder drag (commits on mouseup
                        // over another row; plain clicks just select).
                        p_reorder.update(cx, |this, cx| {
                            this.reorder_drag = Some((lid_reorder.clone(), row_idx));
                            this.reorder_hover = None;
                            this.reorder_start_y = start_y;
                            cx.notify();
                        });
                    })
                    .on_mouse_move(move |event, _window, cx| {
                        // Another row hovered mid-drag → drop target.
                        let y = event.position.y / px(1.0);
                        p_reorder_hover.update(cx, |this, cx| {
                            if let Some((_, from)) = this.reorder_drag.clone() {
                                if (y - this.reorder_start_y).abs() > 4.0 && from != row_idx {
                                    if this.reorder_hover != Some(row_idx) {
                                        this.reorder_hover = Some(row_idx);
                                        cx.notify();
                                    }
                                }
                            }
                        });
                    })
                    .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                        let pos = event.position;
                        p_layer_ctx.update(cx, |this, cx| {
                            this.open_context_menu(ContextMenuTarget::Layer(lid_layer_ctx.clone()), pos);
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
                            // Layer Name (reorder via drag-and-drop onto rows)
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
                            // Parent picker: any layer can parent any other
                            // (except itself and its own descendants, which
                            // would cycle). (Un)parenting preserves the
                            // child's world transform.
                            .child({
                                let p_pick = panel_entity.clone();
                                let has_parent = layer.parent_id.is_some();
                                div()
                                    .id(SharedString::from(format!("parent_picker_{}", layer.id)))
                                    .test_support()
                                    .cursor_pointer()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(if has_parent { ae::green() } else { ae::control() })
                                    .text_color(if has_parent { rgb(0xffffff) } else { ae::dim() })
                                    .hover(|s| s.bg(ae::hover()))
                                    .text_xs()
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        let lid = lid_parent.clone();
                                        p_pick.update(cx, |this, cx| {
                                            if this.active_parent_dropdown.as_deref() == Some(&lid) {
                                                this.close_parent_dropdown();
                                            } else {
                                                this.open_parent_dropdown(lid);
                                            }
                                            cx.notify();
                                        });
                                    })
                                    .child(
                                        layer
                                            .parent_id
                                            .clone()
                                            .unwrap_or_else(|| "None".to_string()),
                                    )
                            }),
                    );

                let span_state = self.state.clone();
                let lid_span = layer.id.clone();
                let p_layer_rclick = panel_entity.clone();
                let lid_rclick = layer.id.clone();

                // Drag-start clones for trim-in, body-slip, trim-out
                let p_drag_tin = panel_entity.clone();
                let lid_drag_tin = layer.id.clone();
                let s_drag_tin = self.state.clone();
                let p_drag_slip = panel_entity.clone();
                let lid_drag_slip = layer.id.clone();
                let s_drag_slip = self.state.clone();
                let p_drag_tout = panel_entity.clone();
                let lid_drag_tout = layer.id.clone();
                let s_drag_tout = self.state.clone();
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
                                        p_drag_tin.update(cx, |this, cx| {
                                            this.drag_action = Some(TimelineDragAction::TrimIn {
                                                layer_id: lid_drag_tin.clone(),
                                                initial_mouse_x: mx,
                                                initial_in_frame: layer_in_frame,
                                                initial_out_frame: layer_out_frame,
                                            });
                                            this.drag_last_x = mx;
                                            cx.notify();
                                        });
                                        // Trim gesture start: one undo step.
                                        s_drag_tin.update(cx, |s, cx| {
                                            s.checkpoint();
                                            s.preview_fast = true;
                                            cx.notify();
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
                                        p_drag_slip.update(cx, |this, cx| {
                                            this.drag_action = Some(TimelineDragAction::SlipLayer {
                                                layer_id: lid_drag_slip.clone(),
                                                initial_mouse_x: mx,
                                                initial_in_frame: layer_in_frame,
                                                initial_out_frame: layer_out_frame,
                                            });
                                            this.drag_last_x = mx;
                                            cx.notify();
                                        });
                                        // Slip gesture start: one undo step.
                                        s_drag_slip.update(cx, |s, cx| {
                                            s.checkpoint();
                                            s.preview_fast = true;
                                            cx.notify();
                                        });
                                    })
                                    .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                        let pos = event.position;
                                        p_layer_rclick.update(cx, |this, cx| {
                                            this.open_context_menu(ContextMenuTarget::Layer(lid_rclick.clone()), pos);
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
                                        p_drag_tout.update(cx, |this, cx| {
                                            this.drag_action = Some(TimelineDragAction::TrimOut {
                                                layer_id: lid_drag_tout.clone(),
                                                initial_mouse_x: mx,
                                                initial_in_frame: layer_in_frame,
                                                initial_out_frame: layer_out_frame,
                                            });
                                            this.drag_last_x = mx;
                                            cx.notify();
                                        });
                                        // Trim gesture start: one undo step.
                                        s_drag_tout.update(cx, |s, cx| {
                                            s.checkpoint();
                                            s.preview_fast = true;
                                            cx.notify();
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
                            .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                let pos = event.position;
                                p_prop_ap.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_ap.clone(),
                                        prop_path: "transform.anchor_point",
                                    }, pos);
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
                                    .child(timeline_scrub(SharedString::from(format!("tl_scrub_{}_anchor_x", layer.id)), "X", format!("{:.0}", ap.x), layer.id.clone(), "anchor_x".to_string(), 1.0, 1.0, &self.state, &panel_entity, cx))
                                    .child(timeline_scrub(SharedString::from(format!("tl_scrub_{}_anchor_y", layer.id)), "Y", format!("{:.0}", ap.y), layer.id.clone(), "anchor_y".to_string(), 1.0, 1.0, &self.state, &panel_entity, cx)),
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
                            .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                let pos = event.position;
                                p_prop_pos.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_pos.clone(),
                                        prop_path: "transform.position",
                                    }, pos);
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
                                    .child(timeline_scrub(SharedString::from(format!("tl_scrub_{}_pos_x", layer.id)), "X", format!("{:.0}", pos.x), layer.id.clone(), "pos_x".to_string(), 1.0, 1.0, &self.state, &panel_entity, cx))
                                    .child(timeline_scrub(SharedString::from(format!("tl_scrub_{}_pos_y", layer.id)), "Y", format!("{:.0}", pos.y), layer.id.clone(), "pos_y".to_string(), 1.0, 1.0, &self.state, &panel_entity, cx)),
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
                            .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                let pos = event.position;
                                p_prop_sc.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_sc.clone(),
                                        prop_path: "transform.scale",
                                    }, pos);
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
                                    .child(timeline_scrub(SharedString::from(format!("tl_scrub_{}_scale_x", layer.id)), "X", format!("{:.0}%", sc.x), layer.id.clone(), "scale_x".to_string(), 0.5, 1.0, &self.state, &panel_entity, cx))
                                    .child(timeline_scrub(SharedString::from(format!("tl_scrub_{}_scale_y", layer.id)), "Y", format!("{:.0}%", sc.y), layer.id.clone(), "scale_y".to_string(), 0.5, 1.0, &self.state, &panel_entity, cx)),
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
                            .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                let pos = event.position;
                                p_prop_rot.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_rot.clone(),
                                        prop_path: "transform.rotation",
                                    }, pos);
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
                                timeline_scrub(SharedString::from(format!("tl_scrub_{}_rotation", layer.id)), "Angle", format!("{:.1}°", rot), layer.id.clone(), "rotation".to_string(), 0.25, 1.0, &self.state, &panel_entity, cx),
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
                            .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                let pos = event.position;
                                p_prop_op.update(cx, |this, cx| {
                                    this.open_context_menu(ContextMenuTarget::Property {
                                        layer_id: lid_prop_op.clone(),
                                        prop_path: "opacity",
                                    }, pos);
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
                                timeline_scrub(SharedString::from(format!("tl_scrub_{}_opacity", layer.id)), "Op", format!("{:.0}%", op), layer.id.clone(), "opacity".to_string(), 0.25, 1.0, &self.state, &panel_entity, cx),
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
                                    .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                        let pos = event.position;
                                        p_fx_menu.update(cx, |this, cx| {
                                            this.open_context_menu(ContextMenuTarget::Effect {
                                                layer_id: lid_fx_menu.clone(),
                                                effect_id: eid_fx_menu.clone(),
                                            }, pos);
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
                                    // Animatable parameters render from the
                                    // effect's widget declarations (the same
                                    // data as the Properties panel — no
                                    // per-effect UI in the timeline either).
                                    let mut param_entries: Vec<(String, String, f32, f32)> = Vec::new();
                                    for decl in effect.declarations() {
                                        if !decl.is_scalar() {
                                            continue;
                                        }
                                        if let Some(prop) = effect.get_param_property(&decl.field) {
                                            param_entries.push((
                                                decl.field.clone(),
                                                decl.label.clone(),
                                                prop.evaluate_at(&current_tc),
                                                decl.meta.step,
                                            ));
                                        }
                                    }

                                    for (p_slug, p_label, p_val, p_step) in param_entries {
                                        // Full effect path carries the effect id, so the
                                        // stopwatch / diamond / prev-next nav resolve the
                                        // exact keyframed property.
                                        let fx_prop_path = format!("effect:{}:{p_slug}", effect.id);

                                        let prop_ref = effect.get_param_property(&p_slug);
                                        let is_anim = prop_ref.map(|p| p.is_animated()).unwrap_or(false);
                                        let has_kf = prop_ref.map(|p| p.has_keyframe_at(&current_tc)).unwrap_or(false);
                                        let prev_kf = prop_ref.and_then(|p| p.previous_keyframe_time(&current_tc)).is_some();
                                        let next_kf = prop_ref.and_then(|p| p.next_keyframe_time(&current_tc)).is_some();
                                        let kf_times: Vec<f64> = prop_ref.map(|p| p.keyframes().iter().map(|k| k.time_seconds()).collect()).unwrap_or_default();

                                        let fx_key = format!("fx:{}:{}", effect.id, p_slug);
                                        let fx_drag = (p_step * 0.25).max(0.05);

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
                                                    .child(timeline_stopwatch_nav(&self.state, &layer.id, &fx_prop_path, is_anim, has_kf, prev_kf, next_kf, cx))
                                                    .child(div().w(px(100.)).truncate().text_color(cx.theme().foreground).child(p_label)),
                                            )
                                            .child(
                                                timeline_scrub(SharedString::from(format!("tl_scrub_{}_{}_{}", layer.id, effect.id, p_slug)), "Val", format!("{:.1}", p_val), layer.id.clone(), fx_key, fx_drag, p_step, &self.state, &panel_entity, cx),
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
        let s_root_up = self.state.clone();
        let s_root_up_out = self.state.clone();
        let p_root_move = panel_entity.clone();
        let s_root_move = self.state.clone();

        let mut root = div()
            .id("timeline_panel")
            .test_support()
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_mouse_move(move |event, window, cx| {
                // Tangent-handle drag wins over keyframe drags.
                let ttdrag = p_root_move.read(cx).graph_tan_drag.clone();
                if let Some(mut td) = ttdrag {
                    let cur_x = event.position.x / px(1.0);
                    let cur_y = event.position.y / px(1.0);
                    let dx = cur_x - td.last_x;
                    let dy = cur_y - td.last_y;
                    if dx != 0.0 || dy != 0.0 {
                        let track_w = (window.bounds().size.width / px(1.0) - 560.0).max(200.0);
                        let seg_frac = (td.seg_t / td.span.max(1e-6)).max(1e-6);
                        let nhx = td.hx + ((dx / track_w) as f64 / seg_frac) as f32;
                        let mut nhy = td.hy;
                        if td.seg_v.abs() > 1e-6 {
                            nhy = td.hy + (-dy / GRAPH_PLOT_H * td.vspan) / td.seg_v;
                        }
                        let nhx = nhx.clamp(0.0, 1.0);
                        let nhy = nhy.clamp(-2.0, 2.0);
                        let (lid, path, at_s, is_in) =
                            (td.layer_id.clone(), td.path.clone(), td.at_s, td.is_in);
                        let (it, ot) = if is_in {
                            (Some((nhx, nhy)), None)
                        } else {
                            (None, Some((nhx, nhy)))
                        };
                        s_root_move.update(cx, |s, cx| {
                            s.preview_fast = true;
                            s.set_graph_key_tangents_live(&lid, &path, at_s, it, ot);
                            cx.notify();
                        });
                        td.hx = nhx;
                        td.hy = nhy;
                        td.last_x = cur_x;
                        td.last_y = cur_y;
                        td.moved = true;
                        p_root_move.update(cx, |this, cx| {
                            this.graph_tan_drag = Some(td);
                            cx.notify();
                        });
                    }
                    return;
                }
                // Spline/graph keyframe drag wins over layer-strip drags.
                let gdrag = p_root_move.read(cx).graph_drag.clone();
                if let Some(mut gd) = gdrag {
                    let cur_x = event.position.x / px(1.0);
                    let cur_y = event.position.y / px(1.0);
                    let dx = cur_x - gd.last_x;
                    let dy = cur_y - gd.last_y;
                    if dx != 0.0 || dy != 0.0 {
                        let track_w = (window.bounds().size.width / px(1.0) - 560.0).max(200.0);
                        let graph_h = GRAPH_PLOT_H;
                        let dt = dx / track_w * gd.span as f32;
                        let vspan = (gd.v_max - gd.v_min).max(1e-5);
                        let dv = -dy / graph_h * vspan;
                        // Current value of the dragged key.
                        let cur_v = {
                            let s = s_root_move.read(cx);
                            s.graph_series(&gd.layer_id)
                                .iter()
                                .find(|se| se.path == gd.path)
                                .and_then(|se| {
                                    se.keys
                                        .iter()
                                        .min_by(|a, b| {
                                            (a.t - gd.at_s)
                                                .abs()
                                                .partial_cmp(&(b.t - gd.at_s).abs())
                                                .unwrap_or(std::cmp::Ordering::Equal)
                                        })
                                        .map(|k| k.v)
                                })
                                .unwrap_or(0.0)
                        };
                        let new_t = (gd.at_s + dt as f64).clamp(0.0, gd.duration);
                        let new_v = (cur_v + dv).clamp(gd.v_min, gd.v_max);
                        let lid = gd.layer_id.clone();
                        let path = gd.path.clone();
                        let at_s = gd.at_s;
                        s_root_move.update(cx, |s, cx| {
                            s.preview_fast = true;
                            s.move_graph_keyframe_live(&lid, &path, at_s, new_t, new_v);
                            cx.notify();
                        });
                        gd.at_s = new_t;
                        gd.last_x = cur_x;
                        gd.last_y = cur_y;
                        gd.moved = true;
                        p_root_move.update(cx, |this, cx| {
                            this.graph_drag = Some(gd);
                            cx.notify();
                        });
                    }
                    return;
                }
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
                } else {
                    // After Effects-style value scrub on timeline rows.
                    let scrub = {
                        let p = p_root_move.read(cx);
                        (
                            p.scrub_layer.clone(),
                            p.scrub_key.clone(),
                            p.scrub_last_x,
                            p.scrub_factor,
                        )
                    };
                    if let (Some(lid), Some(key), Some(last_x)) = (scrub.0, scrub.1, scrub.2) {
                        let cur_x = event.position.x / px(1.0);
                        let dx = cur_x - last_x;
                        if dx != 0.0 {
                            let factor = scrub.3;
                            s_root_move.update(cx, |s, cx| {
                                s.nudge_timeline_value(&lid, &key, dx * factor);
                                cx.notify();
                            });
                            p_root_move.update(cx, |this, cx| {
                                this.scrub_last_x = Some(cur_x);
                                this.scrub_moved = true;
                                cx.notify();
                            });
                        }
                    }
                }
            })
            .on_mouse_up(MouseButton::Left, move |_event, window, cx| {
                // Click (no drag) on a value pill opens keyboard entry.
                let edit = {
                    let p = p_root_up.read(cx);
                    match (&p.scrub_layer, &p.scrub_key) {
                        (Some(lid), Some(key)) if !p.scrub_moved => {
                            Some((lid.clone(), key.clone()))
                        }
                        _ => None,
                    }
                };
                // A graph-key press without movement is a click, not a
                // drag: undo the mousedown checkpoint so clicks leave the
                // undo stack untouched.
                let graph_clicked = p_root_up
                    .read(cx)
                    .graph_drag
                    .clone()
                    .map(|gd| !gd.moved)
                    .unwrap_or(false);
                let tan_clicked = p_root_up
                    .read(cx)
                    .graph_tan_drag
                    .clone()
                    .map(|td| !td.moved)
                    .unwrap_or(false);
                p_root_up.update(cx, |this, cx| {
                    this.is_scrubbing_ruler = false;
                    this.drag_action = None;
                    this.graph_drag = None;
                    this.graph_tan_drag = None;
                    this.scrub_layer = None;
                    this.scrub_key = None;
                    this.scrub_last_x = None;
                    this.scrub_moved = false;
                    cx.notify();
                });
                // Commit a layer reorder drop (dragged onto another row).
                if let (Some((drag_id, _)), Some(target)) = (
                    p_root_up.read(cx).reorder_drag.clone(),
                    p_root_up.read(cx).reorder_hover,
                ) {
                    s_root_up.update(cx, |s, cx| {
                        let _ = s.move_layer_to(&drag_id, target);
                        cx.notify();
                    });
                }
                p_root_up.update(cx, |this, cx| {
                    this.reorder_drag = None;
                    this.reorder_hover = None;
                    cx.notify();
                });
                s_root_up.update(cx, |s, cx| {
                    s.preview_fast = false;
                    if graph_clicked || tan_clicked {
                        s.undo();
                    }
                    cx.notify();
                });
                if let Some((lid, key)) = edit {
                    p_root_up.update(cx, |this, cx| {
                        this.begin_timeline_value_edit(&lid, &key, window, cx);
                    });
                }
            })
            .on_mouse_up_out(MouseButton::Left, move |_event, _window, cx| {
                p_root_up_out.update(cx, |this, cx| {
                    this.is_scrubbing_ruler = false;
                    this.drag_action = None;
                    this.graph_drag = None;
                    this.graph_tan_drag = None;
                    this.scrub_layer = None;
                    this.scrub_key = None;
                    this.scrub_last_x = None;
                    this.scrub_moved = false;
                    this.reorder_drag = None;
                    this.reorder_hover = None;
                    cx.notify();
                });
                s_root_up_out.update(cx, |s, cx| {
                    s.preview_fast = false;
                    cx.notify();
                });
            })
            // Header / Timecode & Transport (AE timeline toolbar chrome).
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(ae::border())
                    .bg(ae::panel())
                    .items_center()
                    .justify_between()
                    // Timecode display
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(ae::text())
                                    .child(format!(
                                        "Timeline: {}",
                                        comp_opt.map(|c| c.name.clone()).unwrap_or_else(|| "No Comp".to_string())
                                    )),
                            )
                            .child(
                                div()
                                    .id("timecode_display")
                                    .test_support()
                                    .px_2()
                                    .py_0p5()
                                    .bg(rgb(0x101010))
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(ae::border())
                                    .font_bold()
                                    .text_sm()
                                    .text_color(ae::timecode())
                                    .child(format!("{current_tc}")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(ae::dim())
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
                                    .child(
                                        Button::new("transport_start_btn")
                                            .compact()
                                            .tooltip("Go to Start (Home)")
                                            .child(icon_box(IconName::SkipBack))
                                            .on_click(move |_, _, cx| {
                                                s_start.update(cx, |s, cx| {
                                                    s.jump_to_start();
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("transport_prev")
                                    .test_support()
                                    .child(
                                        Button::new("transport_prev_btn")
                                            .compact()
                                            .tooltip("Previous Frame (Left)")
                                            .child(icon_box(IconName::StepBack))
                                            .on_click(move |_, _, cx| {
                                                s_step_prev.update(cx, |s, cx| {
                                                    s.step_backward();
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("transport_play")
                                    .test_support()
                                    .child(
                                        Button::new("transport_play_btn")
                                            .compact()
                                            .toggled(is_playing)
                                            .tooltip("Play / Pause (Space)")
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
                                            )
                                            .on_click(move |_, _, cx| {
                                                s_play.update(cx, |s, cx| {
                                                    s.toggle_playback();
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("transport_next")
                                    .test_support()
                                    .child(
                                        Button::new("transport_next_btn")
                                            .compact()
                                            .tooltip("Next Frame (Right)")
                                            .child(icon_box(IconName::StepForward))
                                            .on_click(move |_, _, cx| {
                                                s_step_next.update(cx, |s, cx| {
                                                    s.step_forward();
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("transport_end")
                                    .test_support()
                                    .child(
                                        Button::new("transport_end_btn")
                                            .compact()
                                            .tooltip("Go to End (End)")
                                            .child(icon_box(IconName::SkipForward))
                                            .on_click(move |_, _, cx| {
                                                s_end.update(cx, |s, cx| {
                                                    s.jump_to_end();
                                                    cx.notify();
                                                });
                                            }),
                                    ),
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
                            .child({
                                let s_graph_tl = self.state.clone();
                                let s_graph_gr = self.state.clone();
                                let is_graph = self.state.read(cx).spline_editor_open;
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        div()
                                            .id("timeline_view_timeline")
                                            .test_support()
                                            .cursor_pointer()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(if !is_graph { cx.theme().primary } else { cx.theme().muted })
                                            .text_color(if !is_graph { cx.theme().primary_foreground } else { cx.theme().foreground })
                                            .text_xs()
                                            .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                                                s_graph_tl.update(cx, |s, cx| {
                                                    if s.spline_editor_open {
                                                        s.toggle_spline_editor();
                                                    }
                                                    cx.notify();
                                                });
                                            })
                                            .child("Timeline"),
                                    )
                                    .child(
                                        div()
                                            .id("timeline_view_graph")
                                            .test_support()
                                            .cursor_pointer()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(if is_graph { cx.theme().primary } else { cx.theme().muted })
                                            .text_color(if is_graph { cx.theme().primary_foreground } else { cx.theme().foreground })
                                            .text_xs()
                                            .on_mouse_down(MouseButton::Left, move |_e, _w, cx| {
                                                s_graph_gr.update(cx, |s, cx| {
                                                    if !s.spline_editor_open {
                                                        s.toggle_spline_editor();
                                                    }
                                                    cx.notify();
                                                });
                                            })
                                            .child("Graph"),
                                    )
                            })
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    // Reorder is drag-and-drop onto rows.
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
                    .border_color(ae::border())
                    .bg(ae::panel())
                    .text_xs()
                    .text_color(ae::dim())
                    .child(
                        div()
                            .w(px(380.))
                            .px_3()
                            .border_r_1()
                            .border_color(ae::border())
                            .child("Layer Name / Switches · Parent & Link"),
                    )
                    .child({
                        let p_ruler_down = panel_entity.clone();
                        let s_ruler_down = self.state.clone();
                        let mut ruler_track = div()
                            .id("ruler_track")
                            .test_support()
                            .flex_1()
                            .relative()
                            .h_full()
                            .cursor_col_resize()
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                p_ruler_down.update(cx, |this, cx| {
                                    this.is_scrubbing_ruler = true;
                                    cx.notify();
                                });
                                s_ruler_down.update(cx, |s, cx| {
                                    s.preview_fast = true;
                                    cx.notify();
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
                                        p_slice_down.update(cx, |this, cx| {
                                            this.is_scrubbing_ruler = true;
                                            cx.notify();
                                        });
                                        s_scrub.update(cx, |s, cx| {
                                            s.preview_fast = true;
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
            // Tracks area: classic rows or spline/graph editor.
            .child({
                let p_tl_ctx = panel_entity.clone();
                if state.spline_editor_open {
                    let sel = state.selected_layer_id.clone();
                    let gui = GraphUi {
                        tab: self.graph_tab,
                        isolate: self.graph_isolate,
                        show_grid: self.graph_show_grid,
                        show_keys: self.graph_show_keys,
                        hidden: self.graph_hidden.clone(),
                        view: self.graph_view,
                        drag: self.graph_drag.clone(),
                    };
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(render_graph_view(&self.state, &panel_entity, sel, &gui, cx))
                        .into_any_element()
                } else {
                    v_flex()
                        .id("timeline")
                        .test_support()
                        .flex_1()
                        .overflow_y_scroll()
                        .overflow_x_scroll()
                        .children(timeline_rows)
                        .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                            let pos = event.position;
                            p_tl_ctx.update(cx, |this, cx| {
                                this.open_context_menu(ContextMenuTarget::EmptyTrackArea, pos);
                                cx.notify();
                            });
                        })
                        .into_any_element()
                }
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

        // Parent Picker Dropdown Overlay: every layer except the target
        // itself and its descendants (which would create a cycle), plus
        // None to unparent. (Un)parenting preserves world transform.
        if let Some(ref target_lid) = self.active_parent_dropdown {
            let s_pick = self.state.clone();
            let p_close = panel_entity.clone();
            let target_lid_str = target_lid.clone();

            // Candidate parent ids + display names in layer order.
            let (candidates, current_parent) = {
                let s = self.state.read(cx);
                match s.active_composition() {
                    Some(comp) => {
                        let mut forbidden = vec![target_lid_str.clone()];
                        let mut stack = vec![target_lid_str.clone()];
                        while let Some(id) = stack.pop() {
                            for child in comp.get_children(&id) {
                                forbidden.push(child.id.clone());
                                stack.push(child.id.clone());
                            }
                        }
                        let cands: Vec<(String, String)> = comp
                            .layers
                            .iter()
                            .filter(|l| !forbidden.contains(&l.id))
                            .map(|l| (l.id.clone(), l.name.clone()))
                            .collect();
                        let cur = comp
                            .get_layer(&target_lid_str)
                            .and_then(|l| l.parent_id.clone());
                        (cands, cur)
                    }
                    None => (Vec::new(), None),
                }
            };

            let mut list = v_flex()
                .id("parent_pick_list")
                .test_support()
                .gap_0p5()
                .p_1()
                .overflow_y_scroll();
            {
                let s_item = s_pick.clone();
                let p_close_item = p_close.clone();
                let target_lid_item = target_lid_str.clone();
                let is_cur = current_parent.is_none();
                list = list.child(
                    div()
                        .id("parent_pick_none")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .text_xs()
                        .bg(if is_cur {
                            cx.theme().accent
                        } else {
                            cx.theme().secondary
                        })
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_item.update(cx, |s, cx| {
                                s.set_layer_parent(&target_lid_item, None);
                                cx.notify();
                            });
                            p_close_item.update(cx, |this, cx| {
                                this.close_parent_dropdown();
                                cx.notify();
                            });
                        })
                        .child("None (unparent)"),
                );
            }
            for (cid, cname) in candidates {
                let s_item = s_pick.clone();
                let p_close_item = p_close.clone();
                let target_lid_item = target_lid_str.clone();
                let is_cur = current_parent.as_deref() == Some(&cid);
                let label = format!("{cname} ({cid})");
                list = list.child(
                    div()
                        .id(SharedString::from(format!("parent_pick_{cid}")))
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .text_xs()
                        .bg(if is_cur {
                            cx.theme().accent
                        } else {
                            cx.theme().secondary
                        })
                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_item.update(cx, |s, cx| {
                                s.set_layer_parent(&target_lid_item, Some(cid.clone()));
                                cx.notify();
                            });
                            p_close_item.update(cx, |this, cx| {
                                this.close_parent_dropdown();
                                cx.notify();
                            });
                        })
                        .child(label),
                );
            }

            let p_close_bg = p_close.clone();
            let parent_dropdown_overlay = div()
                .id("parent_picker_dropdown")
                .test_support()
                .absolute()
                .top(px(40.))
                .left(px(180.))
                .w(px(260.))
                .h(px(300.))
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
                                .child(div().font_bold().text_xs().child("Parent Layer"))
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_xs()
                                        .hover(|s| s.text_color(rgb(0xef4444)))
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            p_close_bg.update(cx, |this, cx| {
                                                this.close_parent_dropdown();
                                                cx.notify();
                                            });
                                        })
                                        .child("✕"),
                                ),
                        )
                        .child(list),
                );
            root = root.child(parent_dropdown_overlay);
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

                    let s_piv = s_menu.clone();
                    let p_piv = p_close.clone();
                    let t_piv = target_lid.clone();
                    menu_items = menu_items.child(
                        div()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                s_piv.update(cx, |s, cx| {
                                    s.reset_layer_anchor_center(&t_piv);
                                    cx.notify();
                                });
                                p_piv.update(cx, |this, cx| {
                                    this.close_context_menu();
                                    cx.notify();
                                });
                            })
                            .child("Center Pivot"),
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

            // Cursor-anchored: opens exactly under the mouse (viewport
            // clamped), not at a fixed corner.
            let tl_menu_pos = ctx_menu.pos;
            let context_menu_overlay = deferred(
                Positioner::corner(Anchor::TopLeft, tl_menu_pos)
                    .margin(px(8.))
                    .occlude()
                    .child(
                        div()
                            .id("timeline_context_menu")
                            .test_support()
                            .w(px(220.))
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_md()
                            .shadow_lg()
                            .child(menu_items),
                    ),
            );
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
