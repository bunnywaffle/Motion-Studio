pub(crate) mod panels;
pub mod raster;
pub mod state;
pub mod widgets;
use state::{EditorState, EditorTool};

use std::rc::Rc;

pub use gpui_kit::base::{h_flex, v_flex, Positioner, StyledExt, TestSupportExt};
pub use gpui_kit::component::dock::{
    panel_handle, BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, Panel, PanelStyle,
};
use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeMode};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;

pub use panels::{
    AppPanels, CompositionPanel, CompositionViewerPanel, EffectsPanel, ProjectPanel,
    PropertiesPanel, TimelinePanel,
};

actions!(
    workspace,
    [
        TogglePlayback,
        SelectMoveTool,
        SelectHandTool,
        SelectRotateTool,
        SelectPenTool,
        SelectTextTool,
        CycleShapeTool,
        FitGraphView
    ]
);

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
    /// Open top-level menu (File / Edit / About), if any.
    open_menu: Option<TopMenu>,
    /// About dialog visibility.
    show_about: bool,
    /// New-project dialog visibility.
    show_new_project: bool,
    /// Project settings / manager dialog visibility.
    pub show_project_manager: bool,
    /// Last menu/file action note (saved path, errors).
    menu_note: Option<String>,
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
        // One desktop command per tool (toolbar clicks dispatch the same
        // actions — see the root view's on_action handlers).
        cx.bind_keys([
            KeyBinding::new("v", SelectMoveTool, None),
            KeyBinding::new("h", SelectHandTool, None),
            KeyBinding::new("w", SelectRotateTool, None),
            KeyBinding::new("g", SelectPenTool, None),
            KeyBinding::new("t", SelectTextTool, None),
            KeyBinding::new("q", CycleShapeTool, None),
            KeyBinding::new("f", FitGraphView, None),
        ]);

        // Setup background 60Hz playback loop
        let loop_state = state.clone();
        let playback_task = cx.spawn(|cx: &mut AsyncApp| {
            let cx = cx.clone();
            let state = loop_state;
            async move {
                let mut last_instant = std::time::Instant::now();
                loop {
                    let is_playing = state.read_with(&cx, |editor, _| editor.is_playing);
                    if is_playing {
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
                    } else {
                        cx.background_executor().timer(std::time::Duration::from_millis(80)).await;
                        last_instant = std::time::Instant::now();
                    }
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
            open_menu: None,
            show_about: false,
            show_new_project: false,
            show_project_manager: false,
            menu_note: None,
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

// --- Top menu bar (File / Edit / About) ---

/// Top-level menu ids for the application menu bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TopMenu {
    File,
    Edit,
    Composition,
    Layer,
    View,
    About,
}

impl TopMenu {
    fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::Composition => "Composition",
            Self::Layer => "Layer",
            Self::View => "View",
            Self::About => "About",
        }
    }
}

/// Ask the OS for a project file to open (rfd on a worker thread so the
/// UI never blocks), then load it into the session.
fn request_open_project(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    let s_open = state.clone();
    let a_open = app.clone();
    cx.spawn(|cx: &mut AsyncApp| {
        let cx = cx.clone();
        async move {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("file-dialog-worker".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let file = rfd::FileDialog::new()
                        .add_filter("Motion Project", &["json", "motion"])
                        .pick_file();
                    let _ = tx.send(file);
                });
            if let Ok(Some(path)) = rx.recv() {
                cx.update(|cx| {
                    s_open.update(cx, |s, cx| {
                        match s.load_project_from(&path) {
                            Ok(()) => {
                                a_open.update(cx, |a, cx| {
                                    a.menu_note = Some(format!(
                                        "Opened {}",
                                        path.file_name().and_then(|n| n.to_str()).unwrap_or("project")
                                    ));
                                    cx.notify();
                                });
                            }
                            Err(e) => {
                                a_open.update(cx, |a, cx| {
                                    a.menu_note = Some(format!("Open failed: {e}"));
                                    cx.notify();
                                });
                            }
                        }
                        cx.notify();
                    });
                });
            }
        }
    })
    .detach();
}

/// Ask the OS where to save the project, then save it.
fn request_save_project_as(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    let s_save = state.clone();
    let a_save = app.clone();
    let default_name = state
        .read(cx)
        .project_display_name()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '_' || c == '-' { c } else { '_' })
        .collect::<String>();
    cx.spawn(|cx: &mut AsyncApp| {
        let cx = cx.clone();
        async move {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("file-dialog-worker".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let file = rfd::FileDialog::new()
                        .add_filter("Motion Project", &["json", "motion"])
                        .set_file_name(format!("{default_name}.json"))
                        .save_file();
                    let _ = tx.send(file);
                });
            if let Ok(Some(path)) = rx.recv() {
                cx.update(|cx| {
                    s_save.update(cx, |s, cx| {
                        match s.save_project_to(&path) {
                            Ok(()) => {
                                a_save.update(cx, |a, cx| {
                                    a.menu_note = Some(format!(
                                        "Saved {}",
                                        path.file_name().and_then(|n| n.to_str()).unwrap_or("project")
                                    ));
                                    cx.notify();
                                });
                            }
                            Err(e) => {
                                a_save.update(cx, |a, cx| {
                                    a.menu_note = Some(format!("Save failed: {e}"));
                                    cx.notify();
                                });
                            }
                        }
                        cx.notify();
                    });
                });
            }
        }
    })
    .detach();
}

/// Save to the remembered path, or fall back to Save As for untitled work.
fn request_save_project(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    if state.read(cx).project_path.is_none() {
        request_save_project_as(app, state, cx);
        return;
    }
    let s_save = state.clone();
    let a_save = app.clone();
    s_save.update(cx, |s, cx| {
        match s.save_project() {
            Ok(()) => {
                a_save.update(cx, |a, cx| {
                    a.menu_note = Some(format!("Saved {}", s.project_display_name()));
                    cx.notify();
                });
            }
            Err(e) => {
                a_save.update(cx, |a, cx| {
                    a.menu_note = Some(format!("Save failed: {e}"));
                    cx.notify();
                });
            }
        }
        cx.notify();
    });
}

/// One menu-bar dropdown item (label + optional shortcut hint).
fn menu_item<F>(id: String, label: String, hint: Option<String>, enabled: bool, cx: &App, on_pick: F) -> AnyElement
where
    F: Fn(&mut App) + 'static,
{
    let mut row = h_flex()
        .id(SharedString::from(id))
        .test_support()
        .px_2()
        .py_1()
        .rounded_sm()
        .items_center()
        .justify_between()
        .gap_4()
        .text_xs();
    if enabled {
        row = row
            .cursor_pointer()
            .text_color(cx.theme().foreground)
            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                on_pick(cx);
            });
    } else {
        row = row.text_color(cx.theme().muted_foreground.opacity(0.5));
    }
    row.child(label)
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground.opacity(0.7))
                .child(hint.unwrap_or_default()),
        )
        .into_any_element()
}

/// The File / Edit / About bar above the toolbar. Dropdowns anchor under
/// their buttons (parent-relative, no coordinate math) with viewport
/// clamping handled by the panel layout.
fn render_menubar(
    app: &Entity<AppView>,
    open_menu: Option<TopMenu>,
    menu_note: Option<String>,
    state: &Entity<EditorState>,
    cx: &App,
) -> AnyElement {
    let proj_name = state.read(cx).project_display_name();
    let can_undo = state.read(cx).can_undo();
    let can_redo = state.read(cx).can_redo();
    let has_selection = state.read(cx).selected_layer_id.is_some();
    let recents: Vec<std::path::PathBuf> = state.read(cx).recent_projects.clone();

    let mut bar = h_flex()
        .id("top_menubar")
        .test_support()
        .w_full()
        .h(px(30.))
        .px_2()
        .gap_0p5()
        .items_center()
        .bg(cx.theme().background)
        .border_b_1()
        .border_color(cx.theme().border)
        .text_color(cx.theme().foreground);

    for menu in [TopMenu::File, TopMenu::Edit, TopMenu::Composition, TopMenu::Layer, TopMenu::View, TopMenu::About] {
        let a_toggle = app.clone();
        let is_open = open_menu == Some(menu);
        let mut btn = div()
            .id(SharedString::from(format!("menubar_{:?}", menu).to_lowercase()))
            .test_support()
            .cursor_pointer()
            .px_2()
            .py_1()
            .rounded_sm()
            .text_xs()
            .text_color(cx.theme().foreground)
            .hover(|s| s.bg(cx.theme().muted))
            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                a_toggle.update(cx, |this, cx| {
                    this.open_menu = if this.open_menu == Some(menu) { None } else { Some(menu) };
                    cx.notify();
                });
            })
            .child(menu.label());
        if is_open {
            btn = btn.bg(cx.theme().muted);
            let mut items = v_flex().gap_0p5().p_1().min_w(px(220.));
            match menu {
                TopMenu::File => {
                    {
                        let a = app.clone();
                        items = items.child(menu_item("menu_new_project".to_string(), "New Project…".to_string(), None, true, cx, move |cx| {
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                this.show_new_project = true;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_open_project".to_string(), "Open Project…".to_string(), Some("Ctrl+O".to_string()), true, cx, move |cx| {
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                            request_open_project(&a, &s, cx);
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_save_project".to_string(), "Save".to_string(), Some("Ctrl+S".to_string()), true, cx, move |cx| {
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                            request_save_project(&a, &s, cx);
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_save_as".to_string(), "Save As…".to_string(), None, true, cx, move |cx| {
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                            request_save_project_as(&a, &s, cx);
                        }));
                    }
                    {
                        let a = app.clone();
                        items = items.child(menu_item(
                            "menu_project_manager".to_string(),
                            "Project Settings & Manager…".to_string(),
                            None,
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.show_project_manager = true;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    items = items.child(
                        div().h(px(1.)).my_0p5().bg(cx.theme().border),
                    );
                    if recents.is_empty() {
                        items = items.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground.opacity(0.6))
                                .child("No recent projects"),
                        );
                    } else {
                        for (idx, path) in recents.iter().take(6).enumerate() {
                            let (a, s) = (app.clone(), state.clone());
                            let p = path.clone();
                            let label = p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("project")
                                .to_string();
                            items = items.child(menu_item(
                                format!("menu_recent_{idx}"),
                                label.clone(),
                                None,
                                true,
                                cx,
                                move |cx| {
                                    let label_in = label.clone();
                                    a.update(cx, |this, cx| {
                                        this.open_menu = None;
                                        cx.notify();
                                    });
                                    let p_in = p.clone();
                                    let (a_in, s_in) = (a.clone(), s.clone());
                                    s_in.update(cx, |s, cx| {
                                        match s.load_project_from(&p_in) {
                                            Ok(()) => {
                                                a_in.update(cx, |a, cx| {
                                                    a.menu_note = Some(format!("Opened {label_in}"));
                                                    cx.notify();
                                                });
                                            }
                                            Err(e) => {
                                                a_in.update(cx, |a, cx| {
                                                    a.menu_note = Some(format!("Open failed: {e}"));
                                                    cx.notify();
                                                });
                                            }
                                        }
                                        cx.notify();
                                    });
                                },
                            ));
                        }
                    }
                }
                TopMenu::Edit => {
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_undo".to_string(), "Undo".to_string(), Some("Ctrl+Z".to_string()), can_undo, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                s.undo();
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_redo".to_string(), "Redo".to_string(), Some("Ctrl+Y".to_string()), can_redo, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                s.redo();
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    items = items.child(
                        div().h(px(1.)).my_0p5().bg(cx.theme().border),
                    );
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_duplicate_layer".to_string(),
                            "Duplicate Layer".to_string(),
                            Some("Ctrl+D".to_string()),
                            has_selection,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    if let Some(id) = s.selected_layer_id.clone() {
                                        let _ = s.duplicate_layer(&id);
                                    }
                                    cx.notify();
                                });
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_delete_layer".to_string(),
                            "Delete Layer".to_string(),
                            Some("Del".to_string()),
                            has_selection,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    let _ = s.delete_selected_layer();
                                    cx.notify();
                                });
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                }
                TopMenu::Composition => {
                    {
                        let (a, s) = (app.clone(), state.clone());
                        let playing = state.read(cx).is_playing;
                        items = items.child(menu_item(
                            "menu_play_pause".to_string(),
                            if playing { "Pause".to_string() } else { "Play".to_string() },
                            Some("Space".to_string()),
                            true,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    s.toggle_playback();
                                    cx.notify();
                                });
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_step_back".to_string(), "Step Back".to_string(), Some("←".to_string()), true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                s.step_backward();
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_step_forward".to_string(), "Step Forward".to_string(), Some("→".to_string()), true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                s.step_forward();
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_go_start".to_string(), "Go to Start".to_string(), Some("Home".to_string()), true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                s.jump_to_start();
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_go_end".to_string(), "Go to End".to_string(), Some("End".to_string()), true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                s.jump_to_end();
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    items = items.child(
                        div().h(px(1.)).my_0p5().bg(cx.theme().border),
                    );
                    {
                        let a = app.clone();
                        items = items.child(menu_item(
                            "menu_project_manager2".to_string(),
                            "Project Settings & Manager…".to_string(),
                            None,
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.show_project_manager = true;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                }
                TopMenu::Layer => {
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_new_solid".to_string(), "New Solid".to_string(), None, true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                let _ = s.add_solid_layer("New Solid", project::Color::from_rgba_u8(245, 158, 11, 255), 400, 400);
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_new_text".to_string(), "New Text".to_string(), None, true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                let _ = s.add_text_layer("New Text", None);
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_new_rect".to_string(), "New Rectangle".to_string(), None, true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                let _ = s.add_rectangle_shape_layer(300.0, 200.0, None);
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_new_ellipse".to_string(), "New Ellipse".to_string(), None, true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                let _ = s.add_ellipse_shape_layer(150.0, 100.0, None);
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item("menu_new_adjustment".to_string(), "New Adjustment Layer".to_string(), None, true, cx, move |cx| {
                            s.update(cx, |s, cx| {
                                let _ = s.add_adjustment_layer(None);
                                cx.notify();
                            });
                            a.update(cx, |this, cx| {
                                this.open_menu = None;
                                cx.notify();
                            });
                        }));
                    }
                    items = items.child(
                        div().h(px(1.)).my_0p5().bg(cx.theme().border),
                    );
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_layer_up".to_string(),
                            "Bring Forward".to_string(),
                            None,
                            has_selection,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    let _ = s.move_selected_layer_up();
                                    cx.notify();
                                });
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_layer_down".to_string(),
                            "Send Backward".to_string(),
                            None,
                            has_selection,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    let _ = s.move_selected_layer_down();
                                    cx.notify();
                                });
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                }
                TopMenu::View => {
                    {
                        let (a, s) = (app.clone(), state.clone());
                        let full = state.read(cx).timeline_full_width;
                        items = items.child(menu_item(
                            "menu_timeline_width".to_string(),
                            if full { "Timeline: Docked".to_string() } else { "Timeline: Full Width".to_string() },
                            None,
                            true,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    s.toggle_timeline_full_width();
                                    cx.notify();
                                });
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        let graph = state.read(cx).spline_editor_open;
                        items = items.child(menu_item(
                            "menu_graph_view".to_string(),
                            if graph { "Timeline View".to_string() } else { "Graph (Splines) View".to_string() },
                            None,
                            true,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    s.toggle_spline_editor();
                                    cx.notify();
                                });
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        let half = state.read(cx).preview_quality == crate::state::PreviewQuality::Half;
                        items = items.child(menu_item(
                            "menu_preview_quality".to_string(),
                            if half { "Preview Quality: Full".to_string() } else { "Preview Quality: Half".to_string() },
                            None,
                            true,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    s.set_preview_quality(if half {
                                        crate::state::PreviewQuality::Full
                                    } else {
                                        crate::state::PreviewQuality::Half
                                    });
                                    cx.notify();
                                });
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                }
                TopMenu::About => {
                    let a = app.clone();
                    items = items.child(menu_item("menu_about".to_string(), "About Motion Studio".to_string(), None, true, cx, move |cx| {
                        a.update(cx, |this, cx| {
                            this.open_menu = None;
                            this.show_about = true;
                            cx.notify();
                        });
                    }));
                }
            }
            let dropdown = div()
                .absolute()
                .top_full()
                .left_0()
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .rounded_md()
                .shadow_lg()
                .child(items);
            btn = btn.child(dropdown);
        }
        bar = bar.child(btn);
    }

    bar.child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .justify_center()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .truncate()
                    .child(proj_name),
            ),
    )
    .child(
        div()
            .min_w_0()
            .max_w(px(320.))
            .truncate()
            .text_xs()
            .text_color(cx.theme().primary)
            .child(menu_note.unwrap_or_default()),
    )
    .into_any_element()
}

