mod panels;
pub mod state;
use state::EditorState;

use std::rc::Rc;

pub use gpui_kit::base::{h_flex, v_flex, StyledExt, TestSupportExt};
pub use gpui_kit::component::dock::{
    panel_handle, BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, Panel, PanelStyle,
};
use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeMode};
use gpui_kit::*;

pub use panels::{
    AppPanels, CompositionPanel, CompositionViewerPanel, EffectsPanel, ProjectPanel,
    PropertiesPanel, TimelinePanel,
};

actions!(workspace, [TogglePlayback]);

pub struct AppView {
    state: Entity<EditorState>,
    dock_area: Entity<DockArea>,
    dock_skin: Rc<DockSkin>,
    panels: AppPanels,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _playback_task: Option<Task<()>>,
    /// Last window size the dock layout was fitted to (responsive docks).
    docks_sized_for: Option<(i32, i32)>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let state = cx.new(|_| EditorState::new());
        Self::new_with_state(state, window, cx)
    }

    pub fn new_with_state(state: Entity<EditorState>, window: &mut Window, cx: &mut App) -> Self {
        let (dock_area, dock_skin) = DockSkin::dock_area("workspace_dock", None, window, cx);
        dock_skin.set_panel_style(PanelStyle::TabBar, cx);
        let panels = AppPanels::new(state.clone(), cx);

        // Bind spacebar key to TogglePlayback
        cx.bind_keys([KeyBinding::new("space", TogglePlayback, None)]);

        // Setup background 60Hz playback loop
        let loop_state = state.clone();
        let playback_task = cx.spawn(|cx: &mut AsyncApp| {
            let cx = cx.clone();
            let state = loop_state;
            async move {
                let mut last_instant = std::time::Instant::now();
                loop {
                    cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                    let now = std::time::Instant::now();
                    let dt = now - last_instant;
                    last_instant = now;
                    cx.update(|cx| {
                        state.update(cx, |editor, cx| {
                            if editor.is_playing {
                                let changed = editor.tick(dt);
                                if changed {
                                    cx.notify();
                                }
                            }
                        });
                    });
                }
            }
        });

        // Left dock: Project / Assets
        let left_layout =
            DockLayout::tabs().panel_view(panel_handle(panels.project.clone()), cx);

        // Center dock: Composition Viewer
        let center_layout =
            DockLayout::tabs().panel_view(panel_handle(panels.composition.clone()), cx);

        // Right dock: Tab group containing Properties and Effects tabs (After Effects workspace layout)
        let right_layout = DockLayout::tabs()
            .panel_view(panel_handle(panels.properties.clone()), cx)
            .panel_view(panel_handle(panels.effects.clone()), cx);

        // Bottom dock: Timeline
        let bottom_layout =
            DockLayout::tabs().panel_view(panel_handle(panels.timeline.clone()), cx);

        dock_area.update(cx, |dock, cx| {
            dock.set_dock(DockPlacement::Left, left_layout, window, cx);
            dock.set_dock_size(DockPlacement::Left, px(280.), window, cx);

            dock.set_center(center_layout, window, cx);

            dock.set_dock(DockPlacement::Right, right_layout, window, cx);
            dock.set_dock_size(DockPlacement::Right, px(300.), window, cx);

            dock.set_dock(DockPlacement::Bottom, bottom_layout, window, cx);
            dock.set_dock_size(DockPlacement::Bottom, px(260.), window, cx);
        });

        let focus_handle = cx.focus_handle();

        // cx.on_action(|this, _: &TogglePlayback, cx| {});

        Self {
            state,
            dock_area,
            dock_skin,
            panels,
            focus_handle,
            _subscriptions: Vec::new(),
            _playback_task: Some(playback_task),
            docks_sized_for: None,
        }
    }

    pub fn state(&self) -> &Entity<EditorState> {
        &self.state
    }

    pub fn dock_area(&self) -> &Entity<DockArea> {
        &self.dock_area
    }

    pub fn dock_skin(&self) -> &Rc<DockSkin> {
        &self.dock_skin
    }

    pub fn panels(&self) -> &AppPanels {
        &self.panels
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }
}

fn render_toolbar(state: &Entity<EditorState>, cx: &App) -> impl IntoElement {
    let s_read = state.read(cx);
    let active_tool = s_read.active_tool;
    let s_full = state.clone();
    let is_full_width = s_read.timeline_full_width;
    let full_width_btn = div()
        .id("toggle_timeline_full_width_button")
        .test_support()
        .cursor_pointer()
        .px_2()
        .py_1()
        .rounded_sm()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .bg(if is_full_width { cx.theme().primary } else { cx.theme().muted })
        .text_color(if is_full_width { cx.theme().primary_foreground } else { cx.theme().foreground })
        .hover(|s| s.opacity(0.85))
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            s_full.update(cx, |s, cx| {
                s.toggle_timeline_full_width();
                cx.notify();
            });
        })
        .child(div().w(px(14.)).h(px(14.)).flex().items_center().justify_center().child(if is_full_width { gpui_kit::assets::IconName::Minimize2 } else { gpui_kit::assets::IconName::Maximize2 }))
        .child(if is_full_width { "Timeline: Full Width" } else { "Timeline: Docked" });

    h_flex()
        .id("top_toolbar")
        .test_support()
        .w_full()
        .h(px(36.))
        .px_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .items_center()
        .justify_between()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .font_bold()
                        .text_xs()
                        .child(div().w(px(16.)).h(px(16.)).flex().items_center().justify_center().child(gpui_kit::assets::IconName::Film))
                        .child("Motion Studio"),
                )
                .child(div().w(px(1.)).h(px(16.)).bg(cx.theme().border).mx_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Tool: {active_tool:?}")),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(full_width_btn),
        )
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state_key = self.state.clone();
        let toolbar = render_toolbar(&self.state, cx);

        // Responsive docks: refit fixed dock rails when the window size
        // changes so panels never push UI beyond the screen. User dock
        // resizes are preserved (only window changes re-fit).
        let vw = (window.bounds().size.width / px(1.0)).round() as i32;
        let vh = (window.bounds().size.height / px(1.0)).round() as i32;
        if self.docks_sized_for != Some((vw, vh)) {
            self.docks_sized_for = Some((vw, vh));
            let vw_f = vw as f32;
            let vh_f = vh as f32;
            let left = px(vw_f * 0.22).max(px(200.)).min(px(320.));
            let right = px(vw_f * 0.23).max(px(220.)).min(px(340.));
            let bottom = px(vh_f * 0.32).max(px(180.)).min(px(320.));
            self.dock_area.update(cx, |dock, cx| {
                dock.set_dock_size(DockPlacement::Left, left, &mut *window, cx);
                dock.set_dock_size(DockPlacement::Right, right, &mut *window, cx);
                dock.set_dock_size(DockPlacement::Bottom, bottom, &mut *window, cx);
            });
        }

        let is_full = self.state.read(cx).timeline_full_width;
        let dock_open = self.dock_area.read(cx).is_dock_open(DockPlacement::Bottom);
        if is_full && dock_open {
            self.dock_area.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Bottom, window, cx);
            });
        } else if !is_full && !dock_open {
            self.dock_area.update(cx, |dock, cx| {
                dock.toggle_dock(DockPlacement::Bottom, window, cx);
            });
        }

        let main_workspace = if is_full {
            v_flex()
                .flex_1()
                .size_full()
                .overflow_hidden()
                .child(div().flex_1().size_full().child(self.dock_area.clone()))
                .child(
                    div()
                        .w_full()
                        .h(px(260.))
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(self.panels.timeline.clone()),
                )
                .into_any_element()
        } else {
            div().flex_1().size_full().child(self.dock_area.clone()).into_any_element()
        };

        div()
            .id("app_view")
            .track_focus(&self.focus_handle)
            .test_support()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action({
                let state = self.state.clone();
                move |_: &TogglePlayback, _window, cx| {
                    state.update(cx, |s, cx| {
                        s.toggle_playback();
                        cx.notify();
                    });
                }
            })
            .on_key_down(move |event, _window, cx| {
                let key = event.keystroke.key.to_lowercase();
                if key == "space" || key == " " {
                    state_key.update(cx, |s, cx| {
                        s.toggle_playback();
                        cx.notify();
                    });
                } else if key == "delete" || key == "backspace" {
                    state_key.update(cx, |s, cx| {
                        let _ = s.delete_selected_layer();
                        cx.notify();
                    });
                } else if key == "v" {
                    state_key.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Move);
                        cx.notify();
                    });
                } else if key == "h" {
                    state_key.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Hand);
                        cx.notify();
                    });
                } else if key == "w" {
                    state_key.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Rotate);
                        cx.notify();
                    });
                } else if key == "g" {
                    state_key.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Pen);
                        cx.notify();
                    });
                } else if key == "t" {
                    state_key.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Text);
                        cx.notify();
                    });
                } else if key == "q" {
                    state_key.update(cx, |s, cx| {
                        s.cycle_shape_tool();
                        cx.notify();
                    });
                } else if key == "[" {
                    state_key.update(cx, |s, cx| {
                        let _ = s.trim_selected_layer_in_to_playhead();
                        cx.notify();
                    });
                } else if key == "]" {
                    state_key.update(cx, |s, cx| {
                        let _ = s.trim_selected_layer_out_to_playhead();
                        cx.notify();
                    });
                }
            })
            .child(toolbar)
            .child(main_workspace)
    }
}

