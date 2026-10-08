pub(crate) mod modifier_graph_view;
pub(crate) mod panels;
pub(crate) mod color_editor;
pub mod raster;
pub mod state;
pub mod widgets;
use state::{EditorState, EditorTool};

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub use gpui_kit::base::{h_flex, v_flex, Positioner, StyledExt, TestSupportExt};
pub use gpui_kit::component::dock::{
    panel_handle, BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, Panel, PanelStyle,
};
use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeMode, WindowExt};
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
    /// Export dialog visibility + options.
    pub show_export: bool,
    pub export_format_idx: usize,
    pub export_busy: bool,
    pub export_progress: Option<(usize, usize)>,
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

        // Bind spacebar key to TogglePlayback only outside text inputs
        cx.bind_keys([KeyBinding::new("space", TogglePlayback, Some("!Input"))]);
        // One desktop command per tool (toolbar clicks dispatch the same
        // actions — see the root view's on_action handlers).
        cx.bind_keys([
            KeyBinding::new("v", SelectMoveTool, Some("!Input")),
            KeyBinding::new("h", SelectHandTool, Some("!Input")),
            KeyBinding::new("w", SelectRotateTool, Some("!Input")),
            KeyBinding::new("g", SelectPenTool, Some("!Input")),
            KeyBinding::new("t", SelectTextTool, Some("!Input")),
            KeyBinding::new("q", CycleShapeTool, Some("!Input")),
            KeyBinding::new("f", FitGraphView, Some("!Input")),
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

        dock_area.update(cx, |dock, cx| {
            dock.set_dock(DockPlacement::Left, left_layout, window, cx);
            dock.set_dock_size(DockPlacement::Left, px(280.), window, cx);

            dock.set_center(center_layout, window, cx);

            dock.set_dock(DockPlacement::Right, right_layout, window, cx);
            dock.set_dock_size(DockPlacement::Right, px(300.), window, cx);
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
            show_export: false,
            export_format_idx: 0,
            export_busy: false,
            export_progress: None,
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

    fn menu_x(self) -> Pixels {
        match self {
            Self::File => px(8.),
            Self::Edit => px(50.),
            Self::Composition => px(92.),
            Self::Layer => px(178.),
            Self::View => px(226.),
            Self::About => px(270.),
        }
    }
}

/// Reset the project to a fresh untitled project.
fn request_new_project(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    let s = state.clone();
    let a = app.clone();
    s.update(cx, |s, cx| {
        s.new_project("Untitled Project");
        cx.notify();
    });
    a.update(cx, |a, cx| {
        a.open_menu = None;
        a.menu_note = Some("Created new project".to_string());
        cx.notify();
    });
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

/// Ask the OS for a single media file to import (image, video, audio).
fn request_import_file(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    let s_import = state.clone();
    let a_import = app.clone();
    cx.spawn(|cx: &mut AsyncApp| {
        let cx = cx.clone();
        async move {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("file-dialog-worker".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let file = rfd::FileDialog::new()
                        .add_filter("All Supported Media", &["png", "jpg", "jpeg", "mp4", "mov", "webm", "wav", "mp3", "ogg"])
                        .add_filter("Images", &["png", "jpg", "jpeg"])
                        .add_filter("Videos", &["mp4", "mov", "webm"])
                        .add_filter("Audio", &["wav", "mp3", "ogg"])
                        .pick_file();
                    let _ = tx.send(file);
                });
            if let Ok(Some(path)) = rx.recv() {
                cx.update(|cx| {
                    s_import.update(cx, |s, cx| {
                        match s.import_media_file(path.clone()) {
                            Ok(_) => {
                                a_import.update(cx, |a, cx| {
                                    a.menu_note = Some(format!(
                                        "Imported {}",
                                        path.file_name().and_then(|n| n.to_str()).unwrap_or("media")
                                    ));
                                    cx.notify();
                                });
                            }
                            Err(e) => {
                                a_import.update(cx, |a, cx| {
                                    a.menu_note = Some(format!("Import failed: {e}"));
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

/// Ask the OS for multiple media files to import.
fn request_import_multiple(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    let s_import = state.clone();
    let a_import = app.clone();
    cx.spawn(|cx: &mut AsyncApp| {
        let cx = cx.clone();
        async move {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("file-dialog-worker".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let files = rfd::FileDialog::new()
                        .add_filter("All Supported Media", &["png", "jpg", "jpeg", "mp4", "mov", "webm", "wav", "mp3", "ogg"])
                        .add_filter("Images", &["png", "jpg", "jpeg"])
                        .add_filter("Videos", &["mp4", "mov", "webm"])
                        .pick_files();
                    let _ = tx.send(files);
                });
            if let Ok(Some(paths)) = rx.recv() {
                if !paths.is_empty() {
                    cx.update(|cx| {
                        s_import.update(cx, |s, cx| {
                            match s.import_multiple_media_files(&paths) {
                                Ok(count) => {
                                    a_import.update(cx, |a, cx| {
                                        a.menu_note = Some(format!("Imported {count} files"));
                                        cx.notify();
                                    });
                                }
                                Err(e) => {
                                    a_import.update(cx, |a, cx| {
                                        a.menu_note = Some(format!("Import error: {e}"));
                                        cx.notify();
                                    });
                                }
                            }
                            cx.notify();
                        });
                    });
                }
            }
        }
    })
    .detach();
}

/// Ask the OS for a directory to recursively scan and import supported media.
fn request_import_folder(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    let s_import = state.clone();
    let a_import = app.clone();
    cx.spawn(|cx: &mut AsyncApp| {
        let cx = cx.clone();
        async move {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("file-dialog-worker".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let folder = rfd::FileDialog::new().pick_folder();
                    let _ = tx.send(folder);
                });
            if let Ok(Some(folder)) = rx.recv() {
                cx.update(|cx| {
                    s_import.update(cx, |s, cx| {
                        match s.import_media_folder(&folder) {
                            Ok(count) => {
                                a_import.update(cx, |a, cx| {
                                    a.menu_note = Some(format!(
                                        "Imported {count} items from {}",
                                        folder.file_name().and_then(|n| n.to_str()).unwrap_or("folder")
                                    ));
                                    cx.notify();
                                });
                            }
                            Err(e) => {
                                a_import.update(cx, |a, cx| {
                                    a.menu_note = Some(format!("Import error: {e}"));
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

/// Export the current frame as a PNG image.
fn request_export_frame(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    let s_export = state.clone();
    let a_export = app.clone();
    let cur_frame = state.read(cx).clock.current_frame();
    let default_name = format!("frame_{:04}.png", cur_frame);
    cx.spawn(|cx: &mut AsyncApp| {
        let cx = cx.clone();
        async move {
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("file-dialog-worker".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let file = rfd::FileDialog::new()
                        .add_filter("PNG Image (*.png)", &["png"])
                        .set_file_name(default_name)
                        .save_file();
                    let _ = tx.send(file);
                });
            if let Ok(Some(path)) = rx.recv() {
                cx.update(|cx| {
                    s_export.update(cx, |s, cx| {
                        match s.export_frame_as_png(&path) {
                            Ok(()) => {
                                a_export.update(cx, |a, cx| {
                                    a.menu_note = Some(format!(
                                        "Exported frame to {}",
                                        path.file_name().and_then(|n| n.to_str()).unwrap_or("frame.png")
                                    ));
                                    cx.notify();
                                });
                            }
                            Err(e) => {
                                a_export.update(cx, |a, cx| {
                                    a.menu_note = Some(format!("Export failed: {e}"));
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

/// Quit application immediately.
fn request_quit() {
    std::process::exit(0);
}

/// Kick off a full-composition render on a background thread.
///
/// Nothing here blocks the UI: the native save dialog opens on its own worker
/// thread, frame rasterization runs on the background executor, and a foreground
/// poller copies progress out of an atomic pair every 120ms. The render thread
/// never touches gpui state directly — it only ever sees a cloned `Project` and
/// writes raw RGBA byte buffers.
fn request_export_run(app: &Entity<AppView>, state: &Entity<EditorState>, cx: &mut App) {
    if state.read(cx).active_composition().is_none() {
        app.update(cx, |a, cx| {
            a.menu_note = Some("Nothing to export: no composition".to_string());
            cx.notify();
        });
        return;
    }
    let s_run = state.clone();
    let a_run = app.clone();
    let a_poll = app.clone();
    let fmt_idx = app.read(cx).export_format_idx;
    let format = export::ExportFormat::all()
        .get(fmt_idx)
        .copied()
        .unwrap_or(export::ExportFormat::Mp4);
    let playhead = state.read(cx).clock.current_frame();
    let comp_name = state
        .read(cx)
        .active_composition()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "comp".to_string());
    let safe: String = comp_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let default_name = format!("{safe}.{ext}", ext = format.extension());
    let is_still = format == export::ExportFormat::PngStill;
    // (done, total) — written by the render thread, read by the UI poller.
    let prog = Arc::new((AtomicUsize::new(0), AtomicUsize::new(1)));
    let prog_bg = prog.clone();
    let prog_ui = prog.clone();
    app.update(cx, |a, cx| {
        a.export_busy = true;
        a.export_progress = Some((0, 1));
        cx.notify();
    });
    // Foreground progress poller — exits as soon as the render clears busy.
    cx.spawn(move |cx: &mut AsyncApp| {
        let cx = cx.clone();
        async move {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(120))
                    .await;
                let busy = a_poll.read_with(&cx, |a, _| a.export_busy);
                if !busy {
                    break;
                }
                let done = prog_ui.0.load(Ordering::Relaxed);
                let total = prog_ui.1.load(Ordering::Relaxed).max(1);
                cx.update(|cx| {
                    a_poll.update(cx, |a, cx| {
                        a.export_progress = Some((done, total));
                        cx.notify();
                    });
                });
            }
        }
    })
    .detach();
    cx.spawn(move |cx: &mut AsyncApp| {
        let cx = cx.clone();
        async move {
            // Native dialog on a worker thread so the UI never blocks.
            let (tx, rx) = std::sync::mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("file-dialog-worker".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let dlg = rfd::FileDialog::new()
                        .set_title("Export Composition")
                        .add_filter(format.label(), &[format.extension()]);
                    let out = match format {
                        export::ExportFormat::PngSequence => {
                            dlg.set_file_name(&default_name).pick_folder()
                        }
                        _ => dlg.set_file_name(default_name).save_file(),
                    };
                    let _ = tx.send(out);
                });
            let dest = match rx.recv() {
                Ok(Some(p)) => p,
                _ => {
                    cx.update(|cx| {
                        a_run.update(cx, |a, cx| {
                            a.export_busy = false;
                            a.export_progress = None;
                            a.menu_note = Some("Export cancelled".to_string());
                            cx.notify();
                        });
                    });
                    return;
                }
            };
            let project = s_run.read_with(&cx, |s, _| s.project.clone());
            let comp_id = s_run.read_with(&cx, |s, _| s.active_comp_id.clone());
            let render = cx
                .background_executor()
                .spawn(async move {
                    // A still exports exactly the playhead frame; everything else
                    // renders the composition's full duration.
                    let (start, end) = if is_still {
                        (Some(playhead), Some(playhead))
                    } else {
                        (None, None)
                    };
                    let job = export::ExportJob {
                        comp_id: comp_id.clone(),
                        format,
                        output: dest,
                        start_frame: start,
                        end_frame: end,
                        gif_max_side: 640,
                        gif_fps: 15.0,
                    };
                    let cid = job.comp_id.clone();
                    let report = move |done: usize, total: usize| {
                        prog_bg.0.store(done, Ordering::Relaxed);
                        prog_bg.1.store(total.max(1), Ordering::Relaxed);
                    };
                    export::render_job(
                        &project,
                        &job,
                        |fr, _tc, w, h| EditorState::render_export_frame(&project, &cid, fr, w, h),
                        Some(&report),
                    )
                    .map_err(|e| e.to_string())
                })
                .await;
            cx.update(|cx| {
                a_run.update(cx, |a, cx| {
                    a.export_busy = false;
                    a.export_progress = None;
                    match render {
                        Ok(r) => {
                            let mut msg = format!(
                                "Exported {} frame{} to {}",
                                r.frames,
                                if r.frames == 1 { "" } else { "s" },
                                r.primary.display()
                            );
                            if let Some(n) = r.note {
                                msg.push_str(" (");
                                msg.push_str(&n);
                                msg.push(')');
                            }
                            a.menu_note = Some(msg);
                            a.show_export = false;
                        }
                        Err(e) => a.menu_note = Some(format!("Export failed: {e}")),
                    }
                    cx.notify();
                });
            });
        }
    })
    .detach();
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

/// The File / Edit / About bar above the toolbar.
fn render_menubar(
    app: &Entity<AppView>,
    open_menu: Option<TopMenu>,
    menu_note: Option<String>,
    state: &Entity<EditorState>,
    cx: &App,
) -> AnyElement {
    let proj_name = state.read(cx).project_display_name();

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

/// Renders the dropdown items for the active menu.
fn render_menu_dropdown(
    menu: TopMenu,
    app: &Entity<AppView>,
    state: &Entity<EditorState>,
    cx: &App,
) -> AnyElement {
    let can_undo = state.read(cx).can_undo();
    let can_redo = state.read(cx).can_redo();
    let has_selection = state.read(cx).selected_layer_id.is_some();
    let recents: Vec<std::path::PathBuf> = state.read(cx).recent_projects.clone();

    let mut items = v_flex().gap_0p5().p_1().min_w(px(250.));
    match menu {
        TopMenu::File => {
                    // --- New Group ---
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_new_project".to_string(),
                            "New Project".to_string(),
                            Some("Ctrl+N".to_string()),
                            true,
                            cx,
                            move |cx| {
                                request_new_project(&a, &s, cx);
                            },
                        ));
                    }
                    {
                        let a = app.clone();
                        items = items.child(menu_item(
                            "menu_new_project_named".to_string(),
                            "New Project with Name…".to_string(),
                            None,
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.show_new_project = true;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    {
                        let a = app.clone();
                        items = items.child(menu_item(
                            "menu_new_comp".to_string(),
                            "New Composition…".to_string(),
                            Some("Ctrl+K".to_string()),
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
                    items = items.child(div().h(px(1.)).my_0p5().bg(cx.theme().border));

                    // --- Open Group ---
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_open_project".to_string(),
                            "Open Project…".to_string(),
                            Some("Ctrl+O".to_string()),
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                                request_open_project(&a, &s, cx);
                            },
                        ));
                    }
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
                        for (idx, path) in recents.iter().take(5).enumerate() {
                            let (a, s) = (app.clone(), state.clone());
                            let p = path.clone();
                            let label = p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("project")
                                .to_string();
                            items = items.child(menu_item(
                                format!("menu_recent_{idx}"),
                                format!("  {label}"),
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
                        {
                            let (a, s) = (app.clone(), state.clone());
                            items = items.child(menu_item(
                                "menu_clear_recents".to_string(),
                                "  Clear Recent Projects".to_string(),
                                None,
                                true,
                                cx,
                                move |cx| {
                                    s.update(cx, |s, cx| {
                                        s.clear_recent_projects();
                                        cx.notify();
                                    });
                                    a.update(cx, |this, cx| {
                                        this.open_menu = None;
                                        this.menu_note = Some("Cleared recent projects".to_string());
                                        cx.notify();
                                    });
                                },
                            ));
                        }
                    }
                    items = items.child(div().h(px(1.)).my_0p5().bg(cx.theme().border));

                    // --- Save Group ---
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_save_project".to_string(),
                            "Save Project".to_string(),
                            Some("Ctrl+S".to_string()),
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                                request_save_project(&a, &s, cx);
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_save_as".to_string(),
                            "Save Project As…".to_string(),
                            Some("Ctrl+Shift+S".to_string()),
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                                request_save_project_as(&a, &s, cx);
                            },
                        ));
                    }
                    items = items.child(div().h(px(1.)).my_0p5().bg(cx.theme().border));

                    // --- Import Group ---
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_import_file".to_string(),
                            "Import File…".to_string(),
                            Some("Ctrl+I".to_string()),
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                                request_import_file(&a, &s, cx);
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_import_multiple".to_string(),
                            "Import Multiple Files…".to_string(),
                            None,
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                                request_import_multiple(&a, &s, cx);
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_import_folder".to_string(),
                            "Import Folder…".to_string(),
                            None,
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                                request_import_folder(&a, &s, cx);
                            },
                        ));
                    }
                    items = items.child(div().h(px(1.)).my_0p5().bg(cx.theme().border));

                    // --- Export Group ---
                    {
                        let a = app.clone();
                        items = items.child(menu_item(
                            "menu_export_movie".to_string(),
                            "Export Composition…".to_string(),
                            Some("Ctrl+M".to_string()),
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    this.show_export = true;
                                    this.export_progress = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_export_frame".to_string(),
                            "Export Current Frame As Image…".to_string(),
                            None,
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                                request_export_frame(&a, &s, cx);
                            },
                        ));
                    }
                    {
                        let (a, s) = (app.clone(), state.clone());
                        items = items.child(menu_item(
                            "menu_export_json".to_string(),
                            "Export Project JSON…".to_string(),
                            None,
                            true,
                            cx,
                            move |cx| {
                                a.update(cx, |this, cx| {
                                    this.open_menu = None;
                                    cx.notify();
                                });
                                request_save_project_as(&a, &s, cx);
                            },
                        ));
                    }
                    items = items.child(div().h(px(1.)).my_0p5().bg(cx.theme().border));

                    // --- Project Settings & Manager ---
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
                    items = items.child(div().h(px(1.)).my_0p5().bg(cx.theme().border));

                    // --- Quit Section ---
                    {
                        items = items.child(menu_item(
                            "menu_quit".to_string(),
                            "Quit Motion Effect".to_string(),
                            Some("Ctrl+Q".to_string()),
                            true,
                            cx,
                            |_cx| {
                                request_quit();
                            },
                        ));
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
                                let _ = s.add_solid_layer("New Solid", project::Color::from_rgba_u8(245, 158, 11, 255), 0, 0);
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
                        let current_q = state.read(cx).preview_quality;
                        let next_q = match current_q {
                            crate::state::PreviewQuality::Full => crate::state::PreviewQuality::Half,
                            crate::state::PreviewQuality::Half => crate::state::PreviewQuality::Quarter,
                            crate::state::PreviewQuality::Quarter => crate::state::PreviewQuality::Auto,
                            crate::state::PreviewQuality::Auto => crate::state::PreviewQuality::Full,
                        };
                        let label = format!("Preview Quality: {} (Switch to {})", current_q.label(), next_q.label());
                        items = items.child(menu_item(
                            "menu_preview_quality".to_string(),
                            label,
                            None,
                            true,
                            cx,
                            move |cx| {
                                s.update(cx, |s, cx| {
                                    s.set_preview_quality(next_q);
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
                    items = items.child(menu_item("menu_about".to_string(), "About Motion Effect".to_string(), None, true, cx, move |cx| {
                        a.update(cx, |this, cx| {
                            this.open_menu = None;
                            this.show_about = true;
                            cx.notify();
                        });
                    }));
                }
        }
    items.into_any_element()
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
            let _vh_f = vh as f32;
            let left = px(vw_f * 0.22).max(px(200.)).min(px(320.));
            let right = px(vw_f * 0.23).max(px(220.)).min(px(340.));
            self.dock_area.update(cx, |dock, cx| {
                dock.set_dock_size(DockPlacement::Left, left, &mut *window, cx);
                dock.set_dock_size(DockPlacement::Right, right, &mut *window, cx);
            });
        }

        // Timeline is rendered as a full-width strip outside the dock area (see main_workspace below).
        // Always keep the bottom dock closed to avoid double-rendering.
        if self.dock_area.read(cx).is_dock_open(DockPlacement::Bottom) {
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

        // Render open menubar dropdown as a deferred overlay above all workspace panels and canvas
        if let Some(menu) = self.open_menu {
            let menu_pos = point(menu.menu_x(), px(30.));
            let a_dismiss = cx.entity().clone();
            // Full-window transparent backdrop (starting below menubar) so clicking outside dismisses menu
            dialogs.push(
                deferred(
                    Positioner::corner(Anchor::TopLeft, point(px(0.), px(30.)))
                        .margin(px(0.))
                        .child(
                            div()
                                .id("menu_dismiss_backdrop")
                                .size_full()
                                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                    a_dismiss.update(cx, |this, cx| {
                                        this.open_menu = None;
                                        cx.notify();
                                    });
                                })
                        )
                )
                .into_any_element()
            );
            // Dropdown menu container with occlude so clicks inside execute menu actions
            let dropdown_content = render_menu_dropdown(menu, &cx.entity(), &self.state, cx);
            dialogs.push(
                deferred(
                    Positioner::corner(Anchor::TopLeft, menu_pos)
                        .margin(px(0.))
                        .occlude()
                        .child(
                            div()
                                .id(SharedString::from(format!("dropdown_{:?}", menu).to_lowercase()))
                                .test_support()
                                .min_w(px(250.))
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded_md()
                                .shadow_lg()
                                .p_1()
                                .child(dropdown_content)
                        )
                )
                .into_any_element()
            );
        }
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
                                        .child(div().font_bold().text_sm().child("Motion Effect"))
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                                        )
                                        .child(
                                            div().text_xs().child(
                                                "Motion graphics, visual effects, and compositing: CPU raster viewport, transform gizmo, keyframe spline editor, Shader Lab shaders, and 31 GPU-validated effects.",
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
            let dlg_w = 520.0f32;
            let dlg_h = 560.0f32;
            let dlg_pos = point(
                px((vw_f - dlg_w).max(8.0) / 2.0),
                px((vh_f - dlg_h).max(8.0) / 2.0),
            );

            let pm_name_inp: Entity<InputState> = window.use_keyed_state("pm_new_comp_name", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("Comp 2", window, cx);
                st
            });
            let pm_w_inp: Entity<InputState> = window.use_keyed_state("pm_new_comp_w", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("1920", window, cx);
                st
            });
            let pm_h_inp: Entity<InputState> = window.use_keyed_state("pm_new_comp_h", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("1080", window, cx);
                st
            });
            let pm_fps_inp: Entity<InputState> = window.use_keyed_state("pm_new_comp_fps", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("30", window, cx);
                st
            });
            let pm_dur_inp: Entity<InputState> = window.use_keyed_state("pm_new_comp_dur", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("10.0", window, cx);
                st
            });

            let pm_act_w_inp: Entity<InputState> = window.use_keyed_state("pm_act_w", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("1920", window, cx);
                st
            });
            let pm_act_h_inp: Entity<InputState> = window.use_keyed_state("pm_act_h", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("1080", window, cx);
                st
            });
            let pm_act_fps_inp: Entity<InputState> = window.use_keyed_state("pm_act_fps", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("30", window, cx);
                st
            });
            let pm_act_dur_inp: Entity<InputState> = window.use_keyed_state("pm_act_dur", cx, |window, cx| {
                let mut st = InputState::new(window, cx);
                st.set_value("5.0", window, cx);
                st
            });

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
            let s_new_custom = self.state.clone();
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
                                .w(px(580.0))
                                .max_h(px(640.0))
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
                                                    v_flex()
                                                        .gap_2()
                                                        .p_2()
                                                        .rounded_sm()
                                                        .bg(cx.theme().muted.opacity(0.3))
                                                        .border_1()
                                                        .border_color(cx.theme().border)
                                                        .child(
                                                            h_flex()
                                                                .gap_1p5()
                                                                .items_center()
                                                                .text_xs()
                                                                .child(div().font_semibold().text_color(cx.theme().foreground).child("+ Quick Presets:"))
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
                                                        .child(
                                                            h_flex()
                                                                .gap_1p5()
                                                                .items_center()
                                                                .text_xs()
                                                                .child(div().text_color(cx.theme().muted_foreground).child("Custom:"))
                                                                .child(Input::new(&pm_name_inp).id("pm_custom_name_input").w(px(100.)))
                                                                .child(Input::new(&pm_w_inp).id("pm_custom_w_input").w(px(55.)))
                                                                .child(div().text_color(cx.theme().muted_foreground).child("×"))
                                                                .child(Input::new(&pm_h_inp).id("pm_custom_h_input").w(px(55.)))
                                                                .child(div().text_color(cx.theme().muted_foreground).child("fps:"))
                                                                .child(Input::new(&pm_fps_inp).id("pm_custom_fps_input").w(px(45.)))
                                                                .child(div().text_color(cx.theme().muted_foreground).child("dur:"))
                                                                .child(Input::new(&pm_dur_inp).id("pm_custom_dur_input").w(px(45.)))
                                                                .child(
                                                                    div()
                                                                        .id("pm_create_custom")
                                                                        .test_support()
                                                                        .cursor_pointer()
                                                                        .px_2()
                                                                        .py_1()
                                                                        .rounded_sm()
                                                                        .bg(cx.theme().primary)
                                                                        .text_color(cx.theme().primary_foreground)
                                                                        .font_semibold()
                                                                        .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                                                        .on_mouse_down(MouseButton::Left, {
                                                                            let a = a_new.clone();
                                                                            let p_name = pm_name_inp.clone();
                                                                            let p_w = pm_w_inp.clone();
                                                                            let p_h = pm_h_inp.clone();
                                                                            let p_fps = pm_fps_inp.clone();
                                                                            let p_dur = pm_dur_inp.clone();
                                                                            move |_event, _window, cx| {
                                                                                let name = p_name.read(cx).value().trim().to_string();
                                                                                let w_val: u32 = p_w.read(cx).value().trim().parse().unwrap_or(1920);
                                                                                let h_val: u32 = p_h.read(cx).value().trim().parse().unwrap_or(1080);
                                                                                let fps_val: f64 = p_fps.read(cx).value().trim().parse().unwrap_or(30.0);
                                                                                let dur_val: f64 = p_dur.read(cx).value().trim().parse().unwrap_or(10.0);
                                                                                s_new_custom.update(cx, |s, cx| {
                                                                                    let name_str = if name.trim().is_empty() {
                                                                                        let num = s.project.compositions.len() + 1;
                                                                                        format!("Comp {num}")
                                                                                    } else {
                                                                                        name
                                                                                    };
                                                                                    let _ = s.create_composition(&name_str, w_val, h_val, fps_val, dur_val);
                                                                                    cx.notify();
                                                                                });
                                                                                a.update(cx, |_this, cx| cx.notify());
                                                                            }
                                                                        })
                                                                        .child("+ Create")
                                                                )
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
                                            let s_apply_cust = self.state.clone();
                                            let a_upd = cx.entity().clone();

                                            let (cur_w, cur_h, cur_fps, cur_dur) = active_comp.as_ref().map(|c| (c.width, c.height, c.frame_rate, c.duration.seconds())).unwrap_or((1920, 1080, 30.0, 5.0));

                                            let act_w_clone = pm_act_w_inp.clone();
                                            let act_h_clone = pm_act_h_inp.clone();
                                            let act_fps_clone = pm_act_fps_inp.clone();
                                            let act_dur_clone = pm_act_dur_inp.clone();

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
                                                        .child(div().text_color(cx.theme().muted_foreground).child("Quick Resolution"))
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
                                                        .child(div().text_color(cx.theme().muted_foreground).child("Quick Frame Rate"))
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
                                                        .child(div().text_color(cx.theme().muted_foreground).child("Quick Duration"))
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
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_xs()
                                                        .pt_1()
                                                        .border_t_1()
                                                        .border_color(cx.theme().border)
                                                        .child(
                                                            h_flex()
                                                                .gap_1p5()
                                                                .items_center()
                                                                .child(div().text_color(cx.theme().muted_foreground).child("Custom:"))
                                                                .child(Input::new(&act_w_clone).id("pm_act_w_input").w(px(55.)))
                                                                .child(div().text_color(cx.theme().muted_foreground).child("×"))
                                                                .child(Input::new(&act_h_clone).id("pm_act_h_input").w(px(55.)))
                                                                .child(div().text_color(cx.theme().muted_foreground).child("fps:"))
                                                                .child(Input::new(&act_fps_clone).id("pm_act_fps_input").w(px(45.)))
                                                                .child(div().text_color(cx.theme().muted_foreground).child("dur:"))
                                                                .child(Input::new(&act_dur_clone).id("pm_act_dur_input").w(px(45.)))
                                                        )
                                                        .child(
                                                            div()
                                                                .id("pm_apply_custom_settings")
                                                                .test_support()
                                                                .cursor_pointer()
                                                                .px_2()
                                                                .py_1()
                                                                .rounded_sm()
                                                                .bg(cx.theme().primary)
                                                                .text_color(cx.theme().primary_foreground)
                                                                .font_semibold()
                                                                .hover(|s| s.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                                                                .on_mouse_down(MouseButton::Left, {
                                                                    let a = a_upd.clone();
                                                                    let w_in = act_w_clone.clone();
                                                                    let h_in = act_h_clone.clone();
                                                                    let fps_in = act_fps_clone.clone();
                                                                    let dur_in = act_dur_clone.clone();
                                                                    move |_event, _window, cx| {
                                                                        let w_val: u32 = w_in.read(cx).value().trim().parse().unwrap_or(cur_w);
                                                                        let h_val: u32 = h_in.read(cx).value().trim().parse().unwrap_or(cur_h);
                                                                        let fps_val: f64 = fps_in.read(cx).value().trim().parse().unwrap_or(cur_fps);
                                                                        let dur_val: f64 = dur_in.read(cx).value().trim().parse().unwrap_or(cur_dur);
                                                                        s_apply_cust.update(cx, |s, cx| {
                                                                            s.update_project_settings("", w_val, h_val, fps_val, dur_val);
                                                                            cx.notify();
                                                                        });
                                                                        a.update(cx, |_this, cx| cx.notify());
                                                                    }
                                                                })
                                                                .child("Apply")
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
        if self.show_export {
            let dlg_w = 440.0f32;
            let dlg_h = 330.0f32;
            let dlg_pos = point(
                px((vw_f - dlg_w).max(8.0) / 2.0),
                px((vh_f - dlg_h).max(8.0) / 2.0),
            );
            let a_close = cx.entity().clone();
            let a_fmt = cx.entity().clone();
            let a_go = cx.entity().clone();
            let s_go = self.state.clone();
            let formats = export::ExportFormat::all();
            let cur_fmt = self.export_format_idx.min(formats.len().saturating_sub(1));
            let cur_format = formats[cur_fmt];
            let comp = self.state.read(cx).active_composition().cloned();
            let comp_meta = match &comp {
                Some(c) => format!(
                    "{} · {}×{} · {:.2} fps · {} frames ({:.2}s)",
                    c.name,
                    c.width,
                    c.height,
                    c.frame_rate,
                    c.duration.frames(),
                    c.duration.seconds()
                ),
                None => "No composition open".to_string(),
            };
            let ffmpeg_ok = export::find_ffmpeg().is_some();
            // Format tiles laid out 3 + 2; one test id per format
            // (`export_format_0` … `export_format_4`).
            let mut tile_grid = v_flex().gap_1().w_full();
            for row in 0..2 {
                let mut line = h_flex().gap_1().w_full();
                for col in 0..3 {
                    let i = row * 3 + col;
                    if i >= formats.len() {
                        line = line.child(div().flex_1());
                        continue;
                    }
                    let f = formats[i];
                    let is_cur = i == cur_fmt;
                    let a_pick = a_fmt.clone();
                    line = line.child(
                        div()
                            .id(SharedString::from(format!("export_format_{i}")))
                            .test_support()
                            .flex_1()
                            .cursor_pointer()
                            .px_2()
                            .py_1p5()
                            .rounded_sm()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(if is_cur {
                                cx.theme().primary
                            } else {
                                cx.theme().secondary
                            })
                            .text_color(if is_cur {
                                cx.theme().primary_foreground
                            } else {
                                cx.theme().foreground
                            })
                            .text_xs()
                            .child(f.label())
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                a_pick.update(cx, |this, cx| {
                                    if !this.export_busy {
                                        this.export_format_idx = i;
                                        cx.notify();
                                    }
                                });
                            }),
                    );
                }
                tile_grid = tile_grid.child(line);
            }
            let progress_line = match self.export_progress {
                Some((done, total)) => {
                    let pct = if total > 0 {
                        (done as f32 / total as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    v_flex()
                        .gap_1()
                        .w_full()
                        .child(
                            div()
                                .id("export_progress_label")
                                .test_support()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Rendering… {done}/{total} frames")),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(6.))
                                .rounded_sm()
                                .bg(cx.theme().secondary)
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_sm()
                                        .bg(cx.theme().primary)
                                        .w(px((dlg_w - 32.0) * pct)),
                                ),
                        )
                        .into_any_element()
                }
                None => div().into_any_element(),
            };
            let render_label = if self.export_busy {
                "Rendering…"
            } else {
                "Render"
            };
            let render_enabled = comp.is_some() && !self.export_busy;
            let a_go2 = a_go.clone();
            let s_go2 = s_go.clone();
            dialogs.push(
                deferred(
                    Positioner::corner(Anchor::TopLeft, dlg_pos)
                        .margin(px(8.))
                        .occlude()
                        .child(
                            div()
                                .id("export_dialog")
                                .test_support()
                                .w(px(dlg_w))
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
                                            v_flex()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .font_bold()
                                                        .text_sm()
                                                        .child("Export Composition"),
                                                )
                                                .child(
                                                    div()
                                                        .id("export_comp_meta")
                                                        .test_support()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(comp_meta),
                                                ),
                                        )
                                        .child(tile_grid)
                                        .child(
                                            div()
                                                .id("export_format_hint")
                                                .test_support()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(cur_format.hint()),
                                        )
                                        .child(if cur_format.is_movie() && !ffmpeg_ok {
                                            div()
                                                .id("export_ffmpeg_warning")
                                                .test_support()
                                                .text_xs()
                                                .text_color(cx.theme().warning)
                                                .child(
                                                    "ffmpeg was not found, so frames will be written as a PNG sequence instead.",
                                                )
                                                .into_any_element()
                                        } else {
                                            div().into_any_element()
                                        })
                                        .child(progress_line)
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .justify_end()
                                                .w_full()
                                                .child(
                                                    div()
                                                        .id("export_cancel_btn")
                                                        .test_support()
                                                        .cursor_pointer()
                                                        .px_3()
                                                        .py_1()
                                                        .rounded_sm()
                                                        .border_1()
                                                        .border_color(cx.theme().border)
                                                        .text_xs()
                                                        .text_color(cx.theme().foreground)
                                                        .hover(|s| s.bg(cx.theme().secondary))
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            move |_event, _window, cx| {
                                                                a_close.update(cx, |this, cx| {
                                                                    this.show_export = false;
                                                                    cx.notify();
                                                                });
                                                            },
                                                        )
                                                        .child("Cancel"),
                                                )
                                                .child(
                                                    div()
                                                        .id("export_render_btn")
                                                        .test_support()
                                                        .cursor_pointer()
                                                        .px_3()
                                                        .py_1()
                                                        .rounded_sm()
                                                        .bg(if render_enabled {
                                                            cx.theme().primary
                                                        } else {
                                                            cx.theme().muted
                                                        })
                                                        .text_color(if render_enabled {
                                                            cx.theme().primary_foreground
                                                        } else {
                                                            cx.theme().muted_foreground
                                                        })
                                                        .text_xs()
                                                        .font_semibold()
                                                        .child(render_label)
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            move |_event, _window, cx| {
                                                                if a_go2.read(cx).export_busy {
                                                                    return;
                                                                }
                                                                let (a, s) =
                                                                    (a_go2.clone(), s_go2.clone());
                                                                request_export_run(&a, &s, cx);
                                                            },
                                                        ),
                                                ),
                                        )
                                ),
                        ),
                )
                .into_any_element(),
            );
        }
        if let Some(toast_text) = self.state.read(cx).property_link_toast.clone() {
            let s_dismiss = self.state.clone();
            let toast_pos = point(px((vw_f - 380.0).max(8.0) / 2.0), px(vh_f - 60.0));
            dialogs.push(
                deferred(
                    Positioner::corner(Anchor::TopLeft, toast_pos)
                        .margin(px(8.))
                        .occlude()
                        .child(
                            h_flex()
                                .id("property_link_toast")
                                .test_support()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_1p5()
                                .rounded_full()
                                .bg(rgb(0x1e293b))
                                .border_1()
                                .border_color(rgb(0x3b82f6))
                                .shadow_lg()
                                .child(
                                    div()
                                        .w(px(14.))
                                        .h(px(14.))
                                        .items_center()
                                        .justify_center()
                                        .text_color(rgb(0x60a5fa))
                                        .child(gpui_kit::assets::IconName::Link),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_medium()
                                        .text_color(rgb(0xf8fafc))
                                        .child(toast_text),
                                )
                                .child(
                                    div()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(rgb(0x94a3b8))
                                        .hover(|s| s.text_color(rgb(0xffffff)))
                                        .child("✕")
                                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                            s_dismiss.update(cx, |s, cx| {
                                                s.property_link_toast = None;
                                                cx.notify();
                                            });
                                        }),
                                ),
                        ),
                )
                .into_any_element(),
            );
        }
        // The spline/graph editor needs headroom its chrome (header +
        // legend + ruler + 180px plot + easing row) doesn't fit in the
        // 260px lanes strip, so it gets a taller strip instead of clipping.
        let strip_h = if self.state.read(cx).spline_editor_open {
            px(400.)
        } else {
            px(260.)
        };
        let main_workspace = v_flex()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .child(div().flex_1().size_full().child(self.dock_area.clone()))
            .child(
                div()
                    .w_full()
                    .h(strip_h)
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
            .into_any_element();

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
                move |_: &TogglePlayback, window, cx| {
                    // Typing in any text input (layer text, font/size
                    // fields, scrub editors) must not toggle playback.
                    if window.has_focused_input(cx) {
                        return;
                    }
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
            .on_key_down(move |event, window, cx| {
                // Space with a focused text input types a space; it must
                // not reach the playback toggle below.
                let typing_in_input = window.has_focused_input(cx);
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
                } else if ctrl && key == "n" {
                    let (a, s) = (app_key.clone(), state_key.clone());
                    request_new_project(&a, &s, cx);
                    return;
                } else if ctrl && key == "k" {
                    app_key.update(cx, |this, cx| {
                        this.open_menu = None;
                        this.show_project_manager = true;
                        cx.notify();
                    });
                    return;
                } else if ctrl && key == "m" {
                    app_key.update(cx, |this, cx| {
                        this.open_menu = None;
                        this.show_export = true;
                        this.export_progress = None;
                        cx.notify();
                    });
                    return;
                } else if ctrl && key == "s" && mods.shift {
                    let (a, s) = (app_key.clone(), state_key.clone());
                    request_save_project_as(&a, &s, cx);
                    return;
                } else if ctrl && key == "s" && !mods.shift {
                    let (a, s) = (app_key.clone(), state_key.clone());
                    request_save_project(&a, &s, cx);
                    return;
                } else if ctrl && key == "o" {
                    let (a, s) = (app_key.clone(), state_key.clone());
                    request_open_project(&a, &s, cx);
                    return;
                } else if ctrl && key == "i" {
                    let (a, s) = (app_key.clone(), state_key.clone());
                    request_import_file(&a, &s, cx);
                    return;
                } else if ctrl && key == "q" {
                    request_quit();
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
                        this.show_export = false;
                        cx.notify();
                    });
                    state_key.update(cx, |s, cx| {
                        // Esc also exits mask node-edit mode.
                        s.set_active_mask_edit(None);
                        cx.notify();
                    });
                    // Esc clears the spline/timeline key selection.
                    let panels = app_key.read(cx).panels().clone();
                    panels.timeline.update(cx, |tl, cx| {
                        if !tl.graph_sel_keys.is_empty() {
                            tl.graph_sel_keys.clear();
                            cx.notify();
                        }
                    });
                    return;
                }
                if key == "space" || key == " " {
                    if !typing_in_input {
                        state_key.update(cx, |s, cx| {
                            s.toggle_playback();
                            cx.notify();
                        });
                    }
                } else if key == "delete" || key == "backspace" {
                    if !typing_in_input {
                        // Spline marquee selection deletes keys first (AE).
                        let sel_keys = {
                            let panels = app_key.read(cx).panels().clone();
                            panels.timeline.read(cx).graph_sel_keys.clone()
                        };
                        let spline_open = state_key.read(cx).spline_editor_open;
                        if spline_open && !sel_keys.is_empty() {
                            state_key.update(cx, |s, cx| {
                                s.remove_graph_keys(&sel_keys);
                                cx.notify();
                            });
                            let panels = app_key.read(cx).panels().clone();
                            panels.timeline.update(cx, |tl, cx| {
                                tl.graph_sel_keys.clear();
                                cx.notify();
                            });
                        } else {
                            state_key.update(cx, |s, cx| {
                                if let Some((lid, mid)) = s.active_mask_edit.take() {
                                    let _ = s.remove_layer_mask(&lid, &mid);
                                } else {
                                    let _ = s.delete_selected_layer();
                                }
                                cx.notify();
                            });
                        }
                    }
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
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--render-preview" || a == "--export-demo-frame") {
        let out_path = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("docs/images/composition_preview.png"));
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let state = EditorState::new();
        state.export_frame_as_png(&out_path).expect("export frame as png");
        println!("Exported composition preview to {}", out_path.display());
        return;
    }

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
                let state = cx.new(|_| EditorState::blank());
                let view = cx.new(|cx| AppView::new_with_state(state, window, cx));
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
    use gpui_kit::{px, size, Action as _, AppContext as _, Entity, SharedString, TestAppContext};

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
                // Left/Center/Right docks exist; the timeline is a
                // full-width strip below the dock area, so no Bottom dock.
                assert!(dock.has_dock(DockPlacement::Left));
                assert!(dock.has_dock(DockPlacement::Right));
                assert!(!dock.has_dock(DockPlacement::Bottom));

                // Verify the dock trees are present
                assert!(dock.layout(DockPlacement::Left).is_some());
                assert!(dock.layout(DockPlacement::Center).is_some());
                assert!(dock.layout(DockPlacement::Right).is_some());
                assert!(dock.layout(DockPlacement::Bottom).is_none());

                // Verify docks are open
                assert!(dock.is_dock_open(DockPlacement::Left));
                assert!(dock.is_dock_open(DockPlacement::Right));
                assert!(!dock.is_dock_open(DockPlacement::Bottom));

                // Verify docks are non-empty
                assert!(!dock.is_empty(DockPlacement::Left, cx));
                assert!(!dock.is_empty(DockPlacement::Center, cx));
                assert!(!dock.is_empty(DockPlacement::Right, cx));

                // Verify configured dock sizes (responsive: fitted to the
                // window within sane rails, not fixed px).
                let left = dock.dock_size(DockPlacement::Left).expect("left size");
                let right = dock.dock_size(DockPlacement::Right).expect("right size");
                assert!(dock.dock_size(DockPlacement::Bottom).is_none());
                assert!((200.0..=320.0).contains(&(left / px(1.0))), "left {left:?}");
                assert!((220.0..=340.0).contains(&(right / px(1.0))), "right {right:?}");
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

                // 5. Timeline panel lives outside the dock area (full-width
                // strip below it), so it is not attached to any dock...
                assert!(dock.panel(timeline_id).is_none());
                // ...but the panel entity is still owned by the view.
                let _ = view.panels().timeline.clone();
            });
        });
    }

    #[gpui_kit::test]
    fn test_project_tabs_filter_and_grid_view(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;

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

        let _app_view = app_view_entity.expect("AppView created");

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Verify core Project Panel and its table container exist
            assert!(window.find("project_panel").visible());
            assert!(window.find("project_assets").visible());
            assert!(window.find("project_search_input").visible());

            // Check view toggles and sort buttons
            assert!(window.find("project_view_list").visible());
            assert!(window.find("project_view_grid").visible());
            assert!(window.find("project_sort_name").visible());
            assert!(window.find("project_sort_type").visible());
            assert!(window.find("project_sort_size").visible());
            assert!(window.find("project_sort_framerate").visible());

            // Active comp row is mounted
            assert!(window.find("project_comp_item_comp_main").visible());

            // Add solid button creates a solid item row
            assert!(window.find("add_solid_button").visible());
            window.click("add_solid_button", cx);
            window.render_frame(cx);

            // Switch to grid view and back to list view
            window.click("project_view_grid", cx);
            window.render_frame(cx);
            window.click("project_view_list", cx);
            window.render_frame(cx);
            assert!(window.find("project_comp_item_comp_main").visible());
        })
        .expect("update_window failed");
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
    fn test_effects_search_filters_browser_list(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;

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
            let effects_id = PanelId::from(panels.effects.entity_id());
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(effects_id, window, cx);
            });
            window.render_frame(cx);

            // Search field is present; categories start collapsed (no rows).
            assert!(window.find("effects_search_input").visible());
            assert!(window.try_find("effect_item_blur").is_none());
            // Every category defaults to collapsed — including ones added
            // after the legacy key list (Light, Stylize, Noise, ...).
            assert!(window.try_find("effect_item_glow").is_none());

            // A query auto-opens matching categories and filters rows.
            panels.effects.update(cx, |this, cx| {
                this.search_query = "blur".to_string();
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find("effect_item_blur").visible());
            assert!(window.try_find("effect_item_glow").is_none());

            // Clearing the query restores the collapsed browser.
            panels.effects.update(cx, |this, cx| {
                this.search_query = String::new();
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.try_find("effect_item_blur").is_none());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_effects_browser_pills_counts_and_gpu_badges(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;

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
            let effects_id = PanelId::from(panels.effects.entity_id());
            dock_area.update(cx, |dock, cx| {
                dock.select_panel(effects_id, window, cx);
            });
            window.render_frame(cx);

            // Filter pills are present; All is the default.
            assert!(window.find("effects_filter_all").visible());
            assert!(window.find("effects_filter_gpu").visible());

            // Enabling the GPU filter shows only GPU-backed rows:
            // blur and tint keep their badges, a CPU-only row disappears.
            panels.effects.update(cx, |this, cx| {
                this.gpu_only = true;
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find("effect_item_blur").visible());
            assert!(window.find("effect_gpu_blur").visible());
            assert!(window.find("effect_item_tint").visible());
            assert!(window.find("effect_gpu_tint").visible());
            assert!(window.try_find("effect_item_puppet").is_none());
            // Resampling ports badge too (stock wave): narrow with a query
            // since the full GPU-filtered list scrolls past the fold.
            panels.effects.update(cx, |this, cx| {
                this.search_query = "wave".to_string();
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find("effect_item_wave").visible());
            assert!(window.find("effect_gpu_wave").visible());

            // Category headers carry live counts of their listed rows.
            assert!(window.find("effect_category_count_distort").visible());

            // Back to All plus a query shows the CPU-only row with no badge.
            panels.effects.update(cx, |this, cx| {
                this.gpu_only = false;
                this.search_query = "puppet".to_string();
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find("effect_item_puppet").visible());
            assert!(window.try_find("effect_gpu_puppet").is_none());
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
    fn test_space_in_focused_text_input_does_not_toggle_playback(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;

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
            view.state().update(cx, |s, _| {
                let id = s.add_text_layer("Hi", None).unwrap();
                s.select_layer(Some(id.clone()));
            })
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Focus the inspector text field: playback toggles (both the
            // action and the key fallback) must stand down while typing.
            window.click(SharedString::from("text_content_input"), cx);
            window.render_frame(cx);
            window.dispatch_action(crate::TogglePlayback.boxed_clone(), cx);
            window.dispatch_keystroke(gpui_kit::Keystroke::parse("space").unwrap(), cx);
        })
        .expect("update_window failed");

        cx.run_until_parked();
        assert!(!app_view.read_with(cx, |view, cx| view.state().read(cx).is_playing));

        // Unfocused, the same inputs toggle playback as before.
        cx.update_window(handle.into(), |_, window, cx| {
            let focus_handle = app_view.read(cx).focus_handle().clone();
            window.focus(&focus_handle, cx);
            window.render_frame(cx);
            window.dispatch_action(crate::TogglePlayback.boxed_clone(), cx);
        })
        .expect("update_window failed");

        cx.run_until_parked();
        assert!(app_view.read_with(cx, |view, cx| view.state().read(cx).is_playing));
    }

    #[test]
    fn test_canvas_scale_includes_zoom() {
        // Regression: drag handlers recomputed a zoom-less fit, offsetting
        // mask commits by 1/zoom. Render, overlay, and drags share this.
        let viewport = Some((512.0, 288.0));
        let base = crate::panels::canvas_scale(viewport, None, 1920.0, 1080.0);
        let zoomed = crate::panels::canvas_scale(viewport, Some(2.0), 1920.0, 1080.0);
        assert!((zoomed - base * 2.0).abs() < 1e-5, "{base} {zoomed}");
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

    #[test]
    fn test_bipolar_effect_params_scrub_below_zero() {
        use project::{EffectType, StockPlugin};
        let mut state = EditorState::new();
        // Stock, bipolar via descriptor: color balance shadows must reach
        // the CMY side (regression: scrub floored everything at 0).
        let cb = state
            .add_effect_to_selected_layer(EffectType::Stock {
                plugin: StockPlugin::ColorBalance,
                params: StockPlugin::ColorBalance
                    .descriptor()
                    .params
                    .iter()
                    .map(|p| project::Property::new(p.label, p.default))
                    .collect(),
                colors: Vec::new(),
            })
            .expect("added color balance");
        state
            .nudge_effect_param(&cb, "shadows_cyan_red", -50.0)
            .expect("nudged below zero");
        let layer = state.selected_layer().expect("layer selected");
        let val = layer.effects.iter().find(|e| e.id == cb).unwrap()
            .get_param_property("shadows_cyan_red").unwrap().value;
        assert!((val + 50.0).abs() < 1e-4, "must reach -50, got {val}");
        // Stock clamp still enforced at the descriptor min.
        state
            .nudge_effect_param(&cb, "shadows_cyan_red", -100.0)
            .expect("nudged past min");
        let layer = state.selected_layer().expect("layer selected");
        let val = layer.effects.iter().find(|e| e.id == cb).unwrap()
            .get_param_property("shadows_cyan_red").unwrap().value;
        assert!((val + 100.0).abs() < 1e-4, "must clamp at -100, got {val}");
        // Bespoke bipolar: brightness/contrast ±100.
        let bc = state
            .add_effect_to_selected_layer(EffectType::brightness_contrast(0.0, 0.0))
            .expect("added b/c");
        state
            .nudge_effect_param(&bc, "brightness", -30.0)
            .expect("nudged brightness negative");
        let layer = state.selected_layer().expect("layer selected");
        if let EffectType::BrightnessContrast { brightness, .. } =
            &layer.effects.iter().find(|e| e.id == bc).unwrap().effect_type
        {
            assert!((brightness.value + 30.0).abs() < 1e-4);
        } else {
            panic!("expected BrightnessContrast");
        }
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
            // Rows below the fold stay mounted but report invisible while
            // culled from paint: assert presence, not visibility.
            assert!(window.try_find("effect_item_brightness_contrast").is_some());
            assert!(window.try_find("effect_item_levels").is_some());
            assert!(window.try_find("effect_item_chroma_key").is_some());
            assert!(window.try_find("effect_item_luma_key").is_some());
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
            assert!(window.try_find("effect_item_text_outline").is_some());

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
            // Cards start collapsed: expand via disclosure first.
            window.click(SharedString::from(format!("effect_disclosure_{eff_id}")), cx);
            window.render_frame(cx);
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

    #[gpui_kit::test]
    fn test_graph_key_selection_shows_compact_easing_bar(cx: &mut TestAppContext) {
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
    fn test_set_graph_key_easing_writes_interp_and_tangents() {
        use crate::state::{EditorState, KeyEase};
        use project::KeyframeInterpolation;

        let mut state = EditorState::new();
        // Fixture: accent position.x keys at 0s/2s/4s.
        let at_s = 2.0;
        let read_key = |s: &EditorState| {
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let kf = layer
                .transform
                .position
                .keyframes()
                .iter()
                .find(|k| (k.time_seconds() - at_s).abs() < 1e-6)
                .unwrap()
                .clone();
            (kf.interpolation, kf.in_tangent, kf.out_tangent)
        };
        assert!(state.set_graph_key_easing("layer_accent", "transform.position.x", at_s, KeyEase::EaseIn));
        let (interp, in_tan, out_tan) = read_key(&state);
        assert_eq!(interp, KeyframeInterpolation::Bezier);
        assert_eq!(out_tan.unwrap(), project::KeyframeTangent::new(0.42, 0.0));
        assert_eq!(in_tan.unwrap(), project::KeyframeTangent::new(1.0, 1.0));
        assert!(state.set_graph_key_easing("layer_accent", "transform.position.x", at_s, KeyEase::EaseOut));
        let (interp, in_tan, out_tan) = read_key(&state);
        assert_eq!(interp, KeyframeInterpolation::Bezier);
        assert_eq!(out_tan.unwrap(), project::KeyframeTangent::new(0.0, 0.0));
        assert_eq!(in_tan.unwrap(), project::KeyframeTangent::new(0.58, 1.0));
        assert!(state.set_graph_key_easing("layer_accent", "transform.position.x", at_s, KeyEase::Linear));
        let (interp, _, _) = read_key(&state);
        assert_eq!(interp, KeyframeInterpolation::Linear);
        assert!(state.set_graph_key_easing("layer_accent", "transform.position.x", at_s, KeyEase::Hold));
        let (interp, in_tan, out_tan) = read_key(&state);
        assert_eq!(interp, KeyframeInterpolation::Hold);
        assert!(in_tan.is_none() && out_tan.is_none());
        // Unknown key time resolves false.
        assert!(!state.set_graph_key_easing("layer_accent", "transform.position.x", 99.0, KeyEase::Linear));
    }

    #[test]
    fn test_graph_series_includes_all_effect_scalars() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        // Text layer + split animator with animated progress (none of the
        // animator's scalars were in the old legacy probe list).
        let tid = state.add_text_layer("Hello", None).unwrap();
        state.select_layer(Some(tid.clone()));
        let split = state
            .add_effect_to_selected_layer(project::EffectType::text_split_animator())
            .unwrap();
        let split_path = format!("effect:{split}:progress");
        state.toggle_layer_keyframe_at_current_time(&tid, &split_path);
        // Warp cols was likewise invisible to the old list.
        state.select_layer(Some("layer_accent".to_string()));
        let warp = state
            .add_effect_to_selected_layer(project::EffectType::warp(0.0, 1.0))
            .unwrap();
        let cols_path = format!("effect:{warp}:cols");
        state.toggle_layer_keyframe_at_current_time("layer_accent", &cols_path);

        // Both surface as graph series with readable keys.
        assert!(state.graph_series(&tid).iter().any(|s| s.path == split_path));
        assert!(!state.graph_key_times(&tid, &split_path).is_empty());
        assert!(state.graph_series("layer_accent").iter().any(|s| s.path == cols_path));
        assert!(!state.graph_key_times("layer_accent", &cols_path).is_empty());

        // Color params stay timeline-only: the graph plots f32 curves.
        let tint = state
            .add_effect_to_selected_layer(project::EffectType::tint(
                project::Color::BLACK,
                project::Color::WHITE,
                100.0,
            ))
            .unwrap();
        state.toggle_layer_keyframe_at_current_time(
            "layer_accent",
            &format!("effect:{tint}:map_black"),
        );
        assert!(
            !state.graph_series("layer_accent").iter().any(|s| s.path.contains("map_black")),
            "color params must not become graph series"
        );
    }

    #[test]
    fn test_remove_graph_keys_deletes_selection_at_once() {        use crate::state::EditorState;

        let mut state = EditorState::new();
        // Fixture: accent position keys at 0s/2s/4s.
        let before = state.graph_key_times("layer_accent", "transform.position.x");
        assert!(before.len() >= 3);
        let doomed: Vec<(String, String, f64)> = before
            .iter()
            .take(2)
            .map(|t| ("layer_accent".to_string(), "transform.position.x".to_string(), *t))
            .collect();
        assert_eq!(state.remove_graph_keys(&doomed), 2);
        let after = state.graph_key_times("layer_accent", "transform.position.x");
        assert_eq!(after.len(), before.len() - 2);
        assert_eq!(state.remove_graph_keys(&[]), 0);
    }

    #[gpui_kit::test]
    fn test_graph_marquee_selects_and_delete_clears_keys(cx: &mut TestAppContext) {
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
        // Drag a full-plot marquee: selects every visible key in range.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("graph_plot").bounds();
            let w = bounds.size.width / px(1.0);
            let h = bounds.size.height / px(1.0);
            let from = bounds.origin + point(px(w * 0.02), px(h * 0.02));
            let to = bounds.origin + point(px(w * 0.98), px(h * 0.98));
            window.drag(from, to, cx);
            window.render_frame(cx);
        })
        .expect("update_window failed");
        let sel = app_view.read_with(cx, |view, cx| {
            view.panels().timeline.read(cx).graph_sel_keys.clone()
        });
        assert!(sel.len() >= 3, "marquee must catch the accent keys: {sel:?}");
        assert!(sel.iter().any(|s| s.1 == "transform.position.x" && (s.2 - 2.0).abs() < 0.05));
        // Delete removes exactly the selection (layer survives). Position
        // keys are shared Vec2 structs: removing x@2 also takes y@2, so
        // only the out-of-marquee 0s key remains.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.press("delete", cx);
        })
        .expect("update_window failed");
        cx.run_until_parked();
        assert!(app_view.read_with(cx, |view, cx| {
            view.panels().timeline.read(cx).graph_sel_keys.is_empty()
        }));
        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            s.active_composition().unwrap().get_layer("layer_accent").is_some()
                && s.graph_key_times("layer_accent", "transform.position.x") == vec![0.0]
        }));
    }

    #[gpui_kit::test]
    fn test_graph_marquee_multi_drag_moves_selection(cx: &mut TestAppContext) {
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
        let times_of = |app_view: &Entity<AppView>, cx: &TestAppContext| {
            app_view.read_with(cx, |view, cx| {
                view.state().read(cx).graph_key_times("layer_accent", "transform.position.x")
            })
        };
        assert_eq!(times_of(&app_view, cx).len(), 3);
        // Full-plot marquee selects everything, then a rightward drag of
        // one diamond carries the whole selection later in time.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("graph_plot").bounds();
            let w = bounds.size.width / px(1.0);
            let h = bounds.size.height / px(1.0);
            window.drag(
                bounds.origin + point(px(w * 0.02), px(h * 0.02)),
                bounds.origin + point(px(w * 0.98), px(h * 0.98)),
                cx,
            );
            window.render_frame(cx);
            let snap = window.find("graph_key_layer_accent_transform_position_x_2000");
            let from = snap.bounds().center();
            window.drag(from, from + point(px(60.0), px(0.0)), cx);
            window.render_frame(cx);
        })
        .expect("update_window failed");
        let after = times_of(&app_view, cx);
        assert_eq!(after.len(), 3, "multi-drag must not add/remove keys");
        // Marquee caught the 2s + 4s keys (0s sits outside the 2% margin):
        // both ride the drag later, the 0s key stays put.
        assert!(
            after.iter().any(|t| (*t - 0.0).abs() < 1e-6),
            "unselected 0s key untouched: {after:?}"
        );
        assert!(
            after.iter().any(|t| (*t - 2.43).abs() < 0.1),
            "dragged 2s key moved: {after:?}"
        );
        assert!(
            after.iter().any(|t| (*t - 4.43).abs() < 0.1),
            "selected 4s follower moved: {after:?}"
        );
    }

    #[gpui_kit::test]
    fn test_lane_marquee_selects_prop_range_and_escape_clears(cx: &mut TestAppContext) {
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
        // Lanes mode, accent expanded with Transform twirled down.
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_accent".to_string()));
                cx.notify();
            });
            view.panels().timeline.update(cx, |tl, cx| {
                tl.toggle_layer_expanded("layer_accent");
                tl.toggle_group_expanded("layer_accent:transform");
                cx.notify();
            });
        });
        // Drag across the position lane: catches the 2s + 4s keys only.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("tl_lane_layer_accent_transform_position").bounds();
            let w = bounds.size.width / px(1.0);
            let h = bounds.size.height / px(1.0);
            let from = bounds.origin + point(px(w * 0.3), px(h * 0.5));
            let to = bounds.origin + point(px(w * 0.9), px(h * 0.5));
            window.drag(from, to, cx);
            window.render_frame(cx);
        })
        .expect("update_window failed");
        let sel = app_view.read_with(cx, |view, cx| {
            view.panels().timeline.read(cx).graph_sel_keys.clone()
        });
        assert_eq!(sel.len(), 2, "lane range must catch 2s + 4s: {sel:?}");
        assert!(sel.iter().all(|s| s.0 == "layer_accent" && s.1 == "transform.position"));
        // Escape clears without touching keys.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.press("escape", cx);
        })
        .expect("update_window failed");
        cx.run_until_parked();
        assert!(app_view.read_with(cx, |view, cx| {
            view.panels().timeline.read(cx).graph_sel_keys.is_empty()
        }));
        assert_eq!(
            app_view.read_with(cx, |view, cx| {
                view.state().read(cx).graph_key_times("layer_accent", "transform.position.x").len()
            }),
            3
        );
    }

    #[gpui_kit::test]
    fn test_warp_lattice_overlay_drags_and_resets_pins(cx: &mut TestAppContext) {
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
        // Warp on the accent solid; selecting it shows the lattice.
        let warp_id = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_effect_to_selected_layer(project::EffectType::warp(0.0, 1.0))
                    .unwrap()
            })
        });
        let pin_id = SharedString::from(format!("warp_pin_layer_accent_{warp_id}_0"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(pin_id.clone()).visible());
            // Drag the top-left pin right: its x offset must follow.
            let from = window.find(pin_id.clone()).bounds().center();
            window.drag(from, from + point(px(60.0), px(0.0)), cx);
        })
        .expect("update_window failed");
        let (dx, dy) = app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&warp_id).unwrap();
            match &eff.effect_type {
                project::EffectType::Warp { pins, .. } => {
                    let p = pins[0];
                    (p.dx, p.dy)
                }
                _ => panic!("expected warp"),
            }
        });
        assert!(dx > 1.0, "pin follows rightward drag: dx={dx}");
        assert!(dy.abs() < dx.abs(), "flat drag stays flat: dx={dx} dy={dy}");
        // Right-click resets that pin to identity.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click(pin_id.clone(), cx);
        })
        .expect("update_window failed");
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&warp_id).unwrap();
            match &eff.effect_type {
                project::EffectType::Warp { pins, .. } => {
                    assert!(pins[0].is_identity(), "right-click resets the pin");
                }
                _ => panic!("expected warp"),
            }
        });
    }

    #[gpui_kit::test]
    fn test_corner_pin_overlay_drags_and_resets_corners(cx: &mut TestAppContext) {
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
        // CornerPin on the accent solid; selecting it shows the quad.
        let pin_id = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_effect_to_selected_layer(project::EffectType::Stock {
                    plugin: project::StockPlugin::CornerPin,
                    params: project::EffectType::stock_params(
                        project::StockPlugin::CornerPin,
                    ),
                    colors: project::EffectType::stock_colors(
                        project::StockPlugin::CornerPin,
                    ),
                })
                .unwrap()
            })
        });
        let dot_id = SharedString::from(format!("corner_pin_layer_accent_{pin_id}_2"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(dot_id.clone()).visible());
            // Clicking the dot must not steal selection to an underlying
            // layer (the dot can sit just outside its own layer's box).
            window.click(dot_id.clone(), cx);
            app_view.read_with(cx, |view, cx| {
                assert_eq!(
                    view.state().read(cx).selected_layer_id.as_deref(),
                    Some("layer_accent")
                );
            });
            // Drag the lower-right corner left: lr_x must shrink.
            let from = window.find(dot_id.clone()).bounds().center();
            window.drag(from, from + point(px(-60.0), px(0.0)), cx);
        })
        .expect("update_window failed");
        let lrx = app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&pin_id).unwrap();
            eff.get_param_property("lr_x").unwrap().value
        });
        assert!(lrx < 1.0, "corner follows leftward drag: lr_x={lrx}");
        // Right-click resets that corner to identity.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click(dot_id.clone(), cx);
        })
        .expect("update_window failed");
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&pin_id).unwrap();
            assert_eq!(eff.get_param_property("lr_x").unwrap().value, 1.0);
            assert_eq!(eff.get_param_property("lr_y").unwrap().value, 1.0);
        });
    }

    #[test]
    fn test_puppet_pins_keyframe_through_property_tracks() {
        use crate::state::EditorState;

        let mut state = EditorState::new();
        state.select_layer(Some("layer_accent".to_string()));
        let fx = state
            .add_effect_to_selected_layer(project::EffectType::puppet())
            .unwrap();
        // Declarations expose static sliders with no pins yet.
        let fields0: Vec<String> = state
            .active_composition()
            .unwrap()
            .get_layer("layer_accent")
            .unwrap()
            .get_effect(&fx)
            .unwrap()
            .declarations()
            .iter()
            .map(|d| d.field.clone())
            .collect();
        assert!(fields0.contains(&"expansion".to_string()));
        assert!(!fields0.iter().any(|f| f.starts_with("pin_")));

        // Add + move a pin.
        let idx = state.add_puppet_pin("layer_accent", &fx, 10.0, 20.0).unwrap();
        assert_eq!(idx, 0);
        state.move_puppet_pin_live("layer_accent", &fx, 0, 5.0, -3.0).unwrap();
        let layer = state.active_composition().unwrap().get_layer("layer_accent").unwrap();
        let eff = layer.get_effect(&fx).unwrap();
        // Dynamic per-pin declarations light up timeline lanes as usual.
        let fields: Vec<String> = eff.declarations().iter().map(|d| d.field.clone()).collect();
        assert!(fields.contains(&"pin_0_x".to_string()));
        assert!(fields.contains(&"pin_0_y".to_string()));
        assert!(eff.get_param_property("pin_0_x").unwrap().value == 5.0);

        // Keyframing a pin component shows up in the spline editor.
        state.toggle_layer_keyframe_at_current_time("layer_accent", &format!("effect:{fx}:pin_0_x"));
        assert!(!state.graph_key_times("layer_accent", &format!("effect:{fx}:pin_0_x")).is_empty());
        assert!(
            state.graph_series("layer_accent").iter().any(|s| s.path == format!("effect:{fx}:pin_0_x")),
            "pin tracks must reach the spline editor"
        );

        // Delete + out-of-range errors.
        state.remove_puppet_pin("layer_accent", &fx, 0).unwrap();
        assert!(state.remove_puppet_pin("layer_accent", &fx, 0).is_err());
        assert!(state.move_puppet_pin_live("layer_accent", &fx, 0, 0.0, 0.0).is_err());
    }

    #[gpui_kit::test]
    fn test_puppet_overlay_drags_and_deletes_pins(cx: &mut TestAppContext) {
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
        // Puppet on the accent solid with one centered pin.
        let puppet_id = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                let fx = s.add_effect_to_selected_layer(project::EffectType::puppet()).unwrap();
                s.add_puppet_pin("layer_accent", &fx, 150.0, 150.0).unwrap();
                fx
            })
        });
        let pin_id = SharedString::from(format!("puppet_pin_layer_accent_{puppet_id}_0"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(pin_id.clone()).visible());
            // Drag the pin right: its x offset must follow.
            let from = window.find(pin_id.clone()).bounds().center();
            window.drag(from, from + point(px(60.0), px(0.0)), cx);
        })
        .expect("update_window failed");
        let (dx, dy) = app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&puppet_id).unwrap();
            match &eff.effect_type {
                project::EffectType::Puppet { pins, .. } => {
                    (pins[0].dx.value, pins[0].dy.value)
                }
                _ => panic!("expected puppet"),
            }
        });
        assert!(dx > 1.0, "pin follows rightward drag: dx={dx}");
        assert!(dy.abs() < dx.abs(), "flat drag stays flat: dx={dx} dy={dy}");
        // Drag state fully releases (no stuck gesture leaks into later presses).
        app_view.read_with(cx, |view, cx| {
            view.panels().viewer.read_with(cx, |v, _| {
                assert!(v.puppet_drag.is_none());
                assert!(v.warp_drag.is_none());
                assert!(!v.is_dragging_canvas);
                assert!(!view.state().read(cx).preview_fast);
            })
        });
        // Right-click deletes the pin.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click(pin_id.clone(), cx);
        })
        .expect("update_window failed");
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&puppet_id).unwrap();
            match &eff.effect_type {
                project::EffectType::Puppet { pins, .. } => {
                    assert!(pins.is_empty(), "right-click deletes the pin");
                }
                _ => panic!("expected puppet"),
            }
        });
        // Double-click placement through the panel entry point (the same
        // logic the canvas press handler uses): first press arms, second
        // places on the picked puppet layer; elsewhere only re-arms.
        let (first, second) = app_view.update(cx, |view, cx| {
            view.panels().viewer.update(cx, |p, cx| {
                let first = p.puppet_double_click_at(-200.0, 0.0, 500.0, 300.0, cx);
                let second = p.puppet_double_click_at(-200.0, 0.0, 500.0, 300.0, cx);
                (first, second)
            })
        });
        assert!(!first, "first press only arms");
        assert!(second, "second press places the pin");
        let third = app_view.update(cx, |view, cx| {
            view.panels().viewer.update(cx, |p, cx| {
                p.puppet_double_click_at(900.0, 500.0, 100.0, 100.0, cx)
            })
        });
        assert!(!third, "press elsewhere only re-arms");
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&puppet_id).unwrap();
            match &eff.effect_type {
                project::EffectType::Puppet { pins, .. } => {
                    assert_eq!(pins.len(), 1, "double-click adds a pin");
                }
                _ => panic!("expected puppet"),
            }
        });
    }

    #[test]
    fn test_every_listed_effect_has_a_template() {
        // The Effects panel silently skips descriptors without a template
        // (Puppet Warp shipped invisible this way): every hand-rolled OFX
        // entry must construct.
        for desc in project::OFX_SUITE {
            assert!(
                crate::panels::effect_template_for(desc.id).is_some(),
                "effects panel has no template for {}",
                desc.id
            );
        }
        assert!(
            crate::panels::effect_template_for("net.sf.openfx.puppet").is_some(),
            "puppet warp must be addable from the panel"
        );
    }

    #[gpui_kit::test]
    fn test_effect_gizmo_suppresses_transform_handles(cx: &mut TestAppContext) {
        // An effect gizmo (warp here) takes over the selected layer: the
        // transform dots unmount while effect dots stay. Removing the
        // effect restores the transform handles.
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        let fx = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_effect_to_selected_layer(project::EffectType::warp(0.0, 1.0)).unwrap()
            })
        });
        let t_id = SharedString::from("gizmo_scale_se_layer_accent");
        let warp_dot = SharedString::from(format!("warp_pin_layer_accent_{fx}_0"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(warp_dot.clone()).visible());
            assert!(
                window.try_find(t_id.clone()).is_none(),
                "transform handles hide under the warp gizmo"
            );
        })
        .expect("update_window failed");
        // Disabled effect: overlay gone, transform back.
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.toggle_layer_effect_enabled("layer_accent", &fx).unwrap();
                cx.notify();
            });
        });
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&fx).unwrap();
            assert!(!eff.enabled, "toggle must disable the effect");
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(warp_dot.clone()).is_none());
            assert!(window.find(t_id.clone()).visible());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_puppet_empty_state_shows_ghost_and_hint(cx: &mut TestAppContext) {
        // No pins: the overlay shows a ghost add-dot and the effect card
        // shows a placement hint, so the tool is never a blank panel.
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1280.), px(800.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        let puppet_id = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_effect_to_selected_layer(project::EffectType::puppet()).unwrap()
            })
        });
        let ghost_id = SharedString::from(format!("puppet_add_layer_accent_{puppet_id}"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(ghost_id.clone()).visible());
            window.click(ghost_id.clone(), cx);
        })
        .expect("update_window failed");
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let layer = s.active_composition().unwrap().get_layer("layer_accent").unwrap();
            let eff = layer.get_effect(&puppet_id).unwrap();
            match &eff.effect_type {
                project::EffectType::Puppet { pins, .. } => {
                    assert_eq!(pins.len(), 1, "ghost click materializes a pin");
                    assert!((pins[0].x - 150.0).abs() < 1.0 && (pins[0].y - 150.0).abs() < 1.0);
                }
                _ => panic!("expected puppet"),
            }
        });
        // Effect card carries the placement hint (scroll it into view).
        let disc_id = SharedString::from(format!("effect_disclosure_{puppet_id}"));
        let hint_id = SharedString::from(format!("puppet_hint_{puppet_id}"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            for _ in 0..8 {
                window.scroll(
                    "properties_inspector",
                    gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-400.))),
                    cx,
                );
                window.render_frame(cx);
                if window.try_find(disc_id.clone()).map(|e| e.visible()).unwrap_or(false) {
                    break;
                }
            }
            assert!(window.find(disc_id.clone()).visible());
            window.click(disc_id.clone(), cx);
            window.render_frame(cx);
            assert!(window.find(hint_id.clone()).visible());
        })
        .expect("update_window failed");
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
                    assert_eq!(color_a.value, project::Color::RED);
                    assert_eq!(color_b.value, project::Color::BLUE);
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
            EffectType::NoiseGenerator { monochrome, .. } if monochrome.value
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
    fn test_tangent_handle_vertical_drag_reshapes_value(cx: &mut TestAppContext) {
        // Up/down drags reshape the value influence (hy), not just time.
        use gpui_kit::test::TestWindowExt;
        use gpui_kit::point;

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
        // Handle tip value before: k.v + hy * seg_v (segment descends
        // 200 -> -200, so hy itself is segment-relative).
        let before_tip: f32 = app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let k = s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .transform
                .position
                .keyframes()
                .iter()
                .find(|k| (k.time.seconds() - 2.0).abs() < 1e-6)
                .unwrap();
            k.value.x + k.out_tangent.unwrap().y * -400.0
        });
        // Drag the out-handle up: value influence must grow.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let snap = window.find("graph_tan_layer_accent_transform_position_x_2000_out");
            assert!(snap.visible());
            let from = snap.bounds().center();
            window.drag(from, from + point(px(0.0), px(-40.0)), cx);
        })
        .expect("update_window failed");
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let k = s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .transform
                .position
                .keyframes()
                .iter()
                .find(|k| (k.time.seconds() - 2.0).abs() < 1e-6)
                .unwrap()
                .clone();
            let out = k.out_tangent.unwrap();
            let tip = k.value.x + out.y * -400.0;
            assert!(
                tip > before_tip + 10.0,
                "upward handle drag raises the handle tip: {before_tip} -> {tip}"
            );
        });
    }

    #[gpui_kit::test]
    fn test_in_tangent_handle_drag_reshapes(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt;
        use gpui_kit::point;

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
            let k = s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .transform
                .position
                .keyframes()
                .iter()
                .find(|k| (k.time.seconds() - 2.0).abs() < 1e-6)
                .unwrap();
            let it = k.in_tangent.unwrap();
            (it.x, it.y)
        });
        // Drag in-handle to the right: x influence must grow.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let snap = window.find("graph_tan_layer_accent_transform_position_x_2000_in");
            assert!(snap.visible());
            let from = snap.bounds().center();
            window.drag(from, from + point(px(40.0), px(0.0)), cx);
        })
        .expect("update_window failed");
        app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let k = s
                .active_composition()
                .unwrap()
                .get_layer("layer_accent")
                .unwrap()
                .transform
                .position
                .keyframes()
                .iter()
                .find(|k| (k.time.seconds() - 2.0).abs() < 1e-6)
                .unwrap()
                .clone();
            let it = k.in_tangent.unwrap();
            assert!(
                it.x > before.0,
                "rightward handle drag on in-tangent grows influence: {} -> {}",
                before.0,
                it.x
            );
        });
    }

    #[test]
    fn test_dropdown_anchor_sits_above_button() {
        // Popup origins are computed from the button position (the old
        // code pinned both popups to a fixed window corner).
        use crate::panels::dropdown_above;
        // Button near the bottom bar: popup opens directly above it.
        assert_eq!(dropdown_above(1280.0, Some((500.0, 850.0))), (500.0, 442.0));
        // Clamped inside narrow windows and short heights.
        assert_eq!(dropdown_above(300.0, Some((290.0, 850.0))), (84.0, 442.0));
        assert_eq!(dropdown_above(1280.0, Some((500.0, 100.0))), (500.0, 8.0));
    }

    #[gpui_kit::test]
    fn test_easing_buttons_cache_popup_anchors(cx: &mut TestAppContext) {
        // The bottom-bar buttons report their origins every frame, which
        // is what the popups anchor to (deferred overlays don't register
        // in test snapshots, so pin the inputs instead of the popup).
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
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
        })
        .expect("update_window failed");
        cx.run_until_parked();
        assert!(app_view.read_with(cx, |view, cx| {
            let tl = view.panels().timeline.read(cx);
            // Bottom-bar buttons sit low in a 900px window; popups open
            // ~400px above them.
            tl.ease_btn_pos.is_some_and(|(_, y)| y > 500.0)
                && tl.anim_btn_pos.is_some_and(|(_, y)| y > 500.0)
        }));
    }

    #[gpui_kit::test]
    fn test_tangent_handle_press_ignores_plot_background(cx: &mut TestAppContext) {
        // Grabbing a tangent handle must not seek the playhead or arm a
        // marquee: the handle bubbles through the plot background handler,
        // which used to treat every press as seek + marquee start.
        use gpui_kit::test::TestWindowExt;

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
                if !s.spline_editor_open {
                    s.toggle_spline_editor();
                }
                cx.notify();
            });
        });
        let t0 = app_view.read_with(cx, |view, cx| view.state().read(cx).clock.position_seconds());
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let snap = window.find("graph_tan_layer_accent_transform_position_x_2000_out");
            assert!(snap.visible());
            window.click("graph_tan_layer_accent_transform_position_x_2000_out", cx);
            window.render_frame(cx);
        })
        .expect("update_window failed");
        cx.run_until_parked();
        assert!(app_view.read_with(cx, |view, cx| {
            (view.state().read(cx).clock.position_seconds() - t0).abs() < 1e-9
        }), "handle press must not seek");
        assert!(app_view.read_with(cx, |view, cx| {
            let tl = view.panels().timeline.read(cx);
            tl.graph_marquee.is_none() && tl.graph_sel_keys.is_empty()
        }), "handle press must not marquee or select");
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
            // Card starts collapsed: scroll into view (the full-width
            // timeline strip leaves a shorter inspector), then expand.
            let disc = SharedString::from(format!("effect_disclosure_{noise}"));
            for _ in 0..8 {
                window.scroll(
                    "properties_inspector",
                    gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-400.))),
                    cx,
                );
                window.render_frame(cx);
                if window.try_find(disc.clone()).map(|e| e.visible()).unwrap_or(false) {
                    break;
                }
            }
            window.click(disc, cx);
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
                project::EffectType::NoiseGenerator { monochrome, .. } if monochrome.value
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
            // Card starts collapsed: expand first.
            window.click(SharedString::from(format!("effect_disclosure_{eff}")), cx);
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
    fn test_balance_range_picker_filters_sliders(cx: &mut TestAppContext) {
        // Color Balance range picker: Shadows shows only shadow sliders,
        // switching to Midtones swaps the visible rows.
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
        let bal = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_effect_to_selected_layer(project::EffectType::Stock {
                    plugin: project::StockPlugin::ColorBalance,
                    params: project::EffectType::stock_params(
                        project::StockPlugin::ColorBalance,
                    ),
                    colors: project::EffectType::stock_colors(
                        project::StockPlugin::ColorBalance,
                    ),
                })
                .unwrap()
            })
        });
        app_view.update(cx, |view, cx| {
            view.panels().properties.update(cx, |this, cx| {
                this.source_expanded = false;
                this.transform_expanded = false;
                this.switches_expanded = false;
                this.tools_expanded = false;
                cx.notify();
            });
        });
        let cat = SharedString::from(format!("stock_balance_cat_{bal}"));
        let shadow_row = SharedString::from(format!("param_shadows_cyan_red_{bal}"));
        let mid_row = SharedString::from(format!("param_midtones_cyan_red_{bal}"));
        let hi_row = SharedString::from(format!("param_highlights_cyan_red_{bal}"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Card starts collapsed: expand first.
            window.click(SharedString::from(format!("effect_disclosure_{bal}")), cx);
            window.render_frame(cx);
            assert!(window.find(cat.clone()).visible());
            // Default range: Shadows only.
            assert!(window.find(shadow_row.clone()).visible());
            assert!(window.try_find(mid_row.clone()).is_none());
            assert!(window.try_find(hi_row.clone()).is_none());
            // Switch to Midtones.
            window.click(cat.clone(), cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("update_window failed");
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(mid_row.clone()).visible());
            assert!(window.try_find(shadow_row.clone()).is_none());
            assert!(window.try_find(hi_row.clone()).is_none());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_tiler_enum_and_mirror_widgets(cx: &mut TestAppContext) {
        // Tiler Layout/Cell Comboboxes + Mirror checkbox: keyboard pick
        // commits through the generic enum/bool setters.
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
        let tiler = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_effect_to_selected_layer(project::EffectType::tiler(2.0, 2.0)).unwrap()
            })
        });
        app_view.update(cx, |view, cx| {
            view.panels().properties.update(cx, |this, cx| {
                this.source_expanded = false;
                this.transform_expanded = false;
                this.switches_expanded = false;
                this.tools_expanded = false;
                cx.notify();
            });
        });
        let mode_trigger = SharedString::from(format!("fx_enum_{tiler}_mode"));
        let cell_trigger = SharedString::from(format!("fx_enum_{tiler}_cell"));
        let mirror_toggle = SharedString::from(format!("fx_bool_mirror_{tiler}"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Card starts collapsed: expand first.
            window.click(SharedString::from(format!("effect_disclosure_{tiler}")), cx);
            window.render_frame(cx);
            assert!(window.find(mode_trigger.clone()).visible());
            assert!(window.find(cell_trigger.clone()).visible());
            assert!(window.find(mirror_toggle.clone()).visible());
            // Layout: Grid -> Radial (second option).
            window.click(mode_trigger.clone(), cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            matches!(
                view.state().read(cx).active_composition().unwrap()
                    .get_layer("layer_accent").unwrap()
                    .get_effect(&tiler).unwrap().effect_type,
                project::EffectType::Tiler { mode: project::TileMode::Radial, .. }
            )
        }));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Mirror checkbox toggles on.
            window.click(mirror_toggle.clone(), cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            matches!(
                &view.state().read(cx).active_composition().unwrap()
                    .get_layer("layer_accent").unwrap()
                    .get_effect(&tiler).unwrap().effect_type,
                project::EffectType::Tiler { mirror, .. } if mirror.value
            )
        }));
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
            // Cards start collapsed: expand both first.
            window.click(SharedString::from(format!("effect_disclosure_{ramp}")), cx);
            window.click(SharedString::from(format!("effect_disclosure_{noise}")), cx);
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
    fn test_fill_gradient_tab_commits_model_and_edits_stops(cx: &mut TestAppContext) {
        // Three-mode picker Gradient tab: commits a real fill gradient,
        // then bar-click adds a stop, right-click deletes, angle steps.
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        // Tall window: the inline stop editor lengthens the inspector, and
        // the reverse button must stay on-screen to click.
        let handle = cx.open_window(size(px(1440.), px(1600.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        let text_id = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                let tid = s.add_text_layer("Gradient Text", None).unwrap();
                s.select_layer(Some(tid.clone()));
                tid
            })
        });
        // Keep the fill group on-screen (off-viewport clicks can't land).
        app_view.update(cx, |view, cx| {
            view.panels().properties.update(cx, |this, cx| {
                for key in ["font", "character", "paragraph"] {
                    this.text_collapsed.insert(key);
                }
                cx.notify();
            });
        });
        // No gradient before entering the tab.
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).layer_fill_gradient(&text_id, "text_fill").is_none()
        }));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("text_fill_swatch", cx);
        })
        .expect("update_window failed");
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_fill_tab_gradient").visible());
            window.click("text_fill_btn_gradient", cx);
        })
        .expect("update_window failed");
        // Model now holds a seeded two-stop gradient; editor mounted.
        let g = app_view.read_with(cx, |view, cx| {
            view.state().read(cx).layer_fill_gradient(&text_id, "text_fill")
        });
        assert_eq!(g.as_ref().map(|g| g.stops.len()), Some(2));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("gradient_bar_text_fill").visible());
            assert!(window.find("gradient_stop_text_fill_0").visible());
            assert!(window.find("gradient_stop_text_fill_1").visible());
            assert!(window.find("text_fill_grad_angle_value").visible());
        })
        .expect("update_window failed");
        // Angle stepper commits to the model.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("text_fill_grad_angle_plus", cx);
        })
        .expect("update_window failed");
        assert_eq!(
            app_view
                .read_with(cx, |view, cx| {
                    view.state().read(cx).layer_fill_gradient(&text_id, "text_fill")
                })
                .map(|g| g.angle),
            Some(105.0)
        );
        // Bar click at 50% adds a stop there and selects it.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let b = window.find("gradient_bar_text_fill").bounds();
            let w = b.size.width / px(1.0);
            window.click_at(
                "gradient_bar_text_fill",
                gpui::point(px(w * 0.5), px(11.0)),
                cx,
            );
        })
        .expect("update_window failed");
        let g = app_view
            .read_with(cx, |view, cx| {
                view.state().read(cx).layer_fill_gradient(&text_id, "text_fill")
            })
            .expect("gradient present");
        assert_eq!(g.stops.len(), 3);
        assert!((g.stops[1].offset - 0.5).abs() < 0.03, "{g:?}");
        assert!(app_view.read_with(cx, |view, cx| {
            view.panels().properties.read(cx).color_picker_gradient_stop.get("text_fill").copied() == Some(1)
        }));
        // Right-click the middle diamond deletes it (back to two).
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click("gradient_stop_text_fill_1", cx);
        })
        .expect("update_window failed");
        assert_eq!(
            app_view
                .read_with(cx, |view, cx| {
                    view.state().read(cx).layer_fill_gradient(&text_id, "text_fill")
                })
                .map(|g| g.stops.len()),
            Some(2)
        );
        // Two-stop minimum: deleting again is refused.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click("gradient_stop_text_fill_0", cx);
        })
        .expect("update_window failed");
        assert_eq!(
            app_view
                .read_with(cx, |view, cx| {
                    view.state().read(cx).layer_fill_gradient(&text_id, "text_fill")
                })
                .map(|g| g.stops.len()),
            Some(2)
        );
        // Reverse mirrors the stored stops end-for-end.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("gradient_reverse_text_fill", cx);
        })
        .expect("update_window failed");
        assert_eq!(
            app_view
                .read_with(cx, |view, cx| {
                    view.state().read(cx).layer_fill_gradient(&text_id, "text_fill")
                })
                .map(|g| g.stops[0].offset),
            Some(1.0)
        );
        // Back to Color clears the model gradient.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("text_fill_btn_color", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).layer_fill_gradient(&text_id, "text_fill").is_none()
        }));
    }

    #[gpui_kit::test]
    fn test_gradient_stop_drag_moves_and_bar_adds(cx: &mut TestAppContext) {
        // Effect ramp editor: diamond drag moves the stop (one undo step),
        // bar click adds a stop at the click position.
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
        let (lid, ramp) = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                let ramp = s
                    .add_effect_to_selected_layer(project::EffectType::gradient_ramp(
                        project::Color::BLACK,
                        project::Color::WHITE,
                        90.0,
                    ))
                    .unwrap();
                ("layer_accent".to_string(), ramp)
            })
        });
        app_view.update(cx, |view, cx| {
            view.panels().properties.update(cx, |this, cx| {
                this.source_expanded = false;
                this.transform_expanded = false;
                this.switches_expanded = false;
                this.tools_expanded = false;
                cx.notify();
            });
        });
        let stops_of = |app_view: &Entity<AppView>, cx: &TestAppContext| {
            app_view.read_with(cx, |view, cx| {
                view.state()
                    .read(cx)
                    .active_composition()
                    .unwrap()
                    .get_layer(&lid)
                    .unwrap()
                    .get_effect(&ramp)
                    .unwrap()
                    .effect_type
                    .gradient_ramp_stops()
                    .unwrap()
            })
        };
        assert_eq!(stops_of(&app_view, cx).len(), 2);
        // Drag stop 0 onto the bar center: offset lands near 0.5.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Card starts collapsed: expand first.
            window.click(SharedString::from(format!("effect_disclosure_{ramp}")), cx);
            window.render_frame(cx);
            window.drag_to(
                SharedString::from(format!("gradient_stop0_{ramp}")),
                SharedString::from(format!("gradient_bar_{ramp}")),
                cx,
            );
        })
        .expect("update_window failed");
        let stops = stops_of(&app_view, cx);
        assert!((stops[0].offset - 0.5).abs() < 0.06, "{stops:?}");
        // Bar click at 75% adds a third stop there.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let bar = SharedString::from(format!("gradient_bar_{ramp}"));
            let b = window.find(bar.clone()).bounds();
            let w = b.size.width / px(1.0);
            window.click_at(bar, gpui::point(px(w * 0.75), px(11.0)), cx);
        })
        .expect("update_window failed");
        let stops = stops_of(&app_view, cx);
        assert_eq!(stops.len(), 3);
        assert!((stops[1].offset - 0.75).abs() < 0.05, "{stops:?}");
    }

    #[gpui_kit::test]
    fn test_solid_gradient_tab_commits_model(cx: &mut TestAppContext) {
        // Solid fill picker Gradient tab commits a model gradient too.
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
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_bg".to_string()));
                cx.notify();
            });
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("solid_color_swatch", cx);
        })
        .expect("update_window failed");
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("solid_color_tab_gradient").visible());
            window.click("solid_color_btn_gradient", cx);
        })
        .expect("update_window failed");
        assert_eq!(
            app_view.read_with(cx, |view, cx| {
                view.state().read(cx).layer_fill_gradient("layer_bg", "solid_color").map(|g| g.stops.len())
            }),
            Some(2)
        );
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("gradient_bar_solid_color").visible());
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_color_editor_layout_no_presets_and_commits(cx: &mut TestAppContext) {
        // Reference-styled editor: tabs, SV field, sliders, numeric
        // fields, hex, NEW/ORIG render; no preset swatches anywhere.
        // Bar click commits to the model; ORIG reverts.
        use gpui_kit::test::TestWindowExt;
        use project::LayerSource;

        cx.update(gpui_kit::init);
        let mut app_view_entity = None;
        let handle = cx.open_window(size(px(1440.), px(1600.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| AppView::new(window, cx));
            app_view_entity = Some(view.clone());
            Root::new(view, window, cx)
        });
        let app_view = app_view_entity.expect("AppView created");
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                s.select_layer(Some("layer_bg".to_string()));
                cx.notify();
            });
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("solid_color_swatch", cx);
        })
        .expect("update_window failed");
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Reference layout elements are all present (RGB tab default).
            for id in [
                "solid_color_tab_rgb",
                "solid_color_tab_hex",
                "solid_color_tab_hsv",
                "solid_color_sv_field",
                "solid_color_hue_bar",
                "solid_color_alpha_bar",
                "solid_color_num_r",
                "solid_color_num_g",
                "solid_color_num_b",
                "solid_color_orig_swatch",
            ] {
                assert!(window.find(id).visible(), "{id}");
            }
            // HEX tab swaps the numeric section for the hex field.
            window.click("solid_color_tab_hex", cx);
        })
        .expect("update_window failed");
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("solid_color_hex_input").visible());
            assert!(window.try_find("solid_color_num_r").is_none());
            // Back to RGB for the commit checks below.
            window.click("solid_color_tab_rgb", cx);
            window.render_frame(cx);
            // No preset swatches in the selector.
            assert!(window.try_find("solid_color_palette_#FFFFFF").is_none());
        })
        .expect("update_window failed");
        let solid_of = |app_view: &Entity<AppView>, cx: &mut TestAppContext| {
            app_view.read_with(cx, |view, cx| {
                let layer = view.state().read(cx).active_composition().unwrap()
                    .get_layer("layer_bg").unwrap().clone();
                match layer.source {
                    LayerSource::Solid { color, .. } => color.value,
                    _ => panic!("layer_bg must be solid"),
                }
            })
        };
        let before = solid_of(&app_view, cx);
        // Click the R bar near full: red channel commits live.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let b = window.find("solid_color_r_bar").bounds();
            let w = b.size.width / px(1.0);
            window.click_at("solid_color_r_bar", gpui::point(px(w - 1.0), px(10.0)), cx);
        })
        .expect("update_window failed");
        let picked = solid_of(&app_view, cx);
        assert!(picked.r > 0.95, "R bar click must drive red, got {picked:?}");
        // ORIG reverts to the snapshot taken when the section opened.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("solid_color_orig_swatch", cx);
        })
        .expect("update_window failed");
        let reverted = solid_of(&app_view, cx);
        assert!(
            (reverted.r - before.r).abs() < 0.01
                && (reverted.g - before.g).abs() < 0.01
                && (reverted.b - before.b).abs() < 0.01,
            "ORIG must revert, was {before:?} got {reverted:?}"
        );
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
        // Card starts collapsed: expand first.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(SharedString::from(format!("effect_disclosure_{eff_id}")), cx);
        })
        .expect("window update");
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
        // Direct set (Combobox commit path) + label round-trip.
        state.set_mask_mode(layer_id, &mid, MaskMode::Intersect).expect("mode set");
        assert_eq!(
            state.active_composition().unwrap().get_layer(layer_id).unwrap().get_mask(&mid).unwrap().mode,
            MaskMode::Intersect
        );
        assert_eq!(MaskMode::from_label("Difference"), Some(MaskMode::Difference));
        assert_eq!(MaskMode::from_label("Bogus"), None);
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
        let text_id = state.add_text_layer("Motion Effect Title", None).unwrap();
        {
            let comp = state.active_composition().unwrap();
            let text_layer = comp.get_layer(&text_id).unwrap();
            assert!(matches!(&text_layer.source, LayerSource::Text { text, .. } if text.value == "Motion Effect Title"));
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
        use crate::panels::{ContextMenuTarget, TimelinePanel};
        use project::BlendMode;

        cx.update(gpui_kit::init);
        let state_entity = cx.new(|_| crate::state::EditorState::new());
        let timeline_panel = cx.new(|cx| TimelinePanel::new(state_entity, cx));

        // BlendMode::ALL carries distinct labels for the Combobox options.
        let mut labels: Vec<&str> = BlendMode::ALL.iter().map(|m| m.as_str()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), BlendMode::ALL.len());

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
    fn test_layer_and_asset_clipboard_round_trip() {
        use crate::state::EditorState;

        let mut s = EditorState::new();
        // Empty clipboards refuse to paste; unknown ids refuse to copy.
        assert!(s.paste_copied_layer().is_err());
        assert!(s.paste_copied_asset().is_err());
        assert!(s.copy_layer("missing").is_err());
        assert!(s.copy_asset("missing").is_err());

        // Layer copy/paste: fresh id, "Copy" name, repeatable.
        s.copy_layer("layer_accent").unwrap();
        let n0 = s.active_composition().unwrap().layers.len();
        let nid = s.paste_copied_layer().unwrap();
        assert_ne!(nid, "layer_accent");
        {
            let comp = s.active_composition().unwrap();
            assert_eq!(comp.layers.len(), n0 + 1);
            assert!(comp.get_layer(&nid).unwrap().name.ends_with("Copy"));
        }
        s.paste_copied_layer().unwrap();
        assert_eq!(s.active_composition().unwrap().layers.len(), n0 + 2);

        // Asset copy/paste: pastes the copied asset as a new layer.
        let aid = "clip_asset";
        s.project
            .add_asset(project::Asset::from_path(aid, "Clip", "/tmp/clip.png"))
            .unwrap();
        s.copy_asset(aid).unwrap();
        let n1 = s.active_composition().unwrap().layers.len();
        s.paste_copied_asset().unwrap();
        assert_eq!(s.active_composition().unwrap().layers.len(), n1 + 1);
    }

    #[gpui_kit::test]
    fn test_layer_context_menu_copy_paste_add_mask(cx: &mut TestAppContext) {
        // Timeline layer menu: Copy -> Paste duplicates, Add Mask adds one.
        use crate::panels::ContextMenuTarget;
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
        let open_layer_menu = |app_view: &gpui_kit::Entity<AppView>, cx: &mut TestAppContext| {
            app_view.update(cx, |view, cx| {
                view.panels().timeline.update(cx, |p, _| {
                    p.open_context_menu(
                        ContextMenuTarget::Layer("layer_accent".to_string()),
                        gpui::point(gpui::px(150.), gpui::px(150.)),
                    );
                });
            });
        };
        let n0 = app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap().layers.len()
        });
        open_layer_menu(&app_view, cx);
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("timeline_ctx_copy_layer").visible());
            window.click("timeline_ctx_copy_layer", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| view.state().read(cx).layer_clipboard.is_some()));
        open_layer_menu(&app_view, cx);
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("timeline_ctx_paste_layer").visible());
            window.click("timeline_ctx_paste_layer", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap().layers.len() == n0 + 1
        }));
        open_layer_menu(&app_view, cx);
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("timeline_ctx_add_mask").visible());
            window.click("timeline_ctx_add_mask", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap()
                .get_layer("layer_accent").unwrap().masks.len() == 1
        }));

        // Project asset menu: Copy -> Paste adds the asset as a layer.
        let aid = "menu_asset";
        app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                let _ = s.project.add_asset(project::Asset::from_path(aid, "Menu Clip", "/tmp/menu.png"));
            });
        });
        let open_asset_menu = |app_view: &gpui_kit::Entity<AppView>, cx: &mut TestAppContext| {
            app_view.update(cx, |view, cx| {
                view.panels().project.update(cx, |p, _| {
                    p.open_context_menu(
                        crate::panels::ProjectContextMenuTarget::Asset(aid.to_string()),
                        gpui::point(gpui::px(50.), gpui::px(50.)),
                    );
                });
            });
        };
        open_asset_menu(&app_view, cx);
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("proj_ctx_copy_asset").visible());
            window.click("proj_ctx_copy_asset", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| view.state().read(cx).asset_clipboard.is_some()));
        let n1 = app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap().layers.len()
        });
        open_asset_menu(&app_view, cx);
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("proj_ctx_paste_asset").visible());
            window.click("proj_ctx_paste_asset", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap().layers.len() == n1 + 1
        }));
    }

    #[gpui_kit::test]
    fn test_timeline_row_comboboxes_drive_blend_matte_parent(cx: &mut TestAppContext) {
        // Timeline row Comboboxes: open, arrow down, confirm commits.
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
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("tl_blend_layer_accent").visible());
            assert!(window.find("tl_matte_layer_accent").visible());
            assert!(window.find("parent_picker_layer_accent").visible());
            // Blend: Normal -> Dissolve (second option).
            window.click("tl_blend_layer_accent", cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap().get_layer("layer_accent").unwrap().blend_mode == project::BlendMode::Dissolve
        }));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Matte: None -> Alpha (second option).
            window.click("tl_matte_layer_accent", cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap().get_layer("layer_accent").unwrap().matte_mode == project::TrackMatteMode::Alpha
        }));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            // Parent: None -> first candidate layer.
            window.click("parent_picker_layer_accent", cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap().get_layer("layer_accent").unwrap().parent_id.is_some()
        }));
    }

    #[gpui_kit::test]
    fn test_mask_mode_combobox_sets_mode_and_resyncs(cx: &mut TestAppContext) {
        // Mask mode Comboboxes (properties card + timeline row): picking
        // Subtract commits to the model and both pickers agree afterwards.
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
        let mid = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, _| {
                s.select_layer(Some("layer_accent".to_string()));
                s.add_mask_to_layer("layer_accent").unwrap()
            })
        });
        // Reveal the timeline layer twirl-down + masks group.
        app_view.update(cx, |view, cx| {
            view.panels().timeline.update(cx, |this, cx| {
                this.set_layer_expanded("layer_accent", true);
                this.set_group_expanded("layer_accent:masks", true);
                cx.notify();
            });
        });
        let mask_trigger = SharedString::from(format!("mask_mode_{mid}"));
        let tl_trigger = SharedString::from(format!("tl_mask_mode_layer_accent_{mid}"));
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(mask_trigger.clone()).visible());
            assert!(window.find(tl_trigger.clone()).visible());
            // Mode: Add -> Subtract (second option).
            window.click(mask_trigger.clone(), cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("update_window failed");
        assert!(app_view.read_with(cx, |view, cx| {
            view.state().read(cx).active_composition().unwrap().get_layer("layer_accent").unwrap().get_mask(&mid).unwrap().mode == project::MaskMode::Subtract
        }));
        // The timeline picker re-synced to the same pick.
        assert_eq!(
            app_view.read_with(cx, |view, cx| {
                view.panels().timeline.read(cx).tl_combos.get(&format!("tl_maskmode_layer_accent_{mid}")).and_then(|cb| cb.read(cx).selected_value().map(|v| v.to_string()))
            }),
            Some("Subtract".to_string())
        );
    }

    #[gpui_kit::test]
    fn test_masked_layer_shell_presents_content(cx: &mut TestAppContext) {
        // Viewport shell path for a masked layer: the raster cache entry
        // must exist, hold ink, and average to the layer color (no black
        // wipe), and the shell must present an image.
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
        let lid = app_view.update(cx, |view, cx| {
            view.state().update(cx, |s, cx| {
                let lid = s
                    .add_solid_layer(
                        "Masked Solid",
                        project::Color::from_rgb_u8(200, 100, 50),
                        400,
                        400,
                    )
                    .unwrap();
                s.select_layer(Some(lid.clone()));
                let _ = s.add_mask_path_to_layer(
                    &lid,
                    "M",
                    project::Path::rectangle(50.0, 50.0, 300.0, 300.0),
                );
                cx.notify();
                lid
            })
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(SharedString::from(format!("canvas_layer_{lid}"))).visible());
        })
        .expect("update_window failed");
        let entry = app_view.read_with(cx, |view, cx| {
            view.panels().viewer.read(cx).raster_cache.get(&lid).cloned()
        });
        let entry = entry.expect("raster cache entry for masked layer");
        let ink = entry.bgra.as_chunks::<4>().0.iter().filter(|c| c[3] > 10).count();
        assert!(ink > 1000, "masked shell must hold visible ink, got {ink}");
        // Average trends toward the solid color, not black.
        assert!(
            entry.avg.r > 0.4 && entry.avg.g > 0.15,
            "masked average {:?}",
            entry.avg
        );
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
        if let LayerSource::Solid { color, width, height, .. } = layer_bg.source {
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
                EffectType::ChromaKey { key_color, .. } => assert_eq!(key_color.value, custom_green),
                EffectType::Tint { map_black, map_white, .. } => {
                    assert_eq!(map_black.value, navy);
                    assert_eq!(map_white.value, gold);
                }
                EffectType::DropShadow { color, .. } => assert_eq!(color.value, dark_purple),
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
    fn test_numeric_expression_parsing_and_typed_commit() {
        use crate::state::{parse_numeric_expression, EditorState};

        // 1. Basic numbers
        assert_eq!(parse_numeric_expression("120", None), Some(120.0));
        assert_eq!(parse_numeric_expression("-45.5", None), Some(-45.5));
        assert_eq!(parse_numeric_expression(".5", None), Some(0.5));
        assert_eq!(parse_numeric_expression("+25", None), Some(25.0));

        // 2. Unit suffixes
        assert_eq!(parse_numeric_expression("100px", None), Some(100.0));
        assert_eq!(parse_numeric_expression("75 %", None), Some(75.0));
        assert_eq!(parse_numeric_expression("50%", None), Some(50.0));
        assert_eq!(parse_numeric_expression("45deg", None), Some(45.0));
        assert_eq!(parse_numeric_expression("90°", None), Some(90.0));
        assert_eq!(parse_numeric_expression("2.5s", None), Some(2.5));
        assert_eq!(parse_numeric_expression("10f", None), Some(10.0));
        assert_eq!(parse_numeric_expression("60fps", None), Some(60.0));

        // 3. Thousand commas
        assert_eq!(parse_numeric_expression("1,920", None), Some(1920.0));
        assert_eq!(parse_numeric_expression("1,920 / 2", None), Some(960.0));

        // 4. Arithmetic expressions
        assert_eq!(parse_numeric_expression("1920 / 2", None), Some(960.0));
        assert_eq!(parse_numeric_expression("1920/2", None), Some(960.0));
        assert_eq!(parse_numeric_expression("100 + 50", None), Some(150.0));
        assert_eq!(parse_numeric_expression("100 - 25", None), Some(75.0));
        assert_eq!(parse_numeric_expression("50 * 2.5", None), Some(125.0));
        assert_eq!(parse_numeric_expression("(100 + 20) * 3", None), Some(360.0));
        assert_eq!(parse_numeric_expression("100 + 20 * 3", None), Some(160.0));
        assert_eq!(parse_numeric_expression("-100 + 40", None), Some(-60.0));

        // 5. Relative adjustments against current value
        assert_eq!(parse_numeric_expression("+=50", Some(100.0)), Some(150.0));
        assert_eq!(parse_numeric_expression("-=25", Some(100.0)), Some(75.0));
        assert_eq!(parse_numeric_expression("*=2", Some(100.0)), Some(200.0));
        assert_eq!(parse_numeric_expression("/=2", Some(100.0)), Some(50.0));
        assert_eq!(parse_numeric_expression("* 3", Some(50.0)), Some(150.0));
        assert_eq!(parse_numeric_expression("/ 2", Some(50.0)), Some(25.0));

        // 6. Divide by zero and invalid inputs
        assert_eq!(parse_numeric_expression("100 / 0", None), None);
        assert_eq!(parse_numeric_expression("/= 0", Some(50.0)), None);
        assert_eq!(parse_numeric_expression("", None), None);
        assert_eq!(parse_numeric_expression("   ", None), None);
        assert_eq!(parse_numeric_expression("abc", None), None);

        // 7. Typed commit end-to-end with Undo support
        let mut state = EditorState::new();
        let initial_x = state.scrub_current_value("pos_x").unwrap();

        // Math expression commit: "1920 / 2" -> 960.0
        state.value_edit_key = Some("pos_x".to_string());
        assert!(state.commit_typed_value("1920 / 2"));
        assert!((state.scrub_current_value("pos_x").unwrap() - 960.0).abs() < 1e-4);

        // Relative commit: "+=40" -> 1000.0
        state.value_edit_key = Some("pos_x".to_string());
        assert!(state.commit_typed_value("+=40"));
        assert!((state.scrub_current_value("pos_x").unwrap() - 1000.0).abs() < 1e-4);

        // Negative value commit: "-150" -> -150.0
        state.value_edit_key = Some("pos_x".to_string());
        assert!(state.commit_typed_value("-150"));
        assert!((state.scrub_current_value("pos_x").unwrap() - (-150.0)).abs() < 1e-4);

        // Undo reverts through each committed step
        assert!(state.undo());
        assert!((state.scrub_current_value("pos_x").unwrap() - 1000.0).abs() < 1e-4);
        assert!(state.undo());
        assert!((state.scrub_current_value("pos_x").unwrap() - 960.0).abs() < 1e-4);
        assert!(state.undo());
        assert!((state.scrub_current_value("pos_x").unwrap() - initial_x).abs() < 1e-4);
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
            (project::Effect::cel_shading("c", 4.0, 60.0), "Cel Shading", "edge", 10.0, 70.0),
            (project::Effect::oil_paint("o", 2.0, 100.0), "Oil Painting", "radius", 1.0, 3.0),
            (project::Effect::trim_path("t", 0.0, 100.0, 0.0), "Trim Path", "end", -10.0, 90.0),
            (project::Effect::sine_path("s", 20.0, 1.0, 0.0), "Sine Path", "amplitude", 10.0, 30.0),
            (project::Effect::instance_path("i", 5.0, 100.0, 0.0, 100.0, 100.0), "Instance Path", "count", 2.0, 7.0),
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
            assert_eq!(fill.value, project::Color::from_hex("#10B981").unwrap());
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

            // 1. Source text input is clean: no example-text shortcuts.
            assert!(window.find("text_content_input").visible());
            assert!(window.try_find("text_preset_Title Text").is_none());
            assert!(window.try_find("text_preset_Motion Effect").is_none());
        })
        .expect("update_window failed");

        assert!(app_view.read_with(cx, |view, cx| {
            let s = view.state().read(cx);
            let comp = s.active_composition().unwrap();
            let l = comp.get_layer(&text_id).unwrap();
            match &l.source {
                project::LayerSource::Text { text, .. } => text.value == "Hello GPUI Kit",
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

        // 5. Text Alignment (Combobox): Left -> Center via keyboard pick.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("text_align_combobox").visible());
            assert!(window.find("text_vert_combobox").visible());
            assert!(window.find("text_stroke_pos_combobox").visible());
            assert!(window.find("text_paint_order_combobox").visible());
            window.click("text_align_combobox", cx);
            window.press("down", cx);
            window.press("enter", cx);
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

        // 7. Applied Effects Card with nested Collapsible (collapsed by
        // default; disclosure expands).
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let card_id = SharedString::from(format!("applied_effect_{blur_id}"));
            let disc_id = SharedString::from(format!("effect_disclosure_{blur_id}"));
            // Scroll the shorter inspector until the card is on-screen.
            for _ in 0..8 {
                window.scroll(
                    "properties_inspector",
                    gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-400.))),
                    cx,
                );
                window.render_frame(cx);
                if window.try_find(card_id.clone()).map(|e| e.visible()).unwrap_or(false) {
                    break;
                }
            }
            assert!(window.find(card_id.clone()).visible());
            assert!(window.find(disc_id.clone()).visible());
            // Card starts collapsed: parameter not mounted until expanded.
            assert!(window.try_find(SharedString::from(format!("param_radius_{blur_id}"))).is_none());
            // Click disclosure to expand.
            window.click(disc_id.clone(), cx);
            window.render_frame(cx);
            assert!(window.find(SharedString::from(format!("param_radius_{blur_id}"))).visible());
            // Click disclosure again to collapse.
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
            assert!(window.find("text_align_combobox").visible());
            assert!(window.find("text_vert_combobox").visible());
            assert!(window.find("text_stroke_pos_combobox").visible());
            assert!(window.find("text_paint_order_combobox").visible());
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

    #[test]
    fn test_adjustment_layer_raster_and_mask_stenciling() {
        use crate::raster::{FloatBuf, Px, rasterize_layer};
        use compositor::{LayerStackEvaluator, SceneGraph};
        use project::{Color, Composition, Effect, EffectType, Layer, Mask, Path, Project, Property, TimeCode};
        use std::collections::HashMap;

        let mut project = Project::new("p", "Proj");
        let tc = TimeCode::from_frames(0, 30.0);
        let tc_end = TimeCode::from_frames(150, 30.0);
        let mut comp = Composition::new("c", "Comp", 100, 100, 30.0, tc_end);

        let mut adj = Layer::adjustment("adj1", "Adj Layer", tc, tc_end);
        adj.effects.push(Effect {
            id: "inv1".to_string(),
            name: "Invert".to_string(),
            enabled: true,
            effect_type: EffectType::Invert {
                amount: Property::new("Amount", 100.0),
            },
        });
        comp.add_layer(adj).unwrap();
        project.add_composition(comp).unwrap();

        let graph = SceneGraph::from_project(&project, "c").unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &tc);
        let evaluated_adj = stack.get_layer("adj1").unwrap();

        // Backdrop: solid red 100x100
        let mut backdrop = FloatBuf::clear(100, 100);
        for p in backdrop.px.iter_mut() {
            *p = Px { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        }
        let assets = HashMap::new();

        // 1. Without masks: full invert of red backdrop -> cyan (0.0, 1.0, 1.0)
        let (out, _avg, empty) = rasterize_layer(
            evaluated_adj,
            100.0,
            100.0,
            100,
            100,
            100.0,
            100.0,
            Color::RED,
            Some(&backdrop),
            0.0,
            0,
            false,
            5.0,
            &assets,
        );
        assert!(!empty, "adjustment layer must not be marked empty");
        let center_px = out.get(50, 50);
        assert!(center_px.r < 0.1, "red should be inverted to cyan");
        assert!(center_px.g > 0.9, "green should be inverted to cyan");
        assert!(center_px.b > 0.9, "blue should be inverted to cyan");
        assert_eq!(center_px.a, 1.0);

        // 2. With Mask: rectangular mask covering left half (x from 0 to 50, y from 0 to 100)
        let rect_path = Path::rectangle(0.0, 0.0, 50.0, 100.0);
        let mask = Mask::with_path("m1", "Mask 1", rect_path);
        let comp_mut = project.get_composition_mut("c").unwrap();
        comp_mut.get_layer_mut("adj1").unwrap().masks.push(mask);

        let graph2 = SceneGraph::from_project(&project, "c").unwrap();
        let stack2 = evaluator.evaluate(&graph2, &tc);
        let evaluated_adj2 = stack2.get_layer("adj1").unwrap();

        let (out_masked, _avg, empty_masked) = rasterize_layer(
            evaluated_adj2,
            100.0,
            100.0,
            100,
            100,
            100.0,
            100.0,
            Color::RED,
            Some(&backdrop),
            0.0,
            0,
            false,
            5.0,
            &assets,
        );
        assert!(!empty_masked);

        // Inside mask (x = 25, y = 50): inverted cyan
        let inside = out_masked.get(25, 50);
        assert!(inside.a > 0.9, "inside mask should have full coverage");
        assert!(inside.r < 0.1);
        assert!(inside.g > 0.9);

        // Outside mask (x = 75, y = 50): transparent contribution (a == 0)
        let outside = out_masked.get(75, 50);
        assert_eq!(outside.a, 0.0, "outside mask should be transparent contribution");

        // 3. Opacity: 50% opacity
        let comp_mut2 = project.get_composition_mut("c").unwrap();
        let l = comp_mut2.get_layer_mut("adj1").unwrap();
        l.masks.clear();
        l.opacity.set_value(50.0);

        let graph3 = SceneGraph::from_project(&project, "c").unwrap();
        let stack3 = evaluator.evaluate(&graph3, &tc);
        let evaluated_adj3 = stack3.get_layer("adj1").unwrap();

        let (out_50, _avg, _) = rasterize_layer(
            evaluated_adj3,
            100.0,
            100.0,
            100,
            100,
            100.0,
            100.0,
            Color::RED,
            Some(&backdrop),
            0.0,
            0,
            false,
            5.0,
            &assets,
        );
        let mid_px = out_50.get(50, 50);
        assert!((mid_px.a - 0.5).abs() < 0.05, "alpha should reflect 50% opacity");
    }

    #[test]
    fn test_adjustment_layer_comp_rasterization() {
        use crate::raster::comp::rasterize_comp;
        use compositor::{LayerStackEvaluator, SceneGraph};
        use project::{Color, Composition, Effect, EffectType, Layer, Project, Property, TimeCode};
        use std::collections::HashMap;

        let mut project = Project::new("p", "Proj");
        let tc = TimeCode::from_frames(0, 30.0);
        let tc_end = TimeCode::from_frames(150, 30.0);
        let mut comp = Composition::new("c", "Comp", 64, 64, 30.0, tc_end);

        // Top layer (index 0): Adjustment layer with Invert effect
        let mut adj = Layer::adjustment("adj", "Adjustment", tc, tc_end);
        adj.effects.push(Effect {
            id: "inv".to_string(),
            name: "Invert".to_string(),
            enabled: true,
            effect_type: EffectType::Invert {
                amount: Property::new("Amount", 100.0),
            },
        });
        comp.add_layer(adj).unwrap();

        // Bottom layer (index 1): Solid green background
        let green_solid = Layer::solid("bg", "Background", Color::rgba(0.0, 1.0, 0.0, 1.0), 64, 64, tc, tc_end);
        comp.add_layer(green_solid).unwrap();
        project.add_composition(comp).unwrap();

        let graph = SceneGraph::from_project(&project, "c").unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &tc);

        let mut assets = HashMap::new();
        let comp_out = rasterize_comp(
            &stack,
            64.0,
            64.0,
            Color::BLACK,
            64,
            64,
            0.0,
            0,
            false,
            5.0,
            &mut assets,
        );

        // Green inverted is Magenta: (1.0, 0.0, 1.0)
        let p = comp_out.get(32, 32);
        assert!(p.r > 0.9, "red should be high in magenta");
        assert!(p.g < 0.1, "green should be inverted to 0");
        assert!(p.b > 0.9, "blue should be high in magenta");
    }

    #[gpui_kit::test]
    fn test_adjustment_layer_viewer_selection_and_gizmo(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());

        // Add an adjustment layer to the composition
        state_entity.update(cx, |s, cx| {
            let tc = project::TimeCode::from_frames(0, 30.0);
            let mut layer = project::Layer::adjustment("test_adj", "My Adjustment Layer", tc, project::TimeCode::from_frames(300, 30.0));
            layer.effects.push(project::Effect {
                id: "blur_fx".to_string(),
                name: "Gaussian Blur".to_string(),
                enabled: true,
                effect_type: project::EffectType::GaussianBlur {
                    radius: project::Property::new("Radius", 10.0),
                },
            });
            let comp = s.active_composition_mut().expect("active comp");
            comp.add_layer(layer).unwrap();
            s.select_layer(Some("test_adj".to_string()));
            cx.notify();
        });

        cx.run_until_parked();

        // Verify layer selection
        state_entity.read_with(cx, |s, _| {
            assert_eq!(s.selected_layer().unwrap().id, "test_adj");
            let comp = s.active_composition().unwrap();
            let adj = comp.get_layer("test_adj").unwrap();
            assert_eq!(adj.source, project::LayerSource::Adjustment);
            assert_eq!(adj.effects.len(), 1);
        });
    }

    #[test]
    fn test_file_menu_new_project_reset() {
        let mut state = EditorState::new();
        // Mutate state with custom layers
        let _ = state.add_solid_layer("Temporary Layer", project::Color::WHITE, 200, 200);
        assert!(state.active_composition().unwrap().layers.len() > 3);

        state.new_project("Brand New Project");
        assert_eq!(state.project_display_name(), "Brand New Project");
        assert_eq!(state.project.name, "Brand New Project");
        let comp = state.active_composition().expect("active composition");
        assert_eq!(comp.width, 1920);
        assert_eq!(comp.height, 1080);
        assert_eq!(state.clock.current_frame(), 0);
    }

    #[test]
    fn test_file_menu_import_multiple_files_and_folder() {
        let temp_dir = std::env::temp_dir().join(format!("motion_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create 2 test PNG images and 1 text file (which shouldn't be imported by folder scan)
        let img1_path = temp_dir.join("asset1.png");
        let img2_path = temp_dir.join("asset2.png");
        let txt_path = temp_dir.join("notes.txt");

        let img = image::RgbaImage::new(32, 32);
        img.save(&img1_path).unwrap();
        img.save(&img2_path).unwrap();
        std::fs::write(&txt_path, "not a media file").unwrap();

        let mut state = EditorState::new();

        // Test multi-file import
        let imported_count = state.import_multiple_media_files(&[img1_path.clone(), img2_path.clone()]).expect("import multiple");
        assert_eq!(imported_count, 2);
        assert!(state.project.assets.iter().any(|a| a.name.contains("asset1")));
        assert!(state.project.assets.iter().any(|a| a.name.contains("asset2")));

        // Test folder import
        let mut state2 = EditorState::new();
        let folder_count = state2.import_media_folder(&temp_dir).expect("import folder");
        assert_eq!(folder_count, 2, "Only the 2 image files should be imported from folder");

        // Clean up temp dir
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_file_menu_export_frame_as_png() {
        let temp_dir = std::env::temp_dir().join(format!("motion_export_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let export_path = temp_dir.join("test_frame.png");

        let state = EditorState::new();
        state.export_frame_as_png(&export_path).expect("export frame as png");

        assert!(export_path.exists());
        let meta = std::fs::metadata(&export_path).unwrap();
        assert!(meta.len() > 0, "exported PNG should not be empty");

        // Verify valid image header and dimensions
        let loaded = image::open(&export_path).expect("decode exported PNG");
        assert_eq!(loaded.width(), 1920);
        assert_eq!(loaded.height(), 1080);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_clear_recent_projects() {
        let mut state = EditorState::new();
        state.recent_projects.push(std::path::PathBuf::from("/fake/path1.json"));
        state.recent_projects.push(std::path::PathBuf::from("/fake/path2.json"));
        assert_eq!(state.recent_projects.len(), 2);

        state.clear_recent_projects();
        assert!(state.recent_projects.is_empty());
    }

    #[gpui_kit::test]
    fn test_file_menu_dropdown_elements(cx: &mut TestAppContext) {
        use super::TopMenu;
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        // Open TopMenu::File
        app_view.update(cx, |this, cx| {
            this.open_menu = Some(TopMenu::File);
            cx.notify();
        });
        cx.run_until_parked();

        // Verify TopMenu::File is open in AppView
        app_view.read_with(cx, |this, _| {
            assert_eq!(this.open_menu, Some(TopMenu::File));
        });
    }

    #[test]
    fn test_playback_half_res_blit_back_no_holes() {
        use crate::raster::{FloatBuf, Px};
        // Canvas size: 100 x 100
        let cw = 100;
        let ch = 100;
        // Background initialized to blue
        let mut comp_buf = FloatBuf::clear(cw, ch);
        for y in 0..ch as i32 {
            for x in 0..cw as i32 {
                comp_buf.put(x, y, Px { r: 0.0, g: 0.0, b: 1.0, a: 1.0 });
            }
        }

        // Layer positioned at (10, 10), dimensions 40x40
        let l_x = 10.0f32;
        let l_y = 10.0f32;
        let l_w = 40.0f32;
        let l_h = 40.0f32;

        // Half-res playback buffer: 20x20
        let rw = 20u32;
        let rh = 20u32;
        // Solid green RGBA (in BGRA layout: B=0, G=255, R=0, A=255)
        let mut bgra = Vec::with_capacity((rw * rh * 4) as usize);
        for _ in 0..(rw * rh) {
            bgra.extend_from_slice(&[0, 255, 0, 255]);
        }

        // Pull-sample every covered canvas pixel (same logic as in panels.rs)
        let min_cx = (l_x.floor() as i32).clamp(0, cw as i32);
        let max_cx = ((l_x + l_w).ceil() as i32).clamp(0, cw as i32);
        let min_cy = (l_y.floor() as i32).clamp(0, ch as i32);
        let max_cy = ((l_y + l_h).ceil() as i32).clamp(0, ch as i32);

        let bgra_len = bgra.len();
        for cy in min_cy..max_cy {
            let v = ((cy as f32 + 0.5 - l_y) / l_h).clamp(0.0, 1.0) * (rh as f32) - 0.5;
            let vy = v.clamp(0.0, (rh - 1) as f32);
            let y0 = vy.floor() as i32;
            let y1 = (y0 + 1).min(rh as i32 - 1);
            let fy = (vy - y0 as f32).clamp(0.0, 1.0);

            for cx in min_cx..max_cx {
                let u = ((cx as f32 + 0.5 - l_x) / l_w).clamp(0.0, 1.0) * (rw as f32) - 0.5;
                let ux = u.clamp(0.0, (rw - 1) as f32);
                let x0 = ux.floor() as i32;
                let x1 = (x0 + 1).min(rw as i32 - 1);
                let fx = (ux - x0 as f32).clamp(0.0, 1.0);

                let idx00 = ((y0 as u32 * rw + x0 as u32) * 4) as usize;
                let idx10 = ((y0 as u32 * rw + x1 as u32) * 4) as usize;
                let idx01 = ((y1 as u32 * rw + x0 as u32) * 4) as usize;
                let idx11 = ((y1 as u32 * rw + x1 as u32) * 4) as usize;

                if idx11 + 3 < bgra_len {
                    let a00 = bgra[idx00 + 3] as f32 / 255.0;
                    let a10 = bgra[idx10 + 3] as f32 / 255.0;
                    let a01 = bgra[idx01 + 3] as f32 / 255.0;
                    let a11 = bgra[idx11 + 3] as f32 / 255.0;

                    let mix = |p: f32, q: f32, r: f32, s: f32| {
                        p * (1.0 - fx) * (1.0 - fy) + q * fx * (1.0 - fy) + r * (1.0 - fx) * fy + s * fx * fy
                    };

                    let a = mix(a00, a10, a01, a11);
                    if a > 0.003 {
                        let r = mix(
                            bgra[idx00 + 2] as f32 / 255.0,
                            bgra[idx10 + 2] as f32 / 255.0,
                            bgra[idx01 + 2] as f32 / 255.0,
                            bgra[idx11 + 2] as f32 / 255.0,
                        );
                        let g = mix(
                            bgra[idx00 + 1] as f32 / 255.0,
                            bgra[idx10 + 1] as f32 / 255.0,
                            bgra[idx01 + 1] as f32 / 255.0,
                            bgra[idx11 + 1] as f32 / 255.0,
                        );
                        let b = mix(
                            bgra[idx00] as f32 / 255.0,
                            bgra[idx10] as f32 / 255.0,
                            bgra[idx01] as f32 / 255.0,
                            bgra[idx11] as f32 / 255.0,
                        );

                        let s = Px { r: r * a, g: g * a, b: b * a, a };
                        let mut d = comp_buf.get(cx, cy);
                        let out_a = s.a + d.a * (1.0 - s.a);
                        let out_r = s.r + d.r * (1.0 - s.a);
                        let out_g = s.g + d.g * (1.0 - s.a);
                        let out_b = s.b + d.b * (1.0 - s.a);
                        d.r = out_r;
                        d.g = out_g;
                        d.b = out_b;
                        d.a = out_a;
                        comp_buf.put(cx, cy, d);
                    }
                }
            }
        }

        // Verify that EVERY pixel in 10..50 was updated to green (no checkerboard/holes of pure blue)
        for cy in 10..50 {
            for cx in 10..50 {
                let p = comp_buf.get(cx, cy);
                assert!(p.g > 0.95, "pixel at ({cx}, {cy}) must be green, got g={}", p.g);
                assert!(p.b < 0.05, "pixel at ({cx}, {cy}) must have no stale blue background, got b={}", p.b);
            }
        }

        // Pixels outside bounds must remain pure blue
        let outside_p = comp_buf.get(5, 5);
        assert_eq!(outside_p.b, 1.0);
        assert_eq!(outside_p.g, 0.0);
    }

    #[test]
    fn test_adjustment_layer_cache_key_varies_with_frame_and_backdrop() {
        use crate::raster::layer_cache_key;
        use compositor::{LayerStackEvaluator, SceneGraph};
        use project::{Composition, Project, TimeCode, Layer, Color};

        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc_end = TimeCode::from_frames(150, 30.0);

        let solid = Layer::solid("l1", "Solid", Color::RED, 100, 100, tc0, tc_end);
        comp.add_layer(solid).unwrap();

        let adj = Layer::adjustment("adj", "Adjustment", tc0, tc_end);
        comp.add_layer(adj).unwrap();

        project.add_composition(comp).unwrap();

        let graph = SceneGraph::from_project(&project, "c").unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &tc0);

        let adj_layer = stack.get_layer("adj").expect("adj layer");
        let solid_layer = stack.get_layer("l1").expect("solid layer");

        // Frame sensitivity: adjustment layers must invalidate when frame changes (underlying animation)
        let key_f0 = layer_cache_key(adj_layer, 0, 100, 100, false, 1111);
        let key_f1 = layer_cache_key(adj_layer, 1, 100, 100, false, 1111);
        assert_ne!(key_f0, key_f1, "Adjustment layer cache key MUST differ across frames to reflect underlying animations");

        // Backdrop sensitivity: adjustment layers with Normal blend mode must invalidate when backdrop changes
        let key_b2 = layer_cache_key(adj_layer, 0, 100, 100, false, 2222);
        assert_ne!(key_f0, key_b2, "Adjustment layer cache key MUST differ when backdrop changes");

        // Normal solid without keyframes should preserve frame-invariance
        let solid_f0 = layer_cache_key(solid_layer, 0, 100, 100, false, 1111);
        let solid_f1 = layer_cache_key(solid_layer, 1, 100, 100, false, 1111);
        assert_eq!(solid_f0, solid_f1, "Static solid layers should remain cached across frames when not animated");
    }

    #[test]
    fn test_adjustment_layer_without_effects_returns_empty() {
        use crate::raster::rasterize_layer;
        use compositor::{LayerStackEvaluator, SceneGraph};
        use project::{Composition, Project, TimeCode, Layer, Color, Effect, EffectType, Property};
        use std::collections::HashMap;

        let mut project = Project::new("p", "P");
        let mut comp = Composition::hd_1080p_30fps("c", "C", 5.0);
        let tc0 = TimeCode::from_frames(0, 30.0);
        let tc_end = TimeCode::from_frames(150, 30.0);

        let adj = Layer::adjustment("adj", "Adjustment", tc0, tc_end);
        comp.add_layer(adj).unwrap();
        project.add_composition(comp).unwrap();

        let graph = SceneGraph::from_project(&project, "c").unwrap();
        let evaluator = LayerStackEvaluator::new();
        let stack = evaluator.evaluate(&graph, &tc0);
        let adj_layer = stack.get_layer("adj").expect("adj layer");

        let assets = HashMap::new();
        // 1. Zero effects -> empty = true
        let (_buf, _avg, empty) = rasterize_layer(
            adj_layer,
            100.0,
            100.0,
            100,
            100,
            100.0,
            100.0,
            Color::BLACK,
            None,
            0.0,
            0,
            false,
            10.0,
            &assets,
        );
        assert!(empty, "Adjustment layer with zero effects should be empty pass-through");

        // 2. Active effect -> empty = false
        let mut project2 = Project::new("p2", "P2");
        let mut comp2 = Composition::hd_1080p_30fps("c2", "C2", 5.0);
        let mut adj2 = Layer::adjustment("adj2", "Adjustment", tc0, tc_end);
        adj2.effects.push(Effect {
            id: "blur".to_string(),
            name: "Gaussian Blur".to_string(),
            enabled: true,
            effect_type: EffectType::GaussianBlur {
                radius: Property::new("Radius", 10.0),
            },
        });
        comp2.add_layer(adj2).unwrap();
        project2.add_composition(comp2).unwrap();

        let graph2 = SceneGraph::from_project(&project2, "c2").unwrap();
        let stack2 = evaluator.evaluate(&graph2, &tc0);
        let adj_layer2 = stack2.get_layer("adj2").expect("adj layer 2");

        let (_buf, _avg, empty_with_fx) = rasterize_layer(
            adj_layer2,
            100.0,
            100.0,
            100,
            100,
            100.0,
            100.0,
            Color::BLACK,
            None,
            0.0,
            0,
            false,
            10.0,
            &assets,
        );
        assert!(!empty_with_fx, "Adjustment layer with active effects should produce renderable pixels");
    }

    #[gpui_kit::test]
    fn test_mask_removal_via_delete_mask(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());

        let mid = state_entity.update(cx, |s, cx| {
            let m = s.add_layer_sized_mask("layer_accent").expect("mask added");
            cx.notify();
            m
        });

        // Verify mask exists on layer
        state_entity.read_with(cx, |s, _| {
            let comp = s.active_composition().unwrap();
            let layer = comp.get_layer("layer_accent").unwrap();
            assert_eq!(layer.masks.len(), 1);
            assert_eq!(layer.masks[0].id, mid);
        });

        // Call delete_mask (the action wired to the trash button and context menu)
        state_entity.update(cx, |s, cx| {
            s.delete_mask("layer_accent", &mid).expect("mask deleted");
            cx.notify();
        });

        // Verify mask was cleanly removed
        state_entity.read_with(cx, |s, _| {
            let comp = s.active_composition().unwrap();
            let layer = comp.get_layer("layer_accent").unwrap();
            assert!(layer.masks.is_empty(), "Mask should be removed from layer");
        });
    }

    #[gpui_kit::test]
    fn test_smart_delete_shortcut_deletes_mask_when_active(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());

        let mid = state_entity.update(cx, |s, cx| {
            let m = s.add_layer_sized_mask("layer_accent").expect("mask added");
            s.set_active_mask_edit(Some(("layer_accent".to_string(), m.clone())));
            cx.notify();
            m
        });

        // Verify active_mask_edit is set
        state_entity.read_with(cx, |s, _| {
            assert_eq!(s.active_mask_edit, Some(("layer_accent".to_string(), mid.clone())));
        });

        // Simulate smart Delete key handler behavior:
        state_entity.update(cx, |s, cx| {
            if let Some((lid, mid)) = s.active_mask_edit.take() {
                let _ = s.remove_layer_mask(&lid, &mid);
            } else {
                let _ = s.delete_selected_layer();
            }
            cx.notify();
        });

        // Verify mask was deleted BUT layer remains intact
        state_entity.read_with(cx, |s, _| {
            let comp = s.active_composition().unwrap();
            let layer = comp.get_layer("layer_accent");
            assert!(layer.is_some(), "Layer must NOT be deleted when active_mask_edit was set");
            assert!(layer.unwrap().masks.is_empty(), "Mask should be removed from layer");
            assert!(s.active_mask_edit.is_none(), "active_mask_edit should be cleared");
        });
    }

    #[gpui_kit::test]
    fn test_viewport_canvas_comp_buffer_reuse(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());
        let panels = app_view.read_with(cx, |v, _| v.panels().clone());

        // Add adjustment layer so needs_canvas_comp is true
        state_entity.update(cx, |s, cx| {
            let _ = s.add_adjustment_layer(None);
            cx.notify();
        });
        cx.run_until_parked();

        // Render once and verify canvas_comp_buf is populated and retained
        panels.viewer.update(cx, |_panel, cx| {
            cx.notify();
        });
        cx.run_until_parked();

        panels.viewer.read_with(cx, |panel, _| {
            assert!(panel.canvas_comp_buf.is_some(), "canvas_comp_buf must be retained for reuse");
            let buf = panel.canvas_comp_buf.as_ref().unwrap();
            assert!(buf.w > 0 && buf.h > 0);
        });
    }

    #[gpui_kit::test]
    fn test_context_menu_outside_click_auto_dismissal(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let panels = app_view.read_with(cx, |v, _| v.panels().clone());

        // 1. ProjectPanel Context Menu
        panels.project.update(cx, |panel, cx| {
            panel.open_context_menu(crate::panels::ProjectContextMenuTarget::BinBackground, gpui::point(px(50.), px(50.)));
            cx.notify();
        });
        cx.run_until_parked();
        assert!(panels.project.read_with(cx, |p, _| p.context_menu.is_some()));

        // Outside click / backdrop dismiss
        panels.project.update(cx, |panel, cx| {
            panel.close_context_menu();
            cx.notify();
        });
        cx.run_until_parked();
        assert!(panels.project.read_with(cx, |p, _| p.context_menu.is_none()));

        // 2. CompositionViewerPanel Context Menu
        panels.viewer.update(cx, |panel, cx| {
            panel.open_context_menu(crate::panels::ViewerContextMenuTarget::Canvas, gpui::point(px(100.), px(100.)));
            cx.notify();
        });
        cx.run_until_parked();
        assert!(panels.viewer.read_with(cx, |p, _| p.context_menu.is_some()));

        panels.viewer.update(cx, |panel, cx| {
            panel.close_context_menu();
            cx.notify();
        });
        cx.run_until_parked();
        assert!(panels.viewer.read_with(cx, |p, _| p.context_menu.is_none()));

        // 3. TimelinePanel Context Menu
        panels.timeline.update(cx, |panel, cx| {
            panel.open_context_menu(crate::panels::ContextMenuTarget::EmptyTrackArea, gpui::point(px(150.), px(150.)));
            cx.notify();
        });
        cx.run_until_parked();
        assert!(panels.timeline.read_with(cx, |p, _| p.context_menu.is_some()));

        panels.timeline.update(cx, |panel, cx| {
            panel.close_context_menu();
            cx.notify();
        });
        cx.run_until_parked();
        assert!(panels.timeline.read_with(cx, |p, _| p.context_menu.is_none()));
    }

    #[gpui_kit::test]
    fn test_flexible_new_composition_creation(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());
        let panels = app_view.read_with(cx, |v, _| v.panels().clone());

        // Open New Composition dialog with flexible values
        panels.project.update(cx, |panel, cx| {
            panel.show_new_comp = true;
            panel.nc_name = "My Custom 4K".to_string();
            panel.nc_w = 3840;
            panel.nc_h = 2160;
            panel.nc_fps = 59.94;
            panel.nc_dur = 15.0;
            panel.nc_bg = 2; // Transparent
            cx.notify();
        });
        cx.run_until_parked();

        assert!(panels.project.read_with(cx, |p, _| p.show_new_comp));

        // Test swapping dimensions (3840x2160 -> 2160x3840)
        panels.project.update(cx, |panel, cx| {
            std::mem::swap(&mut panel.nc_w, &mut panel.nc_h);
            cx.notify();
        });
        panels.project.read_with(cx, |p, _| {
            assert_eq!(p.nc_w, 2160);
            assert_eq!(p.nc_h, 3840);
        });

        // Swap back to 3840x2160
        panels.project.update(cx, |panel, cx| {
            std::mem::swap(&mut panel.nc_w, &mut panel.nc_h);
            cx.notify();
        });

        // Simulate creation with these custom parameters
        let (cw, ch, cfps, cdur, cbg, cname) = panels.project.read_with(cx, |p, _| {
            (p.nc_w, p.nc_h, p.nc_fps, p.nc_dur, p.nc_bg, p.nc_name.clone())
        });
        let bg = match cbg {
            1 => project::Color::WHITE,
            2 => project::Color::TRANSPARENT,
            3 => project::Color::from_rgba_u8(38, 38, 38, 255),
            _ => project::Color::BLACK,
        };
        state_entity.update(cx, |s, cx| {
            let res = s.add_composition(&cname, cw, ch, cfps, cdur, bg);
            assert!(res.is_ok());
            cx.notify();
        });
        panels.project.update(cx, |p, cx| {
            p.show_new_comp = false;
            cx.notify();
        });
        cx.run_until_parked();

        // Verify composition was created with exact custom parameters
        state_entity.read_with(cx, |s, _| {
            let comp = s.active_composition().unwrap();
            assert_eq!(comp.name, "My Custom 4K");
            assert_eq!(comp.width, 3840);
            assert_eq!(comp.height, 2160);
            assert!((comp.frame_rate - 59.94).abs() < 1e-4);
            assert_eq!(comp.background_color, project::Color::TRANSPARENT);
        });
        assert!(!panels.project.read_with(cx, |p, _| p.show_new_comp));
    }

    #[gpui_kit::test]
    fn test_new_comp_dialog_outside_click_dismissal(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let panels = app_view.read_with(cx, |v, _| v.panels().clone());

        panels.project.update(cx, |panel, cx| {
            panel.show_new_comp = true;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(panels.project.read_with(cx, |p, _| p.show_new_comp));

        // Outside click backdrop dismisses the dialog
        panels.project.update(cx, |panel, cx| {
            panel.show_new_comp = false;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(!panels.project.read_with(cx, |p, _| p.show_new_comp));
    }

    #[gpui_kit::test]
    fn test_scrubbing_frame_quantization_skips_redundant_seeks(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());

        // At 30 FPS, frame 0 is [0.0, 0.0166s], frame 1 starts at 0.0167s...
        // Seeking from 0.001s to 0.005s should NOT advance the quantized visual frame
        state_entity.update(cx, |s, _| {
            s.seek(0.0);
            assert_eq!(s.clock.current_frame(), 0);

            // Sub-frame nudge within frame 0: should return false (skip seek notify)
            let changed1 = s.scrub_frame_quantized(0.005);
            assert!(!changed1, "Sub-frame scrub within same frame must return false");
            assert_eq!(s.clock.current_frame(), 0);

            // Advancing to 0.040s (frame 1 at 30 FPS): should return true
            let changed2 = s.scrub_frame_quantized(0.040);
            assert!(changed2, "Scrub advancing to next frame must return true");
            assert_eq!(s.clock.current_frame(), 1);

            // Sub-frame nudge within frame 1: should return false
            let changed3 = s.scrub_frame_quantized(0.045);
            assert!(!changed3, "Sub-frame scrub within frame 1 must return false");
            assert_eq!(s.clock.current_frame(), 1);
        });
    }

    #[gpui_kit::test]
    fn test_preview_divisor_adaptive_proxy_resolution(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());

        state_entity.update(cx, |s, _| {
            // Idle divisor matches user quality preference (default Auto = 1 when paused)
            assert!(!s.preview_fast);
            assert!(!s.is_playing);
            assert_eq!(s.preview_divisor(), 1);

            // During preview_fast gesture (e.g. gizmo drag or ruler scrub)
            s.preview_fast = true;
            let d_fast = s.preview_divisor();
            assert!(d_fast >= 2, "preview_divisor during fast gesture must downsample for smoothness (got {d_fast})");

            // On mouse up / gesture release
            s.preview_fast = false;
            assert_eq!(s.preview_divisor(), 1, "preview_divisor must restore to full quality when gesture ends");

            // During playback under Full quality preference, divisor must stay 1 (no blurry downsampling!)
            s.set_preview_quality(crate::state::PreviewQuality::Full);
            s.is_playing = true;
            let d_play = s.preview_divisor();
            assert_eq!(d_play, 1, "preview_divisor during Full quality playback must be 1 to prevent blurry clips");

            s.is_playing = false;
            assert_eq!(s.preview_divisor(), 1);

            // Half quality preference gives divisor 2 during playback
            s.set_preview_quality(crate::state::PreviewQuality::Half);
            s.is_playing = true;
            assert_eq!(s.preview_divisor(), 2);
            s.is_playing = false;
            assert_eq!(s.preview_divisor(), 2);

            // Quarter quality gives divisor 4
            s.set_preview_quality(crate::state::PreviewQuality::Quarter);
            assert_eq!(s.preview_divisor(), 4);

            // Restore Full
            s.set_preview_quality(crate::state::PreviewQuality::Full);
            assert_eq!(s.preview_divisor(), 1);
        });
    }

    #[gpui_kit::test]
    fn test_playback_maintains_full_resolution_under_full_quality(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());

        state_entity.update(cx, |s, _| {
            s.set_preview_quality(crate::state::PreviewQuality::Full);
            s.is_playing = true;

            // Full HD composition dimensions
            let l_w = 1920.0f32;
            let l_h = 1080.0f32;
            let qdiv = s.preview_divisor().max(1);

            let rw = ((l_w / qdiv as f32).ceil().max(1.0) as u32).min(2048);
            let rh = ((l_h / qdiv as f32).ceil().max(1.0) as u32).min(2048);

            // Must preserve 100% full resolution on Full quality playback (not downsampled to 480x270 or 240x135)
            assert_eq!(qdiv, 1, "Playback resolution divisor must be 1 under Full quality");
            assert_eq!(rw, 1920, "Raster width during Full playback must be 1920");
            assert_eq!(rh, 1080, "Raster height during Full playback must be 1080");

            // Auto quality degrades while playing (governor) and restores on pause.
            s.set_preview_quality(crate::state::PreviewQuality::Auto);
            let auto_qdiv = s.preview_divisor().max(1);
            assert_eq!(auto_qdiv, 2, "Auto quality degrades to half res during playback");
            s.is_playing = false;
            assert_eq!(s.preview_divisor().max(1), 1, "Auto quality restores full res on pause");
        });
    }

    #[gpui_kit::test]
    fn test_layer_translation_preserves_raster_cache(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        cx.run_until_parked();

        let state_entity = app_view.read_with(cx, |v, _| v.state().clone());

        state_entity.update(cx, |s, _| {
            s.select_layer(Some("layer_bg".to_string()));
            let initial_pos = s.selected_layer().unwrap().transform.position.value;

            // Move the layer by nudging position
            s.nudge_position(50.0, 30.0);
            let new_pos = s.selected_layer().unwrap().transform.position.value;
            assert_eq!(new_pos.x, initial_pos.x + 50.0);
            assert_eq!(new_pos.y, initial_pos.y + 30.0);
        });
    }
    /// Image ▸ Export Composition must open, list every supported format with
    /// its own test id, remember the picked format, and close from Cancel.
    #[gpui_kit::test]
    fn test_export_dialog_formats_and_cancel(cx: &mut TestAppContext) {
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
            view.show_export = true;
            cx.notify();
        });

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("export_dialog").visible());
            assert!(window.find("export_comp_meta").visible());
            assert!(window.find("export_format_hint").visible());
            assert!(window.find("export_render_btn").visible());
            assert!(window.find("export_cancel_btn").visible());
            for i in 0..export::ExportFormat::all().len() {
                let id = SharedString::from(format!("export_format_{i}"));
                assert!(window.find(id).visible(), "missing format tile {i}");
            }
            window.click("export_format_3", cx);
        })
        .expect("update_window failed");

        assert_eq!(
            app_view.read_with(cx, |view, _| view.export_format_idx),
            3,
            "clicking a format tile records the choice on AppView"
        );
        assert_eq!(
            export::ExportFormat::all()[3],
            export::ExportFormat::PngSequence
        );

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("export_cancel_btn", cx);
        })
        .expect("update_window failed");
        assert!(
            !app_view.read_with(cx, |view, _| view.show_export),
            "Cancel closes the export dialog"
        );
    }

    /// Ctrl+M is the desktop shortcut for the export dialog, Escape closes it.
    #[gpui_kit::test]
    fn test_export_shortcut_and_escape(cx: &mut TestAppContext) {
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

        cx.update_window(handle.into(), |_, window, cx| {
            let focus_handle = app_view.read(cx).focus_handle().clone();
            window.focus(&focus_handle, cx);
            window.render_frame(cx);
            window.dispatch_keystroke(gpui_kit::Keystroke::parse("ctrl-m").unwrap(), cx);
        })
        .expect("update_window failed");

        assert!(
            app_view.read_with(cx, |view, _| view.show_export),
            "Ctrl+M opens the export dialog"
        );

        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("export_dialog").visible());
            window.dispatch_keystroke(gpui_kit::Keystroke::parse("escape").unwrap(), cx);
        })
        .expect("update_window failed");

        assert!(
            !app_view.read_with(cx, |view, _| view.show_export),
            "Escape dismisses the export dialog"
        );
    }

    /// The whole render pipeline end-to-end on the real project: evaluate the
    /// composition, rasterize each frame, and write a PNG sequence to disk.
    #[test]
    fn test_render_project_to_png_sequence_end_to_end() {
        let state = EditorState::new();
        let comp_id = state.active_comp_id.clone();
        let comp = state
            .active_composition()
            .cloned()
            .expect("default project has an active composition");
        assert!(!comp.layers.is_empty(), "default comp has layers to draw");
        let (cw, ch) = (comp.width, comp.height);

        let dir = std::env::temp_dir().join("motion_studio_render_job_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let project = state.project.clone();
        let job = export::ExportJob {
            comp_id: comp_id.clone(),
            format: export::ExportFormat::PngSequence,
            output: dir.join("clip"),
            start_frame: Some(0),
            end_frame: Some(2),
            gif_max_side: 640,
            gif_fps: 15.0,
        };
        let cid = comp_id.clone();
        let res = export::render_job(
            &project,
            &job,
            |fr, _tc, w, h| EditorState::render_export_frame(&project, &cid, fr, w, h),
            None,
        )
        .expect("render the real project to a PNG sequence");

        assert_eq!(res.frames, 3);
        let pngs: Vec<std::path::PathBuf> = std::fs::read_dir(&res.primary)
            .expect("sequence folder")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "png"))
            .collect();
        assert_eq!(pngs.len(), 3, "one PNG per requested frame");

        // Every frame is a decodable PNG at full composition resolution, and not
        // blank: the default composition draws solids over the background.
        for path in &pngs {
            let img = image::open(path).expect("decode exported frame");
            assert_eq!(
                (img.width(), img.height()),
                (cw, ch),
                "export must render at composition resolution"
            );
            let rgba = img.to_rgba8();
            let stride = ((cw * ch / 2048).max(1)) as usize;
            let distinct: std::collections::HashSet<[u8; 4]> = rgba
                .pixels()
                .step_by(stride)
                .map(|p| p.0)
                .collect();
            assert!(
                distinct.len() > 1,
                "frame {} is a flat blank image",
                path.display()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A single-frame still export writes exactly one PNG at the composition size.
    #[test]
    fn test_render_project_to_png_still_end_to_end() {
        let state = EditorState::new();
        let comp_id = state.active_comp_id.clone();
        let comp = state.active_composition().cloned().expect("active comp");
        let (cw, ch) = (comp.width, comp.height);

        let dir = std::env::temp_dir().join("motion_studio_render_still_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let out = dir.join("frame.png");

        let project = state.project.clone();
        let job = export::ExportJob {
            comp_id: comp_id.clone(),
            format: export::ExportFormat::PngStill,
            output: out.clone(),
            start_frame: Some(3),
            end_frame: Some(3),
            gif_max_side: 640,
            gif_fps: 15.0,
        };
        let cid = comp_id.clone();
        let res = export::render_job(
            &project,
            &job,
            |fr, _tc, w, h| EditorState::render_export_frame(&project, &cid, fr, w, h),
            None,
        )
        .expect("render a single frame");

        assert_eq!(res.frames, 1);
        assert_eq!(res.primary, out);
        let img = image::open(&out).expect("decode still");
        assert_eq!((img.width(), img.height()), (cw, ch));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[gpui_kit::test]
    fn test_modifier_graph_window_rendering_and_interactions(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::change(ThemeMode::Dark, None, cx);
        });

        let state = cx.new(|_cx| EditorState::new());
        let layer_id = "layer_accent".to_string();
        let prop_path = "transform.position.x".to_string();

        let mut mg_view = None;
        let (root, _window) = cx.add_window_view(|window, cx| {
            window.activate_window();
            window.set_window_title("Modifier Graph Test");
            Theme::change(ThemeMode::Dark, Some(window), cx);
            let view = cx.new(|cx| {
                crate::modifier_graph_view::ModifierGraphView::new(
                    state.clone(),
                    layer_id.clone(),
                    prop_path.clone(),
                    window,
                    cx,
                )
            });
            mg_view = Some(view.clone());
            Root::new(view, window, cx)
        });

        cx.run_until_parked();
        root.read_with(cx, |_root, _cx| ());

        let view_entity = mg_view.expect("view created");
        view_entity.update(cx, |this, cx| {
            // Add a Math node
            this.add_node(
                project::modifier::NodeKind::Math {
                    op: project::modifier::MathOp::Multiply,
                    default_b: 2.0,
                },
                cx,
            );
            // Reconnect: connect factor node to the math node, and math to output
            let math_node_id = this
                .graph
                .nodes
                .iter()
                .find(|n| matches!(n.kind, project::modifier::NodeKind::Math { .. }))
                .unwrap()
                .id
                .clone();
            // Interactive drag-and-drop: drag wire from node_factor's output socket and drop onto math node card
            let math_node = this.graph.get_node(&math_node_id).unwrap();
            let (m_pos_x, m_pos_y) = (math_node.pos_x, math_node.pos_y);

            this.connecting_wire = Some(crate::modifier_graph_view::WireDragState {
                is_from_input: false,
                node_id: "node_factor".to_string(),
                socket_name: "factor".to_string(),
                cur_x: m_pos_x + 50.0,
                cur_y: m_pos_y + 30.0,
                start_x: 40.0 + 186.0,
                start_y: 220.0 + 41.0,
            });
            let connected = this.try_finish_wire_connection(m_pos_x + 50.0, m_pos_y + 30.0, 0.0, 0.0, cx);
            assert!(connected, "should snap and connect output socket to math node input socket 'a'");
            assert!(this.connecting_wire.is_none());
            assert!(this.graph.connections.iter().any(|c| c.from_node == "node_factor" && c.to_node == math_node_id && c.to_socket == "a"));

            // Drag from math node's output socket to node_output
            let out_node = this.graph.get_node("node_output").unwrap();
            let (o_pos_x, o_pos_y) = (out_node.pos_x, out_node.pos_y);

            this.connecting_wire = Some(crate::modifier_graph_view::WireDragState {
                is_from_input: false,
                node_id: math_node_id.clone(),
                socket_name: "result".to_string(),
                cur_x: o_pos_x + 20.0,
                cur_y: o_pos_y + 40.0,
                start_x: m_pos_x + 186.0,
                start_y: m_pos_y + 41.0,
            });
            let connected_out = this.try_finish_wire_connection(o_pos_x + 20.0, o_pos_y + 40.0, 0.0, 0.0, cx);
            assert!(connected_out, "should connect math output to output node");
            assert!(this.connecting_wire.is_none());

            // Verify evaluation with factor = 0.5: 0.5 * 2.0 = 1.0
            let out = this.graph.evaluate(100.0, 0.5);
            assert_eq!(out, 1.0);

            // Reconnect input socket: disconnect socket 'a' and drag from input socket 'a' back to node_base:value
            this.disconnect_socket(&math_node_id, "a", cx);
            let base_node = this.graph.get_node("node_base").unwrap();
            let (b_pos_x, b_pos_y) = (base_node.pos_x, base_node.pos_y);

            this.connecting_wire = Some(crate::modifier_graph_view::WireDragState {
                is_from_input: true,
                node_id: math_node_id.clone(),
                socket_name: "a".to_string(),
                cur_x: b_pos_x + 186.0,
                cur_y: b_pos_y + 41.0,
                start_x: m_pos_x + 14.0,
                start_y: m_pos_y + 41.0,
            });
            let connected_base = this.try_finish_wire_connection(b_pos_x + 186.0, b_pos_y + 41.0, 0.0, 0.0, cx);
            assert!(connected_base, "should reconnect input socket to base value node");
            assert!(this.connecting_wire.is_none());

            // Verify evaluation with base = 50.0: 50.0 * 2.0 = 100.0
            let out2 = this.graph.evaluate(50.0, 0.5);
            assert_eq!(out2, 100.0);
        });
    }

    #[gpui_kit::test]
    fn test_modifier_graph_wire_renders_single_canvas(cx: &mut TestAppContext) {
        // Committed wires render as one continuous canvas path, not bead
        // divs (plus the midpoint disconnect badge).
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let state = cx.new(|_| EditorState::new());
        let mut mg_view = None;
        let handle = cx.open_window(size(px(1080.), px(720.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| {
                crate::modifier_graph_view::ModifierGraphView::new(
                    state.clone(),
                    "layer_accent".to_string(),
                    "transform.position.x".to_string(),
                    window,
                    cx,
                )
            });
            mg_view = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = mg_view.expect("graph view created");
        let math_id = view.update(cx, |this, cx| {
            this.add_node(
                project::modifier::NodeKind::Math {
                    op: project::modifier::MathOp::Multiply,
                    default_b: 2.0,
                },
                cx,
            );
            let math_node_id = this
                .graph
                .nodes
                .iter()
                .find(|n| matches!(n.kind, project::modifier::NodeKind::Math { .. }))
                .unwrap()
                .id
                .clone();
            let math_node = this.graph.get_node(&math_node_id).unwrap();
            let (m_pos_x, m_pos_y) = (math_node.pos_x, math_node.pos_y);
            this.connecting_wire = Some(crate::modifier_graph_view::WireDragState {
                is_from_input: false,
                node_id: "node_factor".to_string(),
                socket_name: "factor".to_string(),
                cur_x: m_pos_x + 50.0,
                cur_y: m_pos_y + 30.0,
                start_x: 40.0 + 186.0,
                start_y: 220.0 + 41.0,
            });
            assert!(
                this.try_finish_wire_connection(m_pos_x + 50.0, m_pos_y + 30.0, 0.0, 0.0, cx),
                "factor must connect to math.a"
            );
            math_node_id
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let wire_id = SharedString::from(format!("wire_committed_{math_id}_a"));
            assert!(window.find(wire_id).visible(), "single canvas wire must render");
            assert!(
                window.find(SharedString::from(format!("disc_{math_id}_a"))).visible(),
                "disconnect badge must survive"
            );
        })
        .expect("update_window failed");
    }

    #[gpui_kit::test]
    fn test_modifier_graph_right_click_pan_releases(cx: &mut TestAppContext) {        // Right-drag panning the canvas must end on right-button release
        // (it used to stay stuck to the cursor: only Left-up cleared it).
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let state = cx.new(|_| EditorState::new());
        let mut mg_view = None;
        let handle = cx.open_window(size(px(1080.), px(720.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| {
                crate::modifier_graph_view::ModifierGraphView::new(
                    state.clone(),
                    "layer_accent".to_string(),
                    "transform.position.x".to_string(),
                    window,
                    cx,
                )
            });
            mg_view = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = mg_view.expect("graph view created");
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click("modifier_graph_canvas", cx);
        })
        .expect("update_window failed");
        view.read_with(cx, |v, _| {
            assert!(v.panning_canvas.is_none(), "right-button release must end panning");
        });
    }

    #[gpui_kit::test]
    fn test_modifier_graph_click_opens_typed_param_entry(cx: &mut TestAppContext) {
        // Click (no drag) on a param value opens keyboard entry; Enter
        // commits the typed number, Escape cancels without changes.
        use gpui_kit::test::TestWindowExt;

        cx.update(gpui_kit::init);
        let state = cx.new(|_| EditorState::new());
        let mut mg_view = None;
        let handle = cx.open_window(size(px(1080.), px(720.)), |window, cx| {
            window.activate_window();
            let view = cx.new(|cx| {
                crate::modifier_graph_view::ModifierGraphView::new(
                    state.clone(),
                    "layer_accent".to_string(),
                    "transform.position.x".to_string(),
                    window,
                    cx,
                )
            });
            mg_view = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = mg_view.expect("graph view created");
        let const_id = view.update(cx, |v, cx| {
            v.add_node(project::modifier::NodeKind::Constant { value: 1.0 }, cx);
            v.graph
                .nodes
                .iter()
                .find(|n| matches!(n.kind, project::modifier::NodeKind::Constant { .. }))
                .unwrap()
                .id
                .clone()
        });
        let pill = SharedString::from(format!("mg_param_{const_id}_value"));
        let editor_id = SharedString::from(format!("mg_param_editor_{const_id}_value"));
        // Click without dragging opens the editor prefilled with the value.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(pill.clone()).visible());
            window.click(pill.clone(), cx);
            window.render_frame(cx);
            assert!(window.find(editor_id.clone()).visible());
        })
        .expect("update_window failed");
        // Typing 2.5 + Enter commits it to the node.
        cx.update_window(handle.into(), |_, window, cx| {
            let ed = view.read_with(cx, |v, _| v.param_editor.clone().expect("editor open"));
            ed.update(cx, |st, cx| {
                st.set_value("2.5", window, cx);
            });
            window.render_frame(cx);
            window.dispatch_keystroke(gpui_kit::Keystroke::parse("enter").unwrap(), cx);
        })
        .expect("update_window failed");
        view.read_with(cx, |v, _| {
            let n = v.graph.get_node(&const_id).unwrap();
            assert!(
                matches!(n.kind, project::modifier::NodeKind::Constant { value } if (value - 2.5).abs() < 1e-6),
                "typed value must commit to the node"
            );
            assert!(v.editing_param.is_none(), "editor closes after commit");
        });
        // Escape cancels without changes.
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(pill.clone(), cx);
            window.render_frame(cx);
            assert!(window.find(editor_id.clone()).visible());
            window.dispatch_keystroke(gpui_kit::Keystroke::parse("escape").unwrap(), cx);
        })
        .expect("update_window failed");
        view.read_with(cx, |v, _| {
            let n = v.graph.get_node(&const_id).unwrap();
            assert!(
                matches!(n.kind, project::modifier::NodeKind::Constant { value } if (value - 2.5).abs() < 1e-6),
                "escape must keep the old value"
            );
            assert!(v.editing_param.is_none(), "editor closes on escape");
        });
    }

    #[test]
    fn test_effect_path_helpers_for_modifier_graph() {
        use crate::state::EditorState;

        // Scrub keys normalize to the canonical paths the graph, links,
        // and compositor share; other paths pass through untouched.
        assert_eq!(
            crate::panels::effect_path_for_value_key("fx:abc:radius:100"),
            "effect:abc:radius"
        );
        assert_eq!(
            crate::panels::effect_path_for_value_key("fx:abc:radius"),
            "effect:abc:radius"
        );
        assert_eq!(
            crate::panels::effect_path_for_value_key("transform.position.x"),
            "transform.position.x"
        );

        // Base/live reads resolve effect scalars (graph editor display).
        let mut s = EditorState::new();
        s.select_layer(Some("layer_accent".to_string()));
        let fx = s
            .add_effect_to_selected_layer(project::EffectType::gaussian_blur(7.0))
            .unwrap();
        let path = format!("effect:{fx}:radius");
        assert!((s.get_layer_property_base_value("layer_accent", &path) - 7.0).abs() < 1e-6);
        assert!((s.get_layer_property_live_value("layer_accent", &path) - 7.0).abs() < 1e-6);
        assert_eq!(s.get_layer_property_base_value("layer_accent", "effect:missing:radius"), 0.0);
    }

    #[gpui_kit::test]
    fn test_timeline_context_menu_has_modifier_graph_option(cx: &mut TestAppContext) {
        let (root, app_view) = setup_test_window(cx);
        let timeline = app_view.read_with(cx, |view, _cx| view.panels.timeline.clone());
        timeline.update(cx, |this, _cx| {
            this.open_context_menu(
                crate::panels::ContextMenuTarget::Property {
                    layer_id: "layer_accent".to_string(),
                    prop_path: "transform.position",
                },
                gpui_kit::point(px(100.0), px(100.0)),
            );
        });

        root.update(cx, |_root, cx| {
            cx.notify();
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn test_universal_modifier_graph_evaluation_and_ui_indicators(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        let state_entity = app_view.read_with(cx, |view, _| view.state().clone());

        // 1. Add effects and modifier graphs to layer_accent
        state_entity.update(cx, |s, _| {
            s.selected_layer_id = Some("layer_accent".to_string());
            let comp = s.active_composition_mut().unwrap();
            let layer = comp.get_layer_mut("layer_accent").unwrap();
            layer.add_effect(project::Effect::vignette("fx_vig_test", 0.5, 0.5));
            layer.add_effect(project::Effect::exposure("fx_exp_test", 0.0));

            let make_const_graph = |val: f32| project::modifier::ModifierGraph {
                nodes: vec![
                    project::modifier::ModifierNode::new("c", 0.0, 0.0, project::modifier::NodeKind::Constant { value: val }),
                    project::modifier::ModifierNode::new("out", 200.0, 0.0, project::modifier::NodeKind::Output),
                ],
                connections: vec![project::modifier::NodeConnection::new("c", "value", "out", "result")],
            };

            layer.set_modifier_graph("effect:fx_vig_test:amount", make_const_graph(0.92));
            layer.set_modifier_graph("effect:fx_exp_test:exposure", make_const_graph(3.25));
        });

        // 2. Verify live value resolution evaluates the modifier graphs!
        state_entity.read_with(cx, |s, _| {
            let vig_live = s.get_layer_property_live_value("layer_accent", "effect:fx_vig_test:amount");
            assert!((vig_live - 0.92).abs() < 1e-4, "Live value must evaluate modifier graph on vignette amount: got {}", vig_live);

            let exp_live = s.get_layer_property_live_value("layer_accent", "effect:fx_exp_test:exposure");
            assert!((exp_live - 3.25).abs() < 1e-4, "Live value must evaluate modifier graph on exposure: got {}", exp_live);

            assert!(s.get_layer_modifier_graph("layer_accent", "effect:fx_vig_test:amount").is_some());
            assert!(s.get_layer_modifier_graph("layer_accent", "effect:fx_exp_test:exposure").is_some());
        });

        // 3. Render and check that the effect graph badges exist
        let window_handle = cx.windows()[0];
        let _ = cx.update_window(window_handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("effect_graph_badge_fx_vig_test").visible(), "Vignette graph badge must exist in properties panel");
            assert!(window.find("effect_graph_badge_fx_exp_test").visible(), "Exposure graph badge must exist in properties panel");
        });

        // 4. Test removal
        state_entity.update(cx, |s, _| {
            s.remove_layer_modifier_graph("layer_accent", "effect:fx_vig_test:amount");
            assert!(s.get_layer_modifier_graph("layer_accent", "effect:fx_vig_test:amount").is_none());
            // Live value falls back to base
            let vig_fallback = s.get_layer_property_live_value("layer_accent", "effect:fx_vig_test:amount");
            assert_eq!(vig_fallback, 0.5);
        });

        let _ = cx.update_window(window_handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("effect_graph_badge_fx_vig_test").is_none(), "Badge must disappear when modifier graph is removed");
        });
    }

    #[gpui_kit::test]
    fn test_layer_and_value_context_menu_operations(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        let state_entity = app_view.read_with(cx, |view, _| view.state().clone());

        // 1. Layer duplicate and delete
        state_entity.update(cx, |s, _| {
            let initial_count = s.active_composition().unwrap().layers.len();
            let new_id = s.duplicate_layer("layer_accent").expect("duplicate layer");
            assert_eq!(s.active_composition().unwrap().layers.len(), initial_count + 1);
            s.delete_layer(&new_id).expect("delete layer");
            assert_eq!(s.active_composition().unwrap().layers.len(), initial_count);
        });

        // 2. Value context menu: Copy link, Paste link, Reset value
        state_entity.update(cx, |s, _| {
            // Modify transform.position on layer_accent
            let comp = s.active_composition_mut().unwrap();
            let layer = comp.get_layer_mut("layer_accent").unwrap();
            layer.transform.position.clear_keyframes();
            layer.transform.position.value_mut().x = 123.0;

            // Add a test modifier graph
            let mut graph = project::modifier::ModifierGraph::default_passthrough();
            graph.add_node(project::modifier::ModifierNode::new(
                "math_1",
                100.0,
                100.0,
                project::modifier::NodeKind::Math {
                    op: project::modifier::MathOp::Multiply,
                    default_b: 2.0,
                },
            ));
            layer.set_modifier_graph("transform.position.x", graph);

            // Copy link
            s.copy_property_link("layer_accent", "transform.position.x");
            assert!(s.copied_property_link.is_some());

            // Paste link to background layer's position.x
            let ok = s.paste_property_link("layer_bg", "transform.position.x");
            assert!(ok);
            let comp = s.active_composition().unwrap();
            let bg_layer = comp.get_layer("layer_bg").unwrap();
            assert_eq!(bg_layer.transform.position.value().x, 123.0);
            assert!(bg_layer.get_modifier_graph("transform.position.x").is_some());

            // Reset value on layer_accent
            s.reset_layer_property("layer_accent", "transform.position.x");
            let comp = s.active_composition().unwrap();
            let accent_layer = comp.get_layer("layer_accent").unwrap();
            assert_eq!(accent_layer.transform.position.value().x, accent_layer.transform.position.default_value().x);
            assert!(accent_layer.get_modifier_graph("transform.position.x").is_none());
        });

        // 3. Project context menu: Add to composition, Delete solid, Delete all keyframes
        state_entity.update(cx, |s, _| {
            // Add keyframes to layer_accent
            s.toggle_layer_property_keyframe_at_playhead("layer_accent", "transform.position");
            assert!(!s.active_composition().unwrap().get_layer("layer_accent").unwrap().transform.position.keyframes().is_empty());

            // Clear all keyframes
            s.delete_all_keyframes_on_layer("layer_accent");
            assert!(s.active_composition().unwrap().get_layer("layer_accent").unwrap().transform.position.keyframes().is_empty());
        });

        // 4. Viewport context menu: Zoom 150%, Zoom to Fit, Toggle Overlay/Gizmo
        let viewer = app_view.read_with(cx, |view, _| view.panels.composition.clone());
        viewer.update(cx, |v, _| {
            assert!(v.overlays_enabled);
            assert_eq!(v.zoom_factor, None);

            // Zoom to 150%
            v.zoom_factor = Some(1.5);
            assert_eq!(v.zoom_factor, Some(1.5));

            // Zoom to Fit
            v.zoom_factor = None;
            assert_eq!(v.zoom_factor, None);

            // Toggle overlay
            v.overlays_enabled = false;
            assert!(!v.overlays_enabled);
            v.overlays_enabled = true;
            assert!(v.overlays_enabled);
        });

        // Verify PropertiesPanel context menu state
        let props = app_view.read_with(cx, |view, _| view.panels.properties.clone());
        props.update(cx, |p, _| {
            assert!(p.context_menu.is_none());
            p.context_menu = Some(("layer_accent".to_string(), "transform.position.x".to_string(), gpui_kit::point(px(50.0), px(50.0))));
            assert!(p.context_menu.is_some());
            p.close_context_menu();
            assert!(p.context_menu.is_none());
        });

        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn test_property_links_live_workflow(cx: &mut TestAppContext) {
        let (_root, app_view) = setup_test_window(cx);
        let state = app_view.read_with(cx, |view, _| view.state.clone());

        state.update(cx, |s, _| {
            // Setup two layers: Driver (e.g. Master Rotation on layer_bg) and Follower (e.g. layer_accent)
            // 1. Step 1: Copy Driver with Property Links
            s.copy_property_link("layer_bg", "transform.rotation");
            assert_eq!(
                s.copied_property_link,
                Some(("layer_bg".to_string(), "transform.rotation".to_string()))
            );
            assert!(s.property_link_toast.as_ref().unwrap().contains("Copied with Property Links"));

            // 2. Step 2: Paste as Property Link onto layer_accent's rotation
            let pasted = s.paste_property_link("layer_accent", "transform.rotation");
            assert!(pasted);

            // 3. Step 3: Verify link badge, locked input state, and live sync
            assert!(s.is_layer_property_linked("layer_accent", "transform.rotation"));
            assert!(s.is_layer_property_linked("layer_accent", "rotation"));

            // Verify live sync: Initial values match
            let driver_val = s.get_layer_property_live_value("layer_bg", "transform.rotation");
            let follower_val = s.get_layer_property_live_value("layer_accent", "transform.rotation");
            assert_eq!(driver_val, follower_val);

            // Mutate the driver layer's property (e.g. rotate driver by 45 degrees)
            s.select_layer(Some("layer_bg".to_string()));
            s.nudge_rotation(45.0);

            let new_driver_val = s.get_layer_property_live_value("layer_bg", "transform.rotation");
            assert_eq!(new_driver_val, 45.0);

            // Live Sync: follower immediately reflects driver value in real-time
            let new_follower_val = s.get_layer_property_live_value("layer_accent", "transform.rotation");
            assert_eq!(new_follower_val, 45.0);

            // 4. Test Chaining: Third layer links to the follower
            s.copy_property_link("layer_accent", "transform.rotation");
            let pasted_third = s.paste_property_link("layer_badge", "transform.rotation");
            assert!(pasted_third);
            assert!(s.is_layer_property_linked("layer_badge", "transform.rotation"));
            let chained_val = s.get_layer_property_live_value("layer_badge", "transform.rotation");
            assert_eq!(chained_val, 45.0);

            // Mutate driver again to 90 degrees
            s.nudge_rotation(45.0);
            assert_eq!(s.get_layer_property_live_value("layer_bg", "transform.rotation"), 90.0);
            assert_eq!(s.get_layer_property_live_value("layer_accent", "transform.rotation"), 90.0);
            assert_eq!(s.get_layer_property_live_value("layer_badge", "transform.rotation"), 90.0);

            // 5. Test Unlinking / Reset Property
            s.remove_property_link("layer_accent", "transform.rotation");
            assert!(!s.is_layer_property_linked("layer_accent", "transform.rotation"));

            // Resetting driver does not alter unlinked layer
            s.reset_layer_property("layer_accent", "transform.rotation");
            assert_eq!(s.get_layer_property_live_value("layer_accent", "transform.rotation"), 0.0);
            // Driver is still 90.0
            assert_eq!(s.get_layer_property_live_value("layer_bg", "transform.rotation"), 90.0);
        });

        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn test_mask_control_point_downward_drag_moves_down(cx: &mut TestAppContext) {
        use project::Vec2;
        let (_root, app_view) = setup_test_window(cx);
        let state = app_view.read_with(cx, |view, _| view.state.clone());

        // Setup a layer with a mask
        let (lid, mid) = state.update(cx, |s, _| {
            let layer_id = "layer_accent";
            let mid = s.add_mask_to_layer(layer_id).expect("mask added");
            (layer_id.to_string(), mid)
        });

        // Test downward drag: moving mouse down (+50px on Y) must move mask point down (+Y in layer local space)
        state.update(cx, |s, _| {
            let initial_pos = s.active_composition().unwrap().get_layer(&lid).unwrap().get_mask(&mid).unwrap().path.value.points[0].pos;

            // Simulating downward move (+50.0 down in layer local space)
            let new_loc = Vec2::new(initial_pos.x, initial_pos.y + 50.0);
            let res = s.move_mask_point_live(&lid, &mid, 0, new_loc);
            assert!(res.is_ok());

            let updated_pos = s.active_composition().unwrap().get_layer(&lid).unwrap().get_mask(&mid).unwrap().path.value.points[0].pos;
            assert_eq!(updated_pos.y, initial_pos.y + 50.0, "downward drag must increase Y coordinate (downwards, not inverted)");
            assert!(updated_pos.y > initial_pos.y);
        });

        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn test_project_manager_and_new_comp_custom_inputs(cx: &mut TestAppContext) {
        let (root, app_view) = setup_test_window(cx);
        let state = app_view.read_with(cx, |view, _| view.state.clone());

        // 1. Create a custom composition with custom values via EditorState (used by New Comp Dialog and Project Manager)
        state.update(cx, |s, _| {
            let initial_comp_count = s.project.compositions.len();
            let new_comp_id = s.create_composition("Custom 1440p", 2560, 1440, 120.0, 15.0);
            assert_eq!(s.project.compositions.len(), initial_comp_count + 1);

            let comp = s.project.get_composition(&new_comp_id).unwrap();
            assert_eq!(comp.name, "Custom 1440p");
            assert_eq!(comp.width, 2560);
            assert_eq!(comp.height, 1440);
            assert_eq!(comp.frame_rate, 120.0);
            assert_eq!(comp.duration.seconds(), 15.0);
        });

        // 2. Update active composition settings with custom typed values (tested via update_project_settings)
        state.update(cx, |s, _| {
            s.update_project_settings("", 1280, 720, 24.0, 8.5);
            let act = s.active_composition().unwrap();
            assert_eq!(act.width, 1280);
            assert_eq!(act.height, 720);
            assert_eq!(act.frame_rate, 24.0);
            assert_eq!(act.duration.seconds(), 8.5);
        });

        root.update(cx, |_root, cx| {
            cx.notify();
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn test_high_fps_clock_sync_and_playhead_scrub(cx: &mut TestAppContext) {
        use crate::panels::TimelinePanel;
        use crate::state::EditorState;

        cx.update(gpui_kit::init);
        let state = cx.new(|_| EditorState::new());
        let timeline_panel = cx.new(|cx| TimelinePanel::new(state.clone(), cx));

        // 1. Initial 30 fps state
        state.update(cx, |s, _| {
            assert_eq!(s.active_composition().unwrap().frame_rate, 30.0);
            assert_eq!(s.clock.frame_rate().as_f64(), 30.0);
        });

        // 2. Change composition to 60 fps
        state.update(cx, |s, _| {
            s.update_project_settings("60fps Comp", 1920, 1080, 60.0, 10.0);
            assert_eq!(s.active_composition().unwrap().frame_rate, 60.0);
            // Clock must synchronize to 60 fps
            assert_eq!(s.clock.frame_rate().as_f64(), 60.0);
            assert_eq!(s.clock.comp_duration().frames(), 600);

            // Seek to 5.0 seconds (50% of 10s comp)
            s.seek(5.0);
            assert_eq!(s.clock.current_frame(), 300);
            assert!((s.clock.position_seconds() - 5.0).abs() < 1e-4);
        });

        // 3. Verify TimelinePanel visible span and playhead percentage
        timeline_panel.read_with(cx, |p, _| {
            let (start, span) = p.visible_time_span(10.0);
            assert_eq!(start, 0.0);
            assert_eq!(span, 10.0);
        });

        // 4. Change composition to 120 fps
        state.update(cx, |s, _| {
            s.update_project_settings("120fps Comp", 1920, 1080, 120.0, 5.0);
            assert_eq!(s.clock.frame_rate().as_f64(), 120.0);
            assert_eq!(s.clock.comp_duration().frames(), 600);

            s.seek(2.5); // 50% of 5s comp
            assert_eq!(s.clock.current_frame(), 300);
        });
    }

    #[gpui_kit::test]
    fn test_timeline_zoom_and_pan(cx: &mut TestAppContext) {
        use crate::panels::TimelinePanel;
        use crate::state::EditorState;

        cx.update(gpui_kit::init);
        let state = cx.new(|_| EditorState::new());
        let timeline_panel = cx.new(|cx| TimelinePanel::new(state.clone(), cx));

        // 1. Initial 1.0x zoom
        timeline_panel.read_with(cx, |p, _| {
            assert_eq!(p.timeline_zoom, 1.0);
            let (start, span) = p.visible_time_span(10.0);
            assert_eq!(start, 0.0);
            assert_eq!(span, 10.0);
        });

        // 2. Zoom in centered on playhead at 5.0s
        timeline_panel.update(cx, |p, _| {
            p.zoom_in(5.0, 10.0);
            assert!(p.timeline_zoom > 1.0);
            let (start, span) = p.visible_time_span(10.0);
            assert!(span < 10.0);
            // View should be centered around 5.0s
            let center = start + span * 0.5;
            assert!((center - 5.0).abs() < 0.5);
        });

        // 3. Pan left and right
        timeline_panel.update(cx, |p, _| {
            let (orig_start, _) = p.visible_time_span(10.0);
            p.pan_left(10.0);
            let (new_start, _) = p.visible_time_span(10.0);
            assert!(new_start <= orig_start);

            p.pan_right(10.0);
            let (right_start, _) = p.visible_time_span(10.0);
            assert!(right_start >= new_start);
        });

        // 4. Zoom reset
        timeline_panel.update(cx, |p, _| {
            p.zoom_reset();
            assert_eq!(p.timeline_zoom, 1.0);
            let (start, span) = p.visible_time_span(10.0);
            assert_eq!(start, 0.0);
            assert_eq!(span, 10.0);
        });
    }

    #[gpui_kit::test]
    async fn test_solid_layer_full_screen_and_fps_change_preserves_clip(cx: &mut gpui_kit::TestAppContext) {
        let state = cx.new(|_| EditorState::new());

        // 1. Verify solid layer with 0, 0 matches composition dimensions
        let solid_id = state.update(cx, |s, _| {
            s.add_solid_layer("Full Comp Solid", project::Color::BLUE, 0, 0).expect("add solid")
        });

        state.read_with(cx, |s, _| {
            let comp = s.active_composition().unwrap();
            let layer = comp.get_layer(&solid_id).unwrap();
            if let project::LayerSource::Solid { width, height, .. } = layer.source {
                assert_eq!(width, comp.width);
                assert_eq!(height, comp.height);
            } else {
                panic!("Expected Solid layer source");
            }
            // Anchor point centered
            assert_eq!(layer.transform.anchor_point.value.x, (comp.width / 2) as f32);
            assert_eq!(layer.transform.anchor_point.value.y, (comp.height / 2) as f32);
        });

        // 2. Change composition FPS from 30 to 60 fps
        state.update(cx, |s, _| {
            let (w, h, dur) = s.active_composition().map(|c| (c.width, c.height, c.duration_seconds())).unwrap();
            s.update_project_settings("", w, h, 60.0, dur);
        });

        state.read_with(cx, |s, _| {
            let comp = s.active_composition().unwrap();
            assert_eq!(comp.frame_rate, 60.0);
            assert_eq!(comp.duration.frames(), 300); // 5 seconds at 60 fps

            let layer = comp.get_layer(&solid_id).unwrap();
            assert_eq!(layer.out_point.frames(), 300); // spans full 300 frames
            assert!((layer.out_point.seconds() - 5.0).abs() < 1e-4);

            // Layer remains active past halfway (e.g. at frame 150 @ 60fps = 2.5s and frame 280)
            assert!(layer.is_active_at(&project::TimeCode::from_frames(150, 60.0)));
            assert!(layer.is_active_at(&project::TimeCode::from_frames(280, 60.0)));
        });

        // 3. Verify continuous spline graph evaluation without quantization ripples
        state.read_with(cx, |s, _| {
            // Sampling at non-integer frame times produces smooth continuous values
            let val1 = s.evaluate_graph_param("layer_accent", "transform.position.x", 0.1234);
            let val2 = s.evaluate_graph_param("layer_accent", "transform.position.x", 0.1250);
            assert!(val1.is_some());
            assert!(val2.is_some());
            assert_ne!(val1.unwrap(), val2.unwrap());
        });
    }
}