fn render_toolbar(state: &Entity<EditorState>, cx: &App) -> impl IntoElement {
    use crate::state::EditorTool;
    let s_read = state.read(cx);
    let active_tool = s_read.active_tool;
    let (tool_name, tool_key) = match active_tool {
        EditorTool::Move => ("Selection", "V"),
        EditorTool::Hand => ("Hand", "H"),
        EditorTool::Rotate => ("Rotate", "W"),
        EditorTool::Pen => ("Pen", "G"),
        EditorTool::Text => ("Text", "T"),
        EditorTool::ShapeRect => ("Rectangle", "Q"),
        EditorTool::ShapeEllipse => ("Ellipse", "Q"),
    };
    let next_tool = match active_tool {
        EditorTool::Move => EditorTool::Hand,
        EditorTool::Hand => EditorTool::Rotate,
        EditorTool::Rotate => EditorTool::Pen,
        EditorTool::Pen => EditorTool::Text,
        EditorTool::Text => EditorTool::ShapeRect,
        EditorTool::ShapeRect => EditorTool::ShapeEllipse,
        EditorTool::ShapeEllipse => EditorTool::Move,
    };
    let fill = s_read.tool_solid_color;
    let fill_hex = fill.to_hex_rgb();
    let snapping = s_read.snapping;
    let is_full_width = s_read.timeline_full_width;
    let s_tool = state.clone();
    let s_snap = state.clone();
    let s_full = state.clone();
    // AE chrome tokens (match panels::ae).
    let bar = rgb(0x232323);
    let ctl = rgb(0x2e2e2e);
    let hov = rgb(0x3a3a3a);
    let txt = rgb(0xd7d7d7);
    let dim = rgb(0x9a9a9a);
    let acc = rgb(0x2f7cf6);

    h_flex()
        .id("top_toolbar")
        .test_support()
        .w_full()
        .h(px(36.))
        .px_3()
        .gap_3()
        .border_b_1()
        .border_color(rgb(0x101010))
        .bg(bar)
        .items_center()
        .justify_between()
        .text_xs()
        .text_color(txt)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                // Active tool cycler.
                .child(
                    div()
                        .id("active_tool_pill")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(ctl)
                        .hover(|s| s.bg(hov))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_tool.update(cx, |s, cx| {
                                s.set_tool(next_tool);
                                cx.notify();
                            });
                        })
                        .child(format!("Active Tool: {tool_name} ({tool_key})")),
                )
                // Fill / Stroke wells.
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .text_color(dim)
                        .child(div().child("Fill:"))
                        .child(div().w(px(12.)).h(px(12.)).rounded_sm().bg(Rgba { r: fill.r, g: fill.g, b: fill.b, a: 1.0 }))
                        .child(div().font_medium().text_color(txt).child(fill_hex)),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .text_color(dim)
                        .child(div().child("Stroke:"))
                        .child(div().child("None")),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                // Snapping magnet.
                .child(
                    div()
                        .id("snapping_toggle")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(if snapping { acc } else { ctl })
                        .hover(|s| s.bg(hov))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_snap.update(cx, |s, cx| {
                                s.toggle_snapping();
                                cx.notify();
                            });
                        })
                        .child("Snapping"),
                )
                .child(div().text_color(dim).child("100% (Fit)"))
                .child(div().text_color(dim).child("Full Res (1:1)"))
                .child(div().text_color(dim).child("RGB Channel"))
                .child(div().text_color(dim).child("+"))
                .child(
                    div()
                        .id("toggle_timeline_full_width_button")
                        .test_support()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .flex()
                        .items_center()
                        .gap_1()
                        .bg(if is_full_width { acc } else { ctl })
                        .hover(|s| s.opacity(0.85))
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            s_full.update(cx, |s, cx| {
                                s.toggle_timeline_full_width();
                                cx.notify();
                            });
                        })
                        .child(div().w(px(14.)).h(px(14.)).flex().items_center().justify_center().child(if is_full_width { gpui_kit::assets::IconName::Minimize2 } else { gpui_kit::assets::IconName::Maximize2 }))
                        .child(if is_full_width { "Timeline: Full Width" } else { "Timeline: Docked" }),
                ),
        )
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state_key = self.state.clone();
        let app_key = cx.entity().clone();
        let toolbar = render_toolbar(&self.state, cx);
        let menubar = render_menubar(&cx.entity(), self.open_menu, self.menu_note.clone(), &self.state, cx);

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

        let close_key = cx.entity().clone();
        let new_name_input: Entity<InputState> = window.use_keyed_state("new_project_name", cx, |window, cx| {
            InputState::new(window, cx)
        });
        // Centered modal dialogs (About, New Project) via viewport-clamped
        // deferred positioning — same infrastructure as context menus.
        let vw_f = vw as f32;
        let vh_f = vh as f32;
        let mut dialogs: Vec<AnyElement> = Vec::new();
        if self.show_about {
            let a_close = cx.entity().clone();
            let dlg_pos = point(px((vw_f - 320.0).max(8.0) / 2.0), px((vh_f - 260.0).max(8.0) / 2.0));
            dialogs.push(
                deferred(
                    Positioner::corner(Anchor::TopLeft, dlg_pos)
                        .margin(px(8.))
                        .occlude()
                        .child(
                            div()
                                .id("about_dialog")
                                .test_support()
                                .w(px(320.))
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded_md()
                                .shadow_lg()
                                .p_4()
                                .child(
                                    v_flex()
                                        .gap_2()
                                        .child(div().font_bold().text_sm().child("Motion Studio"))
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                                        )
                                        .child(
                                            div().text_xs().child(
                                                "After Effects-style compositing: CPU raster viewport, transform gizmo, keyframe spline editor, Shader Lab shaders, and 31 GPU-validated effects.",
                                            ),
                                        )
                                        .child(
                                            div()
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
                                                    a_close.update(cx, |this, cx| {
                                                        this.show_about = false;
                                                        cx.notify();
                                                    });
                                                })
                                                .child("Close"),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
            );
        }
        if self.show_new_project {
            let a_create = cx.entity().clone();
            let a_cancel = cx.entity().clone();
            let s_create = self.state.clone();
            let input_create = new_name_input.clone();
            let dlg_pos = point(px((vw_f - 320.0).max(8.0) / 2.0), px((vh_f - 200.0).max(8.0) / 2.0));
            dialogs.push(
                deferred(
                    Positioner::corner(Anchor::TopLeft, dlg_pos)
                        .margin(px(8.))
                        .occlude()
                        .child(
                            div()
                                .id("new_project_dialog")
                                .test_support()
                                .w(px(320.))
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded_md()
                                .shadow_lg()
                                .p_4()
                                .child(
                                    v_flex()
                                        .gap_2()
                                        .child(div().font_bold().text_sm().child("New Project"))
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Name the project (a fresh 1080p composition is included):"),
                                        )
                                        .child(Input::new(&new_name_input).id("new_project_name_input").w_full())
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .justify_end()
                                                .child(
                                                    div()
                                                        .id("new_project_create")
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
                                                            let name = input_create.read(cx).value().to_string();
                                                            s_create.update(cx, |s, cx| {
                                                                s.new_project(&name);
                                                                cx.notify();
                                                            });
                                                            a_create.update(cx, |this, cx| {
                                                                this.show_new_project = false;
                                                                this.menu_note = Some("New project created".to_string());
                                                                cx.notify();
                                                            });
                                                        })
                                                        .child("Create"),
                                                )
                                                .child(
                                                    div()
                                                        .cursor_pointer()
                                                        .px_3()
                                                        .py_1()
                                                        .rounded_sm()
                                                        .bg(cx.theme().muted)
                                                        .text_color(cx.theme().muted_foreground)
                                                        .text_xs()
                                                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                            a_cancel.update(cx, |this, cx| {
                                                                this.show_new_project = false;
                                                                cx.notify();
                                                            });
                                                        })
                                                        .child("Cancel"),
                                                ),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
            );
        }
        if self.show_project_manager {
            let a_close = cx.entity().clone();
            let dlg_w = 480.0f32;
            let dlg_h = 440.0f32;
            let dlg_pos = point(
                px((vw_f - dlg_w).max(8.0) / 2.0),
                px((vh_f - dlg_h).max(8.0) / 2.0),
            );

            let s_r = self.state.read(cx);
            let proj_name = s_r.project.name.clone();
            let comps = s_r.project.compositions.clone();
            let active_id = s_r.active_comp_id.clone();
            let asset_count = s_r.project.assets.len();
            let active_comp = s_r.active_composition().cloned();

            let mut comp_rows = v_flex().gap_1p5().w_full();
            for comp in &comps {
                let is_active = comp.id == active_id;
                let c_id = comp.id.clone();
                let s_sw = self.state.clone();
                let s_del = self.state.clone();
                let a_sw = cx.entity().clone();
                let a_del = cx.entity().clone();

                let mut row = h_flex()
                    .w_full()
                    .p_2()
                    .rounded_sm()
                    .items_center()
                    .justify_between()
                    .border_1()
                    .border_color(if is_active { cx.theme().primary } else { cx.theme().border })
                    .bg(if is_active { cx.theme().muted } else { cx.theme().secondary });

                row = row.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .text_xs()
                                .font_semibold()
                                .bg(if is_active { cx.theme().primary } else { cx.theme().muted })
                                .text_color(if is_active { cx.theme().primary_foreground } else { cx.theme().muted_foreground })
                                .child(if is_active { "ACTIVE" } else { "COMP" })
                        )
                        .child(
                            v_flex()
                                .gap_0p5()
                                .child(div().font_semibold().text_xs().text_color(cx.theme().foreground).child(comp.name.clone()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{}×{} • {:.0} fps • {:.1}s • {} layers",
                                            comp.width, comp.height, comp.frame_rate, comp.duration.seconds(), comp.layers.len()
                                        ))
                                )
                        )
                );

                let mut actions = h_flex().gap_1p5().items_center();
                if !is_active {
                    actions = actions.child(
                        div()
                            .id(SharedString::from(format!("pm_switch_{}", c_id)))
                            .test_support()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .text_xs()
                            .font_semibold()
                            .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                let cid = c_id.clone();
                                s_sw.update(cx, |s, cx| {
                                    s.set_active_composition(&cid);
                                    cx.notify();
                                });
                                a_sw.update(cx, |_this, cx| cx.notify());
                            })
                            .child("Switch To"),
                    );
                }
                if comps.len() > 1 {
                    let c_id_del = comp.id.clone();
                    actions = actions.child(
                        div()
                            .id(SharedString::from(format!("pm_delete_{}", c_id_del)))
                            .test_support()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(cx.theme().muted)
                            .text_color(rgb(0xef4444))
                            .text_xs()
                            .hover(|s| s.bg(rgb(0xef4444)).text_color(rgb(0xffffff)))
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                let cid = c_id_del.clone();
                                s_del.update(cx, |s, cx| {
                                    let _ = s.delete_composition(&cid);
                                    cx.notify();
                                });
                                a_del.update(cx, |_this, cx| cx.notify());
                            })
                            .child("Delete"),
                    );
                }
                row = row.child(actions);
                comp_rows = comp_rows.child(row);
            }

            let s_new_1080 = self.state.clone();
            let s_new_4k = self.state.clone();
            let s_new_sq = self.state.clone();
            let a_new = cx.entity().clone();

            dialogs.push(
                deferred(
                    Positioner::corner(Anchor::TopLeft, dlg_pos)
                        .margin(px(8.))
                        .occlude()
                        .child(
                            div()
                                .id("project_manager_dialog")
                                .test_support()
                                .w(px(dlg_w))
                                .max_h(px(520.))
                                .overflow_y_scroll()
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded_md()
                                .shadow_lg()
                                .p_4()
                                .child(
                                    v_flex()
                                        .gap_3()
                                        .child(
                                            h_flex()
                                                .justify_between()
                                                .items_center()
                                                .child(
                                                    h_flex()
                                                        .gap_1p5()
                                                        .items_center()
                                                        .font_bold()
                                                        .text_sm()
                                                        .child(div().w(px(16.)).h(px(16.)).flex().items_center().justify_center().child(gpui_kit::assets::IconName::SlidersHorizontal))
                                                        .child("Project Settings & Manager")
                                                )
                                                .child(
                                                    div()
                                                        .id("pm_close_x")
                                                        .test_support()
                                                        .cursor_pointer()
                                                        .px_2()
                                                        .py_0p5()
                                                        .rounded_sm()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .hover(|s| s.bg(cx.theme().muted).text_color(cx.theme().foreground))
                                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                                            a_close.update(cx, |this, cx| {
                                                                this.show_project_manager = false;
                                                                cx.notify();
                                                            });
                                                        })
                                                        .child("✕")
                                                )
                                        )
                                        .child(
                                            v_flex()
                                                .gap_1p5()
                                                .p_2p5()
                                                .rounded_md()
                                                .bg(cx.theme().secondary)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .child(
                                                    h_flex()
                                                        .justify_between()
                                                        .items_center()
                                                        .child(div().text_xs().font_semibold().text_color(cx.theme().foreground).child(format!("Project: {proj_name}")))
                                                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(format!("{asset_count} Assets • {} Compositions", comps.len())))
                                                )
                                        )
                                        .child(
                                            v_flex()
                                                .gap_2()
                                                .child(
                                                    div().text_xs().font_semibold().text_color(cx.theme().foreground).child("Compositions")
                                                )
                                                .child(comp_rows)
                                                .child(
                                                    h_flex()
                                                        .gap_1p5()
                                                        .items_center()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child("+ New Comp:"))
                                                        .child(
                                                            div()
                                                                .id("pm_create_1080p")
                                                                .test_support()
                                                                .cursor_pointer()
                                                                .px_2()
                                                                .py_0p5()
                                                                .rounded_sm()
                                                                .bg(cx.theme().muted)
                                                                .hover(|s| s.bg(cx.theme().primary).text_color(cx.theme().primary_foreground))
                                                                .on_mouse_down(MouseButton::Left, {
                                                                    let a = a_new.clone();
                                                                    move |_event, _window, cx| {
                                                                        s_new_1080.update(cx, |s, cx| {
                                                                            let num = s.project.compositions.len() + 1;
                                                                            let _ = s.create_composition(&format!("Comp {num} (1080p)"), 1920, 1080, 30.0, 5.0);
                                                                            cx.notify();
                                                                        });
                                                                        a.update(cx, |_this, cx| cx.notify());
                                                                    }
                                                                })
                                                                .child("1080p 30fps")
                                                        )
                                                        .child(
                                                            div()
                                                                .id("pm_create_4k")
                                                                .test_support()
                                                                .cursor_pointer()
                                                                .px_2()
                                                                .py_0p5()
                                                                .rounded_sm()
                                                                .bg(cx.theme().muted)
                                                                .hover(|s| s.bg(cx.theme().primary).text_color(cx.theme().primary_foreground))
                                                                .on_mouse_down(MouseButton::Left, {
                                                                    let a = a_new.clone();
                                                                    move |_event, _window, cx| {
                                                                        s_new_4k.update(cx, |s, cx| {
                                                                            let num = s.project.compositions.len() + 1;
                                                                            let _ = s.create_composition(&format!("Comp {num} (4K)"), 3840, 2160, 60.0, 5.0);
                                                                            cx.notify();
                                                                        });
                                                                        a.update(cx, |_this, cx| cx.notify());
                                                                    }
                                                                })
                                                                .child("4K 60fps")
                                                        )
                                                        .child(
                                                            div()
                                                                .id("pm_create_square")
                                                                .test_support()
                                                                .cursor_pointer()
                                                                .px_2()
                                                                .py_0p5()
                                                                .rounded_sm()
                                                                .bg(cx.theme().muted)
                                                                .hover(|s| s.bg(cx.theme().primary).text_color(cx.theme().primary_foreground))
                                                                .on_mouse_down(MouseButton::Left, {
                                                                    let a = a_new.clone();
                                                                    move |_event, _window, cx| {
                                                                        s_new_sq.update(cx, |s, cx| {
                                                                            let num = s.project.compositions.len() + 1;
                                                                            let _ = s.create_composition(&format!("Comp {num} (Square)"), 1080, 1080, 30.0, 5.0);
                                                                            cx.notify();
                                                                        });
                                                                        a.update(cx, |_this, cx| cx.notify());
                                                                    }
                                                                })
                                                                .child("Square 1:1")
                                                        )
                                                )
                                        )
                                        .child({
                                            let s_res1 = self.state.clone();
                                            let s_res2 = self.state.clone();
                                            let s_fps1 = self.state.clone();
                                            let s_fps2 = self.state.clone();
                                            let s_dur1 = self.state.clone();
                                            let s_dur2 = self.state.clone();
                                            let a_upd = cx.entity().clone();

                                            let (cur_w, cur_h, cur_fps, cur_dur) = active_comp.as_ref().map(|c| (c.width, c.height, c.frame_rate, c.duration.seconds())).unwrap_or((1920, 1080, 30.0, 5.0));

                                            v_flex()
                                                .gap_2()
                                                .p_2p5()
                                                .rounded_md()
                                                .bg(cx.theme().secondary)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .child(div().text_xs().font_semibold().text_color(cx.theme().foreground).child("Active Composition Settings"))
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child("Resolution Preset"))
                                                        .child(
                                                            h_flex()
                                                                .gap_1()
                                                                .child(
                                                                    div()
                                                                        .id("pm_preset_1080p")
                                                                        .test_support()
                                                                        .cursor_pointer()
                                                                        .px_2()
                                                                        .py_0p5()
                                                                        .rounded_sm()
                                                                        .bg(if cur_w == 1920 && cur_h == 1080 { cx.theme().primary } else { cx.theme().muted })
                                                                        .text_color(if cur_w == 1920 && cur_h == 1080 { cx.theme().primary_foreground } else { cx.theme().foreground })
                                                                        .child("1920×1080")
                                                                        .on_mouse_down(MouseButton::Left, {
                                                                            let a = a_upd.clone();
                                                                            move |_event, _window, cx| {
                                                                                s_res1.update(cx, |s, cx| {
                                                                                    s.update_project_settings("", 1920, 1080, cur_fps, cur_dur);
                                                                                    cx.notify();
                                                                                });
                                                                                a.update(cx, |_this, cx| cx.notify());
                                                                            }
                                                                        })
                                                                )
                                                                .child(
                                                                    div()
                                                                        .id("pm_preset_4k")
                                                                        .test_support()
                                                                        .cursor_pointer()
                                                                        .px_2()
                                                                        .py_0p5()
                                                                        .rounded_sm()
                                                                        .bg(if cur_w == 3840 && cur_h == 2160 { cx.theme().primary } else { cx.theme().muted })
                                                                        .text_color(if cur_w == 3840 && cur_h == 2160 { cx.theme().primary_foreground } else { cx.theme().foreground })
                                                                        .child("3840×2160")
                                                                        .on_mouse_down(MouseButton::Left, {
                                                                            let a = a_upd.clone();
                                                                            move |_event, _window, cx| {
                                                                                s_res2.update(cx, |s, cx| {
                                                                                    s.update_project_settings("", 3840, 2160, cur_fps, cur_dur);
                                                                                    cx.notify();
                                                                                });
                                                                                a.update(cx, |_this, cx| cx.notify());
                                                                            }
                                                                        })
                                                                )
                                                        )
                                                )
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child("Frame Rate"))
                                                        .child(
                                                            h_flex()
                                                                .gap_1()
                                                                .child(
                                                                    div()
                                                                        .id("pm_fps_30")
                                                                        .test_support()
                                                                        .cursor_pointer()
                                                                        .px_2()
                                                                        .py_0p5()
                                                                        .rounded_sm()
                                                                        .bg(if (cur_fps - 30.0).abs() < 0.1 { cx.theme().primary } else { cx.theme().muted })
                                                                        .text_color(if (cur_fps - 30.0).abs() < 0.1 { cx.theme().primary_foreground } else { cx.theme().foreground })
                                                                        .child("30 fps")
                                                                        .on_mouse_down(MouseButton::Left, {
                                                                            let a = a_upd.clone();
                                                                            move |_event, _window, cx| {
                                                                                s_fps1.update(cx, |s, cx| {
                                                                                    s.update_project_settings("", cur_w, cur_h, 30.0, cur_dur);
                                                                                    cx.notify();
                                                                                });
                                                                                a.update(cx, |_this, cx| cx.notify());
                                                                            }
                                                                        })
                                                                )
                                                                .child(
                                                                    div()
                                                                        .id("pm_fps_60")
                                                                        .test_support()
                                                                        .cursor_pointer()
                                                                        .px_2()
                                                                        .py_0p5()
                                                                        .rounded_sm()
                                                                        .bg(if (cur_fps - 60.0).abs() < 0.1 { cx.theme().primary } else { cx.theme().muted })
                                                                        .text_color(if (cur_fps - 60.0).abs() < 0.1 { cx.theme().primary_foreground } else { cx.theme().foreground })
                                                                        .child("60 fps")
                                                                        .on_mouse_down(MouseButton::Left, {
                                                                            let a = a_upd.clone();
                                                                            move |_event, _window, cx| {
                                                                                s_fps2.update(cx, |s, cx| {
                                                                                    s.update_project_settings("", cur_w, cur_h, 60.0, cur_dur);
                                                                                    cx.notify();
                                                                                });
                                                                                a.update(cx, |_this, cx| cx.notify());
                                                                            }
                                                                        })
                                                                )
                                                        )
                                                )
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .child(div().text_color(cx.theme().muted_foreground).child("Duration"))
                                                        .child(
                                                            h_flex()
                                                                .gap_1()
                                                                .child(
                                                                    div()
                                                                        .id("pm_dur_5s")
                                                                        .test_support()
                                                                        .cursor_pointer()
                                                                        .px_2()
                                                                        .py_0p5()
                                                                        .rounded_sm()
                                                                        .bg(if (cur_dur - 5.0).abs() < 0.1 { cx.theme().primary } else { cx.theme().muted })
                                                                        .text_color(if (cur_dur - 5.0).abs() < 0.1 { cx.theme().primary_foreground } else { cx.theme().foreground })
                                                                        .child("5.0s")
                                                                        .on_mouse_down(MouseButton::Left, {
                                                                            let a = a_upd.clone();
                                                                            move |_event, _window, cx| {
                                                                                s_dur1.update(cx, |s, cx| {
                                                                                    s.update_project_settings("", cur_w, cur_h, cur_fps, 5.0);
                                                                                    cx.notify();
                                                                                });
                                                                                a.update(cx, |_this, cx| cx.notify());
                                                                            }
                                                                        })
                                                                )
                                                                .child(
                                                                    div()
                                                                        .id("pm_dur_10s")
                                                                        .test_support()
                                                                        .cursor_pointer()
                                                                        .px_2()
                                                                        .py_0p5()
                                                                        .rounded_sm()
                                                                        .bg(if (cur_dur - 10.0).abs() < 0.1 { cx.theme().primary } else { cx.theme().muted })
                                                                        .text_color(if (cur_dur - 10.0).abs() < 0.1 { cx.theme().primary_foreground } else { cx.theme().foreground })
                                                                        .child("10.0s")
                                                                        .on_mouse_down(MouseButton::Left, {
                                                                            let a = a_upd.clone();
                                                                            move |_event, _window, cx| {
                                                                                s_dur2.update(cx, |s, cx| {
                                                                                    s.update_project_settings("", cur_w, cur_h, cur_fps, 10.0);
                                                                                    cx.notify();
                                                                                });
                                                                                a.update(cx, |_this, cx| cx.notify());
                                                                            }
                                                                        })
                                                                )
                                                        )
                                                )
                                        })
                                        .child(
                                            h_flex()
                                                .justify_end()
                                                .child(
                                                    div()
                                                        .id("pm_done_button")
                                                        .test_support()
                                                        .cursor_pointer()
                                                        .px_4()
                                                        .py_1p5()
                                                        .rounded_sm()
                                                        .bg(cx.theme().primary)
                                                        .text_color(cx.theme().primary_foreground)
                                                        .text_xs()
                                                        .font_semibold()
                                                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                                        .on_mouse_down(MouseButton::Left, {
                                                            let a = cx.entity().clone();
                                                            move |_event, _window, cx| {
                                                                a.update(cx, |this, cx| {
                                                                    this.show_project_manager = false;
                                                                    cx.notify();
                                                                });
                                                            }
                                                        })
                                                        .child("Done")
                                                )
                                        )
                                )
                        )
                )
                .into_any_element(),
            );
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
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    // Clicking the workspace dismisses open menus.
                    close_key.update(cx, |this, cx| {
                        if this.open_menu.take().is_some() {
                            cx.notify();
                        }
                    });
                })
                .into_any_element()
        } else {
            div()
                .flex_1()
                .size_full()
                .child(self.dock_area.clone())
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    // Clicking the workspace dismisses open menus.
                    close_key.update(cx, |this, cx| {
                        if this.open_menu.take().is_some() {
                            cx.notify();
                        }
                    });
                })
                .into_any_element()
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
            .on_action({
                let state = self.state.clone();
                move |_: &SelectMoveTool, _window, cx| {
                    state.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Move);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state = self.state.clone();
                move |_: &SelectHandTool, _window, cx| {
                    state.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Hand);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state = self.state.clone();
                move |_: &SelectRotateTool, _window, cx| {
                    state.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Rotate);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state = self.state.clone();
                move |_: &SelectPenTool, _window, cx| {
                    state.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Pen);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state = self.state.clone();
                move |_: &SelectTextTool, _window, cx| {
                    state.update(cx, |s, cx| {
                        s.set_tool(state::EditorTool::Text);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state = self.state.clone();
                move |_: &CycleShapeTool, _window, cx| {
                    state.update(cx, |s, cx| {
                        s.cycle_shape_tool();
                        cx.notify();
                    });
                }
            })
            .on_action({
                let panels = self.panels.clone();
                let state = self.state.clone();
                move |_: &FitGraphView, _window, cx| {
                    if !state.read(cx).spline_editor_open {
                        return;
                    }
                    panels.timeline.update(cx, |this, cx| {
                        this.reset_graph_view();
                        cx.notify();
                    });
                }
            })
            .on_key_down(move |event, _window, cx| {
                let mods = event.keystroke.modifiers;
                let ctrl = mods.control || mods.platform;
                let key = event.keystroke.key.to_lowercase();
                // Global menu shortcuts (modifier-guarded so typing is safe).
                if ctrl && key == "z" && !mods.shift {
                    state_key.update(cx, |s, cx| {
                        s.undo();
                        cx.notify();
                    });
                    return;
                } else if (ctrl && key == "y") || (ctrl && key == "z" && mods.shift) {
                    state_key.update(cx, |s, cx| {
                        s.redo();
                        cx.notify();
                    });
                    return;
                } else if ctrl && key == "s" {
                    let (a, s) = (app_key.clone(), state_key.clone());
                    request_save_project(&a, &s, cx);
                    return;
                } else if ctrl && key == "o" {
                    let (a, s) = (app_key.clone(), state_key.clone());
                    request_open_project(&a, &s, cx);
                    return;
                } else if ctrl && key == "d" {
                    state_key.update(cx, |s, cx| {
                        if let Some(id) = s.selected_layer_id.clone() {
                            let _ = s.duplicate_layer(&id);
                        }
                        cx.notify();
                    });
                    return;
                } else if key == "escape" {
                    app_key.update(cx, |this, cx| {
                        this.open_menu = None;
                        this.show_about = false;
                        this.show_new_project = false;
                        this.show_project_manager = false;
                        cx.notify();
                    });
                    state_key.update(cx, |s, cx| {
                        // Esc also exits mask node-edit mode.
                        s.set_active_mask_edit(None);
                        cx.notify();
                    });
                    return;
                }
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
                } else if key == "home" {
                    state_key.update(cx, |s, cx| {
                        s.jump_to_start();
                        cx.notify();
                    });
                } else if key == "end" {
                    state_key.update(cx, |s, cx| {
                        s.jump_to_end();
                        cx.notify();
                    });
                } else if key == "left" || key == "arrowleft" {
                    state_key.update(cx, |s, cx| {
                        s.step_backward();
                        cx.notify();
                    });
                } else if key == "right" || key == "arrowright" {
                    state_key.update(cx, |s, cx| {
                        s.step_forward();
                        cx.notify();
                    });
                } else if key == "[" {
                    state_key.update(cx, |s, cx| {
                        let _ = s.trim_selected_layer_in_to_playhead();
                        cx.notify();
                    });
                } else if key == "f" {
                    // Fit View in the Graph Editor (no-op elsewhere).
                    if state_key.read(cx).spline_editor_open {
                        let panels = app_key.read(cx).panels().clone();
                        panels.timeline.update(cx, |this, cx| {
                            this.reset_graph_view();
                            cx.notify();
                        });
                    }
                } else if key == "]" {
                    state_key.update(cx, |s, cx| {
                        let _ = s.trim_selected_layer_out_to_playhead();
                        cx.notify();
                    });
                } else if !ctrl && !mods.alt && state_key.read(cx).value_editor.is_none() {
                    if key == "v" {
                        state_key.update(cx, |s, cx| {
                            s.select_tool(EditorTool::Move);
                            cx.notify();
                        });
                    } else if key == "g" {
                        state_key.update(cx, |s, cx| {
                            s.select_tool(EditorTool::Pen);
                            cx.notify();
                        });
                    } else if key == "q" {
                        state_key.update(cx, |s, cx| {
                            s.cycle_shape_tool();
                            cx.notify();
                        });
                    } else if key == "m" {
                        let panels = app_key.read(cx).panels().clone();
                        state_key.update(cx, |s, cx| {
                            let (rev, _is_all) = s.handle_m_shortcut();
                            if let Some(lid) = s.selected_layer_id.clone() {
                                panels.timeline.update(cx, |tl, cx| {
                                    tl.set_layer_expanded(&lid, rev);
                                    cx.notify();
                                });
                            }
                            cx.notify();
                        });
                    }
                }
            })
            .child(menubar)
            .child(toolbar)
            .child(main_workspace)
            .children(dialogs)
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
    use crate::state::EditorState;
    use project::Vec2;
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
        let handle = cx.open_window(size(px(1280.), px(1000.)), |window, cx| {
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
            // With 12 categories the list scrolls: assert top headers
            // first, then scroll for the lower ones.
            assert!(window.find("effects_panel").visible());
            assert!(window.find("effects_categories").visible());
            assert!(window.find("effect_category_blur").visible());
            assert!(window.find("effect_category_keying").visible());
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
            assert!(window.find("effect_item_blur").visible());
            assert!(window.find("effect_item_sharpen").visible());
            // Lower entries need scrolling (expanded categories make the
            // list taller than the dock): step down until each is painted.
            for _ in 0..6 {
                window.scroll(
                    "effects_categories",
                    gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-400.))),
                    cx,
                );
                window.render_frame(cx);
                if window
                    .try_find("effect_item_chroma_key")
                    .map(|e| e.visible())
                    .unwrap_or(false)
                {
                    break;
                }
            }
            assert!(window.find("effect_item_brightness_contrast").visible());
            assert!(window.find("effect_item_levels").visible());
            assert!(window.find("effect_item_chroma_key").visible());
            assert!(window.find("effect_item_luma_key").visible());
            for _ in 0..8 {
                window.scroll(
                    "effects_categories",
                    gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-400.))),
                    cx,
                );
                window.render_frame(cx);
                if window
                    .try_find("effect_category_text")
                    .map(|e| e.visible())
                    .unwrap_or(false)
                {
                    break;
                }
            }
            assert!(window.find("effect_category_text").visible());
            assert!(window.find("effect_item_text_outline").visible());

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

    #[gpui_kit::test]
    fn test_viewport_pick_topmost_layer(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        // Accent box sits inside the full-canvas background: picking its
        // interior must return the accent, not the covered background.
        // Frame 0 accent world x-range is [-350, -50], y-range [-150, 150].
        let (boxes, picked) = app_view.read_with(cx, |view, cx| {
            let panels = view.panels().clone();
            panels.viewer.read_with(cx, |panel, _| {
                (panel.pick_boxes.clone(), panel.pick_top_at(-200.0, 0.0))
            })
        });
        assert!(!boxes.is_empty());
        assert_eq!(boxes[0].id, "layer_accent", "pick order must be topmost-first");
        assert_eq!(picked.as_deref(), Some("layer_accent"));
        // Far corner is background-only.
        let picked_bg = app_view.read_with(cx, |view, cx| {
            let panels = view.panels().clone();
            panels.viewer.read_with(cx, |panel, _| panel.pick_top_at(-900.0, -500.0))
        });
        assert_eq!(picked_bg.as_deref(), Some("layer_bg"));
    }

    #[test]
    fn test_graph_key_drag_moves_time_and_value() {
        use crate::state::EditorState;
        use project::KeyframeInterpolation;

        let mut state = EditorState::new();
        // Accent position.x: bezier keys at 0s/2s/4s, y stays 0.
        let before: Vec<(f64, f32)> = state
            .active_composition()
            .unwrap()
            .get_layer("layer_accent")
            .unwrap()
            .transform
            .position
            .keyframes()
            .iter()
            .map(|k| (k.time.seconds(), k.value.x))
            .collect();
        assert_eq!(before.len(), 3);
        // Drag first key: 0s/-200px -> 0.5s/-150px (no checkpoint version).
        assert!(state.move_graph_keyframe_live("layer_accent", "transform.position.x", 0.0, 0.5, -150.0));
        let keys = &state
            .active_composition()
            .unwrap()
            .get_layer("layer_accent")
            .unwrap()
            .transform
            .position
            .keyframes();
        assert_eq!(keys.len(), 3, "move must not add/remove keys");
        let moved = keys.iter().find(|k| (k.time.seconds() - 0.5).abs() < 1e-6).unwrap();
        assert!((moved.value.x + 150.0).abs() < 1e-3, "value follows the drag");
        assert!(moved.value.y.abs() < 1e-6, "other axis preserved");
        assert_eq!(moved.interpolation, KeyframeInterpolation::Bezier, "interp preserved");
        // Scalar path moves too (rotation 0°@0s -> 1s).
        assert!(state.move_graph_keyframe_live("layer_accent", "transform.rotation", 0.0, 1.0, 45.0));
        let rot = &state
            .active_composition()
            .unwrap()
            .get_layer("layer_accent")
            .unwrap()
            .transform
            .rotation
            .keyframes();
        assert!(rot.iter().any(|k| (k.time.seconds() - 1.0).abs() < 1e-6 && (k.value - 45.0).abs() < 1e-3));
        // Unknown key misses cleanly.
        assert!(!state.move_graph_keyframe_live("layer_accent", "transform.position.x", 99.0, 99.5, 0.0));
    }

    #[gpui_kit::test]
    fn test_graph_key_click_drag_moves(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;
        use gpui_kit::point;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        // Accent position.x has keys at 0s/2s/4s: open Graph on it.
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_accent".to_string()));
                if !s.spline_editor_open {
                    s.toggle_spline_editor();
                }
                cx.notify();
            });
        });
        let before: (f64, f32, usize) = app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let kfs = &s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .transform
                .position
                .keyframes();
            (kfs[1].time.seconds(), kfs[1].value.x, kfs.len())
        });
        // Real pointer drag on the middle diamond (deterministic id).
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let snap = window.find("graph_key_layer_accent_transform_position_x_2000");
            assert!(snap.visible());
            let from = snap.bounds().center();
            window.drag(from, from + point(px(60.0), px(30.0)), cx);
        })
        .expect("update_window failed");
        let after: (f64, f32, usize) = app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let kfs = &s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .transform
                .position
                .keyframes();
            // Keys stay sorted by time; find the moved one by value shift.
            let moved = kfs.iter().min_by(|a, b| {
                (a.time.seconds() - before.0)
                    .abs()
                    .partial_cmp(&(b.time.seconds() - before.0).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            }).unwrap();
            (moved.time.seconds(), moved.value.x, kfs.len())
        });
        assert_eq!(after.2, before.2, "drag must not add/remove keys");
        assert!(after.0 > before.0, "rightward drag moves the key later in time");
        assert!(after.1 < before.1, "downward drag lowers the value, y-axis up");
    }

    #[test]
    fn test_widget_state_commits() {
        use crate::state::EditorState;
        use project::{EffectType, ShaderParam, ShaderParamType, ShaderParamValue};
        use std::collections::HashMap;

        let mut state = EditorState::new();
        state.select_layer(Some("layer_accent".to_string()));
        // ShaderLab with a Vec2 + an Enum param (constructed directly).
        let fx = state
            .add_effect_to_selected_layer(EffectType::ShaderLab {
                source: String::new(),
                params: vec![
                    ShaderParam {
                        name: "offset".to_string(),
                        label: "Offset".to_string(),
                        param_type: ShaderParamType::Vec2,
                        default: ShaderParamValue::Vec2([1.0, 2.0]),
                        min: None,
                        max: None,
                        step: None,
                        group: None,
                    },
                    ShaderParam {
                        name: "mode".to_string(),
                        label: "Mode".to_string(),
                        param_type: ShaderParamType::Enum {
                            options: vec!["Soft".to_string(), "Hard".to_string()],
                        },
                        default: ShaderParamValue::Int(0),
                        min: None,
                        max: None,
                        step: None,
                        group: None,
                    },
                ],
                values: HashMap::new(),
                compile_error: None,
            })
            .unwrap();
        // Linked nudge moves every component together (step 0.05).
        state.nudge_shaderlab_linked(&fx, "offset", 1.0).unwrap();
        // Linked typed entry sets every component at once.
        state.set_shaderlab_all_components(&fx, "offset", 5.0).unwrap();
        {
            let comp = state.active_composition().unwrap();
            let eff = comp.get_layer("layer_accent").unwrap().get_effect(&fx).unwrap();
            let resolved: HashMap<_, _> = eff.resolved_shader_values().into_iter().collect();
            assert_eq!(resolved["offset"], ShaderParamValue::Vec2([5.0, 5.0]));
        }
        // Enum declaration + commit by index.
        {
            let comp = state.active_composition().unwrap();
            let eff = comp.get_layer("layer_accent").unwrap().get_effect(&fx).unwrap();
            let p = eff.shader_params().unwrap().iter().find(|p| p.name == "mode").unwrap();
            assert_eq!(p.param_type.widget_kind(), project::WidgetKind::Dropdown);
        }
        state.set_shaderlab_param(&fx, "mode", 1.0).unwrap();
        {
            let comp = state.active_composition().unwrap();
            let eff = comp.get_layer("layer_accent").unwrap().get_effect(&fx).unwrap();
            let resolved: HashMap<_, _> = eff.resolved_shader_values().into_iter().collect();
            assert_eq!(resolved["mode"], ShaderParamValue::Int(1));
        }
        // Gradient pair commit is a single call (reverse + presets).
        let ramp = state
            .add_effect_to_selected_layer(EffectType::gradient_ramp(
                project::Color::BLACK,
                project::Color::WHITE,
                90.0,
            ))
            .unwrap();
        state
            .set_effect_color_pair(
                "layer_accent",
                &ramp,
                &[("color_a", project::Color::RED), ("color_b", project::Color::BLUE)],
            )
            .unwrap();
        {
            let comp = state.active_composition().unwrap();
            let eff = comp.get_layer("layer_accent").unwrap().get_effect(&ramp).unwrap();
            assert_eq!(eff.stock_color("color_a"), None);
            match &eff.effect_type {
                EffectType::GradientRamp { color_a, color_b, .. } => {
                    assert_eq!(*color_a, project::Color::RED);
                    assert_eq!(*color_b, project::Color::BLUE);
                }
                other => panic!("expected ramp, got {other:?}"),
            }
        }
        // Checkbox commit path.
        let noise = state
            .add_effect_to_selected_layer(EffectType::noise_generator(50.0, false))
            .unwrap();
        state.toggle_noise_monochrome(&noise).unwrap();
        assert!(matches!(
            &state
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .get_effect(&noise)
                .unwrap()
                .effect_type,
            EffectType::NoiseGenerator { monochrome: true, .. }
        ));
    }

    #[gpui_kit::test]
    fn test_graph_tangent_drag_reshapes(cx: &mut TestAppContext) {
        use gpui_kit::point;
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(900.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_accent".to_string()));
                if !s.spline_editor_open {
                    s.toggle_spline_editor();
                }
                cx.notify();
            });
        });
        let before: (f32, f32) = app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let kfs = &s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .transform
                .position
                .keyframes();
            let k = kfs.iter().find(|k| (k.time.seconds() - 2.0).abs() < 1e-6).unwrap();
            let out = k.out_tangent.unwrap();
            (out.x, out.y)
        });
        // Drag the out-handle right: time influence must grow.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let snap = window.find("graph_tan_layer_accent_transform_position_x_2000_out");
            assert!(snap.visible());
            let from = snap.bounds().center();
            window.drag(from, from + point(px(40.0), px(0.0)), cx);
        })
        .expect("update_window failed");
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let kfs = &s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .transform
                .position
                .keyframes();
            assert_eq!(kfs.len(), 3, "handle drag must not add/remove keys");
            let k = kfs.iter().find(|k| (k.time.seconds() - 2.0).abs() < 1e-6).unwrap();
            let out = k.out_tangent.unwrap();
            assert!(out.x > before.0, "rightward handle drag grows influence");
            assert_eq!(k.interpolation, project::KeyframeInterpolation::Bezier);
            // Key time/value untouched by a pure tangent drag.
            assert!((k.value.x - 200.0).abs() < 1e-3);
        });
    }

    #[gpui_kit::test]
    fn test_kit_components_drive_commands(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(900.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        // Keyboard action selects the Pen tool (same command as clicking).
        cx.update_window(handle.into(), |_, window, cx| {
            let focus_handle = app_view.read(cx).focus_handle().clone();
            window.focus(&focus_handle, cx);
            window.render_frame(cx);
            window.press("g", cx);
        })
        .expect("update_window failed");
        assert_eq!(
            app_view.read_with(cx, |view, cx| view.state().read(cx).active_tool),
            crate::state::EditorTool::Pen
        );
        // Side-strip Text button (kit Button) selects Text.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("side_tool_btn_text").visible());
            window.click("side_tool_btn_text", cx);
        })
        .expect("update_window failed");
        assert_eq!(
            app_view.read_with(cx, |view, cx| view.state().read(cx).active_tool),
            crate::state::EditorTool::Text
        );
        // Kit Checkbox toggles the boolean param.
        let noise = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_effect_to_selected_layer(project::EffectType::noise_generator(50.0, false))
                    .unwrap()
            })
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let id = SharedString::from(format!("fx_bool_monochrome_{noise}"));
            assert!(window.find(id.clone()).visible());
            window.click(id, cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let layer = comp.get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&noise).unwrap();
            matches!(
                &eff.effect_type,
                project::EffectType::NoiseGenerator { monochrome: true, .. }
            )
        }));
    }

    #[gpui_kit::test]
    fn test_graph_editor_ae_chrome(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(900.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        // Accent has animated position: open the Graph view on it.
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_accent".to_string()));
                if !s.spline_editor_open {
                    s.toggle_spline_editor();
                }
                cx.notify();
            });
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // AE header chrome.
            assert!(window.find("graph_tab_value").visible());
            assert!(window.find("graph_tab_speed").visible());
            assert!(window.find("graph_fit_view").visible());
            assert!(window.find("graph_fit_sel").visible());
            assert!(window.find("graph_readout").visible());
            assert!(window.find("graph_toggle_grid").visible());
            assert!(window.find("graph_toggle_keys").visible());
            assert!(window.find("graph_isolate_toggle").visible());
            // Legend + ruler + key diamonds + tangent handles.
            assert!(window.find("graph_legend").visible());
            assert!(window.find("graph_ruler").visible());
            assert!(window
                .find("graph_key_layer_accent_transform_position_x_2000")
                .visible());
            assert!(window
                .find("graph_tan_layer_accent_transform_position_x_2000_in")
                .visible());
            assert!(window
                .find("graph_tan_layer_accent_transform_position_x_2000_out")
                .visible());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_widget_dropdown_open_pick(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;
        use project::{EffectType, ShaderParam, ShaderParamType, ShaderParamValue};
        use std::collections::HashMap;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(900.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        let eff = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_effect_to_selected_layer(EffectType::ShaderLab {
                    source: String::new(),
                    params: vec![
                        ShaderParam {
                            name: "offset".to_string(),
                            label: "Offset".to_string(),
                            param_type: ShaderParamType::Vec2,
                            default: ShaderParamValue::Vec2([1.0, 2.0]),
                            min: None,
                            max: None,
                            step: None,
                            group: None,
                        },
                        ShaderParam {
                            name: "mode".to_string(),
                            label: "Mode".to_string(),
                            param_type: ShaderParamType::Enum {
                                options: vec!["Soft".to_string(), "Hard".to_string(), "Glow".to_string()],
                            },
                            default: ShaderParamValue::Int(0),
                            min: None,
                            max: None,
                            step: None,
                            group: None,
                        },
                    ],
                    values: HashMap::new(),
                    compile_error: None,
                })
                .unwrap()
            })
        });
        // Collapse the other inspector cards so the effect rows sit
        // on-screen (off-viewport elements can't take test clicks).
        app_view.update(cx, |view, cx| {
            view.panels().properties.update(cx, |this, cx| {
                this.source_expanded = false;
                this.transform_expanded = false;
                this.switches_expanded = false;
                this.tools_expanded = false;
                cx.notify();
            });
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Vec widget: link pill + per-component rows.
            assert!(window.find(SharedString::from(format!("vec_link_{eff}_offset"))).visible());
            assert!(window.find(SharedString::from(format!("shader_param_{eff}_offset_0"))).visible());
            // Kit Select: click the trigger, arrow to Glow, confirm.
            let trigger = SharedString::from(format!("shader_enum_{eff}_mode"));
            assert!(window.find(trigger.clone()).visible());
            window.click(trigger, cx);
            window.press("down", cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("update_window failed");
        // Pick committed Glow (index 2).
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let eff_ref = s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .get_effect(&eff)
                .unwrap();
            let resolved: HashMap<_, _> = eff_ref.resolved_shader_values().into_iter().collect();
            assert_eq!(resolved["mode"], ShaderParamValue::Int(2));
        });
    }

    #[gpui_kit::test]
    fn test_widget_gradient_and_checkbox(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(900.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        let (ramp, noise) = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                let ramp = s
                    .add_effect_to_selected_layer(project::EffectType::gradient_ramp(
                        project::Color::BLACK,
                        project::Color::WHITE,
                        90.0,
                    ))
                    .unwrap();
                let noise = s
                    .add_effect_to_selected_layer(project::EffectType::noise_generator(50.0, false))
                    .unwrap();
                (ramp, noise)
            })
        });
        // Collapse the other inspector cards so the effect rows sit
        // on-screen (off-viewport elements can't take test clicks).
        app_view.update(cx, |view, cx| {
            view.panels().properties.update(cx, |this, cx| {
                this.source_expanded = false;
                this.transform_expanded = false;
                this.switches_expanded = false;
                this.tools_expanded = false;
                cx.notify();
            });
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Gradient editor: stops, reverse, selected-stop wheel.
            assert!(window.find(SharedString::from(format!("gradient_stop0_{ramp}"))).visible());
            assert!(window.find(SharedString::from(format!("gradient_stop1_{ramp}"))).visible());
            assert!(window.find(SharedString::from(format!("gradient_reverse_{ramp}"))).visible());
            assert!(window.find(SharedString::from(format!("fx_wheel_btn_color_a_{ramp}"))).visible());
            // Checkbox for the boolean.
            assert!(window.find(SharedString::from(format!("fx_bool_monochrome_{noise}"))).visible());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_ae_chrome_visible(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_accent".to_string()));
                cx.notify();
            });
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // AE top bar.
            assert!(window.find("active_tool_pill").visible());
            assert!(window.find("snapping_toggle").visible());
            assert!(window.find("toggle_timeline_full_width_button").visible());
            // Viewer strip + transport.
            assert!(window.find("viewer_tool_strip").visible());
            assert!(window.find("timecode_display").visible());
            assert!(window.find("transport_play").visible());
        })
        .expect("update_window failed");
        // Snapping toggle persists through state (toolbar wiring).
        assert!(app_view.read_with(cx, |view, cx| view.state().read(cx).snapping));
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.toggle_snapping();
                cx.notify();
            });
        });
        assert!(!app_view.read_with(cx, |view, cx| view.state().read(cx).snapping));
    }

    #[gpui_kit::test]
    fn test_pen_curve_overlay_visible(cx: &mut TestAppContext) {
        use project::Vec2;
        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(1000.)), |window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        // Draw a two-point pen path (selects the new path layer).
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                let _ = s.pen_press_at(Vec2::new(-100.0, -50.0), None);
                cx.notify();
            });
        });
        app_view.update(cx, |view, cx| {
            let sel = view.state().read(cx).selected_layer_id.clone().unwrap();
            view.state().update(cx, |s, cx| {
                let _ = s.pen_press_at(Vec2::new(100.0, 60.0), Some(sel));
                cx.notify();
            });
        });
        cx.run_until_parked();
        // The spline overlay must be painted for the selected path layer.
        let visible = cx.update_window(handle.into(), |_, window, _| {
            window.try_find("pen_curve_overlay").map(|e| e.visible())
        }).expect("window update");
        assert_eq!(visible, Some(true));
    }

    #[gpui_kit::test]
    fn test_effect_color_wheels_present(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(1000.)), |window, cx| {
            window.activate_window();
            window.set_window_title("Motion Compositor");
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        // Checkerboard carries two custom colors; both rows need wheels.
        let eff_id = app_view.update(cx, |view, cx| {
            let mut out = String::new();
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_accent".to_string()));
                out = s
                    .add_effect_to_selected_layer(project::EffectType::checkerboard(
                        32.0,
                        project::Color::BLACK,
                        project::Color::WHITE,
                    ))
                    .unwrap();
                cx.notify();
            });
            out
        });
        cx.run_until_parked();
        for field in ["color_a", "color_b"] {
            let id = format!("fx_wheel_btn_{field}_{eff_id}");
            let visible = cx.update_window(handle.into(), |_, window, _| {
                window.try_find(id.clone()).map(|e| e.visible())
            }).expect("window update");
            assert_eq!(visible, Some(true), "{field}");
        }
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

        // Parenting: child cleanly inherits the parent's transform changes
        assert!(state.set_layer_parent(layer_id, Some("layer_bg".to_string())));
        assert_eq!(state.active_composition().unwrap().get_layer(layer_id).unwrap().parent_id.as_deref(), Some("layer_bg"));
        let parent_world = state.layer_world_matrix_fast("layer_bg").unwrap();
        let child_world = state.layer_world_matrix_fast(layer_id).unwrap();
        let (anchor, pos, scale, rot) = state.active_composition().unwrap().get_layer(layer_id).unwrap().transform.evaluate_at(&state.clock.timecode());
        let child_local = compositor::AffineTransform2D::from_transform_components(pos, scale, rot, anchor);
        let expected_child_world = parent_world * child_local;
        for (a, b) in [expected_child_world.a, expected_child_world.b, expected_child_world.c, expected_child_world.d, expected_child_world.tx, expected_child_world.ty]
            .iter()
            .zip([child_world.a, child_world.b, child_world.c, child_world.d, child_world.tx, child_world.ty].iter())
        {
            assert!((a - b).abs() < 1e-3, "{expected_child_world:?} vs {child_world:?}");
        }
        // Any layer can be a parent; cycle rejection
        assert!(!state.set_layer_parent("layer_bg", Some(layer_id.to_string())), "child-as-parent must cycle-reject");
        assert!(!state.set_layer_parent(layer_id, Some(layer_id.to_string())), "self-parent must reject");
        assert!(!state.set_layer_parent(layer_id, Some("nope".to_string())), "missing parent must reject");
        assert!(state.set_layer_parent(layer_id, None));
        assert_eq!(state.active_composition().unwrap().get_layer(layer_id).unwrap().parent_id, None);

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
    fn test_parented_pen_path_stays_rendered() {
        use crate::state::EditorState;
        use project::Vec2;

        let mut state = EditorState::new();
        // Pen path in arbitrary comp coords (negative + off-origin).
        let pid = state.pen_press_at(Vec2::new(-100.0, -50.0), None).unwrap();
        let sel = state.selected_layer_id.clone().unwrap();
        state.pen_press_at(Vec2::new(100.0, 60.0), Some(sel)).unwrap();
        // Parent to the background: layer inherits parent's transform and stays in the render list
        assert!(state.set_layer_parent(&pid, Some("layer_bg".to_string())));
        let parent_world = state.layer_world_matrix_fast("layer_bg").unwrap();
        let child_world = state.layer_world_matrix_fast(&pid).unwrap();
        let (anchor, pos, scale, rot) = state.active_composition().unwrap().get_layer(&pid).unwrap().transform.evaluate_at(&state.clock.timecode());
        let child_local = compositor::AffineTransform2D::from_transform_components(pos, scale, rot, anchor);
        let expected_child_world = parent_world * child_local;
        for (a, b) in [expected_child_world.a, expected_child_world.b, expected_child_world.c, expected_child_world.d, expected_child_world.tx, expected_child_world.ty]
            .iter()
            .zip([child_world.a, child_world.b, child_world.c, child_world.d, child_world.tx, child_world.ty].iter())
        {
            assert!((a - b).abs() < 1e-3, "{expected_child_world:?} vs {child_world:?}");
        }
        let eval = state.evaluate_current_frame().unwrap();
        let layer = eval.get_layer(&pid).expect("parented pen layer evaluated");
        assert!(eval.render_list.contains(&pid), "parented layer must stay rendered");
        let (origin, size) = project::Path::from_svg(match &layer.source {
            project::LayerSource::Shape { shape_type: project::ShapeType::Path { path_data, .. } } => path_data,
            _ => panic!("pen layer must be a path shape"),
        })
        .frame(8.0)
        .unwrap();
        let bbox = layer.local_to_world_bbox(&compositor::BoundingBox2D::from_origin_size(origin, size));
        assert!(bbox.width() > 200.0 && bbox.height() > 100.0, "{bbox:?}");
    }

    #[test]
    fn test_mask_shape_numeric_and_full_size() {
        use crate::state::EditorState;
        use project::MaskShapeKind;

        let mut state = EditorState::new();
        let mid = state.add_mask_to_layer("layer_accent").unwrap();
        // Numeric rectangle.
        state.set_mask_shape_numeric("layer_accent", &mid, MaskShapeKind::Rectangle, 10.0, 20.0, 100.0, 50.0).unwrap();
        let path = state.active_composition().unwrap().get_layer("layer_accent").unwrap().get_mask(&mid).unwrap().path.value.clone();
        let (mn, mx) = path.bounds().unwrap();
        assert!((mn.x - 10.0).abs() < 1e-3 && (mn.y - 20.0).abs() < 1e-3);
        assert!((mx.x - 110.0).abs() < 1e-3 && (mx.y - 70.0).abs() < 1e-3);
        // Numeric ellipse in the same box.
        state.set_mask_shape_numeric("layer_accent", &mid, MaskShapeKind::Ellipse, 10.0, 20.0, 100.0, 50.0).unwrap();
        let path = state.active_composition().unwrap().get_layer("layer_accent").unwrap().get_mask(&mid).unwrap().path.value.clone();
        let (mn, mx) = path.bounds().unwrap();
        assert!((mn.x - 10.0).abs() < 1.0 && (mx.x - 110.0).abs() < 1.0);
        assert!((mn.y - 20.0).abs() < 1.0 && (mx.y - 70.0).abs() < 1.0);
        // Full-layer mask on the 300x300 accent solid.
        let full = state.add_layer_sized_mask("layer_accent").unwrap();
        let path = state.active_composition().unwrap().get_layer("layer_accent").unwrap().get_mask(&full).unwrap().path.value.clone();
        let (mn, mx) = path.bounds().unwrap();
        assert!((mx.x - mn.x - 300.0).abs() < 1e-3 && (mx.y - mn.y - 300.0).abs() < 1e-3);
    }

    #[test]
    fn test_mask_rename_and_lock() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        let mid = state.add_mask_to_layer("layer_accent").unwrap();
        state.rename_mask("layer_accent", &mid, "Hero Window").unwrap();
        assert_eq!(state.active_composition().unwrap().get_layer("layer_accent").unwrap().get_mask(&mid).unwrap().name, "Hero Window");
        state.rename_mask("layer_accent", &mid, "   ").unwrap();
        assert_eq!(state.active_composition().unwrap().get_layer("layer_accent").unwrap().get_mask(&mid).unwrap().name, "Mask");
        // Locked masks reject geometry edits but allow enable toggles.
        state.set_mask_locked("layer_accent", &mid, true).unwrap();
        assert!(state.move_mask_point_live("layer_accent", &mid, 0, project::Vec2::new(1.0, 1.0)).is_err());
        assert!(state.append_mask_point("layer_accent", &mid, project::Vec2::new(1.0, 1.0)).is_err());
        assert!(state.cycle_mask_mode("layer_accent", &mid).is_err());
        assert!(state.toggle_mask_enabled("layer_accent", &mid).is_ok());
        state.set_mask_locked("layer_accent", &mid, false).unwrap();
        assert!(state.cycle_mask_mode("layer_accent", &mid).is_ok());
    }

    #[test]
    fn test_shape_and_motion_path_to_mask() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        // Shape path (200x100 rect) becomes a mask path with equal bounds.
        let rect = state.add_rectangle_shape_layer(200.0, 100.0, None).unwrap();
        let mid = state.shape_path_to_mask(&rect, &rect).unwrap();
        let path = state.active_composition().unwrap().get_layer(&rect).unwrap().get_mask(&mid).unwrap().path.value.clone();
        let (mn, mx) = path.bounds().unwrap();
        assert!((mx.x - mn.x - 200.0).abs() < 1e-3 && (mx.y - mn.y - 100.0).abs() < 1e-3);
        assert!(!state.shape_path_to_mask("layer_bg", &rect).is_ok());
        // Accent position keys (3 bezier keys) become motion-mask keys.
        let mmid = state.motion_path_to_mask("layer_accent", "layer_accent").unwrap();
        let mask = state.active_composition().unwrap().get_layer("layer_accent").unwrap().get_mask(&mmid).unwrap().clone();
        assert!(mask.path.is_animated());
        assert_eq!(mask.path.keyframes().len(), 3);
        assert_eq!(mask.path.value.points.len(), 3);
    }

    #[test]
    fn test_auto_trace_solid_and_text_to_masks() {
        use crate::state::EditorState;
        use project::{AutoTraceOptions, TraceRange};

        let mut state = EditorState::new();
        // Solid accent (full-bleed alpha) traces to exactly one island.
        let ids = state.auto_trace_masks("layer_accent", &AutoTraceOptions::default()).unwrap();
        assert_eq!(ids.len(), 1);
        let mask = state.active_composition().unwrap().get_layer("layer_accent").unwrap().get_mask(&ids[0]).unwrap().clone();
        assert_eq!(mask.mode, project::MaskMode::Add);
        assert!(mask.path.is_animated());
        // Work-area range also works (capped internally).
        let wa = AutoTraceOptions {
            range: TraceRange::WorkArea,
            ..Default::default()
        };
        assert!(!state.auto_trace_masks("layer_accent", &wa).unwrap().is_empty());
        // Text layer becomes a masked solid; source hides.
        let text = state.add_text_layer("Hi", None).unwrap();
        let (solid, tmasks) = state.create_masks_from_text(&text).unwrap();
        assert!(!tmasks.is_empty());
        let comp = state.active_composition().unwrap();
        assert!(!comp.get_layer(&text).unwrap().visible);
        assert!(comp.get_layer(&solid).unwrap().get_mask(&tmasks[0]).is_some());
        assert_eq!(state.selected_layer_id.as_deref(), Some(solid.as_str()));
    }

    #[test]
    fn test_mask_lifecycle_and_path_animation() {
        use crate::state::EditorState;
        use project::{MaskMode, Vec2};

        let mut state = EditorState::new();
        let layer_id = "layer_accent";

        // Add + configure.
        let mid = state.add_mask_to_layer(layer_id).expect("mask added");
        assert!(state.active_composition().unwrap().get_layer(layer_id).unwrap().has_masks());
        state.cycle_mask_mode(layer_id, &mid).expect("mode cycled");
        assert_eq!(
            state.active_composition().unwrap().get_layer(layer_id).unwrap().get_mask(&mid).unwrap().mode,
            MaskMode::Subtract
        );
        state.toggle_mask_invert(layer_id, &mid).expect("invert toggled");
        assert!(state.active_composition().unwrap().get_layer(layer_id).unwrap().get_mask(&mid).unwrap().invert);
        assert!(state.nudge_mask_param(layer_id, &mid, "feather", 6.0).is_ok());
        assert!(!state.nudge_mask_param(layer_id, &mid, "nope", 1.0).is_ok());

        // Node edits.
        let before = state.active_composition().unwrap().get_layer(layer_id).unwrap().get_mask(&mid).unwrap().path.value.points.len();
        state.append_mask_point(layer_id, &mid, Vec2::new(10.0, 20.0)).expect("append");
        state.move_mask_point_live(layer_id, &mid, 0, Vec2::new(-90.0, -90.0)).expect("move");
        state.move_mask_handle_live(layer_id, &mid, 0, false, Vec2::new(-70.0, -90.0)).expect("handle");
        state.cycle_mask_point_kind(layer_id, &mid, 0).expect("kind");
        {
            let layer = state.active_composition().unwrap().get_layer(layer_id).unwrap().clone();
            let mask = layer.get_mask(&mid).unwrap();
            assert_eq!(mask.path.value.points.len(), before + 1);
            assert_eq!(mask.path.value.points[0].pos, Vec2::new(-90.0, -90.0));
        }
        state.delete_mask_point(layer_id, &mid, 0).expect("delete");
        assert_eq!(
            state.active_composition().unwrap().get_layer(layer_id).unwrap().get_mask(&mid).unwrap().path.value.points.len(),
            before
        );

        // Path keyframes morph (same topology interpolates point-wise).
        state.seek(0.0);
        assert!(state.toggle_mask_path_keyframe_at_current_time(layer_id, &mid).expect("key"));
        state.seek(1.0);
        state.move_mask_point_live(layer_id, &mid, 0, Vec2::new(0.0, 0.0)).expect("move at t=1");
        // Mid-way evaluation morphs node 0 between (100,-100) and (0,0).
        state.seek(0.5);
        let eval = state.evaluate_current_frame().unwrap();
        let eval_layer = eval.get_layer(layer_id).unwrap();
        let emask = eval_layer.masks.iter().find(|m| m.id == mid).unwrap();
        assert_eq!(emask.path.points.len(), 4);
        let mid_pos = emask.path.points[0].pos;
        assert!((mid_pos.x - 50.0).abs() < 2.0 && (mid_pos.y + 50.0).abs() < 2.0, "{mid_pos:?}");
        // Topology change holds instead of morphing: 5 static nodes vs 4-pt
        // keys evaluate to the last key past its time.
        state.append_mask_point(layer_id, &mid, Vec2::new(300.0, 300.0)).expect("append2");
        state.seek(2.0);
        let eval2 = state.evaluate_current_frame().unwrap();
        let emask2 = eval2.get_layer(layer_id).unwrap().masks.iter().find(|m| m.id == mid).unwrap();
        assert_eq!(emask2.path.points.len(), 4);
        // Back at frame 0 the diamond is lit; seeking back finds keys.
        state.seek(0.0);
        assert!(state.active_composition().unwrap().get_layer(layer_id).unwrap().get_mask(&mid).unwrap().path.has_keyframe_at(&state.clock.timecode()));
        state.seek(2.0);
        assert!(state.seek_mask_path_keyframe(layer_id, &mid, -1).expect("seek prev"));
        assert_eq!(state.clock.current_frame(), 30);

        // World-space pen point lands in layer-local coords.
        state.set_active_mask_edit(Some((layer_id.to_string(), mid.clone())));
        state.add_pen_point(Vec2::new(0.0, 0.0)).expect("pen to mask");
        assert_eq!(
            state.active_composition().unwrap().get_layer(layer_id).unwrap().get_mask(&mid).unwrap().path.value.points.len(),
            before + 2
        );

        // Cleanup.
        state.remove_layer_mask(layer_id, &mid).expect("mask removed");
        assert!(!state.active_composition().unwrap().get_layer(layer_id).unwrap().has_masks());
        assert!(state.active_mask_edit.is_none());
    }

    #[test]
    fn test_text_path_pen_and_clear() {        use crate::state::EditorState;
        use project::{LayerSource, Vec2};

        let mut state = EditorState::new();
        // Badge layer is a solid; make a text layer instead via existing API.
        state.select_layer(Some("layer_badge".to_string()));
        // Pen on a text layer draws its baseline path.
        state.add_text_layer("Hello", None).expect("text layer");
        let tid = state.selected_layer_id.clone().unwrap();
        state.add_pen_point(Vec2::new(-50.0, 0.0)).expect("pen1");
        state.add_pen_point(Vec2::new(50.0, -20.0)).expect("pen2");
        let has_path = match &state.active_composition().unwrap().get_layer(&tid).unwrap().source {
            LayerSource::Text { text_path, .. } => text_path.as_ref().map(|p| p.points.len()),
            _ => None,
        };
        assert_eq!(has_path, Some(2));
        state.clear_text_path(&tid).expect("clear");
        assert!(matches!(
            &state.active_composition().unwrap().get_layer(&tid).unwrap().source,
            LayerSource::Text { text_path: None, .. }
        ));
    }

    #[test]
    fn test_pen_press_routing() {
        use crate::state::EditorState;
        use project::{LayerSource, ShapeType, Vec2};

        let mut state = EditorState::new();
        // Empty pick creates a fresh path layer.
        let id1 = state.pen_press_at(Vec2::new(10.0, 10.0), None).expect("create");
        // Picking the new path layer appends instead of creating.
        let id2 = state
            .pen_press_at(Vec2::new(20.0, 20.0), Some(id1.clone()))
            .expect("append");
        assert_eq!(id1, id2);
        let count = match &state.active_composition().unwrap().get_layer(&id1).unwrap().source {
            LayerSource::Shape { shape_type: ShapeType::Path { path_data, .. } } => {
                project::Path::from_svg(path_data).points.len()
            }
            _ => 0,
        };
        assert_eq!(count, 2);
        // Picking a text layer selects it and draws its baseline.
        let tid = state.add_text_layer("Hi", None).expect("text");
        let picked = state.pen_press_at(Vec2::new(5.0, 5.0), Some(tid.clone())).expect("text pen");
        assert_eq!(picked, tid);
        assert_eq!(state.selected_layer_id.as_deref(), Some(tid.as_str()));
        // Text tool press selects existing text, creates on empty space.
        let same = state.text_press_at(Vec2::ZERO, Some(tid.clone())).expect("select text");
        assert_eq!(same, tid);
        let fresh = state.text_press_at(Vec2::new(300.0, 300.0), None).expect("new text");
        assert_ne!(fresh, tid);
    }

    #[test]
    fn test_pen_draws_mask_by_default_on_non_shape_layer() {
        use crate::state::EditorState;
        use project::{LayerSource, Vec2};

        let mut state = EditorState::new();
        let layer_id = "layer_accent";
        assert!(matches!(
            state.active_composition().unwrap().get_layer(layer_id).unwrap().source,
            LayerSource::Solid { .. }
        ));

        // 1. First pen click on the solid layer creates a mask and adds vertex 1.
        let ret1 = state.pen_press_at(Vec2::new(10.0, 10.0), Some(layer_id.to_string())).expect("point 1");
        assert_eq!(ret1, layer_id);
        assert!(state.active_mask_edit.is_some());
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(layer_id).unwrap();
            assert_eq!(layer.masks.len(), 1);
            let mask = &layer.masks[0];
            assert_eq!(mask.path.value.points.len(), 1);
            assert!(!mask.path.value.closed);
        }

        // 2. Open / incomplete mask must NOT zero out the layer during rasterization.
        let eval = state.evaluate_current_frame().expect("evaluate");
        let eval_layer = eval.get_layer(layer_id).expect("layer evaluated");
        let (buf, _, _) = crate::raster::rasterize_layer(
            eval_layer,
            300.0,
            300.0,
            100,
            100,
            1920.0,
            1080.0,
            project::Color::BLACK,
            None,
            0.0,
            0,
            false,
            5.0,
            &std::collections::HashMap::new(),
        );
        let has_visible_pixels = buf.px.iter().any(|p| p.a > 0.5);
        assert!(has_visible_pixels, "Layer must remain visible while mask is being drawn");

        // 3. Second and third pen clicks append vertices point by point to the active mask.
        state.pen_press_at(Vec2::new(50.0, 10.0), None).expect("point 2");
        state.pen_press_at(Vec2::new(50.0, 50.0), None).expect("point 3");
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(layer_id).unwrap();
            let mask = &layer.masks[0];
            assert_eq!(mask.path.value.points.len(), 3);
            assert!(!mask.path.value.closed);
        }

        // 4. Fourth click near point 1 closes the mask and ends active mask edit.
        state.pen_press_at(Vec2::new(12.0, 11.0), None).expect("point 4 close");
        assert!(state.active_mask_edit.is_none());
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(layer_id).unwrap();
            let mask = &layer.masks[0];
            assert!(mask.path.value.closed, "Mask must be closed when clicking near first vertex");
        }
    }

    #[test]
    fn test_layer_drag_reorder() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        let before: Vec<String> = state
            .active_composition()
            .unwrap()
            .layers
            .iter()
            .map(|l| l.id.clone())
            .collect();
        assert!(before.len() >= 3);
        // Drag the top layer onto the bottom slot.
        let top = before[0].clone();
        let bottom_idx = before.len() - 1;
        state.move_layer_to(&top, bottom_idx).expect("reorder");
        let after: Vec<String> = state
            .active_composition()
            .unwrap()
            .layers
            .iter()
            .map(|l| l.id.clone())
            .collect();
        assert_eq!(after[bottom_idx], top);
        // No-op reorder pushes no undo step: one undo after [real + noop]
        // must restore the original order exactly.
        state.move_layer_to(&top, bottom_idx).expect("noop");
        assert!(state.undo());
        let restored: Vec<String> = state
            .active_composition()
            .unwrap()
            .layers
            .iter()
            .map(|l| l.id.clone())
            .collect();
        assert_eq!(restored, before);
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
            assert_eq!(dup_layer.transform.anchor_point.value, Vec2::new(150.0, 150.0));
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

    #[test]
    fn test_move_layer_keyframe_time_and_reset_content_center() {
        use crate::state::EditorState;
        use project::{Keyframe, TimeCode, Vec2};

        let mut state = EditorState::new();
        let lid = "layer_accent";

        // Set clean keyframes at 1.0s and 2.0s
        {
            let comp = state.active_composition_mut().unwrap();
            let layer = comp.get_layer_mut(lid).unwrap();
            layer.transform.position.clear_keyframes();
            layer.transform.position.add_keyframe(Keyframe::new(TimeCode::from_seconds(1.0, 30.0), Vec2::new(100.0, 200.0)));
            layer.transform.position.add_keyframe(Keyframe::new(TimeCode::from_seconds(2.0, 30.0), Vec2::new(300.0, 400.0)));
        }

        // Move keyframe from 1.0s to 1.5s
        let moved = state.move_layer_keyframe_time(lid, "transform.position", 1.0, 1.5);
        assert!(moved);

        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(lid).unwrap();
            let kfs = layer.transform.position.keyframes();
            assert_eq!(kfs.len(), 2);
            assert!((kfs[0].time_seconds() - 1.5).abs() < 1e-4);
            assert_eq!(kfs[0].value, Vec2::new(100.0, 200.0));
            assert!((kfs[1].time_seconds() - 2.0).abs() < 1e-4);
        }

        // Test reset layer transform resets pos to ZERO and anchor point to content center (150x150 for 300x300 solid)
        state.reset_layer_transform(lid);
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(lid).unwrap();
            assert_eq!(layer.transform.position.value, Vec2::ZERO);
            assert_eq!(layer.transform.anchor_point.value, Vec2::new(150.0, 150.0));
            assert_eq!(layer.transform.rotation.value, 0.0);
            assert!(!layer.transform.position.is_animated());
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
    fn test_new_effects_round_trip() {        use crate::state::EditorState;
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

    #[test]
    fn test_preview_fast_gesture_flag() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        // Idle: full quality.
        assert!(!state.preview_fast);
        // Gestures opt into fast preview; releases restore quality.
        state.preview_fast = true;
        assert!(state.preview_fast);
        // Ending value edit always drops the flag (even with no editor).
        assert!(!state.end_value_edit_state());
        assert!(!state.preview_fast);
        state.preview_fast = true;
        state.value_edit_key = Some("opacity".to_string());
        assert!(state.end_value_edit_state());
        assert!(!state.preview_fast);
    }

    #[gpui_kit::test]
    fn test_text_properties_gpui_kit_components_and_effects_collapsible(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1440.), px(1200.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");

        let (text_id, blur_id) = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                let tid = s.add_text_layer("Hello GPUI Kit", None).unwrap();
                s.select_layer(Some(tid.clone()));
                let bid = s
                    .add_effect_to_selected_layer(project::EffectType::gaussian_blur(15.0))
                    .unwrap();
                (tid, bid)
            })
        });

        // Ensure Properties panel has source expanded and other sections collapsed so controls fit
        app_view.update(cx, |view, cx| {
            view.panels().properties.update(cx, |this, cx| {
                this.source_expanded = true;
                this.transform_expanded = false;
                this.switches_expanded = false;
                this.tools_expanded = false;
                cx.notify();
            });
        });

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);

            // 1. Text Presets (Button)
            assert!(window.find("text_preset_Title Text").visible());
            assert!(window.find("text_preset_Motion Studio").visible());
            window.click("text_preset_Motion Studio", cx);
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { text, .. } => text.value == "Motion Studio",
                _ => false,
            }
        }));

        // 2. Font Size (Button)
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_font_size_48").visible());
            window.click("text_font_size_48", cx);
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { font_size, .. } => (font_size.value - 48.0).abs() < 0.1,
                _ => false,
            }
        }));

        // 3. Faux Italic and ALL CAPS (Checkbox)
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_italic_toggle").visible());
            assert!(window.find("text_caps_toggle").visible());
            window.click("text_italic_toggle", cx);
            window.click("text_caps_toggle", cx);
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { italic, all_caps, .. } => *italic && *all_caps,
                _ => false,
            }
        }));

        // 4. Font Weight (Button)
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_weight_700").visible());
            window.click("text_weight_700", cx);
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { weight, .. } => *weight == 700,
                _ => false,
            }
        }));

        // Collapse Character group so Paragraph and Stroke groups sit near the top
        app_view.update(cx, |view, cx| {
            view.panels().properties.update(cx, |this, cx| {
                this.text_collapsed.insert("character");
                cx.notify();
            });
        });

        // 5. Text Alignment (Button)
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_align_Center").visible());
            window.click("text_align_Center", cx);
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { align, .. } => *align == project::TextAlign::Center,
                _ => false,
            }
        }));

        // 6. Stroke Preset (Button)
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_stroke_2").visible());
            window.click("text_stroke_2", cx);
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { stroke_width, .. } => (stroke_width.value - 2.0).abs() < 0.1,
                _ => false,
            }
        }));

        // 7. Applied Effects Card with nested Collapsible
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let card_id = SharedString::from(format!("applied_effect_{blur_id}"));
            let disc_id = SharedString::from(format!("effect_disclosure_{blur_id}"));
            assert!(window.find(card_id.clone()).visible());
            assert!(window.find(disc_id.clone()).visible());
            // Parameter is visible while open
            assert!(window.find(SharedString::from(format!("param_radius_{blur_id}"))).visible());
            // Click disclosure to collapse
            window.click(disc_id, cx);
        })
        .expect("update_window failed");

        // Verify collapsed state in PropertiesPanel
        assert!(app_view.read_with(cx, |view, cx| {
            view.panels().properties.read(cx).fx_collapsed.contains(&blur_id)
        }));
    }

    #[gpui_kit::test]
    fn test_text_properties_combobox_scrub_and_three_mode_color_picker(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1440.), px(1200.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");

        let text_id = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                let tid = s.add_text_layer("Editable Text", None).unwrap();
                s.select_layer(Some(tid.clone()));
                tid
            })
        });

        // 1. Verify Comboboxes and UI elements render
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_font_combobox").visible());
            assert!(window.find("text_style_combobox").visible());
            assert!(window.find("text_fill_swatch").visible());
            assert!(window.find("text_stroke_swatch").visible());
        })
        .expect("update_window failed");

        // 2. Test interactive scrubbing on text properties via apply_scrub_delta
        let props_entity = app_view.read_with(cx, |view, _| view.panels().properties.clone());
        props_entity.update(cx, |panel, cx| {
            // Scrub font size (+10.0 -> delta 10.0 * 0.5 = +5.0)
            panel.apply_scrub_delta("font_size", 10.0, cx);
            // Scrub font weight (+10.0 -> +100 to weight)
            panel.apply_scrub_delta("font_weight", 10.0, cx);
            // Scrub box width (+50.0)
            panel.apply_scrub_delta("text_box_w:100", 50.0, cx);
            // Scrub box height (+30.0)
            panel.apply_scrub_delta("text_box_h:100", 30.0, cx);
            // Scrub tracking (+4.0)
            panel.apply_scrub_delta("text_tracking:50", 8.0, cx);
            // Scrub leading (+10.0)
            panel.apply_scrub_delta("text_leading:50", 20.0, cx);
            // Scrub stroke width (+4.0)
            panel.apply_scrub_delta("text_stroke_w:50", 8.0, cx);
        });

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text {
                    font_size,
                    weight,
                    box_width,
                    box_height,
                    tracking,
                    leading,
                    stroke_width,
                    ..
                } => {
                    font_size.value > 48.0
                        && *weight >= 500
                        && box_width.value > 0.0
                        && box_height.value > 0.0
                        && tracking.value > 0.0
                        && leading.value > 0.0
                        && stroke_width.value > 0.0
                }
                _ => false,
            }
        }));

        // 3. Test Three-Mode Color Picker: None, Color, Gradient on Text Fill
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Click fill swatch to open 3-mode picker popover
            window.click("text_fill_swatch", cx);
        })
        .expect("update_window failed");

        // Verify popover modes are visible and clickable
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_fill_tab_none").visible());
            assert!(window.find("text_fill_tab_color").visible());
            assert!(window.find("text_fill_tab_gradient").visible());

            // Click "None" tab
            window.click("text_fill_btn_none", cx);
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { fill_color, .. } => fill_color.value.a == 0.0,
                _ => false,
            }
        }));

        // Click "Color" tab
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("text_fill_btn_color", cx);
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { fill_color, .. } => fill_color.value.a > 0.0,
                _ => false,
            }
        }));
    }

    #[gpui_kit::test]
    fn test_path_tool_point_by_point_drawing_and_blend_mode_backdrop(_cx: &mut gpui::TestAppContext) {
        use crate::state::{EditorState, EditorTool};
        let mut state = EditorState::new();
        state.set_tool(EditorTool::Pen);

        // 1. Click 1: starts Path layer
        let p1 = project::Vec2::new(100.0, 100.0);
        let path_layer_id = state.pen_press_at(p1, None).expect("click 1 creates path layer");
        assert_eq!(state.selected_layer_id.as_deref(), Some(path_layer_id.as_str()));

        // 2. Click 2: appends point 2
        let p2 = project::Vec2::new(200.0, 100.0);
        state.pen_press_at(p2, Some("layer_bg".to_string())).expect("click 2 appends point");

        // 3. Click 3: appends point 3
        let p3 = project::Vec2::new(200.0, 200.0);
        state.pen_press_at(p3, Some("layer_bg".to_string())).expect("click 3 appends point");

        // 4. Click 4: appends point 4
        let p4 = project::Vec2::new(100.0, 200.0);
        state.pen_press_at(p4, Some("layer_bg".to_string())).expect("click 4 appends point");

        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(&path_layer_id).unwrap();
            if let project::LayerSource::Shape { shape_type: project::ShapeType::Path { path_data, .. } } = &layer.source {
                let path = project::Path::from_svg(path_data.as_str());
                assert_eq!(path.points.len(), 4);
                assert!(!path.closed);
            } else {
                panic!("Expected Path shape");
            }
        }

        // 5. Click 5 (near vertex 1): closes the path!
        let p_close = project::Vec2::new(102.0, 99.0);
        state.pen_press_at(p_close, Some("layer_bg".to_string())).expect("click 5 closes path");

        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(&path_layer_id).unwrap();
            if let project::LayerSource::Shape { shape_type: project::ShapeType::Path { path_data, .. } } = &layer.source {
                let path = project::Path::from_svg(path_data.as_str());
                assert!(path.closed, "Path should be closed after clicking near vertex 0");
            } else {
                panic!("Expected Path shape");
            }
        }

        // 6. Verify rasterization fills the path with purple (#A855F7)
        let eval = state.evaluate_current_frame().expect("evaluate");
        let eval_path = eval.get_layer(&path_layer_id).expect("layer evaluated");
        let (path_buf, _, _) = crate::raster::rasterize_layer(
            eval_path,
            120.0,
            120.0,
            120,
            120,
            1920.0,
            1080.0,
            project::Color::BLACK,
            None,
            0.0,
            0,
            false,
            5.0,
            &std::collections::HashMap::new(),
        );
        let filled_pixels = path_buf.px.iter().filter(|p| p.a > 0.8).count();
        assert!(filled_pixels > 100, "Closed path must have filled interior");

        // 7. Verify per-pixel backdrop blending with Multiply blend mode:
        // A solid layer with BlendMode::Multiply blends against backdrop_buf
        let blue = project::Color::from_rgba_u8(0, 0, 255, 255);
        let mut blue_solid = project::Layer::solid(
            "blue_solid",
            "Blue Solid",
            blue,
            100,
            100,
            project::TimeCode::zero(30.0),
            project::TimeCode::from_frames(150, 30.0),
        );
        blue_solid.blend_mode = project::BlendMode::Multiply;
        let comp = state.active_composition_mut().unwrap();
        comp.insert_layer(0, blue_solid).unwrap();

        let eval = state.evaluate_current_frame().expect("evaluate");
        let eval_blue = eval.get_layer("blue_solid").expect("blue solid evaluated");

        // Create a backdrop buffer containing purple pixels (#A855F7)
        let purple_px = crate::raster::Px::from_color(project::Color::from_rgba_u8(168, 85, 247, 255));
        let mut backdrop = crate::raster::FloatBuf::clear(100, 100);
        for p in backdrop.px.iter_mut() {
            *p = purple_px;
        }

        let (blended_buf, _, _) = crate::raster::rasterize_layer(
            eval_blue,
            100.0,
            100.0,
            100,
            100,
            1920.0,
            1080.0,
            project::Color::BLACK,
            Some(&backdrop),
            0.0,
            0,
            false,
            5.0,
            &std::collections::HashMap::new(),
        );

        // Under Multiply, purple (0.658, 0.333, 0.968) * blue (0.0, 0.0, 1.0) = (0.0, 0.0, 0.968)
        let sample = blended_buf.get(50, 50);
        assert!(sample.r < 0.05, "Red channel must be near 0 under Multiply with pure blue: {}", sample.r);
        assert!(sample.g < 0.05, "Green channel must be near 0 under Multiply with pure blue: {}", sample.g);
        assert!(sample.b > 0.8, "Blue channel must remain high under Multiply: {}", sample.b);
    }

    #[test]
    fn test_all_26_blend_modes_evaluation() {
        use project::{BlendMode, Color};

        assert_eq!(BlendMode::ALL.len(), 26);
        let c1 = Color::rgba(0.7, 0.4, 0.2, 1.0);
        let c2 = Color::rgba(0.5, 0.8, 0.6, 1.0);

        for mode in BlendMode::ALL {
            let str_name = mode.as_str();
            assert!(!str_name.is_empty());
            let snake = mode.as_snake_case();
            assert!(!snake.is_empty());
            let parsed = BlendMode::from_name(str_name).expect("parse str");
            assert_eq!(parsed, mode);
            let parsed_snake = BlendMode::from_name(snake).expect("parse snake");
            assert_eq!(parsed_snake, mode);

            // Verify evaluation produces valid non-NaN RGBA
            let blended = mode.blend_rgb(c1, c2);
            assert!(blended.r >= 0.0 && blended.r <= 1.0 && !blended.r.is_nan());
            assert!(blended.g >= 0.0 && blended.g <= 1.0 && !blended.g.is_nan());
            assert!(blended.b >= 0.0 && blended.b <= 1.0 && !blended.b.is_nan());
        }

        // Test specific Linear Burn math: (cb + cs - 1.0).max(0.0)
        let lb = BlendMode::LinearBurn.blend_rgb(Color::rgba(0.6, 0.8, 0.3, 1.0), Color::rgba(0.7, 0.1, 0.9, 1.0));
        assert!((lb.r - (0.6 + 0.7 - 1.0)).abs() < 1e-4);
        assert_eq!(lb.g, 0.0); // 0.8 + 0.1 - 1.0 = -0.1 -> 0.0
        assert!((lb.b - (0.3 + 0.9 - 1.0)).abs() < 1e-4);
    }

    #[test]
    fn test_non_destructive_parenting_zero_jump() {
        use crate::state::EditorState;
        use project::Vec2;

        let mut state = EditorState::new();
        // Create parent layer with translation, rotation, and non-uniform scale
        let parent_id = state.add_solid_layer("Parent", project::Color::RED, 200, 200).expect("add parent");
        state.set_layer_position(&parent_id, Vec2::new(350.0, 250.0));
        state.set_layer_rotation(&parent_id, 45.0);
        state.set_layer_scale(&parent_id, 150.0, 80.0);

        // Create child layer positioned in world space
        let child_id = state.add_solid_layer("Child", project::Color::BLUE, 100, 100).expect("add child");
        state.set_layer_position(&child_id, Vec2::new(400.0, 300.0));
        state.set_layer_rotation(&child_id, 15.0);
        state.set_layer_scale(&child_id, 120.0, 120.0);

        // Capture child's world matrix and world point before parenting
        let world_before = state.layer_world_matrix_fast(&child_id).expect("world before");
        let test_pt = Vec2::new(50.0, 50.0);
        let world_pt_before = world_before.transform_point(test_pt);

        // 1. Parent Child to Parent:
        assert!(state.set_layer_parent(&child_id, Some(parent_id.clone())));

        // Verify child's world matrix after parenting matches world_before (zero visual jump!)
        let world_after = state.layer_world_matrix_fast(&child_id).expect("world after");
        let world_pt_after = world_after.transform_point(test_pt);
        assert!((world_pt_after.x - world_pt_before.x).abs() < 1e-2, "X jumped on parenting: before={}, after={}", world_pt_before.x, world_pt_after.x);
        assert!((world_pt_after.y - world_pt_before.y).abs() < 1e-2, "Y jumped on parenting: before={}, after={}", world_pt_before.y, world_pt_after.y);

        // 2. Unparent Child:
        assert!(state.set_layer_parent(&child_id, None));
        let world_unparent = state.layer_world_matrix_fast(&child_id).expect("world unparent");
        let world_pt_unparent = world_unparent.transform_point(test_pt);
        assert!((world_pt_unparent.x - world_pt_before.x).abs() < 1e-2, "X jumped on unparenting: before={}, after={}", world_pt_before.x, world_pt_unparent.x);
        assert!((world_pt_unparent.y - world_pt_before.y).abs() < 1e-2, "Y jumped on unparenting: before={}, after={}", world_pt_before.y, world_pt_unparent.y);
    }

    #[test]
    fn test_pen_tool_mask_by_default_and_point_by_point() {
        use crate::state::EditorState;
        use project::Vec2;

        let mut state = EditorState::new();
        // Select an existing solid layer
        let sel_id = "layer_accent";
        state.select_layer(Some(sel_id.to_string()));

        let initial_layer_count = state.active_composition().unwrap().layers.len();

        // Click 1 with Pen tool on canvas: MUST NOT create a new layer! Must create Mask on selected layer!
        let ret1 = state.pen_press_at(Vec2::new(10.0, 10.0), Some(sel_id.to_string())).expect("click 1");
        assert_eq!(ret1, sel_id);
        assert_eq!(state.active_composition().unwrap().layers.len(), initial_layer_count, "No new layer created");
        assert!(state.active_mask_edit.is_some(), "Must enter active mask edit");

        // Click 2: appends point 2 to mask
        let ret2 = state.pen_press_at(Vec2::new(100.0, 20.0), None).expect("click 2");
        assert_eq!(ret2, sel_id);
        assert_eq!(state.active_composition().unwrap().layers.len(), initial_layer_count);

        // Click 3: appends point 3 to mask
        let ret3 = state.pen_press_at(Vec2::new(80.0, 120.0), None).expect("click 3");
        assert_eq!(ret3, sel_id);
        assert_eq!(state.active_composition().unwrap().layers.len(), initial_layer_count);

        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(sel_id).unwrap();
            assert_eq!(layer.masks.len(), 1);
            assert_eq!(layer.masks[0].path.value.points.len(), 3);
            assert!(!layer.masks[0].path.value.closed);
        }

        // Click 4 near point 1: closes mask!
        state.pen_press_at(Vec2::new(12.0, 11.0), None).expect("click 4 close");
        assert!(state.active_mask_edit.is_none(), "Mask edit ended on close");
        {
            let comp = state.active_composition().unwrap();
            let layer = comp.get_layer(sel_id).unwrap();
            assert!(layer.masks[0].path.value.closed, "Mask is now closed");
        }
    }

    #[gpui_kit::test]
    async fn test_shape_tool_creates_mask_when_layer_selected_and_shape_layer_when_none(_cx: &mut gpui::TestAppContext) {
        let mut state = EditorState::new();

        // 1. With a layer selected (e.g. layer_accent), adding a shaped mask creates a mask on it
        let sel_id = "layer_accent";
        state.select_layer(Some(sel_id.to_string()));
        let initial_layer_count = state.active_composition().unwrap().layers.len();

        let mid_rect = state
            .add_shaped_mask_at(sel_id, project::MaskShapeKind::Rectangle, Some(Vec2::new(50.0, 50.0)), Some((100.0, 80.0)))
            .expect("add rect mask");
        let mid_ellipse = state
            .add_shaped_mask_at(sel_id, project::MaskShapeKind::Ellipse, Some(Vec2::new(200.0, 150.0)), Some((80.0, 80.0)))
            .expect("add ellipse mask");

        // Layers count did NOT increase (they are masks on the layer)
        assert_eq!(state.active_composition().unwrap().layers.len(), initial_layer_count);

        let comp = state.active_composition().unwrap();
        let layer = comp.get_layer(sel_id).unwrap();
        assert_eq!(layer.masks.len(), 2);
        assert_eq!(layer.masks[0].id, mid_rect);
        assert_eq!(layer.masks[1].id, mid_ellipse);
        assert!(layer.masks[0].path.value.closed);
        assert!(layer.masks[1].path.value.closed);

        // 2. When NO layer is selected, adding rectangle / ellipse shape adds new shape layers
        state.select_layer(None);
        let rect_layer_id = state.add_rectangle_shape_layer(300.0, 200.0, Some(Vec2::new(0.0, 0.0))).expect("rect shape layer");
        assert_eq!(state.active_composition().unwrap().layers.len(), initial_layer_count + 1);
        let rect_layer = state.active_composition().unwrap().get_layer(&rect_layer_id).unwrap();
        assert!(matches!(rect_layer.source, project::LayerSource::Shape { shape_type: project::ShapeType::Rectangle { .. } }));
    }

    #[gpui_kit::test]
    async fn test_pen_tangent_dragging_promotes_symmetric_and_alt_breaks(_cx: &mut gpui::TestAppContext) {
        let mut state = EditorState::new();
        let sel_id = "layer_accent";
        state.select_layer(Some(sel_id.to_string()));

        // Add a mask to layer_accent
        let mid = state.add_mask_to_layer(sel_id).expect("mask");
        state.append_mask_point(sel_id, &mid, Vec2::new(100.0, 100.0)).expect("pt 0");
        let pt_idx = {
            let comp = state.active_composition().unwrap();
            let mask = comp.get_layer(sel_id).unwrap().get_mask(&mid).unwrap();
            mask.path.value.points.len() - 1
        };

        // Point starts as corner with zero tangents
        {
            let comp = state.active_composition().unwrap();
            let mask = comp.get_layer(sel_id).unwrap().get_mask(&mid).unwrap();
            assert_eq!(mask.path.value.points[pt_idx].kind, project::PathPointKind::Corner);
            assert_eq!(mask.path.value.points[pt_idx].out_tan, Vec2::ZERO);
            assert_eq!(mask.path.value.points[pt_idx].in_tan, Vec2::ZERO);
        }

        // Pulling tangent with move_mask_handle_live promotes it to Symmetric and sets mirrored handles!
        state.move_mask_handle_live(sel_id, &mid, pt_idx, false, Vec2::new(150.0, 100.0)).expect("pull handle");
        {
            let comp = state.active_composition().unwrap();
            let mask = comp.get_layer(sel_id).unwrap().get_mask(&mid).unwrap();
            let pt = &mask.path.value.points[pt_idx];
            assert_eq!(pt.kind, project::PathPointKind::Symmetric);
            assert_eq!(pt.out_tan, Vec2::new(50.0, 0.0));
            assert_eq!(pt.in_tan, Vec2::new(-50.0, 0.0)); // Mirrored!
        }

        // Alt-dragging with move_mask_handle_live_break breaks symmetry into Corner!
        state.move_mask_handle_live_break(sel_id, &mid, pt_idx, false, Vec2::new(130.0, 120.0)).expect("break handle");
        {
            let comp = state.active_composition().unwrap();
            let mask = comp.get_layer(sel_id).unwrap().get_mask(&mid).unwrap();
            let pt = &mask.path.value.points[pt_idx];
            assert_eq!(pt.kind, project::PathPointKind::Corner);
            assert_eq!(pt.out_tan, Vec2::new(30.0, 20.0));
            assert_eq!(pt.in_tan, Vec2::new(-50.0, 0.0)); // Preserved independently!
        }
    }

    #[gpui_kit::test]
    async fn test_timeline_mask_shortcuts_m_and_mm_and_property_integration(_cx: &mut gpui::TestAppContext) {
        let mut state = EditorState::new();
        let sel_id = "layer_accent";
        state.select_layer(Some(sel_id.to_string()));

        // Single 'M' press: toggles timeline_masks_reveal_path
        let (rev1, is_all1) = state.handle_m_shortcut();
        assert!(rev1);
        assert!(!is_all1);
        assert!(state.timeline_masks_reveal_path);

        // Immediate second 'M' press (<350ms): triggers MM (reveal all mask properties)
        let (rev2, is_all2) = state.handle_m_shortcut();
        assert!(rev2);
        assert!(is_all2);
        assert!(state.timeline_masks_reveal_all);

        // Add mask and test stopwatch / keyframing navigation for mask: params
        let mid = state.add_shaped_mask(sel_id, project::MaskShapeKind::Rectangle).expect("mask");

        // Toggle mask path keyframe
        let path_key = format!("mask:{}:path", mid);
        state.toggle_layer_property_animation(sel_id, &path_key);
        {
            let comp = state.active_composition().unwrap();
            let mask = comp.get_layer(sel_id).unwrap().get_mask(&mid).unwrap();
            assert!(mask.path.is_animated());
        }

        // Toggle feather animation
        let feather_key = format!("mask:{}:feather", mid);
        state.toggle_layer_property_animation(sel_id, &feather_key);
        {
            let comp = state.active_composition().unwrap();
            let mask = comp.get_layer(sel_id).unwrap().get_mask(&mid).unwrap();
            assert!(mask.feather.is_animated());
        }

        // Nudge feather via timeline value
        state.nudge_timeline_value(sel_id, &feather_key, 25.0);
        let cur_f = state.timeline_current_value(sel_id, &feather_key).expect("feather val");
        assert_eq!(cur_f, 25.0);

        // Nudge opacity via timeline value
        let opacity_key = format!("mask:{}:opacity", mid);
        state.set_timeline_value(sel_id, &opacity_key, 75.0);
        let cur_o = state.timeline_current_value(sel_id, &opacity_key).expect("opacity val");
        assert_eq!(cur_o, 75.0);
    }
}
