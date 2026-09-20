mod panels;
pub mod state;
use state::EditorState;

use std::rc::Rc;

pub use gpui_kit::base::TestSupportExt;
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

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state_key = self.state.clone();
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
                if event.keystroke.key == "space" || event.keystroke.key == " " {
                    state_key.update(cx, |s, cx| {
                        s.toggle_playback();
                        cx.notify();
                    });
                }
            })
            .child(self.dock_area.clone())
    }
}

fn main() {
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
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
    use gpui_kit::{px, size, AppContext as _, Entity, TestAppContext};

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

                // Verify configured initial dock sizes
                assert_eq!(dock.dock_size(DockPlacement::Left), Some(px(280.)));
                assert_eq!(dock.dock_size(DockPlacement::Right), Some(px(300.)));
                assert_eq!(dock.dock_size(DockPlacement::Bottom), Some(px(260.)));
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
        use super::TogglePlayback;

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

            // Dispatch TogglePlayback action (deferred by GPUI)
            window.dispatch_action(Box::new(TogglePlayback), cx);
        })
        .expect("update_window failed");

        cx.run_until_parked();
        assert!(app_view.read_with(cx, |view, cx| view.state().read(cx).is_playing));

        cx.update_window(handle.into(), |_, window, cx| {
            // Dispatch TogglePlayback again (deferred by GPUI)
            window.dispatch_action(Box::new(TogglePlayback), cx);
        })
        .expect("update_window failed");

        cx.run_until_parked();
        assert!(!app_view.read_with(cx, |view, cx| view.state().read(cx).is_playing));
    }
}