fn main() {
    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    app.run(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.activate_window();
                window.set_window_title("Motion Compositor");
                Theme::change(ThemeMode::Dark, Some(window), cx);
                let view = cx.new(|cx| AppView::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            },
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::AppView;
    use gpui_kit::component::dock::{DockPlacement, PanelId};
    use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeMode};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{px, size, AppContext as _, Entity, SharedString, TestAppContext};

    fn setup_test_window(cx: &mut TestAppContext) -> (Entity<Root>, Entity<AppView>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
            assert_eq!(cx.theme().mode, ThemeMode::Dark);
        });

        let mut app_view_entity = None;
        let (root, _window) = cx.add_window_view(|window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        (root, app_view_entity.expect("AppView created"))
    }

    #[gpui_kit::test]
    fn test_app_bootstrap_and_window(cx: &mut TestAppContext) {
        let (root, _) = setup_test_window(cx);
        assert_eq!(cx.windows().len(), 1);
        cx.run_until_parked();
        root.read_with(cx, |_root, _cx| ());
    }

    #[gpui_kit::test]
    fn test_dark_theme_tokens_contrast(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
            let theme = cx.theme();
            assert_eq!(theme.mode, ThemeMode::Dark);
            assert!(theme.background.l < 0.3);
            assert!(theme.foreground.l > 0.7);
        });
    }

    #[gpui_kit::test]
    fn test_dock_layout_regions(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        app_view.read_with(cx, |view, cx| {
            let dock_entity = view.dock_area();
            dock_entity.read_with(cx, |dock, cx| {
                // Verify all four dock regions exist
                assert!(dock.has_dock(DockPlacement::Left));
                assert!(dock.has_dock(DockPlacement::Right));
                assert!(dock.has_dock(DockPlacement::Bottom));

                // Verify the dock trees are present
                assert!(dock.layout(DockPlacement::Left).is_some());
                assert!(dock.layout(DockPlacement::Center).is_some());
                assert!(dock.layout(DockPlacement::Right).is_some());
                assert!(dock.layout(DockPlacement::Bottom).is_some());

                // Verify docks are open
                assert!(dock.is_dock_open(DockPlacement::Left));
                assert!(dock.is_dock_open(DockPlacement::Right));
                assert!(dock.is_dock_open(DockPlacement::Bottom));

                // Verify docks are non-empty
                assert!(!dock.is_empty(DockPlacement::Left, cx));
                assert!(!dock.is_empty(DockPlacement::Center, cx));
                assert!(!dock.is_empty(DockPlacement::Right, cx));
                assert!(!dock.is_empty(DockPlacement::Bottom, cx));

                // Verify configured dock sizes (responsive: fitted to the
                // window within sane rails, not fixed px).
                let left = dock.dock_size(DockPlacement::Left).expect("left size");
                let right = dock.dock_size(DockPlacement::Right).expect("right size");
                let bottom = dock.dock_size(DockPlacement::Bottom).expect("bottom size");
                assert!((200.0..=320.0).contains(&(left / px(1.0))), "left {left:?}");
                assert!((220.0..=340.0).contains(&(right / px(1.0))), "right {right:?}");
                assert!((180.0..=320.0).contains(&(bottom / px(1.0))), "bottom {bottom:?}");
            });
        });
    }

    #[gpui_kit::test]
    fn test_dock_panel_attachments(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        app_view.read_with(cx, |view, cx| {
            let dock_entity = view.dock_area();
            let panels = view.panels();

            let project_id = PanelId::from(panels.project.entity_id());
            let comp_id = PanelId::from(panels.composition.entity_id());
            let properties_id = PanelId::from(panels.properties.entity_id());
            let effects_id = PanelId::from(panels.effects.entity_id());
            let timeline_id = PanelId::from(panels.timeline.entity_id());

            dock_entity.read_with(cx, |dock, cx| {
                // 1. Project panel attached to Left dock
                let project_view = dock.panel(project_id).expect("project panel attached");
                assert_eq!(project_view.panel_name(cx), "project");

                // 2. Composition panel attached to Center dock
                let comp_view = dock.panel(comp_id).expect("composition panel attached");
                assert_eq!(comp_view.panel_name(cx), "composition");

                // 3. Properties panel attached to Right dock tab group
                let properties_view = dock
                    .panel(properties_id)
                    .expect("properties panel attached");
                assert_eq!(properties_view.panel_name(cx), "properties");

                // 4. Effects panel attached to Right dock tab group
                let effects_view = dock.panel(effects_id).expect("effects panel attached");
                assert_eq!(effects_view.panel_name(cx), "effects");

                // 5. Timeline panel attached to Bottom dock
                let timeline_view = dock.panel(timeline_id).expect("timeline panel attached");
                assert_eq!(timeline_view.panel_name(cx), "timeline");
            });
        });
    }

    #[gpui_kit::test]
    fn test_dock_toggle_and_resize(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        let mut app_view_entity = None;
        let (_root, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        let app_view = app_view_entity.expect("AppView created");
        cx.run_until_parked();

        cx.update(|window, cx| {
            let dock_area = app_view.read(cx).dock_area().clone();
            dock_area.update(cx, |dock, cx| {
                dock.set_dock_size(DockPlacement::Left, px(320.), window, cx);
                assert_eq!(dock.dock_size(DockPlacement::Left), Some(px(320.)));

                dock.toggle_dock(DockPlacement::Left, window, cx);
                assert!(!dock.is_dock_open(DockPlacement::Left));

                dock.toggle_dock(DockPlacement::Left, window, cx);
                assert!(dock.is_dock_open(DockPlacement::Left));
            });
        });
    }

    #[gpui_kit::test]
    fn test_dock_rendered_frame_and_panel_visibility(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            Root::new(view, window, cx)
        });

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            // Verify AppView root rendered
            assert!(window.find("app_view").visible());

            // Verify dock panels rendered and visible by ID
            let project = window.find("project_panel");
            let comp = window.find("composition_panel");
            let properties = window.find("properties_panel");
            let timeline = window.find("timeline_panel");

            assert!(project.visible());
            assert!(comp.visible());
            assert!(properties.visible());
            assert!(timeline.visible());

            // Check sub-elements
            assert!(window.find("project_assets").visible());
            assert!(window.find("composition_viewer").visible());
            assert!(window.find("properties_inspector").visible());
            assert!(window.find("timeline").visible());

            // Spatial layout invariants:
            // Left dock (project) is left of center viewer
            assert!(project.bounds().right() <= comp.bounds().left());

            // Center viewer is left of right dock (properties)
            assert!(comp.bounds().right() <= properties.bounds().left());

            // Bottom dock (timeline) is placed below center viewer
            assert!(comp.bounds().bottom() <= timeline.bounds().top());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_dock_effects_tab_activation_and_visibility(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        let app_view = app_view_entity.expect("AppView created");

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            let dock_area = app_view.read(cx).dock_area().clone();
            let panels = app_view.read(cx).panels().clone();
            let properties_id = PanelId::from(panels.properties.entity_id());
            let effects_id = PanelId::from(panels.effects.entity_id());

            // Initially properties tab is active in the right dock
            assert!(window.find("properties_panel").visible());
            assert!(window.find("properties_inspector").visible());
            assert!(window.try_find("effects_panel").is_none());

            // Switch active tab in Right dock to Effects using dock.select_panel
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(effects_id, window, cx);
            });

            window.render_frame(cx);

            // Effects panel is now rendered and visible in Right dock
            let effects = window.find("effects_panel");
            assert!(effects.visible());
            assert!(window.find("effects_categories").visible());
            assert!(window.try_find("properties_panel").is_none());

            // Center viewer is still left of right dock (effects)
            let comp = window.find("composition_panel");
            assert!(comp.bounds().right() <= effects.bounds().left());

            // Bidirectional tab switching: switch back to Properties tab
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(properties_id, window, cx);
            });

            window.render_frame(cx);

            // Properties panel is visible again, effects panel is hidden
            assert!(window.find("properties_panel").visible());
            assert!(window.find("properties_inspector").visible());
            assert!(window.try_find("effects_panel").is_none());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_dock_panel_focus(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        let app_view = app_view_entity.expect("AppView created");

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            let (proj_f, comp_f, prop_f, eff_f, time_f) = app_view.read_with(cx, |view, cx| {
                (
                    view.panels().project.read(cx).focus_handle().clone(),
                    view.panels().composition.read(cx).focus_handle().clone(),
                    view.panels().properties.read(cx).focus_handle().clone(),
                    view.panels().effects.read(cx).focus_handle().clone(),
                    view.panels().timeline.read(cx).focus_handle().clone(),
                )
            });

            // Focus project panel
            window.focus(&proj_f, cx);
            assert!(proj_f.is_focused(window));

            // Focus composition panel
            window.focus(&comp_f, cx);
            assert!(comp_f.is_focused(window));

            // Focus properties panel
            window.focus(&prop_f, cx);
            assert!(prop_f.is_focused(window));

            // Focus effects panel
            window.focus(&eff_f, cx);
            assert!(eff_f.is_focused(window));

            // Focus timeline panel
            window.focus(&time_f, cx);
            assert!(time_f.is_focused(window));

            // Verify focusing panels across tab switching in Right dock
            let dock_area = app_view.read(cx).dock_area().clone();
            let effects_id = PanelId::from(app_view.read(cx).panels().effects.entity_id());
            let properties_id = PanelId::from(app_view.read(cx).panels().properties.entity_id());

            // Switch to Effects tab, render frame, focus effects panel
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(effects_id, window, cx);
            });
            window.render_frame(cx);
            window.focus(&eff_f, cx);
            assert!(eff_f.is_focused(window));

            // Switch back to Properties tab, render frame, focus properties panel
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(properties_id, window, cx);
            });
            window.render_frame(cx);
            window.focus(&prop_f, cx);
            assert!(prop_f.is_focused(window));
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_dock_under_compact_window(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        // Edge case: Compact window dimensions (640x480)
        let handle = cx.open_window(size(px(640.), px(480.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            Root::new(view, window, cx)
        });

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            assert!(window.find("app_view").visible());
            assert!(window.find("project_panel").visible());
            assert!(window.find("composition_panel").visible());
            assert!(window.find("properties_panel").visible());
            assert!(window.find("timeline_panel").visible());
        })
        .expect("compact window update failed");
    }

    #[gpui_kit::test]
    fn test_panel_traits_and_metadata(cx: &mut TestAppContext) {
        use gpui_kit::component::dock::{BasePanel, Panel};

        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        app_view.read_with(cx, |view, cx| {
            let panels = view.panels();

            // 1. ProjectPanel
            panels.project.read_with(cx, |p, cx| {
                assert_eq!(p.panel_name(), "project");
                assert_eq!(p.tab_name(cx), Some("Project".into()));
                let _ = p.focus_handle().clone();
            });

            // 2. CompositionViewerPanel
            panels.composition.read_with(cx, |p, cx| {
                assert_eq!(p.panel_name(), "composition");
                assert_eq!(p.tab_name(cx), Some("Composition".into()));
                let _ = p.focus_handle().clone();
            });

            // 3. PropertiesPanel
            panels.properties.read_with(cx, |p, cx| {
                assert_eq!(p.panel_name(), "properties");
                assert_eq!(p.tab_name(cx), Some("Properties".into()));
                let _ = p.focus_handle().clone();
            });

            // 4. EffectsPanel
            panels.effects.read_with(cx, |p, cx| {
                assert_eq!(p.panel_name(), "effects");
                assert_eq!(p.tab_name(cx), Some("Effects".into()));
                let _ = p.focus_handle().clone();
            });

            // 5. TimelinePanel
            panels.timeline.read_with(cx, |p, cx| {
                assert_eq!(p.panel_name(), "timeline");
                assert_eq!(p.tab_name(cx), Some("Timeline".into()));
                let _ = p.focus_handle().clone();
            });
        });
    }

    #[gpui_kit::test]
    fn test_all_five_panels_standalone_rendering(cx: &mut TestAppContext) {
        use super::{
            CompositionViewerPanel, EffectsPanel, ProjectPanel, PropertiesPanel, TimelinePanel,
        };

        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        // 1. ProjectPanel standalone
        let h_proj = cx.open_window(size(px(400.), px(400.)), |window, cx| {
            let p = cx.new(ProjectPanel::standalone);
            Root::new(p, window, cx)
        });
        cx.update_window(h_proj.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("project_panel").visible());
            assert!(window.find("project_assets").visible());
        })
        .unwrap();

        // 2. CompositionViewerPanel standalone
        let h_comp = cx.open_window(size(px(600.), px(400.)), |window, cx| {
            let p = cx.new(CompositionViewerPanel::standalone);
            Root::new(p, window, cx)
        });
        cx.update_window(h_comp.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("composition_panel").visible());
            assert!(window.find("composition_viewer").visible());
        })
        .unwrap();

        // 3. PropertiesPanel standalone
        let h_prop = cx.open_window(size(px(400.), px(400.)), |window, cx| {
            let p = cx.new(PropertiesPanel::standalone);
            Root::new(p, window, cx)
        });
        cx.update_window(h_prop.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("properties_panel").visible());
            assert!(window.find("properties_inspector").visible());
        })
        .unwrap();

        // 4. EffectsPanel standalone
        let h_eff = cx.open_window(size(px(400.), px(400.)), |window, cx| {
            let p = cx.new(EffectsPanel::new);
            Root::new(p, window, cx)
        });
        cx.update_window(h_eff.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("effects_panel").visible());
            assert!(window.find("effects_categories").visible());
        })
        .unwrap();

        // 5. TimelinePanel standalone
        let h_time = cx.open_window(size(px(800.), px(300.)), |window, cx| {
            let p = cx.new(TimelinePanel::standalone);
            Root::new(p, window, cx)
        });
        cx.update_window(h_time.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("timeline_panel").visible());
            assert!(window.find("timeline").visible());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn test_effects_panel_categories_and_tab_flow(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        let app_view = app_view_entity.expect("AppView created");

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            let dock_area = app_view.read(cx).dock_area().clone();
            let panels = app_view.read(cx).panels().clone();
            let properties_id = PanelId::from(panels.properties.entity_id());
            let effects_id = PanelId::from(panels.effects.entity_id());

            // Check initial state: Properties tab active
            assert!(window.find("properties_panel").visible());
            assert!(window.find("properties_inspector").visible());
            assert!(window.try_find("effects_panel").is_none());
            assert!(window.try_find("effects_categories").is_none());

            // Switch to Effects tab via select_panel
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(effects_id, window, cx);
            });
            window.render_frame(cx);

            // Verify Effects panel and categories are visible
            assert!(window.find("effects_panel").visible());
            assert!(window.find("effects_categories").visible());
            assert!(window.try_find("properties_panel").is_none());
            assert!(window.try_find("properties_inspector").is_none());

            // Switch back to Properties tab
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(properties_id, window, cx);
            });
            window.render_frame(cx);

            // Verify Properties tab restored
            assert!(window.find("properties_panel").visible());
            assert!(window.find("properties_inspector").visible());
            assert!(window.try_find("effects_panel").is_none());
            assert!(window.try_find("effects_categories").is_none());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_editor_state_initialization(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            assert_eq!(state.active_comp_id, "comp_main");
            let comp = state.active_composition().expect("active comp present");
            assert_eq!(comp.name, "Main Composition");
            assert_eq!(comp.width, 1920);
            assert_eq!(comp.height, 1080);
            assert_eq!(comp.layers.len(), 3);
            assert_eq!(state.selected_layer_id, Some("layer_accent".to_string()));
            assert_eq!(state.clock.current_frame(), 0);
            assert!(!state.is_playing);

            let eval_stack = state.evaluate_current_frame().expect("evaluate succeeded");
            assert_eq!(eval_stack.composition_id, "comp_main");
            assert!(eval_stack.render_count() >= 2);
        });
    }

    #[gpui_kit::test]
    fn test_playback_transport_and_clock_stepping(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        // 1. Step forward and backward
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                assert_eq!(s.clock.current_frame(), 0);
                s.step_forward();
                assert_eq!(s.clock.current_frame(), 1);
                s.step_forward();
                assert_eq!(s.clock.current_frame(), 2);
                s.step_backward();
                assert_eq!(s.clock.current_frame(), 1);
                s.jump_to_end();
                assert_eq!(s.clock.current_frame(), 150);
                s.jump_to_start();
                assert_eq!(s.clock.current_frame(), 0);
                s.seek_frame(45);
                assert_eq!(s.clock.current_frame(), 45);
                cx.notify();
            });
        });

        // 2. Play / Pause state toggle
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                assert!(!s.is_playing);
                s.play();
                assert!(s.is_playing);
                s.pause();
                assert!(!s.is_playing);
                s.toggle_playback();
                assert!(s.is_playing);
                s.toggle_playback();
                assert!(!s.is_playing);
                cx.notify();
            });
        });
    }

    #[gpui_kit::test]
    fn test_layer_selection_and_inspector_sync(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        // Initially accent layer selected
        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            assert_eq!(state.selected_layer_id, Some("layer_accent".to_string()));
            let layer = state.selected_layer().unwrap();
            assert_eq!(layer.name, "Animated Box");
        });

        // Switch selection to badge layer
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_badge".to_string()));
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            assert_eq!(state.selected_layer_id, Some("layer_badge".to_string()));
            let layer = state.selected_layer().unwrap();
            assert_eq!(layer.name, "Accent Badge");
        });

        // Clear selection
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(None);
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            assert_eq!(state.selected_layer_id, None);
            assert!(state.selected_layer().is_none());
        });
    }

    #[gpui_kit::test]
    fn test_transform_mutation_and_evaluated_frame_update(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        // Check initial evaluated position
        let initial_pos = app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let eval = state.evaluate_current_frame().unwrap();
            let l = eval.get_layer("layer_accent").unwrap();
            l.transform.position
        });

        // Nudge position
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_accent".to_string()));
                s.nudge_position(50.0, -30.0);
                cx.notify();
            });
        });

        // Verify evaluated position reflects nudge
        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let eval = state.evaluate_current_frame().unwrap();
            let l = eval.get_layer("layer_accent").unwrap();
            assert_eq!(l.transform.position.x, initial_pos.x + 50.0);
            assert_eq!(l.transform.position.y, initial_pos.y - 30.0);
        });

        // Nudge scale, rotation, opacity
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.nudge_scale(20.0, 20.0);
                s.nudge_rotation(45.0);
                s.nudge_opacity(-25.0);
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer("layer_accent").unwrap();
            assert_eq!(layer.opacity.value, 75.0);

            let eval = state.evaluate_current_frame().unwrap();
            let l = eval.get_layer("layer_accent").unwrap();
            assert_eq!(l.transform.rotation, 45.0);
            assert!((l.effective_opacity - 0.75).abs() < 1e-4);
        });
    }

    #[gpui_kit::test]
    fn test_visibility_and_solo_toggles(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        // 1. Visibility toggle
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.toggle_layer_visibility("layer_accent");
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer("layer_accent").unwrap();
            assert!(!layer.visible);

            let eval = state.evaluate_current_frame().unwrap();
            assert!(!eval.render_layers().iter().any(|l| l.id == "layer_accent"));
        });

        // Restore visibility
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.toggle_layer_visibility("layer_accent");
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let eval = state.evaluate_current_frame().unwrap();
            assert!(eval.render_layers().iter().any(|l| l.id == "layer_accent"));
        });

        // 2. Solo toggle
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.toggle_layer_solo("layer_accent");
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let eval = state.evaluate_current_frame().unwrap();
            assert!(eval.has_solo);
            // Only soloed layers are rendered
            for layer in eval.render_layers() {
                assert_eq!(layer.id, "layer_accent");
            }
        });
    }

    #[gpui_kit::test]
    fn test_adding_new_solid_layer(cx: &mut TestAppContext) {
        use project::Color;

        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let new_layer_id = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                let id = s
                    .add_solid_layer("Golden Solid", Color::from_rgba_u8(245, 158, 11, 255), 500, 300)
                    .expect("added solid");
                cx.notify();
                id
            })
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let comp = state.active_composition().unwrap();
            assert_eq!(comp.layers.len(), 4);
            assert_eq!(state.selected_layer_id, Some(new_layer_id.clone()));

            let eval = state.evaluate_current_frame().unwrap();
            assert!(eval.get_layer(&new_layer_id).is_some());
            assert!(eval.render_layers().iter().any(|l| l.id == new_layer_id));
        });
    }

    #[gpui_kit::test]
    fn test_spacebar_action_playback_toggle(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        let app_view = app_view_entity.expect("AppView created");

        cx.update_window(handle.into(), |_, window, cx| {
            let focus_handle = app_view.read(cx).focus_handle().clone();
            window.focus(&focus_handle, cx);
            window.render_frame(cx);

            // Initially paused
            assert!(!app_view.read(cx).state().read(cx).is_playing);

            window.dispatch_keystroke(gpui_kit::Keystroke::parse("space").unwrap(), cx);
        })
        .expect("update_window failed");

        cx.run_until_parked();
        assert!(app_view.read_with(cx, |view, cx| view.state().read(cx).is_playing));

        cx.update_window(handle.into(), |_, window, cx| {
            // Dispatch spacebar keystroke again to pause
            window.dispatch_keystroke(gpui_kit::Keystroke::parse("space").unwrap(), cx);
        })
        .expect("update_window failed");

        cx.run_until_parked();
        assert!(!app_view.read_with(cx, |view, cx| view.state().read(cx).is_playing));
    }

    #[gpui_kit::test]
    fn test_properties_panel_layer_type_and_opacity_evaluation(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        // 1. Initial selection is accent solid (100% opacity at frame 0)
        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let layer = state.selected_layer().unwrap();
            let tc = state.clock.timecode();
            let op = layer.opacity.evaluate_at(&tc).clamp(0.0, 100.0);
            assert_eq!(op, 100.0, "Opacity must evaluate to 100% (not normalized 1.0)");
        });

        // 2. Select badge layer with animated opacity fade-in
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_badge".to_string()));
                // Jump to frame 30 (mid-fade: 15->45)
                s.seek_frame(30);
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let layer = state.selected_layer().unwrap();
            let tc = state.clock.timecode();
            let op = layer.opacity.evaluate_at(&tc).clamp(0.0, 100.0);
            assert!((op - 50.0).abs() < 1e-3, "At frame 30, badge opacity should be 50%");
        });

        // 3. Jump to frame 45 (end of fade)
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.seek_frame(45);
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let layer = state.selected_layer().unwrap();
            let tc = state.clock.timecode();
            let op = layer.opacity.evaluate_at(&tc).clamp(0.0, 100.0);
            assert_eq!(op, 100.0, "At frame 45, badge opacity should reach 100%");
        });

        // 4. Nudge opacity down by -20% and toggle visibility off
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.nudge_opacity(-20.0);
                s.toggle_selected_layer_visibility();
                cx.notify();
            });
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let layer = state.selected_layer().unwrap();
            assert!(!layer.visible);
            let tc = state.clock.timecode();
            let op = layer.opacity.evaluate_at(&tc).clamp(0.0, 100.0);
            assert_eq!(op, 80.0, "Layer opacity property should remain 80% even when visibility is toggled off");
        });
    }

    #[gpui_kit::test]
    fn test_add_solid_centers_in_composition_and_unique_ids(cx: &mut TestAppContext) {
        use project::Color;

        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        // Add 1st solid
        let id1 = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                let id = s
                    .add_solid_layer("Centered Red", Color::RED, 600, 400)
                    .expect("added 1st solid");
                cx.notify();
                id
            })
        });

        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let comp = state.active_composition().unwrap();
            let l = comp.get_layer(&id1).unwrap();

            // Layer should be centered in composition: (0, 0)
            assert_eq!(l.transform.position.value.x, 0.0);
            assert_eq!(l.transform.position.value.y, 0.0);
            // Anchor point should be center of 600x400: (300, 200)
            assert_eq!(l.transform.anchor_point.value.x, 300.0);
            assert_eq!(l.transform.anchor_point.value.y, 200.0);
        });

        // Add 2nd solid: verify distinct unique ID
        let id2 = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                let id = s
                    .add_solid_layer("Centered Blue", Color::BLUE, 400, 400)
                    .expect("added 2nd solid");
                cx.notify();
                id
            })
        });

        assert_ne!(id1, id2);
        app_view.read_with(cx, |view, cx| {
            let state = view.state().read(cx);
            let comp = state.active_composition().unwrap();
            assert!(comp.get_layer(&id1).is_some());
            assert!(comp.get_layer(&id2).is_some());
            assert_eq!(comp.layers.len(), 5);
        });
    }

    #[gpui_kit::test]
    fn test_rendered_frame_canvas_layers_and_timeline_tracks(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        let _app_view = app_view_entity.expect("AppView created");

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            // Verify canvas elements rendered
            assert!(window.find("canvas_layer_layer_bg").visible());
            assert!(window.find("canvas_layer_layer_accent").visible());

            // Verify timeline track spans rendered
            assert!(window.find("track_span_layer_bg").visible());
            assert!(window.find("track_span_layer_accent").visible());
            assert!(window.find("track_span_layer_badge").visible());

            // Verify transport and timecode controls
            assert!(window.find("timecode_display").visible());
            assert!(window.find("transport_start").visible());
            assert!(window.find("transport_prev").visible());
            assert!(window.find("transport_play").visible());
            assert!(window.find("transport_next").visible());
            assert!(window.find("transport_end").visible());
            assert!(window.find("add_solid_button").visible());
        })
        .expect("update_window failed");
    }

    #[test]
    fn test_media_import_and_layer_creation() {
        use crate::state::EditorState;
        use project::LayerSource;

        let temp_dir = std::env::temp_dir();
        let test_img_path = temp_dir.join("motion_studio_test_import.png");
        let img = image::RgbaImage::new(320, 240);
        img.save(&test_img_path).expect("failed to create test image");

        let mut state = EditorState::new();
        let layer_id = state
            .import_media_file(test_img_path.clone())
            .expect("import_media_file failed");

        // Verify layer selection
        assert_eq!(state.selected_layer_id.as_deref(), Some(layer_id.as_str()));

        // Verify asset registered
        assert!(state.project.assets.iter().any(|a| a.path == test_img_path));

        // Verify layer properties in active composition
        let comp = state.active_composition().expect("active comp");
        let layer = comp.get_layer(&layer_id).expect("media layer found");

        match &layer.source {
            LayerSource::Image { asset_id } => {
                assert!(!asset_id.is_empty());
            }
            other => panic!("Expected Image layer source, got {other:?}"),
        }

        // Layer should be centered in composition: (0, 0)
        assert_eq!(layer.transform.position.value.x, 0.0);
        assert_eq!(layer.transform.position.value.y, 0.0);
        // Anchor point centered on image dimensions 320x240
        assert_eq!(layer.transform.anchor_point.value.x, 160.0);
        assert_eq!(layer.transform.anchor_point.value.y, 120.0);

        let _ = std::fs::remove_file(test_img_path);
    }

    #[test]
    fn test_effects_crud_and_parameter_nudging() {
        use crate::state::EditorState;
        use project::EffectType;

        let mut state = EditorState::new();
        let fx1_id = state
            .add_effect_to_selected_layer(EffectType::gaussian_blur(15.0))
            .expect("added gaussian blur");

        let fx2_id = state
            .add_effect_to_selected_layer(EffectType::brightness_contrast(10.0, -5.0))
            .expect("added brightness & contrast");

        let layer = state.selected_layer().expect("layer selected");
        assert_eq!(layer.effects.len(), 2);
        assert_eq!(layer.effects[0].id, fx1_id);
        assert_eq!(layer.effects[1].id, fx2_id);

        // Nudge radius
        state
            .nudge_effect_param(&fx1_id, "radius", 5.0)
            .expect("nudged radius");
        let layer = state.selected_layer().expect("layer selected");
        if let EffectType::GaussianBlur { radius } = &layer.effects[0].effect_type {
            assert_eq!(radius.value, 20.0);
        } else {
            panic!("Expected GaussianBlur");
        }

        // Toggle enabled
        state.toggle_effect_enabled(&fx1_id).expect("toggled enabled");
        let layer = state.selected_layer().expect("layer selected");
        assert!(!layer.effects[0].enabled);

        // Remove effect
        state
            .remove_effect_from_selected_layer(&fx1_id)
            .expect("removed effect");
        let layer = state.selected_layer().expect("layer selected");
        assert_eq!(layer.effects.len(), 1);
        assert_eq!(layer.effects[0].id, fx2_id);
    }

    #[gpui_kit::test]
    fn test_ui_effects_panel_addition_and_properties_inspector_manipulation(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        let app_view = app_view_entity.expect("AppView created");

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            let dock_area = app_view.read(cx).dock_area().clone();
            let panels = app_view.read(cx).panels().clone();
            let properties_id = PanelId::from(panels.properties.entity_id());
            let effects_id = PanelId::from(panels.effects.entity_id());

            // 1. Switch dock to Effects tab
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(effects_id, window, cx);
            });
            window.render_frame(cx);

            // Verify effects items are visible in the DOM. Categories start
            // collapsed (accordion): headers render, rows appear on expand.
            assert!(window.find("effects_panel").visible());
            assert!(window.find("effects_categories").visible());
            assert!(window.find("effect_category_blur").visible());
            assert!(window.find("effect_category_keying").visible());
            assert!(window.find("effect_category_text").visible());
            panels.effects.update(cx, |p, cx| {
                assert!(p.is_collapsed("blur"));
                assert!(p.is_collapsed("keying"));
                p.expand_category("blur");
                p.expand_category("color");
                p.expand_category("keying");
                p.expand_category("text");
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find("effect_item_gaussian_blur").visible());
            assert!(window.find("effect_item_brightness_contrast").visible());
            assert!(window.find("effect_item_chroma_key").visible());
            assert!(window.find("effect_item_luma_key").visible());
            assert!(window.find("effect_item_text_fill").visible());

            // 2. Add effect to selected layer
            let state_entity = app_view.read(cx).state().clone();
            state_entity.update(cx, |s, cx| {
                let _ = s.add_effect_to_selected_layer(project::EffectType::gaussian_blur(10.0));
                cx.notify();
            });

            // 3. Switch back to Properties tab
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(properties_id, window, cx);
            });
            window.render_frame(cx);

            // Verify Properties tab has the applied effect
            let layer = state_entity.read(cx).selected_layer().unwrap().clone();
            assert_eq!(layer.effects.len(), 1);
            let eff_id = layer.effects[0].id.clone();

            assert!(window.find(SharedString::from(format!("effect_toggle_{eff_id}"))).visible());
            assert!(window.find(SharedString::from(format!("effect_delete_{eff_id}"))).visible());
            // Value field scrubs (drag/wheel) and types (click): no +/- buttons.
            assert!(window.find(SharedString::from(format!("param_radius_{eff_id}"))).visible());

            // 4. Test live parameter nudging
            state_entity.update(cx, |s, cx| {
                let _ = s.nudge_effect_param(&eff_id, "radius", 5.0);
                cx.notify();
            });
            window.render_frame(cx);
            let layer_after_nudge = state_entity.read(cx).selected_layer().unwrap().clone();
            if let project::EffectType::GaussianBlur { radius } = &layer_after_nudge.effects[0].effect_type {
                assert_eq!(radius.value, 15.0);
            } else {
                panic!("Expected GaussianBlur");
            }

            // 4b. Test keyboard entry via the scrub-value path
            let key = format!("fx:{eff_id}:radius:100");
            assert!(state_entity.read(cx).scrub_current_value(&key).is_some());
            state_entity.update(cx, |s, cx| {
                assert!(s.set_scrub_value(&key, 42.0));
                cx.notify();
            });
            window.render_frame(cx);
            let layer_after_type = state_entity.read(cx).selected_layer().unwrap().clone();
            if let project::EffectType::GaussianBlur { radius } = &layer_after_type.effects[0].effect_type {
                assert!((radius.value - 42.0).abs() < 1e-4);
            } else {
                panic!("Expected GaussianBlur");
            }

            // 5. Test toggle enabled
            state_entity.update(cx, |s, cx| {
                let _ = s.toggle_effect_enabled(&eff_id);
                cx.notify();
            });
            window.render_frame(cx);
            let layer_after_toggle = state_entity.read(cx).selected_layer().unwrap().clone();
            assert!(!layer_after_toggle.effects[0].enabled);

            // 6. Test delete effect
            state_entity.update(cx, |s, cx| {
                let _ = s.remove_effect_from_selected_layer(&eff_id);
                cx.notify();
            });
            window.render_frame(cx);
            let layer_after_delete = state_entity.read(cx).selected_layer().unwrap().clone();
            assert_eq!(layer_after_delete.effects.len(), 0);
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_ui_project_panel_media_import_and_assets_listing(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });

        let app_view = app_view_entity.expect("AppView created");

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            // Verify project panel and import button
            assert!(window.find("project_panel").visible());
            assert!(window.find("project_assets").visible());
            assert!(window.find("import_media_button").visible());
            assert!(window.find("add_solid_button").visible());

            let state_entity = app_view.read(cx).state().clone();

            // Import an image
            let temp_dir = std::env::temp_dir();
            let test_img_path = temp_dir.join("motion_studio_ui_import_test.png");
            let img = image::RgbaImage::new(400, 300);
            img.save(&test_img_path).expect("failed to create test image");

            let layer_id = state_entity.update(cx, |s, cx| {
                let lid = s.import_media_file(test_img_path.clone()).expect("import succeeded");
                cx.notify();
                lid
            });

            window.render_frame(cx);

            // Verify layer selected and registered
            let state = state_entity.read(cx);
            assert_eq!(state.selected_layer_id.as_deref(), Some(layer_id.as_str()));
            assert!(state.project.assets.iter().any(|a| a.path == test_img_path));

            // Verify canvas rendered new layer
            assert!(window.find(SharedString::from(format!("canvas_layer_{layer_id}"))).visible());

            let _ = std::fs::remove_file(test_img_path);
        })
        .expect("update_window failed");
    }

    #[test]
    fn test_layer_reordering_and_deletion() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        let initial_count = state.active_composition().unwrap().layers.len();
        assert!(initial_count >= 3);

        let initial_first_id = state.active_composition().unwrap().layers[0].id.clone();
        let initial_second_id = state.active_composition().unwrap().layers[1].id.clone();

        // Select second layer and move it up to index 0
        state.select_layer(Some(initial_second_id.clone()));
        assert!(state.move_selected_layer_up().is_ok());

        // Verify reordering succeeded
        assert_eq!(state.active_composition().unwrap().layers[0].id, initial_second_id);
        assert_eq!(state.active_composition().unwrap().layers[1].id, initial_first_id);

        // Move it back down
        assert!(state.move_selected_layer_down().is_ok());
        assert_eq!(state.active_composition().unwrap().layers[0].id, initial_first_id);
        assert_eq!(state.active_composition().unwrap().layers[1].id, initial_second_id);

        // Delete selected layer
        let deleted_id = state.delete_selected_layer().expect("deleted layer");
        assert_eq!(deleted_id, initial_second_id);
        assert_eq!(state.active_composition().unwrap().layers.len(), initial_count - 1);
        assert!(!state.active_composition().unwrap().layers.iter().any(|l| l.id == initial_second_id));
    }

    #[test]
    fn test_glsl_presets_and_shader_code_update() {
        use crate::state::EditorState;
        use project::EffectType;

        let mut state = EditorState::new();
        state.select_layer(Some("layer_bg".to_string()));

        // Add custom GLSL shader
        let fx_id = state
            .add_effect_to_selected_layer(EffectType::glsl_shader(
                project::Effect::default_glsl_code(),
                1.0,
                50.0,
                1.0,
                100.0,
            ))
            .expect("effect added");

        // Verify preset list is populated
        assert_eq!(EditorState::GLSL_PRESETS.len(), 4);
        assert_eq!(EditorState::GLSL_PRESETS[1].0, "Color Wave");

        // Set GLSL code from preset
        let color_wave_code = EditorState::GLSL_PRESETS[1].1.to_string();
        state
            .set_glsl_code(&fx_id, color_wave_code.clone())
            .expect("set glsl code");

        // Verify shader code updated on layer
        let layer = state.selected_layer().unwrap();
        let eff = layer.get_effect(&fx_id).unwrap();
        if let EffectType::GlslShader { code, param1, .. } = &eff.effect_type {
            assert_eq!(code, &color_wave_code);
            assert_eq!(param1.value, 1.0);
        } else {
            panic!("Expected GlslShader effect type");
        }

        // Nudge param1 and param2
        state.nudge_effect_param(&fx_id, "param1", 0.5).unwrap();
        state.nudge_effect_param(&fx_id, "param2", 15.0).unwrap();

        let layer2 = state.selected_layer().unwrap();
        let eff2 = layer2.get_effect(&fx_id).unwrap();
        if let EffectType::GlslShader { param1, param2, .. } = &eff2.effect_type {
            assert_eq!(param1.value, 1.5);
            assert_eq!(param2.value, 65.0);
        }
    }

    #[test]
    fn test_sample_media_generators() {
        use crate::state::EditorState;
        use project::LayerSource;

        let mut state = EditorState::new();
        let initial_layer_count = state.active_composition().unwrap().layers.len();

        // 1. Generate and import sample image
        let img_layer_id = state.import_sample_image().expect("sample image created and imported");
        assert_eq!(state.active_composition().unwrap().layers.len(), initial_layer_count + 1);
        let img_layer = state.active_composition().unwrap().get_layer(&img_layer_id).unwrap();
        assert!(matches!(img_layer.source, LayerSource::Image { .. }));

        // 2. Generate and import sample video
        let vid_layer_id = state.import_sample_video().expect("sample video created and imported");
        assert_eq!(state.active_composition().unwrap().layers.len(), initial_layer_count + 2);
        let vid_layer = state.active_composition().unwrap().get_layer(&vid_layer_id).unwrap();
        assert!(matches!(vid_layer.source, LayerSource::Video { .. }));
    }

    #[gpui_kit::test]
    fn test_property_scrubbing_delta(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let state_entity = cx.new(|_| crate::state::EditorState::new());
        let panel_entity = cx.new(|cx| crate::panels::PropertiesPanel::new(state_entity.clone(), cx));

        // Select background layer
        state_entity.update(cx, |s, cx| {
            s.select_layer(Some("layer_bg".to_string()));
            cx.notify();
        });

        let initial_pos = state_entity.read_with(cx, |s, _| s.selected_layer().unwrap().transform.position.value);
        let initial_rot = state_entity.read_with(cx, |s, _| s.selected_layer().unwrap().transform.rotation.value);

        // Apply scrub delta dx = +20.0 to pos_x
        panel_entity.update(cx, |panel, cx| {
            panel.apply_scrub_delta("pos_x", 20.0, cx);
        });

        let updated_pos = state_entity.read_with(cx, |s, _| s.selected_layer().unwrap().transform.position.value);
        assert_eq!(updated_pos.x, initial_pos.x + 20.0);

        // Apply scrub delta dx = -10.0 to rotation
        panel_entity.update(cx, |panel, cx| {
            panel.apply_scrub_delta("rotation", -10.0, cx);
        });

        let updated_rot = state_entity.read_with(cx, |s, _| s.selected_layer().unwrap().transform.rotation.value);
        assert_eq!(updated_rot, initial_rot - 5.0);
    }

    #[test]
    fn test_timeline_stopwatch_and_keyframe_navigation() {
        use crate::state::EditorState;
        use project::TimeCode;

        let mut state = EditorState::new();
        let fps = 30.0;

        // Position on layer_bg starts with animated = false and 0 keyframes
        let layer_id = "layer_bg";
        let prop_path = "transform.position";
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(layer_id).unwrap();
            assert!(!layer.transform.position.is_animated());
            assert_eq!(layer.transform.position.keyframe_count(), 0);
        }

        // 1. Toggle stopwatch ON at frame 0 -> records initial keyframe at frame 0
        state.clock.seek(TimeCode::from_frames(0, fps));
        state.toggle_layer_property_animation(layer_id, prop_path);
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(layer_id).unwrap();
            assert!(layer.transform.position.is_animated());
            assert_eq!(layer.transform.position.keyframe_count(), 1);
            assert!(layer.transform.position.has_keyframe_at(&TimeCode::from_frames(0, fps)));
        }

        // 2. Seek to frame 30 and nudge position -> records keyframe at frame 30
        state.clock.seek(TimeCode::from_frames(30, fps));
        state.nudge_layer_position(layer_id, 100.0, 50.0);
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(layer_id).unwrap();
            assert_eq!(layer.transform.position.keyframe_count(), 2);
            assert!(layer.transform.position.has_keyframe_at(&TimeCode::from_frames(30, fps)));
        }

        // 3. Keyframe navigation: previous keyframe from frame 30 seeks back to frame 0
        state.seek_previous_keyframe(layer_id, prop_path);
        assert_eq!(state.clock.current_frame(), 0);

        // 4. Next keyframe from frame 0 seeks forward to frame 30
        state.seek_next_keyframe(layer_id, prop_path);
        assert_eq!(state.clock.current_frame(), 30);

        // 5. Toggle keyframe at current time (frame 30) -> removes it
        state.toggle_layer_keyframe_at_current_time(layer_id, prop_path);
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(layer_id).unwrap();
            assert_eq!(layer.transform.position.keyframe_count(), 1);
            assert!(!layer.transform.position.has_keyframe_at(&TimeCode::from_frames(30, fps)));
        }

        // 6. Toggle stopwatch OFF -> clears all keyframes and disables animation
        state.toggle_layer_property_animation(layer_id, prop_path);
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(layer_id).unwrap();
            assert!(!layer.transform.position.is_animated());
            assert_eq!(layer.transform.position.keyframe_count(), 0);
        }
    }

    #[gpui_kit::test]
    fn test_timeline_panel_expansion_state(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let state_entity = cx.new(|_| crate::state::EditorState::new());
        let timeline_panel = cx.new(|cx| crate::panels::TimelinePanel::new(state_entity, cx));

        timeline_panel.read_with(cx, |panel, _| {
            assert!(!panel.is_layer_expanded("layer_bg"));
            assert!(!panel.is_group_expanded("layer_bg:transform"));
        });

        // Toggle layer expansion
        timeline_panel.update(cx, |panel, _| {
            panel.toggle_layer_expanded("layer_bg");
            panel.toggle_group_expanded("layer_bg:transform");
        });

        timeline_panel.read_with(cx, |panel, _| {
            assert!(panel.is_layer_expanded("layer_bg"));
            assert!(panel.is_group_expanded("layer_bg:transform"));
        });
    }

    #[test]
    fn test_layer_controls_mutations() {
        use crate::state::EditorState;
        use project::{BlendMode, TrackMatteMode};

        let mut state = EditorState::new();
        let layer_id = "layer_accent";

        // Blend mode
        state.set_layer_blend_mode(layer_id, BlendMode::Multiply);
        assert_eq!(state.active_composition().unwrap().get_layer(layer_id).unwrap().blend_mode, BlendMode::Multiply);

        // Track Matte
        state.set_layer_track_matte(layer_id, TrackMatteMode::Alpha, Some("layer_bg".to_string()));
        let layer = state.active_composition().unwrap().get_layer(layer_id).unwrap();
        assert_eq!(layer.matte_mode, TrackMatteMode::Alpha);
        assert_eq!(layer.matte_layer_id.as_deref(), Some("layer_bg"));

        // Parenting
        state.set_layer_parent(layer_id, Some("layer_bg".to_string()));
        assert_eq!(state.active_composition().unwrap().get_layer(layer_id).unwrap().parent_id.as_deref(), Some("layer_bg"));

        // Lock
        state.toggle_layer_lock(layer_id);
        assert!(state.active_composition().unwrap().get_layer(layer_id).unwrap().is_locked());
    }

    #[test]
    fn test_remove_layer_effect_directly() {
        use crate::state::EditorState;
        use project::EffectType;

        let mut state = EditorState::new();
        let layer_id = "layer_accent";
        state.select_layer(Some(layer_id.to_string()));

        let fx_id = state.add_effect_to_selected_layer(EffectType::gaussian_blur(15.0)).unwrap();
        assert!(state.active_composition().unwrap().get_layer(layer_id).unwrap().has_effects());

        // Remove effect directly
        state.remove_layer_effect(layer_id, &fx_id).expect("effect removed");
        assert!(!state.active_composition().unwrap().get_layer(layer_id).unwrap().has_effects());
    }

    #[test]
    fn test_editor_tools_switching_and_layer_creation() {
        use crate::state::{EditorState, EditorTool};
        use project::{LayerSource, ShapeType, Vec2};

        let mut state = EditorState::new();
        assert_eq!(state.active_tool, EditorTool::Move);

        // Tool switching
        state.set_tool(EditorTool::Hand);
        assert_eq!(state.active_tool, EditorTool::Hand);
        state.set_tool(EditorTool::Rotate);
        assert_eq!(state.active_tool, EditorTool::Rotate);
        state.set_tool(EditorTool::Pen);
        assert_eq!(state.active_tool, EditorTool::Pen);
        state.set_tool(EditorTool::Text);
        assert_eq!(state.active_tool, EditorTool::Text);
        state.set_tool(EditorTool::ShapeRect);
        assert_eq!(state.active_tool, EditorTool::ShapeRect);

        // Cycle shape tool
        state.cycle_shape_tool();
        assert_eq!(state.active_tool, EditorTool::ShapeEllipse);
        state.cycle_shape_tool();
        assert_eq!(state.active_tool, EditorTool::ShapeRect);

        // Add Text layer
        let text_id = state.add_text_layer("Motion Studio Title", None).unwrap();
        {
            let comp = state.active_composition().unwrap();
            let text_layer = comp.get_layer(&text_id).unwrap();
            assert!(matches!(&text_layer.source, LayerSource::Text { text, .. } if text.value == "Motion Studio Title"));
        }

        // Add Rectangle shape layer
        let rect_id = state.add_rectangle_shape_layer(400.0, 250.0, None).unwrap();
        {
            let comp = state.active_composition().unwrap();
            let rect_layer = comp.get_layer(&rect_id).unwrap();
            assert!(matches!(&rect_layer.source, LayerSource::Shape { shape_type: ShapeType::Rectangle { width, height, .. } } if width.value == 400.0 && height.value == 250.0));
        }

        // Add Ellipse shape layer
        let ellipse_id = state.add_ellipse_shape_layer(120.0, 120.0, None).unwrap();
        {
            let comp = state.active_composition().unwrap();
            let ellipse_layer = comp.get_layer(&ellipse_id).unwrap();
            assert!(matches!(&ellipse_layer.source, LayerSource::Shape { shape_type: ShapeType::Ellipse { radius_x, radius_y, .. } } if radius_x.value == 120.0 && radius_y.value == 120.0));
        }

        // Add Pen point -> new path layer
        state.selected_layer_id = None;
        let path_id = state.add_pen_point(Vec2::new(50.0, 75.0)).unwrap();
        {
            let comp = state.active_composition().unwrap();
            let path_layer = comp.get_layer(&path_id).unwrap();
            assert!(matches!(&path_layer.source, LayerSource::Shape { shape_type: ShapeType::Path { path_data, .. } } if path_data.contains("M 50.0 75.0")));
        }

        // Append next vertex to path layer
        state.selected_layer_id = Some(path_id.clone());
        let same_id = state.add_pen_point(Vec2::new(150.0, 200.0)).unwrap();
        assert_eq!(same_id, path_id);
        {
            let comp = state.active_composition().unwrap();
            let path_layer = comp.get_layer(&path_id).unwrap();
            assert!(matches!(&path_layer.source, LayerSource::Shape { shape_type: ShapeType::Path { path_data, .. } } if path_data.contains("L 150.0 200.0")));
        }
    }

    #[test]
    fn test_layer_duplication_and_reset_transform() {
        use crate::state::EditorState;
        use project::{EffectType, Vec2};

        let mut state = EditorState::new();
        let orig_id = "layer_accent";

        // 1. Duplicate layer
        let dup_id = state.duplicate_layer(orig_id).unwrap();
        assert_ne!(dup_id, orig_id);
        {
            let comp = state.active_composition().unwrap();
            let dup_layer = comp.get_layer(&dup_id).unwrap();
            assert!(dup_layer.name.ends_with("Copy"));
        }

        // 2. Modify transform and reset
        state.nudge_layer_position(&dup_id, 100.0, 50.0);
        state.nudge_layer_rotation(&dup_id, 45.0);
        state.reset_layer_transform(&dup_id);
        {
            let comp = state.active_composition().unwrap();
            let dup_layer = comp.get_layer(&dup_id).unwrap();
            assert_eq!(dup_layer.transform.position.value, Vec2::ZERO);
            assert_eq!(dup_layer.transform.rotation.value, 0.0);
            assert_eq!(dup_layer.opacity.value, 100.0);
        }

        // 3. Duplicate effect
        state.select_layer(Some(dup_id.clone()));
        let fx_id = state.add_effect_to_selected_layer(EffectType::gaussian_blur(20.0)).unwrap();
        let dup_fx_id = state.duplicate_layer_effect(&dup_id, &fx_id).unwrap();
        assert_ne!(fx_id, dup_fx_id);
        {
            let comp = state.active_composition().unwrap();
            let dup_layer = comp.get_layer(&dup_id).unwrap();
            assert_eq!(dup_layer.effects.len(), 2);
            assert!(dup_layer.effects[1].name.ends_with("Copy"));
        }

        // 4. Add keyframe to all transforms at playhead
        state.add_keyframe_to_all_transforms_at_playhead(&dup_id);
        {
            let comp = state.active_composition().unwrap();
            let dup_layer = comp.get_layer(&dup_id).unwrap();
            assert!(dup_layer.transform.position.is_animated());
            assert_eq!(dup_layer.transform.position.keyframe_count(), 1);
            assert!(dup_layer.transform.scale.is_animated());
            assert_eq!(dup_layer.transform.scale.keyframe_count(), 1);
            assert!(dup_layer.transform.rotation.is_animated());
            assert_eq!(dup_layer.transform.rotation.keyframe_count(), 1);
            assert!(dup_layer.transform.anchor_point.is_animated());
            assert_eq!(dup_layer.transform.anchor_point.keyframe_count(), 1);
            assert!(dup_layer.opacity.is_animated());
            assert_eq!(dup_layer.opacity.keyframe_count(), 1);
        }
    }

    #[gpui_kit::test]
    fn test_blend_mode_dropdown_and_context_menus(cx: &mut TestAppContext) {
        use crate::panels::{BLEND_MODE_GROUPS, ContextMenuTarget, TimelinePanel};
        use project::BlendMode;

        cx.update(gpui_kit::init);
        let state_entity = cx.new(|_| crate::state::EditorState::new());
        let timeline_panel = cx.new(|cx| TimelinePanel::new(state_entity, cx));

        // Verify BLEND_MODE_GROUPS covers all 19 modes in BlendMode::ALL
        let mut all_grouped_modes = Vec::new();
        for (_cat, modes) in BLEND_MODE_GROUPS {
            for &m in *modes {
                all_grouped_modes.push(m);
            }
        }
        assert_eq!(all_grouped_modes.len(), BlendMode::ALL.len());
        for mode in BlendMode::ALL {
            assert!(all_grouped_modes.contains(&mode), "Missing mode: {:?}", mode);
        }

        // Blend mode dropdown open/close
        timeline_panel.read_with(cx, |p, _| {
            assert!(p.active_blend_dropdown.is_none());
        });
        timeline_panel.update(cx, |p, _| {
            p.open_blend_dropdown("layer_accent".to_string());
        });
        timeline_panel.read_with(cx, |p, _| {
            assert_eq!(p.active_blend_dropdown.as_deref(), Some("layer_accent"));
        });
        timeline_panel.update(cx, |p, _| {
            p.close_blend_dropdown();
        });
        timeline_panel.read_with(cx, |p, _| {
            assert!(p.active_blend_dropdown.is_none());
        });

        // Context menu open/close
        timeline_panel.read_with(cx, |p, _| {
            assert!(p.context_menu.is_none());
        });
        timeline_panel.update(cx, |p, _| {
            p.open_context_menu(ContextMenuTarget::Layer("layer_accent".to_string()), gpui::point(gpui::px(10.), gpui::px(10.)));
        });
        timeline_panel.read_with(cx, |p, _| {
            assert!(matches!(p.context_menu.as_ref().map(|c| &c.target), Some(ContextMenuTarget::Layer(lid)) if lid == "layer_accent"));
        });
        timeline_panel.update(cx, |p, _| {
            p.close_context_menu();
        });
        timeline_panel.read_with(cx, |p, _| {
            assert!(p.context_menu.is_none());
        });
    }

    #[test]
    fn test_timeline_scrubbing_and_seek() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        assert_eq!(state.clock.current_frame(), 0);

        // Scrub to 2.5 seconds
        state.seek(2.5);
        assert_eq!(state.clock.current_frame(), 75);
        assert!((state.clock.position_seconds() - 2.5).abs() < 0.001);

        // Scrub to 0.0 seconds
        state.seek(0.0);
        assert_eq!(state.clock.current_frame(), 0);
    }

    #[test]
    fn test_layer_source_properties_inspection_and_mutation() {
        use crate::state::EditorState;
        use project::{Color, LayerSource, ShapeType};

        let mut state = EditorState::new();

        // 1. Solid layer source mutation
        let _ = state.nudge_layer_solid_color("layer_bg", 0.1, 0.0, -0.1);
        let layer_bg = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        if let LayerSource::Solid { color, width, height } = layer_bg.source {
            assert!(color.r > 0.05);
            assert_eq!(width, 1920);
            assert_eq!(height, 1080);
        } else {
            panic!("Expected solid layer");
        }

        let _ = state.set_layer_solid_dimensions("layer_bg", 1280, 720);
        let layer_bg = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        if let LayerSource::Solid { width, height, .. } = layer_bg.source {
            assert_eq!(width, 1280);
            assert_eq!(height, 720);
        }

        let _ = state.set_layer_solid_color("layer_bg", Color::from_hex("#FF0000").unwrap());
        let layer_bg = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        if let LayerSource::Solid { color, .. } = layer_bg.source {
            assert!((color.r - 1.0).abs() < 0.01);
            assert!((color.g - 0.0).abs() < 0.01);
            assert!((color.b - 0.0).abs() < 0.01);
        }

        // 2. Text layer source mutation
        let text_id = state.add_text_layer("Initial Text", None).unwrap();
        let _ = state.set_layer_text(&text_id, "Antigravity Studio");
        let _ = state.set_layer_font_size(&text_id, 48.0);
        let _ = state.nudge_layer_font_size(&text_id, 4.0);
        let layer_title = state.active_composition().unwrap().get_layer(&text_id).unwrap().clone();
        if let LayerSource::Text { text, font_size, .. } = layer_title.source {
            assert_eq!(text.value, "Antigravity Studio");
            assert!((font_size.value - 52.0).abs() < 0.01);
        } else {
            panic!("Expected text layer");
        }

        // 3. Shape layer source mutation
        let shape_id = state.add_rectangle_shape_layer(200.0, 100.0, None).unwrap();
        let _ = state.set_layer_rect_dimensions(&shape_id, 250.0, 150.0, 12.0);
        let layer_shape = state.active_composition().unwrap().get_layer(&shape_id).unwrap().clone();
        if let LayerSource::Shape {
            shape_type: ShapeType::Rectangle { width, height, corner_radius, .. },
        } = layer_shape.source {
            assert!((width.value - 250.0).abs() < 0.01);
            assert!((height.value - 150.0).abs() < 0.01);
            assert!((corner_radius.value - 12.0).abs() < 0.01);
        } else {
            panic!("Expected rectangle shape layer");
        }

        let _ = state.nudge_layer_rect_dimensions(&shape_id, 20.0, -10.0, 2.0);
        let layer_shape = state.active_composition().unwrap().get_layer(&shape_id).unwrap().clone();
        if let LayerSource::Shape {
            shape_type: ShapeType::Rectangle { width, height, corner_radius, .. },
        } = layer_shape.source {
            assert!((width.value - 270.0).abs() < 0.01);
            assert!((height.value - 140.0).abs() < 0.01);
            assert!((corner_radius.value - 14.0).abs() < 0.01);
        } else {
            panic!("Expected rectangle shape layer");
        }
    }

    #[test]
    fn test_timeline_layer_trimming_and_slipping() {
        use crate::state::EditorState;
        use project::TimeCode;

        let mut state = EditorState::new();
        state.select_layer(Some("layer_bg".to_string()));

        // Initial in/out
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.in_point.frames(), 0);
        assert_eq!(l.out_point.frames(), 150);

        // Trim In-point to frame 15
        let _ = state.trim_layer_in_point("layer_bg", TimeCode::from_frames(15, 30.0));
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.in_point.frames(), 15);
        assert_eq!(l.out_point.frames(), 150);

        // Trim Out-point to frame 120
        let _ = state.trim_layer_out_point("layer_bg", TimeCode::from_frames(120, 30.0));
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.in_point.frames(), 15);
        assert_eq!(l.out_point.frames(), 120);

        // Nudge In-point and Out-point
        let _ = state.nudge_layer_in_point("layer_bg", 5);
        let _ = state.nudge_layer_out_point("layer_bg", -5);
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.in_point.frames(), 20);
        assert_eq!(l.out_point.frames(), 115);

        // Slip layer forward 10 frames (duration stays 95 frames)
        let dur_before = l.out_point.frames() - l.in_point.frames();
        let _ = state.slip_layer("layer_bg", 10);
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.in_point.frames(), 30);
        assert_eq!(l.out_point.frames(), 125);
        assert_eq!(l.out_point.frames() - l.in_point.frames(), dur_before);

        // Slip layer backward 15 frames
        let _ = state.slip_layer("layer_bg", -15);
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.in_point.frames(), 15);
        assert_eq!(l.out_point.frames(), 110);

        // AE Shortcuts: [ and ] to trim selected layer to playhead
        state.seek(1.0); // frame 30
        let _ = state.trim_selected_layer_in_to_playhead();
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.in_point.frames(), 30);

        state.seek(3.0); // frame 90
        let _ = state.trim_selected_layer_out_to_playhead();
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.out_point.frames(), 90);

        // Reset layer duration to full composition
        let _ = state.reset_layer_duration_to_comp("layer_bg");
        let l = state.active_composition().unwrap().get_layer("layer_bg").unwrap().clone();
        assert_eq!(l.in_point.frames(), 0);
        assert_eq!(l.out_point.frames(), 150);
    }

    #[gpui_kit::test]
    fn test_timeline_continuous_ruler_scrubbing_state(cx: &mut TestAppContext) {
        use crate::panels::TimelinePanel;
        use crate::state::EditorState;

        cx.update(gpui_kit::init);
        let state = cx.new(|_| EditorState::new());
        let timeline_panel = cx.new(|cx| TimelinePanel::new(state.clone(), cx));

        // Initial scrubbing state should be false
        timeline_panel.read_with(cx, |p, _| {
            assert!(!p.is_scrubbing_ruler);
        });

        // Set scrubbing state to true
        timeline_panel.update(cx, |p, _| {
            p.is_scrubbing_ruler = true;
        });
        timeline_panel.read_with(cx, |p, _| {
            assert!(p.is_scrubbing_ruler);
        });

        // Reset scrubbing state to false
        timeline_panel.update(cx, |p, _| {
            p.is_scrubbing_ruler = false;
        });
        timeline_panel.read_with(cx, |p, _| {
            assert!(!p.is_scrubbing_ruler);
        });
    }

    #[test]
    fn test_adjustment_layer_creation_evaluation_and_rendering() {
        use crate::state::EditorState;
        use project::{EffectType, LayerSource};

        let mut state = EditorState::new();
        let adj_id = state
            .add_adjustment_layer(Some("Global Grade"))
            .expect("created adjustment layer");

        let comp = state.active_composition().unwrap();
        let adj_layer = comp.get_layer(&adj_id).unwrap();
        assert_eq!(adj_layer.name, "Global Grade");
        assert!(matches!(adj_layer.source, LayerSource::Adjustment));

        // Add an effect to the adjustment layer
        state.select_layer(Some(adj_id.clone()));
        let _ = state.add_effect_to_selected_layer(EffectType::invert(100.0));

        // Evaluate frame
        let eval = state.evaluate_current_frame().expect("evaluation succeeds");
        let rendered_adj = eval.evaluated_layers.iter().find(|l| l.id == adj_id).expect("found rendered adjustment layer");
        assert!(rendered_adj.is_adjustment());
        assert_eq!(rendered_adj.effects.len(), 1);

        // After Effects semantics: a new adjustment layer lands on top and its
        // effects apply to every layer beneath it.
        let below_ids: Vec<String> = {
            let comp = state.active_composition().unwrap();
            assert_eq!(comp.layers.first().unwrap().id, adj_id);
            comp.layers.iter().skip(1).map(|l| l.id.clone()).collect()
        };
        // Badge starts at frame 15, so move the playhead where all layers are active.
        state.seek_frame(30);
        let eval = state.evaluate_current_frame().expect("evaluation succeeds");
        for layer in below_ids {
            let fx = eval.adjustment_effects_applying_to(&layer);
            assert_eq!(fx.len(), 1, "layer {layer} should inherit the adjustment FX");
            assert!(fx[0].enabled);
        }
        // Nothing applies to the adjustment layer itself.
        assert!(eval.adjustment_effects_applying_to(&adj_id).is_empty());
    }

    #[test]
    fn test_diamond_keyframe_works_with_stopwatch_off() {
        use crate::state::EditorState;
        use project::TimeCode;

        let mut state = EditorState::new();
        // Background solid rotation has no keyframes in the starter project.
        let layer_id = "layer_bg".to_string();
        state.select_layer(Some(layer_id.clone()));
        state.seek_frame(30);

        // Stopwatch off: no animation yet.
        let comp = state.active_composition().unwrap();
        assert!(!comp.get_layer(&layer_id).unwrap().transform.rotation.is_animated());

        // Clicking the diamond (timeline or properties) records the first
        // keyframe and enables animation — no stopwatch pre-toggle needed.
        state.toggle_layer_keyframe_at_current_time(&layer_id, "transform.rotation");

        let comp = state.active_composition().unwrap();
        let rot = &comp.get_layer(&layer_id).unwrap().transform.rotation;
        assert!(rot.is_animated());
        assert!(rot.has_keyframe_at(&TimeCode::from_frames(30, 30.0)));

        // Clicking the diamond again at the same playhead removes it.
        state.toggle_layer_keyframe_at_current_time(&layer_id, "transform.rotation");
        let comp = state.active_composition().unwrap();
        let rot = &comp.get_layer(&layer_id).unwrap().transform.rotation;
        assert!(!rot.has_keyframe_at(&TimeCode::from_frames(30, 30.0)));
    }

    #[test]
    fn test_new_layers_land_on_top_and_demo_bg_is_behind() {
        use crate::state::EditorState;
        use project::Color;

        let mut state = EditorState::new();

        // Starter comp: opaque background sits at the bottom of the stack.
        let comp = state.active_composition().unwrap();
        assert_eq!(comp.layers.last().unwrap().id, "layer_bg");

        // New solids go on top (After Effects convention), visible immediately.
        let top_id = state
            .add_solid_layer("Topper", Color::WHITE, 100, 100)
            .expect("added solid");
        let comp = state.active_composition().unwrap();
        assert_eq!(comp.layers.first().unwrap().id, top_id);

        // New text layers resolve to an installed system font, never a
        // hardcoded family that may be missing on this machine.
        let text_id = state.add_text_layer("Hi", None).expect("added text");
        assert!(!crate::state::default_font_family().is_empty());
        let resolved = crate::state::resolve_font_family("Definitely Not A Real Font 123");
        assert_eq!(resolved, crate::state::default_font_family());
        let comp = state.active_composition().unwrap();
        assert_eq!(comp.layers.first().unwrap().id, text_id);
    }

    #[test]
    fn test_text_properties_font_presets_and_color() {
        use crate::state::EditorState;
        use project::{Color, LayerSource};

        let mut state = EditorState::new();
        let text_id = state
            .add_text_layer("Hello GPUI", None)
            .expect("created text layer");

        let comp = state.active_composition().unwrap();
        let l = comp.get_layer(&text_id).unwrap();
        assert!(matches!(&l.source, LayerSource::Text { text, .. } if text.value == "Hello GPUI"));

        // Mutate font family, size, color, and content
        let _ = state.set_layer_font_family(&text_id, "Courier New");
        let _ = state.set_layer_font_size(&text_id, 72.0);
        let _ = state.set_layer_text_color(&text_id, Color::rgba(0.2, 0.4, 0.8, 1.0));
        let _ = state.set_layer_text(&text_id, "After Effects in Rust");

        let comp = state.active_composition().unwrap();
        let l = comp.get_layer(&text_id).unwrap();
        if let LayerSource::Text { text, font_family, font_size, fill_color, .. } = &l.source {
            assert_eq!(text.value, "After Effects in Rust");
            assert_eq!(font_family, "Courier New");
            assert_eq!(font_size.value, 72.0);
            assert_eq!(fill_color.value, Color::rgba(0.2, 0.4, 0.8, 1.0));
        } else {
            panic!("Expected text layer source");
        }
    }

    #[test]
    fn test_effect_color_pickers_and_presets() {
        use crate::state::EditorState;
        use project::{Color, EffectType};

        let mut state = EditorState::new();
        let comp = state.active_composition().unwrap().clone();
        let layer_id = comp.layers.first().unwrap().id.clone();
        state.select_layer(Some(layer_id.clone()));

        // Add Chroma Key, Tint, and Drop Shadow effects
        let _ = state.add_effect_to_selected_layer(EffectType::chroma_key(Color::GREEN, 0.15, 0.05));
        let _ = state.add_effect_to_selected_layer(EffectType::tint(Color::BLACK, Color::WHITE, 100.0));
        let _ = state.add_effect_to_selected_layer(EffectType::drop_shadow(10.0, 135.0, 5.0, 75.0, Color::BLACK));

        let comp = state.active_composition().unwrap();
        let l = comp.get_layer(&layer_id).unwrap();
        let chroma_id = l.effects.iter().find(|e| matches!(e.effect_type, EffectType::ChromaKey { .. })).unwrap().id.clone();
        let tint_id = l.effects.iter().find(|e| matches!(e.effect_type, EffectType::Tint { .. })).unwrap().id.clone();
        let shadow_id = l.effects.iter().find(|e| matches!(e.effect_type, EffectType::DropShadow { .. })).unwrap().id.clone();

        // Update chroma color
        let custom_green = Color::rgba(0.1, 0.95, 0.2, 1.0);
        let _ = state.set_chroma_key_color(&chroma_id, custom_green);

        // Update tint colors
        let navy = Color::rgba(0.05, 0.1, 0.3, 1.0);
        let gold = Color::rgba(1.0, 0.85, 0.2, 1.0);
        let _ = state.set_tint_colors(&tint_id, Some(navy), Some(gold));

        // Update drop shadow color
        let dark_purple = Color::rgba(0.2, 0.05, 0.25, 0.9);
        let _ = state.set_drop_shadow_color(&shadow_id, dark_purple);

        // Verify mutations
        let comp = state.active_composition().unwrap();
        let l = comp.get_layer(&layer_id).unwrap();
        for eff in &l.effects {
            match &eff.effect_type {
                EffectType::ChromaKey { key_color, .. } => assert_eq!(*key_color, custom_green),
                EffectType::Tint { map_black, map_white, .. } => {
                    assert_eq!(*map_black, navy);
                    assert_eq!(*map_white, gold);
                }
                EffectType::DropShadow { color, .. } => assert_eq!(*color, dark_purple),
                _ => {}
            }
        }
    }

    #[test]
    fn test_properties_panel_keyframe_jump_and_toggle() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        let layer_id = "layer_bg";

        // Enable animation for position at frame 0 (creates initial keyframe at frame 0)
        state.seek(0.0);
        state.toggle_layer_property_animation(layer_id, "transform.position");

        state.seek(1.0); // frame 30
        state.toggle_layer_keyframe_at_current_time(layer_id, "transform.position");

        state.seek(2.0); // frame 60
        state.toggle_layer_keyframe_at_current_time(layer_id, "transform.position");

        // Verify navigation
        state.seek(0.5); // between frame 0 and frame 30
        state.seek_previous_keyframe(layer_id, "transform.position");
        assert_eq!(state.clock.current_frame(), 0);

        state.seek_next_keyframe(layer_id, "transform.position");
        assert_eq!(state.clock.current_frame(), 30);

        state.seek_next_keyframe(layer_id, "transform.position");
        assert_eq!(state.clock.current_frame(), 60);

        state.seek_previous_keyframe(layer_id, "transform.position");
        assert_eq!(state.clock.current_frame(), 30);
    }

    #[test]
    fn test_shader_lab_params_apply_and_fallback() {
        use crate::state::EditorState;
        use project::EffectType;

        let mut state = EditorState::new();
        state.select_layer(Some("layer_accent".to_string()));
        let fx = state
            .add_effect_to_selected_layer(EffectType::shader_lab(project::shader::presets::GRADE))
            .expect("add shader lab");

        // Parameters auto-detected from uniforms.
        let before_params = {
            let comp = state.active_composition().unwrap();
            let eff = comp.get_layer("layer_accent").unwrap().get_effect(&fx).unwrap();
            assert_eq!(eff.shader_params().unwrap().len(), 3);
            assert!(eff.shader_error().is_none());
            eff.shader_source().unwrap().to_string()
        };

        // Live value edits.
        state.set_shaderlab_param(&fx, "brightness", 0.5).expect("set");
        state.nudge_shaderlab_param(&fx, "brightness", 1.0).expect("nudge");
        {
            let comp = state.active_composition().unwrap();
            let eff = comp.get_layer("layer_accent").unwrap().get_effect(&fx).unwrap();
            let resolved: std::collections::HashMap<_, _> =
                eff.resolved_shader_values().into_iter().collect();
            match &resolved["brightness"] {
                project::ShaderParamValue::Float(v) => assert!((v - 0.51).abs() < 1e-4),
                other => panic!("expected float, got {other:?}"),
            }
        }

        // Scrub-key path reaches the same parameter.
        let key = format!("sl:{fx}:brightness");
        assert!(state.set_scrub_value(&key, 0.25));
        assert!((state.scrub_current_value(&key).unwrap() - 0.25).abs() < 1e-4);

        // Broken source: error recorded, last-good source keeps running.
        assert!(state.apply_shader_source(&fx, "this is not a shader {{{").is_err());
        {
            let comp = state.active_composition().unwrap();
            let eff = comp.get_layer("layer_accent").unwrap().get_effect(&fx).unwrap();
            assert_eq!(eff.shader_source().unwrap(), before_params);
            assert!(eff.shader_error().is_some());
        }

        // Good source swaps in, clears the error, regenerates params.
        state
            .apply_shader_source(&fx, project::shader::presets::DUOTONE)
            .expect("duotone applies");
        let comp = state.active_composition().unwrap();
        let eff = comp.get_layer("layer_accent").unwrap().get_effect(&fx).unwrap();
        assert!(eff.shader_error().is_none());
        assert!(eff.shader_source().unwrap().contains("Duotone"));
        assert!(eff.shader_params().unwrap().iter().any(|p| p.name == "mixAmount"));

        // Evaluation carries the shader hash + resolved values to the GPU path.
        let eval = state.evaluate_current_frame().expect("evaluate");
        let layer = eval.get_layer("layer_accent").unwrap();
        let ee = layer.effects.iter().find(|e| e.id == fx).expect("evaluated fx");
        match &ee.effect_type {
            compositor::EvaluatedEffectType::ShaderLab { source_hash, values, .. } => {
                assert_ne!(*source_hash, 0);
                assert!(values.contains_key("mixAmount"));
            }
            other => panic!("expected ShaderLab, got {other:?}"),
        }
    }

    #[test]
    fn test_scale_link_uniform_default_and_toggle() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        state.select_layer(Some("layer_accent".to_string()));

        // Uniform by default: one value drives both axes.
        assert!(state.active_composition().unwrap().get_layer("layer_accent").unwrap().transform.scale_uniform);
        state.nudge_scale(10.0, 0.0);
        let sc = state.active_composition().unwrap().get_layer("layer_accent").unwrap().transform.scale.value;
        assert!((sc.x - 110.0).abs() < 1e-4 && (sc.y - 110.0).abs() < 1e-4);

        // Toggle to manual: axes move independently.
        state.toggle_selected_scale_link();
        assert!(!state.active_composition().unwrap().get_layer("layer_accent").unwrap().transform.scale_uniform);
        state.nudge_scale(10.0, 0.0);
        let sc = state.active_composition().unwrap().get_layer("layer_accent").unwrap().transform.scale.value;
        assert!((sc.x - 120.0).abs() < 1e-4 && (sc.y - 110.0).abs() < 1e-4);

        // Back to uniform snaps Y to X.
        state.toggle_selected_scale_link();
        let t = state.active_composition().unwrap().get_layer("layer_accent").unwrap().transform.clone();
        assert!(t.scale_uniform);
        assert!((t.scale.value.x - t.scale.value.y).abs() < 1e-4);
    }

    #[test]
    fn test_scrub_keyboard_entry_paths() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        state.select_layer(Some("layer_accent".to_string()));

        // Absolute sets through the scrub-key universe.
        assert!(state.set_scrub_value("pos_x", 123.0));
        assert!(state.set_scrub_value("rotation", 30.0));
        assert!(state.set_scrub_value("opacity", 80.0));
        let comp = state.active_composition().unwrap();
        let layer = comp.get_layer("layer_accent").unwrap();
        assert!((layer.transform.position.value.x - 123.0).abs() < 1e-4);
        assert!((layer.transform.rotation.value - 30.0).abs() < 1e-4);
        assert!((layer.opacity.value - 80.0).abs() < 1e-4);

        // Reads mirror writes.
        assert!((state.scrub_current_value("pos_x").unwrap() - 123.0).abs() < 1e-4);
        assert!(state.scrub_current_value("nope").is_none());
        assert!(!state.set_scrub_value("nope", 1.0));

        // Typed commit tolerates unit suffixes and rejects garbage.
        state.value_edit_key = Some("opacity".to_string());
        assert!(state.commit_typed_value("75 %"));
        assert!((state.active_composition().unwrap().get_layer("layer_accent").unwrap().opacity.value - 75.0).abs() < 1e-4);
        assert!(!state.commit_typed_value("abc"));
        assert!(state.end_value_edit_state());
        assert!(!state.end_value_edit_state());
    }

    #[test]
    fn test_timeline_scrub_values_layer_targeted() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        let lid = "layer_accent";

        // Layer-targeted nudges (drag/wheel path, no +/- buttons).
        state.nudge_timeline_value(lid, "pos_x", 10.0);
        state.nudge_timeline_value(lid, "rotation", 45.0);
        state.nudge_timeline_value(lid, "opacity", -5.0);
        assert!((state.timeline_current_value(lid, "pos_x").unwrap()
            - state.active_composition().unwrap().get_layer(lid).unwrap().transform.position.value.x).abs() < 1e-4);
        assert!((state.timeline_current_value(lid, "rotation").unwrap() - 45.0).abs() < 1e-4);

        // Absolute sets (keyboard entry path).
        assert!(state.set_timeline_value(lid, "pos_x", 200.0));
        assert!(state.set_timeline_value(lid, "rotation", 30.0));
        assert!(state.set_timeline_value(lid, "scale_x", 150.0));
        assert!((state.timeline_current_value(lid, "pos_x").unwrap() - 200.0).abs() < 1e-4);
        assert!((state.timeline_current_value(lid, "rotation").unwrap() - 30.0).abs() < 1e-4);

        // Effect params route through the same keys.
        let fx_id = state.add_effect_to_selected_layer(project::EffectType::gaussian_blur(10.0))
            .expect("blur added");
        let key = format!("fx:{fx_id}:radius");
        state.nudge_timeline_value(lid, &key, 5.0);
        assert!((state.timeline_current_value(lid, &key).unwrap() - 15.0).abs() < 1e-4);
        assert!(state.set_timeline_value(lid, &key, 42.0));
        assert!((state.timeline_current_value(lid, &key).unwrap() - 42.0).abs() < 1e-4);

        // tl: edit keys commit through the shared typed path.
        state.value_edit_key = Some(format!("tl:{lid}:opacity"));
        assert!(state.commit_typed_value("60%"));
        assert!((state.timeline_current_value(lid, "opacity").unwrap() - 60.0).abs() < 1e-4);

        // Unknown layers/keys fail cleanly.
        assert!(!state.set_timeline_value("missing", "pos_x", 1.0));
        assert!(state.timeline_current_value("missing", "pos_x").is_none());
        assert!(!state.set_timeline_value(lid, "bogus", 1.0));
    }

    #[test]
    fn test_new_composition_creation() {
        use crate::state::EditorState;
        use project::Color;

        let mut state = EditorState::new();
        let before = state.project.compositions.len();
        let id = state.add_composition("", 1280, 720, 25.0, 5.0, Color::WHITE)
            .expect("composition created");
        assert_eq!(state.project.compositions.len(), before + 1);
        assert_eq!(state.active_comp_id, id);
        let comp = state.active_composition().unwrap();
        assert_eq!(comp.width, 1280);
        assert_eq!(comp.height, 720);
        assert!((comp.frame_rate - 25.0).abs() < 1e-9);
        assert_eq!(comp.background_color, Color::WHITE);
        assert!(comp.name.starts_with("Composition"));
        // New comp starts empty with no selection.
        assert!(state.selected_layer_id.is_none());

        // Transparent background round-trips.
        let id2 = state.add_composition("Vertical", 1080, 1920, 30.0, 10.0, Color::TRANSPARENT)
            .expect("second composition");
        assert_eq!(state.active_composition().unwrap().name, "Vertical");
        assert_eq!(state.active_composition().unwrap().background_color, Color::TRANSPARENT);
        assert_ne!(id, id2);
    }

    #[test]
    fn test_luma_key_effect_round_trip() {        use crate::state::EditorState;
        use project::{Color, EffectType};

        // Model: constructor clamps, params addressable, nudge works.
        let mut fx = project::Effect::luma_key("fx_luma", 20.0, 10.0);
        assert_eq!(fx.type_name(), "Luma Key");
        assert!(matches!(fx.effect_type, EffectType::LumaKey { .. }));
        assert!(fx.get_param_property("threshold").is_some());
        assert!(fx.get_param_property("feather").is_some());
        assert!(fx.get_param_property("nope").is_none());
        assert!(fx.nudge_param("threshold", 5.0));
        assert!((fx.get_param_property("threshold").unwrap().value - 25.0).abs() < 1e-4);

        // Evaluation: dark pixels key out, bright pixels survive.
        let eval = compositor::EvaluatedEffectType::LumaKey { threshold: 50.0, feather: 5.0 };
        let dark = eval.process_color(Color::rgba(0.1, 0.1, 0.1, 1.0));
        assert!(dark.a < 0.01);
        let bright = eval.process_color(Color::rgba(0.9, 0.9, 0.9, 1.0));
        assert!((bright.a - 1.0).abs() < 1e-4);

        // End to end on a layer.
        let mut state = EditorState::new();
        let _ = state.add_effect_to_selected_layer(EffectType::luma_key(20.0, 10.0));
        let layer = state.selected_layer().unwrap();
        assert!(layer.effects.iter().any(|e| matches!(e.effect_type, EffectType::LumaKey { .. })));
    }

    #[test]
    fn test_new_effects_round_trip() {
        use crate::state::EditorState;
        use project::{Color, EffectType};

        // Every new effect constructs, names, nudges, and exposes params.
        let mut fx = project::Effect::checkerboard("c", 32.0, Color::BLACK, Color::WHITE);
        assert_eq!(fx.type_name(), "Checkerboard");
        assert!(fx.nudge_param("size", 8.0));
        assert!(fx.set_color_value("color_a", Color::WHITE));
        assert!(!fx.set_color_value("bogus", Color::WHITE));

        let mut fx = project::Effect::gradient_ramp("g", Color::BLACK, Color::WHITE, 90.0);
        assert_eq!(fx.type_name(), "Gradient Ramp");
        assert!(fx.nudge_param("angle", 10.0));
        assert!(fx.get_param_property("angle").is_some());

        for (mut e, name, param, delta, expect) in [
            (project::Effect::perspective("p", 0.0, 0.0), "Perspective", "skew_x", 5.0, 5.0),
            (project::Effect::text_outline("o", 3.0, Color::BLACK), "Text Outline", "width", 2.0, 5.0),
            (project::Effect::text_bevel("b", 60.0, 30.0), "Text Bevel", "strength", 10.0, 70.0),
            (project::Effect::bloom("bl", 40.0, 10.0), "Bloom", "intensity", 10.0, 50.0),
            (project::Effect::tiler("t", 2.0, 2.0), "Tiler", "tiles_x", 2.0, 4.0),
            (project::Effect::warp("w", 30.0, 1.0), "Warp", "amount", 10.0, 40.0),
            (project::Effect::exposure("e", 0.0), "Exposure", "exposure", 1.0, 1.0),
            (project::Effect::vibrance("v", 30.0), "Vibrance", "vibrance", -10.0, 20.0),
        ] {
            assert_eq!(e.type_name(), name);
            assert!(e.nudge_param(param, delta), "{name}");
            assert!((e.get_param_property(param).unwrap().value - expect).abs() < 1e-4, "{name}");
        }
        let _ = EffectType::noise_generator(10.0, false);

        // Per-pixel math: exposure doubles, vibrance lifts muted color,
        // bloom lifts highlights, spatial ones stay identity.
        let mid = Color::rgba(0.25, 0.25, 0.25, 1.0);
        let ev = compositor::EvaluatedEffectType::Exposure { exposure: 1.0 };
        let out = ev.process_color(mid);
        assert!((out.r - 0.5).abs() < 1e-4);
        let vib = compositor::EvaluatedEffectType::Vibrance { vibrance: 100.0 };
        let muted = Color::rgba(0.5, 0.4, 0.4, 1.0);
        let boosted = vib.process_color(muted);
        assert!((boosted.r - muted.r).abs() > 0.01);
        let gray = Color::rgba(0.5, 0.5, 0.5, 1.0);
        assert!((vib.process_color(gray).r - 0.5).abs() < 1e-4);
        let bl = compositor::EvaluatedEffectType::Bloom { intensity: 100.0, radius: 5.0 };
        let bright = Color::rgba(0.8, 0.8, 0.8, 1.0);
        assert!(bl.process_color(bright).r > 0.85);
        let chk = compositor::EvaluatedEffectType::Checkerboard {
            size: 32.0, color_a: Color::BLACK, color_b: Color::WHITE,
        };
        assert!(chk.is_spatial());
        assert_eq!(chk.process_color(mid), mid);

        // End to end: browser rows apply onto the selected layer.
        let mut state = EditorState::new();
        for et in [
            EffectType::checkerboard(32.0, Color::BLACK, Color::WHITE),
            EffectType::gradient_ramp(Color::BLACK, Color::WHITE, 90.0),
            EffectType::perspective(5.0, -5.0),
            EffectType::text_outline(3.0, Color::BLACK),
            EffectType::text_bevel(60.0, 30.0),
            EffectType::bloom(40.0, 10.0),
            EffectType::tiler(2.0, 2.0),
            EffectType::warp(30.0, 1.0),
            EffectType::exposure(1.0),
            EffectType::vibrance(30.0),
        ] {
            let name = et.type_name();
            state.add_effect_to_selected_layer(et).expect(name);
        }
        assert_eq!(state.selected_layer().unwrap().effects.len(), 10);
    }

    #[test]
    fn test_gizmo_setters_and_center_pivot() {
        use crate::state::EditorState;
        use project::Vec2;

        let mut state = EditorState::new();
        let lid = "layer_accent".to_string();

        // Absolute setters (gizmo drag endpoints).
        state.set_layer_rotation(&lid, 45.0);
        state.set_layer_scale(&lid, 150.0, 120.0);
        state.set_layer_position(&lid, Vec2::new(100.0, -50.0));
        {
            let l = state.active_composition().unwrap().get_layer(&lid).unwrap();
            assert!((l.transform.rotation.value - 45.0).abs() < 1e-4);
            assert!((l.transform.scale.value.x - 150.0).abs() < 1e-4);
            assert!((l.transform.position.value.x - 100.0).abs() < 1e-4);
        }

        // Pan-behind anchor move keeps rendered pixels in place: a fixed
        // content point maps to the same world position after the move.
        let before = state.evaluate_current_frame().unwrap();
        let lay = before.get_layer(&lid).unwrap();
        let probe = Vec2::new(0.0, 0.0);
        let w0 = lay.local_to_world_point(probe);
        state.move_layer_anchor(&lid, Vec2::new(20.0, 10.0));
        let after = state.evaluate_current_frame().unwrap();
        let lay2 = after.get_layer(&lid).unwrap();
        let w1 = lay2.local_to_world_point(probe);
        assert!((w1.x - w0.x).abs() < 1e-3);
        assert!((w1.y - w0.y).abs() < 1e-3);

        // Center pivot lands on the content center.
        state.reset_layer_anchor_center(&lid);
        let l = state.active_composition().unwrap().get_layer(&lid).unwrap();
        // Accent solid is 300x300 -> pivot (150, 150).
        assert!((l.transform.anchor_point.value.x - 150.0).abs() < 1e-4);
        assert!((l.transform.anchor_point.value.y - 150.0).abs() < 1e-4);

        // Tool defaults drive new layers.
        state.tool_font_size = 72.0;
        state.tool_text_color = project::Color::from_hex("#EF4444").unwrap();
        state.tool_shape_fill = project::Color::from_hex("#10B981").unwrap();
        let tid = state.add_text_layer("Hi", None).unwrap();
        let tl = state.active_composition().unwrap().get_layer(&tid).unwrap();
        if let project::LayerSource::Text { font_size, fill_color, .. } = &tl.source {
            assert!((font_size.value - 72.0).abs() < 1e-4);
            assert_eq!(fill_color.value, project::Color::from_hex("#EF4444").unwrap());
        } else {
            panic!("expected text");
        }
        let sid = state.add_rectangle_shape_layer(100.0, 100.0, None).unwrap();
        let sl = state.active_composition().unwrap().get_layer(&sid).unwrap();
        if let project::LayerSource::Shape { shape_type: project::ShapeType::Rectangle { fill, .. } } = &sl.source {
            assert_eq!(*fill, project::Color::from_hex("#10B981").unwrap());
        } else {
            panic!("expected rect");
        }
        assert!(state.set_layer_shape_fill(&sid, project::Color::BLACK).is_ok());
    }
}
